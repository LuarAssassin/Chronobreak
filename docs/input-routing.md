# M2a 输入路由、焦点与捕获

2026-11-13 的独立候选切片，分支 `feat/input-routing`，工作树 `.worktrees/input-routing`，基线 `main@13b5be5aadba3b1ab89ee02745541100859a32e0`。M1a/M1b/M1c 已分别合入 PR #6/#7/#8；本切片待独立审查及远端 CI，不表示 M2 已完成。M2b 的滚动、滚轮、offset 与 `scroll_into_view`，以及 M3 的编辑/IME 均未实现。

## 公共入口

```rust
use chronobreak::{Element, EventKind, EventPhase, MouseButton, Signal};

let disabled = Signal::new(false);
let control = Element::text("可聚焦控件")
    .on_click(|| println!("唯一默认激活"))
    .disabled_when(&disabled)
    .on_event(EventPhase::Target, |event, context| {
        if matches!(event.kind, EventKind::PointerDown(MouseButton::Left)) {
            context.capture_pointer();
            context.stop_propagation();
        }
    });
let pointer_only = Element::text("不进 Tab 序").on_click(|| {}).focusable(false);
let focus_only = Element::text("仅焦点区域").focusable(true).disabled(false);
```

- `.on_event(phase, Fn(&UiEvent, &mut EventContext))` 可多次注册，同节点同阶段按注册顺序调用。闭包可更新 Signal、排队 Children 命令，不得到 Scene、Renderer 或 arena 可变引用。
- `UiEvent.kind` 为指针 down/move/up/enter/leave/cancel、CaptureLost 或 `Key(KeyInput)`；`position` 是逻辑窗口坐标 `Option<(f32, f32)>`，未知则为 `None`，不编造离窗坐标。键盘目标不由此坐标决定。
- `EventContext` 提供 `target/current_target/phase`、独立 `stop_propagation()/prevent_default()` 及查询、`capture_pointer()/release_pointer()/focus()`。捕获与焦点请求在路由后应用；同一路由后请求覆盖前请求。显式请求不是默认动作，`prevent_default` 不撤销它们；若显式聚焦要覆盖随后指针默认聚焦，同时阻止默认动作。
- `KeyInput` 保留 winit 的 logical/physical key、location、state、repeat、synthetic；修饰键在 `UiEvent.modifiers`。根导出 `keyboard`、`ElementState`、`MouseButton`，不把所有键伪装为文本。应用消费没有另设含糊的布尔返回值，传播与默认行为互不混用。
- `.disabled_when(&Signal<bool>)` 挂载读取最新值，后续按属性最终值提交；优先于静态 `.disabled(bool)`。父禁用排除整个后代的输入/Tab，保持布局与绘制；禁用区域不透明穿透到可点击祖先。

## 路由与默认动作

每个输入先复用 M1b `flush`，完成有限结构命令快照、属性最终值与必要布局，之后基于最新逻辑 bounds/clip 命中。指针命中实际最深节点，即使它只是可点击父级内的文字。路径固定为：根至目标父级 Capture → 目标 Target → 父级至根 Bubble；目标不会额外执行 Capture/Bubble。键盘路径来自 focused NodeId；没有焦点时根为 target，根作用域需按此选择 Target 或 Capture 监听。

`stop_propagation` 在当前节点同阶段的全部监听器结束后停止后续路径，不跳过同节点后注册监听器，也不阻止默认动作。`prevent_default` 不停止路径，后续监听器可观察该标志。不同事件各有独立标志；仅当前事件的标志影响当前默认动作。

