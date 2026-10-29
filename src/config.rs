//! Settings, the settings file format and command-line parsing (OS-independent).
//!
//! Settings are layered: defaults < settings file (`nubmiddle.ini`) < command-line options.
//! Every setting has a key; the settings file and the command line both go through
//! [`Settings::set`], so they accept the same values and report the same errors.

use std::path::PathBuf;

use crate::gesture::{DEFAULT_SENSITIVITY, GestureConfig, MoveMode};
use crate::i18n::LangSetting;

/// Usage text shown by `--help`.
pub const USAGE: &str = "\
nubmiddle - middle click and scroll with the middle button of a pointing stick (TrackPoint)

Usage:
    nubmiddle [OPTIONS]

Options:
    --mode <scroll|drag>   What middle button + move does (default: scroll)
    --threshold <PX>       Movement distance [px] beyond which it is no longer
                           a middle click (default: 4)
    --sensitivity <N>      Wheel amount per pixel for both axes; 120 = one notch
                           (default: 10)
    --sensitivity-v <N>    Same, vertical only
    --sensitivity-h <N>    Same, horizontal only
    --no-hwheel            Disable horizontal scrolling
    --no-axis-lock         Scroll both axes at once (by default each scroll is
                           locked to the axis it started along)
    --invert-v             Invert vertical scroll direction
    --invert-h             Invert horizontal scroll direction
    --quantize             Accumulate wheel amount and send it in units of 120
    --no-cursor            Do not change the mouse cursor while scrolling
    --hold-delay <MS>      How long the middle button must be held before the
                           cursor shows the four-way scroll arrow (default: 200)
    --config <FILE>        Settings file to use (default: nubmiddle.ini next to
                           the exe if it exists, else %APPDATA%\\nubmiddle\\nubmiddle.ini)
    --log <FILE>           Also append the log to FILE
    --quit                 Quit the running nubmiddle (waits until it has exited)
    --trace                Log middle button, wheel and in-gesture move events in
                           detail, including events injected by other software or
                           drivers (for troubleshooting)
    -h, --help             Show this help

nubmiddle stays in the notification area (system tray). Use its menu to change
the mode and scroll speeds, pause, quit, or start it automatically at logon with
administrator rights. Changes made in the menu are saved to the settings file;
options given on the command line take precedence over the file.

Internal options (used by the tray menu through a UAC prompt):
    --register-autostart <USER> [ARGS...]
                           Register the logon task for USER, running this exe
                           with ARGS
    --unregister-autostart Delete the logon task
";

/// Settings that can be saved in the settings file.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Gesture recognition settings
    pub gesture: GestureConfig,
    /// Whether to show an arrow cursor for the scroll axis while scrolling
    pub cursor: bool,
    /// Milliseconds the middle button must be held before the four-way arrow cursor shows
    pub hold_delay_ms: u64,
    /// Language of the tray menu and notifications
    pub language: LangSetting,
    /// Devices whose middle button scrolls, as "VVVV:PPPP" ids (or interface names for devices
    /// without ids). Empty means every device. Other devices keep a plain middle button.
    pub devices: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gesture: GestureConfig::default(),
            cursor: true,
            hold_delay_ms: 200,
            language: LangSetting::Auto,
            devices: Vec::new(),
        }
    }
}

/// Keys of the settings file, in the order they are written.
pub const KEYS: [&str; 14] = [
    "mode",
    "threshold",
    "sensitivity_v",
    "sensitivity_h",
    "horizontal",
    "axis_lock",
    "invert_v",
    "invert_h",
    "quantize",
    "cursor",
    "hold_delay_ms",
    "language",
    "devices",
    // Read-only alias that sets both sensitivities (never written)
    "sensitivity",
];

