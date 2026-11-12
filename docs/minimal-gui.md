# 最小 GUI 验证

## 复验

在真实桌面会话运行 `CHRONOBREAK_TRACE=1 cargo run --example counter --locked`，确认日志适配器与画面。截图仅作本地验证，不提交。以下是原型阶段的历史记录；当前 API 与本轮证据见 [可复用核心验证](reusable-core.md)。

1. 确认标题、中文段落、蓝色按钮和初始计数 0，无缺字方框。
2. 点击按钮两次，画面依次显示 1、2，日志 `changed=1`；仅松开或拖出按钮不计数。
3. 缩窄窗口，确认正文行数和按钮位置改变；恢复宽度后回到宽屏布局。
4. 缩短窗口，确认内容按窗口边界裁剪；重新放大恢复。最小原型无滚动。
5. 空闲数秒，日志不持续产生帧；最小化、恢复、关闭不崩溃。不同 DPI 显示器上的移动需要实际多屏环境另验。

## 本次记录

2026-11-12，Linux，Rust 1.99.1。重跑 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`cargo build --locked` 均通过；测试合计 **13 passed / 0 failed**，其中 library 10 项、二进制 3 项、doc-test 0 项。

图形适配器为 `llvmpipe (LLVM 21.1.8, 256 bits)`，CPU / Vulkan，Mesa 26.0.8。已有 GNOME Wayland / XWayland 和 Noto CJK 字体；沙箱内 `xdpyinfo` 无法连接 `:0`，批准在沙箱外运行后可正常连接，并非没有图形环境。

先在 XWayland `:0` 实际启动，cua-driver 0.38.0 单窗口截图可见完整中文卡片，760×520 缩为 400×600 后文字换行正确。但驱动的后台 XSendEvent、前台 XTEST 点击未产生完整的 winit 按下/松开事件，画面计数没有变化；不把驱动成功返回当点击通过。桌面截图诊断返回 X11 `GetImage` / `BadMatch`。未修改共享驱动或桌面安全配置。

随后批准安装 `xvfb`、`xdotool`（附带 `libxdo3`），在独立 X11 `:93` 上启动同一个二进制与真实 wgpu surface，使用软件 Vulkan 和 XTEST，**不是离屏渲染替身，也没有直接改写 Signal 或应用状态**。使用 Pillow / X11 截图并逐张目视检查：

| 操作 | 实际结果 |
| --- | --- |
| 初始 760×520 | 中文无缺字，计数 0，排版总行数 5 |
| 按钮两次真实点击 | 日志出现 Pressed / Released；画面与日志计数 0 → 1 → 2，每次 `changed=1` |
| 缩为 400×600 | 总行数 5 → 8，按钮下移，计数仍为 2 |
| 按钮按下后移到正文松开 | 不增加计数；随后在窄屏正常点击变为 3 |
| 缩为 400×240，再恢复 760×520 | 边界裁剪，无越界绘制；恢复后完整显示，计数 3，行数回到 5 |
| 无操作等待 3 秒 | 帧日志累计数 7 → 7，无连续重绘 |
| 启动时指定 DPI 1.5 | 逻辑 760×520 对应物理 1140×780；物理 (270,381) 点击使计数 0 → 1 |
| DPI 1.5 下缩为物理 600×900 | 逻辑宽 400，排版行数 5 → 8，中文字形与按钮位置正确 |
| 结束 | 向本次四个测试窗口发送标准 `WM_DELETE_WINDOW`，所有运行会话正常结束；未终止共享 cua-driver |

原始日志与 PNG 仅保留于本机 `/tmp/chronobreak-minimal-gui-20261112/`，不纳入 Git，可能随临时目录清理而消失。关键图为 `xvfb-initial.png`、`xvfb-clicked.png`、`xvfb-narrow.png`、`xvfb-clipped.png`、`xvfb-restored.png`、`dpi-clicked.png`、`dpi-narrow.png`；输入与帧日志见 `xvfb-run.log`、`dpi-run.log`。这里的 Xvfb 图为隔离桌面对应窗口区域截图，不冒充 cua-driver 的单窗口认证截图。

