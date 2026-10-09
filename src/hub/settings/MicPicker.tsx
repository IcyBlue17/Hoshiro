import { useEffect, useRef, useState } from "react";
import { Bluetooth, Check, Laptop, Mic, Usb, Wand2 } from "lucide-react";
import { api } from "../../lib/ipc";
import { EVENTS, type MicDevice } from "../../lib/types";
import { useBackendEvent } from "../hooks";
import { Modal, ModalHeader } from "../ui";

const BARS = 28;

function kindInfo(kind: string): { icon: typeof Mic; tag: string | null } {
  switch (kind) {
    case "builtin":
      return { icon: Laptop, tag: "推荐" };
    case "bluetooth":
      return { icon: Bluetooth, tag: "蓝牙麦克风" };
    case "usb":
      return { icon: Usb, tag: "外部麦克风" };
    case "virtual":
      return { icon: Mic, tag: "外部麦克风" };
    default:
      return { icon: Mic, tag: null };
  }
}

export function MicPicker({
  open,
  value,
  onClose,
  onSave,
}: {
  open: boolean;
  value: string | null;
  onClose: () => void;
  onSave: (id: string | null) => void;
}) {
  const [devices, setDevices] = useState<MicDevice[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [sel, setSel] = useState<string | null>(value);
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const lastLevel = useRef(0);

  useEffect(() => {
    if (!open) return;
    setSel(value);
    setLoaded(false);
    api
      .listMicrophones()
      .then(setDevices)
      .catch(() => setDevices([]))
      .finally(() => setLoaded(true));
  }, [open, value]);

  useEffect(() => {
    if (!open) return;
    setLevels(Array(BARS).fill(0));
    api.startMicTest(sel).catch(() => {});
    return () => {
      api.stopMicTest().catch(() => {});
    };
  }, [open, sel]);

  useBackendEvent<number>(EVENTS.micTestLevel, (lv) => {
    if (!open) return;
    const smooth = lastLevel.current * 0.35 + (lv ?? 0) * 0.65;
    lastLevel.current = smooth;
    setLevels((cur) => [...cur.slice(1), smooth]);
  });

  const def = devices.find((d) => d.isDefault);

  return (
    <Modal open={open} onClose={onClose} width={460} zIndex={200}>
      <ModalHeader
        title="麦克风"
        subtitle="选择能捕捉到您声音的麦克风。如果指示条没有移动，请尝试其他麦克风。"
        onClose={onClose}
      />
      <div className="modal-body">
        <div className="mic-meter" aria-hidden>
          {levels.map((l, i) => (
            <span key={i} style={{ height: `${Math.max(3, Math.min(1, Math.pow(l, 0.7)) * 34)}px` }} />
          ))}
        </div>
        <div className="mic-list">
          <MicRow
            icon={Wand2}
            name="自动检测"
            sub={def ? `使用系统默认麦克风（${def.name}）` : "使用系统默认麦克风"}
            selected={sel === null}
            onClick={() => setSel(null)}
          />
          {devices.map((d) => {
            const k = kindInfo(d.kind);
            return (
              <MicRow
                key={d.id}
                icon={k.icon}
                name={d.name}
                tag={k.tag}
                selected={sel === d.id}
                onClick={() => setSel(d.id)}
              />
            );
          })}
          {loaded && devices.length === 0 && (
            <div className="mic-empty">
              <b>未检测到麦克风</b>
              <span>检查您的设备设置或连接麦克风。</span>
            </div>
          )}
        </div>
      </div>
      <div className="modal-footer">
        <button className="btn btn-secondary" onClick={onClose}>
          取消
        </button>
        <button
          className="btn btn-primary"
          onClick={() => {
            onSave(sel);
            onClose();
          }}
        >
          保存
        </button>
      </div>
    </Modal>
  );
}

function MicRow({
  icon: Icon,
  name,
  sub,
  tag,
  selected,
  onClick,
}: {
  icon: typeof Mic;
  name: string;
  sub?: string;
  tag?: string | null;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button className={`mic-row ${selected ? "selected" : ""}`} onClick={onClick}>
      <span className="mic-icon">
        <Icon size={17} />
      </span>
      <span className="mic-text">
        <span className="mic-name">{name}</span>
        {sub && <span className="mic-sub">{sub}</span>}
      </span>
      {tag && <span className="mic-tag">{tag}</span>}
      {selected && <Check size={16} className="mic-check" />}
    </button>
  );
}
