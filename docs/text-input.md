# M3B：受控单行输入

2026-11-13；本页保留 M3B 合同及历史验收，不另维护当前候选状态。当前状态统一见 [Alpha 状态](alpha-delivery-status.md)，后续原生组合输入合同及资格缺口见 [原生 IME](native-ime.md)。

## API 与所有权

```rust,no_run
use chronobreak::{Application, Element, Result, Signal};

fn main() -> Result<()> {
    let value = Signal::new("你好 ASCII 👩🏽‍💻".to_owned());
    let readonly = Signal::new(false);
    let disabled = Signal::new(false);
    let field = Element::text_input(&value)
        .readonly_when(&readonly)
        .disabled_when(&disabled);
    Application::new(field).title("单行输入").run()
}
```

- `Element::text_input(&Signal<String>)` 是真实可编辑叶节点，默认加入 Tab 顺序；可设置字体、颜色、背景及已有事件路由。静态属性为 `.readonly(bool)` / `.disabled(bool)`，动态属性优先于对应静态值。没有公开 Scene、Renderer、编辑器句柄、手工 flush 或请求重绘 API。
- 每个有效 `NodeId` 独占一个 `TextEditModel`、定向 selection、视觉 affinity、局部水平偏移和 blink deadline；同一个 Signal 可绑定多个独立字段。卸载销毁历史/订阅/拖选/光标；同 key 重建不继承旧状态。系统剪贴板服务属 Scene，保持最近复制内容的所有权直到应用关闭，不通过字段订阅闭包反向拥有 Scene。
- `examples/text_input.rs` 是真实库调用者：中文、ASCII、emoji、两个字段共享受控值、字节数同步状态、readonly/disabled 和外部替换按钮、嵌套纵向视口。运行 `cargo run --example text_input --locked`；不是可编辑多行原文工作台。

## 同步、换行与历史

- 每个实际改字的默认动作只向受控 Signal 发布一次最终 String。选区、光标、复制、边界删除等不改字动作不发布；不在 formatter 中写业务状态，也不存在 Signal→handler→Signal 的强引用环。
- 同值 echo 保留光标、方向、历史与局部水平位置。外部值先按单行政策投影，相等/不同以投影后的模型文字判断；不同文字调用 M3A `replace_programmatic`：清 undo/redo，按新字符串字符簇边界向后取整/clamp 端点，而不是无条件跳到末尾；取消旧拖选与异步编辑资格。
- 单窗口只有一个焦点字段。共享 Signal 的用户编辑在当前字段保留用户历史，在其他字段作为程序替换处理，不抢焦点。程序更新按 UI 线程顺序确定；路由监听器先改受控值时，默认编辑读取该 Signal 最新快照并先建立外部替换屏障，再执行本次编辑。若应用订阅者随后再次替换值，最后一次程序写入生效，不暗中覆盖应用权威值。
- **仅本单行字段**将 CRLF 转一个空格，单独 CR、LF、NEL（U+0085）、LS（U+2028）、PS（U+2029）各转一个空格。粘贴与实际打印载荷使用该规则；Enter 不插入换行。键盘控制字符不作为打印文字插入。
- 程序给出含换行的值时只作单行显示/模型投影，不回写原 Signal；导航/选择也不回写。首次实际用户改字后才发布完整的单行编辑结果。因此此构造器只适用于业务明确为单行的字段，**不能绑定工作台原始源文并假称保真**。普通 `TextEditModel` 和普通文本节点不改变其原文/换行合同。
- 每个有效打印载荷、删除、成功 cut/paste 独立成一步；复用 M3A 有界快照历史，无按时钟自动合并。undo/redo 恢复文字与定向选择；相同 echo 不形成屏障，外部不同值形成屏障。

## 键盘与指针

`KeyInput` 增加公开 `text: Option<String>`，由 winit 的 `KeyEvent.text` 转换。**Alpha 源码迁移**：旧 struct literal/fixture 必须补上该字段，无打印载荷用 `None`，字符测试填真实 `Some(...)`；不从 `logical_key` 猜打印内容。logical/physical/location、修饰键、repeat 和 synthetic 仍相互独立。

