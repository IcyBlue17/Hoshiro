import type { HistoryRetention, Shortcut, ShortcutBindings } from "../lib/types";

export const DEFAULT_SHORTCUTS: ShortcutBindings = {
  dictation: ["Fn"],
  askAnything: ["Fn+Space"],
  translation: ["Fn+LeftShift"],
  pasteLastTranscript: ["Ctrl+Cmd+V"],
};

const KEY_LABELS: Record<string, string> = {
  Fn: "Fn",
  Help: "Help",
  Esc: "Esc",
  Space: "Space",
  Tab: "Tab",
  Enter: "Enter",
  Backspace: "Delete",
  LeftShift: "Left Shift",
  RightShift: "Right Shift",
  Shift: "Shift",
  LeftCtrl: "Left ⌃",
  RightCtrl: "Right ⌃",
  Ctrl: "⌃",
  LeftOption: "Left ⌥",
  RightOption: "Right ⌥",
  Option: "⌥",
  LeftCmd: "Left ⌘",
  RightCmd: "Right ⌘",
  Cmd: "⌘",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
  Mouse3: "Mouse 3",
  Mouse4: "Mouse 4",
  Mouse5: "Mouse 5",
};

export function prettyKey(key: string): string {
  return KEY_LABELS[key] ?? key;
}

export function splitShortcut(s: Shortcut): string[] {
  return s.split("+").filter(Boolean);
}

export function shortcutLabel(s: Shortcut): string {
  return splitShortcut(s).map(prettyKey).join(" + ");
}

/** "[[fn]]" style label for bar hints. */
export function normalizeKey(key: string): string {
  return key.replace(/^(Left|Right)(Shift|Ctrl|Option|Cmd)$/, "$2");
}

export function normalizedCombo(s: Shortcut): string {
  return splitShortcut(s).map(normalizeKey).sort().join("+");
}

const RESERVED = ["Cmd+Q", "Cmd+Tab", "Cmd+Space", "Cmd+W", "Cmd+H", "Cmd+M"].map(normalizedCombo);

export type ShortcutError = "tooManyKeys" | "alphanumericOnly" | "systemReserved" | "alreadyInUse";

export const SHORTCUT_ERROR_TEXT: Record<ShortcutError, string> = {
  tooManyKeys: "快捷键必须包含 3 个或更少的键",
  alphanumericOnly: "快捷键不能只包含字母和数字",
  systemReserved: "此快捷键已保留供系统使用",
  alreadyInUse: "此快捷键已在使用中",
};

/** `others` = every other binding currently configured (excluding the one being replaced). */
export function validateShortcut(combo: Shortcut, others: Shortcut[]): ShortcutError | null {
  const keys = splitShortcut(combo);
  if (keys.length > 3) return "tooManyKeys";
  if (keys.length > 0 && keys.every((k) => /^[A-Z0-9]$/.test(k))) return "alphanumericOnly";
  const norm = normalizedCombo(combo);
  if (RESERVED.includes(norm)) return "systemReserved";
  if (others.some((o) => normalizedCombo(o) === norm || o === combo)) return "alreadyInUse";
  return null;
}

export function allBindings(b: ShortcutBindings): Shortcut[] {
  return [...b.dictation, ...b.askAnything, ...b.translation, ...b.pasteLastTranscript];
}

/** 11234 -> "11.2K", 980 -> "980" */
export function formatWords(n: number): string {
  if (n >= 1_000_000) return `${trim1(n / 1_000_000)}M`;
  if (n >= 10_000) return `${trim1(n / 1000)}K`;
  if (n >= 1000) return `${trim1(n / 1000)}K`;
  return String(Math.round(n));
}
function trim1(v: number): string {
  return (Math.round(v * 10) / 10).toString();
}

