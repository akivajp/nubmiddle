//! Notification area (tray) icon and its menu (Windows only).
//!
//! The icon belongs to a hidden window created on the hook thread, so its messages are
//! dispatched by the hook's message loop. Slow work (`schtasks.exe`, the UAC prompt) runs on
//! worker threads: blocking this thread would delay the hook and get it removed by Windows.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::thread;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    IDI_APPLICATION, LoadIconW, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE, WM_ENDSESSION, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

use crate::autostart::{self, Status};
use crate::cursor;
use crate::hook;
use crate::logger::Logger;
use crate::process;

/// Callback message of the tray icon.
const WM_TRAY: u32 = WM_APP + 1;
/// A worker thread reports the autostart status. `lparam` is a `Box<Status>` raw pointer.
const WM_AUTOSTART_STATUS: u32 = WM_APP + 2;

/// Menu item ids.
const ID_AUTOSTART: usize = 1;
const ID_PAUSE: usize = 2;
const ID_QUIT: usize = 3;

/// Id of our icon within the window.
const ICON_ID: u32 = 1;

/// The tray window, for use from other threads (console control handler).
static HWND_TRAY: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());

/// Tray state, owned by the hook thread.
struct Tray {
    log: Logger,
    /// Command-line arguments of this run, reused for the autostart task
    args: Vec<String>,
    /// Autostart state, None while being checked or changed
    autostart: Option<Status>,
    paused: bool,
    /// "TaskbarCreated": Explorer restarted and the icon must be added again
    msg_taskbar_created: u32,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Converts a string to a NUL-terminated UTF-16 buffer.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Copies `s` into a fixed-size UTF-16 field, truncating it and keeping the NUL terminator.
fn fill(dst: &mut [u16], s: &str) {
    let n = dst.len() - 1;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(n).chain(Some(0))) {
        *d = c;
    }
}

