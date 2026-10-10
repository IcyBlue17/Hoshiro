import { useRef, useState } from "react";
import { ChevronDown, Loader2 } from "lucide-react";
import { api } from "../../lib/ipc";
import type { AsrConfig, LlmEndpointConfig, LlmProvider, ReasoningEffort, Settings } from "../../lib/types";
import { useApp } from "../store";
import { Popover, Segmented, Select, Switch } from "../ui";
import { Group, Row, SecretInput, TestButton, TextField, useTester } from "./shared";

/** MiMo pay-as-you-go endpoint. */
export const MIMO_PAYG = "https://api.xiaomimimo.com/v1";
/** MiMo Token Plan (subscription) endpoint. */
export const MIMO_TOKEN_PLAN = "https://token-plan-cn.xiaomimimo.com/v1";

type Plan = "payg" | "plan" | "custom";

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

/** Cloudflare Workers AI speech models. Edit labels here. */
export const CLOUDFLARE_ASR_MODELS: { value: string; label: string }[] = [
  { value: "@cf/openai/whisper-large-v3-turbo", label: "Whisper Large v3 Turbo（推荐）" },
  { value: "@cf/openai/whisper", label: "Whisper 旧版（较快，偶尔繁体）" },
  { value: "@cf/deepgram/nova-3", label: "Deepgram Nova-3（仅英文）" },
];
/** Cloudflare Workers AI text models for rewriting (ids from developers.cloudflare.com/workers-ai/models). Edit labels here. */
export const CLOUDFLARE_LLM_MODELS: { value: string; label: string }[] = [
  { value: "@cf/meta/llama-3.3-70b-instruct-fp8-fast", label: "Llama 3.3 70B Fast（推荐，速度快）" },
  { value: "@cf/mistralai/mistral-small-3.1-24b-instruct", label: "Mistral Small 3.1 24B（多语言）" },
  { value: "@cf/zai-org/glm-4.7-flash", label: "GLM-4.7 Flash（中文好，可能先思考）" },
  { value: "@cf/qwen/qwen3-30b-a3b-fp8", label: "Qwen3 30B-A3B（中文好，可能先思考）" },
  { value: "@cf/openai/gpt-oss-20b", label: "GPT-OSS 20B" },
  { value: "@cf/openai/gpt-oss-120b", label: "GPT-OSS 120B（质量高，较慢）" },
  { value: "@cf/google/gemma-4-26b-a4b-it", label: "Gemma 4 26B" },
];
const CUSTOM = "__custom__";

function CloudflareModel({ models, model, label, onCommit }: {
  models: { value: string; label: string }[];
  model: string;
  label: string;
  onCommit: (model: string) => void;
}) {
  const isPreset = models.some((m) => m.value === model);
  const [custom, setCustom] = useState(!isPreset);
  const selected = custom || !isPreset ? CUSTOM : model;
  const commit = (v: string) => onCommit(v.trim());
  return (
    <div className="cf-model">
      <Select
        value={selected}
        width={300}
        align="end"
        options={[...models, { value: CUSTOM, label: "自定义…" }]}
        onChange={(v) => {
          if (v === CUSTOM) return setCustom(true);
          setCustom(false);
          commit(v);
        }}
      />
      {selected === CUSTOM && (
        <TextField label={label} value={model} mono width={300} placeholder="@cf/…" onCommit={commit} />
      )}
    </div>
  );
}

