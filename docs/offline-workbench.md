# 离线知识工作台（M5B）

这是实际桌面应用示例，不是联网整理完成声明。所有模型、存储、离线规则均在 examples 应用层；只使用框架公开布局、连接线、受控 Signal、主题、语义、动态子树、滚动与后台消息 API，不访问 Scene/Renderer。

## 运行与文件合同

```sh
cargo build --examples --locked
cargo run --locked --example knowledge_workbench -- --path /trusted/local/directory/notes.json
```

必须显式指定可信本地普通文件路径。启动在工作线程读取，缺失文件显示空工作台；损坏文件显示恢复错误，不自动覆盖。文件格式、限额与原子替换边界见 [知识文档](knowledge-document.md)。不承诺恶意目录/符号链接防护、特殊文件读超时、断电持久性、Windows 替换资格或多写者协调。选择可信的普通本地文件；I/O 不占 UI 线程，关窗不等待无限 I/O。保存异步，关闭前必须等到 Saved snapshot；显示的是点击时快照，而非其后所有修改已自动保存。

左侧是待整理原文，右侧是最后有效树。主动 Paste 才读取剪贴板；无监听、网络或自动上传。粘贴只更新待整理区，不替换有效树，不自动落盘。失败保留原有效树。Synthetic fixture 明确加载公开合成示例；Offline line tree 把待整理原文按 LF 分段产生单层树，不声称语义分类或 LLM 输出。源文本不 trim/规范化；标题仅取源行前 60 个 Unicode scalar，空行明确命名。离线规则额外限制为最多 127 行及 64 KiB，超过时拒绝而非截断原文。空原文形成单根树。该规则不是通用长文知识整理或多行编辑器。

树图依知识文档父关系自动生成先序布局、矩形节点与折线连接。每个节点有稳定知识 ID，对应一次装载内稳定挂载的 UI 节点、选择与展开 Signal。展开折叠通过祖先可见性保留整个子树；不卸载、不串状态。**折叠不压缩先序布局中的预留位置**，因此会留下空白及相应滚动范围；这是有界 128 节点 Alpha 的明确取舍，不是虚拟化/自由画布。深度每层缩进 8 像素，宽度在有界深度内收窄，长标题在节点框内裁剪；尚无节点改名、拖拽、横向平移或自由关系连线。

点击节点切换展开并选择它；Tab/Shift+Tab 与 Space/Enter 使用框架默认键盘路径。树和源文各自纵向滚动。主题切换只变主题，不重建文档、挂载节点或展开/选择/滚动；忙碌时仍可切换主题。保存快照记录原样源文、有效树、引用与每个节点展开状态；重启恢复这些数据。选择、主题、滚动是会话视图状态，不写入 M5A 文档，重启回到默认视图。新文档装载/离线整理替换整树；不承诺跨文档保留框架 NodeId。

## 可复验门禁

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --examples --locked
python3 scripts/verify_knowledge_workbench.py \
  --app target/debug/examples/knowledge_workbench \
  --owner target/debug/examples/knowledge_clipboard_owner \
  --output .local/verification/workbench-run
```

需要已有 Xvfb、xdotool、Pillow 与支持的图形/字体环境；脚本不安装软件、不改系统输入法。每次输出目录必须不存在。自有 X11 server 隔离用户桌面和剪贴板；独立 clipboard owner 持有公开合成文本，工作台实际 Paste 异步读取，测试不注入应用消息或改状态文件冒充操作。F9 只在 `CHRONOBREAK_WORKBENCH_VERIFY` 开启时输出所列控件和知识节点的语义身份/几何/状态及源字节数，不输出源文或鉴权。脚本通过真实键鼠触发它、定位控件，比较真实保存文件并重启进程。正常模式不输出这些探针。

2026-11-14 本地 NVIDIA RTX 2060 / Vulkan / Xvfb 1024×768，实际窗口 760×520，17/17 断言通过：空态、合成 fixture、折叠后隐藏语义、主题/选择/挂载身份保留、异步存储、CRLF/Unicode 精确保真、真实进程重启、独立剪贴板粘贴保留旧树和旧文件、显式离线替换、实际滚轮几何、粘贴文档保存重启、真实缺失父目录保存失败保留内存有效树。截图、进程日志、checks.json 保留本地 `.local/verification/loop-2/`，不作为原生 Wayland、实际 IME、新无障碍桥接或网络资格。源文显示无完整控制字符可视化承诺；字节保真由存储与测试验证，不把截图当逐字保真证据。

独立审查和受保护主线集成状态以 [Alpha 交付状态](alpha-delivery-status.md) 为准。M5C 的显式真实请求、取消/超时与失败矩阵，以及 M6 性能/系统无障碍、M7 发布均未由此离线脚本覆盖。
