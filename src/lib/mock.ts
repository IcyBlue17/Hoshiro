// Browser-only mock backend so the UI can be developed with `bun run dev` (no Tauri).
// Agents may enrich the sample data, but must keep the command names identical to ipc.ts.

import type { DictionaryWord, HistoryItem, PersonalProfile, Settings, Stats } from "./types";

type Handler = (payload: any) => void;
const listeners = new Map<string, Set<Handler>>();

export function mockEmit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => h(payload));
}

export async function mockListen<T>(event: string, handler: (payload: T) => void) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler as Handler);
  return () => listeners.get(event)?.delete(handler as Handler);
}

export const defaultSettings: Settings = {
  shortcuts: {
    dictation: ["Fn"],
    askAnything: ["Fn+Space"],
    translation: ["Fn+LeftShift"],
    pasteLastTranscript: ["Ctrl+Cmd+V"],
  },
  microphoneDeviceId: null,
  selectedLanguages: ["zh-CN"],
  autoSelectLanguages: false,
  translationTargets: ["en-US"],
  launchAtLogin: true,
  showBarOnDesktop: false,
  interactionSounds: true,
  showInDock: true,
  muteWhileDictating: true,
  historyRetention: "1m",
  appearance: "system",
  interfaceLanguage: "zh-CN",
  asr: { provider: "mimo",
    cloudflare: { accountId: "", apiKey: "", model: "@cf/openai/whisper-large-v3-turbo" },
    azure: { apiKey: "", region: "eastasia" },
    google: { apiKey: "", serviceAccountJson: "", projectId: "", location: "global", model: "chirp_3" }, mimo: { apiKey: "", baseUrl: "https://api.xiaomimimo.com/v1", model: "mimo-v2.5-asr" } },
  llm: {
    enabled: true,
    provider: "gemini",
    gemini: { apiKey: "", baseUrl: "", model: "gemini-flash-latest" },
    anthropic: { apiKey: "", baseUrl: "", model: "claude-haiku-5-5" },
    openai: { apiKey: "", baseUrl: "https://api.openai.com/v1", model: "gpt-4.1-mini" },
    timeoutSecs: 12,
    reasoningEffort: "off",
    personalization: { followRewrite: true, provider: "gemini", model: "", reasoningEffort: "medium" },
  },
  style: { customInstructions: "", smartFormatting: true, autoAddDictionary: true, aiPersonalization: true, longFormStyle: "paragraphs" },
  onboardingCompleted: true,
};

let settings: Settings = structuredClone(defaultSettings);
// `?shot`: placeholder keys so screenshots show a configured app.
if (typeof location !== "undefined" && new URLSearchParams(location.search).has("shot")) {
  settings.asr.mimo.apiKey = "demo";
  settings.llm.gemini.apiKey = "demo";
}

const now = Date.now();
const iso = (minsAgo: number) => new Date(now - minsAgo * 60_000).toISOString();

