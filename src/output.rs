//! Sending synthetic events (Windows only).
//!
//! Calling `SendInput` inside the hook callback can cause delays, so events are sent from a dedicated thread.

use std::sync::mpsc::{self, Sender};
use std::thread;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};

use crate::gesture::Output;
use crate::logger::Logger;

/// Marker put in `dwExtraInfo` of events we send. "NUBS" in ASCII.
/// The hook checks it so that our own events are not processed again.
pub const MAGIC: usize = 0x4E55_4253;

/// Starts the sender thread and returns the sending end of its queue.
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

/// Sends one synthetic event. Returns true on success.
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
                // The wheel delta is signed but is passed as a DWORD
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
