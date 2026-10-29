//! Low-level mouse hook (Windows only).
//!
//! The `WH_MOUSE_LL` callback passes middle-button and move events to [`Gesture`],
//! swallows the original event as decided, and queues synthetic events for sending.
//! The callback runs inside the message loop of the thread that installed the hook.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, HC_ACTION, LLMHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

use crate::config::Settings;
use crate::cursor::Indicator;
use crate::gesture::{Gesture, GestureConfig, Input, Output};
use crate::logger::Logger;
use crate::output::MAGIC;
use crate::process;

/// State used by the hook callback.
struct Context {
    gesture: Gesture,
    out: Sender<Output>,
    /// Scroll cursor requests (None if the scroll cursor is disabled)
    cursor: Option<Sender<Indicator>>,
    log: Logger,
    trace: bool,
    /// While paused, every event passes through untouched
    paused: bool,
    /// Devices whose middle button scrolls (empty: all), see `Settings::devices`
    devices: Vec<String>,
    /// Device of a raw-input middle press that arrived before the hook saw the press
    raw_down: Option<(String, Instant)>,
    /// The press is held but its raw input (and so its device) has not arrived yet
    awaiting_device: bool,
    /// The press came from a device that does not scroll: pass everything until the release
    bypass: bool,
}

/// How long a raw-input press waits for the matching hook event (they arrive within a few ms).
const RAW_MATCH: Duration = Duration::from_millis(200);

thread_local! {
    // The callback only runs on the thread that installed the hook, so thread_local is enough
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

/// Installs the hook and runs the message loop. Does not return until `WM_QUIT`.
pub fn run(
    gesture: Gesture,
    out: Sender<Output>,
    cursor: Option<Sender<Indicator>>,
    devices: Vec<String>,
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
            paused: false,
            devices,
            raw_down: None,
            awaiting_device: false,
            bypass: false,
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

/// Pauses or resumes processing. Must be called on the thread running [`run`] (the tray window's thread).
pub fn set_paused(paused: bool) {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.paused = paused;
            ctx.gesture.reset();
            if let Some(cursor) = &ctx.cursor {
                let _ = cursor.send(Indicator::Off);
            }
            ctx.log.log(if paused { "paused" } else { "resumed" });
        }
    });
}

/// Whether the middle button is held (gesture in progress). Must be called on the thread running [`run`].
pub fn is_active() -> bool {
    CONTEXT.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|ctx| ctx.gesture.is_active())
    })
}

/// Replaces the gesture settings. Must be called on the thread running [`run`].
pub fn set_config(config: GestureConfig) {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.gesture.set_config(config);
        }
    });
}

/// Replaces the list of devices that scroll. Must be called on the thread running [`run`].
pub fn set_devices(devices: Vec<String>) {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.devices = devices;
        }
    });
}

/// Reports a middle press seen by raw input from the device `id` (physical devices only).
/// Raw input and the hook do not arrive in a fixed order: if the hook already holds the press,
/// it is resolved now; otherwise the device is remembered for the hook event that follows.
/// Must be called on the thread running [`run`].
pub fn on_raw_middle_down(id: String) {
    CONTEXT.with(|c| {
        let mut c = c.borrow_mut();
        let Some(ctx) = c.as_mut() else {
            return;
        };
        if !ctx.awaiting_device {
            ctx.raw_down = Some((id, Instant::now()));
            return;
        }
        ctx.awaiting_device = false;
        if Settings::scrolls(&ctx.devices, Some(&id)) {
            return;
        }
        // Not a scrolling device: replay the held press as a plain middle-button drag
        ctx.log.log(format!(
            "device {id} does not scroll: passing its middle button through"
        ));
        let (before, indicator_before) = (ctx.gesture.state_name(), indicator(&ctx.gesture));
        let emit = ctx.gesture.bypass();
        after_change(ctx, before, indicator_before, emit);
    });
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
        if injected || ctx.paused {
            return false;
        }
        // A press from a device that does not scroll passes untouched, up to its release
        if ctx.bypass {
            if msg == WM_MBUTTONUP {
                ctx.bypass = false;
            }
            return false;
        }
        if msg == WM_MBUTTONDOWN && !ctx.gesture.is_active() && !ctx.devices.is_empty() {
            let raw = ctx
                .raw_down
                .take()
                .filter(|(_, at)| at.elapsed() < RAW_MATCH);
            match raw {
                Some((id, _)) if !Settings::scrolls(&ctx.devices, Some(&id)) => {
                    ctx.log.log(format!(
                        "device {id} does not scroll: passing its middle button through"
                    ));
                    ctx.bypass = true;
                    return false;
                }
                Some(_) => {}
                // Hold the press as usual and decide when its raw input arrives
                None => ctx.awaiting_device = true,
            }
        }
        if msg == WM_MBUTTONUP {
            ctx.awaiting_device = false;
        }
        let (x, y) = (info.pt.x, info.pt.y);
        let input = match msg {
            WM_MBUTTONDOWN => Input::MiddleDown { x, y },
            WM_MBUTTONUP => Input::MiddleUp,
            WM_MOUSEMOVE => Input::Move { x, y },
            _ => return false,
        };
        let (before, indicator_before) = (ctx.gesture.state_name(), indicator(&ctx.gesture));
        let decision = ctx.gesture.handle(input);
        after_change(ctx, before, indicator_before, decision.emit);
        decision.suppress
    })
}

/// Follows up a gesture step: logs a state change, switches the cursor when holding / scrolling
/// starts or ends (done on the cursor thread) and queues the events to send.
fn after_change(
    ctx: &Context,
    before: &'static str,
    indicator_before: Indicator,
    emit: Vec<Output>,
) {
    let (after, indicator_after) = (ctx.gesture.state_name(), indicator(&ctx.gesture));
    if before != after {
        ctx.log.log(format!("state: {before} -> {after}"));
    }
    if indicator_before != indicator_after
        && let Some(cursor) = &ctx.cursor
    {
        let _ = cursor.send(indicator_after);
    }
    for out in emit {
        let _ = ctx.out.send(out);
    }
}

/// What the cursor should show for the gesture state.
fn indicator(gesture: &Gesture) -> Indicator {
    match gesture.scroll_axes() {
        Some(axes) => Indicator::Scroll(axes),
        None if gesture.state_name() == "pending" => Indicator::Hold,
        None => Indicator::Off,
    }
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
    // Source: physical device / nubmiddle itself / other software or driver
    let source = match (injected, info.dwExtraInfo == MAGIC) {
        (false, _) => "device",
        (true, true) => "nubmiddle",
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
