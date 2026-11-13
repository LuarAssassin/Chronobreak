# 初始架构方向

Chronobreak 是用 Rust 构建的保留模式、自绘 GUI 框架，不是对现有完整 GUI 框架的外层封装。

## 第一版取舍

- 界面表达：优先使用原生 Rust builder；在真实使用反馈出现前，不引入自定义语法或过程宏。
- 状态更新：采用 Signal 思路，明确依赖订阅、批量更新和节点销毁时的清理；首版聚焦 UI 线程，不提前泛化为跨线程响应式系统。
- 布局：复用 `taffy` 的 Flexbox；文字测量必须接入真实排版结果，不以字符数乘字号代替。
- 文字：优先探索 `cosmic-text` 与 `glyphon` 的组合，支持中文与字体回退。
- 绘制：从 UI 树生成独立于 GPU 的显示列表，由 `wgpu` 执行；处理绘制顺序与裁剪。
- 平台：使用 `winit` 处理窗口、事件循环、尺寸与 DPI 变化；区分逻辑坐标与物理像素，空闲时不持续重绘。

具体 crate 版本由实现时核对兼容性后锁定。成熟底层库负责标准与平台细节；项目负责组件模型、树的生命周期、更新传播和交互语义。

## 第一个可运行目标

一个由框架自身渲染的信息卡片，包含标题、中文说明和计数按钮。点击按钮更新数字；改变窗口大小后文字正确换行，布局随之更新；交互经过事件命中、回调、状态变化、布局和绘制的完整链路。

验证分开记录：不依赖窗口的状态／布局／命中测试，以及真正运行渲染路径后的画面与交互验证。显示环境缺失时明确指出，不能把编译通过当作图形验证通过。

首版不扩展为完整控件库，不加入过程宏、动画系统、复杂文本编辑、插件系统或跨平台发布流水线。输入法、无障碍等保留为后续明确功能，不虚称首版已支持。

## 当前实现边界