| 输入 | 路由后的默认动作 |
| --- | --- |
| 左键按下 | 记录最近可点击祖先作为 pressed 激活候选；聚焦命中路径最近的可聚焦节点，没有则清焦点。阻止默认动作同时阻止这两项 |
| 左键松开 | 仅 pressed 与当前真实命中的最近可点击控件一致且本次未 prevent 时激活一次；捕获不改变这一判据。松开在控件外/未知坐标不激活 |
| 其他鼠标按钮 | 正常路由，不默认聚焦/激活。最后一个按键释放时自动释放捕获 |
| Tab / Shift+Tab 按下 | 有效树的先序顺序正向/反向循环；隐藏、禁用及显式 opt-out 跳过；初次正向首项、反向末项，空树无动作 |
| Enter 按下 | 仅激活 focused 控件自身的 on_click，不逐级激活可点击祖先 |
| Space 按下/松开 | 按下绑定 focused 完整身份及物理键；匹配松开且仍为同一焦点才激活一次。转移焦点（包括转走后回来）、失效或 preedit 开始即取消 |

Enter/Space/Tab 默认动作忽略 OS repeat、重复物理按下与 synthetic 事件；Ctrl/Alt/Super 修饰组合不执行这些默认动作。路由监听器仍可看到重复键并自行定义快捷键重复策略。Space 松开被 prevent 也会清掉待激活项。默认激活和路由位于同一框架 batch；结构/可见性/禁用写入到正式提交才改变遍历树，不能靠排队隐藏或删除阻止本次尚未执行的默认动作，应显式 `prevent_default()`。

`on_click` 继续是无参 `Fn()`，鼠标与键盘复用此唯一默认激活入口；不再把它当路由监听器，也不把返回值当 redraw 请求。嵌套可点击控件只激活最近一个，不同时激活祖先。自定义监听器若自行执行业务激活，应 prevent 对应默认动作，避免应用自己产生双计数。

## 焦点、反馈与生命周期

clickables 默认成为 Tab 停靠点；`.focusable(false)` 明确退出（仍可鼠标激活），`.focusable(true)` 可用于非点击区域。单窗口/单根焦点作用域，无 modal、多窗口或全局命令注册表。重排保持身份和焦点；已提交隐藏/禁用/卸载焦点目标后，确定回退当前树序首个有效停靠点，无候选则清空，不保留失效身份等待复活。

框架拥有 hovered 路径、pressed、capture、focus、Space 待激活身份。focus 用金色、hover 用灰色的四条 **控件边界内** 矩形绘制；仍受祖先 clip 约束。启用的 clickable hover 或捕获时使用 Pointer cursor。纯焦点/hover 只设置呈现意图，不标记文字测量脏；示例自身日志/计数文字变化仍可能真实布局。这只是基本反馈，色值未来接主题 tokens，不是冰青/炽红主题系统。

| 边界 | 清理规则 |
| --- | --- |
| 有效捕获 | 按键按住期间可从指针 down/move/up 路由请求捕获 current_target；移动/松开路由至捕获路径，即使离开控件；显式 release 可来自键盘作用域 |
| `CursorLeft` | 清 hovered 与已知坐标；无捕获则取消普通按压，有效捕获则保留等待后续窗口事件。不是 OS-global capture 承诺 |
| 显式释放 / 最后鼠标键松开 | 有效旧目标收到 CaptureLost；释放本身不隐式取消尚存的普通 pressed 候选，但仍要求有效的原控件内松开 |
| 失焦 / suspend / 系统 touch cancel / 尺寸或 DPI 变化 | 取消指针手势并释放捕获；有效目标收到 PointerCancel、CaptureLost。失焦/suspend 另清 focus、hover、Space、held keys 与 modifiers；尺寸/DPI 改变保留有效焦点 |
| 已提交隐藏/禁用/卸载 | 清对应交互身份，焦点确定回退；失效节点不再收到清理回调，不对已移除/复用代际派发旧消息 |
| 几何/结构提交 | 必要布局后按当前位置重算 hovered；有效离开节点深至浅、进入节点浅至深，各作为独立完整路由。祖先可看到多个边界通知，使用 target 区分，不误认为单个 PointerMove 的重复路由 |

`Children` 命令仍按 M1a/M1b 边界提交；监听器排队删除不会截断当前路径，当前 bubble 仍可执行。已提交删除后，即使相同 key 或 arena 槽位重用，旧 pressed/capture/focus/Space 不能落到新节点。无效目标清理不再调用其用户闭包；显式/system 取消只通知仍有效目标。这是新增路由清理合同，不删除旧取消回归。