impl Settings {
    /// Sets one setting from its textual value.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let g = &mut self.gesture;
        match key {
            "mode" => {
                g.mode = match value {
                    "scroll" => MoveMode::Scroll,
                    "drag" => MoveMode::Drag,
                    v => return Err(format!("invalid mode: {v} (expected scroll or drag)")),
                }
            }
            "threshold" => {
                g.threshold = parse_num(key, value)?;
                if g.threshold < 0 {
                    return Err("threshold must be 0 or greater".into());
                }
            }
            "sensitivity" => {
                let s = parse_sensitivity(key, value)?;
                (g.sensitivity_v, g.sensitivity_h) = (s, s);
            }
            "sensitivity_v" => g.sensitivity_v = parse_sensitivity(key, value)?,
            "sensitivity_h" => g.sensitivity_h = parse_sensitivity(key, value)?,
            "horizontal" => g.horizontal = parse_bool(key, value)?,
            "axis_lock" => g.axis_lock = parse_bool(key, value)?,
            "invert_v" => g.invert_vertical = parse_bool(key, value)?,
            "invert_h" => g.invert_horizontal = parse_bool(key, value)?,
            "quantize" => g.quantize = parse_bool(key, value)?,
            "cursor" => self.cursor = parse_bool(key, value)?,
            "hold_delay_ms" => self.hold_delay_ms = parse_num(key, value)?,
            "language" => {
                self.language = LangSetting::parse(value)
                    .ok_or_else(|| format!("invalid language: {value} (expected auto, en or ja)"))?
            }
            "devices" => self.devices = parse_devices(value),
            _ => return Err(format!("unknown setting: {key}")),
        }
        Ok(())
    }

    /// Returns the textual value of one setting (the inverse of [`Settings::set`]).
    pub fn get(&self, key: &str) -> Option<String> {
        let g = &self.gesture;
        Some(match key {
            "mode" => match g.mode {
                MoveMode::Scroll => "scroll".into(),
                MoveMode::Drag => "drag".into(),
            },
            "threshold" => g.threshold.to_string(),
            "sensitivity_v" => g.sensitivity_v.to_string(),
            "sensitivity_h" => g.sensitivity_h.to_string(),
            "horizontal" => g.horizontal.to_string(),
            "axis_lock" => g.axis_lock.to_string(),
            "invert_v" => g.invert_vertical.to_string(),
            "invert_h" => g.invert_horizontal.to_string(),
            "quantize" => g.quantize.to_string(),
            "cursor" => self.cursor.to_string(),
            "hold_delay_ms" => self.hold_delay_ms.to_string(),
            "language" => self.language.as_str().into(),
            "devices" if self.devices.is_empty() => "all".into(),
            "devices" => self.devices.join(", "),
            _ => return None,
        })
    }

    /// Applies `key = value` lines of a settings file. Invalid lines are skipped and
    /// reported in the returned warnings (a typo must not stop the tool from starting).
    pub fn apply_ini(&mut self, text: &str) -> Vec<String> {
        let mut warnings = Vec::new();
        for (no, line) in text.lines().enumerate() {
            let line = line.trim();
            // Blank lines, comments and section headers carry no settings
            if line.is_empty() || line.starts_with([';', '#', '[']) {
                continue;
            }
            let result = match line.split_once('=') {
                Some((key, value)) => self.set(key.trim(), value.trim()),
                None => Err(format!("expected key = value: {line}")),
            };
            if let Err(e) = result {
                warnings.push(format!("line {}: {e}", no + 1));
            }
        }
        warnings
    }

    /// Writes all settings as a settings file.
    pub fn to_ini(&self) -> String {
        let mut text = String::from(
            "; nubmiddle settings, written by the tray menu.\n\
             ; Edit freely, then choose \"Reload settings file\" in the menu.\n\
             ; Comments are not kept when the menu saves this file.\n\
             [nubmiddle]\n",
        );
        for key in KEYS {
            if let Some(value) = self.get(key) {
                text += &format!("{key} = {value}\n");
            }
        }
        text
    }

    /// Whether the middle button of the device `id` scrolls. Unknown devices (None) do,
    /// so that a device whose raw input is missing never loses scrolling.
    pub fn scrolls(devices: &[String], id: Option<&str>) -> bool {
        match id {
            Some(id) if !devices.is_empty() => devices.iter().any(|d| d.eq_ignore_ascii_case(id)),
            _ => true,
        }
    }

    /// One-line summary for the log.
    pub fn summary(&self) -> String {
        KEYS.iter()
            .filter_map(|&key| Some(format!("{key}={}", self.get(key)?)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Returns a copy with command-line overrides applied (already validated by [`parse_args`]).
    pub fn with_overrides(&self, overrides: &[(String, String)]) -> Self {
        let mut s = self.clone();
        for (key, value) in overrides {
            let _ = s.set(key, value);
        }
        s
    }
}

/// Scroll speed levels offered in the tray menu, as factors of the default sensitivity
/// (their names are `i18n::Text::Speed`).
pub const SPEED_FACTORS: [f64; 5] = [0.5, 0.7, 1.0, 1.4, 2.0];

/// Sensitivity for a speed level index of [`SPEED_FACTORS`].
pub fn speed_sensitivity(level: usize) -> f64 {
    // Rounded so that the settings file shows 7 rather than 7.000000000000001
    (DEFAULT_SENSITIVITY * SPEED_FACTORS[level] * 100.0).round() / 100.0
}

/// Index of the speed level matching `sensitivity`, or None for a custom value.
pub fn speed_level(sensitivity: f64) -> Option<usize> {
    (0..SPEED_FACTORS.len()).find(|&i| (speed_sensitivity(i) - sensitivity).abs() < 1e-6)
}

/// Runtime options from the command line.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    /// Settings given on the command line, as (key, value), applied over the settings file
    pub overrides: Vec<(String, String)>,
    /// Settings file given with --config
    pub config_file: Option<PathBuf>,
    /// File to append the log to
    pub log_file: Option<PathBuf>,
    /// Whether to log events in detail
    pub trace: bool,
}

/// Result of argument parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Show help and exit
    Help,
    /// Run with the given options
    Run(Options),
    /// Register the autostart task for `user`, running this exe with `args`
    RegisterAutostart { user: String, args: Vec<String> },
    /// Delete the autostart task
    UnregisterAutostart,
    /// Quit the running instance
    Quit,
}

