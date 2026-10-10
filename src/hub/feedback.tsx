import { createContext, type ReactNode, useCallback, useContext, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { AlertCircle, CheckCircle2, Info } from "lucide-react";
import { Modal } from "./ui";

type Tone = "success" | "error" | "info";
interface ToastItem {
  id: number;
  text: string;
  tone: Tone;
}

export interface ConfirmOptions {
  title: string;
  description?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

interface FeedbackApi {
  toast: (text: string, tone?: Tone) => void;
  confirm: (opts: ConfirmOptions) => Promise<boolean>;
}

const Ctx = createContext<FeedbackApi | null>(null);

export function useFeedback(): FeedbackApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("FeedbackProvider missing");
  return v;
}

export function FeedbackProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [dialog, setDialog] = useState<(ConfirmOptions & { open: boolean }) | null>(null);
  const resolver = useRef<((v: boolean) => void) | null>(null);
  const nextId = useRef(1);

  const toast = useCallback((text: string, tone: Tone = "success") => {
    const id = nextId.current++;
    setToasts((t) => [...t.slice(-3), { id, text, tone }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), tone === "error" ? 4500 : 2600);
  }, []);

  const confirm = useCallback((opts: ConfirmOptions) => {
    return new Promise<boolean>((resolve) => {
      resolver.current?.(false);
      resolver.current = resolve;
      setDialog({ ...opts, open: true });
    });
  }, []);

  const settle = (v: boolean) => {
    resolver.current?.(v);
    resolver.current = null;
    setDialog((d) => (d ? { ...d, open: false } : d));
  };

  const api = useMemo(() => ({ toast, confirm }), [toast, confirm]);

  return (
    <Ctx.Provider value={api}>
      {children}
      <Modal open={!!dialog?.open} onClose={() => settle(false)} width={380} zIndex={300}>
        <div className="confirm">
          <h3 className="confirm-title">{dialog?.title}</h3>
          {dialog?.description && <p className="confirm-desc">{dialog.description}</p>}
          <div className="confirm-actions">
            <button className="btn btn-secondary" onClick={() => settle(false)}>
              {dialog?.cancelLabel ?? "取消"}
            </button>
            <button className={`btn ${dialog?.danger ? "btn-danger" : "btn-primary"}`} onClick={() => settle(true)} autoFocus>
              {dialog?.confirmLabel ?? "确认"}
            </button>
          </div>
        </div>
      </Modal>
      <div className="toast-stack">
        <AnimatePresence>
          {toasts.map((t) => (
            <motion.div
              key={t.id}
              className={`toast toast-${t.tone}`}
              layout
              initial={{ opacity: 0, y: 14, scale: 0.96 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
              transition={{ type: "spring", stiffness: 520, damping: 36 }}
            >
              {t.tone === "success" ? <CheckCircle2 size={16} /> : t.tone === "error" ? <AlertCircle size={16} /> : <Info size={16} />}
              <span>{t.text}</span>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </Ctx.Provider>
  );
}
