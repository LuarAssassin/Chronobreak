# M1a 节点生命周期验收

## 状态与入口

2026-11-13，Developer 在 `feat/node-lifecycle`、独立 worktree `.worktrees/node-lifecycle` 实现；基于 `main@b1919410990ca4b6145eb6620383b454546563af`。协调方已独立审查代码可合并并核实下述测试与压力日志；远端 `Rust checks` 状态仍由协调方确认。没有修改 main、合并或替用户选择许可证。M1b/M1c 未交付。

运行：`cargo run --example node_lifecycle --locked`。应用只持有 Element、Signal 和 Children，不持有 Scene/Renderer，不手动 sync/redraw。公共 API 最小用法：

```rust,no_run
use chronobreak::{Application, Element, Result};

fn main() -> Result<()> {
    let (list, children) = Element::keyed_column();
    let add = children.clone();
    let root = Element::column()
        .child(Element::text("新增 a").on_click(move || {
            add.insert("a", Element::text("中文动态节点"));
        }))
        .child(list);
    Application::new(root).run()
}
```

- `insert(key, element)` 追加新子树；key 仅在直接容器内唯一，重复插入拒绝，不隐式覆盖。
- `node(key)` 查询最近已提交的身份；`remove(id)` 真正卸载；`move_before(id, Some(other))` 或 `move_before(id, None)` 在同容器移动；`reorder(&ids)` 必须给出全部直接子节点的精确排列。
- `CommandReceipt::result()`：`None` 尚未消费，插入成功为 `Some(Ok(Some(new_id)))`，其他成功为 `Some(Ok(None))`，失败为 `Some(Err(TreeError))`。错误同时输出诊断；未挂载、场景已销毁、跨场景、失效代际、错误容器、重复 key、非法重排/自移动可区分。
- 创建 Application 时挂载根；之后、run 之前可以排队初始 children。插入尚未提交时，不能用 `node(key)` 同步取其身份，应等待后续 UI 事件或查看收据。
- 重排不重新构造 Element，局部 Signal、订阅、闭包与 NodeId 不变。删除再用同 key 是新身份；若业务需要跨卸载保留数据，显式放到应用模型中。隐藏保留节点、CPU buffer、状态与订阅。

## 实现与提交边界

`src/node.rs` 的自有 arena 用 scene owner + slot + generation 标识节点，free-list 复用槽位；代际耗尽退役该槽位，scene 身份耗尽明确终止而非环绕。没有新增依赖，Cargo.lock 不变。taffy NodeId 仅作为私有布局映射，不成为框架身份。父子、pending、pressed、显示列表和 renderer 映射全部使用完整框架 NodeId。

Children 仅弱引用命令队列；命令持有目标容器身份和负载。每次 UI 边界取出一个队列快照，先逐命令消费结构变化，再消费绑定更新；回调栈中不直接破坏树。失败命令不修改结构，命令之间不是事务，先成功的操作不会因后续失败回滚。只提供同容器排列，不提供跨父级 reparent，因而自环和祖先移入后代没有可执行路径，传入这些目标会明确拒绝。

卸载递归清理父子/key、arena/taffy、订阅、格式化与点击闭包、文字 buffer、pending、按压和失效目标命令负载；整个 Scene 销毁亦清空容器索引并完成未消费收据。已复制的 Signal 监听器可在取消订阅后继续入队，消费时检查完整身份，迟到更新不会落到复用槽位。没有用全面 catch_unwind 或防御性静默回退掩盖内部不变量。

Application 在 `about_to_wait`、redraw 或下一输入入口提交；消费变化后先布局再命中/请求呈现，输入后保守请求帧。UI 提交及输入路径主动清退已卸载 TextRenderer，不必等遮挡窗口恢复。此为 M1a 必要的局部一致性修复，不代表 M1b 带版本呈现快照、重入通知或完整脏域已完成。

Renderer 使用可删除 `BTreeMap<NodeId, TextRenderer>`，按本帧显示列表清退隐藏/完全裁剪的项，只为实际可绘制文字 prepare。显示列表保留背景/文字交错的画家顺序；过期文字引用跳过并诊断，不替换成新节点缓存。CPU buffer 仍供 Scene 测量与帧借用，不声称整个 IR 已自包含、可序列化或脱离 Scene。释放 CPU 引用后，真实 GPU 在途引用交给 wgpu，不手工复用仍被提交引用的资源。

## 自动化验收

