# M2b 初始纵向视口候选

2026-11-13，`feat/scroll-views`，独立工作树 `.worktrees/scroll-views`，基线 `main@4e91628d4c2103058623dc0b310442b28118a180`。M1a/M1b/M1c 与 M2a 已分别合入 PR #6/#7/#8/#9；本切片是**初始纵向滚动候选，待独立审查与远端 CI**，不宣告整个 M2 完成。

## 范围与入口

只做固定逻辑高度的纵向 wrapper、独立内容测量、逻辑 offset/clamp、滚轮剩余链、焦点/显式最近边缘揭示与只读比例指示条。**不实现横向、虚拟列表、惯性、平滑动画、触控手势、文本编辑/IME、主题或 LLM 产品**；M3 未实施。不增加依赖，不改变 Rust CI 门禁，不扩展平台支持声明。

```rust,no_run
use chronobreak::{Application, Element};

let (content, rows) = Element::keyed_column();
let (viewport, scroll) = Element::scroll_view(240.0, content.gap(4.0));
let reveal = scroll.clone();
let selected = rows.clone();
let root = Element::column().child(viewport)
    .child(Element::text("揭示第 20 行").on_click(move || {
        if let Some(target) = selected.node("20") {
            let receipt = reveal.scroll_into_view(target);
            assert!(receipt.result().is_none());
        }
    }));
let app = Application::new(root);
for index in 0..30 {
    rows.insert(index.to_string(), Element::text(format!("中文第 {index} 行")));
}
app.run().unwrap();
```

- `Element::scroll_view(height, content) -> (Element, ScrollHandle)`：`height` 为有限、非负逻辑像素；wrapper 固定高度、不参与纵向压缩，内容独立测量，宽度由原有 flex 布局约束，中文沿用原 WordOrGlyph 换行。可用 `.height(f32)` 表达有限尺寸，修改 builder 仅在挂载前生效，不新增动态尺寸绑定 API。
- wrapper 恰好拥有一个内容子树，再 `.child(...)` 明确拒绝；长列表、padding/gap/背景等通常放在 content 内。wrapper 的背景和比例条固定，子树的文字/背景一起移动，不把长文本自身误当 viewport。窗口或非滚动祖先裁剪 wrapper 自身时，滚动不能把这个视口变大或取消裁剪；狭窄窗口可能裁剪示例视口之外的控制区。
- `scroll_to(offset)` / `scroll_by(delta)`：排队，有限值有效，最终限制到 `[0, max_offset]`，负值可回顶。调用本身不修改 Scene；在 UI 点击/输入/消息回调中提交后由既有提交/重绘循环处理，无 Scene/Renderer 或手动 redraw。UI 句柄不可跨线程；后台线程仍使用 M1c 消息入口。
- `scroll_into_view(NodeId)`：目标必须是当前视口的已挂载、未隐藏后代；先揭示内层，再揭示到该句柄作用域为止，不越过该视口移动外侧滚动祖先。自动焦点揭示则遍历所有滚动祖先。有效的 disabled 目标可显式揭示，但不会因此变成可 Tab 聚焦。
- `id()` 返回完整场景/槽位/代际身份；`snapshot()` 返回最后提交的 `offset / max_offset / viewport_height`，不是 Signal，也不是实时订阅。`ScrollReceipt::result()` 在排队期间为 `None`，提交后为 `Some(Result<(), ScrollError>)`；保存 receipt 可在后续 UI 边界查看，禁止轮询/定时刷新画面。

错误为 `NotMounted / SceneDropped / StaleViewport / ForeignScene / StaleTarget / OutsideScope / Hidden / NonFiniteOffset`。作用域含严格后代，不接受视口自身或兄弟。错误命令不移动任何视口；目标校验发生在最终结构命令提交后，删除并复用槽位也不能命中新节点。隐藏视口（含隐藏祖先）清 offset/max，恢复从零开始；卸载清 snapshot 并使句柄终止，旧 ID 只用于诊断，不延长节点/队列生命周期。Scene 关闭完成剩余 receipt 并释放句柄状态；已卸载句柄保持 stale 终态。

## 测量、几何与提交

