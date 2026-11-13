# M1c：有界后台消息与休眠唤醒

2026-11-13（America/Los_Angeles）。`feat/background-messages` 基于 `main@a15b09b7a6998b40b0b788f42ebe9a2963d93a97`，仅在独立 worktree 实现。M1a/M1b 已分别合入 PR #6/#7；本切片是 **M1c 候选，待独立审查及远端 Rust checks**，不是整个 M1 或 M2+ 完成。无新依赖、任务运行时、网络、LLM 或跨线程 Signal。

## API 与迁移

```rust,no_run
use chronobreak::{Application, Element, Signal, message_channel};
use std::num::NonZeroUsize;

let progress = Signal::new(0);
let model = progress.clone();
let (sender, receiver) = message_channel(NonZeroUsize::new(8).unwrap());
let cancel = receiver.cancellation();
let root = Element::column()
    .child(Element::text("取消").on_click(move || cancel.close()))
    .child(Element::bound_text(&progress, |value| format!("进度：{value}"))
        .on_message(receiver, move |value| model.update(|current| *current = value)));
std::thread::spawn(move || {
    if let Err(error) = sender.try_send(42) {
        eprintln!("消息未入队：{error:?}");
    }
});
Application::new(root).run().unwrap();
```

| 入口 | 合同 |
| --- | --- |
| `message_channel::<T>(NonZeroUsize)` | 显式正容量，每个接收器一个队列，`T: Send + 'static`；不要求 Clone |
| `MessageSender::clone/try_send/is_closed` | sender 可 Send + Sync；拒绝结果为 `TrySendError::Full(T)` / `Closed(T)`，负载归还调用者 |
| `Element::on_message(receiver, FnMut(T))` | 接收器消费进节点作用域；闭包可捕获 Rc/Signal，不要求 Send，不接触 Scene/Renderer |
| `MessageReceiver::close/cancellation` | 未绑定接收器也可关闭；取消句柄仅限 UI 线程、弱引用目标 |
| `MessageCancellation::close` | 幂等，立即关闭并丢弃排队数据、释放处理闭包；已执行中的闭包到返回才释放 |

现有 Application、Element、Children、Signal、batch API 不变；不要求应用手写事件循环、sync 或 request_redraw。旧的 UI 内更新保持原语义；后台代码改为只发送值，由 UI 处理闭包写 Signal。`background_messages` 的 `request` 编号由应用分配；闭包比较 `progress.request != active.get()` 并忽略旧请求。框架不替应用定义“最新请求”、取消 I/O 或业务重试。

## 所有权、背压与关闭

- Signal、Rc、Scene、接收器和处理闭包留在 UI 线程；发送者只持 `Arc<Queue<T>>`，队列只弱引用 wake，wake 注册表也只弱引用队列，不持节点或 UI 闭包。NodeId 校验包含场景、槽位及代际；卸载后同 key/同槽位的新节点不复活旧路由。移除路由同时清理 wake 注册，不因保留旧 sender 积累历史路由。
- 队列元素数始终不超过容量；不是在一个无界通道外检查长度。`try_send` 不等待空位、不阻塞等待 UI；使用短互斥临界区，不声称无锁、实时或绝对不等待锁。UI 只取现有值，不 recv 阻塞、不 join。每 inbox 按成功入队顺序 FIFO；并发生产者按队列互斥的先后线性化，不承诺跨 inbox 全局 FIFO。
- 普通消息不自动丢弃或覆盖；Full/Closed 归还原值，调用方决定保留重试、报告错误或主动舍弃遥测。**Ok 仅表示当时接受入队，不是最终处理承诺**：取消、已提交卸载、Scene/Application drop、退出或 proxy 关闭都可丢弃已接受消息。与退出并发的发送可能返回 Ok 后立即被关闭清空；之后发送返回 Closed。
- `Children::remove` 是排队命令，不是立即取消。消费轮开始先提交已有命令；消息闭包内排队的删除到该批末才提交，因此本批最多 64 条中的后续闭包仍可能运行。要阻止后续尚未开始的消息，调用取消句柄；不能中断已经开始的处理闭包。隐藏保留路由与状态，不是卸载。
- 显式取消/接收器 drop/提交卸载释放队列和 UI 捕获。Application 的 `exiting` 及 Scene drop 关闭全部路由，也释放尚未提交插入中的接收器；发送者存活不延长 UI 生命周期。proxy 发送失败在发送线程关闭已注册队列、释放 Send 负载，不尝试跨线程析构 UI 捕获；UI 捕获随接下来的 UI 关闭/Scene drop 释放。
- 关闭以 sender 的 Closed/is_closed 向示例线程合作通知。示例在延迟期间检查关闭、在 Full 后保留负载有限间隔重试、在 Closed 时退出；Application 不等待线程 join。任意用户阻塞 I/O、线程或死循环不能被框架强制取消，不提供完整取消框架或退出期限管理器。

