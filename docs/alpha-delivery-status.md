# Alpha 交付状态

2026-11-13；本页是**当前进展的唯一权威入口**，集中维护合入/候选状态、执行授权与交付门禁；[技术路线](technical-roadmap.md) 保留历史分析、依赖和验收合同，不维护另一份当前候选清单。本页不替代私有执行计划、决策文件或独立审查。

- **授权与当前范围**：历史上已落实 M3a 停止，随后恢复 M4B；`2026-11-14T08:47:27.432Z` 的 M4B-only 范围现已被用户 `2026-11-14T09:19:27.259Z` 的明确指令覆盖：**持续推进全部 33 个工作块直到完成**，问题记录后继续可独立工作。M4B 沿用保留实现，不重写；后续 M5A/M5B/M5C、M6A/M6B、M7 连续推进。实现仍在隔离分支交付，须通过独立审查、最终候选门禁、远端 CI 与受保护主线合并；外部受阻或未验证项目不得计为完成。
- **已合入基线**：M1a/M1b/M1c（PR #6/#7/#8）、M2a（PR #9）、M2b 初始纵向视口（PR #10）、M3A 逻辑编辑模型（PR #11）、M3B 单行 TextInput / FB-2（PR #12）、M3C 原生 IME / reproducibility FB-3（PR #13）及 **M4A 有限视觉基础（PR #14）**；基线 `a92390b38d006fa82caa9b2aff37ee65a72939f0`，205 unit + 6 doc、12 examples。合同见 [文字模型](text-edit-model.md)、[单行输入](text-input.md)、[原生 IME](native-ime.md)、[视觉原语](visual-primitives.md)。M3C 正式资格仅限下述 Fcitx5 tuple，不代表通用 IME 或跨平台资格。
- **M3C 资格范围**：**Ubuntu 26.04.1 amd64 + winit 0.30.13/XIM + Fcitx5 5.1.19-1 + Pinyin 5.1.12-1/libime 1.1.14-1 + classic UI**；tracked-only clean archive 的 `c1`/`c2`、真实 XTEST 1.0× / 1.5× 各 **14/14**，含系统候选在嵌套滚动、同窗移动与重排后无额外击键跟随，外部 clipboard reader 各 2/2。**IBus 1.5.34~rc2-1/libpinyin 仍各 11/14，三项候选跟随失败未修复**；Wayland 实验性、未获本轮资格。本次视觉块保留此运行时，不重复宣称新引擎资格。
- **后续已合入**：M4B 语义主题与有限控件（PR #15）、M5A 版本化知识文档与原子本地存储（PR #16）；当前主线 `98189c751a50e60d4af4aae5e0587d672c752db1`，230 library tests + 5 schema example tests + 6 doctests。合同见 [主题与控件](themes-controls.md) 与 [知识文档](knowledge-document.md)；不代表 OS 无障碍桥接或网络资格。
- **当前候选**：`feat/offline-workbench` 的 **M5B 离线知识工作台**；主动粘贴、原文预览、自动先序树图/连接线、选择/折叠/滚动、主题、异步保存/重启恢复见 [离线工作台](offline-workbench.md)。真实隔离 X11 流程 17/17 通过，仍须独立审查及受保护 PR/CI/主线复验；不将本地实现称为已合入。
- **后续未完成**：M5B 最终审查/合并、M5C 真实单次网络闭环；M6 性能/资源/AccessKit 系统适配与无障碍/平台资格；M7 打包/许可落地及全部门禁后的发布，以及其他 engine/backend 的额外资格。未实施、受阻或未验证的块仍未完成。多行编辑器、通用横向滚动、完整视觉 bidi 不在本轮小核心交付内。
- **已确认决策**：用户委托后由协调者选择 **MIT OR Apache-2.0**、**Linux X11 优先资格 / Wayland 实验性**；平台资格仍需对应实测，实际许可文件留 M7 打包块落地，尚不宣称已实施。首接服务仅 **DeepSeek Flash**（路线已登记官方 `https://api.deepseek.com`、OpenAI chat 格式与 `deepseek-flash`）；这是接入方向，不是可用性验证。以上委托及确认由本次 FB-1 任务回填，主题沿用既定方向，无新增产品范围。
- **凭据与验证边界**：原始 key 仅留私有 `.env`，空值由 owner 填写；尚无真实网络调用证明。本次 M4B 不读取 `.env`/密钥，不连接模型服务、不改 provider/全局设置或主机输入法；不将服务决策当作 key/服务已验证，不将许可证选择当作许可文件已落地。原文再发布与用户桌面设置不在本次授权内。
- **发布条件**：用户已授权仅在**全部强制门禁通过后发布 GitHub Alpha prerelease**，不是提前放行、正式版或 crates.io 发布授权；本次 FB-1 任务回填该条件，不宣称任何发布已经执行。

