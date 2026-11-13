# M3A：逻辑文字编辑模型

2026-11-13，`feat/text-edit-model`，集成基线 `bea3c46c54fe6951caca0c30510af059fce86536`（M2b 已合入 PR #10）。本块 M3A:B1 仅为**实现候选，待协调者独立审查与合并**；不代表整个 M3 完成，也不代表 TextInput、剪贴板或原生 IME 已可用。

## 入口与边界

`src/text_edit.rs` 提供公开 `TextEditModel`，核心类型由 crate 根重导出；`InvalidTextPosition` 位于 `chronobreak::text_edit`。模型只持有 UTF-8 `String`、定向 selection、文字 revision 与有限撤销记录，不依赖窗口、Signal、Scene 或 Renderer。`examples/text_model.rs` 是真实 CLI 调用者：

```sh
cargo run --example text_model --locked
```

```rust
use chronobreak::{Direction, EditUnit, TextEditModel};

let mut model = TextEditModel::new("你好 👩🏽‍💻");
model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
assert_eq!(model.selected_text(), "👩🏽‍💻");
model.edit_group(|model| {
    model.replace_selection("🇨🇳");
    model.replace_selection("!");
});
assert_eq!(model.text(), "你好 🇨🇳!");
assert!(model.undo());
assert_eq!(model.selected_text(), "👩🏽‍💻");
```

没有 TextInput builder、键盘映射、光标/选区绘制、命中、剪贴板或 preedit 状态。允许逻辑保留换行（CRLF 是一个字符簇），不宣称多行控件。扫描与快照适用于小文本；不引入 rope、大文件编辑器或自研 Unicode/整形。

## 字节、字符簇与逻辑方向

- 复用锁定的 `unicode-segmentation 1.13.3`，增加直接依赖，锁文件仅补当前包的依赖边；扩展字符簇与词边界均由该库的 UAX #29 实现负责。
- `new` 的 anchor/focus 均在文字末尾；`set_selection(anchor, focus)` 使用字节偏移，端点必须是完整字符串的字符簇边界或末尾。越界、UTF-8 字节内部、组合附标/ZWJ/旗帜/肤色字符簇内部均原子返回 `InvalidTextPosition`，包括 `usize::MAX`，不 panic、不部分更新。
- `Selection` 只读端点，`range()` 按大小排序；`focus < anchor` 表示反向。`select_all` 正向全选；`reverse_selection` 仅交换端点。移动时 `extend=true` 固定 anchor，focus 可越过 anchor 反转方向；否则已有选区先收缩到方向对应边，不额外跳一步。
- `Direction::Backward/Forward` 表示字符串逻辑顺序，**不是完整视觉 bidi 光标**；混合方向文本仍保持逻辑字符簇安全。视觉 hit test、行边界、affinity 与字形/坐标映射留给后续排版适配器。
- `replace_selection` 插入或替换整个选区，结束后收缩。插入或删除可能让左右字符重新组成簇（如插入 ZWJ、合并旗帜、CRLF），因此在完整新字符串上重新分段，将拼接末端向前取整到合法边界；可能跨过被合并的右侧文字，不留下非法 caret。
- `delete` 优先删选区，不考虑方向/单位；无选区才删除对应单位。边界上的无操作不产生历史。

## 词、空白与标点政策

`EditUnit::Word` 对移动和删除使用同一规则：从 focus 按方向跳过相邻的全空白 UAX #29 span（每个字符满足 Rust `char::is_whitespace`），再消耗一个非空白 span；位于词内部则走到该 span 的对应边缘。纯空白走到字符串边界。标点与 emoji 保留 UAX #29 自身的 span，不按 ASCII 或正则另行合并；英文 `can't` 作为一个词，中文按 Unicode 边界而非词典分词。最终目标向移动方向取整到字符簇边界，绝不拆簇。

精确例：`hi,  世界!\tcan't 42\r\n` 的正向 word 字节终点为 `2,3,8,11,12,18,21,23`；从末尾反向为 `19,13,11,8,5,2,0`。词删除会连同方向上的邻接空白一起删除，标点则是独立停靠点（以库分段为准）；不声称模拟所有平台编辑器的快捷键习惯。

## 历史、分组与内存

