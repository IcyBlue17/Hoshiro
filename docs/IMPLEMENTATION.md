# 本轮实现与验收

对应 `docs/NEXT.md` 的交接清单，2026-10-09。

## 个性化

- 粘贴后在 3 / 10 / 30 秒观察同一应用中可由首尾锚点唯一定位的文本。保留最后一个可定位版本；单词修正继续进入自动词典，完整修改对写入 `edits`。
- SQLite 从 v1 迁移到 v2，新增修改对、个人档案、已分析会话 ID 和用量表。删除/清理历史会同时删除相关修改对和分析标记；重试同一历史不会丢失标记。`historyRetention=never` 不保存修改对。
- 每 30 分钟检查；首次自动分析需 10 条新记录，之后需 15 条且距离上次至少 20 小时。下一次执行时间额外加入 0–6 小时抖动并持久化。手动需 3 条新记录。同一时间仅运行一个任务。
- 新记录优先，保留完整样本；历史、修改对、当前档案和词典合计不超过 60,000 字符。问答记录不作为表达偏好的训练样本。导入历史按 ID 判断是否分析，不依赖导入时间。
- 严格解析模型 JSON，限制长度、去重；档案、分析标记和新词在同一数据库事务内保存，失败全部回滚；保留用户修改的个人提示词。失败保留上一份档案、记录错误与下次时间，不弹窗。分析期间关闭学习会阻止应用结果。
- 口述/翻译注入口癖、纠错、词汇和个人提示词；问答仅注入词汇与纠错。个人档案不能覆盖原有任务规则，自定义指令在风格偏好上优先。
- 设置提供开关、状态、立即分析、删除学习项、提示词编辑/恢复自动更新、重置。重置档案保留词典，和原接口约定一致。

## 长文本

- 根据字数和录音时长区分短、中、长；短句最小清理，长段整理逻辑、合并重复和放弃的想法，保留最终事实/数字/请求。
- 全长度清理半截话、犹豫和改口。中文数量、编号、日期、金额等改为阿拉伯数字，固定词语/成语除外。
- 增加数字、混合语言改口和 3 组长段示例。关闭智能格式化时不附加长段列表示例。
- 保持用户选择的模型，按录音时长放宽超时（原超时 + 每分钟 6 秒）。没有暗中切换付费模型。
- 输出清理器支持剥离 `<analysis>` 草稿和未闭合草稿；默认提示词仍要求直接输出正文，避免为短句增加思考延迟。
- `src-tauri/examples/rewrite_eval.rs` 从本机数据库读取最近 30 条真实口述，输出原文/原结果/新结果的 JSONL 对照。仓库与此环境没有用户真实历史，未伪造“真实评测集”。

```bash
# 在有真实数据的设备上；输出含私人文本，不要提交到仓库。
cd src-tauri
cargo run --example rewrite_eval -- "$HOME/Library/Application Support/dev.hoshiro.app" --dry-run > /tmp/hoshiro-prompts.jsonl
# 去掉 --dry-run 才会调用设置中的改写模型，产生服务商用量。
cargo run --example rewrite_eval -- "$HOME/Library/Application Support/dev.hoshiro.app" > /tmp/hoshiro-comparison.jsonl
```

## 服务商与 SDK 调研

| 服务 | 实现与依据 |
|---|---|
| MiMo | 原来的停顿分段和并发上限继续使用；记录返回的 token 用量。 |
| Cloudflare | 使用官方仓库维护的 [`cloudflare` 0.14 SDK](https://github.com/cloudflare/cloudflare-rs)。SDK 的旧 ASR 参数只有字节数组、source/target language，因此通过 SDK 的 `EndpointSpec` 扩展 [Whisper Turbo](https://developers.cloudflare.com/workers-ai/models/whisper-large-v3-turbo/) 所需的 base64 `audio`、`language`、`initial_prompt`，复用 SDK 的鉴权、HTTP 和响应处理。非流式，沿用录音分段。 |
| Azure | [官方 Speech SDK 语言列表](https://learn.microsoft.com/azure/ai-services/speech-service/speech-sdk)没有 Rust。用一个 `tokio-tungstenite` 依赖实现 WebSocket 帧，参考[官方 JS Speech SDK 协议实现](https://github.com/microsoft/cognitive-services-speech-sdk-js/tree/master/src/common.speech)。包含 Phrase List、前 4 种候选语言、PCM、结束帧、连续轮次和未确认音频续传。 |
| Google | [`google-cloud-speech-v2` 1.15](https://docs.rs/google-cloud-speech-v2/1.15.0/google_cloud_speech_v2/client/struct.Speech.html#method.streaming_recognize)已有官方双向 `StreamingRecognize`，与交接文档的待核实状态不同。使用该 SDK 及官方 `google-cloud-auth` 完成 API Key / 服务账户鉴权；支持词典 adaptation、前 3 种候选语言。模型/区域需支持相应功能。 |

流式提供商在录音开始时建立任务，音频重采样后实时送出；取消会终止任务，结束后等待最终结果。4 分钟轮换连接，避开 Google 的 5 分钟限制。每条 Google 音频消息低于 25 KB。历史重试按近实时速度送出保存的音频，因此长录音的重试耗时接近录音长度。

新增提供商都有配置表单和静音连接测试。测试会产生少量真实服务用量。中间识别结果没有显示在悬浮条上（原清单的可选项），保留现有悬浮条交互。

TLS 统一使用现有 reqwest 的 Ring provider，关闭 Google SDK 的额外默认 TLS provider，避免与 WebSocket 的 TLS 初始化冲突。

## 统计口径

- 会话、文字、时长：当前保留的历史；含 Typeless 导入记录，仅已完成记录贡献文字和时长。删除/保留策略会影响这些指标。
- token / 请求 / 送识别音频：独立的无文本用量表，来自成功返回的服务响应；包含改写、个性化分析和连接测试，重试单独计数。API 失败、取消或服务商未报告部分不能完整追溯，不能当作服务商账单。
- token 不按字数推算，不倒填旧记录；未知与真实的 0 在存储中分别记录。Anthropic 的缓存读写计入输入，Gemini 的思考 token 计入输出。
- 节省时间：按每分钟打字 40 字估算，最低为 0。日期使用设备本地时区。词典统计始终显示当前总数。
- 统计页沿用原主题、卡片、字号和控件；支持近 7 天、近 30 天、全部，并显示每日趋势、模式、常用应用和模型明细。

## 验证边界

本轮在 Linux 完成 `bun run build`（TypeScript 检查与生产构建）和 `cargo test --all-targets`（145 项通过，0 项失败）。浏览器回归覆盖统计日期筛选、暗色主题、880px 窗宽、个人提示词保存/恢复、删除学习项、四个 ASR 配置表单及原有首页/历史/词典导航，没有捕获到前端运行时错误。另验证了统计空数据、读取失败提示与重试恢复。

环境未配置各服务商真实密钥，也没有用户真实历史，因此没有进行付费 API、真实录音识别准确率和个人历史改写质量评测。macOS Fn、辅助功能、自动粘贴和安装包需在 Mac 上做实机验收。现有 macOS 平台实现未修改；3 个仅用于 Mac 的诊断示例增加了编译平台条件，使 Linux 的全目标检查可以运行；Windows/Linux 原有全局热键与粘贴限制仍在。

没有重新启用或触发 GitHub Actions，没有发布未经实机验收的安装包。
