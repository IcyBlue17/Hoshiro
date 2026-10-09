import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  AlertTriangle,
  ArrowLeftRight,
  Cloud,
  FolderOpen,
  Info,
  Keyboard,
  Languages,
  Mic,
  Shield,
  SlidersHorizontal,
  Sparkles,
  X,
} from "lucide-react";
import { api, isTauri } from "../../lib/ipc";
import type { TypelessDetection, TypelessImportResult } from "../../lib/types";
import { type SettingsCategory, useApp } from "../store";
import { useFeedback } from "../feedback";
import { usePoll } from "../hooks";
import { Checkbox, Modal, Segmented, ShortcutKeys, Spinner, Switch } from "../ui";
import { Group, Row } from "./shared";
import { ShortcutSettings } from "./Shortcuts";
import { MicPicker } from "./MicPicker";
import { LanguageSettings } from "./Language";
import { AsrForm, LlmForm } from "./Providers";
import { PermissionRows } from "./permissions";
import { Personalization } from "./Personalization";
import { useDebouncedField } from "../ui";

const CATEGORIES: { key: SettingsCategory; label: string; icon: typeof Mic }[] = [
  { key: "general", label: "常规", icon: SlidersHorizontal },
  { key: "shortcuts", label: "快捷键", icon: Keyboard },
  { key: "audio", label: "音频", icon: Mic },
  { key: "language", label: "语言", icon: Languages },
  { key: "providers", label: "模型服务", icon: Cloud },
  { key: "style", label: "个性化", icon: Sparkles },
  { key: "data", label: "数据与迁移", icon: ArrowLeftRight },
  { key: "permissions", label: "权限", icon: Shield },
  { key: "about", label: "关于", icon: Info },
];

export function SettingsModal({
  open,
  initialCategory,
  onClose,
}: {
  open: boolean;
  initialCategory: SettingsCategory;
  onClose: () => void;
}) {
  const [cat, setCat] = useState<SettingsCategory>(initialCategory);
  useEffect(() => {
    if (open) setCat(initialCategory);
  }, [open, initialCategory]);
  const current = CATEGORIES.find((c) => c.key === cat)!;

  return (
    <Modal open={open} onClose={onClose} width={860} className="settings-modal">
      <div className="settings-layout">
        <div className="settings-nav">
          <h2 className="settings-heading">设置</h2>
          {CATEGORIES.map(({ key, label, icon: Icon }) => (
            <button key={key} className={`snav-item ${cat === key ? "active" : ""}`} onClick={() => setCat(key)}>
              {cat === key && (
                <motion.span layoutId="snav-pill" className="snav-pill" transition={{ type: "spring", stiffness: 520, damping: 40 }} />
              )}
              <Icon size={17} strokeWidth={1.9} />
              <span>{label}</span>
            </button>
          ))}
        </div>
        <div className="settings-content">
          <div className="settings-content-head">
            <h2>{current.label}</h2>
            <button className="icon-btn" onClick={onClose} aria-label="关闭">
              <X size={18} />
            </button>
          </div>
          <div className="settings-scroll">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={cat}
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.12 }}
              >
                {cat === "general" && <GeneralSection />}
                {cat === "shortcuts" && <ShortcutSettings />}
                {cat === "audio" && <AudioSection />}
                {cat === "language" && <LanguageSettings />}
                {cat === "providers" && (
                  <>
                    <AsrForm />
                    <LlmForm />
                  </>
                )}
                {cat === "style" && <StyleSection />}
                {cat === "data" && <DataSection />}
                {cat === "permissions" && <PermissionsSection />}
                {cat === "about" && <AboutSection />}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>
    </Modal>
  );
}