1. 沿 M1b 提交结构快照、属性最终值及文字格式化；需要时运行 taffy/文字测量。extent 来自未平移、未裁剪的唯一内容子节点布局末端，`max = max(content_bottom - viewport_height, 0)`，不从屏幕 bounds 推导。
2. 按最终内容尺寸 clamp，覆盖动态 insert/remove/hide、文本变短/中文重排及布局尺寸变化。先排队 `scroll_to(250)` 再同批增大内容，使用新 max，不先被旧 max 截断。固定高度本身不随窗口缩高而变小，窗口宽度变化仍会重排内容并重算 max。
3. 顺序应用滚动命令，再执行待处理焦点揭示；同边界焦点转移只胜出一次。纯 offset 变化只重新 placement/clip/hit，不进入 taffy 或文字整形；命令净效果不变则不 placement、不标记 present。`geometry_version` 与最终提交一致。
4. placement 沿祖先累减 offset，wrapper 自身不减自己的 offset，子树统一移动；每级 clip 与祖先 clip 求交。Renderer 的文字位置/可见准备、矩形、显示顺序、命中、焦点/hover 框使用同一 bounds/clip；被裁剪节点继续持有模型、keyed 身份与订阅，既不卸载也不虚拟化。
5. 最后重算静止指针 hover。滚动后紧接点击（尚无 RedrawRequested）仍命中新几何；内容后台变化亦沿原 flush 更新 hover。捕获 owner 不因滚出 clip 而消失，拖动事件坐标仍是窗口逻辑坐标，不冒充内容局部坐标。

仍是全帧绘制，不声称 GPU 局部重绘或大规模虚拟列表性能。100ms surface Retry、零尺寸/遮挡暂停呈现、idle Wait、M1c 每轮最多 64 条与 M1a 资源回收保持不变。

## Wheel 路由与默认链

`EventKind::Wheel(WheelInput)` 保留 winit 原始 `MouseScrollDelta`（横向分量仅保留，不默认执行）、`TouchPhase`；`UiEvent` 同时保留修饰键与可选窗口逻辑位置。`WheelInput::vertical` 为归一化纵向量：每行 **40 逻辑像素**；physical pixel 除窗口 DPI；取原始 y 的负号，正数使 offset 增加、内容向上。phase 只是原始信息，不启动惯性/触摸/定时器。

命中使用实际指针路径，不服从 pointer capture。无指针位置不假造 `(0,0)`，没有 Wheel target/默认滚动；沿用 M2a disabled 子树不穿透命中规则。先走既有 capture→target→bubble，整个事件及默认链仍处在同一 batch：

- `stop_propagation()` 只截断监听器访问；同一节点/phase 的后续监听器及默认动作仍可执行。
- 任一已访问监听器 `prevent_default()` 阻止**整条**默认滚动链。
- 未 prevent 时，不论监听器冒泡停在哪，都从真实命中路径的最内层有效视口开始。每层只消费 `new_offset - old_offset`，仅把 remainder 交给滚动祖先；内层到边界继续交外层，部分消费保留精确余量，全部消费后停止。不是每个 bubble handler 再滚一遍。
- 回调中的结构命令/隐藏仍延迟提交，不半途改写当前路径。边缘/no-op/被 prevent 的滚轮本身不请求帧；若另有 hover、业务或后台内容变更，则按那些变化呈现。

## 焦点、超大目标与比例条

Tab/Shift+Tab 保持 M2a 稳定有效树序。只在焦点**转移**时排队一次 reveal，安全提交时由内到外最近边缘对齐：小目标在上方对齐其上边缘，在下方对齐下边缘；已可见不动。不会在遍历中变更几何，也不会每次 flush 因旧焦点离屏而吸回手动滚轮或 `scroll_to/by`。原 Enter/Space 激活及 prevent 规则不变；此切片没有新增 PageUp/PageDown/Home/End 默认滚动。

目标比视口高时：若跨过视口两边，维持当前 offset；整体偏上时对齐其下边缘，整体偏下时对齐其上边缘，再 clamp。对零高视口、嵌套/非滚动祖先裁剪，仅承诺这个有限几何政策，不承诺不可能的完整可见，也不改变非滚动祖先的尺寸/clip。

2026-11-13 P2 修复保留上述**每个局部视口**的超大目标政策：处理内层 offset 后，将目标纵向区间与该视口的可见区间求交，再平移到父坐标；外层只揭示剩余可显示片段，不携带已裁掉的原始高度。中间非滚动节点同样按自身布局高度求交，不取消它的裁剪；空区间停止向外揭示，区间高度不为负，也不反复改 offset。显式揭示与焦点揭示共用此路径。

精确回归：outer 高 100、offset 0，内容为 spacer 200 + inner 高 40 + spacer 200，inner 的 target 高 200；先将 inner offset 设为 160 并提交。显式 outer reveal 或 Tab 后 inner 仍为 160、outer 为 140，inner 在窗口 y=60..100 完整可见，target 的最终 clip 与它相同且高 40；不再错误停在 outer offset 40、inner y=160..200。另测中间非滚动祖先仅允许 20 / 0 高片段时的最终 clip、重复命令不 churn，以及焦点揭示后的手动滚动不吸回。

