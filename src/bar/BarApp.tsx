import { lazy, Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { Globe, Sparkles, X } from "lucide-react";
import { api, isTauri, listen } from "../lib/ipc";
import { EVENTS, type AlertAction, type AskAnswer, type BarAlert, type BarState } from "../lib/types";
import { Waveform, type LevelRef, type WaveMode } from "./Waveform";
import { AlertCard } from "./AlertCard";
import { AnswerCard } from "./AnswerCard";
import { useHitRects } from "./useHitRects";
import "./bar.css";

const Demo = lazy(() => import("./Demo"));
const DEMO = !isTauri && new URLSearchParams(location.search).get("demo") === "1";

const MAX_ALERTS = 3;
const SPRING = { type: "spring", stiffness: 500, damping: 32 } as const;
const STACK_SPRING = { type: "spring", stiffness: 420, damping: 40, opacity: { duration: 0.18 } } as const;

const HIDDEN: BarState = {
  phase: "hidden", mode: "dictation", locked: false, translationTargetLabel: null,
  elapsedSecs: 0, showIdleOnDesktop: false, dictationKeyLabel: "fn",
};

type Variant = "idle" | "ptt" | "locked" | "processing";

function formatElapsed(secs: number) {
  const s = Math.max(0, Math.floor(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function Elapsed({ startRef }: { startRef: React.MutableRefObject<number | null> }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 500);
    return () => clearInterval(id);
  }, []);
  const start = startRef.current ?? Date.now();
  return <span className="elapsed">{formatElapsed((Date.now() - start) / 1000)}</span>;
}

/** Height-collapsing wrapper so the stack above the pill slides smoothly when items come and go. */
function StackItem({ children }: { children: ReactNode }) {
  return (
    <motion.div
      className="stack-item"
      data-hit-clip
      initial={{ height: 0, opacity: 0, y: 16 }}
      animate={{ height: "auto", opacity: 1, y: 0 }}
      exit={{ height: 0, opacity: 0, y: 6, transition: { duration: 0.2, ease: [0.4, 0, 1, 1] } }}
      transition={STACK_SPRING}
    >
      {children}
    </motion.div>
  );
}

export default function BarApp() {
  const [state, setState] = useState<BarState>(HIDDEN);
  const [alerts, setAlerts] = useState<{ alert: BarAlert; rev: number }[]>([]);
  const [answer, setAnswer] = useState<AskAnswer | null>(null);
  const [hover, setHover] = useState(false);
  const [showTime, setShowTime] = useState(false);

  const rootRef = useRef<HTMLDivElement>(null);
  const levelRef: LevelRef = useRef({ v: 0, t: 0 });
  const recStart = useRef<number | null>(null);
  const gotEvent = useRef(false);

  useHitRects(rootRef);

  const applyState = useCallback((s: BarState) => {
    if (s.phase === "recording") {
      const start = Date.now() - s.elapsedSecs * 1000;
      if (recStart.current == null || (s.elapsedSecs > 0 && Math.abs(recStart.current - start) > 1500)) recStart.current = start;
    } else {
      recStart.current = null;
    }
    setState(s);
  }, []);

  // ── backend wiring ──
  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];
    const add = (p: Promise<() => void>) => p.then((u) => (cancelled ? u() : unlisteners.push(u)));

    add(listen<BarState>(EVENTS.barState, (s) => { gotEvent.current = true; applyState(s); }));
    add(listen<number>(EVENTS.barLevel, (v) => { levelRef.current.v = v; levelRef.current.t = performance.now(); }));
    add(listen<BarAlert>(EVENTS.barAlert, (a) => {
      setAlerts((list) => {
        const i = list.findIndex((x) => x.alert.id === a.id);
        if (i >= 0) return list.map((x, n) => (n === i ? { alert: a, rev: x.rev + 1 } : x));
        return [...list, { alert: a, rev: 0 }].slice(-MAX_ALERTS);
      });
    }));
    add(listen<string>(EVENTS.barAlertDismiss, (id) => setAlerts((list) => list.filter((x) => x.alert.id !== id))));
    add(listen<AskAnswer>(EVENTS.barAnswer, (a) => setAnswer(a)));

    api.bar.getState().then((s) => { if (!cancelled && !gotEvent.current && s) applyState(s); }).catch(() => {});
    return () => { cancelled = true; unlisteners.forEach((u) => u()); };
  }, [applyState]);

  // ── derived pill model ──
  const { phase, mode, locked } = state;
  const visible = phase === "recording" || phase === "processing" || (phase === "idle" && state.showIdleOnDesktop);
  const variant: Variant = phase === "processing" ? "processing" : phase === "recording" ? (locked ? "locked" : "ptt") : "idle";
  const idle = variant === "idle";
  const hovered = idle && hover;

  useEffect(() => { if (!idle) setHover(false); }, [idle]);
  // The webview may never see a pointer-leave once the window turns click-through.
  useEffect(() => {
    const off = () => setHover(false);
    document.addEventListener("mouseleave", off);
    window.addEventListener("blur", off);
    return () => { document.removeEventListener("mouseleave", off); window.removeEventListener("blur", off); };
  }, []);

  // Elapsed time appears in the locked pill only after 60 s.
  useEffect(() => {
    if (variant !== "locked") { setShowTime(false); return; }
    const ms = (recStart.current ?? Date.now()) + 60_000 - Date.now();
    if (ms <= 0) { setShowTime(true); return; }
    setShowTime(false);
    const id = window.setTimeout(() => setShowTime(true), ms);
    return () => clearTimeout(id);
  }, [variant, state.elapsedSecs]);

  let width = 36, height = 8;
  if (variant === "idle") { width = hovered ? 120 : 36; height = hovered ? 28 : 8; }
  else if (variant === "ptt" || variant === "processing") { width = 104; height = 34; }
  else { width = showTime ? 204 : 176; height = 40; }

  const waveMode: WaveMode = variant === "processing" ? "processing" : idle ? "idle" : "live";
  const showWave = !(idle && !hovered);
  const interactive = variant === "idle" || variant === "locked" || variant === "processing";
  const askTint = mode === "ask" && (variant === "locked" || variant === "ptt" || variant === "processing");
  const chipVisible = phase === "recording" && (mode === "translation" || mode === "ask");

  const act = useCallback((a: string) => { void api.bar.action(a); }, []);
  const closeAlert = useCallback((id: string) => setAlerts((l) => l.filter((x) => x.alert.id !== id)), []);
  const alertAction = useCallback((alert: BarAlert, action: AlertAction) => { void api.bar.alertAction(alert.id, action.id); }, []);
  const closeAnswer = useCallback((id: string) => { void api.bar.closeAnswer(id); setAnswer(null); }, []);

  return (
    <MotionConfig reducedMotion="user">
      <div className="bar-root" ref={rootRef}>
        {DEMO && <Suspense fallback={null}><Demo /></Suspense>}

        <div className="stack">
          <AnimatePresence initial={false}>
            {answer && (
              <StackItem key={`answer-${answer.id}`}>
                <AnswerCard answer={answer} onClose={closeAnswer} />
              </StackItem>
            )}
            {alerts.slice(answer ? -2 : -MAX_ALERTS).map(({ alert, rev }) => (
              <StackItem key={alert.id}>
                <AlertCard alert={alert} rev={rev} onClose={closeAlert} onAction={alertAction} />
              </StackItem>
            ))}
            {chipVisible && (
              <StackItem key="chip">
                <div className="chip-row" style={{ width: showTime ? 204 : 176 }}>
                  {mode === "translation" ? (
                    <button className="chip" data-hit data-act="cycle_translation_target" onClick={() => act("cycle_translation_target")} title="切换翻译语言">
                      <Globe size={12} strokeWidth={2.2} />
                      <span>{state.translationTargetLabel ?? "翻译"}</span>
                    </button>
                  ) : (
                    <div className="chip chip-ask">
                      <Sparkles size={12} strokeWidth={2.2} />
                      <span>问任何问题</span>
                    </div>
                  )}
                </div>
              </StackItem>
            )}
          </AnimatePresence>
        </div>

        <div className="pill-zone">
          <AnimatePresence>
            {visible && (
              <motion.div
                key="pill"
                className={`pill v-${variant}${askTint ? " ask" : ""}`}
                role={idle ? "button" : undefined}
                aria-label={idle ? "开始口述" : undefined}
                data-act={idle ? "start" : undefined}
                data-hit={interactive ? "" : undefined}
                data-hit-pad={idle ? "10,8" : undefined}
                onPointerEnter={() => idle && setHover(true)}
                onPointerLeave={() => setHover(false)}
                onClick={() => { if (idle) { setHover(false); act("start"); } }}
                initial={{ opacity: 0, scale: 0.6, width, height, backgroundColor: idle ? "rgba(20,20,20,0.55)" : "rgba(13,13,13,1)" }}
                animate={{
                  opacity: 1, scale: 1, width, height,
                  backgroundColor: idle ? "rgba(20,20,20,0.55)" : "rgba(13,13,13,1)",
                  borderColor: idle ? "rgba(255,255,255,0.18)" : "rgba(255,255,255,0.14)",
                }}
                exit={{ opacity: 0, scale: 0.6, transition: { duration: 0.18, ease: [0.4, 0, 1, 1] } }}
                transition={{ ...SPRING, opacity: { duration: 0.12 }, backgroundColor: { duration: 0.2 }, borderColor: { duration: 0.2 } }}
              >
                <div className="pill-clip">
                  <div className={`shimmer${variant === "processing" ? " on" : ""}`} />
                </div>
                <div className="pill-glow" />

                <div className={`wave-wrap${showWave ? "" : " off"}`}>
                  <Waveform levelRef={levelRef} mode={waveMode} color={askTint ? "232,226,255" : "255,255,255"} />
                  {showTime && variant === "locked" && <Elapsed startRef={recStart} />}
                </div>

                <AnimatePresence initial={false}>
                  {variant === "locked" && (
                    <>
                      <motion.button
                        key="cancel" className="pill-btn cancel" aria-label="取消" data-act="cancel"
                        initial={{ scale: 0.3, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} exit={{ scale: 0.3, opacity: 0, transition: { duration: 0.12 } }}
                        whileTap={{ scale: 0.9 }} transition={SPRING}
                        onClick={() => act("cancel")}
                      >
                        <X size={13} strokeWidth={2.4} />
                      </motion.button>
                      <motion.button
                        key="finish" className="pill-btn finish" aria-label="完成" data-act="finish"
                        initial={{ scale: 0.3, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} exit={{ scale: 0.3, opacity: 0, transition: { duration: 0.12 } }}
                        whileTap={{ scale: 0.9 }} transition={SPRING}
                        onClick={() => act("finish")}
                      >
                        <span className="stop-square" />
                      </motion.button>
                    </>
                  )}
                </AnimatePresence>

                <AnimatePresence>
                  {hovered && (
                    <motion.div
                      className="tooltip"
                      initial={{ opacity: 0, y: 4, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1 }}
                      exit={{ opacity: 0, y: 2, transition: { duration: 0.1 } }}
                      transition={{ duration: 0.14 }}
                    >
                      点击或按住 <kbd className="keycap">{state.dictationKeyLabel}</kbd> 开始口述
                    </motion.div>
                  )}
                </AnimatePresence>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </MotionConfig>
  );
}
