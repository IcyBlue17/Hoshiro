import { useRef, useState } from "react";
import { ChevronDown, Loader2 } from "lucide-react";
import { api } from "../../lib/ipc";
import type { AsrConfig, LlmProvider } from "../../lib/types";
import { useApp } from "../store";
import { Popover, Segmented, Switch } from "../ui";
import { Group, Row, SecretInput, TestButton, TextField, useTester } from "./shared";

export const MIMO_GLOBAL = "https://api.xiaomimimo.com/v1";
export const MIMO_CN = "https://token-plan-cn.xiaomimimo.com/v1";

type Region = "global" | "cn" | "custom";

export const ASR_NAMES = { mimo: "MiMo", cloudflare: "Cloudflare", azure: "Azure", google: "Google" };
export function asrReady(asr: AsrConfig) {
  if (asr.provider === "google") return !!asr.google.projectId.trim() && !!(asr.google.apiKey.trim() || asr.google.serviceAccountJson.trim());
  if (asr.provider === "cloudflare") return !!asr.cloudflare.apiKey.trim() && !!asr.cloudflare.accountId.trim();
  if (asr.provider === "azure") return !!asr.azure.apiKey.trim() && !!asr.azure.region.trim();
  return !!asr.mimo.apiKey.trim();
}

export function AsrForm() {
  const { settings, update } = useApp();
  return <>
    <Group title="语音识别"><Row title="服务商" stacked>
      <Segmented id="asr-provider" value={settings.asr.provider} onChange={(v) => update((d) => void (d.asr.provider = v))}
        options={Object.entries(ASR_NAMES).map(([value, label]) => ({ value: value as AsrConfig["provider"], label }))} />
    </Row></Group>
    {settings.asr.provider === "mimo" ? <MimoForm /> : <OtherAsrForm key={settings.asr.provider} />}
  </>;
}

function OtherAsrForm() {
  const { settings, update } = useApp();
  const provider = settings.asr.provider;
  const tester = useTester(() => api.testAsr(settings.asr));
  const c = settings.asr;
  return <Group title={`语音识别 · ${ASR_NAMES[provider]}`}>
    {provider === "cloudflare" && <>
      <Row title="账户 ID"><TextField label="Cloudflare 账户 ID" value={c.cloudflare.accountId} onCommit={(v) => update((d) => void (d.asr.cloudflare.accountId = v.trim()))} width={300} /></Row>
      <Row title="API Token" desc="需要 Workers AI 读取/运行权限。"><SecretInput label="Cloudflare API Token" value={c.cloudflare.apiKey} onCommit={(v) => update((d) => void (d.asr.cloudflare.apiKey = v.trim()))} width={300} /></Row>
      <Row title="Whisper 模型"><TextField label="Cloudflare 模型" value={c.cloudflare.model} onCommit={(v) => update((d) => void (d.asr.cloudflare.model = v.trim()))} width={300} /></Row>
    </>}
    {provider === "azure" && <>
      <Row title="Speech Key"><SecretInput label="Azure Speech Key" value={c.azure.apiKey} onCommit={(v) => update((d) => void (d.asr.azure.apiKey = v.trim()))} width={300} /></Row>
      <Row title="区域" desc="须与 Speech 资源区域一致，例如 eastasia。"><TextField label="Azure 区域" value={c.azure.region} onCommit={(v) => update((d) => void (d.asr.azure.region = v.trim()))} width={300} /></Row>
      <Row title="实时识别" desc="边说边发送音频，使用词典提示；多语言从听写语言中取前 4 种。" />
    </>}
    {provider === "google" && <>
      <Row title="项目 ID"><TextField label="Google 项目 ID" value={c.google.projectId} onCommit={(v) => update((d) => void (d.asr.google.projectId = v.trim()))} width={300} /></Row>
      <Row title="区域" desc="区域须支持所选模型，例如 global、us 或 eu。"><TextField label="Google 区域" value={c.google.location} onCommit={(v) => update((d) => void (d.asr.google.location = v.trim()))} width={300} /></Row>
      <Row title="模型"><TextField label="Google 模型" value={c.google.model} onCommit={(v) => update((d) => void (d.asr.google.model = v.trim()))} width={300} /></Row>
      <Row title="API Key" desc="使用服务账户时可留空；项目需已启用 Speech-to-Text。"><SecretInput label="Google API Key" value={c.google.apiKey} onCommit={(v) => update((d) => void (d.asr.google.apiKey = v.trim()))} width={300} /></Row>
      <Row title="服务账户 JSON" desc="与 API Key 二选一，填写后优先使用服务账户；仅保存在本机。" stacked>
        <SecretInput label="Google 服务账户 JSON" value={c.google.serviceAccountJson} onCommit={(v) => update((d) => void (d.asr.google.serviceAccountJson = v.trim()))} width={400} placeholder="粘贴服务账户 JSON" />
      </Row>
      <Row title="实时识别" desc="使用 Google 官方 SDK，支持词典提示；多语言取听写语言中的前 3 种，具体支持取决于所选模型和区域。" />
    </>}
    <Row title="连接测试" desc="发送一小段静音验证配置，可能产生少量用量。"><TestButton tester={tester} disabled={!asrReady(c)} /></Row>
  </Group>;
}

