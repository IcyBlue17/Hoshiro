# 参与开发

欢迎提 Issue 和 PR。下面是动手前需要知道的事。

## 环境

- macOS 13+（目前只有 macOS 功能完整）
- [Rust](https://rustup.rs) stable
- [Bun](https://bun.sh)
- Xcode Command Line Tools

```bash
bun install
bun run tauri dev     # 启动完整应用
```

`tauri dev` 跑出来的进程挂在终端名下，辅助功能、输入监控等权限也会算在终端头上。想测试真实的权限和全局快捷键，最好打包后安装再试：

```bash
bun run tauri build --bundles app
```

### 只改界面

不需要 Rust，浏览器里就能看，所有接口由 `src/lib/mock.ts` 的假数据提供：

```bash
bun run dev
```

- 主界面：<http://localhost:1420/>
- 悬浮条演示：<http://localhost:1420/?window=bar&demo=1>（左上角面板可以切换各种状态）
- 引导页：<http://localhost:1420/?onboarding=1>

## 目录结构

```
src/                        前端（React 19 + TypeScript + Vite）
  lib/types.ts              ★ 前后端共享的数据结构
  lib/ipc.ts                ★ 所有后端命令和事件的封装
  lib/mock.ts               浏览器里用的假后端
  hub/                      主窗口：首页、历史、词典、统计、设置、引导、关于
  bar/                      悬浮条：录音胶囊、波形、提示卡片、回答卡片
src-tauri/src/              后端（Rust）
  types.rs                  ★ 与 lib/types.ts 一一对应
  controller.rs             会话状态机：开始、切换、结束、取消、撤销
  pipeline.rs               识别 → 改写 → 插入 → 写历史
  platform/                 系统集成：全局快捷键、辅助功能、粘贴、静音、悬浮面板、图标
  audio/                    录音、重采样、分段、提示音
  providers/                语音识别和大模型客户端、提示词
  personalize.rs            AI 个性化学习
  config.rs, secrets.rs     设置读写；密钥存系统钥匙串
  db.rs, stats.rs, usage.rs SQLite、统计、用量
```

标 ★ 的是前后端约定：改了一边，另一边要同步改。

## 一次口述是怎么跑完的

1. `platform/hotkey.rs` 的事件监听拿到按键，经手势状态机（`gesture.rs`）判断是按住、双击还是组合键，发出 `Start / Finish / Cancel…`。
2. `controller.rs` 串行处理这些动作：先改状态，再启动录音、预热连接、采集当前应用的上下文。
3. 录音超过 25 秒会在停顿处切段，切好的段马上在后台识别；松手时通常只剩最后一段。
4. `pipeline.rs` 拼好识别结果，交给改写模型；改写失败就退回原始转录。
5. `platform/insert.rs` 通过剪贴板粘贴，再把剪贴板恢复成原样。
6. 写历史、更新统计，悬浮条收起。

## 测试

```bash
cd src-tauri && cargo test --lib
bunx tsc --noEmit -p .
```

- 新逻辑尽量拆成纯函数再测，比如手势状态机、提示词拼装、统计口径。
- 网络请求用 `providers::http::testserver` 起本地模拟服务测，不打真实接口。
- 不要写会碰真实钥匙串或用户数据目录的测试。

## 约定

- 界面文案用简体中文，短、直接，不写大段解释。
- 不在日志里打印密钥、转录文本或用户输入的内容；日志只记录耗时、长度、状态。
- 新增依赖要克制：能用现有的 `reqwest` 解决的，不再引入新的 HTTP / SDK。
- 用户可见的报错要说清原因和怎么办，服务商返回的原因尽量保留。
- 提交前确保 `cargo check` 没有警告、`tsc` 通过。

## 提交 Issue

请附上：macOS 版本、Hoshiro 版本（「关于」页）、使用的语音识别和改写服务商、复现步骤。日志在 `~/Library/Application Support/cn.icybit.hoshiro/logs/`，贴之前确认没有你不想公开的内容。
