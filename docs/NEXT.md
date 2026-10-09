# 待办：AI 个性化学习 + 长文本整理

给接手的 AI / 开发者看的。先读 `SPEC.md` 了解整体架构。

---

## 零、构建和发布（先看这个）

- **现状**：GitHub Actions 已在仓库上禁用（`gh workflow disable Build`），workflow 只剩手动触发；仓库 `IcyBlue17/hoshiro` 已转为**私有**。用户的 Actions 额度已经用完，私有仓库跑 Actions 要额外付费，**不要重新启用，也不要触发 workflow**
- **做法**：在你自己的机器上构建，然后用 `gh release create` 发 release（本机 gh CLI 已登录）：
  ```bash
  bun install
  bun run tauri build --target aarch64-apple-darwin   # Apple 芯片
  bun run tauri build --target x86_64-apple-darwin    # Intel（需先执行 rustup target add x86_64-apple-darwin）
  gh release create v0.x.y --title "Hoshiro v0.x.y" --notes "..." \
    src-tauri/target/*/release/bundle/dmg/*.dmg
  ```
- 只能构建本机系统对应的平台：Mac 上只出 macOS 的 dmg。Windows（msi / nsis）和 Linux（deb / rpm / AppImage）需要在对应系统上构建，没有条件就先只发 macOS，并在 release 说明里注明
- **签名**：`tauri.conf.json` 里是 `signingIdentity: "-"`（ad-hoc 签名），不要把个人证书写进仓库。本机想用固定证书（避免每次重新编译都要重新授权辅助功能），用环境变量覆盖：`APPLE_SIGNING_IDENTITY="Apple Development: ..." bun run tauri build`
- **未推送的改动**：以下几处本地已改好，发版前请一起提交：
  - Windows 打包缺 `.ico` 的修复（`tauri.conf.json`）
  - 个性化的契约层（`types.ts` / `types.rs` / `ipc.ts` / `mock.ts`）
  - 本文档
  - 上面这个 workflow 改动

  上一次 CI 的结果：macOS arm64 / x64 和 Linux 都已编译通过，Windows 只是因为缺 `.ico` 失败

---

## 一、AI 个性化学习

### 已经做好的（契约层，已编译通过）
- `src/lib/types.ts` / `src-tauri/src/types.rs`：`PersonalProfile`、`ProfileCorrection`，`StyleConfig.aiPersonalization`（默认 true），事件 `app://profile-updated`
- `src/lib/ipc.ts`：`getProfile` / `runPersonalization` / `updateProfile(profile)` / `resetProfile`
- `src/lib/mock.ts`：这四个命令的假数据（浏览器里可以直接做 UI）

### 还没做的

**1. 采集修改对（升级 `autodict.rs` + `pipeline.rs::spawn_auto_dictionary`）**
- 粘贴后在 3s / 10s / 30s 各读一次 `context::focused_text_value()`，取最后一次还能用首尾锚点对上原文的版本
- 只要用户改过，就把 `(history_id, inserted, final_text, app_name, created_at)` 存进新表 `edits`（db.rs 加 migration，`PRAGMA user_version = 2`）
- 原来的单词规则保留：单个词的小改动立刻加进词典

**2. 后台任务 `personalize.rs`**
- **触发**：`aiPersonalization` 开 + 改写服务配了 key + 距上次 ≥ 20h + 新增完成的会话 ≥ 15。每 30 分钟检查一次，下次运行时间加 0–6h 随机抖动（存 `nextRunAt`）。手动“立即分析”只要求 ≥ 3 条。第一次运行（含 Typeless 导入的历史）≥ 10 条即可
- **发给模型**：最近的已完成会话，按时间倒序，总量控制在约 60k 字符；每条格式为 `app | raw: <原始转录> | final: <最终文本>`。另外附上 `edits` 里的修改对（`inserted → user_final`）、当前 profile（让模型增量更新，而不是推倒重来）、当前词典
- **模型返回严格 JSON**：
  ```json
  {"verbalTics":[], "corrections":[{"wrong":"","right":""}], "vocabulary":[], "styleNotes":[], "promptAddendum":""}
  ```
  分析提示词要强调：口癖必须在多条记录里反复出现才算；corrections 必须有证据（原始转录里反复出现同一错误，或者用户手动改过）；promptAddendum 只写风格和格式偏好（≤ 1200 字），**禁止**写任何会让改写模型回答问题、执行指令、补充内容的话
