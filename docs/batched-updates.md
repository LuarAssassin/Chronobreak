# M1b：通知、批处理与提交边界

2026-11-13（America/Los_Angeles）。`feat/batched-updates` 基于 `main@e2aa129e1b70a4ae5f7344817301ef0910e1c1b8`，仅修改独立 worktree。状态为 **M1b 候选，待独立审查及远端 Rust checks**；不是整个 M1 完成。M1a 的身份、卸载、资源与 1000 轮逻辑生命周期回归保留，本轮不重复真实 1000 轮图形压力。

## 公开 API 与迁移

唯一新增业务 API 是 UI 线程的 `chronobreak::batch`（亦可从 `signal` 模块导入）；现有 `Signal::new/get/update/subscribe`、RAII `Subscription`、Element 和 Children 用法不变。不公开 Scene、调试计数或手动 flush。

```rust
use chronobreak::{Signal, batch};

let left = Signal::new(0);
let right = Signal::new(0);
let result = batch(|| {
    left.update(|value| *value = 1);
    batch(|| {
        right.update(|value| *value = 7);
        left.update(|value| *value = 2);
    });
    assert_eq!(left.get(), 2);
    (left.get(), right.get())
});
assert_eq!(result, (2, 7));
```

普通 update 无须 batch 或手动 flush：在非通知/非批上下文中，返回前自动 drain。主动选择 batch 用于合并一个业务动作中的多次写入；batch 不等于 Scene 提交、GPU present 或事务。

**通知兼容性变化**：不再复制所有监听器闭包后递归调用。取消订阅会阻止尚未开始的回调；旧 dispatch 可以因新写入提前结束。依赖“递归写入时 B 仍收到旧值”或“drop 后仍执行已复制监听器”的应用需迁移，不应依赖这些旧行为。M1a 对应回归改为验证在途未开始的监听器被取消；旧 NodeId/pending 的代际防线仍单独保留。

## 通知与批合同

1. `Signal<T>` 仍为 `Rc<RefCell<_>>`，不可跨线程。每次 update 比较写入前后值；相同不入队，不同则递增私有版本。写入立即生效，get 返回模型当前值；**写入闭包持有可变借用，禁止在其中直接或间接读写同一 Signal**，这仍会触发借用 panic，不是重入通知方案。
2. 线程局部 FIFO 按 Signal 首次入队顺序派发。同 Signal 同时最多一个待派发任务；多次写入合并到 dispatch 开始时的最新值。任务只弱引用 Signal 和排队标志，不捕获强 Signal，不以调度器维持 owner 存活。
3. 每个 dispatch 取值、版本及注册 token 的有限快照；按 token 顺序、每次调用前重新检查版本及订阅存续。监听器中的新写入只入队，不递归调用监听器。版本变化后放弃旧 dispatch 剩余回调，新任务排在现有队尾。
4. 精确顺序：先注册 A，再挂 B；A 收到 1 写 2。轨迹为 `A(1) → A(2) → B(2)`，B 不收到旧 1；update 返回时 get=2，Scene flush 后绑定文字=2，后续 flush 不倒退。该策略不是跨 Signal 拓扑一致性：别的 Signal 既有排队任务可能先执行，不承诺派生菱形无中间值。
5. 批内 get 可见自己的写入，最外层 batch 正常退出后才 drain；返回闭包计算所得的返回值，不因批末监听器继续写入而重新计算，因此返回值可能早于 drain 后的模型值。通知内部嵌套 batch 仍由当前 drain 继续消费，不另起通知栈。多个 Signal 不会互相覆盖任务。
6. 同值往返政策：`0→1→0` 有真实写入，批末通知一次最终 0；不是按“批前/批后相等”撤销任务。全程 `0→0` 不通知。Scene 仍可格式化一次，但结果文字和可见性相同时不更改 buffer/revision，不布局、不请求呈现。
7. subscribe 不主动发初值。dispatch 开始前（包括批内）新增的订阅参与该次快照；通知过程中新增订阅只参与下一次 dispatch。取消阻止未开始的回调，不能中止正在运行的回调或自动撤回已经产生的其他副作用/pending。
8. Subscription 弱引用 owner；owner 先释放时取消为空操作。任务排队不保活 owner；当前 dispatch 临时保活 owner 直到本次处理完成。移除监听器后在释放 State 借用之外销毁其捕获，允许其析构继续释放同 Signal 的其他 Subscription。业务手写监听器强捕获源 Signal 仍需按时释放 Subscription；框架不替用户打破任意 Rc 环。

## 传播预算与失败

策略是**因果深度预算**，不是队列总长度/整批写入总数：根任务深度为 0，在任务回调期间排入的新任务深度为当前深度加 1。每条因果链最多运行 1024 个通知任务，下一任务触发明确 panic：

```text
Signal propagation budget exceeded (1024 causal notification steps); values retained, pending notifications discarded
```

