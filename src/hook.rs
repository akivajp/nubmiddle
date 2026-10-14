//! Low-level mouse hook (Windows only).
//!
//! The `WH_MOUSE_LL` callback passes middle-button and move events to [`Gesture`],
//! swallows the original event as decided, and queues synthetic events for sending.
//! The callback runs inside the message loop of the thread that installed the hook.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, HC_ACTION, LLMHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

use crate::gesture::{Axes, Gesture, Input, Output};
use crate::logger::Logger;
use crate::output::MAGIC;
use crate::process;

/// State used by the hook callback.
struct Context {
    gesture: Gesture,
    out: Sender<Output>,
    /// Scroll cursor requests (None if the scroll cursor is disabled)
    cursor: Option<Sender<Option<Axes>>>,
    log: Logger,
    trace: bool,
}

thread_local! {
    // The callback only runs on the thread that installed the hook, so thread_local is enough
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

/// Installs the hook and runs the message loop. Does not return until `WM_QUIT`.
pub fn run(
    gesture: Gesture,
    out: Sender<Output>,
    cursor: Option<Sender<Option<Axes>>>,
    log: Logger,
    trace: bool,
) -> Result<(), String> {
    CONTEXT.with(|c| {
        *c.borrow_mut() = Some(Context {
            gesture,
            out,
            cursor,
            log,
            trace,
        })
    });
    unsafe {
        let module = GetModuleHandleW(null());
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), module, 0);
        if hook.is_null() {
            return Err(format!(
                "SetWindowsHookExW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // Low-level hooks are only called while a message loop is running
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
    }
    Ok(())
}

/// Callback for `WH_MOUSE_LL`.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if handle(wparam as u32, info) {
            // Returning non-zero swallows the event
            return 1;
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// Processes one mouse event. Returns true to swallow it.
/// This is subject to the LowLevelHooksTimeout limit, so do nothing heavy here.
fn handle(msg: u32, info: &MSLLHOOKSTRUCT) -> bool {
    CONTEXT.with(|c| {
        let mut c = c.borrow_mut();
        let Some(ctx) = c.as_mut() else {
            return false;
        };
        let injected = info.flags & LLMHF_INJECTED != 0;
        if ctx.trace {
            trace(ctx, msg, info, injected);
        }
        // Let injected events through (our own and those from other software or drivers)
        if injected {
            return false;
        }
        let (x, y) = (info.pt.x, info.pt.y);
        let input = match msg {
            WM_MBUTTONDOWN => Input::MiddleDown { x, y },
            WM_MBUTTONUP => Input::MiddleUp,
            WM_MOUSEMOVE => Input::Move { x, y },
            _ => return false,
        };
        let (before, axes_before) = (ctx.gesture.state_name(), ctx.gesture.scroll_axes());
        let decision = ctx.gesture.handle(input);
        let (after, axes_after) = (ctx.gesture.state_name(), ctx.gesture.scroll_axes());
        if before != after {
            ctx.log.log(format!("state: {before} -> {after}"));
        }
        // Switch the cursor when scrolling starts or ends (done on the cursor thread)
        if axes_before != axes_after
            && let Some(cursor) = &ctx.cursor
        {
            let _ = cursor.send(axes_after);
        }
        for out in decision.emit {
            let _ = ctx.out.send(out);
        }
        decision.suppress
    })
}

/// Detailed log for troubleshooting. Middle button and wheel events are always logged; moves only while the middle button is held.
fn trace(ctx: &Context, msg: u32, info: &MSLLHOOKSTRUCT, injected: bool) {
    let name = match msg {
        WM_MBUTTONDOWN => "WM_MBUTTONDOWN",
        WM_MBUTTONUP => "WM_MBUTTONUP",
        WM_MOUSEWHEEL => "WM_MOUSEWHEEL",
        WM_MOUSEHWHEEL => "WM_MOUSEHWHEEL",
        WM_MOUSEMOVE if ctx.gesture.is_active() => "WM_MOUSEMOVE",
        _ => return,
    };
    // Source: physical device / nubscroll itself / other software or driver
    let source = match (injected, info.dwExtraInfo == MAGIC) {
        (false, _) => "device",
        (true, true) => "nubscroll",
        (true, false) => "injected",
    };
    let mut line = format!(
        "[trace] {name:<15} pt=({},{}) source={source}",
        info.pt.x, info.pt.y
    );
    if matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
        // The wheel delta is the (signed) high word of mouseData
        line += &format!(" delta={}", (info.mouseData >> 16) as u16 as i16);
    }
    if msg == WM_MOUSEMOVE {
        // The hook's pt is where the cursor is about to go. Comparing it with the actual
        // cursor position shows whether swallowing (pinning the cursor) works
        let mut cur = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut cur) };
        line += &format!(" cursor=({},{})", cur.x, cur.y);
    }
    if msg == WM_MBUTTONDOWN && !injected {
        // Check whether the window under the press is elevated (i.e. affected by UIPI)
        line += &format!(
            " target={}",
            process::describe_window_at(info.pt.x, info.pt.y)
        );
    }
    if injected && info.dwExtraInfo != MAGIC {
        line += &format!(" extra={:#x}", info.dwExtraInfo);
    }
    ctx.log.log(line);
}
