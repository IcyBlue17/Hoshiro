import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, RotateCcw, X } from "lucide-react";
import { api } from "../../lib/ipc";
import { EVENTS, type ShortcutBindings, type ShortcutCaptureEvent } from "../../lib/types";
import { useBackendEvent } from "../hooks";
import { useApp } from "../store";
import { DEFAULT_SHORTCUTS, SHORTCUT_ERROR_TEXT, allBindings, validateShortcut } from "../format";
import { ShortcutKeys, Tooltip } from "../ui";
import { Group, Row } from "./shared";

type Feature = keyof ShortcutBindings;

const FEATURES: { key: Feature; title: string; desc: string }[] = [
  { key: "dictation", title: "语音输入", desc: "按住说话，松开结束；双击进入免提模式。" },
  { key: "askAnything", title: "随便问", desc: "按下开始和停止随便提问。" },
  { key: "translation", title: "翻译", desc: "按下开始和停止翻译。" },
  { key: "pasteLastTranscript", title: "粘贴上一次转录", desc: "再次粘贴最近一次的转录结果。" },
];

interface Recording {
  feature: Feature;
  /** index of the chip being replaced, or -1 for a new one */
  index: number;
}

export function ShortcutSettings() {
  const { settings, update } = useApp();
  const [rec, setRec] = useState<Recording | null>(null);
  const [live, setLive] = useState("");
  const [error, setError] = useState<{ feature: Feature; text: string } | null>(null);
  const recRef = useRef(rec);
  recRef.current = rec;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  // Capture is active exactly while a chip is recording.
  const recording = rec !== null;
  useEffect(() => {
    if (!recording) return;
    api.startShortcutCapture().catch(() => {});
    return () => {
      api.stopShortcutCapture().catch(() => {});
    };
  }, [recording]);

  // Keep keystrokes made while recording from reaching the modal (Esc would close it) or the page.
  useEffect(() => {
    if (!recording) return;
    const block = (e: KeyboardEvent) => {
      e.stopPropagation();
      e.preventDefault();
    };
    window.addEventListener("keydown", block, true);
    return () => window.removeEventListener("keydown", block, true);
  }, [recording]);

  const stopRec = useCallback(() => {
    setRec(null);
    setLive("");
  }, []);

  const finalize = useCallback(
    (combo: string) => {
      const r = recRef.current;
      if (!r) return;
      if (combo === "Esc") return stopRec();
      const s = settingsRef.current;
      const list = s.shortcuts[r.feature];
      const replaced = r.index >= 0 ? list[r.index] : null;
      const others = allBindings(s.shortcuts).filter((b, i, arr) => {
        if (replaced === null) return true;
        // drop exactly one occurrence of the shortcut being replaced
        return !(b === replaced && arr.indexOf(b) === i);
      });
      const err = validateShortcut(combo, others);
      if (err) {
        setError({ feature: r.feature, text: SHORTCUT_ERROR_TEXT[err] });
        setLive("");
        api.startShortcutCapture().catch(() => {}); // re-arm
        return;
      }
      update((d) => {
        const arr = d.shortcuts[r.feature];
        if (r.index >= 0) arr[r.index] = combo;
        else arr.push(combo);
      });
      setError(null);
      stopRec();
    },
    [update, stopRec],
  );

  useBackendEvent<ShortcutCaptureEvent>(EVENTS.shortcutCapture, (e) => {
    if (!recRef.current || !e) return;
    setLive(e.keys);
    if (e.done && e.keys) finalize(e.keys);
  });

  const startRec = (feature: Feature, index: number) => {
    setError(null);
    setLive("");
    setRec({ feature, index });
  };

  return (
    <Group>
      {FEATURES.map((f) => {
        const list = settings.shortcuts[f.key];
        const isDefault = JSON.stringify(list) === JSON.stringify(DEFAULT_SHORTCUTS[f.key]);
        const featErr = error?.feature === f.key ? error.text : null;
        return (
          <Row
            key={f.key}
            stacked
            title={
              <span className="sc-head">
                {f.title}
                {!isDefault && (
                  <Tooltip label="重置快捷键">
                    <button
                      className="icon-btn xs"
                      onClick={() => {
                        stopRec();
                        update((d) => void (d.shortcuts[f.key] = [...DEFAULT_SHORTCUTS[f.key]]));
                      }}
                      aria-label="重置快捷键"
                    >
                      <RotateCcw size={13} />
                    </button>
                  </Tooltip>
                )}
              </span>
            }
            desc={f.desc}
          >
            <div className="chip-line">
              {list.map((s, i) => {
                const isRec = rec?.feature === f.key && rec.index === i;
                return (
                  <ShortcutChip
                    key={`${s}-${i}`}
                    shortcut={s}
                    recording={isRec}
                    live={live}
                    onClick={() => (isRec ? stopRec() : startRec(f.key, i))}
                    onRemove={
                      f.key === "dictation" && list.length <= 1
                        ? undefined
                        : () => {
                            stopRec();
                            update((d) => void d.shortcuts[f.key].splice(i, 1));
                          }
                    }
                  />
                );
              })}
              {rec?.feature === f.key && rec.index === -1 ? (
                <ShortcutChip shortcut="" recording live={live} onClick={stopRec} />
              ) : (
                <button className="btn btn-ghost btn-sm" onClick={() => startRec(f.key, -1)}>
                  <Plus size={14} /> {list.length === 0 ? "添加快捷方式" : "添加另一个"}
                </button>
              )}
            </div>
            {featErr && <div className="field-error">{featErr}</div>}
          </Row>
        );
      })}
    </Group>
  );
}

function ShortcutChip({
  shortcut,
  recording,
  live,
  onClick,
  onRemove,
}: {
  shortcut: string;
  recording: boolean;
  live: string;
  onClick: () => void;
  onRemove?: () => void;
}) {
  return (
    <span className={`sc-chip ${recording ? "recording" : ""}`}>
      <button className="sc-chip-main" onClick={onClick} title={recording ? "取消录制" : "更改快捷键"}>
        {recording ? (
          live ? (
            <ShortcutKeys shortcut={live} />
          ) : (
            <span className="sc-placeholder">按下键盘按键或鼠标按钮</span>
          )
        ) : (
          <ShortcutKeys shortcut={shortcut} />
        )}
      </button>
      {!recording && onRemove && (
        <button className="sc-chip-x" onClick={onRemove} aria-label="禁用快捷键" title="禁用快捷键">
          <X size={12} />
        </button>
      )}
    </span>
  );
}
