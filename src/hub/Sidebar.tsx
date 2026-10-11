import { ASR_NAMES, asrReady as isAsrReady, llmReady as isLlmReady } from "./settings/Providers";
import { useState } from "react";
import { motion } from "motion/react";
import { BarChart3, AlertTriangle, BookOpen, CircleHelp, History, House, Settings as SettingsIcon } from "lucide-react";
import logo from "./assets/logo.png";
import { type Page, useApp } from "./store";
import { Modal, ModalHeader, ShortcutKeys } from "./ui";
import type { LlmProvider } from "../lib/types";
import { useUpdate } from "./update";
import { DEFAULT_SHORTCUTS } from "./format";

const NAV: { page: Page; label: string; icon: typeof House }[] = [
  { page: "home", label: "首页", icon: House },
  { page: "history", label: "历史记录", icon: History },
  { page: "dictionary", label: "词典", icon: BookOpen },
  { page: "statistics", label: "使用统计", icon: BarChart3 },
];

const LLM_NAMES: Record<LlmProvider, string> = { gemini: "Gemini", anthropic: "Claude", openai: "OpenAI 兼容", cloudflare: "Cloudflare" };

export function Sidebar({ page }: { page: Page }) {
  const { navigate, openSettings, settings } = useApp();
  const [help, setHelp] = useState(false);
  const upd = useUpdate();

  const asrReady = isAsrReady(settings.asr);
  const llm = settings.llm;
  const llmReady = isLlmReady(settings);
  const missing: string[] = [];
  if (!asrReady) missing.push(ASR_NAMES[settings.asr.provider]);
  if (!llmReady) missing.push(LLM_NAMES[llm.provider]);

  return (
    <aside className="sidebar">
      <div className="sidebar-top" data-tauri-drag-region />
      <div className="brand">
        <img src={logo} alt="" className="brand-logo" draggable={false} />
        <span className="brand-name">Hoshiro</span>
      </div>

      <nav className="nav">
        {NAV.map(({ page: p, label, icon: Icon }) => (
          <button key={p} className={`nav-item ${page === p ? "active" : ""}`} onClick={() => navigate(p)}>
            {page === p && (
              <motion.span
                layoutId="nav-pill"
                className="nav-pill"
                transition={{ type: "spring", stiffness: 520, damping: 40 }}
              />
            )}
            <Icon size={19} strokeWidth={1.9} className="nav-icon" />
            <span className="nav-label">{label}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar-spacer" />

      <button
        className={`provider-card ${missing.length ? "warn" : ""}`}
        onClick={() => openSettings("providers")}
        title="模型服务设置"
      >
        <div className="provider-line">
          <span className="provider-key">语音识别</span>
          <span className="provider-val">
            <i className={`dot ${asrReady ? "ok" : "bad"}`} />
            {ASR_NAMES[settings.asr.provider]}
          </span>
        </div>
        <div className="provider-line">
          <span className="provider-key">改写</span>
          <span className="provider-val">
            {llm.enabled ? (
              <>
                <i className={`dot ${llmReady ? "ok" : "bad"}`} />
                {LLM_NAMES[llm.provider]}
              </>
            ) : (
              <>
                <i className="dot off" />已关闭
              </>
            )}
          </span>
        </div>
        {missing.length > 0 && (
          <div className="provider-warn">
            <AlertTriangle size={13} />
            <span>缺少 {missing.join("、")} 的 API Key，点击配置</span>
          </div>
        )}
      </button>

      {(upd.phase === "available" || upd.phase === "installing") && (
        <button className="update-pill" onClick={() => openSettings("about")}>
          <i className="update-dot" />
          {upd.phase === "installing" ? "正在更新…" : `新版本 ${upd.info?.version}`}
        </button>
      )}

      <div className="sidebar-footer">
        <button className="icon-btn" onClick={() => openSettings()} aria-label="设置" title="设置">
          <SettingsIcon size={18} />
        </button>
        <button className="icon-btn" onClick={() => setHelp(true)} aria-label="帮助" title="帮助">
          <CircleHelp size={18} />
        </button>
      </div>

      <Modal open={help} onClose={() => setHelp(false)} width={440}>
        <ModalHeader title="使用帮助" onClose={() => setHelp(false)} />
        <div className="help-list">
          {[
            ["口述", settings.shortcuts.dictation[0] ?? DEFAULT_SHORTCUTS.dictation[0], settings.shortcuts.trigger === "toggle" ? "按一下开始说话，再按一下结束并自动输入。" : "按住说话，松开后自动输入；双击进入免提模式。"],
            ["随便问", settings.shortcuts.askAnything[0] ?? DEFAULT_SHORTCUTS.askAnything[0], "对选中的文字下指令，或直接提问。"],
            ["翻译", settings.shortcuts.translation[0] ?? DEFAULT_SHORTCUTS.translation[0], "把你说的话翻译成目标语言后输入。"],
            ["粘贴上一次转录", settings.shortcuts.pasteLastTranscript[0] ?? DEFAULT_SHORTCUTS.pasteLastTranscript[0], "再次粘贴最近一次的结果。"],
            ["取消", "Esc", "录音或处理过程中按下即可取消。"],
          ].map(([t, k, d]) => (
            <div className="help-row" key={t}>
              <div>
                <div className="help-title">{t}</div>
                <div className="help-desc">{d}</div>
              </div>
              <ShortcutKeys shortcut={k} size="sm" />
            </div>
          ))}
        </div>
      </Modal>
    </aside>
  );
}
