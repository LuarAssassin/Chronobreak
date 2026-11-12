# 协作与分支

采用轻量 GitHub Flow，不维护长期 `develop` 或笼统的“测试分支”。

## 分工

- `main`：已完成审查与验证的集成基线；早期可以是文档基线，不等同于已经发布的软件。
- `feat/<主题>`：独立功能；当前首个功能分支为 `feat/minimal-gui`。
- `fix/<主题>`、`docs/<主题>`、`chore/<主题>`：修复、文档和维护。
- `experiment/<主题>`：探索性验证；不默认进入 `main`。

一个任务一个短期分支，一个 agent 一个独立 Git worktree。并行任务先约定文件所有权；不能让多个 agent 在同一个工作目录切换分支或同时修改相同文件。worktree 放在本地 `.worktrees/`，不提交。

## 合并

1. 从最新 `main` 创建任务分支；提交只包含该任务的改动。
2. 提交 Pull Request，说明目的、实现、验证命令与结果，以及明确存在的限制。
3. 由负责实现以外的审查者检查差异；多个 agent 共用 GitHub 身份时，不伪造独立账户审批。
4. 合并前必须通过 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`cargo build --examples --locked`；图形路径还需实际运行验证，不能把单元测试通过称为 GUI 已跑通。
5. 默认 squash merge，保留清晰的功能级历史；合并后同步 `main`，删除不再使用的分支与 worktree。

不直接向 `main` 堆积未完成实现，不 force-push `main`。相关 CI 与仓库端合并限制须在配置并验证后才称为已启用；本文件是协作约定，不是设置生效的证明。

## 提交范围

- 提交源码、测试、依赖锁文件、必要文档及 CI；不提交构建产物、临时截图、机器配置或密钥。
- 第三方参考全文、附件与原始快照保留本地；公开仓库保留出处与归档说明，不在许可不明时公开再分发。
- 不擅自替项目选择开源许可证；公开可见与授予开源许可是两件事。
- `AGENTS.md` 只做极简目录索引，细节归入对应文档。