- Left/Right：逻辑字符簇；Ctrl/Super 或 Alt + Left/Right：M3A 逻辑词；Home/End：字符串首尾；Shift 固定 anchor 扩展；Backspace/Delete 同样支持字符簇/词。Ctrl/Super+A/C/X/V/Z、Shift+Z、Y 为全选/复制/剪切/粘贴/撤销/重做。
- Ctrl/Alt/Super 组合不会插入其 `text`，包括 winit 返回字母文本的 Ctrl+C；不宣称所有平台 AltGr/Option 输入习惯均已适配。synthetic 与组合保护期间不编辑。真实 OS repeat 和重复物理按下允许打印/删除，不放宽既有按钮的 Enter/Space 去重。Tab 仍交回默认焦点遍历，Enter 在字段内被消费而不插换行。
- 默认编辑发生在路由之后，capture/target/bubble 的 `prevent_default()` 可阻止；仅 stop propagation 不阻止。结构/可见性/禁用属性沿用 M2 的正式提交合同；想在当前监听器阻止当前默认动作应明确 prevent。
- readonly 可聚焦、导航、选择和复制；不改字、不 cut/paste/undo/redo、不 blink。disabled（含祖先禁用）不参与编辑、选择默认动作、激活或焦点顺序。动态 readonly 取消正在进行的拖选，之后允许新选择；动态 disabled 清理捕获并执行既有焦点回退，二者不删除已有模型历史。
- 指针左键采用 M2 节点级 capture，以当前窗口逻辑坐标命中；Shift+click 扩展，拖出行/窗口仍取最近合法视觉停靠点；up/cancel/capture loss/focus loss/卸载清理拖选。不使用桌面级 grab/confine，也没有持续自动拖动滚屏计时器。
- 指针默认 focus 不请求焦点揭示，避免 PR #10 已修正的点击位置突然被滚走；Tab/显式焦点仍通过既有嵌套纵向视口揭示新焦点。字段自己的水平 caret tracking 独立于纵向 offset，**不交付通用横向 ScrollView**。

## 整形、绘制与局限

复用真实 cosmic-text `Buffer`（Advanced shaping、Wrap::None）的 glyph cluster、x/width 与 bidi level；只在完整字符串的 Unicode 字符簇边界建立停靠点。一个整形 cluster 内含多个字符簇时，按数量分配其真实整形 advance，供 hit/caret/选择共同使用；不是固定字宽或 UTF-8 字节插值，也不宣称读取字体 GDEF 的精确 ligature caret table。

同一逻辑边界可有前/后视觉 affinity，指针保存所选边；键盘移动仍是 **逻辑顺序**，移动后选择既定 affinity，尚无完整视觉 bidi 导航/平台方向策略。选区按各 glyph 的字符簇片段绘制，避免用跨越所有混向内容的单一包围矩形。覆盖中文、组合附标、ZWJ emoji、拉丁连字与基本混向，不将这些 fixture 称为全部脚本资格。

字段内边距为 4 logical px；选区在文字之前、caret 在文字之后，三者同用内 clip；焦点环仍由 M2 外框绘制。过长单行不换行，通过私有水平偏移让活动 caret 留在内视口；Home/End、鼠标/键盘选择、编辑及窗口 resize 都复用同一整形几何。仅光标/选区/水平偏移变化不使 taffy/文字测量变脏。

## 系统剪贴板与晚到结果