- 每次实际改字的用户操作独立成步，不看时钟、不隐式合并连续输入。`edit_group` 以最外层闭包为显式合并边界，可嵌套，返回闭包值；before-selection 取首个实际改字前，after-selection 取最外层退出时，包含方向。组内 undo/redo 返回 false，避免跨越未完成组。
- undo/redo 恢复精确文字与定向 selection，但文字 revision 向前递增、不恢复旧值。新有效编辑清 redo；空组、边界删除、相同文字替换不清 redo。相同文字替换允许仅收缩 selection；组内最终文字与起始相同也不留历史、保留 redo，但期间真实改字的 revision 仍累计。
- 分组不是回滚事务。闭包 panic 时保留已改文字，并由 guard 在 unwind 时完成分组；测试覆盖嵌套 panic 后模型仍可继续编辑。
- 默认 **128 步 / 1 MiB 文字负载**，由 `HistoryLimits` 在构造时指定。undo+redo 共用预算，条目持有前后两个 `Box<str>` 快照；`HistoryStats` 报告两栈条目数及精确 UTF-8 负载总字节。撤销/重做只移动条目，不复制历史。元数据量由条目上限限制；不将分配器开销冒充精确 RSS。
- 超限从最旧 undo 步淘汰，单步自身超预算则所有旧步和该步均不保留，仍完成编辑；任一预算为零即禁用历史。文字负载预算**不是当前文档大小限制**，也不涵盖操作临时快照和显式组期间一个完整起始快照；长时间不结束的组仍会保留该起始文字。没有大文件或全进程内存上限承诺。

## 程序替换与未来 Signal 适配器

| 操作 | selection | undo/redo | revision |
| --- | --- | --- | --- |
| 用户有效改字 | 拼接后向前字符簇取整并收缩 | 独立步或显式组，提交有效分支时清 redo | 每次有效改字 +1 |
| 仅移动/选择、无效偏移 | 移动按合同；无效偏移保持原值 | 不变 | 不变 |
| 相同文字用户替换 | 收缩到选区末端 | 不变 | 不变 |
| `replace_programmatic` 相同值回声 | 完全保持 | 完全保持 | 不变 |
| `replace_programmatic` 不同值 | 保留各端点数值，先限幅到新长度、再向后取合法字符簇边界 | 全清，活动组起始快照也清除 | +1 |
| undo/redo | 精确恢复快照方向 | 两栈间移动 | +1 |

revision 从零开始，仅表示文字变化次数，`u64::MAX` 饱和而不环绕；既不是跨模型版本号，也不是视觉 selection 版本。不同程序值在组内是硬边界，只能撤销之后的新用户编辑，不会跨越外部权威值。

M3B 的 `Signal<String>` 适配器应在事件/模型提交边界读取模型文字，在 Signal 借用结束后显式更新业务值；显式组只发布最终值。外部监听值先比较内容，相同回声完全忽略，不调用用户编辑、不重置 selection/history；不同权威值调用 `replace_programmatic`，不作为新的用户回写再次发布。应保留最近已发布内容/来源令牌以识别批内延迟回声，并以当前内容比较而非仅 revision 去重；外部最终不同值优先于过期本地回声。此块没有实际双向绑定，也不声称适配器重入/批顺序已验收。`bound_text` 等格式化闭包必须保持纯格式化，不要求在其中写业务状态。

## 验证与后续

新增 28 个聚焦模型测试与 1 个公开 API 文档测试；原 122 unit + 4 doc 必须全部保留。用例覆盖精确字符串/selection/revision/undo 轨迹、两类预算、分支、嵌套/空/净零组、程序替换、字节拒绝、邻接重分段；固定种子 8 × 512 次操作与逐边界 Unicode 插入/undo/redo 循环验证 UTF-8、字符簇端点和预算，不依赖时钟或随机依赖。

交付已通过 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（150 unit + 5 doc）、`cargo build --examples --locked` 和 CLI 示例。结果与限制由 [Alpha 交付状态](alpha-delivery-status.md) 汇总；全部原始日志与编译/临时产物只放本工作树 `.local/verification/`。本块纯模型，不新增 GUI，不运行桌面或原生 IME；M3B 绑定/控件与后续原生输入合同仍未完成。