/// Creates the hidden window and the tray icon, then checks the autostart task in the background.
/// Must be called on the thread that will run the hook's message loop.
pub fn create(log: Logger, args: Vec<String>) -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("nubmiddle.tray");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        if RegisterClassW(&wc) == 0 {
            return Err(format!(
                "RegisterClassW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // A normal (never shown) top-level window: message-only windows miss the TaskbarCreated broadcast
        let title = wide("nubmiddle");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(format!(
                "CreateWindowExW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        HWND_TRAY.store(hwnd, Ordering::Release);
        let msg_taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        TRAY.with(|t| {
            *t.borrow_mut() = Some(Tray {
                log: log.clone(),
                args,
                autostart: None,
                paused: false,
                msg_taskbar_created,
            })
        });
        if !add_icon(hwnd, false) {
            return Err(format!(
                "Shell_NotifyIconW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        check_autostart(hwnd, log);
    }
    Ok(())
}

/// Removes the tray icon. Can be called from any thread.
pub fn remove_icon() {
    let hwnd = HWND_TRAY.load(Ordering::Acquire);
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let mut nid = icon_data(hwnd);
        nid.uFlags = 0;
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Asks the hook thread to quit (e.g. on Ctrl+C). Returns false if there is no tray window yet.
pub fn request_quit() -> bool {
    let hwnd = HWND_TRAY.load(Ordering::Acquire);
    !hwnd.is_null() && unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } != 0
}

/// Base `NOTIFYICONDATAW` identifying our icon.
fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ICON_ID;
    nid
}

/// Tooltip text for the current state.
fn tooltip(paused: bool) -> String {
    let mut tip = String::from("nubmiddle");
    if process::is_current_elevated() {
        tip += " (administrator)";
    }
    if paused {
        tip += " - paused";
    }
    tip
}

/// Adds the icon (or updates its tooltip with `modify`).
fn add_icon(hwnd: HWND, modify: bool) -> bool {
    let paused = TRAY.with(|t| t.borrow().as_ref().is_some_and(|t| t.paused));
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    fill(&mut nid.szTip, &tooltip(paused));
    let op = if modify { NIM_MODIFY } else { NIM_ADD };
    unsafe { Shell_NotifyIconW(op, &nid) != 0 }
}

/// Shows a balloon notification with a warning icon.
fn notify(hwnd: HWND, title: &str, text: &str) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_WARNING;
    fill(&mut nid.szInfoTitle, title);
    fill(&mut nid.szInfo, text);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

/// Sends a worker's autostart status back to the tray window.
fn post_status(hwnd: usize, status: Status) {
    let ptr = Box::into_raw(Box::new(status));
    if unsafe { PostMessageW(hwnd as HWND, WM_AUTOSTART_STATUS, 0, ptr as LPARAM) } == 0 {
        // Not delivered (window gone): free it here
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// Checks the autostart task in the background. If it points to another exe (moved or copied)
/// and we run elevated, re-registers it silently for this exe, keeping its arguments.
fn check_autostart(hwnd: HWND, log: Logger) {
    let hwnd = hwnd as usize;
    let _ = thread::Builder::new()
        .name("autostart-check".into())
        .spawn(move || {
            let mut status = autostart::status();
            if let Status::Stale(task) = &status {
                log.log(format!(
                    "autostart task points to another exe: {}",
                    task.command
                ));
                if process::is_current_elevated() {
                    match autostart::register(&autostart::current_user(), &task.arguments) {
                        Ok(()) => log.log("autostart task re-registered for this exe"),
                        Err(e) => log.log(format!("[warn] cannot re-register autostart: {e}")),
                    }
                    status = autostart::status();
                }
            }
            log.log(format!("autostart: {status:?}"));
            post_status(hwnd, status);
        });
}

/// Registers or deletes the autostart task in the background (with a UAC prompt unless elevated).
fn toggle_autostart(hwnd: HWND, log: Logger, register: bool, args: Vec<String>) {
    let hwnd = hwnd as usize;
    let _ = thread::Builder::new()
        .name("autostart-change".into())
        .spawn(move || {
            let user = autostart::current_user();
            let result = if process::is_current_elevated() {
                if register {
                    autostart::register(&user, &autostart::join_args(&args))
                } else {
                    autostart::unregister()
                }
            } else {
                // Run ourselves elevated in the internal registration mode
                let mut params = if register {
                    vec!["--register-autostart".to_string(), user]
                } else {
                    vec!["--unregister-autostart".to_string()]
                };
                if register {
                    params.extend(args);
                }
                match autostart::run_elevated(&autostart::join_args(&params)) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err("the elevated registration failed".into()),
                    Err(e) => Err(e),
                }
            };
            let action = if register { "register" } else { "delete" };
            match result {
                Ok(()) => log.log(format!("autostart task: {action}d")),
                Err(e) => log.log(format!("[warn] cannot {action} autostart task: {e}")),
            }
            post_status(hwnd, autostart::status());
        });
}

/// Shows the menu at the cursor and handles the chosen item.
fn show_menu(hwnd: HWND) {
    // Read the state, then release the borrow: TrackPopupMenu runs a nested message loop
    // that dispatches messages back into wnd_proc
    let Some((autostart, paused)) =
        TRAY.with(|t| t.borrow().as_ref().map(|t| (t.autostart.clone(), t.paused)))
    else {
        return;
    };
    unsafe {
        let menu = CreatePopupMenu();
        let title = wide(&format!("nubmiddle {}", env!("CARGO_PKG_VERSION")));
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, title.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let (label, flags) = match &autostart {
            None => ("Start with Windows (checking...)", MF_GRAYED),
            Some(Status::NotRegistered) => ("Start with Windows (as administrator)", 0),
            Some(Status::Registered) => ("Start with Windows (as administrator)", MF_CHECKED),
            Some(Status::Stale(_)) => ("Start with Windows: update to this exe", 0),
        };
        let label = wide(label);
        AppendMenuW(menu, MF_STRING | flags, ID_AUTOSTART, label.as_ptr());
        let pause = wide("Pause");
        let pause_flags = if paused { MF_CHECKED } else { 0 };
        AppendMenuW(menu, MF_STRING | pause_flags, ID_PAUSE, pause.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let quit = wide("Quit");
        AppendMenuW(menu, MF_STRING, ID_QUIT, quit.as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Without this the menu does not close when clicking elsewhere
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match cmd as usize {
            ID_AUTOSTART => {
                let job = TRAY.with(|t| {
                    let mut t = t.borrow_mut();
                    let t = t.as_mut()?;
                    // Registered -> delete; not registered or stale -> register for this exe
                    let register = !matches!(t.autostart, Some(Status::Registered));
                    t.autostart = None;
                    Some((t.log.clone(), register, t.args.clone()))
                });
                if let Some((log, register, args)) = job {
                    toggle_autostart(hwnd, log, register, args);
                }
            }
            ID_PAUSE => {
                TRAY.with(|t| {
                    if let Some(t) = t.borrow_mut().as_mut() {
                        t.paused = !paused;
                    }
                });
                hook::set_paused(!paused);
                add_icon(hwnd, true);
            }
            ID_QUIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

/// Window procedure of the hidden tray window.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            // Legacy (version 0) callback: the mouse message is in lparam
            if matches!(lparam as u32, WM_RBUTTONUP | WM_LBUTTONUP) {
                show_menu(hwnd);
            }
            0
        }
        WM_AUTOSTART_STATUS => {
            let status = *unsafe { Box::from_raw(lparam as *mut Status) };
            if let Status::Stale(task) = &status {
                notify(
                    hwnd,
                    "nubmiddle: autostart needs an update",
                    &format!(
                        "Autostart still starts {}. Choose \"Start with Windows: update to this exe\" in the tray menu.",
                        task.command
                    ),
                );
            }
            TRAY.with(|t| {
                if let Some(t) = t.borrow_mut().as_mut() {
                    t.autostart = Some(status);
                }
            });
            0
        }
        WM_CLOSE => {
            // Ctrl+C in the console, or another polite close request
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_ENDSESSION if wparam != 0 => {
            // Logoff / shutdown: the process may be killed without returning from the message loop
            cursor::restore();
            remove_icon();
            0
        }
        _ => {
            let taskbar_created = TRAY.with(|t| t.borrow().as_ref().map(|t| t.msg_taskbar_created));
            if taskbar_created == Some(msg) {
                // Explorer restarted: the icon is gone and must be added again
                add_icon(hwnd, false);
                return 0;
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}
