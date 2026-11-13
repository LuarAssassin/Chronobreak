# 单行原生 IME：合同与复验

2026-11-13。当前合入、候选及资格结论只见 [Alpha 状态](alpha-delivery-status.md)；本页记录实现合同、真实系统证据和复验限制，不能把局部成功视为 M3C 通过。

## 使用与迁移

```rust,no_run
use chronobreak::{Application, Element, Result, Signal};

fn main() -> Result<()> {
    let value = Signal::new(String::new());
    Application::new(Element::text_input(&value)).run()
}
```

没有新增公开编辑器、渲染器、IME provider 或手工 flush API。已有 `Element::text_input(&Signal<String>)` 自动接受 winit 原生 `Ime`；业务仍只观察已提交的 Signal。`TextEditModel`、M1/M2 路由和受控同步 API 不变。`cargo run --example native_ime --locked` 提供两个独立字段、嵌套视口、只读/禁用/隐藏/外部替换及真正卸载按钮；`text_input` 保留同 Signal 的双字段示例。

仅单行；不扩展到多行、完整视觉 bidi、富文本、主题、无障碍或 Windows/macOS 资格。IBus/libpinyin、Fcitx5/Pinyin 都只是独立验证服务；crate 不复制、链接或分发其 GPL 引擎，不新增 engine-specific 运行时依赖，继续使用 winit 原生 IME。

## 事件、所有权与事务

- 目标必须是有效、可见、未禁用且非 readonly 的 focused TextInput。每个 Editor 拥有 composition、捕获的定向逻辑 selection、剪贴板 epoch 和独立 IME session；Scene 以完整带 generation 的 `NodeId + session` 关联原生流，不以全局 composing 布尔值作为写入权限。
- Application 在目标变化时先关闭旧 IME，等待 `Disabled` 后才允许新目标，并等待 `Enabled` 才接受 preedit/commit。窗口失焦、挂起、隐藏/禁用、readonly、卸载/同槽重建和有效外部值替换均使旧 session 失效。收到旧目标事件时先同步受控值，再校验所有权；不能写入替换值或第二字段。
- winit **0.30.13** 的 `Ime::Preedit` cursor 是 **UTF-8 字节偏移**，`None` 隐藏 caret；X11 adapter 已将 XIM 字符位置转换成 UTF-8 字节。检查两端是否为合法字符边界，拒绝越界、控制字符及多行 preedit；不把字节数当字符数，也不要求 OS cursor 必须是 UAX #29 字符簇边界。落在同一整形簇内部时，视觉位置取最近的已整形 stop，逻辑模型不因此改变。
- 非空 preedit 仅替换**显示字符串**中捕获的逻辑选区；临时文字、全段下划线、cursor 范围背景与 caret 由节点原有 cosmic-text Buffer / glyphon 路径整形和绘制。没有 Signal 发布、模型修改或 undo 条目；取消恢复原模型、定向 selection 和历史。
- winit 在 Commit **之前**发送空 Preedit。因此空 preedit 立即清除临时显示，但暂留捕获的 selection，不能在此提前丢弃 commit 归属。随后的 Commit 将 selection 替换一次，只发布一次有效修改、产生一个 undo 事务；空 Commit 不删除选区。下一个普通编辑/点击会撤销残留的空 composition；`Disabled` 必定清除。
- 非空 composition 时，框架不分发 Key 路由或默认按钮激活，避免全局/作用域快捷键抢夺 Space、Esc、Backspace、方向/候选选择键；这些先由原生 IME 处理。Ctrl/Super+C/X/V 是明确支持的例外：先取消临时 composition，再对原逻辑选区复制/剪切/粘贴，不把 preedit 放入剪贴板。点击/拖选也先取消，再在已提交文字上命中。开始/更新 composition、取消、编辑和受控替换会使旧异步剪贴板回复过期；新请求仅在原 NodeId、epoch、焦点、可编辑状态及期限仍有效时应用。
- `Enabled` 本身不抑制正常 Latin 键盘；来自 winit 的普通文本载荷仍走既有编辑路径。依据 backend 的 Ime/KeyboardInput 去重合同消费 Commit，不猜测、缓存或吞掉下一次相同 Latin 字符。也不声称能识别一个违反 `Disabled → Enabled` 顺序、无来源 token 的任意伪造/重复 OS Commit；单测中的过期流断言针对真实生命周期隔离，不是任意畸形流证明。

