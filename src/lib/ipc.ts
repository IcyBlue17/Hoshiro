// Typed wrappers around Tauri commands/events.
// When running in a plain browser (vite dev without Tauri) every call is served by ./mock.ts so the UI
// can be developed and screenshotted without the backend.

import type {
  AppInfo,
  AsrConfig,
  BarAlert,
  BarState,
  DictionaryWord,
  HistoryItem,
  HistoryQuery,
  HitRect,
  LlmEndpointConfig,
  LlmProvider,
  MicDevice,
  MimoAsrConfig,
  PersonalProfile,
  PermissionKind,
  Permissions,
  Settings,
  Stats,
  UsageStats,
  TestResult,
  TypelessDetection,
  TypelessImportOptions,
  TypelessImportResult,
} from "./types";
import { mockInvoke, mockListen } from "./mock";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Unlisten = () => void;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) return mockInvoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  if (!isTauri) return mockListen<T>(event, handler);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => handler(e.payload));
}

/** Label of the current webview window: "main" | "bar". */
export function currentWindowLabel(): string {
  if (!isTauri) {
    const p = new URLSearchParams(location.search).get("window");
    return p ?? "main";
  }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? "main";
}

export const api = {
  // ── app / settings ──
  appInfo: () => call<AppInfo>("app_info"),
  getSettings: () => call<Settings>("get_settings"),
  /** Persists and applies immediately (hotkeys, dock, login item...). Returns the saved settings. */
  saveSettings: (settings: Settings) => call<Settings>("save_settings", { settings }),
  completeOnboarding: () => call<void>("complete_onboarding"),

  // ── permissions ──
  getPermissions: () => call<Permissions>("get_permissions"),
  requestPermission: (kind: PermissionKind) => call<Permissions>("request_permission", { kind }),
  openPermissionSettings: (kind: PermissionKind) => call<void>("open_permission_settings", { kind }),
  /** Opens System Settings > Keyboard so the user can set "Press 🌐 key to: Do Nothing". */
  openKeyboardSettings: () => call<void>("open_keyboard_settings"),

  // ── audio ──
  listMicrophones: () => call<MicDevice[]>("list_microphones"),
  /** Starts a mic test; levels arrive on EVENTS.micTestLevel. */
  startMicTest: (deviceId: string | null) => call<void>("start_mic_test", { deviceId }),
  stopMicTest: () => call<void>("stop_mic_test"),

  // ── shortcuts ──
  /** Begin capturing a shortcut; combos arrive on EVENTS.shortcutCapture. Hotkeys are suspended meanwhile. */
  startShortcutCapture: () => call<void>("start_shortcut_capture"),
  stopShortcutCapture: () => call<void>("stop_shortcut_capture"),

  // ── history ──
  listHistory: (query: HistoryQuery) => call<HistoryItem[]>("list_history", { query }),
  deleteHistory: (id: string) => call<void>("delete_history", { id }),
  deleteAllHistory: () => call<void>("delete_all_history"),
  /** Re-runs transcription + rewrite from the saved audio. Returns the updated item. */
  retryHistory: (id: string) => call<HistoryItem>("retry_history", { id }),
  /** Save the item's audio as WAV to `path` (picked with the dialog plugin). */
  exportHistoryAudio: (id: string, path: string) => call<void>("export_history_audio", { id, path }),

  // ── dictionary ──
  listDictionary: () => call<DictionaryWord[]>("list_dictionary"),
  addDictionaryWords: (words: string[], source: "manual" | "auto" = "manual") =>
    call<DictionaryWord[]>("add_dictionary_words", { words, source }),
  updateDictionaryWord: (id: string, word: string) => call<DictionaryWord>("update_dictionary_word", { id, word }),
  deleteDictionaryWords: (ids: string[]) => call<void>("delete_dictionary_words", { ids }),
  /** Parses a one-column CSV and returns the valid words (not yet added). */
  previewDictionaryCsv: (path: string) => call<string[]>("preview_dictionary_csv", { path }),

  // ── AI personalization ──
  getProfile: () => call<PersonalProfile>("get_profile"),
  /** Runs the analysis now (resolves when finished; progress also via EVENTS.profileUpdated with running=true). */
  runPersonalization: () => call<PersonalProfile>("run_personalization"),
  /** Save user edits (removed items, edited promptAddendum). Returns the saved profile. */
  updateProfile: (profile: PersonalProfile) => call<PersonalProfile>("update_profile", { profile }),
  /** Forget everything learned (keeps dictionary words). */
  resetProfile: () => call<PersonalProfile>("reset_profile"),

  // ── stats ──
  getStats: () => call<Stats>("get_stats"),
  getUsage: (days = 30) => call<UsageStats>("get_usage", { days }),

  // ── providers ──
  testAsr: (config: MimoAsrConfig | AsrConfig) => call<TestResult>("test_asr", { config }),
  testLlm: (provider: LlmProvider, config: LlmEndpointConfig) => call<TestResult>("test_llm", { provider, config }),
  listModels: (provider: LlmProvider, config: LlmEndpointConfig) => call<string[]>("list_models", { provider, config }),

  // ── migration ──
  detectTypeless: () => call<TypelessDetection>("detect_typeless"),
  importTypeless: (options: TypelessImportOptions) => call<TypelessImportResult>("import_typeless", { options }),
  quitTypeless: () => call<void>("quit_typeless"),

  // ── misc ──
  copyText: (text: string) => call<void>("copy_text", { text }),
  openUrl: (url: string) => call<void>("open_url", { url }),

  // ── floating bar (called from the bar window) ──
  bar: {
    /** "cancel" | "finish" | "start" (click on idle pill starts a hands-free dictation) | "cycle_translation_target" */
    action: (action: string) => call<void>("bar_action", { action }),
    alertAction: (alertId: string, actionId: string) => call<void>("bar_alert_action", { alertId, actionId }),
    /** Regions (CSS px, relative to bar window) that should receive mouse events. Everything else is click-through. */
    setHitRects: (rects: HitRect[]) => call<void>("bar_set_hit_rects", { rects }),
    getState: () => call<BarState>("bar_get_state"),
    closeAnswer: (id: string) => call<void>("bar_close_answer", { id }),
  },
};

export type { BarAlert, BarState };