## 唤醒与每轮提交

`EventLoop<UserEvent>` 的 proxy 仅投递无负载 Messages 通知；业务数据仍在有界队列。Scene 共享一个 scheduled 位，合并所有 inbox 的唤醒，避免每条数据都进入 winit 的无界用户事件通道。

1. 生产者持队列锁检查 Closed/Full、追加数据并读取弱 wake，然后释放锁；若 wake 未 scheduled 则置位，并在释放 wake 锁后调用 proxy。UI 处理闭包、负载析构及唤醒闭包均不在队列/wake 锁内运行。
2. 未挂载时先保留队列；挂载绑定 wake 并检查非空。run 前尚未安装 proxy 时保留 scheduled，安装时重新发出通知。因此在建树前、挂载后或 run 前入队都不丢首批。
3. `user_event` 只标记就绪；在本轮 `about_to_wait` 确认 scheduled，并先提交已有结构命令。随后按 inbox 轮转、**全场景合计至多 64 条**，在一个 M1b batch 内调用 UI 处理闭包，批末提交结构/绑定最终值，再沿原路径必要布局、收集卸载资源与请求呈现。
4. 确认前的发送已有事件覆盖；确认后的发送重新投递后继事件。消费后若还有数据也执行同一幂等 notify，所以生产者在最后 pop/空检查两侧发送均不会被“清位”吞掉。允许已有后继事件最终发现空队列，但不会无条件自唤醒。
5. 重新投递只在有限消费结束后执行，下一轮仍处理原生输入/窗口/关闭事件，不在 user_event 内无限重发消费。64 是保守策略，不是性能保证：任意用户闭包、一次必要布局或 GPU 工作仍可能慢，不提供通用执行器的抢占或公平耗时承诺。

保留 M1b 的当前已提交几何命中：消息尚未消费不属于已提交模型；一旦 UI 应用，则鼠标入口与呈现仍先完成必要布局，不能用新属性搭旧 bounds。Signal 仍有版本通知与 1024 因果传播预算；批内更新立即可读，通知/绑定只提交最终值。

没有消息/输入/重试时是 `ControlFlow::Wait`，不设周期消息定时器，不持续 redraw。surface Retry 仍等 100ms，保留最新呈现意图；成功清除重试。遮挡、零尺寸、suspended 暂停 GPU 呈现与重试，不清掉 Scene 的呈现意图；事件循环仍调度时消息继续有限更新模型，无窗口则延后布局/呈现。平台真正暂停分发时数据只进入有界队列，容量满仍返回 Full；恢复后消费、呈现最新值，不因后台完成而忙重绘。

## 逻辑门禁

精确命令全部通过，日志位于本 worktree 的 `.local/verification/gate-{fmt,clippy,test,build}.log`：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

**66 unit + 4 doc**（2 个成功编译示例、2 个预期编译失败），保留全部原 51 unit + 1 doc。新增 15 个逻辑测试包括：

- 真实 worker 和 UI 消费线程身份不同；UI 闭包捕获 Rc；sender Send + Sync 正向检查，receiver 和 Signal 跨线程的编译失败检查。
- 非 Clone 负载/FIFO、正容量、Full/Closed 原值返回；建树/安装 hook 前入队、合并通知；mpsc/Barrier 明确控制确认前后、最后 pop 两侧与 wake hook 在途交错，不用 sleep 碰运气。
- 全场景有限 64/64/22 消费、轮转公平、批末一次通知、轮间输入仍工作；空队列不 rearm。
- 锁外处理闭包与负载析构；批内取消及预绑定取消；提交卸载的负载/捕获析构、同槽位新代际隔离、保留旧 sender 不积累路由。
- app drop/显式关闭释放捕获和负载，关闭未提交插入；失败 wake 模拟关闭事件循环后所有队列清空、后续 Closed 且不循环 rearm。真实 event loop 退出另以 GUI 末尾 late send 验证，不把模拟失败称为系统故障注入。

CI 保持原触发器与 `Rust checks` job 名称；此处只记录本地门禁，不宣称远端 CI 已通过。

## 真实 GUI 验证

