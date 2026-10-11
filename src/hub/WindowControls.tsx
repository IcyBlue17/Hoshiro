import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { IS_WINDOWS } from "../lib/platform";

/**
 * Windows caption buttons for the frameless hub window (macOS keeps its native traffic lights).
 * Drawn with the Segoe Fluent / MDL2 glyphs Windows itself uses, so they look and size like the real ones.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!IS_WINDOWS) return;
    const w = getCurrentWindow();
    let un: (() => void) | undefined;
    let alive = true;
    const sync = () => w.isMaximized().then((m) => alive && setMaximized(m)).catch(() => {});
    sync();
    w.onResized(sync).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, []);

  if (!IS_WINDOWS) return null;
  const w = () => getCurrentWindow();
  return (
    <div className="win-controls" onDoubleClick={(e) => e.stopPropagation()}>
      <button className="win-btn" aria-label="最小化" title="最小化" onClick={() => w().minimize()}>
        <span className="win-glyph">{""}</span>
      </button>
      <button
        className="win-btn"
        aria-label={maximized ? "向下还原" : "最大化"}
        title={maximized ? "向下还原" : "最大化"}
        onClick={() => w().toggleMaximize()}
      >
        <span className="win-glyph">{maximized ? "" : ""}</span>
      </button>
      <button className="win-btn close" aria-label="关闭" title="关闭" onClick={() => w().close()}>
        <span className="win-glyph">{""}</span>
      </button>
    </div>
  );
}
