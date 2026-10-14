//! 合成イベントの送出 (Windows 専用)。
//!
//! `SendInput` はフックのコールバック内で呼ぶと遅延の原因になりうるため、専用スレッドで送る。

use std::sync::mpsc::{self, Sender};
use std::thread;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};

use crate::gesture::Output;
use crate::logger::Logger;

/// 自分が送出したイベントの目印 (`dwExtraInfo` に入れる)。ASCII で "NUBS"。
/// フック側でこの値を見て、自分の送ったイベントを再処理しないようにする。
pub const MAGIC: usize = 0x4E55_4253;

/// 送出スレッドを起動し、送出キューの送り口を返す。
pub fn start(log: Logger) -> std::io::Result<Sender<Output>> {
    let (tx, rx) = mpsc::channel::<Output>();
    thread::Builder::new()
        .name("output".into())
        .spawn(move || {
            for out in rx {
                if !send(out) {
                    log.log(format!(
                        "[warn] SendInput failed: {out:?} (error={})",
                        std::io::Error::last_os_error()
                    ));
                }
            }
        })?;
    Ok(tx)
}

/// 合成イベントを 1 つ送出する。成功したら true。
fn send(out: Output) -> bool {
    let (flags, data) = match out {
        Output::MiddleDown => (MOUSEEVENTF_MIDDLEDOWN, 0),
        Output::MiddleUp => (MOUSEEVENTF_MIDDLEUP, 0),
        Output::Wheel(delta) => (MOUSEEVENTF_WHEEL, delta),
        Output::HWheel(delta) => (MOUSEEVENTF_HWHEEL, delta),
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                // ホイール量は符号付きだが、型は DWORD として渡す
                mouseData: data as _,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MAGIC,
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    sent == 1
}
