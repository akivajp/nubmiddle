//! コマンドライン引数の解析 (OS 非依存)。
//!
//! 試作段階ではコマンドラインだけで設定する。ini ファイルでの設定は後で追加する。

use std::path::PathBuf;

use crate::gesture::{GestureConfig, MoveMode};

/// `--help` で表示する使い方。
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
    --invert-v             Invert vertical scroll direction
    --invert-h             Invert horizontal scroll direction
    --quantize             Accumulate wheel amount and send it in units of 120
    --log <FILE>           Also append the log to FILE
    --trace                Log middle button, wheel and in-gesture move events in
                           detail, including events injected by other software or
                           drivers (for troubleshooting)
    -h, --help             Show this help
";

/// 実行時の設定。
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// ジェスチャー判定の設定
    pub gesture: GestureConfig,
    /// ログの追記先ファイル
    pub log_file: Option<PathBuf>,
    /// イベントの詳細ログを出すか
    pub trace: bool,
}

/// 引数解析の結果。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// ヘルプを表示して終了する
    Help,
    /// 指定の設定で常駐する
    Run(Options),
}

/// コマンドライン引数 (プログラム名を除く) を解析する。
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut gesture = GestureConfig::default();
    let mut log_file = None;
    let mut trace = false;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        // `--name=value` 形式も受け付ける
        let (name, inline_value) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        // 値を取るオプション用: インライン値か次の引数を取り出す
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
            "--invert-v" => gesture.invert_vertical = true,
            "--invert-h" => gesture.invert_horizontal = true,
            "--quantize" => gesture.quantize = true,
            "--log" => log_file = Some(PathBuf::from(value()?)),
            "--trace" => trace = true,
            _ => return Err(format!("unknown option: {arg} (see --help)")),
        }
    }
    Ok(Command::Run(Options {
        gesture,
        log_file,
        trace,
    }))
}

/// 数値オプションの値を解析する。
fn parse_num<T: std::str::FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid number for {name}: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文字列スライスから引数を解析する
    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        let Command::Run(opts) = parse(&[]).unwrap() else {
            panic!("Run を期待")
        };
        assert_eq!(opts.gesture, GestureConfig::default());
        assert_eq!(opts.log_file, None);
        assert!(!opts.trace);
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
            "--invert-v",
            "--invert-h",
            "--quantize",
            "--log",
            "a.log",
            "--trace",
        ])
        .unwrap() else {
            panic!("Run を期待")
        };
        let g = &opts.gesture;
        assert_eq!(g.mode, MoveMode::Drag);
        assert_eq!(g.threshold, 8);
        assert_eq!(g.sensitivity, 2.5);
        assert!(!g.horizontal && g.invert_vertical && g.invert_horizontal && g.quantize);
        assert_eq!(opts.log_file, Some(PathBuf::from("a.log")));
        assert!(opts.trace);
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
