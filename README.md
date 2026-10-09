<p align="center"><img src="assets/icon-source.png" width="96" alt="Hoshiro"></p>

<h1 align="center">Hoshiro</h1>

<p align="center">按住 Fn 说话，松开就是一段写好的字。</p>

---

Typeless 额度总是不够用，所以干脆自己做了一个。操作习惯和 Typeless 基本一样，老用户可以直接上手，设置和历史记录也能一键搬过来。语音识别和 AI 改写都用你自己的 API Key，不走任何中间服务器。

## 能干啥

- **按住 Fn 说话**，松开自动插入到光标处；**双击 Fn** 进入免提模式，再按一次结束；Esc 取消（能撤销）
- **AI 自动整理**：去掉嗯啊、口误自动纠正（“周三，不对，周四” → “周四”）、自动分段和列表、中英文之间加空格
- **Fn + Left Shift 翻译**：说中文直接出地道的英文（或者别的语言）
- **Fn + Space 随便问**：选中一段文字说“改礼貌点”就地替换，不选就直接问问题
- **Ctrl + ⌘ + V** 再贴一次上一条
- 历史记录、个人词典、使用统计、悬浮条动画，该有的都有
- **从 Typeless 迁移**：快捷键、语言、偏好设置和本地历史一键导入

## 用的服务

| 干啥 | 支持 |
|---|---|
| 语音识别 | 小米 MiMo ASR（`mimo-v2.5-asr`，国际 / 国内节点都行） |
| AI 改写 | Google Gemini、Anthropic Claude、任何 OpenAI 兼容接口 |

后面会加更多。

## 下载

去 [Releases](../../releases) 下对应的包：

- **macOS**：`.dmg`（Apple 芯片选 `aarch64`，Intel 选 `x64`）
- **Windows**：`.exe` 或 `.msi`
- **Linux**：`.AppImage` / `.deb` / `.rpm`

> macOS 的包没做公证，第一次打开如果提示“已损坏”，在终端跑一下：
> ```bash
> xattr -cr /Applications/Hoshiro.app
> ```
> 然后按引导给 **麦克风**、**辅助功能**、**输入监控** 三个权限。顺便把「系统设置 › 键盘 › 按下 🌐 键时」改成「不执行任何操作」，不然 Fn 会弹表情面板。
>
> 如果 Typeless 还开着，记得先退掉，两个会抢 Fn。

## 平台进度

- **macOS**：全部功能可用
- **Windows / Linux**：界面、设置、历史这些能用，但全局快捷键和自动粘贴还在做，暂时没法正经用来打字

## 自己编译

需要 [Bun](https://bun.sh) 和 Rust。

```bash
bun install
bun run tauri dev      # 开发
bun run tauri build    # 打包
```

只改界面的话，`bun run dev` 然后打开 `http://localhost:1420`（悬浮条看 `?window=bar&demo=1`），浏览器里有假数据可以直接看效果。

项目结构和实现细节见 [SPEC.md](SPEC.md)。
