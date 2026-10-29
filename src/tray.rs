//! Notification area (tray) icon and its menu (Windows only).
//!
//! The icon belongs to a hidden window created on the hook thread, so its messages are
//! dispatched by the hook's message loop. Slow work (`schtasks.exe`, the UAC prompt) runs on
//! worker threads: blocking this thread would delay the hook and get it removed by Windows.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::thread;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_INPUT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    HMENU, IDI_APPLICATION, LoadIconW, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE, WM_ENDSESSION,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, HICON, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXSMICON, SM_CYSMICON,
};
use windows_sys::core::PCWSTR;

use crate::autostart::{self, Status};
use crate::config::{self, SPEED_FACTORS, Settings};
use crate::cursor;
use crate::gesture::MoveMode;
use crate::hook;
use crate::i18n::{Lang, LangSetting, Text, tr, trf};
use crate::logger::Logger;
use crate::process;
use crate::rawinput;
use crate::settings;

/// Callback message of the tray icon.
const WM_TRAY: u32 = WM_APP + 1;
/// A worker thread reports the autostart status. `lparam` is a `Box<Status>` raw pointer.
const WM_AUTOSTART_STATUS: u32 = WM_APP + 2;

/// Menu item ids.
const ID_AUTOSTART: usize = 1;
const ID_PAUSE: usize = 2;
const ID_QUIT: usize = 3;
const ID_MODE_SCROLL: usize = 10;
const ID_MODE_DRAG: usize = 11;
/// First of the vertical speed levels (one id per entry of SPEED_LEVELS)
const ID_SPEED_V: usize = 20;
/// First of the horizontal speed levels
const ID_SPEED_H: usize = 30;
const ID_RESET: usize = 40;
const ID_OPEN_SETTINGS: usize = 41;
const ID_RELOAD_SETTINGS: usize = 42;
const ID_LANG_AUTO: usize = 50;
const ID_LANG_EN: usize = 51;
const ID_LANG_JA: usize = 52;
const ID_DEV_ALL: usize = 60;
/// First of the device items (one id per entry of `Tray::menu_devices`)
const ID_DEV_FIRST: usize = 61;
const MAX_DEVICES: usize = 30;

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
    /// Settings file and its contents
    settings_path: PathBuf,
    saved: Settings,
    /// Command-line settings, applied over the file until the same setting is changed in the menu
    overrides: Vec<(String, String)>,
    /// Device ids in the order of the device items of the last menu shown
    menu_devices: Vec<String>,
    /// "TaskbarCreated": Explorer restarted and the icon must be added again
    msg_taskbar_created: u32,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    /// Whether raw input events are logged (--trace)
    static TRACE_RAW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// Settings handed to the tray, which changes and saves them from the menu.
pub struct SettingsState {
    /// Settings file
    pub path: PathBuf,
    /// Settings loaded from the file (over the defaults)
    pub saved: Settings,
    /// Command-line settings applied over them
    pub overrides: Vec<(String, String)>,
}

