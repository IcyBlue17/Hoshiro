# 贡献指南

感谢关注 Hoshiro。在提交 Issue 或 Pull Request 前，请阅读以下开发说明与约定。

## 开发环境

- macOS 13+
- [Rust](https://rustup.rs) (stable)
- [Bun](https://bun.sh)
- Xcode Command Line Tools

```bash
bun install
bun run tauri dev     # 启动完整桌面端应用
```

> **权限提示**：`tauri dev` 运行在当前终端进程树下，辅助功能与输入监控权限会绑定在终端应用上。如需测试真实的系统权限申请与全局快捷键捕获，建议打包安装测试：
> ```bash
> bun run tauri build --bundles app
> ```

### 纯前端开发

若仅调试 UI，无需编译 Rust 后端，可使用 `src/lib/mock.ts` 提供的模拟数据在浏览器中开发：

```bash
bun run dev
```

- 主窗口：<http://localhost:1420/>
- 悬浮条预览：<http://localhost:1420/?window=bar&demo=1>
- 引导页：<http://localhost:1420/?onboarding=1>

## 项目架构

```
src/                        前端（React 19 + TypeScript + Vite）
  lib/types.ts              ★ 前后端数据结构契约（与 types.rs 镜像）
  lib/ipc.ts                ★ 后端命令与事件封装
  lib/mock.ts               浏览器端 Mock 数据
  hub/                      主窗口：首页、历史、词典、统计、设置、引导
  bar/                      悬浮条：录音胶囊、波形、提示卡片、问答卡片
src-tauri/src/              后端（Rust）
  types.rs                  ★ 前后端数据结构契约（与 lib/types.ts 镜像）
  controller.rs             输入会话状态机
  pipeline.rs               处理管线：识别 → 改写 → 插入 → 持久化
  platform/                 平台集成：CGEventTap 快捷键、辅助功能、粘贴、面板管理
  audio/                    音频采集、重采样、分段切片、提示音
  providers/                ASR / LLM 客户端与提示词模板
  personalize.rs            个性化学习引擎
  config.rs, secrets.rs     配置持久化与系统钥匙串集成
  db.rs, stats.rs, usage.rs SQLite 存储、指标统计、用量追踪
```

> 标 ★ 的文件为前后端接口契约，修改时须保持双向同步。

## 核心交互流水线

1. **按键捕获**：`platform/hotkey.rs` 通过 `CGEventTap` 捕获底层按键，由 `gesture.rs` 状态机判断长按、双击或组合键，触发对应动作。
2. **会话调度**：`controller.rs` 调度状态迁移，启动录音并采集当前焦点应用上下文。
3. **音频流切片**：长音频按静音停顿点后台分段切片，并发发起后台 ASR 转录。
4. **模型改写**：`pipeline.rs` 拼接转录结果并交由 LLM 处理；改写异常时自动降级回退至原始转录。
5. **文本插入**：`platform/insert.rs` 借助系统剪贴板执行模拟粘贴，并自动恢复用户原有剪贴板内容。
6. **落库与更新**：写入 SQLite 历史记录与用量统计，悬浮条触发收起动画。

## 测试与代码规范

```bash
# 后端测试
cd src-tauri && cargo test --lib

# 前端类型检查
bunx tsc --noEmit -p .
```

- **纯函数优先**：涉及状态机转移、提示词组装、统计口径等逻辑应尽量抽离为纯函数编写单元测试。
- **避免外部依赖污染**：网络交互使用本地 mock 接口验证，不得在测试中调用真实 API 或读写用户系统钥匙串。
- **日志安全**：严禁在日志中输出用户 API Key、录音内容或转录明文；仅允许记录执行耗时、文本长度及状态码。
- **依赖控制**：尽量复用现有依赖（如 `reqwest`），避免引入非必要的三方库。
- **提交检查**：提交前确保 `cargo check` 无警告且 `tsc` 检查通过。

## 提交 Issue

提交问题报告时，请附带：
1. macOS 版本与芯片架构
2. Hoshiro 版本号（可在主界面「关于」中查看）
3. 当前配置的 ASR 与改写模型服务商
4. 复现步骤与异常表现
5. 相关运行日志（位于 `~/Library/Application Support/cn.icybit.hoshiro/logs/`，上传前请确认已脱敏）