## M4B 验证入口

本切片验证合同与实测证据只在 [主题与控件](themes-controls.md) 维护，原始产物在 `.worktrees/themes-controls/.local/verification/`。保留 M4A 参考色板及所有旧像素阈值；OS 系统适配/资格留 M6B。实现者不调用计划 CLI、不推送/合并或修改 main；后续任务范围以最新用户指令为准。

## M4A 历史验证（PR #14）

2026-11-13，隔离工作树 `.worktrees/visual-primitives`；实现/脚本/公开示例提交 `e702bd3bcf99bd05567be8504b8faf425ce01a5f` 的 64 tracked 文件 clean archive 验证通过。精确 fmt、strict all-targets clippy、locked workspace test 与 examples build 全通过：**205 unit + 6 doc、12 examples**，保留原 192 + 6。

真实 X11 surface 三 DPI（1.0 / 1.5 / 2.0）**546/546**：交错文字/透明图形及 clip 正确，合批逐字节零差异；实际 primitive draws 连续场景 **33→1**、交错场景 **42→8**。40 次有限资源循环、原有 TextInput/输入路由/滚动/后台唤醒与 idle smoke 通过，自有进程已停止。证据、预登记容差、颜色迁移、资源与平台边界只在 [视觉原语](visual-primitives.md) 维护；本块不新增真实 IME 资格或主题/控件完成声明。

该记录是实现者当时的本地交付，不重复维护当前状态；后续已由父协调者完成 PR #14 集成。原切片未读密钥、连接模型服务或操作主机输入法。当前进展以上方唯一状态入口为准。

## M3C 合入资格记录（PR #13）

2026-11-13，隔离工作树 `.worktrees/native-ime`，main 未改动；全部产物在该工作树 `.local/verification/`。没有调用计划 CLI、委派、推送、鉴权或合并，也没有切换/停止 owner 桌面 IBus。

| 门禁 | 结果 | 证据入口 |
| --- | --- | --- |
| fmt / strict workspace all-targets clippy / locked workspace test / locked examples build | FB-3 clean archive 四项通过；192 unit + 6 doc，11 examples | `fb3/gates.json`、`fb3/gate-*.log`；历史 `gates.json`、`gate-*.log` 保留 |
| tracked reader + 正式 native example 构建 / Python prerequisite | 提交 `ba9cd36` 的 58 tracked 文件归档；两个 locked build 通过；缺失/不可执行 reader 在普通/`-O` 均 exit 2 | `fb3/archive.json`、`fb3/build-{reader,native}.log`、`fb3/python-checks.json`、`fb3/reader-race.json` |
| IBus X11 libpinyin：preedit、单次中文 commit/undo/redo、Esc、Latin 混合、双字段/卸载隔离、实际滚动与初始系统候选锚点 | 两 DPI 各 11/14；不是完整资格通过 | `x8/`（1.0×）、`x9/`（1.5×）的 `checks.json`、PNG/geometry/model/event/operation trace |
| IBus 系统候选在嵌套滚动、同窗 move、resize/reflow 后即时跟随 | 两 DPI 各三项失败；独立 async XIM 替代也未解决 | `x6/cursor-protocol.log`、`x8/13-*`、`x8/15-*`、`x8/16-*`、`x4/` |
| 独立外部进程读取真实 GUI copy 文本 | 两 DPI 2/2 通过，不注入模型 | `x8/clipboard-reader.json`、`x9/clipboard-reader.json` |
| 独立真实 Fcitx5 XIM/Pinyin/classic UI；上述全部行为与可见候选跟随 | clean tracked harness 的 `c1`/`c2` 各 **14/14**、entrypoint exit 0；正式资格仅此 tuple | `c1/`、`c2/` 的 `checks.json`、PNG、events/Signal/geometry；历史 `f4`—`f9` 保留 |
| Fcitx5 独立 clipboard reader / 进程与库来源 | 两 DPI 2/2；新 reader/app binary 与归档 source/probe hash 均匹配；5357 既有文件 hash 不变，自有进程/socket 已清理 | `c1/clipboard-reader.json`、`c2/clipboard-reader.json`、两 run 的 `provenance.json`/`source-hashes.json`；汇总 `fb3/qualification.json`，历史官方 `native-stack/{package,source}-manifest.json` 保留 |

