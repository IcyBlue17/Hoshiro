# 数据与隐私

Hoshiro 不设中心化账户体系，无自建后端服务。

## 本地存储位置

| 数据类型 | macOS | Windows | Linux |
|---|---|---|---|
| 历史记录、录音音频、词典、统计指标 | `~/Library/Application Support/cn.icybit.hoshiro/` | `%APPDATA%\cn.icybit.hoshiro\` | `~/.local/share/cn.icybit.hoshiro/` |
| 服务商 API Key | 系统钥匙串（条目名 `cn.icybit.hoshiro`） | 凭据管理器（`credentials.cn.icybit.hoshiro`） | Secret Service：GNOME 钥匙环 / KWallet（服务名 `cn.icybit.hoshiro`） |
| 运行日志 | `~/Library/Application Support/cn.icybit.hoshiro/logs/` | `%APPDATA%\cn.icybit.hoshiro\logs\` | `~/.local/share/cn.icybit.hoshiro/logs/` |

本地应用配置文件中不存储任何明文 API 凭证。

## 数据流向

```text
本地设备 ──音频数据──▶ 用户配置的语音识别服务商
本地设备 ──文本数据──▶ 用户配置的 AI 润色服务商
```

音频与文本数据仅在发起识别或改写时直连发送至用户指定的服务商接口，无中间服务器中转。第三方服务商对数据的处理方式遵循其各自公布的 API 隐私条款。

## 数据保留策略

在「历史记录」界面可配置数据的自动清理周期（支持 从不保存 / 24小时 / 1周 / 1个月 / 1年 / 永久保留）。过期记录与对应的本地音频切片将自动物理删除。

## 运行日志

本地日志仅记录 API 请求耗时、字符数量及 HTTP 状态码，**严禁记录** 音频原始流及转写文本明文。用户在 GitHub 提交 Issue 时可安全附带日志文件。

## 完整卸载

**macOS**

1. 移除应用程序：`/Applications/Hoshiro.app`。
2. 清理用户数据目录：`~/Library/Application Support/cn.icybit.hoshiro/`。
3. 打开系统「钥匙串访问」，删除条目 `cn.icybit.hoshiro`。

**Windows**

1. 在「设置 › 应用 › 已安装的应用」中卸载 Hoshiro。
2. 删除用户数据目录：`%APPDATA%\cn.icybit.hoshiro\`。
3. 打开「控制面板 › 凭据管理器 › Windows 凭据」，删除以 `credentials.cn.icybit.hoshiro` 开头的条目。

**Linux**

1. 卸载：`sudo apt remove hoshiro`（deb）/ `sudo dnf remove hoshiro`（rpm），AppImage 直接删除文件。
2. 删除用户数据目录：`~/.local/share/cn.icybit.hoshiro/`。
3. 在「密码和密钥」（Seahorse）或 KWallet 管理器中删除 `cn.icybit.hoshiro` 条目。