- `src/node.rs`：框架 `NodeId = scene owner + slot + generation`，free-list 复用槽位；代际耗尽退役槽位，scene 身份耗尽明确终止，不环绕复活。taffy ID 仅存在私有节点映射与布局树中，不是应用身份。
- `src/ui.rs`、`src/children.rs`：builder 挂载与 UI 提交队列管理动态树；节点维护父子链接，容器内 key 唯一。`insert/remove/move_before/reorder` 在提交边界验证身份、代际与直接父级；只支持同容器重排，不提供可形成环的跨容器 reparent。文本 buffer 同时用于布局测量和 glyphon 绘制，宽度约束使用 `WordOrGlyph`。
- `src/input.rs`：已合入 M2a 的类型化事件/上下文与 Scene 内交互状态，已合入 M2b 增加 Wheel。最深命中节点的真实祖先路径按 capture→target→bubble 访问；独立传播/默认标志，最后执行唯一控件默认动作。hover、pressed、capture、focus、Space 身份均使用完整 NodeId；回调无 arena 可变引用，仍由正式提交统一失效。行为与迁移见 [输入路由](input-routing.md)。
- `src/scroll.rs`：M2b 初始纵向视口已合入 PR #10；固定高度 wrapper、UI 句柄、最终测量后 clamp、剩余量链与一次性焦点揭示。
- `src/text_edit.rs`：独立于 Signal/布局的 UTF-8 逻辑模型；UAX #29 字符簇/词、定向 selection、有界历史及程序替换，见 [模型合同](text-edit-model.md)。
- `src/text_input.rs`：节点拥有的单行编辑适配、受控 Signal、整形 hit/affinity、私有水平 caret tracking、异步系统剪贴板及聚焦 deadline；经现有路由/捕获、纵向焦点揭示、裁剪绘制与 WaitUntil/Retry 集成，不暴露 Scene/Renderer。合同与平台边界见 [单行输入](text-input.md)，原生 IME 另行接入。
- `src/signal.rs`：UI 线程 `Rc<RefCell<_>>` 状态；变化递增版本，由线程局部弱引用 FIFO 非递归通知。普通更新结束即 drain，`batch` 嵌套到最外层结束；同 Signal 合并最终值，新版本放弃旧版本余下回调。1024 因果通知任务预算超限明确 panic，不把大量独立更新误判为循环。没有回滚、跨线程状态或 effect 图。
- `Element::bound_text(&Signal<T>, Fn(T) -> String)` 支持 `Clone + PartialEq + 'static` 状态与可捕获环境的闭包。私有类型擦除把最新原始值排队，格式化闭包归节点所有，不放进 Signal 监听器；即使格式化闭包捕获源 Signal，也不形成 Signal → 监听器 → Signal 强引用环。初始化在挂载时读取最新值，之后同帧多次更新按节点、属性合并，格式化只处理最终文本值。`Scene::sync` 修改绑定节点并标记 taffy 脏路径，不重建 UI 树。
- `Element::visible(&Signal<bool>)` 使用 taffy `Display::None`，隐藏父节点排除整棵后代的布局空间、显示列表与命中，保留状态、节点和订阅；取消被隐藏子树内的按压，不误取消其他按钮。卸载递归移除 arena/taffy/父子链接、key、订阅、格式化/点击闭包、buffer、pending、按压与失效命令负载。取消订阅阻止尚未开始的通知，pending 消费仍校验完整 NodeId，防止历史负载命中复用槽位。
- `src/render.rs`：显示列表保持背景/文本交错画家顺序，`Paint::Text(NodeId)` 为可失效进程内引用；Renderer 的可删除 `BTreeMap<NodeId, TextRenderer>` 仅保留本帧可绘制文字，隐藏/完全裁剪节点不 prepare 并逐帧清退缓存。CPU buffer/局部状态仍保留，恢复时重建对应 renderer。真实资源在途引用由 wgpu 管理，不手工复用。显示列表仍借用 Scene 的 buffer，不声称完整 IR 已序列化、脱离 Scene 或支持多后端；每次仍为全帧绘制。
- `src/application.rs`：`Application::new(root).title(...).run()` 统一拥有窗口、`EventLoop<UserEvent>`、Scene、Renderer 和重绘调度，无业务专用状态。布局和命中使用逻辑像素，输入/surface 使用物理像素；零尺寸、遮挡和暂停不忙重绘，保留呈现意图。surface Retry 沿用 100ms 等待；成功且无新变化时回到 Wait。
- `src/messages.rs`：`message_channel(NonZeroUsize)` 返回可跨线程的 `MessageSender<T>` 和 UI 专属接收器，`Element::on_message` 将处理闭包绑定完整 NodeId。每 inbox 独立有界，Full/Closed 归还数据；Scene 共用一个合并唤醒状态和无负载 EventLoopProxy 事件，回调/丢弃负载/调用 proxy 前释放锁。发送者只持队列及弱 wake，不持节点/闭包；取消、已提交卸载与关闭断开路由。
- `examples/`：基础面板、动态树、批处理与后台消息共用唯一窗口事件循环；`background_messages` 用真实 std 线程展示延迟进度、背压、取消及目标替换，不新增公开调试 API。

## 同步与生命周期

Signal 修改值的闭包仍持有可变借用，不能在其中直接或间接再次读写同一 Signal。监听器释放借用后按注册 token 顺序执行；每次 dispatch 快照值、版本及 token，逐个检查版本与是否仍订阅。A 收到 1 写 2 时，B 不再接收旧 1；队尾新 dispatch 给 B 最新 2。新增订阅不加入当前快照，取消不能中止已开始的回调。批内 get 立即读到写入值；批末只通知最新值（包括有变化后回到起点），格式化仍延迟到 Scene 提交。

订阅取消通过弱引用，不延长 Signal 生命周期；框架监听器仅弱引用待更新队列，节点持有格式化闭包与所有订阅。应用自己调用 `Signal::subscribe` 时应避免把源 Signal 强引用捕获进监听器，或确保外部 Subscription 按时释放。没有跨线程事务或完整 effect 系统。

私有 `Scene::flush(width, height)` 连接有限结构命令快照、节点/属性最终值、格式化、structure/text_measurement/visibility/viewport 脏原因、必要布局和呈现意图。文字/可见性实际相同、无效命令、同序重排不产生可见提交；隐藏子树的文字/结构更新保留最新内容，恢复时才布局呈现。结构命令先于属性，格式化闭包归节点；只应格式化，不承担业务写入（示例日志仅作验证）。新入队结构命令留下一边界，不提供事务回滚。

