# Chronobreak

用 Rust 构建我们自己的 GUI 框架。

采用保留 UI 树、自绘渲染与响应式状态更新，逐步打通「界面描述 → 状态更新 → 布局与文字排版 → 绘制 → 窗口与交互」的闭环。复用成熟底层库，把精力投入框架核心。

## 当前状态

提供可复用的 `Application` 库入口，业务界面位于 `examples/`。M1a/M1b/M1c 已分别合入 PR #6/#7/#8，M2a 路由/焦点/捕获/键盘激活已合入 PR #9。本分支增加 **M2b 初始纵向滚动候选，待审查**：固定高度独立视口、滚轮剩余链、clamp 与焦点/显式揭示；不表示整个 M2 完成。横向滚动、虚拟化、惯性/动画/触控、M3 编辑/IME、主题系统与 LLM 产品均未实施。taffy、cosmic-text / glyphon、wgpu / winit 负责布局、文字、自绘与窗口事件；当前不是完整控件库。

## 运行与验证

使用稳定版 Rust（开发环境为 1.99.1）。Ubuntu / Debian 依赖：

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1 libvulkan1 mesa-vulkan-drivers fonts-noto-cjk
cargo run --example counter --locked
cargo run --example state_panel --locked
cargo run --example node_lifecycle --locked
cargo run --example batched_updates --locked
cargo run --example background_messages --locked
cargo run --example input_routing --locked
cargo run --example scroll_views --locked
```

需要可访问的 X11 或 Wayland 会话；无独立 GPU 时可以使用 Mesa 的 Vulkan 软件适配器。中文依赖系统 CJK 字体回退；不内嵌或重新分发字体。依赖版本以提交的 `Cargo.lock` 为准，glyphon 0.9 与 wgpu 25 / cosmic-text 0.14 配套使用。

`counter` 提供简单计数卡片；`state_panel` 提供独立计数器、中文说明及显示开关。隐藏不占布局空间、不绘制、不参与命中，恢复不重置计数。缩窄窗口中文自动换行，过小窗口裁剪，不提供滚动。普通按钮按下/松开必须对应同一控件，拖出释放不激活。失焦、尺寸/DPI 变化、已提交隐藏/禁用/卸载取消按压；无捕获时离窗取消，有效控件级捕获不因 `CursorLeft` 丢失。关闭窗口退出。

### 输入交互（M2a，已合入 PR #9）

既有 `.on_click(|| ...)` 保持单次激活回调，现默认成为 Tab 停靠点；`.focusable(false)` 显式退出。新增 `.focusable(true)`、`.disabled(bool)`、`.disabled_when(&Signal<bool>)` 和 `.on_event(EventPhase, handler)`。回调只得到类型化事件与上下文，不持 Scene/Renderer；结构修改仍使用 `Children` 延迟提交。

`input_routing` 显示嵌套面板路由/标志、A/B 键盘计数、B 动态禁用/隐藏/删除重建、Ctrl+R 作用域快捷键和 Esc 显式释放的捕获拖动区域。Tab/Shift+Tab、Enter、Space 松开激活与金色焦点框由框架管理；灰色 hover 框/指针图标不等于后续主题系统。完整顺序、默认动作、清理、迁移及 1.0/1.5 DPI 实测证据见 [输入路由](docs/input-routing.md)。

```rust,no_run
use chronobreak::{Application, Element, Result, Signal};

fn main() -> Result<()> {
    let name = Signal::new(String::from("世界"));
    let prefix = String::from("你好，");
    let root = Element::bound_text(&name, move |text| format!("{prefix}{text}"));
    Application::new(root).title("我的应用").run()
}
```

绑定使用显式依赖，无宏或自动依赖追踪；格式化闭包可捕获环境，其他捕获 Signal 不会自动成为依赖。UI 点击或消息回调修改绑定状态、提交结构命令后，框架负责同步、布局与重绘；应用无需访问 Renderer / Scene 或窗口事件循环。没有全树 diff 或完整通知事务。

### 纵向滚动（M2b 候选，待审查）

```rust,no_run
use chronobreak::{Application, Element};
let content = Element::column().child(Element::text("长内容\n".repeat(60)));
let (viewport, scroll) = Element::scroll_view(240.0, content);
let down = scroll.clone();
let root = Element::column().child(viewport)
    .child(Element::text("向下 80").on_click(move || { down.scroll_by(80.0); }));
Application::new(root).run().unwrap();
```

`scroll_to` / `scroll_by` / `scroll_into_view(NodeId)` 排队到 UI 提交边界，按最终内容尺寸 clamp，无需 Scene/Renderer/手动重绘。目标 ID 可取自 `Children::node`；receipt 在提交后给出作用域/代际/隐藏错误，`snapshot()` 为最后提交几何，不是响应式订阅。固定高度不能抵消窗口或非滚动祖先对视口自身的裁剪；比例指示条仅显示，不拖动。最近边缘对齐、嵌套链、超大目标策略与图形证据见 [纵向视口合同](docs/scroll-views.md)。

### 后台消息（M1c，已合入 PR #8）

```rust,no_run
use chronobreak::{Application, Element, Signal, message_channel};
use std::num::NonZeroUsize;

let value = Signal::new(0);
let model = value.clone();
let (sender, receiver) = message_channel(NonZeroUsize::new(8).unwrap());
let cancel = receiver.cancellation();
let root = Element::bound_text(&value, |value| format!("后台进度：{value}"))
    .on_message(receiver, move |value| model.update(|current| *current = value));
