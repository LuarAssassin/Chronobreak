# 可复用核心验证

## 能力与入口

`Application::new(root).title(...).run()` 统一管理窗口、事件循环与重绘；`examples/counter.rs` 和 `examples/state_panel.rs` 只定义业务状态及 Element，不持有 Renderer / Scene，不手写同步或请求重绘。运行命令为 `cargo run --example counter --locked`、`cargo run --example state_panel --locked`，不再提供默认二进制。

`Element::bound_text(&signal, move |value| ...)` 支持泛型数字、String、bool 和捕获闭包；`Element::visible(&shown)` 保留节点、状态与订阅，通过 display 控制布局、绘制和命中。具体同步与生命周期边界见 [架构](architecture.md)。不包含动态列表、动态子树 diff、自动依赖追踪、宏或跨线程事务。

## 自动化结果

2026-11-12，Linux，Rust 1.99.1，以下命令全部退出 0：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

结果为 **19 单元测试通过、1 文档编译测试通过，0 失败**。原有 13 项保留或迁移至库，包括 100ms 呈现重试、恢复空闲 Wait、surface 错误分类、DPI 裁剪、文字换行、按压命中和 Signal RAII；新增 6 项测试覆盖：

- 泛型数字 / String / bool、捕获闭包、同帧多次更新只格式化最终值，节点互不干扰。
- 两个按钮通过命中与按压／松开链路独立更新。
- 隐藏父节点排除后代布局空间、显示列表及命中，隐藏时修改状态后恢复仍保留；按下后隐藏、恢复后仅松开均不能触发。
- 同节点文字与显示两个订阅合并更新，场景丢弃释放全部订阅、格式化闭包及待更新项。
- 初始隐藏根节点不绘制、不命中、不占高度，可恢复。
- 构造后、挂载前修改状态读取最新值；外部 Signal 所有者先释放，场景仍可同步并最终释放所有捕获状态。

CI job 精确名称仍为 `Rust checks`，保留所有分支 push 与面向 main 的 PR 触发，并执行 examples 构建；此处记录本地结果，不宣称远端 CI 已运行。

## 实际图形结果

在本 worktree 的隔离 Xvfb 显示 `:2`（1600×1200）中运行真实 examples 和 wgpu surface，使用 xdotool / XTEST 输入，Pillow / X11 截取隔离桌面的窗口区域并逐项目视核对。不是离屏绘制替身，不是 cua-driver 的认证单窗口截图，没有直接写入 Signal 或业务状态。没有触碰用户当前桌面。

适配器为 llvmpipe（LLVM 21.1.8，CPU / Vulkan），Mesa 26.0.8，系统 Noto CJK 字体。首次隔离启动继承了 Wayland 环境，连接 compositor 失败；仅在验证子进程清除 `WAYLAND_DISPLAY`，随后成功运行，不修改主机会话配置。

| 操作 | 实际结果 | 本地证据文件 |
| --- | --- | --- |
| 面板初始 760×520 | 中文正常，A=0 / B=0 | `panel-initial.png` |
| 点击 A 两次、B 一次 | A=2 / B=1，彼此独立；每次绑定更新 changed=1 | `panel-a1.png`、`panel-a2.png`、`panel-b1.png` |
| 点击切换说明 | String 说明变为“说明已切换…” | `panel-message.png` |
| 隐藏 B | B 消失，后续按钮和卡片底边上移，无布局空位 | `panel-hidden.png` |
| 隐藏时点击 A，再恢复 B | A=3 / B=1，B 没有重建或重置 | `panel-hidden-a3.png`、`panel-restored.png` |
| 缩为 360×620 并点击 B | 中文标题和说明换行，B 可点击变为 2，A 仍为 3 | `panel-narrow.png`、`panel-narrow-b2.png` |
| A 按下后移出松开 | A 保持 3，B 保持 2 | `panel-drag-cancelled.png` |
| 缩为 360×250，再恢复 760×520 | 内容按窗口裁剪；恢复后 A=3 / B=2，布局完整 | `panel-clipped.png`、`panel-wide-restored.png` |
| 面板空闲 3 秒 | 帧累计数 12 → 12，没有持续重绘 | `actions.jsonl`、`panel.log` |
| 以 1.5 DPI 启动，点击 A、B 与说明切换、隐藏／恢复 | 物理 1140×780，A=1 / B=1，说明切换和状态保留正确 | `panel-dpi-initial.png`、`panel-dpi-b1.png`、`panel-dpi-message.png`、`panel-dpi-hidden.png`、`panel-dpi-restored.png` |
| 1.5 DPI 下缩为物理 540×930，点击 B | 相当于逻辑宽 360，中文重排，A=1 / B=2 | `panel-dpi-narrow.png`、`panel-dpi-narrow-b2.png` |
| DPI 面板空闲 3 秒 | 点击前后分别验证 7 → 7、8 → 8 帧 | `actions.jsonl`、`panel-dpi.log` |
| 简单 counter 示例点击、缩窄 | 计数 0 → 1，中文换行，使用同一库事件循环 | `counter-initial.png`、`counter-clicked.png`、`counter-narrow.png` |

所有脚本、PNG、日志保留在当前 worktree 的 `.local/verification/`，已被 gitignore 排除，不提交；等待主协调独立审查结束后统一清理。`gui.py` 是 stdin JSON 操作驱动，`actions.jsonl` 记录真实输入坐标、截图文件与空闲帧数断言；`rust-checks.log` 保存四项精确命令的完整结果。日志中的 text_lines 是全部保留文本 buffer 的诊断计数，包含隐藏节点，不作为可见文本行数的证明；可见性以显示列表测试及实际画面为准。

## 未验证与边界

本轮未验证原生 Wayland、真实多屏动态 DPI、非 Linux 平台、IME、无障碍、最小化／系统挂起恢复。100ms surface 临时故障的重试沿用并保留单元测试，没有在本轮真实 GPU 会话中注入 surface 故障。测试结束仅终止本轮子进程和 Xvfb，不宣称验证了桌面窗口管理器的关闭协议。过小窗口只有裁剪、没有滚动；隐藏节点继续保留订阅与业务状态，未引入通用子树增删。