缓存源码核对入口：`winit-0.30.13/src/event.rs` 的 `Ime`、`window.rs::set_ime_cursor_area`、`platform_impl/linux/x11/{event_processor.rs,ime/context.rs}`。这些文件的副本及哈希随本地证据保存；官方入口为 [Ime 文档](https://docs.rs/winit/0.30.13/winit/event/enum.Ime.html)。

## Caret 与候选坐标

几何使用当前**实际显示字符串**的整形 stop，而非提交模型的旧 caret。私有横向揭示先作用于文字 origin；节点 bounds 已包含各层纵向 scroll offset，clip 取有效内容区域与所有祖先裁剪的交集。caret 垂直区间再次与内容 clip 相交；零内容 clip 不提交候选区域，保留 M3B FB-2 的 blink 停帧约定。没有 Scene/Renderer 资源转移到应用层。

向 winit 传**窗口客户区局部逻辑坐标**，由其按 `scale_factor` 转物理像素；不重复加窗口屏幕原点或重复缩放。非 X11 传 caret 矩形；X11 的 winit 0.30.13 只支持位置、忽略 size，因此传可见 caret 底端作为 spot，避免候选覆盖文字。编辑、preedit、selection、横向揭示、嵌套滚动、重排/resize、窗口移动、DPI 和焦点变更后更新；以区域、窗口位置及 DPI 的快照去重。

两个有界 X11 兼容处理不能误当成系统候选移动证明：

1. 缓存 winit 仅在处理 **XEvent** 后 drain IME 请求通道；`request_redraw()` 单独不会产生 XEvent。`wake_ime_requests` 刷新**应用自身相同标题**，产生原生 PropertyNotify 并 flush Xlib，再请求绘制。只在 IME 状态/几何变化时调用，不改用户会话、不模拟 IME/键盘事件、不引入空闲轮询；真实首字符不再在等待 enable 的队列里泄漏成 Latin。
2. winit X11 以局部 spot 缓存位置。窗口屏幕原点变化时先提交真实 caret 顶端，再提交最终底端，迫使其重新发送；两个点均来自实际 caret。**下面的实际 IBus popup 移动检查仍失败**，不能以这两个 API 分支执行过就宣布跟随成功。

## 隔离系统复验

全部作者产物位于本 feature 的 `.local/verification/`；不在 main、`/tmp`、用户 HOME 或桌面会话写配置。仅 Xvfb 自己创建并清理系统 X socket/lock。`private_session.py` 以独立、带随机 MIT-MAGIC-COOKIE 的 Xvfb，私有 DBus/IBus 地址及私有 HOME、XDG config/cache/data/runtime 启动；不运行 `xhost`、不关闭 access control、不输出 cookie。短 socket 路径避免 AF_UNIX 的 108-byte 限制；子进程环境为白名单，不继承 Wayland、owner DBus/IBus 或凭据。`isolation.json` 核对私有 IBus 地址、socket 存在及**私有 session bus 查询到的 daemon PID**，而非仅凭 DISPLAY 推断隔离。

所用已有系统工具/服务：IBus `1.5.34~rc2-1`、ibus-libpinyin `1.16.5-2`（组件版本 `1.16.5`）、libX11 `1.8.13`、Xvfb `21.1.22`、xdotool `3.20160805.1`；winit `0.30.13`，NVIDIA Vulkan。没有窗口管理器，使用 `xdotool windowfocus`、直接 move/resize、XTEST 真键鼠及 Pillow 的**仅测试显示器**抓图。应用和真实 `ibus-ui-gtk3` 候选窗均查询 X11 window id、PID/WM_CLASS、物理几何并分别裁图；候选不是框架画的假窗。

复验顺序（脚本为本地审计产物，不作为 crate 依赖）：

```sh
export CARGO_TARGET_DIR="$PWD/.local/verification/target"
cargo build --examples --locked
/usr/bin/python3 .local/verification/private_session.py x6 1.0
# 在另一个终端；每次使用新的短 run 名，避免覆盖旧证据。
/usr/bin/python3 .local/verification/exercise.py x6
```

1. 私有 daemon 使用 `ibus-memconf`；通过该私有 IBus config 显式设置并读回 `general/embed-preedit-text=true`，不碰 dconf。只在已验证的私有 bus 用 GI/`ibus engine libpinyin` 选引擎。CLI 的首次选引擎可能返回 1；必须用实际 engine 查询和后续原生事件判断，不能把命令返回码当输入成功。
2. XTEST 在实际字段键入 `nihao`，查看 `你好` 临时文字、下划线、系统候选窗；Space 真选候选，然后一次 Ctrl+Z/重做。再以 Shift 切引擎 Latin/中文模式，输入 `你好 Latin 中文`。
3. Esc 取消、未提交时点击第二字段、真正卸载第一字段，再在第二字段按 Space；确认无旧中文泄漏。组合期间在嵌套视口实际滚动，并保存部分裁剪 caret；移动同一窗口、缩窄触发重排。1.0× / 1.5× 各自读回物理窗口，不能把静态 scale 两轮称作动态跨屏 DPI 变化。
4. `operations.jsonl` 保存操作/回读，`app.log` 保存实际 winit Ime/模型/scroll/frame trace；每个 checkpoint 保存 app、candidate 与隔离显示 PNG 和 JSON。以最新逻辑 caret × DPI + 实际窗口原点，比较**实际 popup**的位置，而非只断言 API 调用。外部独立 arboard reader 验证 Ctrl+A/C 的最终字段文本。
5. 结束时只停止这次的应用/服务/自有 X server，保留 cleanup 结果。源码、最终二进制、工具/服务版本、脱敏环境、源/产物/操作 SHA-256 与所有失败尝试均归档；不把系统引擎代码加入公开交付。

## IBus 历史证据与未通过项

最终源码/二进制入口为 `.local/verification/final-source-binary-manifest.json`、`final-source/`、`bin/native_ime-final`；native 示例二进制 SHA-256 为 `2e3e189e0e73eaa22a5671563d05b70df4d774fe52d5894acd79a3940917eb0a`。`x8/` 与 `x9/` 分别为最终 1.0× / 1.5× 真实引擎流程，`x4/` 为 `IBUS_ENABLE_SYNC_MODE=0` 的独立替代尝试。`x6/` / `x7/` 保存最后一次仅变量改名前的完整真实流程及协议定位，旧源码/二进制/hash 另保留在 `source-binary-manifest.json`、`source/`、`bin/native_ime`。`x11-first/`、`x1/`、`x2/`、`x3/`、`x5/` 保留探索失败，不用旧二进制/错误断言结果冒充最终资格。

确定性门禁：192 unit + 6 doc 全通过（原 176 + 6 保留，新增 16 unit）；fmt、strict clippy all-targets、workspace locked test、examples locked build 均通过。真实 X11 已有非合成的 preedit、不发布 Signal、Space 中文单次提交/一次撤销、取消、混合 Latin/中文、双字段与移除隔离证据。系统候选静态初始锚点正确。

**该 IBus tuple 的动态候选跟随仍未通过，历史记录当时未签发 M3C 资格。** 例如 1.0×：窗口从 `(40,40)` 移至 `(210,130)`，当前 caret `(52,83,1.5,27)` 要求 popup spot `(262,240)`，实际 popup 仍在 `(92,150)`；随后重排 caret y 变为 134，预期 y=291，popup 仍不动。嵌套滚动后有效 caret y=178、高 24，预期 popup y=242，实际仍 y=282。只有下一次真实引擎表格更新后 popup 才重新定位；不能加一次键来掩盖静止窗口移动失败。1.5× 和独立 async XIM 模式也保留同类失败。

上游 [IBus CandidatePanel](https://github.com/ibus/ibus/blob/1.5.34/ui/gtk3/candidatepanel.vala) 的 `set_cursor_location` 只存矩形、在后续 update/size allocation 才调定位，与上述实测一致；[XIM adapter](https://github.com/ibus/ibus/blob/1.5.34/client/x11/main.c) 则在 SetICValues 后发送 cursor location。本机额外以只监听 `SetCursorLocation` 的私有 bus trace 确认 InputContext → Engine / **Panel** 已收到变更：`x6/cursor-protocol.log` 中移窗后的 Panel spot 为 `(297,367)`，`protocol-moved` 对应实际 popup 仍为 `(267,337)`。监视器先报告私有 IBus 不支持 DBus.Monitoring，随后按其官方 fallback 监听成功，原错误也保留。没有改系统 panel 或伪造候选窗来让门禁变绿；两 DPI 各 14 项系统断言为 11 成功、上述 3 项失败，独立 clipboard reader 为 2/2 成功。

滚轮注意：本机 XTEST 的按钮 5 按下/松开各产生一条 winit wheel；部分裁剪 checkpoint 在实际按下后抓图，再释放、反滚恢复，完整原生 trace 保留，不修改既有 M2 滚轮行为。真实系统仅覆盖嵌套视口中的 inner offset；双层 offset 数学由 unit 验证。组合中系统 cut/paste、动态 readonly/disable/hide/外部替换与重用 ID 的晚到流主要由确定性测试覆盖，不冒称都完成系统注入。

原剩余条件是实际系统候选响应 cursor-location 后复跑全部门禁；下述独立 Fcitx5 tuple 已满足，**没有修复或替换 owner 的 IBus panel**。IBus 自身仍为部分通过。原生 Wayland 仍为实验性、未实测；Windows/macOS、多屏动态 DPI、全视觉 bidi、WM 关闭协议与硬件 surface 故障亦未签发资格。

## 独立 Fcitx5 资格与复验

2026-11-13，在同一 feature 的 `.local/verification/` 内作有界替代尝试，不改 Rust 运行时、不改系统 panel、不停止 host 服务、不安装系统软件或更改默认输入法、dconf、owner HOME。正式范围见 [Alpha 状态](alpha-delivery-status.md)：X11 + Fcitx5 XIM/Pinyin/classic UI；不是指定 IBus 必须通过，也不是任意 IME 都已通过。

### 先核对 IBus 与 WM 条件

保留的旧记录只有源码链接，没有精确版本源码；本次从 Ubuntu 官方 archive 补留 `ibus_1.5.34~rc2.orig.tar.gz`、`ibus_1.5.34~rc2-1.dsc` 与 Debian patch archive。`native-stack/source/ibus-1.5.34-rc2/ui/gtk3/panel.vala:1831` 转发到 CandidatePanel，`candidatepanel.vala:168` 无条件只保存新矩形，并明确注释此处不调用 `adjust_window_position`；构造函数的 `Gtk.WindowType.POPUP` / `size_allocate` 才连接定位。Ubuntu 四个 patch 均不改该调用链。旧私有协议证据已显示 Panel 收到变更，因此缺少 WM 并不能补上这个未发生的定位调用；这不是“换一个 WM 就必定修好 IBus”的实测结论。

Fcitx5 同样在**无 WM** 的自有 Xvfb 通过跟随，故没有为假设而再引入 WM。官方 Fcitx5 `src/ui/classic/classicui.cpp:387` 监听 `InputContextCursorRectChanged`，经 `xcbui.cpp:709` 调 `XCBInputWindow::updatePosition`，对真实 popup 发 XCB configure。未修改、重编或伪造这两个系统 panel。

### 官方软件包与私有路径

Ubuntu 26.04 resolute amd64 的 `apt-get -s --no-install-recommends install fcitx5 fcitx5-pinyin` **仅用于计算依赖**；实际只 `apt-get download`、`dpkg-deb --extract` 到 `native-stack/root/`。没有执行 install、维护脚本、im-config、provider 配置或不可信脚本。最初不存在独立的 `fcitx5-module-xorg` 包；本发行版真实 `libxim.so`、`libxcb.so`、`libclassicui.so` 由 `fcitx5-modules` 提供，不能把空 candidate 当成整个资源不可用。

18 个归档及 SHA-256 均与本机官方 APT metadata 核对，原 metadata、下载来源与完整哈希在 `native-stack/package-manifest.json`、`packages/*.metadata.txt` 和 `packages/sha256sums.txt`：

| 包组 | 实际版本 |
| --- | --- |
| fcitx5、fcitx5-data、fcitx5-modules、libfcitx5config6、libfcitx5core7、libfcitx5utils2 | `5.1.19-1` |
| fcitx5-pinyin、fcitx5-chinese-addons-data、fcitx5-module-punctuation | `5.1.12-1` |
| libimecore0、libimepinyin0、libime-data、libime-data-language-model | `1.1.14-1` |
| libxcb-ewmh2、libxcb-icccm4、libxcb-keysyms1 | `0.4.2-1build1`、`0.4.2-1build1`、`0.4.1-1build1` |
| libxcb-imdkit1、libxcb-xinerama0 | `1.0.9-1build1`、`1.17.0-2ubuntu1` |

路径不是靠猜测：精确 Fcitx5 源码 `src/lib/fcitx-utils/standardpath.cpp:127` 定义 `FCITX_DATA_DIRS`、`XDG_DATA_DIRS`、`FCITX_ADDON_DIRS`；Pinyin 的 `im/pinyin/pinyin.cpp:711` 从 XDG data 读取 `libime/sc.dict`；libime 的 `src/libime/core/languagemodel.cpp:311` 定义 `LIBIME_MODEL_DIRS`。回归脚本显式设置这四项与私有 `LD_LIBRARY_PATH`，实际 `/proc/<owned pid>/maps` 证明载入该目录的 XIM、classic UI、Pinyin/libime；不依赖“启动没退出”判定成功。

官方 `xim.h` 定义的 `UseOnTheSpot=True` 必须在 daemon 启动前设置，否则没有 winit 非空 Preedit；Pinyin enum 必须用官方序列化值 `PreeditMode="Commit preview"`，不是 C++ enum 标识符 `CommitPreview`。私有 `ActiveByDefault=True` 让重建的字段 context 仍为中文。只启用 `xcb,xim,dbus,keyboard,classicui,pinyin,punctuation`，不启用 cloudpinyin、IBus frontend 或 test frontend/UI/engine；`fcitx5-remote` 与私有 DBus 所有者 PID 均回读校验。首次空 HOME 的缺失 `pinyin/user.history` 日志是源码捕获的初始历史读取错误，原日志保留；成功依据真实候选、原生事件与 Signal/撤销断言，不把该日志当作输入成功。

### 可重跑的目标回归

`scripts/native_ime/{session,actions,exercise}.py` 与 `examples/ime_clipboard_reader.rs` 是项目自有、仅验证用源文件；reader 复用锁定的 arboard 依赖，不读取原 `.local/verification/clipboard_reader.rs`，不复制系统工具源码。当前共 **11 个 Cargo examples**，其中 reader 不是 GUI/业务示例。干净 checkout 只用 tracked 文件即可构建两个必需程序：

```sh
export CARGO_TARGET_DIR="$PWD/.local/verification/target"
cargo build --example ime_clipboard_reader --locked
cargo build --example native_ime --locked
/usr/bin/python3 scripts/native_ime/session.py c1 1.0 \
  --clipboard-reader "$CARGO_TARGET_DIR/debug/examples/ime_clipboard_reader" \
  --app "$CARGO_TARGET_DIR/debug/examples/native_ime"
/usr/bin/python3 scripts/native_ime/session.py c2 1.5 \
  --clipboard-reader "$CARGO_TARGET_DIR/debug/examples/ime_clipboard_reader" \
  --app "$CARGO_TARGET_DIR/debug/examples/native_ime"
```

已 provision 的 Xvfb、DBus、xauth、xdotool、xwininfo/xprop、Python/Pillow，以及上述官方 Fcitx5/Pinyin/libime 私有提取根，是合法的**外部系统测试依赖**，不属于 tracked 构建输入，也不随仓库分发。默认提取根为 `.local/verification/native-stack/root/usr`；跨 clean checkout 复用时以 `--stack-root /absolute/path/to/native-stack/root/usr` 指定，以 `--verification-root /absolute/path/to/feature/.local/verification` 保存证据（短路径避免私有 DBus Unix socket 路径超限）。程序路径可显式指定；省略时取进程 `CARGO_TARGET_DIR`（未设置则项目 `target`）下 `debug/examples/`，不会回退至旧 `.local/verification/bin` 产物。程序缺失或不可执行，在创建 run/启动服务前非零退出；先检查 reader，clipboard copy/read/Signal 对比随后**无条件执行**，检查后被移走也必须非零退出，不能跳过后报告成功。源码 manifest 含 reader 与三个 probe 脚本，provenance 同时保存实际 app/reader 路径和 SHA-256；archive 无 `.git` 时 `source_head` 为 null，由外层 archive manifest 绑定提交，而不是误报外层 worktree HEAD。

历史 `f6`—`f9` 使用的原生程序来自 `b3666b6`，SHA-256 `2e3e189e0e73eaa22a5671563d05b70df4d774fe52d5894acd79a3940917eb0a`，见原 `final-source-binary-manifest.json`；旧 reader、失败尝试、hash 与证据均保留，不作为上述 tracked recipe 的输入。

每次使用**新的短 alphanumeric run 名**；已存在目录直接拒绝覆盖。脚本白名单环境，自有 Xvfb 随机 cookie、无 TCP、私有 DBus 与 HOME/XDG/runtime/TMPDIR；不输出 cookie。有限启动等待、180 秒操作超时，结束只回收自己创建的 process groups。所有 PNG、日志、环境、socket/auth、源 hash、进程 maps 和退出状态均留在本 feature `.local/verification/<run>/`；只有 X server 自己创建系统 X socket/lock。不将包或源码 archive 加入公开发布。

保留原 14 项 checkpoint，不减少 native 门禁：非空中文 preedit 不发布、Space commit 仅一次、一次 undo/redo、Latin/中文混合、Esc 不发布、焦点转移/独立字段/卸载无旧提交、真实嵌套滚动不改模型，以及初始/滚动/移动/reflow 的**可见真实候选**锚定。卸载后以实际 Tab 导航定位剩余字段，不沿用已经因裁剪/reveal 变化失效的像素坐标。移动/滚动/reflow 后不输入额外键或更新引擎表格，只等待回读/抓图；维持原 2px 容差。

Fcitx5 对 XIM zero-height spot 的原生 UI 间距必须显式计入：`xim.cpp:331` 构造高 0 的 cursor rect，`xcbinputwindow.cpp:102` 在 96 DPI 的 Xvfb 加 **10 physical px** 再放候选。故精确断言为 `x = window_x + caret_x × app_scale`、`y = window_y + (caret_y + caret_height) × app_scale + 10`；不是事后放宽误差，也不是固定使用旧 popup 位置。该 10px 属于原样系统 UI 的布局；1.5× 是应用 DPI，系统 UI 仍为 Xvfb 的 96 DPI，不冒称动态跨屏或全桌面 DPI 切换。

### FB-3 当前 tracked clean 复现证据

2026-11-13，先提交 helper/harness 为 `ba9cd362e6c895ed34916eb756c2b4b703089865`，再由 `git archive` 解包其全部 **58 个 tracked 文件**到本 feature `.local/verification/clean-checkout/`；初始目录没有 `.local/`、旧 reader/source 或其他 untracked 输入。归档 SHA-256 为 `4918ecd3496bcbab33881089b8de9f070c24a28e8273bbc7b5a06532a0fa7ecc`，逐文件清单见 `.local/verification/fb3/archive.json`，原归档为 `fb3/tracked.tar`。下述命令全部以该 clean checkout 为 cwd，复用 feature 内 `CARGO_TARGET_DIR=.local/verification/target` 的**绝对路径**缓存；Cargo 日志明确记录编译源目录为 clean checkout，而非原工作树。

| 实际命令/检查 | 结果 | `.local/verification/` 下证据 |
| --- | --- | --- |
| `cargo build --example ime_clipboard_reader --locked` | tracked reader 构建通过 | `fb3/build-reader.log` |
| `cargo build --example native_ime --locked` | 正式原生 GUI example 构建通过 | `fb3/build-native.log` |
| `cargo fmt --all -- --check` | 通过 | `fb3/gate-fmt.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过，含新 reader | `fb3/gate-clippy.log` |
| `cargo test --workspace --locked` | 192 unit + 6 doc（4 regular + 2 compile-fail）；计数不变 | `fb3/gate-test.log` |
| `cargo build --examples --locked` | 全部 11 examples 通过 | `fb3/gate-examples.log`、`fb3/gates.json` |
| clean `scripts/native_ime/session.py c1 1.0` / `c2 1.5` | 各 14/14，必需 clipboard 合计 2/2，entrypoint 均 exit 0 | `c1/`、`c2/`，`fb3/session-{1.0,1.5}.log`、`fb3/session-exits.json` |
| Python AST parse / 缺失或不可执行 reader | 三脚本解析通过；普通/`-O` 各两例均 exit 2，未创建 run/服务 | `fb3/python-checks.json` |
| preflight 后删除 reader 的故障注入 | 执行未改写的 tracked clipboard block，仅 stub copy 动作；`FileNotFoundError`，不输出成功摘要；不是额外原生引擎试验 | `fb3/reader-race.json` |

两次 entry 均显式传 `--verification-root` 为 feature 证据根、`--stack-root` 为原已 provision 官方提取根的绝对路径；`c1` 显式传两个 example 程序路径，`c2` 省略这两项，实际证明 `CARGO_TARGET_DIR` 默认解析。二进制均取新构建的 `target/debug/examples/`，没有使用旧 `bin/clipboard_reader` 或 `bin/native_ime-final`。本次产物 SHA-256：

| 源/产物 | SHA-256 |
| --- | --- |
| `examples/ime_clipboard_reader.rs` | `fca5bd7f1885171b4fcd7387396ea17f35f291390c3cd40f4e7975fa770d143b` |
| `scripts/native_ime/session.py` | `d9032a5268318914e798665c0b49990c1022ef57672cd478f7354e5dcdb33bfa` |
| `target/debug/examples/ime_clipboard_reader` | `f8f411ba82f6a696ee39c3580f991106fee6a1b37dcad9bbb1540f81770cc816` |
| `target/debug/examples/native_ime` | `9393eb312cb5954c374ca75504d11a7d2d8d65d70bc3a4c96d21dc2ead9ab040` |

`c1/source-hashes.json`、`c2/source-hashes.json` 中 reader/三个 probe 源与归档 manifest 逐一匹配，两个 `provenance.json` 中 app/reader binary hash 与新构建产物逐一匹配。所有 58 个归档源文件保持原 hash；5357 个既有本地证据/源/产物（排除允许复用的 target 缓存）与 `fb3/history-before.json` 逐一相同，原 `x*`/`f*` 失败及成功、旧 hash manifest 均不覆盖。新证据 hash 入口为 `fb3/evidence-hashes.json`，汇总为 `fb3/qualification.json`。`c1`、`c2` 的四个自有进程均已停止，各自 X socket/lock 已自动消失；无 owner 进程清理、host/provider 配置、密钥读取、push/merge 或计划 CLI 操作。对比 `b828c0c5`，`src/`、`Cargo.toml`、`Cargo.lock` 与正式 `examples/native_ime.rs` 均无变化；新 Rust 源仅验证 helper。正式资格仍限上述 Fcitx5 tuple；IBus 历史各 11/14、三项跟随失败原样保留。这里只修正 scoped reproducibility P2，不宣布整体任务完成。

### 历史实际结果与来源

`f4/`、`f5/` 首次修正配置后的完整流程各 **14/14**；自回收脚本重复于 `f6/`、`f7/`，仍各 **14/14**，clipboard 2/2；最后加入必需 reader 的启动前检查后，提交脚本再跑 `f8/`、`f9/`，仍各 **14/14**、clipboard 2/2。`f6/13-nested-scrolled.json`：实际 caret `(52,178,1.5,24)`、窗口 `(40,40)`，可见候选 `(92,252)`；`f7/16-reflow.json`：caret `(34,134,1.5,27)`、窗口 `(210,130)`，可见候选 `(261,382)`。均有对应独立 popup crop、完整 display PNG、WM_CLASS/PID、操作与原生 Ime/Signal trace，不以空候选或仅 API trace 代替通过。

`f1/` 保留首次未启用 on-the-spot、未正确识别 popup 的失败和空 current-IM 初始化；`f2/` 保留 enum 拼写和旧卸载点击坐标导致的 12/14；`f3/` 是未运行检查的预备启动，不计成功。`x8/`、`x9/` 的原 IBus 各 11/14 与三项失败完全保留，不覆盖旧 `native-summary.json`/报告。

官方 source archive 全部经对应 `.dsc` SHA-256 核对，原包均只保留本地：

| 官方归档 | SHA-256 |
| --- | --- |
| `ibus_1.5.34~rc2.orig.tar.gz` | `6b94af30b257cb8d54fcbb0e72d56d0f2b7a4a62f4632459ee88ba078d28793d` |
| `fcitx5_5.1.19.orig.tar.xz` | `8957c0a5bbd3caad42215496c21666abefe3be7399a38928639f3bb484ed6a1b` |
| `fcitx5-chinese-addons_5.1.12.orig.tar.xz` | `76c1dcadfbaff3bc1b9ade936ecca887021fe87038aa7fe7000d4f5dfd7307fb` |
| `libime_1.1.14.orig.tar.xz` | `5c37a871db829c07f800994b896aaf1e06094fa7a60f95bde54e8dfc15663dfe` |

发行来源为 Ubuntu 官方 `cn.archive.ubuntu.com/ubuntu/pool/` 的 `main/i/ibus/`、`universe/f/fcitx5/`、`universe/f/fcitx5-chinese-addons/`、`universe/libi/libime/`；完整 APT 元数据、manifest、hash、下载/初始资源错误与清理入口见 `native-stack/qualification.json`、`native-stack/report.md`。源码核对不能声称对任意新版 IBus、任意 WM 或其他 Linux 输入栈都验证通过；正式声明仍严格限于本次全部 native gate 通过的 tuple。
