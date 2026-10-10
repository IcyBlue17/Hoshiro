<p align="center"><img src="assets/icon-source.png" width="104" alt="Hoshiro"></p>

<h1 align="center">Hoshiro</h1>

<p align="center">基于 Tauri 2 的语音输入法，只需按住 Fn 说话，松开，整理好的文字自动插到光标处。</p>

<p align="center">
  <a href="../../releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/IcyBlue17/hoshiro?style=flat-square&label=%E6%9C%80%E6%96%B0%E7%89%88&color=5b7cfa"></a>
  <a href="../../releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/IcyBlue17/hoshiro/total?style=flat-square&label=%E4%B8%8B%E8%BD%BD%E9%87%8F&color=5b7cfa"></a>
  <img alt="macOS 13+" src="https://img.shields.io/badge/macOS-13%2B-000?style=flat-square&logo=apple&logoColor=white">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/github/license/IcyBlue17/hoshiro?style=flat-square&label=%E8%AE%B8%E5%8F%AF%E8%AF%81&color=3da639"></a>
  <a href="https://app.fossa.com/projects/git%2Bgithub.com%2FIcyBlue17%2Fhoshiro?ref=badge_shield"><img alt="FOSSA Status" src="https://app.fossa.com/api/projects/git%2Bgithub.com%2FIcyBlue17%2Fhoshiro.svg?type=shield"></a>
</p>

<p align="center">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-000?style=flat-square&logo=rust&logoColor=white">
  <img alt="React" src="https://img.shields.io/badge/React-19-149ECA?style=flat-square&logo=react&logoColor=white">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-3178C6?style=flat-square&logo=typescript&logoColor=white">
  <a href="../../stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/IcyBlue17/hoshiro?style=flat-square&color=f5b301"></a>
  <a href="../../commits/main"><img alt="Last commit" src="https://img.shields.io/github/last-commit/IcyBlue17/hoshiro?style=flat-square&label=%E6%9C%80%E8%BF%91%E6%9B%B4%E6%96%B0"></a>
</p>

<p align="center"><img src="docs/images/home.png" width="760" alt="Hoshiro 主界面"></p>

操作和 [Typeless](https://www.typeless.com) 基本一致，设置和历史记录可以直接导入。语音识别和润色完全 Bring your own key，请求直接发给你选的服务商，不经过中间服务器。

## 功能

### 说完就是成稿

- 自动去掉“嗯、啊、那个”这类口癖和思考时的停顿
- 说错了改口，只留最后的说法：“周三，不对，周四” → “周四”
- 数字写成 123，中英文之间自动加空格
- 一口气说一大段，可以按话题分段，也可以整理成 1. 2. 3. 列表

### 悬浮条

录音时屏幕底部出现一条黑色小胶囊，白色波形跟着声音跳，处理时有扫光动画，不抢焦点、不挡内容。

<p><img src="docs/images/bar-recording.png" height="54" alt="录音中">&nbsp;&nbsp;<img src="docs/images/bar.png" height="120" alt="翻译模式与提示"></p>

### 翻译与随便问

- **翻译**：`Fn + Left Shift` 说中文，直接输出地道的英文或其他语言。
- **随便问**：`Fn + Space`。选中一段文字说“改礼貌点”会原地替换；没选中就直接提问。

### 越用越懂你

自动学习你的常用词、口癖和纠错习惯，下次就不会再错。学到了什么都摆在明面上，可以逐条查看和删除。

### 按应用看你的使用习惯

这是 Hoshiro 最有意思的地方之一：每一次口述都会记下发生在哪个应用里，于是你能清楚看到自己在 Claude、ChatGPT、QQ、Telegram 里分别“说”了多少字、各占多少比例，哪个应用最离不开语音输入一目了然。

不止于此，耗时会拆到语音识别、改写、上屏每一段，平均值、中位数、P90 都有，换个服务商快了还是慢了，看一眼就知道；成功、取消、无声各多少次，各种模式用了多少，平均每次说多少字、说多久，也都算得明明白白。所有统计只在本机计算，不上传任何地方。

<p align="center"><img src="docs/images/usage.png" width="760" alt="使用统计：速度、结果、使用方式和常用应用"></p>

## 支持的服务

| 用途 | 服务商 |
|---|---|
| 语音识别 | 小米 MiMo（中文推荐）、Cloudflare Workers AI（Whisper）、Azure Speech（流式）、Google Cloud Speech（流式） |
| AI 润色 | Google Gemini、Anthropic 兼容接口、OpenAI 兼容接口 |

配置方法见[使用说明](docs/guide.md#配置服务商)。

## 安装

从 [Releases](../../releases/latest) 下载 `.dmg`，Apple 芯片选 `aarch64`，Intel 选 `x64`，拖进「应用程序」。之后会自动检查更新。

若首次打开提示“已损坏”或“无法验证开发者”时，在终端执行：

```bash
xattr -cr /Applications/Hoshiro.app
```

## 上手

1. 打开 Hoshiro，按引导授予麦克风、辅助功能、输入监控三个权限。
2. 在「设置 › 模型服务」填一个语音识别 Ke 和一个改写模型 Key。
3. 把「系统设置 › 键盘 › 按下 🌐 键时」改成「不执行任何操作」，否则 Fn 会弹出表情面板。
4. 点进任意输入框，按住 Fn 说话，松开，体验你的新 3 输入法

| 快捷键 | 作用 |
|---|---|
| 按住 `Fn` | 说话，松开后插入 |
| 双击 `Fn` | 免提模式，再按一次 `Fn` 结束 |
| `Fn` + `Left Shift` | 翻译 |
| `Fn` + `Space` | 随便问 |
| `Esc` | 取消（可撤销） |
| `Ctrl` + `⌘` + `V` | 再贴一次上一条 |

快捷键可以在设置里改，一个功能也能绑多个键。更多内容和常见问题见[使用说明](docs/guide.md)。

## 从 Typeless 迁移

在引导页或「设置 › 数据与迁移」点「从 Typeless 导入」，快捷键、语言、偏好和本机历史会一起导

Typeless 的词典在云端，无法自动导入。可以整理成单列 CSV，在「词典」页导入。

## 平台

| 平台 | 状态 |
|---|---|
| macOS 13+ | 完整可用 |
| Windows / Linux | 支持中 |

## 开发

Tauri 2(Rust)+ React。环境、目录结构和提交约定见 [CONTRIBUTING.md](CONTRIBUTING.md)。问题和建议请开 [Issue](../../issues)。

## 许可证

[MIT](LICENSE)

[![FOSSA Status](https://app.fossa.com/api/projects/git%2Bgithub.com%2FIcyBlue17%2Fhoshiro.svg?type=large)](https://app.fossa.com/projects/git%2Bgithub.com%2FIcyBlue17%2Fhoshiro?ref=badge_large)

Hoshiro 是独立项目，与 Typeless 及文中提到的服务商无关。
