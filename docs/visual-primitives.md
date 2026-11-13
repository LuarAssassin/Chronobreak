# 有限视觉原语（M4A）

状态以 [Alpha 交付状态](alpha-delivery-status.md) 为准；本块不实现主题/控件系统。

## 验证前登记的容差

2026-11-13，在读取新渲染截图前确定以下规则，失败后不放宽：

- 真 X11 surface，1.0 / 1.5 / 2.0 DPI，逐窗口查询几何并截图；不把桌面黑图或 CPU reference 称作渲染通过。
- 同场景连续合批/禁用合批图像逐字节相同（0 容差），并从实际 `pass.draw` 计数确认减少；文字调用单列，不冒充 glyphon 内部 draw 数。
- 不透明内区、线性 alpha 内区与大字形全覆盖内区：各 sRGB 字节通道误差 ≤ 2；边缘 SDF/AA 与有限阴影 CPU 参考误差 ≤ 4。字形必须有 ≥ 50 个全覆盖色像素，不比较不同字库的轮廓。
- 圆角明显外区、阴影支持范围之外及祖先 clip 外区：背景字节误差 ≤ 2；分数 DPI 边缘必须存在部分覆盖，不将所有边缘硬化。
- 有限资源循环只测 40 次 replace/hide/unmount；每次清空后 text cache 回到 0，固定最大场景 vertex 容量/分配数达到高水位后不再增长；RSS 最后 20 次跨度 ≤ 16 MiB。RSS 非 VRAM，glyphon 图集容量及设备总显存未由此证明。

## 样式与坐标

`Element::corner_radius`、`Border::new`、`Shadow::new(...).offset(...).spread(...)` 为有限装饰。非有限颜色/视觉长度立即拒绝；颜色 [0,1]、圆角/线宽 [0,4096]、blur [0,64]、offset 每轴 [-256,256]、spread [-64,64] 钳位。绘制时圆角/内描边宽度再限制到短边一半；负 spread 可缩成零面积而不绘制。均为逻辑像素。

描边完全在矩形内，不占布局尺寸。阴影为圆角矩形距离场的有限 smoothstep 衰减（轮廓内外 ±blur），不是 Gaussian/CSS 阴影；阴影先画，随后本体、文字、子树。阴影支持范围额外包含一个物理像素 AA 护栏，只扩大自己的视觉范围；不扩大布局/命中。祖先、窗口、滚动视口的轴对齐 clip 始终生效，不能复用包含自身矩形的 `node.clip` 把阴影截掉。圆角背景不是圆角子树裁剪。

`Element::canvas(width,height)` 是固定尺寸容器，`positioned(x,y)` 为直接父节点坐标的绝对放置，不参与父节点内容测量；应用负责位置/树布局。`segment(from,to,width,color)` 为父坐标圆头线段，端点为圆心，零长度为圆点，零宽不画。先插连接线再插节点，按插入顺序绘制。无自动图布局、拖线或自由画布 API；不公开 Scene/Renderer。

## 色彩与 alpha

所有旧 `[f32;4]` 与新增 `srgb8` / `srgba8` 都明确为 **encoded sRGB RGB + 线性 straight alpha**，不是线性 RGB，也不预乘。旧矩形路径误把同一数组作为线性光送入 sRGB target，导致矩形比文字偏亮；现统一解码一次，旧手调矩形会变暗，若原值来自线性光应先逐 RGB `linear_to_srgb` 迁移，alpha 不转换。主题 hex 可直接分解为 `srgb8`。

GPU primitive 使用 CPU 解码后的线性 RGB；SDF coverage 乘 alpha。描边/填充交界先按 coverage 求临时预乘和，再还原 straight 输出，避免透明边界混色。target 只接受 sRGB format；blend 为 `ALPHA_BLENDING`（SrcAlpha / OneMinusSrcAlpha，alpha 为 One / OneMinusSrcAlpha），硬件在线性光合成，target 写入时编码一次。glyphon 0.9 的本地 `text_atlas.rs` / `text_render.rs` / `shader.wgsl` 已核对：显式 `ColorMode::Accurate`，字节颜色在 vertex 解码为线性，mask 仅乘 alpha，彩色图集为 sRGB。文字量化为最近的 8-bit sRGB 字节；不从 token 数学声称完整对比度认证。

