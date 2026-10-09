import { type ReactNode, useState } from "react";
import { Check, Eye, EyeOff, X } from "lucide-react";
import type { TestResult } from "../../lib/types";
import { Spinner, useDebouncedField } from "../ui";

export function Group({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <section className="sgroup">
      {title && <h3 className="sgroup-title">{title}</h3>}
      <div className="sgroup-card">{children}</div>
    </section>
  );
}

export function Row({
  title,
  desc,
  children,
  stacked,
  disabled,
}: {
  title: ReactNode;
  desc?: ReactNode;
  children?: ReactNode;
  stacked?: boolean;
  disabled?: boolean;
}) {
  return (
    <div className={`srow ${stacked ? "stacked" : ""} ${disabled ? "disabled" : ""}`}>
      <div className="srow-text">
        <div className="srow-title">{title}</div>
        {desc && <div className="srow-desc">{desc}</div>}
      </div>
      {children !== undefined && <div className="srow-control">{children}</div>}
    </div>
  );
}

export function TextField({
  value,
  onCommit,
  placeholder,
  mono,
  width,
  type = "text",
  disabled,
  label,
}: {
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  mono?: boolean;
  width?: number | string;
  type?: "text" | "number";
  disabled?: boolean;
  label?: string;
}) {
  const [local, setLocal] = useDebouncedField(value, onCommit);
  return (
    <input
      aria-label={label}
      className={`input ${mono ? "mono" : ""}`}
      style={{ width }}
      type={type}
      value={local}
      placeholder={placeholder}
      disabled={disabled}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      onChange={(e) => setLocal(e.target.value)}
    />
  );
}

export function SecretInput({
  value,
  onCommit,
  placeholder = "粘贴 API Key",
  width,
  disabled,
  label,
}: {
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  width?: number | string;
  disabled?: boolean;
  label?: string;
}) {
  const [local, setLocal] = useDebouncedField(value, onCommit);
  const [show, setShow] = useState(false);
  return (
    <div className="input-wrap" style={{ width }}>
      <input
        aria-label={label}
        className="input mono"
        type={show ? "text" : "password"}
        value={local}
        placeholder={placeholder}
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        onChange={(e) => setLocal(e.target.value)}
      />
      <button type="button" className="input-eye" onClick={() => setShow((s) => !s)} aria-label={show ? "隐藏" : "显示"} tabIndex={-1}>
        {show ? <EyeOff size={15} /> : <Eye size={15} />}
      </button>
    </div>
  );
}

export function useTester(run: () => Promise<TestResult>) {
  const [state, setState] = useState<{ loading: boolean; result: TestResult | null }>({ loading: false, result: null });
  const start = async () => {
    setState({ loading: true, result: null });
    try {
      const result = await run();
      setState({ loading: false, result });
    } catch (e) {
      setState({ loading: false, result: { ok: false, message: String(e), latencyMs: 0 } });
    }
  };
  const reset = () => setState({ loading: false, result: null });
  return { ...state, start, reset };
}

export function TestButton({
  tester,
  disabled,
}: {
  tester: { loading: boolean; result: TestResult | null; start: () => void };
  disabled?: boolean;
}) {
  const r = tester.result;
  return (
    <div className="test-row">
      <button className="btn btn-secondary btn-sm" onClick={tester.start} disabled={disabled || tester.loading}>
        {tester.loading ? <Spinner size={13} /> : null}
        测试连接
      </button>
      {r && (
        <span className={`test-result ${r.ok ? "ok" : "bad"}`}>
          {r.ok ? <Check size={14} /> : <X size={14} />}
          <span>
            {r.ok ? `${r.message || "连接成功"}${r.latencyMs ? ` · ${r.latencyMs} ms` : ""}` : r.message || "连接失败"}
          </span>
        </span>
      )}
    </div>
  );
}
