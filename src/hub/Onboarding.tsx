import { useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, ArrowLeft, ArrowRight, Check } from "lucide-react";
import logo from "./assets/logo.png";
import { api } from "../lib/ipc";
import type { TypelessDetection, TypelessImportResult } from "../lib/types";
import { useApp } from "./store";
import { useFeedback } from "./feedback";
import { Keycap, ShortcutKeys, Spinner } from "./ui";
import { Group } from "./settings/shared";
import { PermissionRows } from "./settings/permissions";
import { AsrForm, LlmForm } from "./settings/Providers";

type StepId = "welcome" | "import" | "permissions" | "providers" | "fn" | "try" | "done";

export function Onboarding({ onDone }: { onDone: () => Promise<void> }) {
  const { toast } = useFeedback();
  const [det, setDet] = useState<TypelessDetection | null>(null);
  const [detDone, setDetDone] = useState(false);
  const [idx, setIdx] = useState(0);
  const [dir, setDir] = useState(1);
  const [finishing, setFinishing] = useState(false);

  useEffect(() => {
    api
      .detectTypeless()
      .then(setDet)
      .catch(() => {})
      .finally(() => setDetDone(true));
  }, []);

  const steps = useMemo<StepId[]>(() => {
    const s: StepId[] = ["welcome"];
    if (det?.installed && det.dataFound) s.push("import");
    s.push("permissions", "providers", "fn", "try", "done");
    return s;
  }, [det]);

  const step = steps[Math.min(idx, steps.length - 1)];
  const last = idx >= steps.length - 1;
  const go = (n: number) => {
    setDir(n > idx ? 1 : -1);
    setIdx(Math.max(0, Math.min(steps.length - 1, n)));
  };

  const finish = async () => {
    setFinishing(true);
    try {
      await onDone();
    } catch (e) {
      toast(`完成引导失败：${String(e)}`, "error");
      setFinishing(false);
    }
  };

  return (
    <div className="onboarding">
      <div className="drag-strip" data-tauri-drag-region />
      <div className="ob-glow" />
      <div className="ob-shell">
        <div className="ob-stage">
          <AnimatePresence mode="wait" custom={dir} initial={false}>
            <motion.div
              key={step}
              className="ob-step"
              custom={dir}
              initial={{ opacity: 0, x: 28 * dir }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -28 * dir }}
              transition={{ duration: 0.2, ease: "easeOut" }}
            >
              {step === "welcome" && <Welcome />}
              {step === "import" && <ImportStep det={det} onDetect={setDet} />}
              {step === "permissions" && <PermStep />}
              {step === "providers" && <ProvidersStep />}
              {step === "fn" && <FnStep />}
              {step === "try" && <TryStep />}
              {step === "done" && <DoneStep />}
            </motion.div>
          </AnimatePresence>
        </div>

        <div className="ob-footer">
          <button className="btn btn-ghost" onClick={() => go(idx - 1)} style={{ visibility: idx === 0 ? "hidden" : "visible" }}>
            <ArrowLeft size={16} /> 上一步
          </button>
          <div className="ob-dots">
            {steps.map((s, i) => (
              <span key={s} className={`ob-dot ${i === idx ? "active" : i < idx ? "past" : ""}`} />
            ))}
          </div>
          {last ? (
            <button className="btn btn-primary" onClick={finish} disabled={finishing}>
              {finishing ? <Spinner size={14} /> : <Check size={16} />} 开始使用
            </button>
          ) : (
            <button className="btn btn-primary" onClick={() => go(idx + 1)} disabled={!detDone}>
              {step === "welcome" ? "开始设置" : step === "import" || step === "providers" ? "继续" : "下一步"} <ArrowRight size={16} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function Welcome() {
  return (
    <div className="ob-center">
      <motion.img
        src={logo}
        alt=""
        className="ob-logo"
        initial={{ scale: 0.8, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 22 }}
      />
      <h1 className="ob-title">欢迎使用 Hoshiro</h1>
      <p className="ob-sub">说话，不要打字。按住 Fn 说话，松开后，整理好的文字会直接出现在任何应用的光标处。</p>
      <div className="ob-feature-row">
        <div className="ob-feature">
          <b>口述</b>
          <span>杂乱的想法 → 清晰的文字</span>
        </div>
        <div className="ob-feature">
          <b>翻译</b>
          <span>自然的语言 → 地道的写作</span>
        </div>
        <div className="ob-feature">
          <b>问任何问题</b>
          <span>编辑或询问所选文本</span>
        </div>
      </div>
    </div>
  );
}

function ImportStep({ det, onDetect }: { det: TypelessDetection | null; onDetect: (d: TypelessDetection) => void }) {
  const { toast } = useFeedback();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<TypelessImportResult | null>(null);
  const [quitting, setQuitting] = useState(false);

  const run = async () => {
    setBusy(true);
    try {
      setResult(await api.importTypeless({ settings: true, history: true }));
    } catch (e) {
      toast(`导入失败：${String(e)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ob-center">
      <h1 className="ob-title">从 Typeless 迁移</h1>
      <p className="ob-sub">一键导入快捷键、语言、偏好设置和历史记录，使用习惯完全不变。</p>
      <div className="ob-card">
        <div className="import-found">
          <div>
            <div className="srow-title">已检测到 Typeless</div>
            <div className="srow-desc">找到 {det?.historyCount ?? 0} 条历史记录</div>
          </div>
          {det?.shortcuts && (
            <div className="import-keys">
              {det.shortcuts.dictation.map((s) => (
                <ShortcutKeys key={s} shortcut={s} size="sm" />
              ))}
            </div>
          )}
        </div>
        {det?.running && (
          <div className="banner warn inline">
            <AlertTriangle size={16} />
            <span className="banner-text">Typeless 正在运行，会与 Hoshiro 抢占 Fn 键</span>
            <button
              className="btn btn-secondary btn-sm"
              disabled={quitting}
              onClick={async () => {
                setQuitting(true);
                await api.quitTypeless();
                setTimeout(async () => {
                  setQuitting(false);
                  onDetect(await api.detectTypeless());
                }, 600);
              }}
            >
              退出 Typeless
            </button>
          </div>
        )}
        {result ? (
          <div className="import-result">
            <b>
              <Check size={14} /> 导入完成
            </b>
            <ul>
              <li>{result.settingsImported ? "已导入设置" : "未导入设置"}</li>
              <li>
                历史记录：导入 {result.historyImported} 条，跳过 {result.historySkipped} 条
              </li>
              {result.messages.map((m, i) => (
                <li key={i}>{m}</li>
              ))}
            </ul>
          </div>
        ) : (
          <button className="btn btn-primary btn-lg" onClick={run} disabled={busy}>
            {busy && <Spinner size={15} />} 从 Typeless 导入
          </button>
        )}
        <p className="srow-desc">Typeless 的词典保存在其云端，无法自动导入，可稍后在「词典」中通过 CSV 导入。</p>
      </div>
    </div>
  );
}

function PermStep() {
  return (
    <div className="ob-center">
      <h1 className="ob-title">在您的电脑上设置 Hoshiro</h1>
      <p className="ob-sub">需要以下权限，才能听到您说话、识别快捷键并把文字输入到光标处。</p>
      <div className="ob-card flush">
        <Group>
          <PermissionRows />
        </Group>
      </div>
    </div>
  );
}

function ProvidersStep() {
  return (
    <div className="ob-center top">
      <h1 className="ob-title">连接模型服务</h1>
      <p className="ob-sub">Hoshiro 使用您自己的 API Key，音频和文字只会发送给这些服务商。可以稍后在设置中修改。</p>
      <div className="ob-scroll">
        <AsrForm />
        <LlmForm />
      </div>
    </div>
  );
}

function FnStep() {
  return (
    <div className="ob-center">
      <div className="ob-globe">
        <Keycap>🌐</Keycap>
        <Keycap>Fn</Keycap>
      </div>
      <h1 className="ob-title">设置 Fn 键</h1>
      <p className="ob-sub">
        将 <b>系统设置</b> › <b>键盘</b> › “按下 🌐 键时” 设为 <b>不执行任何操作</b>，这样用 Fn 说话时不会触发表情符号或听写。
      </p>
      <button className="btn btn-secondary btn-lg" onClick={() => api.openKeyboardSettings()}>
        打开键盘设置
      </button>
    </div>
  );
}

function TryStep() {
  const { settings } = useApp();
  const first = settings.shortcuts.dictation[0] ?? "Fn";
  const toggle = settings.shortcuts.trigger === "toggle";
  return (
    <div className="ob-center">
      <h1 className="ob-title">试一试</h1>
      <p className="ob-sub">
        {toggle ? (
          <>点击下面的输入框，然后按一下 <ShortcutKeys shortcut={first} size="sm" /> 开始说话，再按一下即可输入。</>
        ) : (
          <>点击下面的输入框，然后按住 <ShortcutKeys shortcut={first} size="sm" /> 说话，松开即可输入。</>
        )}
      </p>
      <textarea className="textarea ob-textarea" rows={6} placeholder={toggle ? "按一下 fn 开始说话，再按一下即可输入" : "按住 fn 说话，松开即可输入"} autoFocus spellCheck={false} />
      <p className="srow-desc">
        {toggle ? "录音时按 Esc 可取消。" : `轻点 ${first === "Fn" ? "Fn" : "快捷键"} 后再按一次进入免提模式；按 Esc 取消。`}
      </p>
    </div>
  );
}

function DoneStep() {
  const { settings } = useApp();
  const sc = settings.shortcuts;
  return (
    <div className="ob-center">
      <motion.div
        className="ob-check"
        initial={{ scale: 0.6, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 380, damping: 20 }}
      >
        <Check size={34} strokeWidth={3} />
      </motion.div>
      <h1 className="ob-title">一切就绪</h1>
      <p className="ob-sub">只需自然地说话，Hoshiro 会把您的想法变成精炼的文字。</p>
      <div className="ob-card flush">
        {[
          ["口述", sc.dictation],
          ["翻译", sc.translation],
          ["问任何问题", sc.askAnything],
        ].map(([label, list]) => (
          <div className="ob-sc-row" key={label as string}>
            <span>{label as string}</span>
            <div className="chip-line">
              {(list as string[]).map((s) => (
                <ShortcutKeys key={s} shortcut={s} size="sm" />
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
