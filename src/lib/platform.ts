/**
 * Host OS, known synchronously (WebView2's user agent says "Windows NT"; WKWebView's says "Macintosh").
 * `?platform=windows|macos` overrides it for previewing the other platform in a browser during development.
 */
function detect(): "windows" | "macos" {
  if (typeof location !== "undefined") {
    const forced = new URLSearchParams(location.search).get("platform");
    if (forced === "windows" || forced === "macos") return forced;
  }
  if (typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent)) return "windows";
  return "macos";
}

export const PLATFORM = detect();
export const IS_WINDOWS = PLATFORM === "windows";
export const IS_MAC = PLATFORM === "macos";

/** Pick the copy / value for the current platform. */
export function byPlatform<T>(values: { mac: T; win: T }): T {
  return IS_WINDOWS ? values.win : values.mac;
}
