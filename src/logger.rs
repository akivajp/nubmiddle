//! Timestamped logging (Windows only).
//!
//! Windows silently removes a hook whose callback is too slow, so writing to the
//! console or file happens on a dedicated thread; callers only enqueue.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// Logging handle. Can be cloned and used from multiple threads.
#[derive(Clone)]
pub struct Logger {
    tx: Sender<(SYSTEMTIME, String)>,
}

impl Logger {
    /// Starts the logging thread. With `file`, the log is also appended to that file.
    pub fn start(file: Option<&Path>) -> io::Result<Self> {
        let mut file = match file {
            Some(path) => Some(OpenOptions::new().create(true).append(true).open(path)?),
            None => None,
        };
        let (tx, rx) = mpsc::channel::<(SYSTEMTIME, String)>();
        thread::Builder::new()
            .name("logger".into())
            .spawn(move || {
                for (t, msg) in rx {
                    let line = format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {msg}",
                        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
                    );
                    println!("{line}");
                    if let Some(f) = file.as_mut() {
                        // Failing to write the log must not stop the tool
                        let _ = writeln!(f, "{line}").and_then(|_| f.flush());
                    }
                }
            })?;
        Ok(Self { tx })
    }

    /// Enqueues one log line with the current time.
    pub fn log(&self, msg: impl Into<String>) {
        // Use the time of the call, not the time of output
        let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut now) };
        let _ = self.tx.send((now, msg.into()));
    }
}