function OtherAsrForm() {
  const { settings, update } = useApp();
  const provider = settings.asr.provider;
  const tester = useTester(() => api.testAsr(settings.asr));
  const c = settings.asr;
  return <Group title={`语音识别 · ${ASR_NAMES[provider]}${provider === "azure" || provider === "google" ? " · 流式" : ""}`}>
    {provider === "cloudflare" && <>
      <Row title="账户 ID"><TextField label="Cloudflare 账户 ID" value={c.cloudflare.accountId} onCommit={(v) => update((d) => void (d.asr.cloudflare.accountId = v.trim()))} width={300} /></Row>
      <Row title="API Token" desc="需要 Workers AI 读取/运行权限。"><SecretInput label="Cloudflare API Token" value={c.cloudflare.apiKey} onCommit={(v) => update((d) => void (d.asr.cloudflare.apiKey = v.trim()))} width={300} /></Row>
      <Row title="识别模型">
        <CloudflareModel models={CLOUDFLARE_ASR_MODELS} model={c.cloudflare.model} label="自定义 Cloudflare 模型" onCommit={(v) => update((d) => void (d.asr.cloudflare.model = v))} />
      </Row>
    </>}
    {provider === "azure" && <>
      <Row title="Speech Key"><SecretInput label="Azure Speech Key" value={c.azure.apiKey} onCommit={(v) => update((d) => void (d.asr.azure.apiKey = v.trim()))} width={300} /></Row>
      <Row title="区域" desc="须与 Speech 资源区域一致，例如 eastasia。"><TextField label="Azure 区域" value={c.azure.region} onCommit={(v) => update((d) => void (d.asr.azure.region = v.trim()))} width={300} /></Row>
    </>}
    {provider === "google" && <>
      <Row title="项目 ID"><TextField label="Google 项目 ID" value={c.google.projectId} onCommit={(v) => update((d) => void (d.asr.google.projectId = v.trim()))} width={300} /></Row>
      <Row title="区域" desc="区域须支持所选模型，例如 global、us 或 eu。"><TextField label="Google 区域" value={c.google.location} onCommit={(v) => update((d) => void (d.asr.google.location = v.trim()))} width={300} /></Row>
      <Row title="模型"><TextField label="Google 模型" value={c.google.model} onCommit={(v) => update((d) => void (d.asr.google.model = v.trim()))} width={300} /></Row>
      <Row title="API Key" desc="使用服务账户时可留空；项目需已启用 Speech-to-Text。"><SecretInput label="Google API Key" value={c.google.apiKey} onCommit={(v) => update((d) => void (d.asr.google.apiKey = v.trim()))} width={300} /></Row>
      <Row title="服务账户 JSON" desc="与 API Key 二选一，优先使用服务账户。" stacked>
        <SecretInput label="Google 服务账户 JSON" value={c.google.serviceAccountJson} onCommit={(v) => update((d) => void (d.asr.google.serviceAccountJson = v.trim()))} width={400} placeholder="粘贴服务账户 JSON" />
      </Row>
    </>}
    <Row title="连接测试" desc="会产生极少量用量。"><TestButton tester={tester} disabled={!asrReady(c)} /></Row>
  </Group>;
}