let history: HistoryItem[] = [
  {
    id: "h1", createdAt: iso(3), mode: "dictation", status: "completed",
    rawText: "嗯那个我们周三开会，不对，周四开会，然后要准备三个东西第一个是预算第二个是排期第三个是人员",
    refinedText: "我们周四开会，需要准备三样东西：\n\n1. 预算\n2. 排期\n3. 人员",
    selectedText: null, durationSecs: 9.2, appName: "Slack", bundleId: "com.tinyspeck.slackmacgap", windowTitle: null,
    errorMessage: null, hasAudio: true, source: "hoshiro",
  },
  {
    id: "h2", createdAt: iso(42), mode: "translation", status: "completed",
    rawText: "帮我跟他说明天下午三点可以开会", refinedText: "Tomorrow at 3 PM works for the meeting.",
    selectedText: null, durationSecs: 3.1, appName: "Mail", bundleId: "com.apple.mail", windowTitle: null,
    errorMessage: null, hasAudio: true, source: "hoshiro",
  },
  {
    id: "h3", createdAt: iso(60 * 20), mode: "dictation", status: "dismissed",
    rawText: null, refinedText: null, selectedText: null, durationSecs: 2.4, appName: "Notes", bundleId: null, windowTitle: null,
    errorMessage: null, hasAudio: true, source: "hoshiro",
  },
  {
    id: "h4", createdAt: iso(60 * 26), mode: "ask", status: "completed",
    rawText: "把这段改得更礼貌一点", refinedText: "麻烦您有空的时候帮忙看一下这个问题，非常感谢！",
    selectedText: "你赶紧看下这个问题", durationSecs: 2.8, appName: "WeChat", bundleId: "com.tencent.xinWeChat", windowTitle: null,
    errorMessage: null, hasAudio: true, source: "hoshiro",
  },
  {
    id: "h5", createdAt: iso(60 * 27), mode: "dictation", status: "error",
    rawText: null, refinedText: null, selectedText: null, durationSecs: 5.2, appName: "Notes", bundleId: null, windowTitle: null,
    errorMessage: "网络连接失败，请检查网络后重试。", hasAudio: true, source: "hoshiro",
  },
  {
    id: "h6", createdAt: iso(60 * 28), mode: "dictation", status: "empty",
    rawText: null, refinedText: null, selectedText: null, durationSecs: 1.1, appName: "Notes", bundleId: null, windowTitle: null,
    errorMessage: null, hasAudio: true, source: "hoshiro",
  },
  {
    id: "h7", createdAt: iso(60 * 24 * 3), mode: "ask", status: "completed",
    rawText: "Tauri 的 NSPanel 是什么意思", refinedText: "NSPanel 是 macOS AppKit 中 NSWindow 的子类，常用于浮动面板、工具窗口等不抢占焦点的界面。Tauri 通过插件可以把窗口转换成 NSPanel，以实现类似 Spotlight 的悬浮效果。",
    selectedText: null, durationSecs: 4.4, appName: "Safari", bundleId: null, windowTitle: null,
    errorMessage: null, hasAudio: true, source: "hoshiro",
  },
  ...Array.from({ length: 70 }, (_, i): HistoryItem => ({
    id: `x${i}`, createdAt: iso(60 * 24 * (4 + i / 3)), mode: "dictation", status: "completed",
    rawText: `嗯这是第 ${i + 1} 条用来测试分页的口述内容那个就是很长很长`,
    refinedText: `这是第 ${i + 1} 条用来测试分页的口述内容，内容比较长。`,
    selectedText: null, durationSecs: 6, appName: "WeChat", bundleId: null, windowTitle: null,
    errorMessage: null, hasAudio: i % 5 !== 0, source: i % 4 === 0 ? "typeless" : "hoshiro",
  })),
];

let dictionary: DictionaryWord[] = [
  { id: "d1", word: "Typeless", source: "manual", createdAt: iso(1000) },
  { id: "d2", word: "MiMo", source: "manual", createdAt: iso(900) },
  { id: "d3", word: "Hoshiro", source: "auto", createdAt: iso(100) },
  { id: "d4", word: "Tauri", source: "auto", createdAt: iso(50) },
];

function mockStats(): Stats {
  const heatmap = [];
  const today = new Date();
  const rnd = (n: number) => { const x = Math.sin(n * 12.9898) * 43758.5453; return x - Math.floor(x); };
  for (let i = 364; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(d.getDate() - i);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const active = i < 70 && rnd(i + 1) > 0.35;
    heatmap.push({
      date: key,
      words: active ? Math.floor(rnd(i + 99) * 900) + 20 : 0,
      sessions: active ? Math.floor(rnd(i + 7) * 20) + 1 : 0,
    });
  }
  return {
    totalWords: 11234, totalDictationSecs: 4320, averageWpm: 230, timeSavedSecs: 20700,
    activeDays: 35, currentStreak: 0, longestStreak: 14, heatmap,
  };
}

let typelessRunning = !(typeof location !== "undefined" && new URLSearchParams(location.search).has("shot"));
let captureCleanup: (() => void) | null = null;
function stopCapture() { captureCleanup?.(); captureCleanup = null; }
let profile: PersonalProfile = {
  updatedAt: new Date(Date.now() - 3 * 3600_000).toISOString(),
  analyzedSessions: 214,
  pendingSessions: 9,
  nextRunAt: new Date(Date.now() + 19 * 3600_000).toISOString(),
  verbalTics: ["就是说", "然后的话", "对吧", "怎么说呢"],
  corrections: [
    { wrong: "米莫", right: "MiMo" },
    { wrong: "type less", right: "Typeless" },
    { wrong: "陶瑞", right: "Tauri" },
  ],
  vocabulary: ["Hoshiro", "Typeless", "MiMo", "Tauri", "Gemini"],
  styleNotes: ["经常中英混说技术词，英文术语保持原样", "聊天软件里偏口语、短句，不加句号", "长段口述常用“第一、第二”列点"],
  promptAddendum: "用户是开发者，经常在中文里夹英文技术词（API、Tauri、prompt），保持英文原样和大小写。\n在微信 / Slack 中保持口语化短句，不加结尾句号；在邮件和文档中使用完整标点和分段。",
  addendumEditedByUser: false,
  lastError: null,
  running: false,
};

