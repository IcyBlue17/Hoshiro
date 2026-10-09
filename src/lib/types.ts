// Shared contract between the Rust backend (src-tauri/src/types.rs) and the UI.
// Field names are camelCase on the wire (serde rename_all = "camelCase").
// Keep this file in sync with types.rs — it is the single source of truth for the UI.

export type Mode = "dictation" | "translation" | "ask";

export type Appearance = "system" | "light" | "dark";

/** How long history is kept on this device (mirrors Typeless options). */
export type HistoryRetention = "never" | "24h" | "1w" | "1m" | "1y" | "forever";

export type LlmProvider = "gemini" | "anthropic" | "openai";

/**
 * A shortcut is a "+"-joined list of key names, e.g. "Fn", "RightOption", "Fn+LeftShift", "Ctrl+Cmd+V".
 * Key names: Fn, Help, Esc, Space, Tab, Enter, Backspace,
 *   LeftShift RightShift Shift, LeftCtrl RightCtrl Ctrl, LeftOption RightOption Option, LeftCmd RightCmd Cmd,
 *   A..Z, 0..9, F1..F20, Up Down Left Right, Mouse3 Mouse4 Mouse5 (extra mouse buttons).
 */
export type Shortcut = string;

export interface ShortcutBindings {
  /** Hold to talk, double-press for hands-free. Multiple bindings allowed (Typeless default: ["Fn"]). */
  dictation: Shortcut[];
  /** Press to start/stop "Ask anything" (voice command / question). Default ["Fn+Space"]. */
  askAnything: Shortcut[];
  /** Press to start translation mode, press dictation key to finish. Default ["Fn+LeftShift"]. */
  translation: Shortcut[];
  /** Paste the last transcript again. Default ["Ctrl+Cmd+V"]. */
  pasteLastTranscript: Shortcut[];
}

export interface MimoAsrConfig {
  apiKey: string;
  /** "https://api.xiaomimimo.com/v1" (global, default) or "https://token-plan-cn.xiaomimimo.com/v1" (China token plan) or custom */
  baseUrl: string;
  model: string; // "mimo-v2.5-asr"
}

export interface AsrConfig {
  provider: "mimo";
  mimo: MimoAsrConfig;
}

export interface LlmEndpointConfig {
  apiKey: string;
  /** Empty = official default endpoint. */
  baseUrl: string;
  model: string;
}

export interface LlmConfig {
  /** When false, the raw ASR transcript is inserted without AI polishing. */
  enabled: boolean;
  provider: LlmProvider;
  gemini: LlmEndpointConfig; // default model "gemini-flash-latest"
  anthropic: LlmEndpointConfig; // default model "claude-haiku-5-5"
  openai: LlmEndpointConfig; // OpenAI-compatible; baseUrl e.g. "https://api.openai.com/v1"
  /** Request timeout in seconds before falling back to the raw transcript. */
  timeoutSecs: number;
}

export interface StyleConfig {
  /** Free-form extra instructions appended to the rewrite prompt ("个性化"). */
  customInstructions: string;
  /** Turn spoken enumerations into lists, add paragraphs, etc. (Typeless "smart formatting"). */
  smartFormatting: boolean;
  /** Learn words automatically from the user's edits after insertion. */
  autoAddDictionary: boolean;
  /** Periodically let the rewrite model analyse history + edits and build a personal profile. */
  aiPersonalization: boolean;
}

export interface Settings {
  shortcuts: ShortcutBindings;
  /** null = auto-detect (system default input). Otherwise a device id from listMicrophones(). */
  microphoneDeviceId: string | null;
  /** Dictation languages in BCP-47 with variant, e.g. ["zh-CN"], ["zh-CN","en-US"]. */
  selectedLanguages: string[];
  autoSelectLanguages: boolean;
  /** Up to 3 translation targets, e.g. ["en-US"]. The first one is the active target. */
  translationTargets: string[];
  launchAtLogin: boolean;
  /** Show the tiny idle floating bar at the bottom of the screen when not dictating. */
  showBarOnDesktop: boolean;
  interactionSounds: boolean;
  showInDock: boolean;
  muteWhileDictating: boolean;
  historyRetention: HistoryRetention;
  appearance: Appearance;
  interfaceLanguage: "zh-CN" | "en-US";
  asr: AsrConfig;
  llm: LlmConfig;
  style: StyleConfig;
  /** Set once onboarding is finished. */
  onboardingCompleted: boolean;
}

export type HistoryStatus = "completed" | "dismissed" | "error" | "empty" | "processing";

export interface HistoryItem {
  id: string;
  /** ISO 8601 */
  createdAt: string;
  mode: Mode;
  status: HistoryStatus;
  /** Raw ASR transcript */
  rawText: string | null;
  /** Final (polished / translated / answered) text */
  refinedText: string | null;
  /** For ask mode: the spoken instruction is rawText, selected text (if any) here */
  selectedText: string | null;
  durationSecs: number;
  appName: string | null;
  bundleId: string | null;
  windowTitle: string | null;
  errorMessage: string | null;
  hasAudio: boolean;
  /** "hoshiro" or "typeless" (imported) */
  source: string;
}

export interface HistoryQuery {
  /** null = all */
  mode: Mode | null;
  limit: number;
  offset: number;
  search?: string;
}

export interface DictionaryWord {
  id: string;
  word: string;
  source: "manual" | "auto";
  createdAt: string;
}

