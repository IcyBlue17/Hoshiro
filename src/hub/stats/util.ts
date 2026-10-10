import type { UsageBucket } from "../../lib/types";
import { localDateKey } from "../format";

export const num = (n: number) => Math.round(n).toLocaleString("zh-CN");

/** ms -> {value, unit} for BigNumber-like displays. */
export function msParts(ms: number): { value: string; unit: string } {
  if (ms < 1000) return { value: String(Math.round(ms)), unit: "毫秒" };
  return { value: (ms / 1000).toFixed(ms < 10_000 ? 1 : 0), unit: "秒" };
}
export function msText(ms: number): string {
  const p = msParts(ms);
  return `${p.value} ${p.unit}`;
}

export function minutesText(secs: number): string {
  const m = secs / 60;
  return m >= 100 ? String(Math.round(m)) : m.toFixed(1);
}

export function tokenText(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}K`;
  return num(n);
}

/** 1, 2, 5 × 10^k ceiling for a y-axis maximum. */
export function niceMax(v: number): number {
  if (v <= 4) return 4;
  const p = Math.pow(10, Math.floor(Math.log10(v)));
  for (const m of [1, 2, 2.5, 5, 10]) if (v <= m * p) return m * p;
  return 10 * p;
}

const MS_DAY = 86_400_000;

/** One entry per local day from the window start to today (missing days are zero). */
export function fillDays(days: UsageBucket[], range: number): UsageBucket[] {
  const byKey = new Map(days.map((d) => [d.label, d]));
  const today = new Date();
  today.setHours(12, 0, 0, 0);
  let span = range;
  if (range === 0) {
    const first = days.length ? new Date(`${days[0].label}T12:00:00`) : today;
    span = Math.min(90, Math.max(7, Math.round((today.getTime() - first.getTime()) / MS_DAY) + 1));
  }
  const out: UsageBucket[] = [];
  for (let i = span - 1; i >= 0; i--) {
    const key = localDateKey(new Date(today.getTime() - i * MS_DAY));
    out.push(
      byKey.get(key) ?? {
        label: key, sessions: 0, words: 0, audioSecs: 0, inputTokens: 0, outputTokens: 0,
        requests: 0, unreportedRequests: 0, kind: "", cachedTokens: 0, medianMs: null, latencySamples: 0,
      },
    );
  }
  return out;
}

/** Stable hue from a name so each app keeps its colour. */
export function nameHue(name: string): number {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.codePointAt(0)!) >>> 0;
  return h % 360;
}
export function initial(name: string): string {
  const c = Array.from(name.trim())[0] ?? "?";
  return c.toUpperCase();
}

export function splitLabel(label: string): [string, string] {
  const i = label.indexOf(" · ");
  return i < 0 ? [label, ""] : [label.slice(0, i), label.slice(i + 3)];
}