function GeneralSection() {
  const { settings, update } = useApp();
  return (
    <Group>
      <Row title="外观" desc="选择明亮模式或黑暗模式。">
        <Segmented
          id="appearance"
          size="sm"
          value={settings.appearance}
          onChange={(v) => update((d) => void (d.appearance = v))}
          options={[
            { value: "system", label: "系统" },
            { value: "light", label: "明亮" },
            { value: "dark", label: "黑暗" },
          ]}
        />
      </Row>
      <Row title="在 Dock 中显示应用" desc="在您的 Mac Dock 中显示 Hoshiro 以便快速访问。">
        <Switch checked={settings.showInDock} onChange={(v) => update((d) => void (d.showInDock = v))} label="在 Dock 中显示应用" />
      </Row>
      <Row title="开机启动" desc="登录 Mac 时自动启动 Hoshiro。">
        <Switch checked={settings.launchAtLogin} onChange={(v) => update((d) => void (d.launchAtLogin = v))} label="开机启动" />
      </Row>
      <Row title="在桌面上显示悬浮条" desc="未录音时在屏幕底部显示一个小小的待机条，点击即可开始口述。">
        <Switch checked={settings.showBarOnDesktop} onChange={(v) => update((d) => void (d.showBarOnDesktop = v))} label="在桌面上显示悬浮条" />
      </Row>
    </Group>
  );
}

function AudioSection() {
  const { settings, update } = useApp();
  const [open, setOpen] = useState(false);
  const [devices, setDevices] = useState<{ id: string; name: string }[]>([]);
  useEffect(() => {
    api.listMicrophones().then(setDevices).catch(() => {});
  }, [open]);
  const cur = settings.microphoneDeviceId;
  const name = cur === null ? "自动检测" : (devices.find((d) => d.id === cur)?.name ?? "未知设备");
  return (
    <>
      <Group>
        <Row title="麦克风" desc="选择您首选的麦克风，以便 Hoshiro 捕捉您的声音。">
          <button className="btn btn-secondary" onClick={() => setOpen(true)}>
            <Mic size={15} /> <span className="btn-ellipsis">{name}</span>
          </button>
        </Row>
        <Row title="交互声音" desc="为开始/停止等关键操作播放声音。">
          <Switch checked={settings.interactionSounds} onChange={(v) => update((d) => void (d.interactionSounds = v))} label="交互声音" />
        </Row>
        <Row title="语音输入时静音" desc="在语音输入时自动静音其他活动音频。">
          <Switch checked={settings.muteWhileDictating} onChange={(v) => update((d) => void (d.muteWhileDictating = v))} label="语音输入时静音" />
        </Row>
      </Group>
      <MicPicker
        open={open}
        value={cur}
        onClose={() => setOpen(false)}
        onSave={(id) => update((d) => void (d.microphoneDeviceId = id))}
      />
    </>
  );
}

function StyleSection() {
  const { settings, update } = useApp();
  const [text, setText] = useDebouncedField(settings.style.customInstructions, (v) =>
    update((d) => void (d.style.customInstructions = v)),
  );
  return (
    <>
      <Personalization />
      <Group>
        <Row title="智能格式化" desc="将口述的列举转成列表，长篇内容自动分段，并把口述的符号、邮箱、网址写成书面形式。">
          <Switch checked={settings.style.smartFormatting} onChange={(v) => update((d) => void (d.style.smartFormatting = v))} label="智能格式化" />
        </Row>
        <Row title="自动添加到词典" desc="在您修正已输入的文字后，自动把更正后的专有名词学习到词典中。">
          <Switch checked={settings.style.autoAddDictionary} onChange={(v) => update((d) => void (d.style.autoAddDictionary = v))} label="自动添加到词典" />
        </Row>
      </Group>
      <Group>
        <Row title="自定义指令" desc="附加到改写提示词末尾，例如“用正式语气”“把英文标点改成中文标点”。需要开启改写。" stacked>
          <textarea
            className="textarea"
            rows={5}
            value={text}
            spellCheck={false}
            placeholder="例如：保持简洁；称呼对方为“您”；不要在句末加句号。"
            onChange={(e) => setText(e.target.value)}
          />
        </Row>
      </Group>
    </>
  );
}

