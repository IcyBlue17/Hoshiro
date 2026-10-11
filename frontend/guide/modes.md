# 输入模式与快捷键

Hoshiro 提供四种输入模式，默认围绕 <Kbd size="sm">fn</Kbd> 键及组合键展开。

::: tip Windows / Linux 默认按键
PC 键盘的 Fn 不经过系统，Windows 和 Linux 上由 <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Win</Kbd>（Linux 上叫 Super）承担 fn 的角色；下文中的 ⌘ 对应 Win / Super 键，⌥ 对应 Alt。

| 功能 | macOS | Windows / Linux |
|---|---|---|
| 口述 / 免提 | <Kbd size="sm">fn</Kbd> | <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Win</Kbd> |
| 翻译 | <Kbd size="sm">fn</Kbd> + <Kbd size="sm">Left Shift</Kbd> | <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Win</Kbd> + <Kbd size="sm">Left Shift</Kbd> |
| 随便问 | <Kbd size="sm">fn</Kbd> + <Kbd size="sm">Space</Kbd> | <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Win</Kbd> + <Kbd size="sm">Space</Kbd> |
| 重新粘贴 | <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">⌘</Kbd> + <Kbd size="sm">V</Kbd> | <Kbd size="sm">Alt</Kbd> + <Kbd size="sm">Shift</Kbd> + <Kbd size="sm">V</Kbd> |
:::

## 模式一览

| 模式 | 默认按键 | 说明 |
|---|---|---|
| 口述 | 按住 <Kbd size="sm">fn</Kbd>，松开结束 | 语音转文字并自动润色，结果插入当前光标处 |
| 免提 | 双击 <Kbd size="sm">fn</Kbd> 开始，再次按下结束 | 无需持续按键，适合长文本录入 |
| 翻译 | <Kbd size="sm">fn</Kbd> + <Kbd size="sm">Left Shift</Kbd> | 口述源语言，直接输出目标语言并插入 |
| 随便问 | <Kbd size="sm">fn</Kbd> + <Kbd size="sm">Space</Kbd> | 选中文本时就地改写；未选中文本时发起问答 |

## 口述

基础输入模式。按住 <Kbd size="sm">fn</Kbd> 键说话，松开后：

1. 音频发送至配置的语音识别服务完成转写；
2. 转写结果由大模型进行润色：过滤语气词与停顿、纠偏改口内容，并规范标点符号、数字与中英文空格排版；
3. 处理后的文本通过系统粘贴直接插入光标位置。

<div class="hs-demo">
  <Pill state="recording" elapsed="0:04" />
  <span class="cap">录音中：实时音量波形与计时</span>
  <Pill state="processing" />
  <span class="cap">处理中：云端识别与润色</span>
</div>

::: tip 单击防误触
为防止误触，轻按单点 <Kbd size="sm">fn</Kbd> 不会触发录音，仅提示长按使用。
:::

## 免提模式

双击 <Kbd size="sm">fn</Kbd> 锁定录音状态，松手即可自由口述；再次按下 <Kbd size="sm">fn</Kbd> 即可结束录音并触发转录上屏。亦可点击悬浮条上的完成按钮（■）或取消按钮（✕）。

## 语音翻译

按下 <Kbd size="sm">fn</Kbd> + <Kbd size="sm">Left Shift</Kbd> 开启翻译模式，口述源语言，松开后直接将目标语言文本插入光标处。悬浮条上方的指示标签显示当前目标语言，点击可快速切换。

目标语言可在「设置 › 语言」中配置。

## 随便问：改写与问答

按下 <Kbd size="sm">fn</Kbd> + <Kbd size="sm">Space</Kbd> 触发（悬浮条呈现紫色光晕）。系统依据当前光标状态执行不同处理：

<div class="hs-demo">
  <Pill state="recording" ask />
  <span class="cap">“随便问”模式录音状态</span>
</div>

- **已选中文本**：根据口述指令就地改写并替换当前选中文本。例如：
  - “改得更商务正式”
  - “翻译为英文”
  - “精简为一句话”
  - “整理为序号列表”
- **未选中文本**：将语音作为提示词发起问答，回答卡片悬浮于屏幕底部展示，支持 Markdown 渲染并可一键复制。

## 取消与撤销

- 录音或处理过程中按 <Kbd size="sm">Esc</Kbd> 可取消本次输入。
- 取消后 **5 秒内**，可点击悬浮提示卡片上的「撤销」恢复刚才的音频继续处理。

## 重新粘贴

快捷键 <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">⌘</Kbd> + <Kbd size="sm">V</Kbd>（Windows / Linux：<Kbd size="sm">Alt</Kbd> + <Kbd size="sm">Shift</Kbd> + <Kbd size="sm">V</Kbd>）：重新粘贴上一条处理后的文本。若因输入框失焦导致未能直接插入，可通过此快捷键直接补贴。

## 自定义快捷键

在「设置 › 快捷键」中进行配置。每项功能均支持绑定多个按键，并支持：

- 独立修饰键：如单独 <Kbd size="sm">fn</Kbd>、右 <Kbd size="sm">⌥ Option</Kbd>、右 <Kbd size="sm">⌘</Kbd>；Windows / Linux 上如右 <Kbd size="sm">Alt</Kbd>、<Kbd size="sm">Caps Lock</Kbd>
- 组合键：如 <Kbd size="sm">⌃</Kbd> + <Kbd size="sm">⌥</Kbd> + <Kbd size="sm">Space</Kbd>
- 鼠标功能侧键