/// Parses command-line arguments (excluding the program name).
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut opts = Options::default();
    // Values are checked against a scratch copy so that errors show up right away
    let mut check = Settings::default();

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
        // Setting changed by this option, if any
        let setting: Option<(&str, String)> = match name.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            // Everything after the user is passed verbatim to the registered task
            "--register-autostart" => {
                let user = value()?;
                return Ok(Command::RegisterAutostart {
                    user,
                    args: args.collect(),
                });
            }
            "--unregister-autostart" => return Ok(Command::UnregisterAutostart),
            "--quit" => return Ok(Command::Quit),
            "--mode" => Some(("mode", value()?)),
            "--threshold" => Some(("threshold", value()?)),
            "--sensitivity" => Some(("sensitivity", value()?)),
            "--sensitivity-v" => Some(("sensitivity_v", value()?)),
            "--sensitivity-h" => Some(("sensitivity_h", value()?)),
            "--no-hwheel" => Some(("horizontal", "false".into())),
            "--no-axis-lock" => Some(("axis_lock", "false".into())),
            "--invert-v" => Some(("invert_v", "true".into())),
            "--invert-h" => Some(("invert_h", "true".into())),
            "--quantize" => Some(("quantize", "true".into())),
            "--no-cursor" => Some(("cursor", "false".into())),
            "--hold-delay" => Some(("hold_delay_ms", value()?)),
            "--config" => {
                opts.config_file = Some(PathBuf::from(value()?));
                None
            }
            "--log" => {
                opts.log_file = Some(PathBuf::from(value()?));
                None
            }
            "--trace" => {
                opts.trace = true;
                None
            }
            _ => return Err(format!("unknown option: {arg} (see --help)")),
        };
        if let Some((key, value)) = setting {
            check.set(key, &value).map_err(|e| format!("{name}: {e}"))?;
            // `sensitivity` sets both axes; store it as the two real keys
            if key == "sensitivity" {
                opts.overrides.push(("sensitivity_v".into(), value.clone()));
                opts.overrides.push(("sensitivity_h".into(), value));
            } else {
                opts.overrides.push((key.into(), value));
            }
        }
    }
    Ok(Command::Run(opts))
}

