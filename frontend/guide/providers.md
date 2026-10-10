# 服务商配置

Hoshiro 采用 **Bring Your Own Key (BYOK)** 模式：语音与文本请求直接由本地发往配置的服务商端点，无中间服务器中转。语音识别（ASR）与文本润色（LLM）独立配置、按需搭配。

在「设置 › 模型服务」中配置，完成后可通过「测试连接」验证连通性。

::: tip 推荐搭配
**中文为主**：小米 MiMo（ASR）+ Google Gemini Flash（LLM）——识别准确、响应低延迟、调用成本低。
:::

## 语音识别（ASR）

| 服务商 | 必填项 | 特点 |
|---|---|---|
| **小米 MiMo**（推荐） | API Key、服务区域（Global / 中国） | 中文识别率高，响应快，自带标点。模型 `mimo-v2.5-asr` |
| **Cloudflare Workers AI** | Account ID、API Token | 推荐模型 `Whisper Large v3 Turbo`；受官方排队机制影响延迟约 2~3 秒 |
| **Azure Speech** | Speech Key、Region（如 `eastasia`） | 流式传输，边说边识别 |
| **Google Cloud Speech** | 项目 ID、区域、模型；API Key 或 Service Account JSON | 流式传输，需在 GCP 启用 Speech-to-Text API |

### 小米 MiMo

1. 在小米 MiMo 开放平台获取 API Key。
2. 「服务区域」需与账号归属地保持一致：国内账号选择 **中国**，海外账号选择 **Global**。

### Cloudflare Workers AI

1. 在 Cloudflare 控制台主界面右侧获取 **Account ID**。
2. 创建具备 **Workers AI 读取/运行** 权限的 API Token。
3. 模型选择 `Whisper Large v3 Turbo`。

### Azure Speech

1. 在 Azure Portal 创建「语音服务 (Speech Services)」资源。
2. 填写资源对应的 Key 与区域（Region，例如 `eastasia`）。

### Google Cloud Speech

1. 在 GCP 项目中启用 **Speech-to-Text API**。
2. 填写项目 ID、区域（如 `global` / `us` / `eu`）以及模型名称。
3. 凭据二选一：直接填写 API Key，或粘贴 Service Account JSON 全文。

## AI 润色（LLM）

| 服务商 | 必填项 | 默认模型 |
|---|---|---|
| **Google Gemini** | API Key | `gemini-flash-latest` |
| **Anthropic Claude** | API Key | `claude-haiku-5-5` |
| **OpenAI 兼容接口** | Base URL、API Key、模型名 | 自定义 |

「OpenAI 兼容接口」适用于 OpenAI、DeepSeek、OpenRouter 以及各类自建代理中转，常见示例如下：

| 服务 | Base URL |
|---|---|
| OpenAI | `https://api.openai.com/v1` |
| DeepSeek | `https://api.deepseek.com/v1` |
| OpenRouter | `https://openrouter.ai/api/v1` |

Gemini 与 Anthropic 的 Base URL 默认留空即可直连官方地址，仅在使用自定义代理时填写。

::: warning 思考强度说明
「改写」设置中的思考强度（Thinking）默认保持关闭。开启后模型将增加思维链耗时，显著增加上屏延迟，常规口述润色建议关闭。
:::

## 降级与兜底策略

若在设置中关闭「启用改写」，Hoshiro 将直接插入语音识别的原始转录文本（此时语气词过滤、改口修正与排版规范化将不生效）。

当改写服务因网络异常、API Key 无效或欠费导致请求失败时，系统将自动降级并插入原始识别结果，同时弹出「改写失败，已插入原始转录」提示，确保口述文本不丢失。