## 呈现重试修复复验

`Renderer::draw` 明确返回 `Presented` 或 `Retry`，仅真正调用 `present` 后才报告呈现。`Timeout` / `Other` 保留待呈现意图；`Lost` / `Outdated` 重配后也返回 `Retry`，不再立即进行第二次 acquisition，下一次 acquisition 的临时失败走同一路径。App 使用单个重试期限和 `ControlFlow::WaitUntil`，每次失败至少间隔 100ms，期限到达时请求重绘；期限前的额外重绘事件不会提前 acquisition。成功后清除重试并回到 `Wait`；零尺寸、遮挡或暂停停止重试，由恢复事件重新请求绘制。不可恢复 acquisition 错误（如 `OutOfMemory`）保留原错误并退出，其他渲染错误仍正常上报。

新增 6 项回归测试：3 项 acquisition 逻辑测试覆盖临时错误、重配后超时再成功、不可恢复错误；3 项可控时间的调度测试覆盖 `Scene::sync` 已清空更新队列后首次失败、无第二次输入仍安排呈现，连续 5 次失败逐次等待，以及成功后不再请求重复重绘。测试注入 acquisition 结果和 `DrawOutcome`，并检查期限、`ControlFlow` 与保留文本；**没有实测诱发 GPU 超时、surface 丢失或显存耗尽，也不把逻辑测试称为真实 GPU 故障恢复验证**。

修复后在新建独立 Xvfb `:94` 上短实际复验，同一 llvmpipe / Vulkan 适配器与真实 wgpu surface：XTEST 点击使画面与日志计数 0 → 1（`changed=1`）；760×520 → 400×600 → 760×520 时排版行数 5 → 8 → 5，计数保持 1；无输入等待 3 秒，帧日志累计数 **4 → 4**；标准 `WM_DELETE_WINDOW` 后进程退出码 0，并仅关闭本次 Xvfb。逐张目视检查 `retry-initial.png`、`retry-clicked.png`、`retry-narrow.png`，日志为 `retry-run.log`，均加入上述本地临时目录；旧验证产物全部保留，等待用户截图检查，不新增临时脚本、不提交截图。

CI 配置的 push 分支过滤已改为 `['**']`，覆盖包括未来 `feat/*`、`fix/*`、`docs/*`、`chore/*`、`experiment/*` 在内的所有分支；仍保留面向 `main` 的 `pull_request` 触发与精确 job name `Rust checks`。这里只确认配置与本地验证，未宣称远端 Actions 已运行或通过。

## 隔离环境重跑

可选验证工具（不属于应用运行依赖，也不被静态 CI 使用）：

```sh
sudo apt-get install --no-install-recommends -y xvfb xdotool python3-pil
Xvfb :93 -screen 0 1280x900x24 -nolisten tcp
```

确认 `:93` 未被其他会话占用。另一个终端从项目根运行（软件适配器路径按发行版调整）：

```sh
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET DISPLAY=:93 \
  WGPU_BACKEND=vulkan VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
  CHRONOBREAK_TRACE=1 cargo run --locked
```

再从另一终端发现本次窗口并发送真实输入：

```sh
DISPLAY=:93 xdotool search --onlyvisible --class chronobreak
DISPLAY=:93 xdotool mousemove --sync 180 254 click --repeat 2 --delay 250 1
DISPLAY=:93 xdotool windowsize --sync <本次窗口ID> 400 600
```

运行前核对截图和窗口几何，不把示例窗口 ID 当固定值。DPI 验证在启动命令增加 `WINIT_X11_SCALE_FACTOR=1.5`，点击坐标也按比例转换。清理仅限本次创建的窗口与 Xvfb，不能终止其他会话。

## 未验证

原生 Wayland 渲染与输入、真实多屏间动态 DPI 切换、有窗口管理器的最小化/恢复、硬件 GPU 与非 Linux 平台未实测。GitHub Actions 远端结果由 PR 审查另行核验，本地通过不等于远端 CI 已通过。