比例指示条在该 wrapper 子内容之后绘制，受同一 clip 约束：宽最多 4 逻辑像素，`thumb = viewport² / (viewport + max)`，位置随 `offset/max` 线性变化；无溢出不绘制。它是**非交互指示条**，不拦截命中、不可拖动、无伪节点/完整 scrollbar 语义或无障碍认证承诺，不引入主题。

## API 迁移

原 `Element`、`Children`、Signal、消息和 on_click 仍可使用，不要求业务接触 Scene/Renderer。新增 scroll builder/handle/snapshot/receipt/error、`Element::height`、`WheelInput` 以及 winit `MouseScrollDelta / TouchPhase` 再导出。穷尽匹配 `EventKind` 的应用需处理 Wheel；因 Wheel 含浮点数，`EventKind` 从 `Eq + PartialEq` 改为 `PartialEq`，`KeyInput` 仍保留 Eq。`UiEvent.position`/捕获坐标仍为窗口逻辑空间。

## 验证与保留证据

所有构建目标、临时目录、软件渲染缓存、原始二进制、脚本、日志与匹配窗口裁剪截图位于本工作树 `.local/verification/`（忽略提交，交协调者审阅后清理）。基线 88 unit + 4 doc；P2 修复后 **114 unit + 4 doc 全通过**，其中 26 个直接滚动回归（原候选 23 + 本次 3），未删除或弱化旧测试。原候选 `9a71bf6` 的证据保持历史身份；协调者已发布相同内容为 `a355281fb7924b49ba23205684219639fb9ea26d` / PR #10，本次保留本地提交链，不 reset、push、认证或合并；仍待协调者复审与远端 CI，不宣告 M2b accepted。

四个精确门禁均通过：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

修复后本地设置 `CARGO_TARGET_DIR=$PWD/.local/verification/target`、`TMPDIR=$PWD/.local/verification/postfix/tmp`；四个精确门禁日志为 `postfix/fmt.log / postfix/clippy.log / postfix/test.log / postfix/build-examples.log`，退出码见 `postfix/gates.tsv`。`postfix/regression-before.log` 记录修复前显式/Tab 两例均实际失败（outer 40 而非 140）；`postfix/scroll-tests.log` 记录修复后 26 项滚动测试通过。原根目录门禁日志不覆盖。CI `.github/workflows/rust.yml` 保留同样四项，未声称远端已通过。

`src/scroll_tests.rs` 覆盖有限视口长内容、line/pixel/DPI/sign、嵌套部分余量与正反边界链、stop/prevent 分离、无指针、真实 hit 与捕获分离、内容隐藏/移除/文本缩短/重排/视口改尺寸 clamp、增长前 scroll_to、未 redraw 的新命中/clip、静止 hover、捕获坐标、Tab/反 Tab/安全 reveal、scope/foreign/stale/hidden/超大目标、旧焦点不吸回、offset-only placement、no-op/净零命令不 layout/present、可见文字/订阅与卸载/关闭句柄。

### P2 修复后 GUI 复验

2026-11-13，以最终源码重新通过四项精确门禁后保存 `postfix/bin/scroll_views`，SHA-256 为 `2622c11df6ac9dc3eceda3b849f53adb0592e8612aa12fa864e1ac8a209ef5b2`。`postfix/bin/SHA256SUMS`、`postfix/SOURCE_SHA256SUMS` 与 `postfix/provenance.json` 绑定二进制、两份修改后 Rust 源码及本地父提交 `9a71bf6`；不是把旧二进制或旧截图标为本次证据。

复用未修改的 `gui_setup.py / gui_checks.py / send_close.py`，由 `postfix/replay.py` 将运行目录及二进制切到 `postfix/`；隔离 Xvfb + XTest + Pillow / Vulkan CPU adapter，两种 DPI **各 16/16 项通过**，包含显式 reveal、Tab/Shift+Tab 嵌套揭示、缩减 clamp、窄窗中文重排与恢复、滚轮链/no-op/idle。结果分别为 `postfix/gui-1x/checks.json`、`postfix/gui-1_5x/checks.json`，每组保留新 `app.log / screenshots.tsv / initial.png / run1-*.png`。原 1×、1.5× 与 M1c 记录和二进制均不覆盖；本次不重跑大型 M1 压测或 M1c GUI 烟测。