## 帧与资源

Scene 在提交/布局后产生借用帧：显示命令 + `NodeId → (&Buffer, version, origin, clip, color)` + `&mut FontSystem`。Renderer 不读取可变 Scene 或 arena 数组位置；NodeId 保留 owner/slot/generation。内容、原生 preedit、约束/字体整形的变动使文字资源版本递增；布局整形可保守增加版本，不等同 Signal revision。帧借用阻止边画边卸载；draw 前校验当前版本，卸载时清理，隐藏/完全裁剪文字不 prepare。

glyphon 的图集 trim/重用要求当前帧可见文字每次 prepare；本块不假定未 prepare 的旧 atlas UV 永久有效，不宣称文字 GPU prepare 零成本。`prepared` / `skipped` / `prepare_us` / `text_created` / `text_cache` / `text_high_water` 记录实际调用与资源对象，不冒充文字上传字节或 VRAM。`shape_calls_total` 是累计 `shape_until_scroll` 调用，`layout_us` 是布局总耗时，不是字形实际重算次数；完全裁剪文字仍可能参与布局测量，隐藏 buffer 保留，现有放置步骤可保守调用整形。仅承诺 visible-only prepare，不承诺所有不可见 CPU 工作为零。primitive buffer 容量按二次幂增长复用，每帧至多一次非空 `queue.write_buffer`；`upload_bytes` 仅 primitive，`vertex_allocations` 包括初始分配。wgpu 管理 in-flight 对象生命周期，无 host unsafe reuse。

显示列表只合并连续、同 pipeline/clip 的 primitive；文字与 clip 改变是硬边界，不按类型全局重排。`CHRONOBREAK_UNBATCHED=1` 仅用于同路径对照，`CHRONOBREAK_TRACE=1` 输出实际提交计数；无局部 GPU 重绘承诺。

## 复验入口

公开树图：`cargo run --example visual_primitives --locked`。`--verification` 为固定颜色/交错参考，`--contiguous` 为 32 连续 primitive + 背景，`--resources` 为 16 可见 + 64 隐藏 + 64 裁剪外文字，`--resources-visible` 为相同 16 可见文字基线，均经公开 Element/Application API。

确定性 geometry/color/order/resource 测试随 workspace tests 运行。系统需安装 Xvfb、xdotool、Pillow 与字体（不随仓库打包）；先构建所有 examples，再执行独立 tracked 脚本，输出必须在当前 source 根的 `.local/verification/`：

```sh
cargo build --examples --locked
python3 scripts/visual_primitives/verify.py \
  --app "$CARGO_TARGET_DIR/debug/examples/visual_primitives" \
  --output .local/verification/surface
```

脚本仅启动专用 Xvfb/Vulkan surface、真实 XTEST 键鼠，查询 client 几何后裁取屏幕；不是 offscreen，也不是 Cua-attested capture。全部自有进程在 finally 停止，不触碰 owner 桌面/全局 provider/IME 设置。`--source-head <40位SHA>` 可用于没有 `.git` 的 clean archive，source 哈希从本地 tracked 源目录独立计算；不依赖 `.local` helper。

## 本轮证据（2026-11-13）

实现/公开示例/脚本提交 `e702bd3bcf99bd05567be8504b8faf425ce01a5f` 的 **64 个 tracked 文件**经 `git archive` 导出，全文件 hash 与工作树相同；从无 `.git`、无旧 `.local` helper 的 source 构建并运行，entrypoint exit 0，**546/546** 检查通过。随后只补交文档记录；最终提交另跑四项精确门禁，不把源码归档的证据混称新版本运行。

