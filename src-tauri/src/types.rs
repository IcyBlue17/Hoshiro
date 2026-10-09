//! Shared types. Mirror of `src/lib/types.ts` — keep both in sync.
//! Wire format is camelCase JSON.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Dictation,
    Translation,
    Ask,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Dictation => "dictation",
            Mode::Translation => "translation",
            Mode::Ask => "ask",
        }
    }
    pub fn from_str(s: &str) -> Mode {
        match s {
            "translation" => Mode::Translation,
            "ask" => Mode::Ask,
            _ => Mode::Dictation,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindings {
    pub dictation: Vec<String>,
    pub ask_anything: Vec<String>,
    pub translation: Vec<String>,
    pub paste_last_transcript: Vec<String>,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            dictation: vec!["Fn".into()],
            ask_anything: vec!["Fn+Space".into()],
            translation: vec!["Fn+LeftShift".into()],
            paste_last_transcript: vec!["Ctrl+Cmd+V".into()],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct MimoAsrConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl Default for MimoAsrConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.xiaomimimo.com/v1".into(),
            model: "mimo-v2.5-asr".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AsrConfig {
    pub provider: String, // "mimo"
    pub mimo: MimoAsrConfig,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self { provider: "mimo".into(), mimo: MimoAsrConfig::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum LlmProvider {
    #[default]
    Gemini,
    Anthropic,
    Openai,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LlmEndpointConfig {
    pub api_key: String,
    /// Empty = official default endpoint.
    pub base_url: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub provider: LlmProvider,
    pub gemini: LlmEndpointConfig,
    pub anthropic: LlmEndpointConfig,
    pub openai: LlmEndpointConfig,
    pub timeout_secs: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: LlmProvider::Gemini,
            gemini: LlmEndpointConfig { api_key: String::new(), base_url: String::new(), model: "gemini-flash-latest".into() },
            anthropic: LlmEndpointConfig { api_key: String::new(), base_url: String::new(), model: "claude-haiku-5-5".into() },
            openai: LlmEndpointConfig {
                api_key: String::new(),
                base_url: "https://api.openai.com/v1".into(),
                model: "gpt-4.1-mini".into(),
            },
            timeout_secs: 12,
        }
    }
}

impl LlmConfig {
    pub fn active_endpoint(&self) -> &LlmEndpointConfig {
        match self.provider {
            LlmProvider::Gemini => &self.gemini,
            LlmProvider::Anthropic => &self.anthropic,
            LlmProvider::Openai => &self.openai,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleConfig {
    pub custom_instructions: String,
    pub smart_formatting: bool,
    pub auto_add_dictionary: bool,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self { custom_instructions: String::new(), smart_formatting: true, auto_add_dictionary: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub shortcuts: ShortcutBindings,
    pub microphone_device_id: Option<String>,
    pub selected_languages: Vec<String>,
    pub auto_select_languages: bool,
    pub translation_targets: Vec<String>,
    pub launch_at_login: bool,
    pub show_bar_on_desktop: bool,
    pub interaction_sounds: bool,
    pub show_in_dock: bool,
    pub mute_while_dictating: bool,
    /// "never" | "24h" | "1w" | "1m" | "1y" | "forever"
    pub history_retention: String,
    /// "system" | "light" | "dark"
    pub appearance: String,
    /// "zh-CN" | "en-US"
    pub interface_language: String,
    pub asr: AsrConfig,
    pub llm: LlmConfig,
    pub style: StyleConfig,
    pub onboarding_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcuts: ShortcutBindings::default(),
            microphone_device_id: None,
            selected_languages: vec!["zh-CN".into()],
            auto_select_languages: false,
            translation_targets: vec!["en-US".into()],
            launch_at_login: true,
            show_bar_on_desktop: false,
            interaction_sounds: true,
            show_in_dock: true,
            mute_while_dictating: true,
            history_retention: "1m".into(),
            appearance: "system".into(),
            interface_language: "zh-CN".into(),
            asr: AsrConfig::default(),
            llm: LlmConfig::default(),
            style: StyleConfig::default(),
            onboarding_completed: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub created_at: String,
    pub mode: Mode,
    /// "completed" | "dismissed" | "error" | "empty" | "processing"
    pub status: String,
    pub raw_text: Option<String>,
    pub refined_text: Option<String>,
    pub selected_text: Option<String>,
    pub duration_secs: f64,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub error_message: Option<String>,
    pub has_audio: bool,
    pub source: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub mode: Option<Mode>,
    pub limit: u32,
    pub offset: u32,
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryWord {
    pub id: String,
    pub word: String,
    /// "manual" | "auto"
    pub source: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapDay {
    pub date: String,
    pub words: u64,
    pub sessions: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total_words: u64,
    pub total_dictation_secs: f64,
    pub average_wpm: f64,
    pub time_saved_secs: f64,
    pub active_days: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub heatmap: Vec<HeatmapDay>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MicDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// "builtin" | "bluetooth" | "usb" | "virtual" | "unknown"
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub microphone: PermissionState,
    pub accessibility: PermissionState,
    pub input_monitoring: PermissionState,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Microphone,
    Accessibility,
    InputMonitoring,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TypelessDetection {
    pub installed: bool,
    pub running: bool,
    pub data_found: bool,
    pub history_count: u32,
    pub shortcuts: Option<ShortcutBindings>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TypelessImportOptions {
    pub settings: bool,
    pub history: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TypelessImportResult {
    pub settings_imported: bool,
    pub history_imported: u32,
    pub history_skipped: u32,
    pub messages: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCaptureEvent {
    pub keys: String,
    pub done: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub message: String,
    pub latency_ms: u64,
}

// ───────────────────────── Floating bar ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum BarPhase {
    #[default]
    Hidden,
    Idle,
    Recording,
    Processing,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct BarState {
    pub phase: BarPhase,
    pub mode: Mode,
    pub locked: bool,
    pub translation_target_label: Option<String>,
    pub elapsed_secs: f64,
    pub show_idle_on_desktop: bool,
    pub dictation_key_label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AlertAction {
    pub id: String,
    pub label: String,
    /// "primary" | "secondary" | "danger"
    pub style: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BarAlert {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
    pub actions: Vec<AlertAction>,
    pub duration_ms: Option<u64>,
    /// "info" | "warning" | "error" | "success"
    pub tone: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AskAnswer {
    pub id: String,
    pub question: String,
    pub answer: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub mod events {
    pub const BAR_STATE: &str = "bar://state";
    pub const BAR_LEVEL: &str = "bar://level";
    pub const BAR_ALERT: &str = "bar://alert";
    pub const BAR_ALERT_DISMISS: &str = "bar://alert-dismiss";
    pub const BAR_ANSWER: &str = "bar://answer";
    pub const HISTORY_UPDATED: &str = "app://history-updated";
    pub const SETTINGS_UPDATED: &str = "app://settings-updated";
    pub const STATS_UPDATED: &str = "app://stats-updated";
    pub const NAVIGATE: &str = "app://navigate";
    pub const SHORTCUT_CAPTURE: &str = "app://shortcut-capture";
    pub const MIC_TEST_LEVEL: &str = "app://mic-test-level";
    pub const PERMISSIONS_CHANGED: &str = "app://permissions-changed";
}
