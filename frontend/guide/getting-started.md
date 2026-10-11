# 安装与首次设置

本页介绍 Hoshiro 的系统要求、安装步骤与基础配置。

## 系统要求

| 项目 | 要求 |
|---|---|
| 系统 | macOS 13 Ventura 及以上；Windows 10 / 11（x64）；或 Linux（x64：Ubuntu 22.04+、Debian 12+、Fedora 38+ 等，X11 / Wayland） |
| 芯片 | Mac：Apple Silicon（M 系列）或 Intel；PC：x64 |
| 网络 | 能够访问所配置的语音识别 / 大模型服务商 API |
| 凭据 | 至少一个语音识别服务的 API Key；建议同时配置文本润色模型 |

## 1. 下载安装

前往 [GitHub Releases](https://github.com/IcyBlue17/hoshiro/releases/latest) 下载安装包。

### Windows

下载 `x64-setup.exe` 并运行。安装包按当前用户安装，不需要管理员权限，系统自带的 WebView2 即可运行。

::: warning 提示“Windows 已保护你的电脑”
安装包没有代码签名，SmartScreen 可能拦截。点击「更多信息」›「仍要运行」即可。
:::

Windows 上全局快捷键与文字插入不需要额外授权，可跳过下面的第 2 步与第 4 步。

### Linux

| 发行版 | 下载 | 安装 |
|---|---|---|
| Debian / Ubuntu / Deepin / 统信 | `amd64.deb` | `sudo apt install ./Hoshiro_*_amd64.deb` |
| Fedora / openSUSE / RHEL | `x86_64.rpm` | `sudo dnf install ./Hoshiro-*.x86_64.rpm` |
| 其他发行版 | `amd64.AppImage` | `chmod +x` 后直接运行 |

Hoshiro 直接从内核读取键盘（`/dev/input`）并通过虚拟键盘（`/dev/uinput`）粘贴，X11 与 Wayland 都能用。deb / rpm 安装包自带对应的 udev 规则，装好即可使用；AppImage 在引导页的权限步骤点「授权」并输入管理员密码即可。手动配置与已知限制见 [常见问题](./faq#linux-使用说明)。

### macOS

下载对应架构的 `.dmg` 安装包：

- **Apple Silicon**（M 系列芯片）：下载包含 `aarch64` 的文件
- **Intel 芯片**：下载包含 `x64` 的文件

::: tip 芯片架构确认
点击屏幕左上角  ›「关于本机」，若「芯片」项显示 Apple M 系列即为 Apple Silicon；若显示 Intel 处理器即为 Intel 架构。
:::

双击挂载 `.dmg`，将 Hoshiro 拖入「应用程序」目录。

::: warning 提示“已损坏”或“无法验证开发者”
若打开应用时系统提示未签名或已损坏，在终端中运行以下命令移除安全隔离属性即可：

```bash
xattr -cr /Applications/Hoshiro.app
```
:::

## 2. 授予系统权限（macOS）

首次启动时应用会引导授予以下三项系统权限：

| 权限 | 用途 |
|---|---|
| **麦克风** | 音频信号录制 |
| **辅助功能** | 模拟键盘粘贴将文本插入当前光标处、读取选中内容（改写功能） |
| **输入监控** | 全局监听快捷键（如 <Kbd size="sm">fn</Kbd>）按下与松开事件 |

授权完成后若状态指示未立即更新，重启 Hoshiro 即可生效。后续可在「设置 › 权限」中随时检查权限状态。

::: tip Windows 的麦克风权限
Windows 只需要麦克风权限。若提示麦克风不可用，打开「设置 › 隐私和安全性 › 麦克风」，开启「麦克风访问权限」与「允许桌面应用访问麦克风」。
:::

## 3. 配置 API Key

打开「设置 › 模型服务」：

1. **语音识别**：选择服务商并填写 API Key。中文场景推荐 **小米 MiMo**，识别准确、自带标点且响应迅速。
2. **AI 改写**：选择模型服务商并填写 API Key。推荐 **Google Gemini**（默认 `gemini-flash-latest`），速度快且成本低。
3. 点击「测试连接」，确认接口连通性。

所有 API Key 均保存在系统凭据库（macOS 钥匙串 / Windows 凭据管理器 / Linux 的 GNOME 钥匙环或 KWallet）中，不写入本地配置文件。各服务商详细配置参数参见 [服务商配置](./providers)。

## 4. 关闭系统的 🌐 键默认行为（macOS）

macOS 默认将单独按下 <Kbd size="sm">fn</Kbd>（🌐）映射为弹出表情符号面板或切换输入法，该行为会与 Hoshiro 的长按监听产生冲突：

打开系统「设置 › 键盘」，将 **「按下 🌐 键时」** 选项调整为 **「不执行任何操作」**。

## 5. 测试输入

在任意输入框中点击聚焦，然后：

<div class="hs-demo">
  <Kbd>fn</Kbd>
  <span class="cap">长按说话 →</span>
  <Pill cycle />
  <span class="cap">→ 松开后文本自动插入光标处</span>
</div>

Windows 上请按住 <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Win</Kbd>，Linux 上按住 <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Super</Kbd>（PC 键盘的 Fn 由键盘固件处理，系统收不到）。

屏幕底部的胶囊为 Hoshiro 悬浮条：录音时展示实时波形，松开后显示处理状态，完成后文本自动插入。

下一步：查阅 [输入模式与快捷键](./modes)。