环境：独立 Xvfb + NVIDIA GeForce RTX 2060 / Vulkan / NVIDIA 595.99.02，`Bgra8UnormSrgb` + glyphon 0.9 Accurate；Noto Sans Regular（字体文件与 app/source hash 见 provenance），Python 3.14.4 / Pillow 12.1.1。客户端实际为 760×520 / 1140×780 / 1520×1040，不是 offscreen。已打开实际窗口 PNG 检查三 DPI 颜色/边缘与树图，修正示例 intrinsic text 取整导致的标签换行后才冻结上述提交。

| 项目 | 实测 | 本地证据（相对 feature worktree） |
| --- | --- | --- |
| 精确 fmt / strict workspace all-targets clippy / locked workspace test / locked examples build | 全通过；**205 unit + 6 doc，12 examples**，原 192 + 6 保留 | `.local/verification/repro-final/gate-*.log` |
| 三 DPI，Rect→Text→透明 Rect→Text、clip 切换、颜色、大字形、圆角/内描边/阴影/线段 | 预登记容差内；圆角 AA 参考最大字节误差 **1**（门槛 4），所有合批对照 **0 字节差异** | `repro-final/source/.local/verification/surface-final/{checks,provenance}.json`、`reference-*.png` |
| 三 DPI 连续 32 primitive + 背景 | 实际 `pass.draw` **33→1**；相同 14,256 bytes / 1 次 primitive upload，容量 16,384 bytes、分配 2 次 | 同目录 `contiguous-*.log/png`、`measurements.json` |
| 三 DPI 交错参考 | 实际 primitive draws **42→8**；文字 render 调用 5 不变；18,144 bytes / 1 次 primitive upload，容量 32,768 bytes、分配 2 次 | 同目录 `reference-*.log` |
| 相同 16 可见文字，另增 64 隐藏 + 64 裁剪外文字 | prepare/cache 保持 16，跳过 128，primitive upload 保持 2,160 bytes；累计 shape 调用首次 **32→224**、布局耗时 **165→1,013µs**；cold prepare **12,597 / 13,151µs**，仅为样本，不推导加速比 | `resources-visible.log`、`resources.log`、`resource-samples.json` |
| 40 次 mount/hide/show/replace/unmount，200 次真实点击 | cache 峰值 16，hide/unmount 为 0；每轮卸载 live=2 / buffer=0 / subscription=0，slot 高水位 147；最后 20 轮卸载 RSS 跨度 **4,132 KiB**（门槛 16 MiB）；vertex 容量 4,096 bytes / 分配 3 次不再增长 | 同目录 `resource-samples.json`、三组恢复/替换/卸载 PNG；RSS 非 VRAM |
| 原有集成 smoke | 真实 TextInput 受控输入与候选区域路径、Tab/Enter 的 capture/bubble 激活、嵌套纵向滚动、后台线程唤醒/完成回 Wait 全通过 | 同目录 `smoke-*.log/png`、`checks.json` |
| 清理 / prerequisites | 所有自有进程已停止；缺失 binary 的普通与 `python -O` 均 exit 2、未创建输出目录 | 同目录 `cleanup.json`；`repro-final/prerequisite-*.log` |

表中简写 `repro-final/` 均在 `.local/verification/` 下；PNG、原始日志和归档保留到审查。`archive-manifest.json` 给出归档和全部 tracked 源 hash；不是把未跟踪 helper 当交付。初轮 `surface-a` 482 检查通过；中间 `surface-b` 的视觉/资源通过但输入 smoke 因 windowfocus 后立即发 Tab 产生合成按键竞争而 exit 1，不作为交付通过证据。tracked verifier 现等待真实 Focused(true) 后再发键，最终 clean archive 完整通过；容差未放宽。

边界：无 FPS/无限工作负载/VRAM/图集显存释放或完整对比度认证结论；未减少隐藏/裁剪外文字的所有 CPU 测量/整形。真实 IME 引擎资格沿用 PR #13 的 Fcitx5 tuple，本轮只保留既有 IME unit/code-state 与候选区域 smoke，不宣称重跑真实引擎。原生 Wayland、Windows/macOS、动态跨屏 DPI、device 故障和更大压力仍需独立验收。主题/控件、图布局、SVG/图片、自由画布与发布不在本块。
