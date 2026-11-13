# 文档与参考资料

- [技术路线与交付里程碑](technical-roadmap.md)：框架全局架构、分阶段验收与已确认方向/待确认合同。
- [知识整理工作台核心](knowledge-workbench.md)：树形思维导图小闭环、分切验收与框架/应用边界。
- [明暗主题方向](theme-direction.md)：冰青/炽红配色、语义 token 与可读性基线。
- [初始架构方向](architecture.md)：第一版技术取舍与可运行目标。
- [最小 GUI 验证](minimal-gui.md)：运行方式、交互复验与图形证据。
- [可复用核心验证](reusable-core.md)：库入口、泛型绑定、显示开关与双计数器验收。
- [M1a 节点生命周期](node-lifecycle.md)：代际身份、keyed 命令、资源回收与真实图形验收。
- [M1b 批处理与提交](batched-updates.md)：重入通知、批边界、传播诊断及输入几何验收。
- [M1c 后台消息](background-messages.md)：有界背压、UI 作用域、EventLoopProxy 唤醒与验收。
- [协作与分支](../CONTRIBUTING.md)：分支、agent 隔离、验证与合并约定。

## 外部参考

### vibe 一个你自己的 GUI 框架

AKR（[@object_nullll](https://x.com/object_nullll)）撰写，发布于 2026-11-12。介绍从 DSL、UI 树与增量更新，到布局、文字排版、GPU 渲染和窗口事件的自绘 GUI 框架实现思路，主要使用 Rust 生态举例。

- [原始长文](https://x.com/i/article/2120793311216836608)
- [原始推文](https://x.com/object_nullll/status/2120801603284500797)

本地完整快照位于 `docs/references/vibe-your-own-gui-framework/`，含 `article.md`、`source.json`、`manifest.json` 和 `assets/`。正文按原顺序保留 267 个内容块，含 13 个代码／表格块、12 个原文超链接；封面、两张正文配图、计数器动图 MP4 及预览图共 5 个附件已保存，覆盖计数与 SHA-256 已验证。

归档日期为 2026-11-12（UTC-08:00）。该完整快照由 `.gitignore` 排除，**不包含在公开仓库或 Git 历史中**；克隆公开仓库不会获得全文或附件。完整性以当次取得的结构化文章为基准，在线内容后续可能更新。

版权归原作者所有，未见明确转载许可，保留本地学习资料而不公开再分发。原文技术建议是参考，项目方向另见架构文档。