- 使用锁定的 `arboard 3.7.0`，关闭 image-data 默认特性，仅文字；Linux 当前为 X11/XWayland 服务，不是内存 mock。Windows/macOS 后端随库编译路径保留，但本块未实测；原生 Wayland 无可用 X11 服务时会报错，不标成功。
- 惰性启动单个后台 worker，持有长寿命 `Clipboard`，避免 X11 在复制后立刻失去 owner。UI 请求使用容量 8 的非阻塞队列；结果走 M1c 容量 8 的通道，receiver/结果处理/Signal 留在 UI 线程。无永久轮询，无后台修改 UI，无请求泛滥时无界增线程。
- 满队列、服务不可达、转换失败等写 stderr；不以空字符串伪造读取成功。cut 只有系统写入成功且 ticket 仍有效才删除选择；失败保留原文。空 paste 不删选区。copy 的系统所有权操作一经发出，不保证因后续失焦撤回，但不会修改字段。
- 编辑 ticket 包含场景/代际 `NodeId`、动作 epoch 和从入队起 5 秒失效点。改字、导航/选择、外部不同值、失焦、readonly/disabled、卸载、过期任一条件均可拒绝晚到的 cut/paste；结果不迁移到新焦点或同 key 新节点。多个同 epoch 的异步编辑结果最多首个有效结果成功，其余因 epoch 改变失效，不保证每个重复 paste 都排队执行。
- 平台读取不在 UI 线程等待；X11 库普通 selection 等待有约 4 秒 timeout，恶意/缓慢 INCR 或连接本身可能更久，但仍仅占该 worker，UI 不等待 join/读取。5 秒 ticket 让过晚结果不能改字；worker 未返回时后续请求有界拒绝。关闭 Scene 关闭 UI 通道/请求发送端，worker 返回后自行释放 owner。不是自研剪贴板协议，也不承诺应用退出后的剪贴板持久化。

## 闪烁与空闲

只有聚焦、可编辑、未禁用且文字/caret 的有效内容 clip 非空的字段拥有 500ms deadline；该 clip 复用 `text_geometry` 的 4px inset 与祖先裁剪交集，不以外框 clip 非空替代。编辑/选择重启可见 caret；失焦、只读、隐藏/内容裁至零（含仅 padding 可见）、窗口零尺寸、occluded 或 suspended 停止 deadline，恢复内容可见时重启可见 caret，blink 自身不触发布局或格式化。当前活动 caret 是**有界低频动画，不是零空闲帧**；无焦点/只读/遮挡时才回到无限 Wait。

Application 将 caret deadline 与既有 100ms surface Retry 取最早 `WaitUntil`，不引入 60fps 常驻循环。到期仅要求 paint，不增加 layout/format 计数；仍复用已有 Renderer 的文本 prepare/绘制路径。真实 GPU surface 故障分类/Retry 用逻辑回归验证，不假称本次制造了硬件丢失。

## 验证与证据

全部产物、脚本、源码快照/哈希、二进制和日志保留在此隔离工作树 `.local/verification/`，不提交截图或工具产物；未写 main 或操纵用户桌面。Xvfb 的短寿命 X server 锁属于系统运行时机制，不是保存在 `/tmp` 的测试产物；使用 TCP-only X display 避免 Unix socket，所有启动的服务均自行清理。

四项最终门禁：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`cargo build --examples --locked`。最终结果和原始日志索引见 [Alpha 状态](alpha-delivery-status.md)。FB-2 后为 176 unit + 6 doc，较 150 unit + 5 doc 基线新增 26 + 1；纯逻辑覆盖受控 echo/外部替换/共享值/路由内程序写、载荷/repeat/快捷键/default flags、Unicode/历史、真实整形位置、capture/clamp/clip/hscroll、readonly/disabled/卸载/晚到结果、blink/retry/无焦点无忙循环。

### 历史实际 GUI（`3d0f6af`）

`gui_verify.py` 在专用 Xvfb 1600×1100×24、无 WM、`localhost:137` 上启动真实 `text_input` 示例，逐次以 pid + 精确标题查询 window id 并读回几何。winit `WINIT_X11_SCALE_FACTOR=1.0/1.5`；初始窗口分别 760×520 / 1140×780，resize 至 420×600 / 630×900。Pillow XCB 按当前窗口几何精确裁图，不把窗外黑桌面或缺失标题当绘制缺陷。适配器实际记录为 NVIDIA RTX 2060 / Vulkan，不沿用旧块 llvmpipe 推断。

两 DPI 各 20 张 PNG，`gui-summary.json` 保留逐图 SHA-256、窗口位置/大小、外部 reader 实际字符串、每项断言与 blink 差异框；`gui-{1_0,1_5}.log` 保留真实键盘事件、Signal 通知、layout/format 和 Wait/WaitUntil 轨迹。`source-manifest.json` 关联历史提交、源文件、示例及 probe 二进制与脚本哈希；`contact-sheet.png` 供逐图概览。FB-2 不覆盖或重标这些证据；旧 GUI 二进制 SHA-256 `78c1500c…` 仍可在 `target/debug/examples/text_input-173b8631ecd63b6e` 找到，未将任务开始前已变动的无后缀构建别名当作旧 GUI 二进制。