function MimoForm() {
  const { settings, update } = useApp();
  const mimo = settings.asr.mimo;
  const known: Region | null = mimo.baseUrl === MIMO_GLOBAL ? "global" : mimo.baseUrl === MIMO_CN ? "cn" : null;
  const [forceCustom, setForceCustom] = useState(known === null);
  const region: Region = forceCustom ? "custom" : (known ?? "custom");
  const tester = useTester(() => api.testAsr(settings.asr.mimo));

  const setRegion = (r: Region) => {
    if (r === "custom") return setForceCustom(true);
    setForceCustom(false);
    update((d) => void (d.asr.mimo.baseUrl = r === "global" ? MIMO_GLOBAL : MIMO_CN));
  };

  return (
    <Group title="语音识别 · 小米 MiMo">
      <Row title="API Key" desc="用于 MiMo ASR；仅保存在本机。">
        <SecretInput value={mimo.apiKey} onCommit={(v) => update((d) => void (d.asr.mimo.apiKey = v.trim()))} width={300} />
      </Row>
      <Row title="区域" desc={region === "custom" ? "自定义接口地址" : region === "global" ? MIMO_GLOBAL : MIMO_CN}>
        <Segmented
          id="mimo-region"
          size="sm"
          value={region}
          onChange={setRegion}
          options={[
            { value: "global", label: "Global" },
            { value: "cn", label: "中国" },
            { value: "custom", label: "自定义" },
          ]}
        />
      </Row>
      {region === "custom" && (
        <Row title="接口地址">
          <TextField
            value={mimo.baseUrl}
            mono
            width={300}
            placeholder={MIMO_GLOBAL}
            onCommit={(v) => update((d) => void (d.asr.mimo.baseUrl = v.trim()))}
          />
        </Row>
      )}
      <Row title="模型">
        <TextField value={mimo.model} mono width={300} placeholder="mimo-v2.5-asr" onCommit={(v) => update((d) => void (d.asr.mimo.model = v.trim()))} />
      </Row>
      <Row title="连接测试" desc="发送一小段静音音频验证 Key 与网络。">
        <TestButton tester={tester} disabled={!mimo.apiKey} />
      </Row>
    </Group>
  );
}

const PROVIDERS: { value: LlmProvider; label: string; keyHint: string; urlHint: string }[] = [
  { value: "gemini", label: "Gemini", keyHint: "Google AI Studio API Key", urlHint: "留空使用官方地址 generativelanguage.googleapis.com" },
  { value: "anthropic", label: "Anthropic", keyHint: "Anthropic API Key", urlHint: "留空使用官方地址 api.anthropic.com" },
  { value: "openai", label: "OpenAI 兼容", keyHint: "API Key", urlHint: "必填，例如 https://api.openai.com/v1" },
];

