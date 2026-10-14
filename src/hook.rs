//! 低レベルマウスフック (Windows 専用)。
//!
//! `WH_MOUSE_LL` のコールバックで中ボタン・移動のイベントを [`Gesture`] に渡し、
//! 判定結果に従って元のイベントを握りつぶし、合成イベントを送出キューに積む。
//! コールバックはフックを設置したスレッドのメッセージループ内で呼ばれる。

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, LLMHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

use crate::gesture::{Gesture, Input, Output};
use crate::logger::Logger;
use crate::output::MAGIC;

/// フックのコールバックから参照する状態。
struct Context {
    gesture: Gesture,
    out: Sender<Output>,
    log: Logger,
    trace: bool,
}

thread_local! {
    // コールバックはフックを設置したスレッドでしか呼ばれないので thread_local で足りる
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

/// フックを設置し、メッセージループを回す。`WM_QUIT` を受け取るまで戻らない。
pub fn run(gesture: Gesture, out: Sender<Output>, log: Logger, trace: bool) -> Result<(), String> {
    CONTEXT.with(|c| {
        *c.borrow_mut() = Some(Context {
            gesture,
            out,
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
        // 低レベルフックはメッセージループを回していないと呼ばれない
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
    }
    Ok(())
}

/// `WH_MOUSE_LL` のコールバック。
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if handle(wparam as u32, info) {
            // 0 以外を返すとイベントが握りつぶされる
            return 1;
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// 1 つのマウスイベントを処理する。握りつぶすなら true を返す。
/// この関数は処理時間の制限 (LowLevelHooksTimeout) を受けるので、重い処理はしないこと。
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
        // 合成されたイベント (自分の送出分・他のソフトやドライバの送出分) は素通しする
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
        let before = ctx.gesture.state_name();
        let decision = ctx.gesture.handle(input);
        let after = ctx.gesture.state_name();
        if before != after {
            ctx.log.log(format!("state: {before} -> {after}"));
        }
        for out in decision.emit {
            let _ = ctx.out.send(out);
        }
        decision.suppress
    })
}

/// 動作検証用の詳細ログ。中ボタン・ホイールは常に、移動は中ボタン押下中のみ出力する。
fn trace(ctx: &Context, msg: u32, info: &MSLLHOOKSTRUCT, injected: bool) {
    let name = match msg {
        WM_MBUTTONDOWN => "WM_MBUTTONDOWN",
        WM_MBUTTONUP => "WM_MBUTTONUP",
        WM_MOUSEWHEEL => "WM_MOUSEWHEEL",
        WM_MOUSEHWHEEL => "WM_MOUSEHWHEEL",
        WM_MOUSEMOVE if ctx.gesture.is_active() => "WM_MOUSEMOVE",
        _ => return,
    };
    // 送出元: 物理デバイス / nubscroll 自身 / 他のソフト・ドライバ
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
        // ホイール量は mouseData の上位ワード (符号付き)
        line += &format!(" delta={}", (info.mouseData >> 16) as u16 as i16);
    }
    if injected && info.dwExtraInfo != MAGIC {
        line += &format!(" extra={:#x}", info.dwExtraInfo);
    }
    ctx.log.log(line);
}