Rust 1.99.1，以下精确命令全部退出 0；完整日志位于本 worktree `.local/verification/rust-checks.log`：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
```

**30 单元测试 + 1 文档编译测试，0 失败**。保留原 19+1 的用例；新增 2 个 arena 和 9 个生命周期测试。原显示列表测试迁移为完整 ID 并断言对应 first/last 文本，未削弱为只检查两个不同 ID。CI job 仍精确为 `Rust checks`，未修改 CI 文件；本地检查不替代远端 CI 与独立 review。

| 合同 | 确定性证据 |
| --- | --- |
| `[a,b,c]` 插入/删除/排列保留身份、局部值、订阅；同 key 重建归零 | `keyed_insert_delete_reorder_preserves_survivors_and_resets_rebuilt_key` |
| 重复 key、失效/跨场景/跨容器、自移动、祖先目标及非法排列拒绝 | `invalid_commands_are_diagnostic_and_do_not_mutate_either_scope` 及 stale/handle 用例 |
| 排队更新→卸载→实际槽位复用；旧 ID/旧消息/旧松开无效 | `queued_update_unmount_slot_reuse_and_old_release_cannot_hit_new_node`，容量断言为 3，不是只换了 ID |
| 真实已复制的 Signal 监听器在卸载后继续入队 | `already_copied_signal_listener_can_enqueue_after_unmount_but_is_ignored` |
| 后代订阅、格式化闭包和排队 Element 负载均释放 | `subtree_unmount_drops_descendant_subscriptions_formatters_and_queued_payloads`，弱引用归零 |
| 隐藏恢复保留身份/局部值，隐藏与完全裁剪不产生文字帧资源 | `hidden_and_clipped_texts_skip_frame_resources_but_hidden_identity_survives` |
| 回调仅入队；排队重排后输入不使用变化前几何 | `callback_structure_changes_wait_for_commit_and_queued_reorder_refreshes_hits`；不等于 M1b 全部快照合同 |
| 1,000 次挂卸可复用、有界逻辑资源 | `thousand_mount_unmount_cycles_bound_slots_and_logical_resources` |

逻辑压力测试使用真实字体测量、布局、Signal 和显示列表，但文字资源值是 `()`，仅复用真实 renderer 的映射清退算法，**不是 GPU 压力测试**。1000 次后：活节点/taffy = 1/1，订阅/pending/结构队列/CPU text buffer/逻辑文字缓存 = 0；arena 槽位保持 4，逻辑文字缓存峰值 2。日志为 `.local/verification/logical-stress.log`。

## 真实 surface 与资源证据

使用本轮独立 Xvfb `:93`（1800×1400），不触碰用户桌面。cua-driver 0.38.0 的窗口截图返回 `screenshot_frame_valid: true`，但这不证明截图对应最近输入完成后的 present；后台 XSendEvent 路由虽返回发送成功，但没有应用输入日志，未把它算作有效交互。之后改用明确指向隔离显示的 desktop target，以输入日志、资源/成功呈现记录与有效截图分别核对；窄屏 resize 用 xdotool 直接改变 X11 窗口尺寸。不是直接改 Signal，也不是无绘制命令队列循环。

1.0 DPI 硬件路径：NVIDIA GeForce RTX 2060，Vulkan，驱动 595.99.02，dev profile，Noto Sans CJK SC Regular。真实 surface 绘制了中文、矩形/文字交错与裁剪；Xvfb 不代表原生桌面/Wayland 支持。

| 操作 | 可支持的结论与边界 | 证据文件（`.local/verification/`）或逻辑测试 |
| --- | --- | --- |
| 反转并保留计数 | 1.5 DPI 软件截图明确为行 3/2/1、值 0/1/2；不作为硬件反转截图证明 | `dpi-reversed.png` |
| 窄屏计数与换行 | 1.5 DPI 软件截图明确为行 1/2/3、值 2/2/0，中文换行可见 | `dpi-narrow-counted.png`、`lifecycle-dpi.log` |
| 隐藏/恢复 | 日志支持隐藏时 prepare/cache 降低、恢复回基线；身份、局部值与文字资源合同由逻辑测试支持，不声称隐藏截图已验证 | `lifecycle.log`；`hidden_and_clipped_texts_skip_frame_resources_but_hidden_identity_survives` |
| 新增/删除 | 压力区间成功呈现帧支持活节点 20→22→20；局部计数保留由逻辑测试支持，不以逐步截图证明 | `lifecycle.log`；`keyed_insert_delete_reorder_preserves_survivors_and_resets_rebuilt_key` |
| 相同 key 重建、旧 ID 探针 | 同 key 新身份/归零由逻辑测试支持；压力后日志记录 `StaleNode` 且资源仍为 22，不能据此证明新行画面值为 0 | `lifecycle.log`；`keyed_insert_delete_reorder_preserves_survivors_and_resets_rebuilt_key` |
| 低高度裁剪、三行基线 | 硬件 360×240 截图裁剪正常；`stress-final.png` 可见行 1/2/3、值 2/1/0，不代表完整会话最后一帧 | `clipped.png`、`stress-final.png` |
| 空闲 3 秒 | 帧计数保持不变，无忙循环 | `actions.jsonl` |

截图限制：`reversed.png` 仍为行 1/2/3，`hidden.png` 仍显示三行，`same-key-reset.png` 未显示重建行，均不能支持文件名暗示的操作结果。`hardware-final-baseline.png` 仍显示行 4，不能证明三行最终基线。上述旧证据保留供协调方审查，不删除；未补拍，不声称每一步都有正确截图。

最终构建实际逐次绘制的 1,000 轮 insert/remove，**压力区间为 `lifecycle.log` 第 118–14115 行，共 4,000 个资源记录与成功呈现帧一一匹配**，用 trace 验证每轮先进入 22 个活节点的帧、再回到 20，共 2,000 次状态转换；不是同一提交里插入即删除。完整会话为 **4,038 个成功呈现帧**：压力前 32 帧、压力区间 4,000 帧、压力后 6 帧。末尾 6 帧包含额外重建/旧 ID 探针/删除，最后资源记录回到 20 节点基线；该结论来自日志，不来自 `hardware-final-baseline.png`。压力区间约 139.8 秒，包含驱动延时，不作为性能基准。摘要为 `gpu-stress-summary.json`，原始资源/呈现记录为 `lifecycle.log`；脚本 `gui.py` 在每批后断言基线、槽位和文字缓存。`stress-final.png` 仅支持所拍时刻三行画面。

| 资源 | 保留三行的基线 | 挂载一行时 | 卸载后 |
| --- | --- | --- | --- |
| 活节点 / taffy | 20 / 20 | 22 / 22 | 20 / 20 |
| 订阅 | 4 | 5 | 4 |
| pending / 结构队列（提交后） | 0 / 0 | 0 / 0 | 0 / 0 |
| CPU text buffer | 10 | 11 | 10 |
| 实际 TextRenderer map | 10 | 11 | 10 |
| arena 槽位 | 首次 20，热身后 22 | 22 | 始终 22 |

文字缓存高水位 11，不按历史挂载数增长。保留三行时 prepare=10/skipped=0/cache=10；隐藏三行时仍 live=20/subscriptions=4/buffers=10，但 prepare=7/skipped=3/cache=7；恢复回到 10。360×240 时 prepare=5/skipped=5/cache=5。计数是一次绘制尝试的 TextRenderer prepare 数量，本轮逐条核对其后成功呈现；`prepared_total` 故意累计，不是活资源数。

### 1.5 DPI 软件 Vulkan 复验

仅在验证子进程指定 `WINIT_X11_SCALE_FACTOR=1.5` 与 lvp ICD，不改变系统或项目配置。适配器为 llvmpipe（LLVM 21.1.8，Mesa 26.0.8-1ubuntu0.3，CPU / Vulkan）。初始物理 1140×780，窄屏物理 540×930（逻辑 360×620），裁剪物理 540×360。计数、新增/删除、重排、隐藏/恢复、同 key 重建和重建后旧 ID 拒绝均实际操作；窄屏点击行 2 后为 2，行 1 仍为 2，截图 `dpi-narrow-counted.png` 与日志吻合。

此最终软件路径另完成 **128 轮 / 512 帧 / 256 次挂卸状态转换**，压力区间为 `lifecycle-dpi.log` 第 136–1925 行，含 512 个资源记录及对应成功呈现帧；**完整会话为 550 帧**，其中压力前 38 帧。压力末尾日志回到与硬件路径相同的资源基线，槽位 22、TextRenderer map 峰值 11；摘要为 `dpi-stress-summary.json`，日志 `lifecycle-dpi.log`。两条路径空闲 3 秒均无新增帧。软件 Vulkan 与硬件 Vulkan 分别记录，不互相替代。

截图可能早于最后一次输入所请求的 redraw（日志已记录更新，截图仍为上一帧）；驱动增加等待和重跑并不足以证明所有截图均与对应 present 同步。本文只采用上表逐张目视确认的画面结论；其他操作虽有输入与资源/呈现日志，也不据此宣称已看到正确画面。发送成功、已排队更新、成功 present 与截图属于不同证据层次。此校验不声称实现了 M1b 的输入/呈现快照版本协议。

这些证据区分了槽位、CPU buffer 与实际 GPU renderer 对象数，但**没有测量驱动 VRAM 字节或证明提交完成时显存已全部返还系统**。glyph atlas、SwashCache、FontSystem 仍有共享缓存；此压力使用有限字形集，不推断无限新字体/字形的内存稳态，也不承诺性能延迟。资源在途生命周期依赖 wgpu 正确引用，而非自制复用池。

## 证据与未完成项

临时脚本、PNG、完整日志、环境与构建指纹仅放当前 worktree `.local/verification/`，由 gitignore 排除，保留给主协调审查，不提交图片或原文档案。入口：`rust-checks.log`、`logical-stress.log`、`actions.jsonl`、`gui-final-run.log`、`gpu-stress-summary.json`、`dpi-run.log`、`dpi-stress-summary.json`、`environment.json`、`example.sha256`、`cleanup.json`。首轮硬件/软件验证另留 initial 日志与摘要，最终构建重放分开记录。验证结束停止本轮应用、专用 cua-driver 与 Xvfb，不终止用户桌面服务。

未扩展 M1b 的 Signal 重入顺序/事务/完整脏域/带版本几何呈现快照，未扩展 M1c 外部唤醒/有界消息。没有焦点/键盘/IME、滚动、全树 diff、effect 系统、知识工作台或网关；没有模型/设置/许可证变更。100ms surface 重试与 Wait 沿用原测试；未在真实系统注入 device/surface lost。原生 Wayland、跨屏动态 DPI、其他平台与系统输入法均未验证。
