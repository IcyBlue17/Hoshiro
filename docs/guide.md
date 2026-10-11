# 使用指南

- [基础操作](#基础操作)
- [服务商配置](#服务商配置)
- [个性化与词典](#个性化与词典)
- [数据与隐私](#数据与隐私)
- [常见问题](#常见问题)

## 基础操作

### 输入模式

> Windows / Linux 上 `Fn` 对应 `Ctrl + Win`（Linux 上叫 `Ctrl + Super`；PC 键盘的 Fn 由键盘固件处理，系统收不到），`⌘` 对应 Win / Super 键，`⌥` 对应 Alt。

| 模式 | 默认按键 | 说明 |
|---|---|---|
| 口述 | 按住 `Fn`，松开结束 | 语音转文字并自动润色，结果插入当前光标处 |
| 免提 | 双击 `Fn` 开始，再次按下结束 | 适用于长文本录入 |
| 翻译 | `Fn + Left Shift` 开始，按 `Fn` 结束 | 语音转录后翻译为目标语言并插入 |
| 随便问 | `Fn + Space` 开始，按 `Fn` 结束 | 选中文本：按指令改写并替换原内容<br>未选中文本：在悬浮条展示回答结果 |

- 录音或处理中按 `Esc` 可取消输入；取消后 5 秒内可点击提示卡片的「撤销」恢复。
- 单击 `Fn` 不会触发录音，仅提示长按使用。
- `Ctrl + ⌘ + V`（Windows / Linux：`Alt + Shift + V`）可快速重新粘贴上一条转录内容。

### 快捷键自定义

在「设置 › 快捷键」中配置。支持配置多个触发键，支持单独的修饰键（如 `Fn`、右 `Option`、右 `⌘`）、组合键以及鼠标侧键。

## 服务商配置

在「设置 › 模型服务」中分别配置语音识别与 AI 改写。配置后可通过「测试连接」验证连通性。

### 语音识别 (ASR)

| 服务商 | 必填项 | 说明 |
|---|---|---|
| 小米 MiMo（推荐） | API Key、套餐（按量付费 / Token Plan） | 中文识别准确率高，响应快，自带标点。模型：`mimo-v2.5-asr`。按量付费的 Key 以 `sk-` 开头，Token Plan 的 Key 以 `tp-` 开头，两者不能混用 |
| Cloudflare Workers AI | Account ID、API Token | 模型推荐 `Whisper Large v3 Turbo`。响应受服务端排队影响（约 2~3 秒） |
| Azure Speech | Speech Key、Region（如 `eastasia`） | 流式传输，边说边识别 |
| Google Cloud Speech | Service Account JSON（不支持 API Key）；区域默认 `us`，项目 ID 可从 JSON 读取 | 流式传输，需启用 Speech-to-Text API，服务账户需“Cloud Speech 客户端”角色 |

### AI 改写 (LLM)

| 服务商 | 必填项 | 说明 |
|---|---|---|
| Google Gemini | API Key | 默认模型 `gemini-flash-latest` |
| Anthropic Claude | API Key | 默认模型 `claude-haiku-5-5` |
| Cloudflare Workers AI | Account ID、API Token、模型 | 默认模型 `Llama 3.3 70B Fast`。Account ID 与 Token 留空时，沿用语音识别中填写的 Cloudflare 设置 |
| OpenAI 兼容接口 | 平台、API Key、模型名 | 在「平台」下拉框中选择 OpenAI、Vercel AI Gateway、OpenRouter、DeepSeek、智谱、Kimi、阿里云百炼、火山方舟、硅基流动、Groq、xAI、Mistral 或本地 Ollama，会自动填入接口地址与默认模型。选择「自定义」可手动填写任意兼容服务的 Base URL |

> **思考强度**：「改写」设置中的思考强度（Thinking）默认关闭。开启后会增加额外响应延迟，建议保持关闭。

## 个性化与词典

在「设置 › 个性化」中调整文本处理偏好：

- **智能格式化**：将口述列表转为序号排版，规范化邮箱、链接等书面文本。
- **长段整理**：提供「分段」（按语意切分）与「列点」（整理为编号列表）两种模式。
- **自动添加词典**：手动修改刚插入的文字后，系统自动将修正词记录到词典。
- **个性化学习**：定期分析历史记录，自适应学习常用词、口语停顿与纠错偏好。可在「已学会」列表中查看并管理。
- **自定义指令**：向改写模型注入额外提示词（如“语气保持商务正式”、“称呼使用‘您’”等）。
- **专有名词词典**：在「词典」界面维护人名、项目名、技术术语，支持通过单列 CSV 批量导入。词典内容会优先参与纠错。

## 数据与隐私

- **本地存储**：历史记录、录音音频、词典与统计数据均保存在本地目录：  
  `~/Library/Application Support/cn.icybit.hoshiro/`（Windows：`%APPDATA%\cn.icybit.hoshiro\`；Linux：`~/.local/share/cn.icybit.hoshiro/`）
- **凭据加密**：API Key 仅保存在系统凭据库（macOS 钥匙串 / Windows 凭据管理器 / Linux 的 Secret Service，即 GNOME 钥匙环或 KWallet，条目名：`cn.icybit.hoshiro`），配置文件中不保存明文。
- **数据流向**：音频及转录文本仅直接发送至用户配置的服务商，无中转服务器。
- **保留策略**：可在「历史记录」中设置保留周期（从不 / 24小时 / 1周 / 1个月 / 1年 / 永久），过期数据自动清理。
- **本地日志**：日志位于数据目录下的 `logs/`，仅记录请求耗时、字数及状态，不记录录音内容及转录文本。

## Linux

支持 x64 的 Ubuntu 22.04+、Debian 12+、Fedora 38+ 等发行版，X11 与 Wayland（GNOME、KDE Plasma 等）均可使用。

### 安装

| 发行版 | 安装包 | 安装方式 |
|---|---|---|
| Debian / Ubuntu / Deepin / 统信 | `amd64.deb` | `sudo apt install ./Hoshiro_*_amd64.deb` |
| Fedora / openSUSE / RHEL | `x86_64.rpm` | `sudo dnf install ./Hoshiro-*.x86_64.rpm` |
| 其他发行版 | `amd64.AppImage` | `chmod +x Hoshiro_*.AppImage` 后直接运行 |

### 权限

Hoshiro 直接从内核读取键盘（`/dev/input`，用于全局快捷键），并通过虚拟键盘（`/dev/uinput`）发送粘贴快捷键。这样在 X11 和所有 Wayland 桌面上表现一致，但需要访问这些设备：

- **deb / rpm**：安装包自带 udev 规则 `/usr/lib/udev/rules.d/70-hoshiro.rules`，装好即可使用。
- **AppImage**：在「设置 › 权限」点击「授权」，输入管理员密码即可（会安装同样的规则到 `/etc/udev/rules.d/`）。
- **手动安装规则**：

  ```bash
  sudo tee /etc/udev/rules.d/70-hoshiro.rules >/dev/null <<'EOF'
  KERNEL=="uinput", SUBSYSTEM=="misc", TAG+="uaccess", OPTIONS+="static_node=uinput"
  SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_KEYBOARD}=="1", TAG+="uaccess"
  SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_MOUSE}=="1", TAG+="uaccess"
  EOF
  echo uinput | sudo tee /etc/modules-load.d/hoshiro-uinput.conf
  sudo modprobe uinput
  sudo udevadm control --reload-rules && sudo udevadm trigger
  ```

规则使用 `uaccess`：只有当前登录在本机桌面的用户能访问这些设备，无需加入 `input` 组，也无需重新登录。没有 systemd-logind 的系统可以改为把用户加入 `input` 组，并给 `/dev/uinput` 设置 `GROUP="input", MODE="0660"`。

### 默认快捷键

与 Windows 相同：按住 `Ctrl + Super` 口述，`Ctrl + Super + Space` 问答，`Ctrl + Super + Left Shift` 翻译，`Alt + Shift + V` 重新粘贴。Super 参与快捷键后，松开它不会再打开 GNOME 的活动概览。

### 已知限制

- **快捷键只监听、不拦截**：组合键里的普通键（如 `Ctrl + Super + Space` 的空格）以及录音时按下的 `Esc` 也会传给当前应用，因此默认快捷键都只用修饰键。
- **Wayland**：Hoshiro 通过 XWayland 运行，以便把悬浮条固定在屏幕底部并置顶。原生 Wayland 应用的名称和图标无法识别（只影响统计页的应用分布，不影响输入）。设置环境变量 `HOSHIRO_WAYLAND=1` 可改用原生 Wayland，但悬浮条位置由桌面决定。
- **非 QWERTY 键盘布局**：粘贴按的是 `V` 键的物理位置，Dvorak 等布局下可能需要把系统布局切回 QWERTY 后使用。
- **读取选中文本**：「问答」读取选中文本时会模拟一次复制（终端里用 `Ctrl + Shift + C`），完成后恢复剪贴板。
- **改动学习**：Linux 上无法读取输入框内容，「根据修改自动添加词典」不可用。
- **透明悬浮条**需要合成窗口管理器（GNOME、KDE 等主流桌面默认开启）。

## 常见问题

**按 Fn 键弹出系统表情面板或听写**  
打开 macOS「系统设置 › 键盘」，将「按下 🌐 键时」设为「不执行任何操作」。

**Windows：在某些程序里快捷键或输入无效**  
以管理员身份运行的程序（如部分游戏、安装程序、任务管理器）不接受普通权限程序发送的按键。需要在这类程序里使用时，请右键以管理员身份运行 Hoshiro。

**按住快捷键无响应**  
1. 检查「设置 › 权限」中麦克风、辅助功能、输入监控三项权限是否已全部启用（Windows 只需麦克风；Linux 见上文[权限](#权限)）。
2. 确认是否同时运行了 Typeless，两个应用会争夺底层按键监听。
3. 检查菜单栏是否存在 Hoshiro 图标，若无请重新启动应用。

**输入完成后未插入文本，弹出「复制」提示**  
当前光标未处于可编辑的文本框内。点击目标输入区域后再试，或直接点击提示卡片的「复制」。

**提示「改写失败，已插入原始转录」**  
改写服务请求失败（常见原因为 API Key 错误、账户欠费或模型无权限）。可在「设置 › 模型服务」中点击「测试连接」排查报错信息。

**语音识别延迟较高**  
在「使用统计 › 速度」中查看各阶段耗时。Cloudflare Workers AI 延迟主要来自官方排队；中文场景推荐使用小米 MiMo。若改写阶段耗时过长，确认思考强度已关闭或更换响应更快的模型。

**钥匙串授权弹窗频繁出现**  
系统提示「Hoshiro 想要使用钥匙串中的机密信息」时，选择「始终允许」。应用升级且签名变动时可能需要重新确认一次。

**如何完全卸载**  
1. 删除 `/Applications/Hoshiro.app`。
2. 删除配置及数据目录：`~/Library/Application Support/cn.icybit.hoshiro/`。
3. 在系统「钥匙串访问」中删除 `cn.icybit.hoshiro` 条目。
