/**
 * Host OS, known synchronously (WebView2's user agent says "Windows NT"; WKWebView's says "Macintosh";
 * WebKitGTK's says "X11; Linux").
 * `?platform=windows|macos|linux` overrides it for previewing another platform in a browser during development.
 */
export type Platform = "windows" | "macos" | "linux";

function detect(): Platform {
  if (typeof location !== "undefined") {
    const forced = new URLSearchParams(location.search).get("platform");
    if (forced === "windows" || forced === "macos" || forced === "linux") return forced;
  }
  if (typeof navigator !== "undefined") {
    if (/Windows/i.test(navigator.userAgent)) return "windows";
    if (/Linux|X11/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent)) return "linux";
  }
  return "macos";
}

export const PLATFORM = detect();
export const IS_WINDOWS = PLATFORM === "windows";
export const IS_MAC = PLATFORM === "macos";
export const IS_LINUX = PLATFORM === "linux";
/** PC keyboard (Windows / Linux): no Fn key, Alt and Win / Super instead of ⌥ and ⌘. */
export const IS_PC = !IS_MAC;

/** Name of the Cmd-family key on this platform's keyboards. */
export const SUPER_KEY = IS_LINUX ? "Super" : "Win";

/** Pick the copy / value for the current platform (`linux` falls back to `win`). */
export function byPlatform<T>(values: { mac: T; win: T; linux?: T }): T {
  if (IS_LINUX) return values.linux ?? values.win;
  return IS_WINDOWS ? values.win : values.mac;
}
