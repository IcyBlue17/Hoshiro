# 常见问题

## 按 Fn 键弹出系统表情面板或切换输入法

打开 macOS「系统设置 › 键盘」，将「按下 🌐 键时」选项调整为 **「不执行任何操作」**。

## 按住快捷键无响应

1. 检查「设置 › 权限」中 **麦克风、辅助功能、输入监控** 三项系统权限是否均已获得授权（Windows 只需麦克风；Linux 见下文 [Linux 使用说明](#linux-使用说明)）。
2. 确认未同时运行 Typeless（两者均通过底层的系统事件钩子监听按键，同时运行会发生冲突）。
3. 检查系统菜单栏（Windows：任务栏右下角的通知区域，可能收在 ^ 里；Linux：顶栏或面板的托盘区，GNOME 需要 AppIndicator 扩展）是否存在 Hoshiro 图标，若不存在请重新启动应用。
4. Windows：如果只在某个程序里无效，看看它是不是以管理员身份运行的（见下文）。

## 输入完成后未插入文字，弹出「复制」提示

当前输入焦点未处于可编辑的文本框内。可点击目标输入框获取光标后再试；亦可直接点击提示卡片上的「复制」按钮，或使用快捷键 <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">⌘</Kbd> + <Kbd size="sm">V</Kbd>（Windows / Linux：<Kbd size="sm">Alt</Kbd> + <Kbd size="sm">Shift</Kbd> + <Kbd size="sm">V</Kbd>）手动粘贴。

## 提示「改写失败，已插入原始转录」

改写服务请求失败。常见原因包括：API Key 填写错误、账户额度耗尽、选定的模型名称不存在或无访问权限。请前往「设置 › 模型服务」点击「测试连接」查看具体的错误返回码及响应信息。

## 输入整体延迟较高

请前往「使用统计 › 速度」查验各阶段耗时分布，具体分析建议参见 [性能排查](./statistics#速度分析与性能排查)。

## 系统钥匙串授权提示频繁弹出（macOS）

系统弹出「Hoshiro 想要使用钥匙串中的机密信息」授权对话框时，请选择 **「始终允许」**。当应用进行版本更新且签名发生变化时，系统可能会重新请求确认一次。

## 打开时提示应用“已损坏”（macOS）

在终端中执行以下命令清除文件隔离标志：

```bash
xattr -cr /Applications/Hoshiro.app
```

## Windows：安装时提示“Windows 已保护你的电脑”

安装包没有代码签名，点击「更多信息」›「仍要运行」即可。

## Windows：在某些程序里快捷键或输入无效

以管理员身份运行的程序（如部分游戏、安装程序、任务管理器）不接受普通权限程序发送的按键。需要在这类程序里使用时，右键以管理员身份运行 Hoshiro。

## Windows：为什么默认快捷键不是 Fn？

PC 键盘上的 Fn 由键盘固件处理，按下时 Windows 根本收不到，因此默认使用 <Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Win</Kbd>。可在「设置 › 快捷键」中改为右 Alt、Caps Lock 等单键。从 Mac 导入的 Fn 快捷键会自动换成 Windows 默认值。

## Linux 使用说明

Linux（x64，X11 / Wayland）已正式支持。Hoshiro 从 `/dev/input` 读取键盘、通过 `/dev/uinput` 发送粘贴快捷键，需要一条 udev 规则授予当前桌面用户访问权限。deb / rpm 安装包会自动安装；AppImage 在「设置 › 权限」点「授权」即可。也可以手动安装：

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

已知限制：

- 快捷键只监听、不拦截：组合键里的普通键（如空格）和录音时按的 Esc 也会传给当前应用，所以默认快捷键只用修饰键（<Kbd size="sm">Ctrl</Kbd> + <Kbd size="sm">Super</Kbd>）。
- Wayland 会话下通过 XWayland 运行，以便悬浮条置顶并固定在屏幕底部；原生 Wayland 应用的名称与图标无法识别，不影响输入。
- 粘贴按的是 V 键的物理位置，Dvorak 等非 QWERTY 布局下可能失效。
- 「根据修改自动添加词典」在 Linux 上不可用。

## 问题反馈

如遇其他未列出的异常，欢迎在 [GitHub Issues](https://github.com/IcyBlue17/hoshiro/issues) 中提交反馈。
