# Linux 系统无障碍与窗口验证

本切片通过 AccessKit/winit 的真实 Linux AT-SPI 总线桥接，不把框架内部语义树称为系统支持。节点保留稳定、带世代的身份；移除节点撤销旧动作，文本值通过 TextRun 暴露给系统读取接口；角色、名称、勾选、只读、不可用、焦点及编辑动作来自同一活语义快照。

## 实测边界

2026-11-14，Ubuntu 26.04.1 amd64、Linux X11/Xwayland、NVIDIA RTX 2060 / 595.99.02。隔离 dbus-run-session + 私有 Weston 14 的 x11 backend/Xwayland；Weston GL 明确使用 `/dev/dri/renderD129` 和 NVIDIA renderer，应用 wgpu Vulkan 同为 DiscreteGpu。未改变宿主输入法或全局设置。原生 Wayland 仍实验性，Windows/macOS 未获资格。

实际 AT-SPI 客户端 16/16：按钮/勾选/文本角色，执行勾选并读取改变后状态，系统请求焦点，Unicode 编辑与系统读回，只读拒绝写入且值不变，不可用状态，真实座席 Tab 转移焦点，WM_DELETE_WINDOW 正常退出及系统树移除。两项 Rust 桥接回归覆盖物理坐标/Unicode/动作与移除重用后的旧请求拒绝。

早期 headless Weston 没有输入座席，XSetInputFocus/EWMH 不能建立合成器真实焦点；该失败保留，不以延长等待或伪造 Focused 修复。使用嵌套 x11 backend 的实际座席，外层 XTEST 经 Weston 送入私有 Xwayland，解决焦点和键盘验证问题。headless/pixman 不能替代硬件资格。

## 复验

离线构建 `cargo build --offline --locked --example themes_controls`。系统需 Python GI/Atspi、dbus-run-session、xdotool、Xwayland，以及完整私有 Weston 包根；脚本不下载或安装依赖。对正在使用的 Linux 桌面仅新建并关闭自有嵌套窗口。

```sh
python3 scripts/isolated_xwayland.py --nested-x11 --renderer gl \
  --root /absolute/private/weston-root --output .local/verification/compositor-new \
  -- dbus-run-session -- /usr/bin/python3 scripts/verify_accessibility.py \
  --app /absolute/target/debug/examples/themes_controls \
  --output .local/verification/atspi-new --existing-isolated-display
```

输出目录必须不存在，失败也写出已完成断言；异常清理自有进程。唯一 Weston 窗口无法识别时拒绝键入。原始证据位于本候选 `.local/verification/M6B-{private-hardware,atspi-hardware}`；最终四门禁日志 `M6B-final-{fmt,clippy,test,build}.log`。

M3C 的有限 Fcitx5/X11 输入法双 DPI 资格及 IBus 三项已知失败仍见 [原生 IME](native-ime.md)，本切片不冒充重新取得通用 IME、屏幕阅读器完整产品或全平台资格。
