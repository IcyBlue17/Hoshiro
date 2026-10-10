# 安装与首次设置

本页介绍 Hoshiro 的系统要求、安装步骤与基础配置。

## 系统要求

| 项目 | 要求 |
|---|---|
| 系统 | macOS 13 Ventura 及以上 |
| 芯片 | Apple Silicon（M 系列）或 Intel |
| 网络 | 能够访问所配置的语音识别 / 大模型服务商 API |
| 凭据 | 至少一个语音识别服务的 API Key；建议同时配置文本润色模型 |

> Windows / Linux 平台目前处于适配中。

## 1. 下载安装

前往 [GitHub Releases](https://github.com/IcyBlue17/hoshiro/releases/latest) 下载对应架构的 `.dmg` 安装包：

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

## 2. 授予系统权限

首次启动时应用会引导授予以下三项系统权限：

| 权限 | 用途 |
|---|---|
| **麦克风** | 音频信号录制 |
| **辅助功能** | 模拟键盘粘贴将文本插入当前光标处、读取选中内容（改写功能） |
| **输入监控** | 全局监听快捷键（如 <Kbd size="sm">fn</Kbd>）按下与松开事件 |

授权完成后若状态指示未立即更新，重启 Hoshiro 即可生效。后续可在「设置 › 权限」中随时检查权限状态。

## 3. 配置 API Key

打开「设置 › 模型服务」：

1. **语音识别**：选择服务商并填写 API Key。中文场景推荐 **小米 MiMo**，识别准确、自带标点且响应迅速。
2. **AI 改写**：选择模型服务商并填写 API Key。推荐 **Google Gemini**（默认 `gemini-flash-latest`），速度快且成本低。
3. 点击「测试连接」，确认接口连通性。

所有 API Key 均保存在 macOS 系统钥匙串中，不写入本地配置文件。各服务商详细配置参数参见 [服务商配置](./providers)。

## 4. 关闭系统的 🌐 键默认行为

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

屏幕底部的胶囊为 Hoshiro 悬浮条：录音时展示实时波形，松开后显示处理状态，完成后文本自动插入。

下一步：查阅 [输入模式与快捷键](./modes)。