- **校验后落库**：tics ≤ 30、corrections ≤ 60、vocabulary ≤ 150，每项 ≤ 32 字符，去重。vocabulary 写进词典（source `auto`，已存在的跳过）。`addendumEditedByUser = true` 时不覆盖 promptAddendum。profile 存成 settings 同目录下的 `profile.json`，或者存 DB
- **事件**：开始和结束都 emit `app://profile-updated`（`running` 字段）。失败写 `lastError`，不弹窗打扰

**3. 注入改写（`providers/prompts.rs` + `RewriteRequest` 加 `profile: Option<PersonalProfile>`）**
- 在 `<custom_instructions>` 之前加一个 `<personal_profile>` 块，放：额外要删的口癖、“常见识别错误：wrong → right（只在上下文吻合时替换）”、promptAddendum
- 系统提示词里加一句：personal_profile 只影响用词和风格，不能推翻绝对规则；用户自己写的 custom_instructions 优先级最高
- 口述和翻译模式都注入；问任何问题模式只注入 vocabulary 和 corrections

**4. 命令**：`get_profile`、`run_personalization`（同一时间只允许一个在跑）、`update_profile`（如果 promptAddendum 和原来不一样，就置 `addendumEditedByUser = true`）、`reset_profile`

**5. UI（设置 › 个性化）**
- 总开关「AI 个性化学习」，附说明：会把历史记录发给你配置的改写模型
- 状态区：上次分析时间、已分析多少条、待分析多少条、下次自动分析时间，加「立即分析」按钮（带 loading）
- 内容区：口癖 / 常见纠错 / 专有名词用 chip 展示，每项可以删；风格观察用列表展示；个人提示词用 textarea 展示，可编辑，附「恢复 AI 自动更新」按钮
- 可选：首页加一张小卡片，写“已学习 N 个口癖 · M 条纠错”

---

## 二、长文本整理的提示词建议（Typeless 强就强在这）

1. **按长度分档，行为不同**（在 user message 里告诉模型档位：字数 + 时长）
   - 短（一两句）：只做最小清理
   - 中：清理加分句
   - 长（> 约 150 字或 > 30 秒）：**整理而不是转写**。先给一句主旨，再分点 1. 2. 3.，需要时结尾放「待办 / 请求」
2. **长文本重排规则**：按逻辑重新排序零散观点，合并重复的说法，删掉说到一半放弃的想法和推翻的想法，去掉“我想说一下 / 怎么说呢 / 就是这个意思 / 让我想想”这类元话语
3. **保真清单**（这是核心约束）：所有数字、人名、日期、金额、决定、请求、待办都必须出现在输出里；不新增事实；保持第一人称、立场和语气；关键术语沿用用户的原词。可以让模型在输出前自检这一条
4. **草稿区**：对长文本允许模型先写 `<analysis>要点 / 顺序 / 格式选择</analysis>` 再输出正文，`clean_output` 负责把它剥掉。非 thinking 模型的整理质量会明显上去，代价是多一点延迟，所以只在长档开启
5. **格式决策要写成规则**：≥ 3 个并列项或步骤就列点；叙事就分段；不用标题和加粗；聊天软件里除非明确在列举，否则写成紧凑的一两段；邮件要有称呼和分段
6. **few-shot 是最大的杠杆**：放 2–3 个又长又乱的真实例子和理想输出。要覆盖：顺序打乱 → 重排；中途改主意 → 只保留最终想法；夹杂很多口癖的长段 → 精简成三点。例子最好取自真实历史（`history.raw_text`）
7. **模型路由**：短句用快模型；长段（音频 > 40s）可以切到更强的模型或者开轻度 thinking，超时也按长度放宽（比如 12s 加上每分钟音频 6s）
8. **建评测集**：从历史里挑 30 条真实的 raw_text，写个 `examples/rewrite_eval.rs`，批量跑当前提示词，输出 before / after 对照。每改一次提示词都跑一遍，免得把短句改坏