`commit_version` 是 Scene 内容提交或逻辑 viewport 改变后的单调序号，不是 Signal 版本或帧数；`geometry_version` 表示当前 bounds/clip 已包含的提交。无几何影响时复用 bounds 并推进版本，有几何影响先完成布局再推进。`about_to_wait`、redraw、尺寸/DPI 更新和每次鼠标按下/松开都使用同一提交边界；鼠标入口先读取当前窗口尺寸/scale，再命中最新已提交几何，不混用新属性和旧 bounds。允许该版本先于上次实际显示版本，不维持旧画面树。

呈现意图保持到成功 present；应用只合并请求 redraw，不因点击返回值或无变化 flush 连续请求帧。临时 surface 错误保留意图并等待 100ms，重试提交期间最新值，成功回到 Wait。仍为全帧 GPU 绘制，不声称 GPU 局部重绘。M1a/M1b/M1c 已分别合入 PR #6/#7/#8；历史证据见 [M1b 验收](batched-updates.md) 与 [后台消息](background-messages.md)。

M1c 的 user_event 仅标记消息已就绪；每个 `about_to_wait` 最多轮转处理全场景 64 条，避免某些平台同一轮继续消费新 proxy 事件导致饥饿。消费前确认唤醒并先提交旧结构命令；消息回调在一个 M1b batch 内执行，批后统一提交，再沿原路径必要布局/呈现。队列仍有消息则重新投递一次 proxy；生产者在确认前发送由已有事件覆盖，确认后发送会投递后继事件，不因消费端末尾检查而丢唤醒。卸载命令在批末提交，同批尚未开始的消息不因排队删除立即失效；需要立即关闭时使用 UI 取消句柄。暂停/遮挡/零尺寸继续接收和有限更新模型，但无轮询或忙重绘；恢复后呈现最新状态。详见 [后台消息合同](background-messages.md)。

M2a 已合入 PR #9：在窗口输入前复用同一 flush/逻辑几何快照；鼠标移动/各按钮、键盘及修饰键接入原循环，DPI 变化重投影已有物理指针。单个路由及默认动作在 M1b batch 内完成，事件末由原调度合并呈现。仅 hover/focus 变化不标记文字测量脏；边框使用四条裁剪内矩形，未修改 Renderer 或引入主题样式层。失焦/暂停清理交互，尺寸/DPI 变化保留原按压取消规则；有效捕获允许控件外路由，但不调用桌面 cursor grab/confine。仅收到非空 preedit 时暂缓路由快捷键并撤销 Space 待激活，不是 IME 支持。

M2b 初始纵向滚动已合入 **PR #10**。`Element::scroll_view(height, content)` 建立固定逻辑高度的专用 wrapper，单独内容子树参与完整测量；`src/scroll.rs` 管理 UI 线程句柄、有限命令快照、extent/clamp、最近边缘揭示和只读比例指示条。extent 取未平移 taffy 布局，绘制几何在 placement 中减去累计祖先 offset，逐层求 clip 交集；文字位置、绘制顺序、命中、焦点框和捕获共用这份窗口逻辑几何。纯 offset 更新只 placement，不重跑 taffy 或文字整形；可见文字准备仍由原 display list 控制，不卸载或虚拟化裁剪内容。

flush 先提交结构/属性并测量、clamp，再执行排队滚动与一次性焦点揭示，刷新 placement，最后对静止指针重算 hover。新焦点只排队揭示，不在 Tab 遍历中变更几何，不因旧焦点每次 flush 抢回手动滚动。滚轮走实际指针命中路径而非捕获 owner；先完成普通路由，未 prevent 时独立执行由内到外的剩余量链，stop 不改变默认链。geometry_version 包含滚动提交，无变化不请求帧。100ms surface Retry、零尺寸/遮挡/Wait 与 M1c 公平调度不变。完整 API/有限可见性政策见 [纵向视口](scroll-views.md)。横向、虚拟化、惯性/平滑/触控、M3 编辑/IME、主题、任务运行时和 LLM 产品不在此切片。

## 参考来源

设计背景来自 [AKR 的原文](https://x.com/i/article/2120793311216836608)，归档边界见 [参考资料索引](README.md)。参考原文是学习资料，不是已经验证的实现或性能保证。
