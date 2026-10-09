import {
  type CSSProperties,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "motion/react";
import { Check, ChevronDown, X } from "lucide-react";
import { prettyKey, splitShortcut } from "./format";

/* ───────────── Keycaps ───────────── */

export function Keycap({ children, size = "md" }: { children: ReactNode; size?: "sm" | "md" }) {
  return <kbd className={`keycap keycap-${size}`}>{children}</kbd>;
}

export function ShortcutKeys({ shortcut, size = "md" }: { shortcut: string; size?: "sm" | "md" }) {
  return (
    <span className="keycaps">
      {splitShortcut(shortcut).map((k, i) => (
        <Keycap key={i} size={size}>
          {prettyKey(k)}
        </Keycap>
      ))}
    </span>
  );
}

/* ───────────── Switch / Checkbox / Segmented ───────────── */

export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`switch ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-knob" />
    </button>
  );
}

export function Checkbox({
  checked,
  onChange,
  disabled,
  indeterminate,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  indeterminate?: boolean;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      disabled={disabled}
      className={`checkbox ${checked ? "on" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
    >
      {checked && <Check size={12} strokeWidth={3.2} />}
      {!checked && indeterminate && <span className="checkbox-dash" />}
    </button>
  );
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  id,
  size = "md",
  disabled,
}: {
  options: { value: T; label: ReactNode }[];
  value: T;
  onChange: (v: T) => void;
  id: string;
  size?: "sm" | "md";
  disabled?: boolean;
}) {
  return (
    <div className={`segmented segmented-${size} ${disabled ? "disabled" : ""}`} role="tablist">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="tab"
          aria-selected={o.value === value}
          disabled={disabled}
          className={`seg-item ${o.value === value ? "active" : ""}`}
          onClick={() => onChange(o.value)}
        >
          {o.value === value && (
            <motion.span
              layoutId={`seg-${id}`}
              className="seg-pill"
              transition={{ type: "spring", stiffness: 520, damping: 40 }}
            />
          )}
          <span className="seg-label">{o.label}</span>
        </button>
      ))}
    </div>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} />;
}

/* ───────────── Modal ───────────── */

const modalStack: symbol[] = [];

export function Modal({
  open,
  onClose,
  children,
  width = 440,
  className = "",
  dismissOnBackdrop = true,
  zIndex = 100,
}: {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  width?: number | string;
  className?: string;
  dismissOnBackdrop?: boolean;
  zIndex?: number;
}) {
  const idRef = useRef<symbol>(Symbol("modal"));
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const id = idRef.current;
    modalStack.push(id);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && modalStack[modalStack.length - 1] === id) {
        e.stopPropagation();
        closeRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      const i = modalStack.indexOf(id);
      if (i >= 0) modalStack.splice(i, 1);
    };
  }, [open]);

  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div
          className="modal-backdrop"
          style={{ zIndex }}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.16 }}
          onMouseDown={(e) => {
            if (dismissOnBackdrop && e.target === e.currentTarget) onClose();
          }}
        >
          <motion.div
            className={`modal ${className}`}
            style={{ width }}
            role="dialog"
            aria-modal="true"
            initial={{ opacity: 0, scale: 0.96, y: 8 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: 4 }}
            transition={{ type: "spring", stiffness: 520, damping: 38 }}
          >
            {children}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

export function ModalHeader({ title, onClose, subtitle }: { title: ReactNode; onClose?: () => void; subtitle?: ReactNode }) {
  return (
    <div className="modal-header">
      <div>
        <h2 className="modal-title">{title}</h2>
        {subtitle && <p className="modal-subtitle">{subtitle}</p>}
      </div>
      {onClose && (
        <button className="icon-btn" onClick={onClose} aria-label="关闭">
          <X size={18} />
        </button>
      )}
    </div>
  );
}

/* ───────────── Popover / Select / Menu ───────────── */