| 两 DPI 均实际执行 | 可复核证据 |
| --- | --- |
| XTEST 输入 `ASCII Ω`，持 a / Backspace 产生 OS repeat | 外部进程 reader 逐字相同；trace 有 `repeat: true`，打印和删除有效 |
| `中é👩🏽‍💻` 末尾 Shift+Left，Backspace、Left/Delete、undo/redo | 外部 reader 只读到完整 `👩🏽‍💻`；组合 é 一次删除而不拆簇；`unicode-grapheme-selection` |
| 外部 writer 写中文、组合 é、ZWJ emoji、拉丁文字、希伯来文和 CRLF，字段 paste；copy/cut | 独立 `clipboard_probe` 进程通过真实 X11 selection 读写；cut 后 reader 是规范化全文，字段为空；undo/redo/undo 逐字恢复 |
| Home、Ctrl+Shift+Right、Shift+End | 外部 reader 分别读到 `Hello` 和全文；`word-selection` / `keyboard-selection` |
| 左键拖出行并释放，复制 Unicode 选择 | 外部 reader 等于完整目标字符串；`pointer-selection`；不是仅在内部断言某个 String |
| readonly 开关、选择/复制、打印/删除/cut/undo；disabled 点击/键盘与 Tab | 受控值不变；`readonly` / `disabled-tab-skip`；静态与动态清理另有 unit 回归 |
| 外部替换按钮后 Ctrl+Z | 共享值为 `外部替换 External replacement 🦀`，旧历史不可撤回；`external-replace-history-barrier` |
| Tab 经三个按钮到第二个共享字段 | 纵向 offset 0→54，局部 caret 独立；`nested-tab-reveal` |
| 1,410-byte 长文、Home/End、实际 resize | `long-{home,end}` / `resized-long-{home,end}`，caret 与文本同用内 clip，无多行排版 |
| 聚焦 blink；独立 Xlib 窗口完全遮挡后移除；清窗口焦点 | 约 1.4s 仅 3 帧，layout/format 保持 44/118；PNG 差异仅 caret：1.0 为 `(140,107)–(142,134)`，1.5 为 `(210,160)–(213,201)`，右下端为排他边界。遮挡/失焦各 1.2s 无新增帧；trace 有 Occluded(true/false)，揭开恢复，失焦末尾 Wait |

`clipboard_probe.rs` 是独立真实系统服务调用者：writer 持有 owner 等待，reader 在另一进程新建系统句柄读取；其 source/binary hash 与 GUI source/binary provenance 一同归档。第一轮 GUI 因继承 Wayland 环境而启动失败，移除**仅子进程**的 `WAYLAND_DISPLAY/WAYLAND_SOCKET` 后重跑；没有调整用户会话配置，也没有将失败当黑帧。

### FB-2：内容 clip 闪烁回归

2026-11-13；只修正 blink eligibility。两项直接回归先在旧逻辑失败，再在修正后通过：正外框下 `.max_width(8)` 导致内容宽度为零，以及真实 scroll ancestor 仅留下 1/2/3/4 逻辑像素 padding。两者均断言清空 deadline，settle 后跨 6 个 500ms 周期不再要求 present；恢复内容 clip 后重新显示 caret、恢复 500ms deadline，随后正常闪烁且 layout/format 计数不变。恢复几何所需的真实 layout 不计作 blink 诱发布局。

独立证据目录 `.local/verification/blink-clip/`：`regressions-{before,after}.log`、四项门禁日志、`source-manifest.json` 与 `gui-summary.json`；使用独立 `target/`，保留旧目录的全部二进制、40 张图和 manifest/hash。新示例二进制 SHA-256 `88ec04803310e6b7a036be4dd989a6efd8fb675780f91c0cbe84ea912f7a3df1` 在专用 `localhost:138` / NVIDIA Vulkan 上实际启动，窗口分别为 760×520 @1.0、1140×780 @1.5；新初始截图、逐次 Cua 调用和帧轨迹单独存档。