使用隔离 Xvfb + xdotool/Pillow、现有 Noto CJK、Mesa llvmpipe Vulkan，无安装/设置变更。脚本、日志、JSON 和 PNG 全在本 worktree 忽略的 `.local/verification/`；只停止自己启动的应用和 Xvfb。截屏读取 `xdotool getwindowgeometry --shell` 的实际窗口矩形并裁剪，不把窗口外黑桌面当黑帧；实际查看关键 PNG，区分模型/布局/present 日志与像素证据。

最终二进制 SHA-256：`91a941e5ec59b37e6a4f7fe1ca111e456ffc35d56d0a3804c38ec02c9901dfab`。`final-1.0-*` 与 `dpi-1.5-*` 为此二进制；先期 `dpi-1.0-*` 为首个核心提交 `ea23333` 的验证，二进制 SHA-256 `201f59b352dd587ce6b000167203c9d02ab130345c25351c705e6e6a100a1a67`，保留作迭代证据，不混称最终重跑。

- **无注入输入唤醒**：最终 1.0 DPI 为 760×520，1.5 DPI 为 1140×780；启动各 1 帧后 0.7 秒不增帧，约第 3 秒后台首条消息经 user_event→UI callback→提交/必要布局→present。期间无注入鼠标或 resize；首帧前 X11 自身的 Focused/初始 CursorMoved 不误归为工作线程唤醒。11 条进度使总帧数到 12，完成后 1 秒无新增帧；日志末为 Wait，图片可读中文进度/完成状态。
- **突发与输入**：先期 1.0 的 20,001 条、容量 128，Full 343 次保留重试，完整交付；每次 dispatch 不超过 64，最后 33 条。过程中计数点击变为 1、展开中文段落、改为 900×650，图中进度 3135/20000。最终 1.5 同样完整处理 20,001 条、Full 416 次，1350×1000 图中进度 3199/20000 且计数为 1；输入、换行和呈现均在后台完成前工作。突发结束总帧 325，1 秒后仍为 325；这是实测帧计数，不是目标吞吐保证。
- **卸载/晚到/替换**：先期 1.0 在下一次突发中移除，UI 清掉目标，worker 在第 576 条返回 Closed（Full 8 次）；最终 1.5 同样第 576 条返回 Closed（Full 9 次）。两者恢复同 key 目标仍是初始等待内容，旧 request=3 不更新新目标。新请求延迟期间取消，worker 合作退出，图显示“已取消”；不是强制取消 I/O。最终 1.5 再缩到 720×1140，中文段落多行换行、顶部标题、计数与取消状态仍可读；窄屏无滚动，超小窗口按既有裁剪合同处理。
- **关闭**：最终 1.0 在新 worker 三秒延迟内发送 WM_DELETE_WINDOW，退出码 0，run 返回后 probe `closed=true late_closed=true`。不 join，不用 kill 代替成功关闭；脚本 finally 仅回收自身显示进程。

关键已查看图片（均为窗口裁剪，非整桌面）：

| `.local/verification/` 下的文件 | SHA-256 | 内容 |
| --- | --- | --- |
| `final-1.0-complete.png` | `da0af9cad16a7e05df15d85d529a0a8557f61deeda2e7b29a9c3f8b25b727dc6` | 760×520，标题、中文与请求 1 完成 10/10 |
| `dpi-1.5-burst-responsive.png` | `058e93ee6a4ab8eb57e983fbf79f4bc8ca991efe781fc53d5558d42898a204ab` | 1350×1000，突发 3199/20000、计数 1、已展开段落 |
| `dpi-1.5-burst-complete.png` | `d8144f4a5a94a3ae0c6bf062db65930bdaaab18a6ba6bfdcbb99fc6422d8871f` | 同尺寸，20,000/20,000 完成状态 |
| `dpi-1.5-narrow.png` | `e857436dcd6e0483a5db40a82a134b762f75c314fd6e62c4b09594f581deaa71` | 720×1140，中文换行、计数 1 与取消状态 |

各 `*-records.json` 给出准确动作时间、窗口几何、帧数、PNG SHA-256，`*-app.log` 保存适配器、输入、回调、dispatch/布局/成功 present 与关闭原始日志；`summary.json` 自动核对裁剪尺寸/指纹、每轮预算、突发完整交付、输入交错及退出。所有三个会话由 WM_DELETE_WINDOW 正常退出，脚本 finally 回收自己启动的 Xvfb，三个已知 app PID 均已不存在。图片仅证明捕获时状态，不能证明任意时刻无坏帧或 GPU 完成时间；未复验 M1a 1000 帧压力，不把历史“缺标题”视觉误判当渲染缺陷。未验证原生 Wayland、动态跨屏 DPI、其他平台、真实 surface/device lost 注入或任意慢用户处理闭包；原有 100ms Retry/零尺寸/遮挡调度由回归覆盖。