export function LlmForm() {
  const { settings, update } = useApp();
  const llm = settings.llm;
  const provider = llm.provider;
  const info = PROVIDERS.find((p) => p.value === provider)!;
  const cfg = llm[provider];
  const off = !llm.enabled;

  return (
    <Group title="改写（AI 润色）">
      <Row title="启用改写" desc="关闭后将直接输入原始转录，不经过 AI 润色、翻译和提问。">
        <Switch checked={llm.enabled} onChange={(v) => update((d) => void (d.llm.enabled = v))} label="启用改写" />
      </Row>
      <Row title="服务商" disabled={off}>
        <Segmented
          id="llm-provider"
          size="sm"
          disabled={off}
          value={provider}
          onChange={(v) => update((d) => void (d.llm.provider = v))}
          options={PROVIDERS.map((p) => ({ value: p.value, label: p.label }))}
        />
      </Row>
      <ProviderFields key={provider} provider={provider} info={info} disabled={off} cfg={cfg} />
      <Row title="超时" desc="超过该时间仍未返回时，改为输入原始转录。" disabled={off}>
        <div className="unit-input">
          <TextField
            type="number"
            width={84}
            disabled={off}
            value={String(llm.timeoutSecs)}
            onCommit={(v) => {
              const n = Math.round(Number(v));
              if (Number.isFinite(n) && n > 0) update((d) => void (d.llm.timeoutSecs = Math.min(120, Math.max(3, n))));
            }}
          />
          <span>秒</span>
        </div>
      </Row>
    </Group>
  );
}

function ProviderFields({
  provider,
  info,
  disabled,
  cfg,
}: {
  provider: LlmProvider;
  info: (typeof PROVIDERS)[number];
  disabled: boolean;
  cfg: { apiKey: string; baseUrl: string; model: string };
}) {
  const { update } = useApp();
  const tester = useTester(() => api.testLlm(provider, cfg));
  const [models, setModels] = useState<string[] | null>(null);
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelErr, setModelErr] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const [modelKey, setModelKey] = useState(0);

  const fetchModels = async () => {
    setOpen(true);
    setLoadingModels(true);
    setModelErr(null);
    try {
      setModels(await api.listModels(provider, cfg));
    } catch (e) {
      setModels(null);
      setModelErr(String(e));
    } finally {
      setLoadingModels(false);
    }
  };

  const set = (field: "apiKey" | "baseUrl" | "model", v: string) =>
    update((d) => void (d.llm[provider][field] = v.trim()));

  return (
    <>
      <Row title="API Key" desc={info.keyHint} disabled={disabled}>
        <SecretInput value={cfg.apiKey} onCommit={(v) => set("apiKey", v)} width={300} disabled={disabled} />
      </Row>
      <Row title="接口地址" desc={info.urlHint} disabled={disabled}>
        <TextField
          value={cfg.baseUrl}
          mono
          width={300}
          disabled={disabled}
          placeholder={provider === "openai" ? "https://api.openai.com/v1" : "默认"}
          onCommit={(v) => set("baseUrl", v)}
        />
      </Row>
      <Row title="模型" disabled={disabled}>
        <div className="model-field">
          <TextField key={modelKey} value={cfg.model} mono width={300} disabled={disabled} onCommit={(v) => set("model", v)} />
          <button ref={btnRef} className="btn btn-secondary btn-sm" disabled={disabled || !cfg.apiKey} onClick={fetchModels}>
            获取模型列表 <ChevronDown size={14} />
          </button>
          <Popover open={open} onClose={() => setOpen(false)} anchorRef={btnRef} align="end" minWidth={260}>
            <div className="menu-list models-list">
              {loadingModels && (
                <div className="menu-note">
                  <Loader2 size={14} className="spin" /> 正在获取…
                </div>
              )}
              {modelErr && <div className="menu-note bad">{modelErr}</div>}
              {!loadingModels && !modelErr && models?.length === 0 && <div className="menu-note">没有可用的模型</div>}
              {models?.map((m) => (
                <button
                  key={m}
                  className={`menu-item ${m === cfg.model ? "selected" : ""}`}
                  onClick={() => {
                    set("model", m);
                    setModelKey((k) => k + 1);
                    setOpen(false);
                  }}
                >
                  <span className="menu-item-label mono">{m}</span>
                </button>
              ))}
            </div>
          </Popover>
        </div>
      </Row>
      <Row title="连接测试" disabled={disabled}>
        <TestButton tester={tester} disabled={disabled || !cfg.apiKey} />
      </Row>
    </>
  );
}