此前 Cua 0.38.0 尝试未完成 GUI clip/restore：background 输入未到达 winit，resize 返回 `suspected_noop`，foreground 返回 `foreground_unavailable`。这些是该工具在无 WM Xvfb 上的方法限制，**不是项目 headless 路径的外部阻塞**；原调用、截图、哈希与全部停止进程记录保持不变。

同日补做既有 **Xvfb + 直接 xdotool/XTEST + Pillow** 路径，1.0× / 1.5× 四项 clip/restore 均通过。仅在自有 `-displayfd` 分配的 `localhost:2` 启动窗口，无 WM、无 Cua；子进程移除 Wayland 变量，`windowfocus` 直接设置 X focus，真实 Tab 聚焦后按 `x` / BackSpace 产生公开 Signal 通知 `Root inputx` → `Root input`。使用独立 `root_input.rs`，通过公开 `Element::text_input(&value)` 构建根字段并链接 FB-2 缓存库；不修改运行时代码、不注入私有状态、不触碰主桌面。

几何同时由 X11 `getwindowgeometry` 与 winit presented trace 读回；每个 DPI 的两项均缩放**同一个已聚焦窗口**，位置保持 `(40,40)`。模型/clip 基础来自 `Scene::layout/place`、默认 18px 字号与 `text_geometry`：根宽跟随 viewport，高为 35 逻辑像素，outer 为根 bounds 与 viewport 的交集，内容再取 4px inset 与 outer 交集。截图根字段下边界实际在 y=35 / 52 物理像素；这些模型尺寸是源码合同与像素交叉核对，不冒称私有几何遥测。

| 情形 | 1.0× 实际物理窗口 | 1.5× 实际物理窗口 | settle 后各等待 1.35s | 恢复后的帧 / layout / format |
| --- | --- | --- | --- | --- |
| 宽 8 逻辑像素；outer 为 8×35，内容宽 0 | 8×520 | 12×780 | 两 DPI 均 11 → 11 帧；保持 X focus，无 FocusOut，trace 为 `Wait` | 恢复 760×520 / 1140×780 后均 14 → 17 帧；layout/format 固定 5/2 |
| 高 4 逻辑像素；outer 为 760×4，仅顶部 padding 可见，内容高 0 | 760×4 | 1140×6 | 两 DPI 均 19 → 19 帧；保持 X focus，无 FocusOut，trace 为 `Wait` | 恢复 760×520 / 1140×780 后均 22 → 25 帧；layout/format 固定 7/2 |

恢复后的相邻帧 PNG 差异仅为 caret：1.0× `(93,4)–(94,31)`，1.5× `(139,6)–(141,46)`，右下端为排他边界；每次恢复仅几何重排增加一次 layout，随后跨多个 blink 周期无 layout/format 增量。证据在 `blink-clip/xtest-attempt/`：`summary.json`、`native-calls.jsonl`、`app-{1_0,1_5}.log`、16 张匹配窗口 PNG、公开 probe/验证脚本与 `provenance.json`。首次 verifier 把启动时正常的 `Focused(false)` 当成聚焦后 FocusOut，已原样保留于 `xtest-startup-assertion-attempt/`；修正断言观察起点后通过，不是 XTEST 原生错误。所有自有应用均以 SIGTERM 停止，Xvfb 退出 0；不宣称验证 WM 关闭协议。旧 40 图、5 项历史门禁日志、历史二进制及 FB-2 原 49 项证据哈希再次核对不变；176 unit + 6 doc 门禁沿用已通过结果，未重跑全场景。

仍未验证：真实原生 IME 引擎/preedit/commit/候选窗及系统取消生命周期（M3C）、全视觉 bidi、AltGr/Option 平台组合、原生 Wayland、Windows/macOS、多屏动态 DPI、实际零尺寸硬件窗口与 surface 丢失。零尺寸/Retry/过期剪贴板等边界由定向 unit 覆盖，不冒称真实系统注入。M3C 应在现有 focused editor、实际文本载荷/组合保护、整形 caret 几何处分层接入，不修改 M3A 原文模型或暴露渲染内部。
