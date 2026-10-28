//! nubscroll: a Windows tray tool that makes the TrackPoint middle button do both middle click and scrolling.
//!
//! Built as a GUI program so that no console window opens; when started from a console it
//! attaches to it, so `--help` and the log are still printed there.

// No console window on Windows
#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod config;
#[cfg(windows)]
mod cursor;
mod gesture;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod logger;
#[cfg(windows)]
mod output;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod tray;

use std::process::ExitCode;

use config::Command;

fn main() -> ExitCode {
    #[cfg(windows)]
    console::attach();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match config::parse_args(args.clone()) {
        Ok(Command::Help) => {
            print!("{}", config::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Command::Run(opts)) => run(opts, args),
        #[cfg(windows)]
        Ok(Command::RegisterAutostart { user, args }) => {
            change_autostart(|| autostart::register(&user, &autostart::join_args(&args)))
        }
        #[cfg(windows)]
        Ok(Command::UnregisterAutostart) => change_autostart(autostart::unregister),
        #[cfg(not(windows))]
        Ok(Command::RegisterAutostart { .. } | Command::UnregisterAutostart) => {
            change_autostart(|| Err("autostart is available only on Windows".into()))
        }
        Err(e) => {
            report_error(&e);
            ExitCode::from(2)
        }
    }
}

/// Shows an error on the console, or in a message box when there is no console.
fn report_error(msg: &str) {
    eprintln!("error: {msg}");
    #[cfg(windows)]
    if !console::attached() {
        console::message_box(&format!("error: {msg}"));
    }
}

/// Runs the internal autostart registration mode (started elevated from the tray menu).
fn change_autostart(change: impl FnOnce() -> Result<(), String>) -> ExitCode {
    match change() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report_error(&e);
            ExitCode::FAILURE
        }
    }
}

/// Shows the settings, installs the hook and the tray icon, and keeps running.
/// `args` are the original arguments, reused when registering autostart from the tray menu.
#[cfg(windows)]
fn run(opts: config::Options, args: Vec<String>) -> ExitCode {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    // Only one instance: a second hook would process every event twice
    if !single_instance() {
        report_error("nubscroll is already running (see the notification area)");
        return ExitCode::FAILURE;
    }
    let log = match logger::Logger::start(opts.log_file.as_deref()) {
        Ok(log) => log,
        Err(e) => {
            report_error(&format!("cannot open log file: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let g = &opts.gesture;
    log.log(format!("nubscroll {} started", env!("CARGO_PKG_VERSION")));
    log.log(format!(
        "settings: mode={:?} threshold={}px sensitivity={} hwheel={} axis_lock={} invert_v={} invert_h={} quantize={} cursor={} hold_delay={}ms trace={}",
        g.mode, g.threshold, g.sensitivity, g.horizontal, g.axis_lock, g.invert_vertical,
        g.invert_horizontal, g.quantize, opts.cursor, opts.hold_delay_ms, opts.trace
    ));
    if let Some(path) = &opts.log_file {
        log.log(format!("log file: {}", path.display()));
    }
    log.log(format!("exe: {}", autostart::current_exe()));
    if process::is_current_elevated() {
        log.log("running as administrator: works on elevated windows too");
    } else {
        log.log("running without administrator rights: does not work on elevated windows (UIPI)");
    }
    log.log("quit from the tray menu (or with Ctrl+C in this console)");

    let out = match output::start(log.clone()) {
        Ok(out) => out,
        Err(e) => {
            report_error(&format!("cannot start output thread: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let cursor = if opts.cursor {
        match cursor::start(
            log.clone(),
            std::time::Duration::from_millis(opts.hold_delay_ms),
        ) {
            Ok(tx) => Some(tx),
            Err(e) => {
                report_error(&format!("cannot start cursor thread: {e}"));
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    // The tray window lives on this thread, whose message loop is run by hook::run
    if let Err(e) = tray::create(log.clone(), args) {
        report_error(&e);
        return ExitCode::FAILURE;
    }
    // Ctrl+C or closing the console must not leave the scroll cursor or a dead icon behind
    unsafe { SetConsoleCtrlHandler(Some(on_console_ctrl), 1) };
    let result = hook::run(
        gesture::Gesture::new(opts.gesture),
        out,
        cursor,
        log,
        opts.trace,
    );
    tray::remove_icon();
    cursor::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report_error(&e);
            ExitCode::FAILURE
        }
    }
}

/// Takes the per-session "running" mutex. Returns false if another instance holds it.
#[cfg(windows)]
fn single_instance() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\nubscroll.running"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        // The handle is kept open (leaked) for the lifetime of the process
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        let err = GetLastError();
        // An elevated instance's mutex cannot even be opened by a normal one (access denied)
        !(err == ERROR_ALREADY_EXISTS || (handle.is_null() && err == ERROR_ACCESS_DENIED))
    }
}

/// Console control handler (Ctrl+C, closing the console, logoff, shutdown).
#[cfg(windows)]
unsafe extern "system" fn on_console_ctrl(ctrl_type: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    // Ctrl+C / Ctrl+Break: quit cleanly through the message loop
    if matches!(ctrl_type, CTRL_C_EVENT | CTRL_BREAK_EVENT) && tray::request_quit() {
        return 1;
    }
    // Otherwise the process is about to be terminated: clean up now
    cursor::restore();
    tray::remove_icon();
    0
}

/// Does not run outside Windows (only the logic tests do).
#[cfg(not(windows))]
fn run(_opts: config::Options, _args: Vec<String>) -> ExitCode {
    report_error("nubscroll runs only on Windows");
    ExitCode::FAILURE
}

/// Console handling for a GUI-subsystem program.
#[cfg(windows)]
mod console {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    static ATTACHED: AtomicBool = AtomicBool::new(false);

    /// Attaches to the console of the parent process (if started from one) for stdout / stderr.
    pub fn attach() {
        let ok = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0;
        ATTACHED.store(ok, Ordering::Relaxed);
    }

    /// Whether output goes to a console.
    pub fn attached() -> bool {
        ATTACHED.load(Ordering::Relaxed)
    }

    /// Shows an error message box.
    pub fn message_box(msg: &str) {
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (text, caption) = (wide(msg), wide("nubscroll"));
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
    }
}