export function Popover({
  open,
  onClose,
  anchorRef,
  children,
  align = "start",
  minWidth,
  className = "",
  matchWidth,
  maxHeight = 320,
}: {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  children: ReactNode;
  align?: "start" | "end";
  minWidth?: number;
  matchWidth?: boolean;
  className?: string;
  maxHeight?: number;
}) {
  const popRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<CSSProperties | null>(null);

  const place = useCallback(() => {
    const a = anchorRef.current;
    if (!a) return;
    const r = a.getBoundingClientRect();
    const popH = popRef.current?.offsetHeight ?? maxHeight;
    const spaceBelow = window.innerHeight - r.bottom - 12;
    const up = spaceBelow < Math.min(popH, 220) && r.top > spaceBelow;
    const style: CSSProperties = { position: "fixed", zIndex: 400 };
    if (up) style.bottom = window.innerHeight - r.top + 6;
    else style.top = r.bottom + 6;
    if (align === "end") style.right = Math.max(8, window.innerWidth - r.right);
    else style.left = Math.max(8, r.left);
    if (matchWidth) style.width = r.width;
    style.maxHeight = Math.max(160, up ? r.top - 16 : spaceBelow);
    setPos(style);
  }, [anchorRef, align, matchWidth, maxHeight]);

  useLayoutEffect(() => {
    if (open) place();
  }, [open, place]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (popRef.current?.contains(t) || anchorRef.current?.contains(t)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    const onScroll = (e: Event) => {
      if (popRef.current?.contains(e.target as Node)) return;
      place();
    };
    document.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open, onClose, anchorRef, place]);

  return createPortal(
    <AnimatePresence>
      {open && pos && (
        <motion.div
          ref={popRef}
          className={`popover ${className}`}
          style={{ ...pos, minWidth }}
          initial={{ opacity: 0, scale: 0.97, y: -4 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.98 }}
          transition={{ duration: 0.12 }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  width,
  placeholder,
  disabled,
  align = "start",
}: {
  value: T | null;
  options: { value: T; label: ReactNode; hint?: ReactNode }[];
  onChange: (v: T) => void;
  width?: number | string;
  placeholder?: string;
  disabled?: boolean;
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const cur = options.find((o) => o.value === value);
  return (
    <>
      <button
        ref={ref}
        type="button"
        disabled={disabled}
        className={`select ${open ? "open" : ""}`}
        style={{ width }}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="select-label">{cur?.label ?? placeholder ?? ""}</span>
        <ChevronDown size={16} className="select-chevron" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={ref} matchWidth minWidth={140} align={align}>
        <div className="menu-list">
          {options.map((o) => (
            <button
              key={o.value}
              type="button"
              className={`menu-item ${o.value === value ? "selected" : ""}`}
              onClick={() => {
                setOpen(false);
                onChange(o.value);
              }}
            >
              <span className="menu-item-label">{o.label}</span>
              {o.hint && <span className="menu-item-hint">{o.hint}</span>}
              {o.value === value && <Check size={15} />}
            </button>
          ))}
        </div>
      </Popover>
    </>
  );
}

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  onClick: () => void;
}

export function DropdownMenu({
  trigger,
  items,
  align = "end",
  className = "",
  onOpenChange,
}: {
  trigger: (props: { open: boolean; toggle: () => void; ref: RefObject<HTMLButtonElement | null> }) => ReactNode;
  items: MenuItem[];
  align?: "start" | "end";
  className?: string;
  onOpenChange?: (open: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const cb = useRef(onOpenChange);
  cb.current = onOpenChange;
  useEffect(() => cb.current?.(open), [open]);
  return (
    <>
      {trigger({ open, toggle: () => setOpen((o) => !o), ref })}
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={ref} align={align} minWidth={160} className={className}>
        <div className="menu-list">
          {items.map((it) => (
            <button
              key={it.label}
              type="button"
              disabled={it.disabled}
              className={`menu-item ${it.danger ? "danger" : ""}`}
              onClick={() => {
                setOpen(false);
                it.onClick();
              }}
            >
              {it.icon}
              <span className="menu-item-label">{it.label}</span>
            </button>
          ))}
        </div>
      </Popover>
    </>
  );
}

/* ───────────── Tooltip ───────────── */

export function Tooltip({
  label,
  children,
  forceOpen,
  side = "top",
  delay = 250,
}: {
  label: ReactNode;
  children: ReactNode;
  forceOpen?: boolean;
  side?: "top" | "bottom";
  delay?: number;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const [hover, setHover] = useState(false);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const show = hover || !!forceOpen;

  useLayoutEffect(() => {
    if (show && ref.current) setRect(ref.current.getBoundingClientRect());
  }, [show]);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <span
      ref={ref}
      className="tip-host"
      onMouseEnter={() => {
        window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => setHover(true), delay);
      }}
      onMouseLeave={() => {
        window.clearTimeout(timer.current);
        setHover(false);
      }}
    >
      {children}
      {createPortal(
        <AnimatePresence>
          {show && rect && (
            <div
              className={`tooltip-anchor tooltip-anchor-${side}`}
              style={{
                position: "fixed",
                left: rect.left + rect.width / 2,
                top: side === "top" ? rect.top - 8 : rect.bottom + 8,
                zIndex: 600,
              }}
            >
              <motion.div
                className={`tooltip tooltip-${side}`}
                initial={{ opacity: 0, y: side === "top" ? 3 : -3 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.12 }}
              >
                {label}
              </motion.div>
            </div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </span>
  );
}

/* ───────────── Misc ───────────── */

export function PageTitle({ children, right }: { children: ReactNode; right?: ReactNode }) {
  return (
    <div className="page-title-row">
      <h1 className="page-title">{children}</h1>
      {right && <div className="page-title-right">{right}</div>}
    </div>
  );
}

/** Debounced text field used for settings values that shouldn't be saved on every keystroke. */
export function useDebouncedField(value: string, commit: (v: string) => void, delay = 400) {
  const [local, setLocal] = useState(value);
  const timer = useRef<number | undefined>(undefined);
  const dirty = useRef(false);
  const commitRef = useRef(commit);
  commitRef.current = commit;
  const localRef = useRef(local);
  localRef.current = local;

  useEffect(() => {
    if (!dirty.current) setLocal(value);
  }, [value]);

  useEffect(
    () => () => {
      if (dirty.current) {
        window.clearTimeout(timer.current);
        commitRef.current(localRef.current);
        dirty.current = false;
      }
    },
    [],
  );

  const onChange = (v: string) => {
    setLocal(v);
    dirty.current = true;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      dirty.current = false;
      commitRef.current(v);
    }, delay);
  };
  return [local, onChange] as const;
}
