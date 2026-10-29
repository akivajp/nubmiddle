//! Translations of the tray menu and notifications (OS-independent).
//!
//! Only user-facing UI text is translated. Logs, `--help` and error messages stay in English
//! so that they can be shared in bug reports.

/// Language of the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

/// Language setting: a fixed language, or follow the Windows display language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LangSetting {
    #[default]
    Auto,
    Fixed(Lang),
}

impl LangSetting {
    /// Parses a settings-file value (`auto`, `en` or `ja`).
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "en" => Some(Self::Fixed(Lang::En)),
            "ja" => Some(Self::Fixed(Lang::Ja)),
            _ => None,
        }
    }

    /// The settings-file value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fixed(Lang::En) => "en",
            Self::Fixed(Lang::Ja) => "ja",
        }
    }

    /// Resolves `Auto` with the Windows UI language id (LANGID; None if unknown).
    pub fn resolve(self, ui_langid: Option<u16>) -> Lang {
        match self {
            Self::Fixed(lang) => lang,
            Self::Auto => match ui_langid {
                // The low 10 bits are the primary language; 0x11 is Japanese
                Some(id) if id & 0x3FF == 0x11 => Lang::Ja,
                _ => Lang::En,
            },
        }
    }
}

/// UI texts. `{}` in a text is replaced by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    MoveMenu,
    ModeScroll,
    ModeDrag,
    VerticalSpeed,
    HorizontalSpeed,
    /// Speed levels, in the order of `config::SPEED_FACTORS`
    Speed(usize),
    /// A speed that is not one of the levels; `{}` is the value
    CustomSpeed,
    DevicesMenu,
    DevicesAll,
    /// Suffix for a device in the settings that is not connected
    NotConnected,
    ResetDefaults,
    OpenSettings,
    ReloadSettings,
    Autostart,
    AutostartChecking,
    AutostartUpdate,
    Pause,
    Quit,
    LanguageMenu,
    LanguageAuto,
    /// Tooltip suffixes
    TipAdministrator,
    TipPaused,
    AutostartStaleTitle,
    /// `{}` is the exe the task still starts
    AutostartStaleBody,
    SettingsErrorsTitle,
    /// `{}` is the number of lines, then the details
    SettingsErrorsBody,
    SettingsReloaded,
}

/// Every text, for tests.
#[cfg(test)]
const ALL: [Text; 31] = [
    Text::MoveMenu,
    Text::ModeScroll,
    Text::ModeDrag,
    Text::VerticalSpeed,
    Text::HorizontalSpeed,
    Text::Speed(0),
    Text::Speed(1),
    Text::Speed(2),
    Text::Speed(3),
    Text::Speed(4),
    Text::CustomSpeed,
    Text::DevicesMenu,
    Text::DevicesAll,
    Text::NotConnected,
    Text::ResetDefaults,
    Text::OpenSettings,
    Text::ReloadSettings,
    Text::Autostart,
    Text::AutostartChecking,
    Text::AutostartUpdate,
    Text::Pause,
    Text::Quit,
    Text::LanguageMenu,
    Text::LanguageAuto,
    Text::TipAdministrator,
    Text::TipPaused,
    Text::AutostartStaleTitle,
    Text::AutostartStaleBody,
    Text::SettingsErrorsTitle,
    Text::SettingsErrorsBody,
    Text::SettingsReloaded,
];