/// Creates the hidden window and the tray icon, then checks the autostart task in the background.
/// Must be called on the thread that will run the hook's message loop.
pub fn create(
    log: Logger,
    args: Vec<String>,
    settings: SettingsState,
    trace: bool,
) -> Result<(), String> {
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
                settings_path: settings.path,
                saved: settings.saved,
                overrides: settings.overrides,
                menu_devices: Vec::new(),
                msg_taskbar_created,
            })
        });
        if !add_icon(hwnd, false) {
            return Err(format!(
                "Shell_NotifyIconW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // Raw input tells which device a middle press comes from (for "Devices that scroll")
        match rawinput::register(hwnd) {
            Ok(()) => {
                for device in rawinput::list_mice() {
                    log.log(format!("mouse device: {}", rawinput::describe(device)));
                }
                TRACE_RAW.with(|t| t.set(trace));
            }
            Err(e) => log.log(format!("[warn] {e} (all devices will scroll)")),
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

/// Language of the menu and notifications, from the setting and the Windows display language.
fn current_lang() -> Lang {
    let setting = TRAY.with(|t| t.borrow().as_ref().map(|t| t.saved.language));
    let ui = unsafe { GetUserDefaultUILanguage() };
    setting.unwrap_or_default().resolve((ui != 0).then_some(ui))
}

/// Tooltip text for the current state.
fn tooltip(lang: Lang, paused: bool) -> String {
    let mut tip = String::from("nubmiddle");
    if process::is_current_elevated() {
        tip += tr(lang, Text::TipAdministrator);
    }
    if paused {
        tip += tr(lang, Text::TipPaused);
    }
    tip
}

/// Resource ids of the icons embedded by build.rs (assets/nubmiddle.rc).
const ICON_NORMAL: u16 = 1;
const ICON_PAUSED: u16 = 2;

/// Loads an embedded icon at the small-icon size, falling back to the generic application icon.
/// Shared icons are cached by the system, so loading again on every update is cheap.
fn load_icon(id: u16) -> HICON {
    unsafe {
        let icon = LoadImageW(
            GetModuleHandleW(null()),
            // MAKEINTRESOURCE: the id travels in the pointer value
            id as usize as PCWSTR,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        );
        if icon.is_null() {
            LoadIconW(null_mut(), IDI_APPLICATION)
        } else {
            icon
        }
    }
}

/// Adds the icon (or updates its tooltip and image with `modify`).
fn add_icon(hwnd: HWND, modify: bool) -> bool {
    let paused = TRAY.with(|t| t.borrow().as_ref().is_some_and(|t| t.paused));
    let lang = current_lang();
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = load_icon(if paused { ICON_PAUSED } else { ICON_NORMAL });
    fill(&mut nid.szTip, &tooltip(lang, paused));
    let op = if modify { NIM_MODIFY } else { NIM_ADD };
    unsafe { Shell_NotifyIconW(op, &nid) != 0 }
}

/// Shows a balloon notification (`kind`: NIIF_INFO or NIIF_WARNING).
fn notify(hwnd: HWND, kind: NOTIFY_ICON_INFOTIP_FLAGS, title: &str, text: &str) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = kind;
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

/// Applies the effective settings (file + remaining overrides) to the hook.
fn apply_settings(t: &Tray) {
    let effective = t.saved.with_overrides(&t.overrides);
    hook::set_config(effective.gesture.clone());
    hook::set_devices(effective.devices.clone());
    t.log.log(format!("settings: {}", effective.summary()));
}

/// Saves the settings file on a worker thread (the file may sit in a slow, synced folder).
fn save_settings(t: &Tray) {
    let (path, saved, log) = (t.settings_path.clone(), t.saved.clone(), t.log.clone());
    let _ =
        thread::Builder::new()
            .name("settings-save".into())
            .spawn(move || match settings::save(&path, &saved) {
                Ok(()) => log.log(format!("settings saved: {}", path.display())),
                Err(e) => log.log(format!(
                    "[warn] cannot save settings to {}: {e}",
                    path.display()
                )),
            });
}

/// Changes one setting from the menu: it takes effect now, is saved, and replaces
/// a command-line value for the same setting.
fn change_setting(key: &str, value: &str) {
    TRAY.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            if let Err(e) = t.saved.set(key, value) {
                t.log.log(format!("[warn] cannot change {key}: {e}"));
                return;
            }
            t.overrides.retain(|(k, _)| k != key);
            apply_settings(t);
            save_settings(t);
        }
    });
}

/// Handles the settings items of the menu.
fn settings_command(hwnd: HWND, cmd: usize) {
    let levels = SPEED_FACTORS.len();
    match cmd {
        ID_DEV_ALL => change_setting("devices", "all"),
        c if (ID_DEV_FIRST..ID_DEV_FIRST + MAX_DEVICES).contains(&c) => {
            // Toggle the device in the list; an empty list means all devices
            let value = TRAY.with(|t| {
                let t = t.borrow();
                let t = t.as_ref()?;
                let id = t.menu_devices.get(c - ID_DEV_FIRST)?;
                let mut devices = t.saved.with_overrides(&t.overrides).devices;
                match devices.iter().position(|d| d.eq_ignore_ascii_case(id)) {
                    Some(i) => {
                        devices.remove(i);
                    }
                    None => devices.push(id.clone()),
                }
                Some(if devices.is_empty() {
                    "all".to_string()
                } else {
                    devices.join(", ")
                })
            });
            if let Some(value) = value {
                change_setting("devices", &value);
            }
        }
        ID_LANG_AUTO | ID_LANG_EN | ID_LANG_JA => {
            let value = match cmd {
                ID_LANG_EN => "en",
                ID_LANG_JA => "ja",
                _ => "auto",
            };
            change_setting("language", value);
            // The tooltip is translated too
            add_icon(hwnd, true);
        }
        ID_MODE_SCROLL => change_setting("mode", "scroll"),
        ID_MODE_DRAG => change_setting("mode", "drag"),
        c if (ID_SPEED_V..ID_SPEED_V + levels).contains(&c) => change_setting(
            "sensitivity_v",
            &config::speed_sensitivity(c - ID_SPEED_V).to_string(),
        ),
        c if (ID_SPEED_H..ID_SPEED_H + levels).contains(&c) => change_setting(
            "sensitivity_h",
            &config::speed_sensitivity(c - ID_SPEED_H).to_string(),
        ),
        ID_RESET => TRAY.with(|t| {
            if let Some(t) = t.borrow_mut().as_mut() {
                t.saved = Settings::default();
                t.overrides.clear();
                apply_settings(t);
                save_settings(t);
            }
        }),
        ID_OPEN_SETTINGS => {
            let Some((path, saved, log)) = TRAY.with(|t| {
                t.borrow()
                    .as_ref()
                    .map(|t| (t.settings_path.clone(), t.saved.clone(), t.log.clone()))
            }) else {
                return;
            };
            let _ = thread::Builder::new()
                .name("settings-open".into())
                .spawn(move || {
                    // Create the file first so that there is something to edit
                    if !path.exists()
                        && let Err(e) = settings::save(&path, &saved)
                    {
                        log.log(format!("[warn] cannot create {}: {e}", path.display()));
                        return;
                    }
                    if let Err(e) = std::process::Command::new("notepad.exe").arg(&path).spawn() {
                        log.log(format!("[warn] cannot open {}: {e}", path.display()));
                    }
                });
        }
        ID_RELOAD_SETTINGS => {
            let warnings = TRAY.with(|t| {
                let mut t = t.borrow_mut();
                let t = t.as_mut()?;
                let (saved, warnings) = settings::load(&t.settings_path);
                for w in &warnings {
                    t.log
                        .log(format!("[warn] {}: {w}", t.settings_path.display()));
                }
                t.log
                    .log("settings file reloaded (cursor settings take effect after a restart)");
                t.saved = saved;
                apply_settings(t);
                Some(warnings)
            });
            let lang = current_lang();
            match warnings {
                Some(w) if !w.is_empty() => notify(
                    hwnd,
                    NIIF_WARNING,
                    tr(lang, Text::SettingsErrorsTitle),
                    &(trf(lang, Text::SettingsErrorsBody, &w.len().to_string()) + &w.join("; ")),
                ),
                Some(_) => notify(
                    hwnd,
                    NIIF_INFO,
                    "nubmiddle",
                    tr(lang, Text::SettingsReloaded),
                ),
                None => {}
            }
            add_icon(hwnd, true);
        }
        _ => {}
    }
}

/// Appends a string item to `menu`, checked and/or grayed as given.
unsafe fn append(menu: HMENU, id: usize, label: &str, checked: bool, grayed: bool) {
    let label = wide(label);
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if grayed {
        flags |= MF_GRAYED;
    }
    unsafe { AppendMenuW(menu, flags, id, label.as_ptr()) };
}

/// Appends a submenu choosing one of the speed levels for `sensitivity`.
unsafe fn append_speed_menu(
    menu: HMENU,
    lang: Lang,
    label: &str,
    first_id: usize,
    sensitivity: f64,
) {
    unsafe {
        let sub = CreatePopupMenu();
        let current = config::speed_level(sensitivity);
        for i in 0..SPEED_FACTORS.len() {
            append(
                sub,
                first_id + i,
                tr(lang, Text::Speed(i)),
                current == Some(i),
                false,
            );
        }
        // A value from the settings file or the command line that is not one of the levels
        if current.is_none() {
            let custom = trf(lang, Text::CustomSpeed, &sensitivity.to_string());
            append(sub, 0, &custom, true, true);
        }
        let label = wide(label);
        AppendMenuW(menu, MF_POPUP, sub as usize, label.as_ptr());
    }
}

/// Shows the menu at the cursor and handles the chosen item.
fn show_menu(hwnd: HWND) {
    // Read the state, then release the borrow: TrackPopupMenu runs a nested message loop
    // that dispatches messages back into wnd_proc
    let Some((autostart, paused, effective)) = TRAY.with(|t| {
        t.borrow().as_ref().map(|t| {
            (
                t.autostart.clone(),
                t.paused,
                t.saved.with_overrides(&t.overrides),
            )
        })
    }) else {
        return;
    };
    let lang = current_lang();
    let t = |text| tr(lang, text);
    unsafe {
        let menu = CreatePopupMenu();
        let title = wide(&format!("nubmiddle {}", env!("CARGO_PKG_VERSION")));
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, title.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());

        // Settings
        let g = &effective.gesture;
        let mode = CreatePopupMenu();
        append(
            mode,
            ID_MODE_SCROLL,
            t(Text::ModeScroll),
            g.mode == MoveMode::Scroll,
            false,
        );
        append(
            mode,
            ID_MODE_DRAG,
            t(Text::ModeDrag),
            g.mode == MoveMode::Drag,
            false,
        );
        let mode_label = wide(t(Text::MoveMenu));
        AppendMenuW(menu, MF_POPUP, mode as usize, mode_label.as_ptr());
        append_speed_menu(
            menu,
            lang,
            t(Text::VerticalSpeed),
            ID_SPEED_V,
            g.sensitivity_v,
        );
        append_speed_menu(
            menu,
            lang,
            t(Text::HorizontalSpeed),
            ID_SPEED_H,
            g.sensitivity_h,
        );
        // Devices that scroll: connected devices, plus listed ones that are not connected
        let devices = CreatePopupMenu();
        append(
            devices,
            ID_DEV_ALL,
            t(Text::DevicesAll),
            effective.devices.is_empty(),
            false,
        );
        AppendMenuW(devices, MF_SEPARATOR, 0, null());
        let mut items: Vec<(String, String)> = rawinput::connected()
            .into_iter()
            .map(|(id, product)| {
                let label = match product {
                    Some(product) => format!("{product} ({id})"),
                    None => id.clone(),
                };
                (id, label)
            })
            .collect();
        for id in &effective.devices {
            if !items
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(id))
            {
                items.push((id.clone(), format!("{id}{}", t(Text::NotConnected))));
            }
        }
        items.truncate(MAX_DEVICES);
        for (i, (id, label)) in items.iter().enumerate() {
            let listed = effective.devices.iter().any(|d| d.eq_ignore_ascii_case(id));
            append(devices, ID_DEV_FIRST + i, label, listed, false);
        }
        TRAY.with(|tr| {
            if let Some(tr) = tr.borrow_mut().as_mut() {
                tr.menu_devices = items.into_iter().map(|(id, _)| id).collect();
            }
        });
        let devices_label = wide(t(Text::DevicesMenu));
        AppendMenuW(menu, MF_POPUP, devices as usize, devices_label.as_ptr());
        append(menu, ID_RESET, t(Text::ResetDefaults), false, false);
        append(menu, ID_OPEN_SETTINGS, t(Text::OpenSettings), false, false);
        append(
            menu,
            ID_RELOAD_SETTINGS,
            t(Text::ReloadSettings),
            false,
            false,
        );
        // Language
        let language = CreatePopupMenu();
        let setting = effective.language;
        append(
            language,
            ID_LANG_AUTO,
            t(Text::LanguageAuto),
            setting == LangSetting::Auto,
            false,
        );
        append(
            language,
            ID_LANG_EN,
            "English",
            setting == LangSetting::Fixed(Lang::En),
            false,
        );
        append(
            language,
            ID_LANG_JA,
            "日本語",
            setting == LangSetting::Fixed(Lang::Ja),
            false,
        );
        let language_label = wide(t(Text::LanguageMenu));
        AppendMenuW(menu, MF_POPUP, language as usize, language_label.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());

        let (label, checked, grayed) = match &autostart {
            None => (Text::AutostartChecking, false, true),
            Some(Status::NotRegistered) => (Text::Autostart, false, false),
            Some(Status::Registered) => (Text::Autostart, true, false),
            Some(Status::Stale(_)) => (Text::AutostartUpdate, false, false),
        };
        append(menu, ID_AUTOSTART, t(label), checked, grayed);
        append(menu, ID_PAUSE, t(Text::Pause), paused, false);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        append(menu, ID_QUIT, t(Text::Quit), false, false);

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
            0 => {}
            other => settings_command(hwnd, other),
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
                let lang = current_lang();
                notify(
                    hwnd,
                    NIIF_WARNING,
                    tr(lang, Text::AutostartStaleTitle),
                    &trf(lang, Text::AutostartStaleBody, &task.command),
                );
            }
            TRAY.with(|t| {
                if let Some(t) = t.borrow_mut().as_mut() {
                    t.autostart = Some(status);
                }
            });
            0
        }
        WM_INPUT => {
            if let Some(ev) = rawinput::read(lparam) {
                // Tell the hook which device a middle press comes from (injected input has none)
                if ev.middle_down()
                    && let Some(id) = rawinput::device_id(ev.device)
                {
                    hook::on_raw_middle_down(id);
                }
                // Trace: button transitions always; moves only while the middle button is held
                if TRACE_RAW.with(|t| t.get()) && (ev.buttons != 0 || hook::is_active()) {
                    let log = TRAY.with(|t| t.borrow().as_ref().map(|t| t.log.clone()));
                    if let Some(log) = log {
                        log.log(format!(
                            "[raw] buttons={:#06x}{}{} d=({},{}) device={}",
                            ev.buttons,
                            if ev.middle_down() { " MIDDLE_DOWN" } else { "" },
                            if ev.middle_up() { " MIDDLE_UP" } else { "" },
                            ev.dx,
                            ev.dy,
                            rawinput::describe(ev.device)
                        ));
                    }
                }
            }
            // DefWindowProc releases the raw input buffer
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
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
