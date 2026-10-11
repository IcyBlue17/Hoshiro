# 贡献指南

感谢关注 Hoshiro。在提交 Issue 或 Pull Request 前，请阅读以下开发说明与约定。

## 开发环境

- macOS 13+（Xcode Command Line Tools）、Windows 10/11（Visual Studio 2022 Build Tools，勾选「使用 C++ 的桌面开发」；系统自带 WebView2）或 Linux（Debian / Ubuntu：`sudo apt install build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libasound2-dev`）
- [Rust](https://rustup.rs) (stable)
- [Bun](https://bun.sh)

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
  platform/                 平台集成：快捷键、上下文读取、粘贴、悬浮条、静音、图标
                            macOS：CGEventTap / Accessibility / NSPanel（*.rs 内的 mac_impl）
                            Windows：低级键盘钩子 / UI Automation / 剪贴板 / 不抢焦点的工具窗口（*_win.rs）
                            Linux：evdev / uinput 虚拟键盘 / X11 剪贴板与活动窗口 / GTK 通知窗口（*_linux.rs、linux.rs）
  audio/                    音频采集、重采样、分段切片、提示音
  providers/                ASR / LLM 客户端与提示词模板
  personalize.rs            个性化学习引擎
  config.rs, secrets.rs     配置持久化与系统钥匙串集成
  db.rs, stats.rs, usage.rs SQLite 存储、指标统计、用量追踪
```

> 标 ★ 的文件为前后端接口契约，修改时须保持双向同步。

## 核心交互流水线

1. **按键捕获**：`platform/hotkey.rs` 通过 `CGEventTap`（Windows：`WH_KEYBOARD_LL` / `WH_MOUSE_LL` 钩子，见 `hotkey_win.rs`；Linux：evdev，见 `hotkey_linux.rs`）捕获底层按键，由 `gesture.rs` 状态机判断长按、双击或组合键，触发对应动作。所有平台共用同一套键名（`Option` = Alt，`Cmd` = Win / Super 键）与状态机。
2. **会话调度**：`controller.rs` 调度状态迁移，启动录音并采集当前焦点应用上下文。
3. **音频流切片**：长音频按静音停顿点后台分段切片，并发发起后台 ASR 转录。
4. **模型改写**：`pipeline.rs` 拼接转录结果并交由 LLM 处理；改写异常时自动降级回退至原始转录。
5. **文本插入**：`platform/insert.rs` 借助系统剪贴板执行模拟粘贴，并自动恢复用户原有剪贴板内容。
6. **落库与更新**：写入 SQLite 历史记录与用量统计，悬浮条触发收起动画。

### Windows 平台说明

- **Alt / Win 的松开会被“屏蔽”**：Alt 或 Win 参与了快捷键后，单独松开会打开开始菜单或激活应用菜单栏（会吞掉随后的 `Ctrl+V`）。钩子会吞掉这次松开，先注入一个未分配的虚拟键 `0xE8` 再补发松开（与 AutoHotkey 相同的做法）。
- **注入期间推迟修饰键松开**：模拟 `Ctrl+V` 的几毫秒内，用户物理松开的修饰键会被钩子暂存并在注入结束后补发，避免 `Ctrl+V` 变成单个 `v` 落进输入法。
- **悬浮条的窗口样式归 tao 管**：tao 每次改变可见性 / 点击穿透都会重写整个窗口样式，所以显示、隐藏、点击穿透都走 Tauri API；`WS_EX_NOACTIVATE` / `WS_EX_TOOLWINDOW` 在 `WM_STYLECHANGING` 中强制补上。
- **终端**：在终端里 `Ctrl+C` 是中断信号，读取选中文本的回退方案在终端中改用 `Ctrl+Insert`；PuTTY / mintty 等改用 `Shift+Insert` 粘贴。
- **钩子诊断**：设置环境变量 `HOSHIRO_HOOK_TRACE=1` 后启动，日志会记录钩子收到的每个按键事件（不在钩子回调里写日志，不会拖慢输入）。
- **需要真实桌面的测试**：`cargo test --lib -- --ignored` 会注入真实按键、读写剪贴板、短暂静音、读写凭据管理器，只应在本机交互式桌面上运行。

### Linux 平台说明

- **输入走内核，不走显示服务器**：快捷键从 `/dev/input/event*` 读取（evdev），粘贴快捷键通过 uinput 虚拟键盘「Hoshiro Virtual Keyboard」发送，X11 与各家 Wayland 合成器行为一致。设备权限来自 `src-tauri/linux/70-hoshiro.rules`（`uaccess`），deb / rpm 安装时写入，AppImage 通过 pkexec 安装。
- **只能监听、不能拦截**：evdev 读不到「吞掉」按键的能力，所以默认快捷键只用修饰键；`Outcome::swallow` 在 Linux 上被忽略。
- **注入前等用户松开修饰键**：所有键盘设备的修饰键状态会被合并，用户还按着 Super 时发 `Ctrl+V` 会变成 `Super+Ctrl+V`。`linux::wait_modifiers_released` 根据 evdev 看到的物理状态最多等 1.5 秒。
- **Super / Alt 的松开会被“屏蔽”**：参与快捷键后在虚拟键盘上点一下 `F24`，GNOME 就不会把 Super 的松开当成单击打开活动概览。
- **XWayland**：`linux::prefer_x11_backend` 在 Wayland 会话里设置 `GDK_BACKEND=x11`，悬浮条才能定位、置顶并用 X 输入形状实现点击穿透；`_NET_ACTIVE_WINDOW` 只能看到 X 窗口。
- **终端**：粘贴用 `Ctrl+Shift+V`，读取选中文本用 `Ctrl+Shift+C`；xterm 一族改用 `Shift+Insert` 与 PRIMARY 选区。
- **CI 构建镜像**：Linux 任务在 `ghcr.io/icyblue17/hoshiro-linux-build:22.04` 容器里编译（Ubuntu 22.04 / glibc 2.35 + Tauri 依赖 + Rust + Bun），镜像由 `.github/docker/linux-build.Dockerfile` 经 `linux-image.yml` 每周重建。容器里没有 FUSE，镜像设置了 `APPIMAGE_EXTRACT_AND_RUN=1`。仓库变量 `LINUX_RUNS_ON` 可把任务切到自建 runner（需装 Docker）。

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