这限制无限通知链，不误报 2048 个独立根 Signal 的正常批量更新；也可能拒绝超过预算的有限长链，**没有证明存在循环**。每个任务内的用户回调自身必须终止，不能阻断用户闭包自己的无限循环或外部阻塞。未实现时间切片、自动恢复或依赖图。

预算超限或监听器 panic 时 RAII drain guard 重置调度状态、释放所有待通知任务/排队标志；最外层 batch 闭包展开时亦丢弃待通知任务。模型写入保留，已经完成的通知/格式化/副作用不回滚；部分观察者可能未更新，不能把失败当作成功提交。正常 Application 不吞 panic，默认终止/展开由 Rust panic 策略决定。测试以 catch_unwind 捕获后，释放错误监听器，再做一次**有变化的新写入**验证调度器可恢复；相同值 update 不会重放已丢弃通知。写入闭包自身 panic 的部分修改亦不提供回滚。

## Scene、几何与呈现

私有 `Scene::flush(width, height)` 是共同提交边界：

```text
结构命令有限快照 → 按 NodeId/属性合并最终值 → 格式化一次
→ 实际值比较/记录脏原因 → 必要布局与 clip → 呈现意图
```

- 结构先于绑定；无效命令有原有收据错误，不算可见变化。同序 reorder/move 不产生变化。文字比较最终格式化结果，可见性比较最终 bool；同一节点多属性一次 revision。既有节点不因别的节点更新而重建或更改属性。
- Dirty 仅含本轮有业务支持的 `structure/text_measurement/visibility/viewport`。taffy 管理必要脏路径；触发布局时仍遍历放置 bounds/clip，不声称完整增量布局，更不声称 GPU 局部重绘。隐藏祖先下的文字/结构更新仍提交内容，但不要求立即布局或呈现；恢复时使用最新内容。
- 格式化闭包归节点，Signal 监听器只弱引用 pending。格式化只应转换值，不能承担业务写入；格式化中新排队结构命令留下一提交边界，不为副作用反复 drain Scene。示例格式化日志只作验证。
- `commit_version` 是 Scene 有效内容提交或逻辑 viewport 变化的序号，不是 Signal 版本/帧号；一次 flush 可以有内容和 viewport 两次推进。`geometry_version` 表示 bounds/clip 包含到哪个提交：有脏原因先布局，再追到 commit；隐藏内容等无几何影响则复用 bounds 并推进版本。
- 输入策略明确选择**先提交并完成必要布局，然后命中最新几何**，不保留第二套旧画面树。这个几何版本可以先于上次实际显示的版本；不声称输入永远对应上一次已 present 的旧画面。
- about_to_wait、redraw、尺寸/DPI 处理及鼠标按下/松开使用同一边界。正式鼠标入口先读取当前 window size/scale 转换 viewport 与物理指针，Scene 的 pointer_button 也先 flush 最近已知 viewport；不会只 sync 不 layout。尚无 viewport 时不命中。CursorLeft/失焦取消按压，不执行命中。
- 按压以完整 NodeId 保存。卸载目标/祖先隐藏后取消；仅隐藏无关节点不误取消当前按压。重排/文字换行后松开必须仍命中同一存续按钮才执行；尺寸/DPI 变化继续取消当前按压。
- 呈现意图保持到 Presented，redraw 请求去重；无变化提交或无状态变化的点击不主动请求帧。平台 expose/恢复仍可要求重绘。Retry 保留意图、至少等待 100ms，期间继续提交新值，重试显示最新状态；成功后回到 Wait。零尺寸/遮挡暂停绘制，恢复可重新请求。

**非目标**：M1c 外部消息/EventLoopProxy 唤醒、跨线程 Signal、完整 effect/derived/菱形图、事务回滚、滚轮/键盘/焦点/IME、主题、LLM 产品。本轮仅现有 UI 线程事件入口能驱动提交；不能声称 UI 线程外修改模型会自动唤醒休眠事件循环。

## 确定性验证

本轮最终 **51 unit + 1 doc**，四项命令均通过，CI job 名保持 `Rust checks`，没有修改工作流：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

- Signal：A1→2/B 不回退；普通/嵌套 batch、返回值/FIFO/多 Signal；同值往返；取消待回调/新增订阅；owner 与监听器释放；200 次有限重入且回调栈深度不叠加；双 Signal 无限链预算 panic/随后恢复；2048 独立 Signal 不误报；批/监听器 panic 清理；写入闭包借用边界。
- Scene：先挂 A 后挂绑定的最终文字=2；多属性/多 Signal/结构变更一次 flush 一次布局/最终格式化；无关节点属性/revision 保持；相同格式化输出/往返无可见提交；隐藏更新不无谓布局/呈现；viewport 与 no-op 结构。
- 输入：已有布局后，未 redraw 即换行、隐藏、插入、重排；旧位置不误触发、新位置正确；按下后换行/祖先隐藏/卸载再松开不误触发，无关隐藏不误取消。通过逻辑输入测试明确证明“未 redraw”顺序，不把真实输入日志冒充这个确定性条件。
- 应用：请求合并、Retry 中最新提交、连续临时故障至少 100ms 重试、成功 Wait；零尺寸/遮挡暂停重试而保留 Scene 呈现意图，恢复才请求帧；渲染 acquire 错误、DPI clip 原回归保留。没有在真实 surface 注入 Timeout/Lost。
- M1a：跨场景/代际/槽位复用、keyed 命令、卸载/订阅/捕获释放、1000 轮逻辑生命周期仍在本次测试中。迁移了取消语义断言，没有删除身份/过期 pending 防线。