已查看两种 DPI 的 Tab、反向 Tab、显式外层 20 行、缩减及窄窗截图（本地 `postfix/visual-review.png` 汇总）；并核验全部新截图哈希，见 `postfix/evidence-check.log`。两次应用均经 WM_DELETE_WINDOW 正常退出 0（约 36ms / 31ms）；独立服务器随后停止，`postfix/gui-*/cleanup.json` 记录两端均已终止，未操作宿主应用。仍是软件渲染/X11 的有限 GUI 复验，不代表原生 Wayland 或 GPU 资格；精确超大目标组合由上述几何回归验证，不声称演示 GUI 包含该 200/40/100 用例。

### 历史候选 GUI 证据（修复前）

以下记录对应原候选二进制，不是修复后运行。原 `bin/scroll_views` SHA-256 为 `10839163d41b049bb1ca7efe6350dec8f2972536bf755a9e09d2870e40a5c9f7`，`bin/background_messages` 为 `851f496666d76f4b5f549321d69a1c211e4e0b33cf33564aa4340aee3580d8af`，本次未覆盖，校验仍通过。

真实图形路径是隔离 Xvfb + xdotool/XTest + Pillow，Vulkan llvmpipe；普通尺寸逻辑 760×520，实际裁剪为 1.0× 的 760×520 和 1.5× 的 1140×780；窄窗逻辑 410×650，分别裁剪 410×650 / 615×975。使用匹配窗口坐标裁剪，桌面外的黑色不纳入判断。`gui_setup.py / gui_checks.py / send_close.py` 保留实际操作与协议关闭路径，`bin/` 保留原候选验证时原始二进制与 SHA256SUMS。

两种 DPI 各 **16 项 GUI 检查通过**：内层先滚、移动后点中内层 02 行且独立计数 +1、部分/到边界链、reset、prevent、Tab/Shift+Tab、显式外层 20 行、动态缩减 clamp、窄窗中文重排、外层边界/no-op、idle 和无 panic。证据为 `gui-1x/checks.json` 与 `gui-1_5x/checks.json`，原始 `app.log`、`screenshots.tsv` 及 PNG；1× 完整成功序列 `run2-*`，1.5× 为 `run1-*`。人工查看移动点击、反向 Tab、显式揭示、缩减、窄窗与 1.5× 对应截图，不仅检查文件存在。

实际值：初始 outer max 1044、inner max 306；点击部分可见行的一次焦点揭示使 inner 从 80 对齐到 74，之后 240 纵向量中 inner 消费 232、outer 接 8，再到边缘接 80；缩减后 outer clamp 到 8、inner 到 0。窄窗 max 为 outer 1764、inner 594。原始日志显示此 XTest 后端一次 wheel 按下/松开各交付一个 `LineDelta(-1)`，所以一对产生两次 40；框架每个收到的 winit 事件消费一次，不伪造硬件滚轮校准或去重策略。单位/部分余量另由确定性测试断言。

1× 边缘追加 5 对滚轮后帧数维持 71，1.5× 维持 55；再静置一秒帧数不变且相邻窗口裁剪逐字节一致。idle PNG SHA-256 分别为 `8cb4409b8248b090a34733a56dbb54144e230fdfdb4d6a744616b9c5bc1b0aca` 与 `b6a2d5d09576a9a2ad5af0e3a2b4431f57da51ec40437a88167f90f45ff168f8`。纯滚轮日志的 layouts/formats 不增长；没有 1000 帧长跑。

M1c 短烟测 `background-smoke/` 验证真实后台线程无输入交付到 10/10、随后实际点击计数 +1、空闲帧稳定为 14，并经 WM_DELETE_WINDOW 正常退出（约 30ms）；保留日志、checks.json 和三张窗口裁剪图。未重跑无关的 20,001 条突发长验收，完整公平/关闭逻辑回归仍在 111 项中。

验证限制：Xvfb 的 EGL/DRI3 警告后实际选择 Vulkan CPU adapter，不是 GPU 硬件资格。初始失败探针误选宿主 Wayland，且非 root 的 `-nolock` 未抑制自身 X191 lock；停止后仅清除此探针自身陈旧 lock，保留 `probe-1x/` 日志。后续修正为取消 Wayland 环境、`-displayfd` + 禁用 unix/local transport 的独立 TCP X server，未再创建 `/tmp` socket/lock，未操作宿主应用。未实测原生 Wayland、IME、真实硬件滚轮/触控、Windows/macOS 或系统无障碍；不把 winit M2 事件当作 M3 支持证据。
