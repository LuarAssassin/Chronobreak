# Chronobreak

用 Rust 构建我们自己的 GUI 框架。

采用保留 UI 树、自绘渲染与响应式状态更新，逐步打通「界面描述 → 状态更新 → 布局与文字排版 → 绘制 → 窗口与交互」的闭环。复用成熟底层库，把精力投入框架核心。

## 当前状态

提供可复用的 `Application` 库入口，业务界面位于 `examples/`。原生 builder 构建保留 UI 树，`Signal<T>` 绑定文字，`Signal<bool>` 控制保留节点显示；taffy 负责 Flexbox，cosmic-text / glyphon 负责真实文本测量与绘制，wgpu / winit 负责自绘与窗口事件。当前仍是早期框架，不是完整控件库。

## 运行与验证

使用稳定版 Rust（开发环境为 1.99.1）。Ubuntu / Debian 依赖：

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1 libvulkan1 mesa-vulkan-drivers fonts-noto-cjk
cargo run --example counter --locked
cargo run --example state_panel --locked
```

需要可访问的 X11 或 Wayland 会话；无独立 GPU 时可以使用 Mesa 的 Vulkan 软件适配器。中文依赖系统 CJK 字体回退；不内嵌或重新分发字体。依赖版本以提交的 `Cargo.lock` 为准，glyphon 0.9 与 wgpu 25 / cosmic-text 0.14 配套使用。

`counter` 提供简单计数卡片；`state_panel` 提供两个独立计数器、可变中文说明及计数器 B 的显示开关。隐藏不占布局空间、不绘制、不参与命中，恢复不重置计数。面板采用竖排，缩窄窗口时中文自动换行；过小窗口裁剪，不提供滚动。按下和松开必须命中同一个按钮才触发。失焦、指针离开、窗口尺寸变化或隐藏节点取消当前按压。关闭窗口退出。

```rust,no_run
use chronobreak::{Application, Element, Result, Signal};

fn main() -> Result<()> {
    let name = Signal::new(String::from("世界"));
    let prefix = String::from("你好，");
    let root = Element::bound_text(&name, move |text| format!("{prefix}{text}"));
    Application::new(root).title("我的应用").run()
}
```

绑定使用显式依赖，无宏或自动依赖追踪；格式化闭包可捕获环境，其他捕获 Signal 不会自动成为依赖。UI 点击回调修改绑定状态后，框架负责同步、布局与重绘；应用无需访问 Renderer / Scene 或窗口事件循环。没有跨线程唤醒器、动态列表或子树 diff。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

测试覆盖 Signal 更新与 RAII 清理、同一帧状态合并与局部更新、真实文字测量和窄屏重排、命中与点击、裁剪和 DPI 坐标。GitHub Actions 的 `Rust checks` 执行这些静态检查与单元测试，不代替实际图形运行。

原型图形复验见 [最小 GUI 验证](docs/minimal-gui.md)，本轮框架验证见 [可复用核心验证](docs/reusable-core.md)。可用 `CHRONOBREAK_TRACE=1 cargo run --example state_panel --locked` 输出适配器、物理尺寸、缩放、变更节点数和排版行数；库的诊断不包含业务计数字段，空闲时不持续输出帧。

本机已在 Xvfb + Mesa 软件 Vulkan 下实际验证中文绘制、点击、窗口重排、裁剪、空闲休眠及 1.5 倍 DPI 点击；隔离图形验证的可选工具为 `xvfb xdotool python3-pil`。原生 Wayland、多屏动态 DPI 和其他平台仍需另验。

## 入口

- [文档与参考资料](docs/README.md)
- [初始架构方向](docs/architecture.md)
- [协作与分支](CONTRIBUTING.md)
- [项目索引](AGENTS.md)

## 许可

项目暂未选择开源许可证。第三方文章全文和媒体保留本地，公开仓库只提供来源引用与归档说明。