窗口后端把 CursorMoved、所有鼠标按钮、KeyboardInput、ModifiersChanged 接到同一 flush/批次/呈现路径；DPI 变化重投影已有物理指针。不调用 `set_cursor_grab` 或限制/挪动真实桌面指针。winit/窗口系统没有交付窗口外事件时，框架不能补造它们；本次 X11 隐式抓取实际交付了窗口外 move/up，不能外推至 Wayland、所有桌面或其他平台。

仅对已收到的非空 `Ime::Preedit` 设置短暂组合保护：暂缓键路由、撤销 Space 待激活，空 preedit/commit/disabled 结束保护。不启用文本编辑器、候选定位、selection、输入法生命周期或完整跨平台键优先级；未收到 preedit 的平台行为没有保证，M3 必须真实输入法另验。

## Alpha 迁移

- 原 `.on_click` 源码不用改；新增默认 Tab 停靠、指针默认聚焦、Enter/Space 激活。不要继续用业务 Signal 手动模拟框架焦点；需仅指针行为时加 `.focusable(false)`。
- 原“离窗一律取消”改为“无捕获取消、有捕获保留”。原拖出释放不激活、失焦/尺寸/DPI/隐藏/卸载取消，以及后台消息取消时序继续保留。
- 属性更新仍非遍历中的即时树修改；回调排队 remove/disabled 不等价于 prevent_default。winit 类型重导出属于 Alpha 公共 API，未来升级需同步迁移。
- 未删除既有测试；仅为内部 Update 测试字面量补 `disabled: None`。M1a/M1b/M1c 的通知、取消、同 key、资源、批次、公平性、Retry/occlusion 等回归保持。

## 验证与证据

全部中间文件只放本工作树 `.local/verification/`，忽略且不提交。实现先以 `07295ded8e6d105865dabdd7006b4d7544a01f8f` checkpoint 落地；最终追加未知坐标使用 None 的修正，并对最终二进制重放相同短图形流程，早期证据保留在 `core/`。原始请求见 `replay-requests.json`，执行器 `gui_probe.py/replay_gui.py`，逐动作、实际 pid/window、几何、最后 presented frame、PNG 哈希见 `gui-actions.json`；完整文件哈希见 `evidence-sha256.json`。本地证据不能当作远端 CI 通过。

四项原样 gate 全通过，日志为 `gate-fmt.log`、`gate-clippy.log`、`gate-test.log`、`gate-build.log`：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

**87 unit + 4 doc 全通过**：其中新增 20 项输入回归、1 项后端 DPI 坐标回归，保留原 66 unit + 4 doc。覆盖精确三层 trace、target/current_target、stop/prevent 独立、延迟结构提交、捕获 move/up/离窗/显式释放/失效、隐藏禁用父级、hover 几何重算、Tab/反向/回退/重排、Enter/Space/OS repeat/焦点转移、键与修饰载荷、可见焦点且不重排、无坐标释放及旧代际。没有重跑千帧 GUI 压测；既有 1,000 次挂卸的逻辑测试仍随完整 suite 执行。

实际环境：隔离 Xvfb 1600×1400×24，无窗口管理器；xdotool XTEST 输入，Pillow XCB 截图；llvmpipe LLVM 21.1.8 / Mesa 26.0.8 / Vulkan；`WINIT_X11_SCALE_FACTOR=1.0/1.5`。每次根据当前 pid 查询窗口并读取几何，宽屏 `(60,40,760,760)` 或 `(60,40,1140,1140)`，窄屏 400×900 或 600×1350；PNG 从整个 Xvfb 帧精确裁到实际应用窗口，稳定 presented 后才取图。没有把窗外黑桌面或标题当绘制缺陷。