let mockUpdate: any = null;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function mockInvoke<T>(cmd: string, args: any = {}): Promise<T> {
  await sleep(30);
  const r = (v: unknown) => v as T;
  switch (cmd) {
    case "check_update": {
      await sleep(700);
      const q = typeof location !== "undefined" ? new URLSearchParams(location.search).get("update") : null;
      if (q === "err") throw "无法检查更新，请稍后再试";
      mockUpdate = q === "1"
        ? { available: true, currentVersion: "0.1.0", version: "0.2.0", date: "2026-10-10T08:00:00Z",
            notes: "- 新增检查更新\n- 修复长句口述偶尔漏字\n- 用量页加载更快\n- 改进数字转换\n- 若干界面细节" }
        : { available: false, currentVersion: "0.1.0" };
      if (mockUpdate.available) mockEmit("app://update-available", mockUpdate);
      return r(mockUpdate);
    }
    case "get_update_state": return r(mockUpdate);
    case "install_update": {
      const total = 18_000_000; let done = 0;
      await new Promise<void>((res) => { const t = setInterval(() => { done += 900_000; mockEmit("app://update-progress", { downloaded: Math.min(done, total), total }); if (done >= total) { clearInterval(t); res(); } }, 150); });
      return new Promise<T>(() => {}); // the app would relaunch here
    }
    case "take_pending_route": return r(null);
    case "app_info": return r({ name: "Hoshiro", version: "0.1.0", platform: "macos" });
    case "get_settings": return r(structuredClone(settings));
    case "save_settings": settings = structuredClone(args.settings); mockEmit("app://settings-updated", settings); return r(settings);
    case "complete_onboarding": settings.onboardingCompleted = true; return r(undefined);
    case "get_permissions":
    case "request_permission": return r({ microphone: "granted", accessibility: "granted", inputMonitoring: "notDetermined" });
    case "list_microphones": return r([
      { id: "builtin", name: "MacBook Pro麦克风", isDefault: true, kind: "builtin" },
      { id: "airpods", name: "AirPods", isDefault: false, kind: "bluetooth" },
    ]);
    case "start_mic_test": {
      const t = setInterval(() => mockEmit("app://mic-test-level", Math.random()), 50);
      (window as any).__micTest = t; return r(undefined);
    }
    case "start_shortcut_capture": {
      // Browser dev only: synthesize capture events from DOM key events.
      stopCapture();
      const held = new Map<string, string>();
      const nameOf = (e: KeyboardEvent): string => {
        const m: Record<string, string> = {
          ShiftLeft: "LeftShift", ShiftRight: "RightShift", ControlLeft: "LeftCtrl", ControlRight: "RightCtrl",
          AltLeft: "LeftOption", AltRight: "RightOption", MetaLeft: "LeftCmd", MetaRight: "RightCmd", Space: "Space",
          Escape: "Esc", Enter: "Enter", Tab: "Tab", Backspace: "Backspace", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
        };
        if (m[e.code]) return m[e.code];
        if (e.code.startsWith("Key")) return e.code.slice(3);
        if (e.code.startsWith("Digit")) return e.code.slice(5);
        return e.code;
      };
      const order = ["Fn", "Ctrl", "Option", "Shift", "Cmd"];
      const rank = (k: string) => { const i = order.findIndex((o) => k.endsWith(o)); return i < 0 ? 99 : i; };
      const combo = () => [...held.values()].sort((a, b) => rank(a) - rank(b)).join("+");
      const down = (e: KeyboardEvent) => { e.preventDefault(); held.set(e.code, nameOf(e)); lastCombo = combo(); mockEmit("app://shortcut-capture", { keys: lastCombo, done: false }); };
      const up = (e: KeyboardEvent) => { e.preventDefault(); held.delete(e.code); if (held.size === 0 && lastCombo) { mockEmit("app://shortcut-capture", { keys: lastCombo, done: true }); lastCombo = ""; } };
      let lastCombo = "";
      window.addEventListener("keydown", down, true);
      window.addEventListener("keyup", up, true);
      captureCleanup = () => { window.removeEventListener("keydown", down, true); window.removeEventListener("keyup", up, true); };
      return r(undefined);
    }
    case "stop_shortcut_capture": stopCapture(); return r(undefined);
    case "stop_mic_test": clearInterval((window as any).__micTest); return r(undefined);
    case "list_history": {
      const q = args.query;
      let items = history.filter((h) => !q.mode || h.mode === q.mode);
      if (q.search) items = items.filter((h) => (h.refinedText ?? "").includes(q.search));
      return r(items.slice(q.offset, q.offset + q.limit));
    }
    case "delete_history": history = history.filter((h) => h.id !== args.id); return r(undefined);
    case "delete_all_history": history = []; return r(undefined);
    case "retry_history": {
      await sleep(700);
      history = history.map((h) => h.id === args.id ? { ...h, status: "completed", rawText: h.rawText ?? "重试后得到的转录内容", refinedText: h.refinedText ?? "重试后得到的转录内容。", errorMessage: null } : h);
      return r(history.find((h) => h.id === args.id));
    }
    case "list_dictionary": return r([...dictionary]);
    case "add_dictionary_words": {
      const added = (args.words as string[]).map((w, i) => ({ id: `d${Date.now()}${i}`, word: w, source: args.source, createdAt: new Date().toISOString() }));
      dictionary = [...added, ...dictionary]; return r(added);
    }
    case "update_dictionary_word": {
      dictionary = dictionary.map((d) => (d.id === args.id ? { ...d, word: args.word } : d));
      return r(dictionary.find((d) => d.id === args.id));
    }
    case "delete_dictionary_words": dictionary = dictionary.filter((d) => !args.ids.includes(d.id)); return r(undefined);
    case "preview_dictionary_csv": return r(["Kubernetes", "Tauri", "Zustand"]);
    case "get_usage": {
      const n = args.days === 7 ? 7 : args.days === 0 ? 90 : 30;
      const z = { label: "", sessions: 0, words: 0, audioSecs: 0, inputTokens: 0, outputTokens: 0, requests: 0, unreportedRequests: 0, kind: "", cachedTokens: 0, medianMs: null, latencySamples: 0 };
      const k = n / 30;
      const asr = { ...z, label: "MiMo · mimo-v2.5-asr", kind: "asr", requests: Math.round(60 * k), audioSecs: 3100 * k, inputTokens: 900, outputTokens: 400, medianMs: 780, latencySamples: 50 };
      const cf = { ...z, label: "Cloudflare · @cf/openai/whisper-large-v3-turbo", kind: "asr", requests: Math.round(8 * k), audioSecs: 420 * k, medianMs: 1250, latencySamples: 8 };
      const gem = { ...z, label: "Gemini · gemini-flash-latest", kind: "llm", requests: Math.round(70 * k), inputTokens: 48200 * k, outputTokens: 16300 * k, cachedTokens: 9000 * k, medianMs: 640, latencySamples: 60 };
      const cl = { ...z, label: "Anthropic · claude-haiku-5-5", kind: "llm", requests: Math.round(12 * k), inputTokens: 7000 * k, outputTokens: 2100 * k, unreportedRequests: 1, medianMs: 910, latencySamples: 10 };
      const asrTotal = { ...z, requests: asr.requests + cf.requests, audioSecs: asr.audioSecs + cf.audioSecs, inputTokens: 900, outputTokens: 400 };
      const llmTotal = { ...z, requests: gem.requests + cl.requests, inputTokens: gem.inputTokens + cl.inputTokens, outputTokens: gem.outputTokens + cl.outputTokens, cachedTokens: gem.cachedTokens, unreportedRequests: 1 };
      const total = { ...z, sessions: Math.round(92 * k), words: Math.round(12600 * k), audioSecs: asrTotal.audioSecs, inputTokens: llmTotal.inputTokens + 900, outputTokens: llmTotal.outputTokens + 400, requests: asrTotal.requests + llmTotal.requests, unreportedRequests: 1 };
      const days = Array.from({ length: n }, (_, i) => ({ ...z, label: new Date(Date.now() - i * 86400000).toISOString().slice(0, 10), words: 100 + (i * 137) % 800, sessions: i % 5 + 1, audioSecs: 60 + (i * 53) % 300 })).reverse();
      const hours = [0, 0, 0, 0, 0, 1, 2, 6, 14, 22, 18, 12, 9, 11, 19, 24, 17, 10, 8, 12, 9, 5, 2, 1].map((v) => Math.round(v * k));
      return r({
        total, completedSessions: Math.round(80 * k), errorSessions: 2, dictionaryWords: dictionary.length, autoWords: dictionary.filter((w) => w.source === "auto").length,
        days,
        modes: [{ ...z, label: "dictation", sessions: Math.round(64 * k), words: 9200 * k, audioSecs: 2800 * k }, { ...z, label: "translation", sessions: Math.round(11 * k), words: 2200 * k, audioSecs: 500 * k }, { ...z, label: "ask", sessions: Math.round(5 * k), words: 1200 * k, audioSecs: 220 * k }],
        apps: [{ ...z, label: "Cursor", words: 4200 * k, sessions: 20 }, { ...z, label: "微信", words: 3300 * k, sessions: 30 }, { ...z, label: "Notes", words: 1900 * k, sessions: 12 }, { ...z, label: "Typeless 导入", words: 1146 * k, sessions: 9 }, { ...z, label: "Safari", words: 700 * k, sessions: 5 }, { ...z, label: "未知应用", words: 300 * k, sessions: 4 }],
        providers: [gem, asr, cl, cf], asrTotal, llmTotal, asrModels: [asr, cf], llmModels: [gem, cl],
        outcomes: { completed: Math.round(80 * k), dismissed: Math.round(7 * k), error: 2, empty: Math.round(3 * k) },
        latency: { total: { samples: 58, medianMs: 1480, p90Ms: 2650, meanMs: 1710 }, asr: { samples: 58, medianMs: 820, p90Ms: 1500, meanMs: 960 }, llm: { samples: 52, medianMs: 610, p90Ms: 1180, meanMs: 700 } },
        hours, weekdays: [18, 22, 20, 17, 15, 6, 4].map((v) => Math.round(v * k)),
        longestSessionSecs: 184, averageSessionSecs: 38.5, averageWordsPerSession: 157,
      });
    }
    case "app_icons": return r({});
    case "get_stats": return r(mockStats());
    case "get_profile": return r(structuredClone(profile));
    case "run_personalization": {
      profile.running = true; mockEmit("app://profile-updated", structuredClone(profile));
      await sleep(2500);
      profile = { ...profile, running: false, pendingSessions: 0, analyzedSessions: profile.analyzedSessions + profile.pendingSessions, updatedAt: new Date().toISOString() };
      mockEmit("app://profile-updated", structuredClone(profile)); return r(structuredClone(profile));
    }
    case "update_profile": {
      const next: PersonalProfile = structuredClone(args.profile);
      next.addendumEditedByUser ||= next.promptAddendum !== profile.promptAddendum;
      profile = next;
      mockEmit("app://profile-updated", structuredClone(profile));
      return r(structuredClone(profile));
    }
    case "reset_profile": profile = { ...profile, verbalTics: [], corrections: [], vocabulary: [], styleNotes: [], promptAddendum: "", addendumEditedByUser: false, updatedAt: null, analyzedSessions: 0 }; return r(structuredClone(profile));
    case "test_asr":
    case "test_llm": await sleep(600); return r({ ok: true, message: "连接成功", latencyMs: 612 });
    case "list_models": return r(["gemini-flash-latest", "gemini-2.5-flash", "gemini-2.5-pro"]);
    case "detect_typeless": return r({ installed: true, running: typelessRunning, dataFound: true, historyCount: 23, shortcuts: settings.shortcuts });
    case "import_typeless": await sleep(800); return r({ settingsImported: true, historyImported: 23, historySkipped: 0, messages: [] });
    case "quit_typeless": typelessRunning = false; return r(undefined);
    case "bar_get_state": return r({ phase: "idle", mode: "dictation", locked: false, translationTargetLabel: null, elapsedSecs: 0, showIdleOnDesktop: true, dictationKeyLabel: "fn" });
    default: return r(undefined);
  }
}