/// Parses a numeric value.
fn parse_num<T: std::str::FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid number for {name}: {value}"))
}

/// Parses the device list: `all` (or nothing) for every device, else ids separated by commas.
fn parse_devices(value: &str) -> Vec<String> {
    if value.trim().eq_ignore_ascii_case("all") {
        return Vec::new();
    }
    let mut devices: Vec<String> = Vec::new();
    for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        if !devices.iter().any(|d| d.eq_ignore_ascii_case(id)) {
            devices.push(id.to_string());
        }
    }
    devices
}

/// Parses a positive, finite sensitivity.
fn parse_sensitivity(name: &str, value: &str) -> Result<f64, String> {
    let s: f64 = parse_num(name, value)?;
    if s.is_finite() && s > 0.0 {
        Ok(s)
    } else {
        Err(format!("{name} must be a positive number"))
    }
}

/// Parses a boolean (true / false, also yes / no, on / off and 1 / 0).
fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(format!(
            "invalid value for {name}: {value} (expected true or false)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses arguments given as string slices
    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    /// Parses arguments and returns the options (panics unless it is a Run command)
    fn run_opts(args: &[&str]) -> Options {
        let Command::Run(opts) = parse(args).unwrap() else {
            panic!("expected Run")
        };
        opts
    }

    /// Effective settings for the given arguments over the defaults
    fn settings(args: &[&str]) -> Settings {
        Settings::default().with_overrides(&run_opts(args).overrides)
    }

    #[test]
    fn defaults() {
        let opts = run_opts(&[]);
        assert_eq!(opts, Options::default());
        let s = settings(&[]);
        assert_eq!(s.gesture, GestureConfig::default());
        assert!(s.cursor);
        assert_eq!(s.hold_delay_ms, 200);
    }

    #[test]
    fn help() {
        assert_eq!(parse(&["--help"]).unwrap(), Command::Help);
        assert_eq!(parse(&["--trace", "-h"]).unwrap(), Command::Help);
    }

    #[test]
    fn all_options() {
        let args = [
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
            "--config",
            "my.ini",
            "--trace",
            "--no-cursor",
            "--hold-delay",
            "350",
        ];
        let opts = run_opts(&args);
        assert_eq!(opts.log_file, Some(PathBuf::from("a.log")));
        assert_eq!(opts.config_file, Some(PathBuf::from("my.ini")));
        assert!(opts.trace);
        let s = settings(&args);
        let g = &s.gesture;
        assert_eq!(g.mode, MoveMode::Drag);
        assert_eq!(g.threshold, 8);
        assert_eq!((g.sensitivity_v, g.sensitivity_h), (2.5, 2.5));
        assert!(!g.horizontal && !g.axis_lock && g.invert_vertical && g.invert_horizontal);
        assert!(g.quantize);
        assert!(!s.cursor);
        assert_eq!(s.hold_delay_ms, 350);
    }

    #[test]
    fn per_axis_sensitivity() {
        let g = settings(&["--sensitivity", "5", "--sensitivity-h", "20"]).gesture;
        assert_eq!((g.sensitivity_v, g.sensitivity_h), (5.0, 20.0));
        // `sensitivity` is stored as the two per-axis keys
        assert_eq!(
            run_opts(&["--sensitivity", "5"]).overrides,
            vec![
                ("sensitivity_v".to_string(), "5".to_string()),
                ("sensitivity_h".to_string(), "5".to_string())
            ]
        );
    }

    #[test]
    fn autostart_commands() {
        assert_eq!(
            parse(&[
                "--register-autostart",
                r"PC\me",
                "--trace",
                "--log",
                "a b.log"
            ])
            .unwrap(),
            Command::RegisterAutostart {
                user: r"PC\me".into(),
                args: vec!["--trace".into(), "--log".into(), "a b.log".into()],
            }
        );
        assert_eq!(
            parse(&["--register-autostart", "me"]).unwrap(),
            Command::RegisterAutostart {
                user: "me".into(),
                args: vec![],
            }
        );
        assert!(parse(&["--register-autostart"]).is_err());
        assert_eq!(
            parse(&["--unregister-autostart"]).unwrap(),
            Command::UnregisterAutostart
        );
        assert_eq!(parse(&["--quit"]).unwrap(), Command::Quit);
    }

    #[test]
    fn errors() {
        assert!(parse(&["--mode", "fly"]).is_err());
        assert!(parse(&["--threshold"]).is_err());
        assert!(parse(&["--threshold", "-1"]).is_err());
        assert!(parse(&["--sensitivity", "0"]).is_err());
        assert!(parse(&["--sensitivity", "abc"]).is_err());
        assert!(parse(&["--sensitivity-v", "-2"]).is_err());
        assert!(parse(&["--hold-delay", "-1"]).is_err());
        assert!(parse(&["--unknown"]).is_err());
        assert!(Settings::default().set("language", "fr").is_err());
    }

    #[test]
    fn ini_round_trip() {
        let mut s = Settings::default();
        s.gesture.mode = MoveMode::Drag;
        s.gesture.sensitivity_v = 14.0;
        s.gesture.horizontal = false;
        s.hold_delay_ms = 300;
        s.language = LangSetting::Fixed(crate::i18n::Lang::Ja);
        s.devices = vec!["17EF:6009".into(), "04FE:0021".into()];
        let mut loaded = Settings::default();
        assert_eq!(loaded.apply_ini(&s.to_ini()), Vec::<String>::new());
        assert_eq!(loaded, s);
    }

    #[test]
    fn ini_skips_bad_lines_with_warnings() {
        let mut s = Settings::default();
        let warnings = s.apply_ini(
            "; comment\n# comment\n[nubmiddle]\n\n  mode = drag  \nsensitivity_v = fast\nbogus = 1\nno equals\nsensitivity = 7\n",
        );
        assert_eq!(s.gesture.mode, MoveMode::Drag);
        // The bad sensitivity_v line is skipped; the later `sensitivity` sets both
        assert_eq!(
            (s.gesture.sensitivity_v, s.gesture.sensitivity_h),
            (7.0, 7.0)
        );
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].starts_with("line 6:"));
    }

    #[test]
    fn overrides_win_over_file() {
        let mut file = Settings::default();
        file.apply_ini("mode = drag\nsensitivity_v = 20\n");
        let s = file.with_overrides(&run_opts(&["--mode", "scroll"]).overrides);
        assert_eq!(s.gesture.mode, MoveMode::Scroll);
        assert_eq!(s.gesture.sensitivity_v, 20.0);
    }

    #[test]
    fn device_list() {
        let mut s = Settings::default();
        assert_eq!(s.get("devices").unwrap(), "all");
        s.set("devices", " 17EF:6009 ,04fe:0021,, 17ef:6009")
            .unwrap();
        assert_eq!(s.devices, vec!["17EF:6009", "04fe:0021"]);
        assert_eq!(s.get("devices").unwrap(), "17EF:6009, 04fe:0021");
        assert!(Settings::scrolls(&s.devices, Some("04FE:0021")));
        assert!(!Settings::scrolls(&s.devices, Some("046D:C52B")));
        // Unknown devices and an empty list (= all devices) scroll
        assert!(Settings::scrolls(&s.devices, None));
        assert!(Settings::scrolls(&[], Some("046D:C52B")));
        s.set("devices", "ALL").unwrap();
        assert!(s.devices.is_empty());
    }

    #[test]
    fn speed_levels_center_on_default() {
        assert_eq!(speed_level(DEFAULT_SENSITIVITY), Some(2));
        assert_eq!(speed_sensitivity(0), 5.0);
        assert_eq!(speed_sensitivity(4), 20.0);
        assert_eq!(speed_level(speed_sensitivity(3)), Some(3));
        assert_eq!(speed_level(12.0), None);
    }
}
