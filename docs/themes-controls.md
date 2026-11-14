# 语义主题与有限控件

## 范围与入口

统一使用 [主题方向](theme-direction.md) 的初始色值，不使用官方素材、字体或角色资产。实际状态只在 [Alpha 状态](alpha-delivery-status.md) 维护。示例 `cargo run --example themes_controls --locked`：同一保留树、双字段/错误字段、主次按钮、复选框、keyed 可折叠树、滚动、必要连接线和真实 M1c 工作线程。不是完整工作台、CSS/动画/编辑器平台。

`ThemeController::new(ThemeMode::Light)` / `set(ThemeMode::Dark)` / `signal()` 是 UI 线程入口；传给 `Application::theme(&controller)` 显式启用。新主题默认 Light；现有未调用 `.theme` 的应用保留 M4A 默认文字、clear、选择及焦点色，原视觉参考 fixture 无改动。新 `button/secondary_button/checkbox/tree_node` 自带 Light 有限样式；要让整窗、普通 `text`、旧 TextInput 和新控件一起切换，必须安装同一控制器。无 OS 自动主题。

`SemanticTheme::color(ThemeColor)` 集中 background/surface/surface-raised/text/muted/border/primary/on-primary/accent/on-accent/focus/error/success/selection。`Element::theme_color/theme_background/connector` 直接引用角色；显式 `.color/.background/.border` 仍覆盖默认，不能保证应用自己覆盖后的可读性。主题无字体/度量 token，两模式共享字号、整形与布局合同；调用原 `.font_size` 的节点仍走既有挂载测量，不引入可变字体主题。

## 状态与失效合同

- 主按钮正常/hover 保持 primary + on-primary；hover 加粗 muted 内边界，不提亮红色。press 使用 selection + on-primary；禁用使用 surface-raised + muted。次按钮、输入、树标题、复选框使用 surface + text，hover surface-raised，press/selected 使用 selection。显式 error 状态以 error 内边界及示例 `!` 错误文字表达，不把品牌红当错误标签。
- 必要边界、复选形状和连线用 muted/text，不用装饰 border；焦点为 2 逻辑像素 focus 内环，沿有效 clip 与 bounds 的交集绘制，整个控件充满/超出视口时仍在视口内可见。选择用底色，复选用方形内标记，展开用加/减号；不是单靠色相辨识。
- theme generation 只在最终 mode 改变时推进。主题与 selected/expanded/invalid 的颜色状态更新只标记 paint；不改 text resource version，不整形、布局或 placement。结构/文字/尺寸更新仍保留原几何与资源版本合同。颜色继续为 encoded-sRGB / straight alpha，clear 只在 GPU 接口处解码一次，glyphon 继续 Accurate。
- 不重建节点、编辑模型或接收器，不重置 Signal、历史、方向选区、插入点、滚动、展开、NodeId、活动消息或 native composition owner/session。普通点击主题按钮按输入默认策略可以移动焦点；F6 是保焦主题处理器，用它验证精确保留，而不声称按钮点击不会聚焦。

## 控件与活语义边界

`Element::label/button/secondary_button/text_input/checkbox/tree_header/tree_node` 返回 Element；`scroll_view` 与 `keyed_column` 沿用已交付组合。checkbox 使用 `Signal<bool>`，tree_node 使用展开 Signal 并保留隐藏子树；可用 tree_header + visible 内容组合需要 selected_when 的树标题（示例即此模式），选中另加侧条，不与 focus 混用。checkbox 的 selected/value 来自核心值，不允许再用 selected_when 改绑；误用明确 panic，不悄悄形成双重真值。鼠标、Enter、匹配 Space 松开与语义 Activate 都调用同一核心 activate：先切换值，再执行应用 `.on_click`。on_click 替换应用回调，不覆盖核心切换；stop_propagation 不取消默认动作，prevent_default 是明确取消本次原生默认动作的策略。应用回调若主动再次写同一 Signal，最终值仍以应用写入为准。disabled（含祖先禁用）拒绝动作；readonly 拒绝修改/激活，但可以聚焦并复制输入内容。

`Application::semantics(&Semantics)` 是可供应用/测试连接的框架内部活 UI producer/consumer，不是冻结的 OS adapter ABI；不暴露 Scene。每个正式 flush 在结构/属性、动作、布局/scroll/focus/caret 后发布 snapshot；`snapshot()` 是最新提交，`take_delta()` 合并上次读取以来的 changed/removed，先删 removed 再更新 changed 即可。无变化不推进语义 version；只改主题不会伪造语义变化。

