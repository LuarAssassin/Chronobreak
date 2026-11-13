# Alpha 交付状态

2026-11-13；本页是**当前进展的唯一权威入口**，集中维护合入/候选状态、执行授权与交付门禁；[技术路线](technical-roadmap.md) 保留历史分析、依赖和验收合同，不维护另一份当前候选清单。本页不替代私有执行计划、决策文件或独立审查。

- **连续授权**：用户于 `2026-11-13T13:40:27.621Z` 明确授权持续推进剩余 M3—M7；M3A 是其中一个实现块，不是停止点。普通实现无需逐步等待“继续”，仍须通过受保护 PR、独立审查与强制门禁；方向变化、未决业务合同和访问条件仍需确认。协调者负责后续编排、审查、合并及远端发布；本次 FB-1 文档修正不修改执行计划、不推送或合并。
- **已合入基线**：M1a/M1b/M1c（PR #6/#7/#8）、M2a（PR #9）、M2b 初始纵向视口（PR #10）及 **M3A 逻辑编辑模型（PR #11）**；当前基线 `245f8eacca6b08f5838e1370e17aba274c993b75`，150 unit + 5 doc。模型合同见 [文字模型](text-edit-model.md)，不代表整个 M3 或全平台资格已完成。
- **当前候选**：`feat/text-input` 的 **M3B:B1 单行 TextInput**，受控 Signal、整形选区/caret、私有水平揭示、readonly/disabled、异步系统剪贴板与聚焦 deadline，见 [单行输入](text-input.md)。FB-2 修正有效内容 clip 的 blink eligibility，本地四项门禁与独立复审已通过；双 DPI 完整 GUI/剪贴板结果属历史 `3d0f6af`，补充新二进制直接 XTEST 的 clip/restore 已在两 DPI 成功验证，见下表与末段。最初 Cua/无 WM 方法限制保留为历史失败，不再作为当前缺口。最终 CI/集成状态以对应 PR 与执行计划为准，不将本地提交称为整个 Alpha 完成。
- **后续未完成**：M3B 审查/合并及 M3C 原生 IME/preedit/commit/候选定位；M4 视觉/主题/最小控件；M5 应用闭环；M6 性能/资源/无障碍/平台资格；M7 打包/许可落地及全部门禁后的发布。多行编辑器、通用横向滚动、完整视觉 bidi 不在本单行块交付内。
- **已确认决策**：用户委托后由协调者选择 **MIT OR Apache-2.0**、**Linux X11 优先资格 / Wayland 实验性**；平台资格仍需对应实测，实际许可文件留 M7 打包块落地，尚不宣称已实施。首接服务仅 **DeepSeek Flash**（路线已登记官方 `https://api.deepseek.com`、OpenAI chat 格式与 `deepseek-flash`）；这是接入方向，不是可用性验证。以上委托及确认由本次 FB-1 任务回填，主题沿用既定方向，无新增产品范围。
- **凭据与验证边界**：原始 key 仅留私有 `.env`，空值由 owner 填写；尚无真实网络调用证明。本次 M3B 不读取 `.env`/密钥，不连接模型服务、不改 provider/全局设置；不将服务决策当作 key/服务已验证，不将许可证选择当作许可文件已落地。原文再发布与用户桌面设置不在本次授权内。
- **发布条件**：用户已授权仅在**全部强制门禁通过后发布 GitHub Alpha prerelease**，不是提前放行、正式版或 crates.io 发布授权；本次 FB-1 任务回填该条件，不宣称任何发布已经执行。

## 本块验证

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

首次 Clippy 的可折叠 if 已修正，早期日志保留；测试构造错误和继承 Wayland 变量导致的 GUI 首次启动失败均已定位修正，并非跳过。真实验证是专用 Xvfb / NVIDIA Vulkan，两个 DPI 各自查询窗口几何并裁图；外部 writer 持有 X11 owner，独立 reader 逐字核对字段 copy/cut/Unicode paste。闪烁无文字重排，原生 X11 全遮挡/失焦停止帧；详细合同、逐项证据及限制见 [单行输入](text-input.md)。原生 IME、Wayland、Windows/macOS 与真实 GPU 故障仍未验证。未推送、鉴权、合并或修改 main；父协调者继续审查与后续块。

FB-2 两项新回归已先复现失败，再验证修正：正外框/零宽内容及 scroll ancestor 仅剩 1–4px padding，均停止 deadline/持续 present，恢复后 blink 不增加 layout/format。补充真实 XTEST 根字段 probe 确认窗口 8×520 / 12×780、760×4 / 1140×6：两 DPI 停帧分别 11→11、19→19，恢复分别 14→17、22→25，layout/format 保持 5/2、7/2。原证据与 `blink-clip/source-manifest.json` 不覆盖，后续成功及启动焦点断言误报另见 `xtest-attempt/provenance.json`；全部自有进程已停止，无运行时代码变化。这里只交付 scoped P2 修正，不宣布整个连续任务完成，不操作父协调者计划 CLI。
