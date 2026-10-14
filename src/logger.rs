//! 日時付きのログ出力 (Windows 専用)。
//!
//! フックのコールバックは処理が遅いと Windows に黙って外されるため、
//! コンソールやファイルへの書き込みは専用スレッドで行い、呼び出し側はキューに積むだけにする。

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// ログの送り口。複製して複数スレッドから使える。
#[derive(Clone)]
pub struct Logger {
    tx: Sender<(SYSTEMTIME, String)>,
}

impl Logger {
    /// ログ出力スレッドを起動する。`file` を指定するとコンソールに加えてファイルにも追記する。
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
                        // ログが書けなくても本体の動作は止めない
                        let _ = writeln!(f, "{line}").and_then(|_| f.flush());
                    }
                }
            })?;
        Ok(Self { tx })
    }

    /// 現在時刻を付けてログを 1 行キューに積む。
    pub fn log(&self, msg: impl Into<String>) {
        // 時刻は出力時ではなく呼び出し時点のものを使う
        let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut now) };
        let _ = self.tx.send((now, msg.into()));
    }
}