| 两个 DPI 均实际执行的动作 | 结果 / PNG 后缀 |
| --- | --- |
| Tab，Enter，持 Space 1.1 秒后松开 | A 0→1；OS repeat=true，松开前仍 1，松开后仅 2；`keyboard-focus` |
| Shift+Tab | A 反向循环到拖动区域，金色焦点可见；`reverse-tab-drag-focus`；两次 presented 的 layouts 均 7，仅焦点变化未重排 |
| Tab 到禁用切换，Enter；Shift+Tab，Enter | B 被跳过，A 总计 3；`disabled-skip` |
| 恢复启用，聚焦 B，Enter；Ctrl+R，Enter | B 激活总计 4；有真实 logical Character("r")、physical Code(KeyR)、CONTROL 载荷；B 删除后回退 A，总计 5；`removal-fallback` |
| 持 Space，Tab 转移后松开 | 总计保持 5，没有激活新焦点；`space-transfer-cancel` |
| 重建 B、隐藏、反向 Tab；恢复显示再激活 | 隐藏时跳过 B，A 总计 6；恢复后 B 总计 7；`hidden-skip/restored-b` |
| 点 A 的文字；切换 stop；再切换 prevent | 普通一次到 8，stop 仍到 9，prevent 保持 9 且焦点留原处；`pointer-route/stop-keeps-default/prevent-cancels-default` |
| 捕获区域按住，移到窗口右外再松开 | 1.0 DPI 真实逻辑 `(890,560)`、1.5 DPI `(826.6667,573.3333)` 的 move/up 均到 drag；CaptureLost 后释放；`capture-outside-window/capture-released` |
| 捕获中 Esc；再捕获并把窗口焦点移到 Xvfb root | 分别显式 CaptureLost、PointerCancel+CaptureLost；`capture-explicit-release/focus-loss-cancel` |
| A 按下拖出释放；随后回到 A 正常点击 | 拖出总计仍 9，恢复输入才到 10；`button-drag-out-cancel` |
| 缩窄 / 恢复；无输入等待两秒 | 中文真实换行、焦点边框在界内，计数保持 10；`narrow-chinese/idle-recovery`；无连续空闲帧 |

`background_probe.py` 另对既有 M1c 示例执行无 burst 的短回归：初始 1→1 帧休眠；后台 step=3 时 Tab×6 + Enter 响应计数到 1；后台显示 10/10，完成后 19→19 帧；WM_DELETE_WINDOW 正常退出 0，`closed=true late_closed=true`。证据 `background-actions.json`、`background-runtime.log`、`background-completed.png`，不把慢速真实工作线程回归外推为洪峰性能基准。

关键已逐张打开检查的图（相对 `.local/verification/`；其余图与逐动作绑定见 manifest）：

| PNG | SHA-256 |
| --- | --- |
| `gui-1.0-disabled-skip.png` | `19486dbda7b4b17b40edd2b2111e601d7fcbd5eb9dc74e1358ff9cafc4f13bb7` |
| `gui-1.0-prevent-cancels-default.png` | `9f8546f40259fbff441d4c76b3242799a258e88026d8004cf290a4065470ce15` |
| `gui-1.0-capture-outside-window.png` | `8ee055ae68a70282fbb2564a730059c5acf7638e62910975e5a3b01ba5a8537e` |
| `gui-1.5-disabled-skip.png` | `57dccbce4da1ec5f78d04e966a0c1ee18dddc2bf3e13460c75b8d7d5edef04c5` |
| `gui-1.5-capture-outside-window.png` | `55a4a35dfe8c036874e91cf4a9af2b9df2b67fbb84dc63c288250551d6a1b87d` |
| `gui-1.5-narrow-chinese.png` | `8985b38a485f5a9419020046355131ac2e01b34c913643f3569c037f30560c63` |
| `background-completed.png` | `7c74c8509283b974d3cbdb87932f5c30c9032f24f323a8c8de74ef8ffe5fdde8` |

原生 Wayland、真实输入法、Windows/macOS、多屏动态 DPI、硬件 GPU、有窗口管理器的遮挡/最小化没有实测；Retry/100ms、零尺寸与 occlusion 由既有逻辑回归保持，不宣称本次实际诱发 surface 故障。没有编辑、滚动、拖放、主题 token、无障碍桥接或 LLM 产品扩展；CI job 仍为 `Rust checks`，未改变工作流或平台支持声明。
