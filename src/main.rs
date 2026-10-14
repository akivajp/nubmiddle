//! nubscroll: トラックポイントの中ボタンで中クリックとスクロールを両立させる Windows 常駐ツール。
//!
//! 現在は動作検証用の試作 (コンソール常駐版)。トレイアイコンと自動起動は今後追加する。

mod config;
mod gesture;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod logger;
#[cfg(windows)]
mod output;

use std::process::ExitCode;

use config::Command;

fn main() -> ExitCode {
    match config::parse_args(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            print!("{}", config::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Command::Run(opts)) => run(opts),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

/// 設定を表示し、フックを設置して常駐する。
#[cfg(windows)]
fn run(opts: config::Options) -> ExitCode {
    let log = match logger::Logger::start(opts.log_file.as_deref()) {
        Ok(log) => log,
        Err(e) => {
            eprintln!("error: cannot open log file: {e}");
            return ExitCode::FAILURE;
        }
    };
    let g = &opts.gesture;
    log.log(format!("nubscroll {} started", env!("CARGO_PKG_VERSION")));
    log.log(format!(
        "settings: mode={:?} threshold={}px sensitivity={} hwheel={} invert_v={} invert_h={} quantize={} trace={}",
        g.mode, g.threshold, g.sensitivity, g.horizontal, g.invert_vertical,
        g.invert_horizontal, g.quantize, opts.trace
    ));
    if let Some(path) = &opts.log_file {
        log.log(format!("log file: {}", path.display()));
    }
    if is_elevated() {
        log.log("running as administrator: works on elevated windows too");
    } else {
        log.log("running without administrator rights: does not work on elevated windows (UIPI)");
    }
    log.log("press Ctrl+C to quit");

    let out = match output::start(log.clone()) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("error: cannot start output thread: {e}");
            return ExitCode::FAILURE;
        }
    };
    match hook::run(gesture::Gesture::new(opts.gesture), out, log, opts.trace) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Windows 以外では動かない (判定ロジックのテストのみ可能)。
#[cfg(not(windows))]
fn run(_opts: config::Options) -> ExitCode {
    eprintln!("error: nubscroll runs only on Windows");
    ExitCode::FAILURE
}

/// 自プロセスが管理者権限 (昇格済みトークン) で動いているかを返す。
#[cfg(windows)]
fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}