function MimoForm() {
  const { settings, update } = useApp();
  const mimo = settings.asr.mimo;
  const known: Plan | null = mimo.baseUrl === MIMO_PAYG ? "payg" : mimo.baseUrl === MIMO_TOKEN_PLAN ? "plan" : null;
  const [forceCustom, setForceCustom] = useState(known === null);
  const plan: Plan = forceCustom ? "custom" : (known ?? "custom");
  const tester = useTester(() => api.testAsr(settings.asr.mimo));

  const setPlan = (r: Plan) => {
    if (r === "custom") return setForceCustom(true);
    setForceCustom(false);
    update((d) => void (d.asr.mimo.baseUrl = r === "payg" ? MIMO_PAYG : MIMO_TOKEN_PLAN));
  };

  return (
    <Group title="语音识别 · 小米 MiMo">
      <Row title="API Key" desc="用于 MiMo ASR；仅保存在本机。按量付费使用 sk- 开头的 Key，Token Plan 使用 tp- 开头的 Key，两者不能混用。">
        <SecretInput value={mimo.apiKey} onCommit={(v) => update((d) => void (d.asr.mimo.apiKey = v.trim()))} width={300} />
      </Row>
      <Row title="套餐" desc={plan === "custom" ? "自定义接口地址" : plan === "payg" ? MIMO_PAYG : MIMO_TOKEN_PLAN}>
        <Segmented
          id="mimo-plan"
          size="sm"
          value={plan}
          onChange={setPlan}
          options={[
            { value: "payg", label: "按量付费" },
            { value: "plan", label: "Token Plan" },
            { value: "custom", label: "自定义" },
          ]}
        />
      </Row>
      {plan === "custom" && (
        <Row title="接口地址">
          <TextField
            value={mimo.baseUrl}
            mono
            width={300}
            placeholder={MIMO_PAYG}
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

export const PROVIDERS: { value: LlmProvider; label: string; keyHint: string; urlHint: string }[] = [
  { value: "gemini", label: "Gemini", keyHint: "Google AI Studio API Key", urlHint: "留空使用官方地址 generativelanguage.googleapis.com" },
  { value: "anthropic", label: "Anthropic", keyHint: "Anthropic API Key", urlHint: "留空使用官方地址 api.anthropic.com" },
  { value: "openai", label: "OpenAI 兼容", keyHint: "API Key", urlHint: "必填，例如 https://api.openai.com/v1" },
  { value: "cloudflare", label: "Cloudflare", keyHint: "", urlHint: "" },
];

/**
 * Common OpenAI-compatible services for the 「OpenAI 兼容」 provider. Picking one fills the Base URL and (when the
 * model is empty or still the previous preset's default) the model. `model: ""` = no verified default, pick one via
 * 「获取模型列表」. Edit this table to add or fix services.
 */
export const OPENAI_PRESETS: { id: string; label: string; baseUrl: string; model: string; note?: string; keyless?: boolean }[] = [
  { id: "openai", label: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-6-luna" },
  { id: "vercel", label: "Vercel AI Gateway", baseUrl: "https://ai-gateway.vercel.sh/v1", model: "", note: "模型名形如 provider/model，例如 anthropic/claude-opus-5。" },
  { id: "openrouter", label: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "", note: "模型名形如 provider/model，可点「获取模型列表」选择。" },
  { id: "deepseek", label: "DeepSeek 深度求索", baseUrl: "https://api.deepseek.com", model: "deepseek-flash", note: "默认开启思考，会明显变慢。" },
  { id: "bigmodel", label: "智谱 GLM · 中国（bigmodel.cn）", baseUrl: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4.7-flash" },
  { id: "zai", label: "智谱 GLM · 国际（z.ai）", baseUrl: "https://api.z.ai/api/paas/v4", model: "glm-4.7-flash" },
  { id: "moonshot-cn", label: "Kimi · 中国（moonshot.cn）", baseUrl: "https://api.moonshot.cn/v1", model: "kimi-k2.6", note: "Kimi 的 API Key 区分中国与国际站，不能混用。" },
  { id: "moonshot", label: "Kimi · 国际（moonshot.ai）", baseUrl: "https://api.moonshot.ai/v1", model: "kimi-k2.6", note: "Kimi 的 API Key 区分中国与国际站，不能混用。" },
  { id: "dashscope-cn", label: "阿里云百炼 · 中国（北京）", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-flash", note: "API Key 与地域绑定，不能混用。" },
  { id: "dashscope-intl", label: "阿里云百炼 · 国际（新加坡）", baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", model: "qwen-flash", note: "API Key 与地域绑定，不能混用。" },
  { id: "ark", label: "火山方舟（豆包）", baseUrl: "https://ark.cn-beijing.volces.com/api/v3", model: "", note: "模型填写豆包模型名或接入点 ID（ep-…）。" },
  { id: "siliconflow", label: "硅基流动 SiliconFlow", baseUrl: "https://api.siliconflow.cn/v1", model: "deepseek-ai/DeepSeek-V4-Flash" },
  { id: "groq", label: "Groq", baseUrl: "https://api.groq.com/openai/v1", model: "openai/gpt-oss-120b" },
  { id: "xai", label: "xAI", baseUrl: "https://api.x.ai/v1", model: "grok-4-1-fast-non-reasoning" },
  { id: "mistral", label: "Mistral", baseUrl: "https://api.mistral.ai/v1", model: "mistral-small-latest" },
  { id: "ollama", label: "Ollama（本地）", baseUrl: "http://localhost:11434/v1", model: "", keyless: true, note: "无需真实 Key，已自动填入占位值；模型填写已拉取的名称，如 qwen3:8b。" },
];
const sameUrl = (a: string, b: string) => a.trim().replace(/\/+$/, "").toLowerCase() === b.trim().replace(/\/+$/, "").toLowerCase();
const presetOf = (baseUrl: string) => OPENAI_PRESETS.find((p) => sameUrl(p.baseUrl, baseUrl));

/** The endpoint a provider will actually use. Cloudflare has no Base URL and borrows the speech-recognition credentials when its own are empty. */
export function llmEndpoint(s: Settings, provider: LlmProvider): LlmEndpointConfig {
  if (provider !== "cloudflare") return s.llm[provider];
  const c = s.llm.cloudflare;
  return { apiKey: c.apiKey.trim() || s.asr.cloudflare.apiKey.trim(), baseUrl: "", model: c.model };
}
export function llmReady(s: Settings) {
  const { llm } = s;
  if (!llm.enabled) return true;
  if (llm.provider === "cloudflare") return !!llmEndpoint(s, "cloudflare").apiKey && !!(llm.cloudflare.accountId.trim() || s.asr.cloudflare.accountId.trim());
  return !!llm[llm.provider].apiKey.trim();
}

export const EFFORT_OPTIONS: { value: ReasoningEffort; label: string }[] = [
  { value: "off", label: "关闭" },
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
];

export function LlmForm() {
  const { settings, update } = useApp();
  const llm = settings.llm;
  const provider = llm.provider;
  const info = PROVIDERS.find((p) => p.value === provider)!;
  const off = !llm.enabled;

  return (
    <Group title="改写（AI 润色）">
      <Row title="启用改写" desc="关闭后直接输入原始转录。">
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
      {provider === "cloudflare" ? <CloudflareLlmFields disabled={off} /> : <ProviderFields key={provider} provider={provider} info={info} disabled={off} cfg={llm[provider]} />}
      <Row
        title="思考强度"
        disabled={off}
        desc={
          <>
            推荐关闭。开启后会明显变慢，效果提升有限。
            {llm.reasoningEffort !== "off" && <div className="effort-warn">不推荐：会明显变慢</div>}
          </>
        }
      >
        <Segmented
          id="llm-effort"
          size="sm"
          disabled={off}
          value={llm.reasoningEffort}
          onChange={(v) => update((d) => void (d.llm.reasoningEffort = v))}
          options={EFFORT_OPTIONS}
        />
      </Row>
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
  provider: Exclude<LlmProvider, "cloudflare">;
  info: (typeof PROVIDERS)[number];
  disabled: boolean;
  cfg: { apiKey: string; baseUrl: string; model: string };
}) {
  const { update } = useApp();
  const tester = useTester(() => api.testLlm(provider, cfg));
  const set = (field: "apiKey" | "baseUrl" | "model", v: string) =>
    update((d) => void (d.llm[provider][field] = v.trim()));
  const preset = provider === "openai" ? presetOf(cfg.baseUrl) : undefined;
  const [forceCustom, setForceCustom] = useState(provider === "openai" && !preset);
  const choice = forceCustom || !preset ? CUSTOM : preset.id;
  const pick = (id: string) => {
    if (id === CUSTOM) return setForceCustom(true);
    const next = OPENAI_PRESETS.find((p) => p.id === id)!;
    setForceCustom(false);
    update((d) => {
      const ep = d.llm.openai;
      ep.baseUrl = next.baseUrl;
      // Replace the model only when it is empty or still a default we filled in before.
      const old = presetOf(cfg.baseUrl);
      if (!ep.model.trim() || ep.model === old?.model || ep.model === "gpt-4.1-mini") ep.model = next.model;
      if (next.keyless && !ep.apiKey.trim()) ep.apiKey = "ollama";
    });
  };

  return (
    <>
      <Row title="API Key" desc={info.keyHint} disabled={disabled}>
        <SecretInput value={cfg.apiKey} onCommit={(v) => set("apiKey", v)} width={300} disabled={disabled} />
      </Row>
      {provider === "openai" && (
        <Row title="平台" desc={choice === CUSTOM ? "其他 OpenAI 兼容服务，请手动填写接口地址。" : (preset?.note ?? preset?.baseUrl)} disabled={disabled}>
          <Select
            value={choice}
            width={300}
            align="end"
            disabled={disabled}
            options={[...OPENAI_PRESETS.map((p) => ({ value: p.id, label: p.label })), { value: CUSTOM, label: "自定义" }]}
            onChange={pick}
          />
        </Row>
      )}
      {(provider !== "openai" || choice === CUSTOM) && (
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
      )}
      <Row title="模型" disabled={disabled}>
        <ModelInput provider={provider} cfg={cfg} value={cfg.model} disabled={disabled} onPick={(m) => set("model", m)} />
      </Row>
      <Row title="连接测试" disabled={disabled}>
        <TestButton tester={tester} disabled={disabled || !cfg.apiKey} />
      </Row>
    </>
  );
}

function CloudflareLlmFields({ disabled }: { disabled: boolean }) {
  const { settings, update } = useApp();
  const c = settings.llm.cloudflare;
  const asr = settings.asr.cloudflare;
  const tester = useTester(() => api.testLlm("cloudflare", llmEndpoint(settings, "cloudflare")));
  const sharedId = !c.accountId.trim() && !!asr.accountId.trim();
  const sharedKey = !c.apiKey.trim() && !!asr.apiKey.trim();
  const hint = (shared: boolean, text: string) => (shared ? "留空，正沿用语音识别中的 Cloudflare 设置。" : text);
  return (
    <>
      <Row title="账户 ID" desc={hint(sharedId, "留空则沿用语音识别中的 Cloudflare 账户。")} disabled={disabled}>
        <TextField label="Cloudflare 账户 ID" value={c.accountId} onCommit={(v) => update((d) => void (d.llm.cloudflare.accountId = v.trim()))} width={300} disabled={disabled} />
      </Row>
      <Row title="API Token" desc={hint(sharedKey, "需要 Workers AI 权限；留空则沿用语音识别中的 Token。")} disabled={disabled}>
        <SecretInput label="Cloudflare API Token" value={c.apiKey} onCommit={(v) => update((d) => void (d.llm.cloudflare.apiKey = v.trim()))} width={300} disabled={disabled} />
      </Row>
      <Row title="模型" disabled={disabled}>
        <CloudflareModel models={CLOUDFLARE_LLM_MODELS} model={c.model} label="自定义 Cloudflare 模型" onCommit={(v) => update((d) => void (d.llm.cloudflare.model = v))} />
      </Row>
      <Row title="连接测试" disabled={disabled}>
        <TestButton tester={tester} disabled={disabled || !llmReady({ ...settings, llm: { ...settings.llm, enabled: true, provider: "cloudflare" } })} />
      </Row>
    </>
  );
}

/** Model text field + 「获取模型列表」 dropdown. Credentials/base URL come from `cfg`. */
export function ModelInput({
  provider,
  cfg,
  value,
  onPick,
  disabled,
  placeholder,
}: {
  provider: LlmProvider;
  cfg: LlmEndpointConfig;
  value: string;
  onPick: (model: string) => void;
  disabled?: boolean;
  placeholder?: string;
}) {
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

  return (
    <div className="model-field">
      <TextField key={modelKey} value={value} mono width={300} disabled={disabled} placeholder={placeholder} onCommit={onPick} />
      {provider !== "cloudflare" && (
        <button ref={btnRef} className="btn btn-secondary btn-sm" disabled={disabled || !cfg.apiKey} onClick={fetchModels}>
          获取模型列表 <ChevronDown size={14} />
        </button>
      )}
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
              className={`menu-item ${m === value ? "selected" : ""}`}
              onClick={() => {
                onPick(m);
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
  );
}