/** Seconds -> [{value, unit}] e.g. 20700 -> [5 小时, 45 分钟] */
export function formatDuration(secs: number): { value: number; unit: string }[] {
  const total = Math.max(0, Math.round(secs));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  if (h > 0) return [{ value: h, unit: "小时" }, { value: m, unit: "分钟" }];
  if (m > 0) return [{ value: m, unit: "分钟" }];
  return [{ value: total, unit: "秒" }];
}

export const RETENTION_OPTIONS: { value: HistoryRetention; label: string }[] = [
  { value: "never", label: "从不" },
  { value: "24h", label: "24 小时" },
  { value: "1w", label: "1 周" },
  { value: "1m", label: "1 个月" },
  { value: "1y", label: "1 年" },
  { value: "forever", label: "永远" },
];
export const retentionRank = (r: HistoryRetention) => RETENTION_OPTIONS.findIndex((o) => o.value === r);
export const retentionLabel = (r: HistoryRetention) => RETENTION_OPTIONS.find((o) => o.value === r)?.label ?? r;

export const LANGUAGES: { code: string; label: string }[] = [
  { code: "zh-CN", label: "简体中文（中国大陆）" },
  { code: "zh-TW", label: "繁體中文（台湾）" },
  { code: "zh-HK", label: "繁體中文（香港）" },
  { code: "en-US", label: "英语（美国）" },
  { code: "en-GB", label: "英语（英国）" },
  { code: "en-AU", label: "英语（澳大利亚）" },
  { code: "en-CA", label: "英语（加拿大）" },
  { code: "ja-JP", label: "日语" },
  { code: "ko-KR", label: "韩语" },
  { code: "fr-FR", label: "法语（法国）" },
  { code: "fr-CA", label: "法语（加拿大）" },
  { code: "de-DE", label: "德语" },
  { code: "es-ES", label: "西班牙语（西班牙）" },
  { code: "es-MX", label: "西班牙语（墨西哥）" },
  { code: "es-US", label: "西班牙语（美国）" },
  { code: "pt-BR", label: "葡萄牙语（巴西）" },
  { code: "pt-PT", label: "葡萄牙语（葡萄牙）" },
  { code: "it-IT", label: "意大利语" },
  { code: "ru-RU", label: "俄语" },
  { code: "nl-NL", label: "荷兰语" },
  { code: "pl-PL", label: "波兰语" },
  { code: "tr-TR", label: "土耳其语" },
  { code: "uk-UA", label: "乌克兰语" },
  { code: "vi-VN", label: "越南语" },
  { code: "th-TH", label: "泰语" },
  { code: "id-ID", label: "印尼语" },
  { code: "ms-MY", label: "马来语" },
  { code: "hi-IN", label: "印地语" },
  { code: "ar-SA", label: "阿拉伯语（沙特阿拉伯）" },
  { code: "ar-EG", label: "阿拉伯语（埃及）" },
  { code: "he-IL", label: "希伯来语" },
  { code: "fa-IR", label: "波斯语" },
  { code: "sv-SE", label: "瑞典语" },
  { code: "da-DK", label: "丹麦语" },
  { code: "nb-NO", label: "挪威语" },
  { code: "fi-FI", label: "芬兰语" },
  { code: "cs-CZ", label: "捷克语" },
  { code: "el-GR", label: "希腊语" },
  { code: "hu-HU", label: "匈牙利语" },
  { code: "ro-RO", label: "罗马尼亚语" },
];
export const languageLabel = (code: string) => LANGUAGES.find((l) => l.code === code)?.label ?? code;

export function localDateKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: true });
}

const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
export function groupLabel(key: string): string {
  const today = new Date();
  const y = new Date();
  y.setDate(today.getDate() - 1);
  if (key === localDateKey(today)) return "今天";
  if (key === localDateKey(y)) return "昨天";
  const [yy, mm, dd] = key.split("-").map(Number);
  const d = new Date(yy, mm - 1, dd);
  const base = `${mm}月${dd}日 ${WEEKDAYS[d.getDay()]}`;
  return yy === today.getFullYear() ? base : `${yy}年${base}`;
}