未安装 handle 的旧应用不构造语义副本。安装后只保留当前与上次 take_delta 的两个快照，不积累每个临时挂载 ID；同 ID 隐藏再恢复且最终值相同可合并为无变化 delta，但 version 保留期间的提交进度。克隆 handle 共享一个 delta 消费游标，snapshot 可多读；每窗安装一个 handle，由一个 adapter 消费 delta。语义新增测试先复现了无请求时双 sync 破坏 M1b 有限命令边界，随后恢复每次 flush 的原单次命令快照；动作各自验证后的模型同步与新 formatter 命令的下一边界政策分开。

节点包含真实 role/name/committed value、disabled/focusable/readonly/focused/selected/expanded/invalid、parent/children NodeId、窗口逻辑 bounds 和可用 actions。默认 TextInput name 为空，必须用 accessible_name 指定标签；其 value 是编辑模型已提交文本，不是 preedit 显示字符串；checkbox value 为核心布尔值的 true/false 字符串，同时提供 selected。隐藏子树不发布；挂载但屏外保留，并支持 Focus/ScrollIntoView；隐藏/恢复保留 ID，卸载重挂新 ID。父子顺序来自实际 keyed 树；tree_header 与可见内容在真实组合的 Group 下保持关联，不冒充完整 ARIA Tree/OS TreeView 键盘模型。控件 snapshot/action 是真实通路，不宣称 OS 无障碍资格。

`perform(id, SemanticAction)` 排队、唤醒同一 Application，返回 receipt；正式提交重新检查完整 ID、当前 visibility/disabled/readonly。Activate/Expand/Collapse 使用同一控件核心，Focus 使用原焦点/揭示路径；SetValue 使用同一规范化/历史编辑收尾，preedit 活动期间拒绝替换，非程序替换历史屏障。动作之后发布新快照。示例 F8 真正读取该边界并排队聚焦只读字段；F7 聚焦大于裁剪视口的按钮。每个 handle 只接一个 Application；跨线程 OS adapter 留 M6B。

默认不打印字段或语义值。示例 `--verify` 才开启 F9 受控 fixture 明文 probe；`CHRONOBREAK_TRACE` 是既有显式诊断开关，不能对私人输入默认启用。snapshot 自身必然包含用户文字，调用者须按 UI 数据处理，不作遥测。AccessKit、系统读屏、平台 action qualification 均未在本切片完成。

## 预登记验证规则

以下规则先于首次图像采集固定；不降低 M4A 任何旧阈值：

- 全部三 DPI 1 / 1.5 / 2，Light/Dark 和 hover/press/focus/checked/disabled/error/selection/scroll 实际 surface。flat interior 与实心 glyph 像素每通道容差 **2/255**；每文字 crop 至少 8 个符合预期前景的实心像素，另保留实际抗锯齿边缘，不拿边缘的混合像素代替文字前景。
- 在图像中取真实底色和 glyph 实心前景，按 WCAG 相对亮度计算，普通/辅助/按钮/错误/成功文字 **≥4.5:1**；必要输入边界、连接线、复选标记与 focus 相邻色 **≥3:1**。各色先匹配已登记 token，再测合成像素比值，不仅计算 token 表。
- 稳定 NodeId/几何、真实键鼠编辑、外部进程读取 copy 的选区前后逐字相等；F6 后仍可 undo/redo、展开/滚动仍保留、工作线程继续并完成。composition owner/epoch/preedit 与 paint-only 精确计数由单元测试验证，不操纵主机 M3 输入法，也不替代 M6B 原生系统复验。

## 复验入口与依赖

使用系统 `Xvfb`、`xdotool`（XTEST）、`fc-match`、Python 3 与 Pillow，以及可用的 Vulkan wgpu surface。仅私有 X server，不继承主机 DISPLAY/Wayland/DBus/provider，不安装/更改输入法。外部剪贴板 reader 使用 tracked `examples/ime_clipboard_reader.rs`；缺依赖/reader/app 必须 exit 2，不允许 skip，Python `-O` 不关闭检查。

```sh
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
python3 scripts/themes_controls/verify.py --app target/debug/examples/themes_controls --reader target/debug/examples/ime_clipboard_reader --output .local/verification/themes-gui
```

原始二进制、图片、操作、crop、源/二进制 hash、清理结果与 clean tracked archive 复验均留 feature worktree `.local/verification/`，不提交产物。最终门禁结果与已知局限随本切片证据更新。许可已选 MIT OR Apache-2.0，文件落地属 M7，本切片不添加第三方资产或声称许可文件已交付。