/// Returns the text in the given language.
pub fn tr(lang: Lang, text: Text) -> &'static str {
    use Lang::*;
    use Text::*;
    match (text, lang) {
        (MoveMenu, En) => "Middle button + move",
        (MoveMenu, Ja) => "中ボタン + 移動",
        (ModeScroll, En) => "Scroll",
        (ModeScroll, Ja) => "スクロール",
        (ModeDrag, En) => "Middle-button drag (no scrolling)",
        (ModeDrag, Ja) => "中ボタンドラッグ (スクロールしない)",
        (VerticalSpeed, En) => "Vertical scroll speed",
        (VerticalSpeed, Ja) => "縦スクロールの速さ",
        (HorizontalSpeed, En) => "Horizontal scroll speed",
        (HorizontalSpeed, Ja) => "横スクロールの速さ",
        (Speed(0), En) => "Slower (x0.5)",
        (Speed(0), Ja) => "とても遅い (x0.5)",
        (Speed(1), En) => "Slow (x0.7)",
        (Speed(1), Ja) => "遅い (x0.7)",
        (Speed(2), En) => "Normal",
        (Speed(2), Ja) => "標準",
        (Speed(3), En) => "Fast (x1.4)",
        (Speed(3), Ja) => "速い (x1.4)",
        (Speed(_), En) => "Faster (x2)",
        (Speed(_), Ja) => "とても速い (x2)",
        (CustomSpeed, En) => "Custom ({})",
        (CustomSpeed, Ja) => "カスタム ({})",
        (DevicesMenu, En) => "Devices that scroll",
        (DevicesMenu, Ja) => "スクロールするデバイス",
        (DevicesAll, En) => "All mice and pointing devices",
        (DevicesAll, Ja) => "すべてのマウス・ポインティングデバイス",
        (NotConnected, En) => " (not connected)",
        (NotConnected, Ja) => " (未接続)",
        (ResetDefaults, En) => "Reset to defaults",
        (ResetDefaults, Ja) => "既定値に戻す",
        (OpenSettings, En) => "Open settings file",
        (OpenSettings, Ja) => "設定ファイルを開く",
        (ReloadSettings, En) => "Reload settings file",
        (ReloadSettings, Ja) => "設定ファイルを読み込み直す",
        (Autostart, En) => "Start with Windows (as administrator)",
        (Autostart, Ja) => "Windows の起動時に開始 (管理者)",
        (AutostartChecking, En) => "Start with Windows (checking...)",
        (AutostartChecking, Ja) => "Windows の起動時に開始 (確認中...)",
        (AutostartUpdate, En) => "Start with Windows: update to this exe",
        (AutostartUpdate, Ja) => "Windows の起動時に開始: この exe に更新",
        (Pause, En) => "Pause",
        (Pause, Ja) => "一時停止",
        (Quit, En) => "Quit",
        (Quit, Ja) => "終了",
        // Readable whatever the current language is
        (LanguageMenu, _) => "Language / 言語",
        (LanguageAuto, En) => "Auto (Windows display language)",
        (LanguageAuto, Ja) => "自動 (Windows の表示言語)",
        (TipAdministrator, En) => " (administrator)",
        (TipAdministrator, Ja) => " (管理者)",
        (TipPaused, En) => " - paused",
        (TipPaused, Ja) => " - 一時停止中",
        (AutostartStaleTitle, En) => "nubmiddle: autostart needs an update",
        (AutostartStaleTitle, Ja) => "nubmiddle: 自動起動の更新が必要です",
        (AutostartStaleBody, En) => {
            "Autostart still starts {}. Choose \"Start with Windows: update to this exe\" in the tray menu."
        }
        (AutostartStaleBody, Ja) => {
            "自動起動はまだ {} を起動します。トレイメニューの「Windows の起動時に開始: この exe に更新」を選んでください。"
        }
        (SettingsErrorsTitle, En) => "nubmiddle: settings file has errors",
        (SettingsErrorsTitle, Ja) => "nubmiddle: 設定ファイルに誤りがあります",
        (SettingsErrorsBody, En) => "{} line(s) were skipped: ",
        (SettingsErrorsBody, Ja) => "{} 行を読み飛ばしました: ",
        (SettingsReloaded, En) => "Settings file reloaded.",
        (SettingsReloaded, Ja) => "設定ファイルを読み込み直しました。",
    }
}

/// Returns the text with its `{}` replaced by `arg`.
pub fn trf(lang: Lang, text: Text, arg: &str) -> String {
    tr(lang, text).replacen("{}", arg, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_every_language() {
        for text in ALL {
            for lang in [Lang::En, Lang::Ja] {
                assert!(!tr(lang, text).is_empty(), "{text:?} in {lang:?}");
            }
            // Texts with a placeholder have it in both languages
            assert_eq!(
                tr(Lang::En, text).contains("{}"),
                tr(Lang::Ja, text).contains("{}"),
                "{text:?}"
            );
        }
    }

    #[test]
    fn auto_follows_the_windows_language() {
        // 0x0411 = ja-JP, 0x0409 = en-US, 0x0407 = de-DE
        assert_eq!(LangSetting::Auto.resolve(Some(0x0411)), Lang::Ja);
        assert_eq!(LangSetting::Auto.resolve(Some(0x0409)), Lang::En);
        assert_eq!(LangSetting::Auto.resolve(Some(0x0407)), Lang::En);
        assert_eq!(LangSetting::Auto.resolve(None), Lang::En);
        assert_eq!(LangSetting::Fixed(Lang::Ja).resolve(Some(0x0409)), Lang::Ja);
    }

    #[test]
    fn setting_values_round_trip() {
        for s in [
            LangSetting::Auto,
            LangSetting::Fixed(Lang::En),
            LangSetting::Fixed(Lang::Ja),
        ] {
            assert_eq!(LangSetting::parse(s.as_str()), Some(s));
        }
        assert_eq!(LangSetting::parse("JA"), Some(LangSetting::Fixed(Lang::Ja)));
        assert_eq!(LangSetting::parse("fr"), None);
    }

    #[test]
    fn trf_fills_the_placeholder() {
        assert_eq!(trf(Lang::En, Text::CustomSpeed, "12"), "Custom (12)");
        assert_eq!(trf(Lang::Ja, Text::CustomSpeed, "12"), "カスタム (12)");
    }
}
