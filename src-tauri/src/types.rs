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

/// How the dictation hotkey starts/stops a recording.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum TriggerMode {
    /// Hold to talk, release to finish; double-press for hands-free.
    #[default]
    Hold,
    /// Press once to start, press again to finish; releasing does nothing.
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindings {
    /// Missing in older settings files => `hold`.
    #[serde(default)]
    pub trigger: TriggerMode,
    pub dictation: Vec<String>,
    pub ask_anything: Vec<String>,
    pub translation: Vec<String>,
    pub paste_last_transcript: Vec<String>,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            trigger: TriggerMode::default(),
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
    pub provider: String, // mimo | cloudflare | azure | google
    pub mimo: MimoAsrConfig,
    pub cloudflare: CloudflareAsrConfig,
    pub azure: AzureAsrConfig,
    pub google: GoogleAsrConfig,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self { provider: "mimo".into(), mimo: MimoAsrConfig::default(), cloudflare: CloudflareAsrConfig::default(), azure: AzureAsrConfig::default(), google: GoogleAsrConfig::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum LlmProvider {
    #[default]
    Gemini,
    Anthropic,
    Openai,
    Cloudflare,
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
    /// Workers AI text generation; empty credentials fall back to the Cloudflare speech-recognition ones.
    pub cloudflare: CloudflareLlmConfig,
    pub timeout_secs: u32,
    /// Thinking effort for interactive rewrites: "off" | "low" | "medium" | "high". "off" is fastest.
    pub reasoning_effort: String,
    /// Model used by the background personalization analysis.
    pub personalization: PersonalizationLlm,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalizationLlm {
    /// Use the rewrite provider + model (credentials always come from that provider's endpoint config).
    pub follow_rewrite: bool,
    pub provider: LlmProvider,
    /// Empty = that provider's configured rewrite model.
    pub model: String,
    /// "off" | "low" | "medium" | "high"
    pub reasoning_effort: String,
}

impl Default for PersonalizationLlm {
    fn default() -> Self {
        Self { follow_rewrite: true, provider: LlmProvider::Gemini, model: String::new(), reasoning_effort: "medium".into() }
    }
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
            cloudflare: CloudflareLlmConfig::default(),
            timeout_secs: 12,
            reasoning_effort: "off".into(),
            personalization: PersonalizationLlm::default(),
        }
    }
}

/// Cloudflare REST API root; Workers AI's OpenAI-compatible API lives under `/accounts/{id}/ai/v1`.
pub const CLOUDFLARE_API_BASE: &str = "https://api.cloudflare.com/client/v4";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudflareLlmConfig { pub account_id: String, pub api_key: String, pub model: String }
impl Default for CloudflareLlmConfig {
    fn default() -> Self { Self { account_id: String::new(), api_key: String::new(), model: "@cf/meta/llama-3.3-70b-instruct-fp8-fast".into() } }
}
impl CloudflareLlmConfig {
    /// Empty account id / token borrow the speech-recognition ones.
    pub fn resolved(&self, asr: &CloudflareAsrConfig) -> Self {
        let pick = |own: &str, shared: &str| if own.trim().is_empty() { shared.trim().to_string() } else { own.trim().to_string() };
        Self { account_id: pick(&self.account_id, &asr.account_id), api_key: pick(&self.api_key, &asr.api_key), model: self.model.clone() }
    }
    /// OpenAI-compatible endpoint for this account; `base_url` is empty when the account id is missing or malformed.
    pub fn endpoint(&self, api_base: &str) -> LlmEndpointConfig {
        let id = self.account_id.trim();
        let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
        LlmEndpointConfig {
            api_key: self.api_key.clone(),
            base_url: if valid { format!("{}/accounts/{id}/ai/v1", api_base.trim_end_matches('/')) } else { String::new() },
            model: self.model.clone(),
        }
    }
}

impl LlmConfig {
    pub fn active_endpoint(&self) -> LlmEndpointConfig { self.endpoint_for(self.provider) }
    pub fn endpoint_for(&self, provider: LlmProvider) -> LlmEndpointConfig {
        match provider {
            LlmProvider::Gemini => self.gemini.clone(),
            LlmProvider::Anthropic => self.anthropic.clone(),
            LlmProvider::Openai => self.openai.clone(),
            LlmProvider::Cloudflare => self.cloudflare.endpoint(CLOUDFLARE_API_BASE),
        }
    }
    /// Copy with the Cloudflare credentials resolved against the speech-recognition ones; use this at request time.
    pub fn resolved(&self, asr: &AsrConfig) -> LlmConfig {
        let mut c = self.clone();
        c.cloudflare = self.cloudflare.resolved(&asr.cloudflare);
        c
    }
    /// The active provider has everything it needs to be called.
    pub fn ready(&self) -> bool {
        let ep = self.active_endpoint();
        !ep.api_key.trim().is_empty() && (self.provider != LlmProvider::Cloudflare || !ep.base_url.is_empty())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleConfig {
    pub custom_instructions: String,
    pub smart_formatting: bool,
    pub auto_add_dictionary: bool,
    pub ai_personalization: bool,
    /// How long dictation is organised: "paragraphs" (conservative, default) | "list" (Typeless-like numbered lists).
    pub long_form_style: String,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            custom_instructions: String::new(),
            smart_formatting: true,
            auto_add_dictionary: true,
            ai_personalization: true,
            long_form_style: "paragraphs".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCorrection {
    pub wrong: String,
    pub right: String,
}

/// Result of the AI personalization job. Mirror of `PersonalProfile` in types.ts.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalProfile {
    pub updated_at: Option<String>,
    pub analyzed_sessions: u32,
    pub pending_sessions: u32,
    pub next_run_at: Option<String>,
    pub verbal_tics: Vec<String>,
    pub corrections: Vec<ProfileCorrection>,
    pub vocabulary: Vec<String>,
    pub style_notes: Vec<String>,
    pub prompt_addendum: String,
    pub addendum_edited_by_user: bool,
    pub last_error: Option<String>,
    pub running: bool,
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
    pub const PROFILE_UPDATED: &str = "app://profile-updated";
    pub const UPDATE_AVAILABLE: &str = "app://update-available";
    pub const UPDATE_CHECKED: &str = "app://update-checked";
    pub const UPDATE_PROGRESS: &str = "app://update-progress";
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// RFC 3339 publish date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// Payload of `app://update-checked`: the outcome of any check (background, tray or button).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChecked {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<UpdateInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    pub label: String,
    pub sessions: u64,
    pub words: u64,
    pub audio_secs: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub requests: u64,
    /// LLM requests whose response carried no token usage (never set for speech recognition).
    pub unreported_requests: u64,
    /// "asr" | "llm" | "" (non-provider buckets).
    pub kind: String,
    pub cached_tokens: u64,
    /// Median speech-recognition / rewrite latency for this model (ms), when history has timings.
    pub median_ms: Option<u64>,
    pub latency_samples: u64,
    /// App buckets: bundle id of the app (for its icon), when known.
    #[serde(default)]
    pub bundle_id: Option<String>,
}

/// Distribution summary over a set of timings (ms). `samples == 0` means no data.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStat {
    pub samples: u64,
    pub median_ms: u64,
    pub p90_ms: u64,
    pub mean_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LatencyReport {
    /// Release-to-insert time.
    pub total: LatencyStat,
    pub asr: LatencyStat,
    pub llm: LatencyStat,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeCounts {
    pub completed: u64,
    pub dismissed: u64,
    pub error: u64,
    pub empty: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub total: UsageBucket,
    pub completed_sessions: u64,
    pub error_sessions: u64,
    pub dictionary_words: u64,
    pub auto_words: u64,
    pub days: Vec<UsageBucket>,
    pub modes: Vec<UsageBucket>,
    pub apps: Vec<UsageBucket>,
    pub providers: Vec<UsageBucket>,
    // ── additive (usage page v2) ──
    /// Speech-recognition requests only (tokens, if any, are incidental).
    pub asr_total: UsageBucket,
    /// Rewrite-model requests only.
    pub llm_total: UsageBucket,
    pub asr_models: Vec<UsageBucket>,
    pub llm_models: Vec<UsageBucket>,
    pub outcomes: OutcomeCounts,
    pub latency: LatencyReport,
    /// Completed sessions per local hour of day (24 entries).
    pub hours: Vec<u64>,
    /// Completed sessions per weekday, Monday first (7 entries).
    pub weekdays: Vec<u64>,
    pub longest_session_secs: f64,
    pub average_session_secs: f64,
    pub average_words_per_session: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudflareAsrConfig { pub account_id: String, pub api_key: String, pub model: String }
impl Default for CloudflareAsrConfig {
    fn default() -> Self { Self { account_id: String::new(), api_key: String::new(), model: "@cf/openai/whisper-large-v3-turbo".into() } }
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AzureAsrConfig { pub api_key: String, pub region: String }
impl Default for AzureAsrConfig {
    fn default() -> Self { Self { api_key: String::new(), region: "eastasia".into() } }
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct GoogleAsrConfig {
    pub api_key: String, pub service_account_json: String, pub project_id: String,
    pub location: String, pub model: String,
}
impl Default for GoogleAsrConfig {
    fn default() -> Self { Self { api_key: String::new(), service_account_json: String::new(), project_id: String::new(), location: "us".into(), model: "chirp_3".into() } }
}
impl GoogleAsrConfig {
    /// Project to bill: the explicit setting, else the `project_id` inside the service-account JSON.
    pub fn resolved_project(&self) -> Option<String> {
        let explicit = self.project_id.trim();
        if !explicit.is_empty() {
            return Some(explicit.to_string());
        }
        let json: serde_json::Value = serde_json::from_str(&self.service_account_json).ok()?;
        json.get("project_id")?.as_str().map(str::to_string).filter(|p| !p.is_empty())
    }
}
impl AsrConfig {
    pub fn ready(&self) -> bool {
        match self.provider.as_str() {
            "mimo" => !self.mimo.api_key.trim().is_empty(),
            "cloudflare" => !self.cloudflare.api_key.trim().is_empty() && !self.cloudflare.account_id.trim().is_empty(),
            "azure" => !self.azure.api_key.trim().is_empty() && !self.azure.region.trim().is_empty(),
            // Speech-to-Text v2 rejects API keys, so only a service account counts.
            "google" => !self.google.service_account_json.trim().is_empty() && self.google.resolved_project().is_some(),
            _ => false,
        }
    }
    pub fn streaming(&self) -> bool { matches!(self.provider.as_str(), "azure" | "google") }
}