## 真实图形证据与限制

所有脚本、PNG、完整日志及摘要仅放本 worktree 的 `.local/verification/`，不提交产物，保留到协调方审查清理。`gui.py` 只创建独立 Xvfb `:91`、启动正式 `target/debug/examples/batched_updates`、用 xdotool 发送真实鼠标/resize，再用 Pillow 捕获。cua-driver 仅做能力 preflight，未操作/停止已有共享 daemon 或桌面；未改变全局 provider/配置。

**最终构建**的软件 Vulkan（llvmpipe）有 1.0 和 1.5 DPI 两次独立会话：`software-release.log`、`software-dpi-release.log`；二进制 SHA 分别存 `software-release-binary.sha256`、`software-dpi-release-binary.sha256`，两者相同。动作逐条记录在同名前缀的 `*-actions.jsonl`。启动分别为物理 760×520、1140×780，缩窄分别为物理 360×950、540×1050（后者逻辑 360×700）。

两次会话均由真实点击得到：重入最终 2；一次 batch 的三次写入最终 3，`format batch=3` 仅一次；换行与缩窄后点击新位置目标=1；插入前缀后目标下移、再点击新位置目标=2。每次会话总计 8 个成功 present；版本与 layouts 从 1 到 8，formats 从 0 到 5（不含挂载初始格式化）。batch 动作单独增加 1 次 format、1 次内容提交、1 次 layout、1 个成功 present；不是把三个不同更新伪称一次。空闲 3 秒两次均从 8 帧保持 8 帧，此结论来自日志与脚本断言，不来自静态截图。

**逐张核对后采用的画面证据**：

| 文件（相对验证目录） | 实际支持的结论 |
| --- | --- |
| `software-release-inserted-target-two.png` | 1.0 DPI；同一画面可读 Reentry final: 2、Batch final: 3、Target clicks: 2、插入前缀及中文换行，顶部静态控制仍可见 |
| `software-dpi-release-inserted-target-two.png` | 1.5 DPI；同样的 2/3/2、中文换行与前缀，目标已下移且完整可读 |

这些图是在动作的目标模型/格式化日志及其后成功 present 已出现后捕获，并实际查看像素，非仅依据驱动发送成功。每张最终图对应各自第 8 个成功 present。计数合并来自完整日志，图仅证明所拍状态，不证明 GPU 完成时间或性能。

**验收误报更正**：初次报告把部分图片判断为缺字或黑帧。独立审查随后逐张查看关键 PNG、核对 `summary.json` 指纹，并对目录内 55 张 PNG 按对应窗口尺寸做像素统计：窗口矩形内部纯黑像素为 0，黑色区域属于窗口外的虚拟桌面；所有软件图的顶部区域与同 DPI 初始图逐像素一致。主协调再次查看上表两张最终图片，确认两者均有完整顶部标题及所列 2/3/2、中文换行与前缀。1.0 DPI 图 SHA-256 为 `876cf8431d325ebbaceaa2348dc5b04979cb9097cd9deca460b02166cfb30de3`，1.5 DPI 为 `1436c6ad3cb3230a8aa992118987b2bcae06fc4187f19e6446eb44bd078ee40a`，均与原摘要一致。因此撤回“这些保留文件缺字/全黑、最终 1.5 DPI 缺顶部标题”的结论，不将未提供对应坏帧的描述登记为已确认渲染故障，也没有为此增加持续重绘或修改渲染代码。探索文件仍保留到审查清理；以上结论覆盖实际检查的图片和已列操作，不把 present 日志解释为任意时刻像素完整或其他平台已通过。

各会话 `*-cleanup.json` 记录本轮应用/Xvfb 已停止；没有终止共享显示服务。`summary.json` 汇总最终两会话的二进制/截图指纹、帧与合并断言。`counter/state_panel/node_lifecycle` 除通过 examples 构建，还在独立 `:92` 软件 Vulkan 中分别完成 1 次成功 present 与 1 秒无新增帧的启动冒烟，见 `smoke-summary.json`、`smoke-*.log`；此冒烟只作运行/日志证据，不声称复验了各自全部交互或 M1a 完整图形压力。没有验证原生 Wayland、多屏动态 DPI、其他平台、真实 surface 故障注入或驱动 VRAM 字节回收。
