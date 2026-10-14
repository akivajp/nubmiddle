//! Command-line argument parsing (OS-independent).
//!
//! For the prototype, settings come only from the command line. An ini file will be added later.

use std::path::PathBuf;

use crate::gesture::{GestureConfig, MoveMode};

/// Usage text shown by `--help`.
pub const USAGE: &str = "\
nubscroll - middle click and scroll with the TrackPoint middle button

Usage:
    nubscroll [OPTIONS]

Options:
    --mode <scroll|drag>   What middle button + move does (default: scroll)
    --threshold <PX>       Movement distance [px] beyond which it is no longer
                           a middle click (default: 4)
    --sensitivity <N>      Wheel amount per pixel; 120 = one notch (default: 10)
    --no-hwheel            Disable horizontal scrolling
    --no-axis-lock         Scroll both axes at once (by default each scroll is
                           locked to the axis it started along)
    --invert-v             Invert vertical scroll direction
    --invert-h             Invert horizontal scroll direction
    --quantize             Accumulate wheel amount and send it in units of 120
    --no-cursor            Do not change the mouse cursor while scrolling
    --log <FILE>           Also append the log to FILE
    --trace                Log middle button, wheel and in-gesture move events in
                           detail, including events injected by other software or
                           drivers (for troubleshooting)
    -h, --help             Show this help
";

/// Runtime options.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Gesture recognition settings
    pub gesture: GestureConfig,
    /// File to append the log to
    pub log_file: Option<PathBuf>,
    /// Whether to log events in detail
    pub trace: bool,
    /// Whether to show an arrow cursor for the scroll axis while scrolling
    pub cursor: bool,
}

/// Result of argument parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Show help and exit
    Help,
    /// Run with the given options
    Run(Options),
}

/// Parses command-line arguments (excluding the program name).
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut gesture = GestureConfig::default();
    let mut log_file = None;
    let mut trace = false;
    let mut cursor = true;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        // Also accept the `--name=value` form
        let (name, inline_value) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        // For options that take a value: use the inline value or the next argument
        let mut value = || {
            inline_value
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match name.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--mode" => {
                gesture.mode = match value()?.as_str() {
                    "scroll" => MoveMode::Scroll,
                    "drag" => MoveMode::Drag,
                    v => {
                        return Err(format!(
                            "invalid value for --mode: {v} (expected scroll or drag)"
                        ));
                    }
                }
            }
            "--threshold" => {
                gesture.threshold = parse_num(&name, &value()?)?;
                if gesture.threshold < 0 {
                    return Err("--threshold must be 0 or greater".into());
                }
            }
            "--sensitivity" => {
                gesture.sensitivity = parse_num(&name, &value()?)?;
                if !(gesture.sensitivity.is_finite() && gesture.sensitivity > 0.0) {
                    return Err("--sensitivity must be a positive number".into());
                }
            }
            "--no-hwheel" => gesture.horizontal = false,
            "--no-axis-lock" => gesture.axis_lock = false,
            "--invert-v" => gesture.invert_vertical = true,
            "--invert-h" => gesture.invert_horizontal = true,
            "--quantize" => gesture.quantize = true,
            "--log" => log_file = Some(PathBuf::from(value()?)),
            "--trace" => trace = true,
            "--no-cursor" => cursor = false,
            _ => return Err(format!("unknown option: {arg} (see --help)")),
        }
    }
    Ok(Command::Run(Options {
        gesture,
        log_file,
        trace,
        cursor,
    }))
}

/// Parses the value of a numeric option.
fn parse_num<T: std::str::FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid number for {name}: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses arguments given as string slices
    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        let Command::Run(opts) = parse(&[]).unwrap() else {
            panic!("expected Run")
        };
        assert_eq!(opts.gesture, GestureConfig::default());
        assert_eq!(opts.log_file, None);
        assert!(!opts.trace);
        assert!(opts.cursor);
    }

    #[test]
    fn help() {
        assert_eq!(parse(&["--help"]).unwrap(), Command::Help);
        assert_eq!(parse(&["--trace", "-h"]).unwrap(), Command::Help);
    }

    #[test]
    fn all_options() {
        let Command::Run(opts) = parse(&[
            "--mode",
            "drag",
            "--threshold=8",
            "--sensitivity",
            "2.5",
            "--no-hwheel",
            "--no-axis-lock",
            "--invert-v",
            "--invert-h",
            "--quantize",
            "--log",
            "a.log",
            "--trace",
            "--no-cursor",
        ])
        .unwrap() else {
            panic!("expected Run")
        };
        let g = &opts.gesture;
        assert_eq!(g.mode, MoveMode::Drag);
        assert_eq!(g.threshold, 8);
        assert_eq!(g.sensitivity, 2.5);
        assert!(
            !g.horizontal && !g.axis_lock && g.invert_vertical && g.invert_horizontal && g.quantize
        );
        assert_eq!(opts.log_file, Some(PathBuf::from("a.log")));
        assert!(opts.trace);
        assert!(!opts.cursor);
    }

    #[test]
    fn errors() {
        assert!(parse(&["--mode", "fly"]).is_err());
        assert!(parse(&["--threshold"]).is_err());
        assert!(parse(&["--threshold", "-1"]).is_err());
        assert!(parse(&["--sensitivity", "0"]).is_err());
        assert!(parse(&["--sensitivity", "abc"]).is_err());
        assert!(parse(&["--unknown"]).is_err());
    }
}
