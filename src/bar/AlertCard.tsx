import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";
import type { AlertAction, BarAlert } from "../lib/types";

/** Renders `[[fn]]` and `*Esc*` tokens as keycaps. */
export function withKeycaps(text: string): ReactNode {
  const parts = text.split(/(\[\[[^\]]+\]\]|\*[^*\s][^*]*\*)/g);
  return parts.map((p, i) => {
    const m = /^\[\[([^\]]+)\]\]$/.exec(p) ?? /^\*([^*]+)\*$/.exec(p);
    return m ? <kbd key={i} className="keycap">{m[1]}</kbd> : <Fragment key={i}>{p}</Fragment>;
  });
}

function ToneIcon({ tone }: { tone: string }) {
  const p = { size: 16, strokeWidth: 2.2 };
  switch (tone) {
    case "warning": return <TriangleAlert {...p} />;
    case "error": return <CircleAlert {...p} />;
    case "success": return <CircleCheck {...p} />;
    default: return <Info {...p} />;
  }
}

interface Props {
  alert: BarAlert;
  /** Bumped when an alert with the same id is re-sent: restarts the auto-dismiss timer. */
  rev: number;
  onClose: (id: string) => void;
  onAction: (alert: BarAlert, action: AlertAction) => void;
}

export function AlertCard({ alert, rev, onClose, onAction }: Props) {
  const [hovered, setHovered] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const remaining = useRef(alert.durationMs ?? 0);
  const startedAt = useRef(0);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  // Auto-dismiss: pauses while hovered, resumes with the time left (at least 1 s).
  useEffect(() => { remaining.current = alert.durationMs ?? 0; }, [rev, alert.durationMs]);
  useEffect(() => {
    if (alert.durationMs == null || hovered) return;
    startedAt.current = performance.now();
    const id = window.setTimeout(() => closeRef.current(alert.id), remaining.current);
    return () => {
      clearTimeout(id);
      remaining.current = Math.max(1000, remaining.current - (performance.now() - startedAt.current));
    };
  }, [hovered, rev, alert.durationMs, alert.id]);

  const click = (a: AlertAction) => {
    onAction(alert, a);
    if (a.id === "copy") {
      setCopiedId(a.id);
      window.setTimeout(() => closeRef.current(alert.id), 700);
    } else {
      closeRef.current(alert.id);
    }
  };

  return (
    <div
      className={`alert-card tone-${alert.tone}`}
      data-hit
      data-kind={alert.kind}
      role="alert"
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <span className="alert-icon"><ToneIcon tone={alert.tone} /></span>
      <div className="alert-main">
        <div className="alert-title">{withKeycaps(alert.title)}</div>
        {alert.description && <div className="alert-desc">{withKeycaps(alert.description)}</div>}
        {alert.actions.length > 0 && (
          <div className="alert-actions">
            {alert.actions.map((a) => (
              <button
                key={a.id}
                className={`abtn abtn-${a.style}${copiedId === a.id ? " done" : ""}`}
                onClick={() => click(a)}
                disabled={copiedId !== null}
              >
                {copiedId === a.id ? "已复制" : a.label}
              </button>
            ))}
          </div>
        )}
      </div>
      <button className="alert-close" aria-label="关闭" onClick={() => onClose(alert.id)}>
        <X size={11} strokeWidth={2.6} />
      </button>
    </div>
  );
}