复验程序、具体差值、系统版本、源/二进制/操作哈希与 IBus 余留限制见 [原生 IME](native-ime.md)。X11 的上述 Fcitx5 tuple 通过全部原生门禁，不等于修复 IBus；原生 Wayland 实验性且未实测，跨平台及 engine-agnostic winit 架构不变，不声称 Windows/macOS 资格或整个 Alpha 完成。

## M3B 历史验证

2026-11-13，M3B 隔离工作树 `.worktrees/text-input`；构建、临时目录和全部产物置于本工作树 `.local/verification/`。仅下载所需文字剪贴板依赖，不连接业务/模型服务：

| 命令 | 结果 | 原始日志 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | FB-2 通过 | `blink-clip/fmt.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | FB-2 通过，无警告 | `blink-clip/clippy.log` |
| `cargo test --workspace --locked` | 176 unit + 6 doc 全通过；保留原 150 + 5，新增 26 + 1 | `blink-clip/test.log` |
| `cargo build --examples --locked` | FB-2 全部示例通过 | `blink-clip/build-examples.log` |
| `python3 .local/verification/gui_verify.py` | 历史 `3d0f6af`：真示例双 DPI、系统键鼠/外部剪贴板、遮挡/失焦及 40 图断言通过；不代表 FB-2 新二进制 | 原 `gui-command.log`、`gui-summary.json`、`gui-{1_0,1_5}.log`，均保留 |
| `python3 .local/verification/blink-clip/gui_verify.py` | 历史 Cua 尝试：两 DPI 初始渲染，resize/foreground 方法限制；原证据保留，不作为外部阻塞 | `blink-clip/gui-command.log`、`blink-clip/gui-summary.json`、`blink-clip/cua-calls.jsonl` |
| `python3 .local/verification/blink-clip/xtest-attempt/verify.py` | FB-2 直接 XTEST：1.0× / 1.5× 同窗宽 8、高 4 逻辑像素均停帧 1.35s；焦点不丢，恢复 blink 且 layout/format 不变 | `blink-clip/xtest-attempt/{summary,provenance}.json`、`native-calls.jsonl`、`app-*.log`、16 PNG |

首次 Clippy 的可折叠 if 已修正，早期日志保留；测试构造错误和继承 Wayland 变量导致的 GUI 首次启动失败均已定位修正，并非跳过。真实验证是专用 Xvfb / NVIDIA Vulkan，两个 DPI 各自查询窗口几何并裁图；外部 writer 持有 X11 owner，独立 reader 逐字核对字段 copy/cut/Unicode paste。闪烁无文字重排，原生 X11 全遮挡/失焦停止帧；详细合同、逐项证据及限制见 [单行输入](text-input.md)。该历史验收未覆盖原生 IME、Wayland、Windows/macOS 与真实 GPU 故障；后续 IME 证据见本页 M3C 记录。其当时未推送、鉴权、合并或修改 main；后续集成状态以本页顶部为准。

FB-2 两项新回归已先复现失败，再验证修正：正外框/零宽内容及 scroll ancestor 仅剩 1–4px padding，均停止 deadline/持续 present，恢复后 blink 不增加 layout/format。补充真实 XTEST 根字段 probe 确认窗口 8×520 / 12×780、760×4 / 1140×6：两 DPI 停帧分别 11→11、19→19，恢复分别 14→17、22→25，layout/format 保持 5/2、7/2。原证据与 `blink-clip/source-manifest.json` 不覆盖，后续成功及启动焦点断言误报另见 `xtest-attempt/provenance.json`；全部自有进程已停止，无运行时代码变化。这里只交付 scoped P2 修正，不宣布整个连续任务完成，不操作父协调者计划 CLI。