function DataSection() {
  const { toast } = useFeedback();
  const [det, setDet] = useState<TypelessDetection | null>(null);
  const [optSettings, setOptSettings] = useState(true);
  const [optHistory, setOptHistory] = useState(true);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<TypelessImportResult | null>(null);
  const [quitting, setQuitting] = useState(false);

  const detect = useCallback(() => {
    api.detectTypeless().then(setDet).catch(() => {});
  }, []);
  usePoll(detect, 4000);

  const doImport = async () => {
    setBusy(true);
    try {
      const r = await api.importTypeless({ settings: optSettings, history: optHistory });
      setResult(r);
    } catch (e) {
      toast(`导入失败：${String(e)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const openFolder = async () => {
    if (!isTauri) return toast("仅在桌面应用中可用", "info");
    try {
      const { appDataDir, join } = await import("@tauri-apps/api/path");
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      const dir = await appDataDir();
      try {
        await revealItemInDir(await join(dir, "settings.json"));
      } catch {
        await revealItemInDir(dir);
      }
    } catch (e) {
      toast(`无法打开数据文件夹：${String(e)}`, "error");
    }
  };

  const found = det?.installed && det.dataFound;

  return (
    <>
      <Group title="从 Typeless 导入">
        <div className="import-card">
          {!det ? (
            <div className="muted">
              <Spinner /> 正在检测…
            </div>
          ) : !found ? (
            <div className="import-empty">
              <b>未检测到 Typeless 数据</b>
              <span>未在 ~/Library/Application Support/Typeless 中找到设置或历史记录。</span>
            </div>
          ) : (
            <>
              <div className="import-found">
                <div>
                  <div className="srow-title">已检测到 Typeless</div>
                  <div className="srow-desc">
                    {det.running ? "正在运行 · " : ""}找到 {det.historyCount} 条历史记录
                  </div>
                </div>
                {det.shortcuts && (
                  <div className="import-keys">
                    <span className="muted">口述快捷键</span>
                    {det.shortcuts.dictation.map((s) => (
                      <ShortcutKeys key={s} shortcut={s} size="sm" />
                    ))}
                  </div>
                )}
              </div>
              {det.running && (
                <div className="banner warn inline">
                  <AlertTriangle size={16} />
                  <span className="banner-text">Typeless 正在运行，会与 Hoshiro 抢占 Fn 键</span>
                  <button
                    className="btn btn-secondary btn-sm"
                    disabled={quitting}
                    onClick={async () => {
                      setQuitting(true);
                      await api.quitTypeless();
                      setTimeout(() => {
                        setQuitting(false);
                        detect();
                      }, 600);
                    }}
                  >
                    退出 Typeless
                  </button>
                </div>
              )}
              <label className="check-row">
                <Checkbox checked={optSettings} onChange={setOptSettings} />
                <span>设置（快捷键、语言、外观等偏好）</span>
              </label>
              <label className="check-row">
                <Checkbox checked={optHistory} onChange={setOptHistory} />
                <span>历史记录（不含音频）</span>
              </label>
              <div className="import-actions">
                <button className="btn btn-primary" onClick={doImport} disabled={busy || (!optSettings && !optHistory)}>
                  {busy && <Spinner size={14} />} 从 Typeless 导入
                </button>
              </div>
              {result && (
                <div className="import-result">
                  <b>导入完成</b>
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
              )}
            </>
          )}
          <p className="srow-desc import-note">
            Typeless 的词典保存在其云端，无法自动导入；可整理成单列 CSV 后在「词典」页面使用「导入 CSV」。
          </p>
        </div>
      </Group>
      <Group title="本地数据">
        <Row title="数据文件夹" desc="设置、历史记录与录音都存放在此设备的应用数据目录中。">
          <button className="btn btn-secondary" onClick={openFolder}>
            <FolderOpen size={15} /> 打开数据文件夹
          </button>
        </Row>
      </Group>
    </>
  );
}

function PermissionsSection() {
  return (
    <>
      <Group>
        <PermissionRows />
      </Group>
      <Group>
        <Row title="Fn 键" desc="请将 系统设置 › 键盘 › “按下 🌐 键时” 设为 “不执行任何操作”，以免 Fn 触发系统功能。">
          <button className="btn btn-secondary" onClick={() => api.openKeyboardSettings()}>
            打开键盘设置
          </button>
        </Row>
      </Group>
    </>
  );
}

function AboutSection() {
  const { version } = useApp();
  const { toast } = useFeedback();
  return (
    <Group>
      <Row title="版本" desc={`Hoshiro v${version || "0.1.0"}`}>
        <button className="btn btn-secondary" onClick={() => toast("暂不支持自动检查更新", "info")}>
          检查更新
        </button>
      </Row>
      <Row title="隐私" desc="历史记录、词典和 API Key 仅保存在此设备上；音频与文字只会发送给您自行配置的语音识别与改写服务商。" />
    </Group>
  );
}