std::thread::spawn(move || {
    if let Err(error) = sender.try_send(42) { eprintln!("提交失败：{error:?}"); }
});
Application::new(root).run().unwrap();
cancel.close();
```

`MessageSender<T>` 可跨线程克隆；只有 `T: Send + 'static` 的数据跨线程，Signal、接收器、取消句柄及处理闭包仍归 UI。`try_send` 不等空位，`Full(T)` / `Closed(T)` 归还负载；入队成功不保证取消、已提交卸载或关闭后仍交付。每 UI 轮全场景最多处理 64 条，自动批处理、提交并必要布局；剩余消息重新唤醒，空队列回到 Wait，不轮询。显式 `cancel.close()` 立即关闭，排队 `Children::remove` 则到提交才关闭；隐藏不取消。完整背压、关闭、迁移与实测证据见 [后台消息](docs/background-messages.md)。

`background_messages` 启动真实工作线程，窗口先空闲三秒，再无输入更新中文进度；附新请求/旧结果过滤、取消、移除/恢复、20,001 条有界突发与响应计数。示例工作线程在 Full 时保留负载重试、在 Closed 时合作退出，不提供通用任务运行时，也不在 UI 内 join。

### 显式批处理

```rust
use chronobreak::{Signal, batch};

let count = Signal::new(0);
let result = batch(|| {
    count.update(|value| *value = 1);
    batch(|| count.update(|value| *value = 2));
    count.get()
});
assert_eq!(result, 2);
```

批内模型立即可读，通知在最外层结束时按首次入队 FIFO 合并；普通 `update` 仍即用，不要求手动 flush。监听器内写入排队而非递归调用；新版本终止旧版本尚未开始的回调，A 收到 1 写 2、后注册绑定 B 的最终值不会退回 1。`update` 的写入闭包内仍不能读写同一 Signal。批内 `0→1→0` 通知一次最终 0，但最终格式化文字相同不会重排或请求重绘。

单条因果传播最多执行 1024 个通知任务，超限明确 panic（传播预算超限，不证明有环），清理待通知队列而不回滚模型；监听器/批闭包 panic 同样不能当成成功提交。独立 Signal 的大批更新不共用这个计数。取消订阅会阻止尚未开始的回调；通知中新增订阅从下一次 dispatch 开始参与。

`batched_updates` 通过真实按钮展示最终 2、三次写入只格式化/提交一次，以及文字换行/插入后目标按钮的新命中位置。布局脏原因与呈现意图独立于全帧 GPU 绘制；输入前先提交并完成必要布局，不维持第二套旧画面树。完整合同、迁移与分层证据见 [M1b 验收](docs/batched-updates.md)。UI 线程外消息唤醒仍属 M1c，不能声称休眠事件循环会被外部模型修改自动唤醒。

### 动态 children

```rust,no_run
use chronobreak::{Application, Element, Result};

fn main() -> Result<()> {
    let (list, children) = Element::keyed_column();
    let add = children.clone();
    let remove = children.clone();
    let root = Element::column()
        .child(Element::text("新增 a").on_click(move || {
            add.insert("a", Element::text("中文动态节点"));
        }))
        .child(Element::text("卸载 a").on_click(move || {
            if let Some(id) = remove.node("a") { remove.remove(id); }
        }))
        .child(list);
    Application::new(root).run()
}
```

`Children::insert` 追加新子树；`move_before(id, before)` 与 `reorder(&ids)` 仅重排同容器存续节点，保留局部 Signal、订阅和身份。`remove(id)` 真正卸载；相同 key 重建产生新身份，不恢复已销毁局部状态。隐藏仍保留。key 在容器内唯一；`CommandReceipt::result()` 在提交前为 `None`，之后给出新 ID 或明确错误，拒绝命令也输出诊断。`node(key)` 只查询最近已提交的 key，不能用它同步读取刚排队的插入。

`node_lifecycle` 演示独立行计数、新增/删除/反转、隐藏恢复、同 key 重建和旧 ID 拒绝；按压卸载后的旧松开回归由确定性输入测试覆盖。API 不支持跨容器 reparent、React diff、滚动或节点脱离挂载。完整合同、计数及图形证据见 [M1a 验收](docs/node-lifecycle.md)。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

测试覆盖 Signal 更新与 RAII 清理、同一帧状态合并与局部更新、真实文字测量和窄屏重排、命中与点击、裁剪和 DPI 坐标。GitHub Actions 的 `Rust checks` 执行这些静态检查与单元测试，不代替实际图形运行。

原型图形复验见 [最小 GUI 验证](docs/minimal-gui.md)，既有框架验证见 [可复用核心验证](docs/reusable-core.md)。可用 `CHRONOBREAK_TRACE=1 cargo run --example node_lifecycle --locked` 输出适配器、尺寸、缩放、活节点/taffy/槽位、订阅/pending/队列、CPU 文本 buffer、实际 prepare/跳过数及 TextRenderer 缓存/峰值；空闲时不持续输出帧。缓存计数不是驱动 VRAM 回收证明。

本机已在 Xvfb + Mesa 软件 Vulkan 下实际验证中文绘制、点击、窗口重排、裁剪、空闲休眠及 1.5 倍 DPI 点击；隔离图形验证的可选工具为 `xvfb xdotool python3-pil`。原生 Wayland、多屏动态 DPI 和其他平台仍需另验。

## 入口

- [文档与参考资料](docs/README.md)
- [初始架构方向](docs/architecture.md)
- [协作与分支](CONTRIBUTING.md)
- [项目索引](AGENTS.md)

## 许可

项目暂未选择开源许可证。第三方文章全文和媒体保留本地，公开仓库只提供来源引用与归档说明。