export interface HeatmapDay {
  /** YYYY-MM-DD (local time) */
  date: string;
  words: number;
  sessions: number;
}

export interface Stats {
  totalWords: number;
  totalDictationSecs: number;
  /** Average words per minute */
  averageWpm: number;
  /** Estimated vs typing at 40 wpm */
  timeSavedSecs: number;
  activeDays: number;
  currentStreak: number;
  longestStreak: number;
  /** Last ~ 52 weeks, oldest first */
  heatmap: HeatmapDay[];
}

export interface ProfileCorrection {
  /** What the recognizer/rewrite tends to produce */
  wrong: string;
  /** What the user actually means */
  right: string;
}

/** Result of the AI personalization job (see SPEC §9). */
export interface PersonalProfile {
  /** ISO time of the last successful analysis, null = never */
  updatedAt: string | null;
  /** Sessions included in the last analysis */
  analyzedSessions: number;
  /** Completed sessions recorded since the last analysis */
  pendingSessions: number;
  /** ISO time of the next automatic run (null when disabled / not scheduled) */
  nextRunAt: string | null;
  /** Verbal tics / fillers specific to this user (removed during rewrite) */
  verbalTics: string[];
  /** Recurring recognition errors or user fixes */
  corrections: ProfileCorrection[];
  /** Proper nouns / terms learned (also added to the dictionary as "auto") */
  vocabulary: string[];
  /** Human-readable observations about the user's style, shown in the UI */
  styleNotes: string[];
  /** Generated personal prompt section injected into rewrites (user-editable) */
  promptAddendum: string;
  /** True once the user edited promptAddendum — AI runs then stop overwriting it */
  addendumEditedByUser: boolean;
  lastError: string | null;
  running: boolean;
}

export interface MicDevice {
  id: string;
  name: string;
  isDefault: boolean;
  /** "builtin" | "bluetooth" | "usb" | "virtual" | "unknown" */
  kind: string;
}

export type PermissionState = "granted" | "denied" | "notDetermined";

export interface Permissions {
  microphone: PermissionState;
  accessibility: PermissionState;
  inputMonitoring: PermissionState;
}

export type PermissionKind = "microphone" | "accessibility" | "inputMonitoring";

export interface TypelessDetection {
  installed: boolean;
  running: boolean;
  dataFound: boolean;
  historyCount: number;
  /** Shortcuts found in Typeless settings, for preview */
  shortcuts: ShortcutBindings | null;
}

export interface TypelessImportOptions {
  settings: boolean;
  history: boolean;
}

export interface TypelessImportResult {
  settingsImported: boolean;
  historyImported: number;
  historySkipped: number;
  messages: string[];
}

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
}

export interface ShortcutCaptureEvent {
  /** Current combination being held, e.g. "Fn+LeftShift" */
  keys: Shortcut;
  /** True when the user released everything — `keys` is the final combo */
  done: boolean;
}

export interface TestResult {
  ok: boolean;
  message: string;
  latencyMs: number;
}

// ───────────────────────── Floating bar ─────────────────────────

export type BarPhase =
  | "hidden" // nothing visible
  | "idle" // tiny resting pill (only when showBarOnDesktop)
  | "recording"
  | "processing";

export interface BarState {
  phase: BarPhase;
  mode: Mode;
  /** Hands-free / toggle session: show cancel (X) and finish buttons */
  locked: boolean;
  /** e.g. "English" — shown in translation mode */
  translationTargetLabel: string | null;
  /** Seconds elapsed in this recording (for long sessions) */
  elapsedSecs: number;
  showIdleOnDesktop: boolean;
  /** Label of the dictation key for hints, e.g. "fn" */
  dictationKeyLabel: string;
}

export type AlertActionStyle = "primary" | "secondary" | "danger";

export interface AlertAction {
  id: string; // e.g. "retry" | "copy" | "undo" | "open_settings" | "open_history" | "dismiss"
  label: string;
  style: AlertActionStyle;
}

export interface BarAlert {
  id: string;
  /** e.g. "transcript_cancelled" | "transcription_error" | "no_network" | "copy_last" | "hint" | "dictionary_added" ... */
  kind: string;
  /** May contain key tokens like "[[fn]]" which render as keycaps. */
  title: string;
  description: string | null;
  actions: AlertAction[];
  /** Auto-dismiss after this many ms; null = sticky until action/close */
  durationMs: number | null;
  /** "info" | "warning" | "error" | "success" */
  tone: string;
}

export interface AskAnswer {
  id: string;
  question: string;
  answer: string;
}

export interface HitRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Event names (Rust -> UI) */
export const EVENTS = {
  barState: "bar://state",
  barLevel: "bar://level", // payload: number 0..1
  barAlert: "bar://alert", // payload: BarAlert
  barAlertDismiss: "bar://alert-dismiss", // payload: string (alert id)
  barAnswer: "bar://answer", // payload: AskAnswer
  historyUpdated: "app://history-updated",
  settingsUpdated: "app://settings-updated", // payload: Settings
  statsUpdated: "app://stats-updated",
  navigate: "app://navigate", // payload: "home" | "history" | "dictionary" | "settings"
  shortcutCapture: "app://shortcut-capture", // payload: ShortcutCaptureEvent
  micTestLevel: "app://mic-test-level", // payload: number 0..1
  permissionsChanged: "app://permissions-changed", // payload: Permissions
  profileUpdated: "app://profile-updated", // payload: PersonalProfile
} as const;