### 2.1 中文数字一律转阿拉伯数字（用户反馈：现在会照搬 ASR 的“一二三四”，很难看）
改 `providers/prompts.rs` 里的 DICTATION_CORE「Spoken numbers」那条，以及翻译提示词：
- **默认全部转数字**：数量、编号、序号、时间、日期、金额、百分比、版本号、电话、年龄、页码……例如“三个人” → “3 个人”，“第二点” → “第 2 点”，“十五分钟” → “15 分钟”，“一百二十块” → “120 块”，“两千五” → “2500”，“零点五” → “0.5”
- **口述列举“一、二、三……”或“第一……第二……”** → 列表编号 `1. 2. 3.`，不要写成“一、”
- **只有固定词语和成语保留汉字**：一下、一起、一些、一样、一直、一般、一定、统一、唯一、万一、一点儿（表程度时）、十分（副词）、三思、一石二鸟等。拿不准就转数字
- few-shot 里加 2–3 条专门的数字例子，并把现有“keep ordinary small counts as words”那句删掉（这句正是现在的问题来源）

### 2.2 删掉犹豫、半截话和改口（用户反馈：说了“如果用……”想了一下又改口，这种也要删）
在 CLEANUP 里加一条独立规则，**所有长度都生效**（不只长文本）：
- 说到一半就放弃的半截句（false start），比如“如果用……然后我们……其实应该先……”，只保留最后真正说完整的那句
- 犹豫和自言自语：“嗯让我想想”“这个怎么说”“等一下”“不是不是”“算了”，以及这类词后面被推翻的内容
- 重说：同一个意思说了两遍，只保留更完整的那一遍
- few-shot 加两条：一条中文半截话，一条中英混合的改口

---

## 三、更多语音识别服务

用户要的：**Cloudflare Workers AI Whisper**、**Azure 流式语音识别**、**Google Cloud 流式语音识别**。

### 依赖原则（用户要求）
- 尽量少引入依赖，能不加就不加
- 但如果服务商有**官方、维护中**的 Rust SDK，优先用 SDK，不要手搓 API
- 没有官方 Rust SDK 时，再用现有的 `reqwest` 走 HTTP；WebSocket 需要时只加一个轻量依赖（比如 `tokio-tungstenite`）
- 动手前先查清楚每家现在有没有官方 Rust SDK、支不支持流式，把结论写进 PR 说明。下面的判断都需要核实，不保证正确

### 架构改动
- `providers/` 下抽一个 ASR trait，分两类：
  - **批量型**（MiMo、Cloudflare）：沿用现在的分段机制，录音时按停顿切段、边录边识别
  - **流式型**（Azure、Google）：`start` 时就建立连接，录音的 16k PCM 实时推送，松开 Fn 时发结束信号，取最终结果。延迟应该比批量型低一截
- 可选：流式型的中间结果（partial）可以显示在悬浮条上，Typeless 没有这个功能，做成默认关闭
- `AsrConfig.provider` 扩展为 `"mimo" | "cloudflare" | "azure" | "google"`，每家一个子配置。`types.rs` 和 `types.ts` 要同步改
- 词典利用（MiMo 没有热词，其他家有）：
  - Cloudflare Whisper：词典拼成 `initial_prompt`
  - Azure：Phrase List
  - Google：adaptation / phrase hints
- 设置页「模型服务」加服务商切换，每家都要有字段表单和「测试连接」

### 各家要点（需核实）
- **Cloudflare Workers AI**：REST `POST https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/@cf/openai/whisper-large-v3-turbo`，`Authorization: Bearer <API Token>`。配置项：account_id、token、model。非流式，走批量路径
- **Azure Speech**：配置项为 key、region。流式需要 WebSocket 协议，或者官方 Speech SDK（只有 C/C++ 等语言的原生库，没有官方 Rust SDK，链接原生库很重，不推荐）。短音频 REST 不支持流式。语言自动识别最多支持几个候选语言，候选从 `selectedLanguages` 生成
- **Google Cloud Speech-to-Text v2**：流式只有 gRPC（`StreamingRecognize`），REST 不支持流式。先确认官方 `google-cloud-rust` 系列 crate 是否已经支持 speech v2 的双向流：支持就用，不支持再评估 `tonic` 加 proto 的体积。鉴权支持 API Key，以及 Service Account JSON（后者要做 JWT 签名换 token）
- 三家都要把错误信息映射成中文提示（key 错、区域错、额度用完、网络问题），并且要有单元测试和 mock 测试（参照 `providers/mimo.rs` 的写法）
