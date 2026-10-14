//! nubscroll: a Windows tray tool that makes the TrackPoint middle button do both middle click and scrolling.
//!
//! Currently a console prototype for testing. The tray icon and autostart will be added later.

mod config;
mod gesture;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod logger;
#[cfg(windows)]
mod output;
#[cfg(windows)]
mod process;

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

/// Shows the settings, installs the hook and keeps running.
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
        "settings: mode={:?} threshold={}px sensitivity={} hwheel={} axis_lock={} invert_v={} invert_h={} quantize={} trace={}",
        g.mode, g.threshold, g.sensitivity, g.horizontal, g.axis_lock, g.invert_vertical,
        g.invert_horizontal, g.quantize, opts.trace
    ));
    if let Some(path) = &opts.log_file {
        log.log(format!("log file: {}", path.display()));
    }
    if process::is_current_elevated() {
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

/// Does not run outside Windows (only the logic tests do).
#[cfg(not(windows))]
fn run(_opts: config::Options) -> ExitCode {
    eprintln!("error: nubscroll runs only on Windows");
    ExitCode::FAILURE
}
