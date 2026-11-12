# Chronobreak

用 Rust 构建我们自己的 GUI 框架。

采用保留 UI 树、自绘渲染与响应式状态更新，逐步打通「界面描述 → 状态更新 → 布局与文字排版 → 绘制 → 窗口与交互」的闭环。复用成熟底层库，把精力投入框架核心。

## 当前状态

首个 Rust 实现提供可调整窗口大小、中文自动换行和点击计数的信息卡片。原生 builder 构建保留 UI 树，Signal 局部更新文本，taffy 负责 Flexbox，cosmic-text / glyphon 负责真实文本测量与绘制，wgpu / winit 负责自绘与窗口事件。当前仍是最小原型，不是完整控件库。

## 运行与验证

使用稳定版 Rust（开发环境为 1.99.1）。Ubuntu / Debian 依赖：

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1 libvulkan1 mesa-vulkan-drivers fonts-noto-cjk
cargo run --locked
```

需要可访问的 X11 或 Wayland 会话；无独立 GPU 时可以使用 Mesa 的 Vulkan 软件适配器。中文依赖系统 CJK 字体回退；不内嵌或重新分发字体。依赖版本以提交的 `Cargo.lock` 为准，glyphon 0.9 与 wgpu 25 / cosmic-text 0.14 配套使用。

点击蓝色按钮增加数字，缩窄窗口观察中文换行和按钮位置变化；过小窗口会裁剪内容，不提供滚动。按下和松开必须命中同一个按钮才计数。失焦、指针离开或窗口尺寸变化取消当前按压。关闭窗口退出。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

测试覆盖 Signal 更新与 RAII 清理、同一帧状态合并与局部更新、真实文字测量和窄屏重排、命中与点击、裁剪和 DPI 坐标。GitHub Actions 的 `Rust checks` 执行这些静态检查与单元测试，不代替实际图形运行。

图形复验流程与本次证据见 [最小 GUI 验证](docs/minimal-gui.md)。可用 `CHRONOBREAK_TRACE=1 cargo run --locked` 输出适配器、物理尺寸、缩放、计数、变更节点数和排版行数；空闲时不持续输出帧。

本机已在 Xvfb + Mesa 软件 Vulkan 下实际验证中文绘制、点击、窗口重排、裁剪、空闲休眠及 1.5 倍 DPI 点击；隔离图形验证的可选工具为 `xvfb xdotool python3-pil`。原生 Wayland、多屏动态 DPI 和其他平台仍需另验。

## 入口

- [文档与参考资料](docs/README.md)
- [初始架构方向](docs/architecture.md)
- [协作与分支](CONTRIBUTING.md)
- [项目索引](AGENTS.md)

## 许可

项目暂未选择开源许可证。第三方文章全文和媒体保留本地，公开仓库只提供来源引用与归档说明。
