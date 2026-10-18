# 项目约定

## 分层与依赖方向

纯 Rust 的 Cargo 工作区，Windows 原生界面（Win32 + Direct2D + DirectWrite + DirectComposition）。

```
app  →  export  →  text  →  core
  ↘      table  ↗
```

- `crates/core`：文件、路径、设置、会话、历史、整理、插件安装、界面文案（`i18n`）。不碰任何窗口或绘制。
- `crates/text`：Markdown 解析配置、语言检测、代码高亮、大纲、链接、编辑操作。纯函数。
- `crates/table`：表格引擎，纯函数。
- `crates/export`：纯 Markdown / ZIP / HTML / PDF 导出。
- `crates/app`：唯一依赖 `windows` 的 crate，负责窗口、绘制、输入。下层 crate 不得引用 `app`，也不得引入 Win32。
- 启动性能是首要指标：冷启动路径上不加同步 IO 或重初始化，能放到后台线程或首帧之后的就放过去。改动启动路径后用 `MDWISP_TRACE=<文件>` 看各阶段耗时。

## 路径：唯一入口

任何写进 markdown 的资源路径都必须经过 `crates/core/src/paths.rs` 的 `build_relative_path` 或 `to_link`。它会拒绝绝对路径、盘符和 `file://`。

新增涉及路径的功能时：

1. 不要自己拼字符串。找到 `build_relative_path` 或 `to_link`。
2. 如果现有函数覆盖不了你的场景，改那个文件，不要在旁边另写一个。
3. 写完之后问自己：这个文档文件夹拷到另一台机器上，链接还成立吗？不成立就是 bug。

## 文档文件夹：一种格式

标准形态只有一种：`名称/名称.md` + `名称/名称_img/`。不在文件夹里的是散装文件，由 `crates/core/src/organize.rs` 转换。

- 不要再引入第二种布局或布局设置。应用、转换、插件三处都只认这一种。
- 需要遍历文档里的图片引用时用 `organize.rs` 的 `map_image_links`，它跳过围栏代码块和行内代码。自己写正则会把示例当成引用。
- 任何按"文档所在文件夹"整体操作的功能（如 ZIP）必须先确认文档在文件夹内，否则对散装文件就是在操作别人的目录。

## AI 插件

`plugin/` 是 Claude Code 插件，也是格式规范的**唯一文本来源**：帮助面板运行时读取安装目录里 `plugin/mdwisp/skills/*/SKILL.md`（英文界面读 `SKILL.en.md`），安装到项目的技能也从这里复制。

- 改规则只改 `plugin/mdwisp/skills/*/SKILL*.md`，不要在应用里另存一份。
- `skills/doc/scripts/check.mjs` 必须零依赖、可独立运行，不能 import 应用代码。它与 `map_image_links` 的扫描规则是有意重复的，改一处时两处一起改。
- 规则与应用行为必须一致：应用写出的链接要能通过 `check.mjs`（`crates/core/src/plugin.rs` 的测试用它校验欢迎文档，需要本机有 Node.js）。

## 交互层：动作注册表

快捷键、右键菜单、命令面板三处界面**全部**从 `crates/app/src/actions.rs` 的 `ACTIONS` 生成。

不允许：

- 在窗口过程里判断某个按键直接调业务逻辑 —— 变成一个 `Action`。
- 手写菜单项的标题与快捷键提示 —— 从注册表读，否则一定会和快捷键脱节。
- 给某个面板单独维护一份能做的事的列表。

新增动作时，`group` 决定它进右键菜单的哪一段，`scope` 决定它在什么上下文可用，`key` 决定快捷键。三者都填对，三处界面自动同步。

## 界面

即时模式：每帧重画，命中区域来自上一帧（`crates/app/src/ui.rs` 的 `Hit`）。

- 需要跨窗口共享的状态放 `main.rs` 的 `App`；单个窗口的状态放 `Window`，面板自己的状态放各自 `shell/` 模块的结构里。不要为了让某处能读到就提升到 `App`。
- 界面文案一律写中文原文并用 `t()` / `tf()` 包住，英文在 `crates/core/src/locale_en.rs`。新增文案必须同时补英文。

## 编辑器

`crates/app/src/editor/` 是单页实时编辑：源码始终是 markdown 原文，`deco.rs` 只算装饰（隐藏标记、行样式、表格与图片部件），`layout.rs` 按装饰排版，不改写文本。

- 新增语法的显示效果时加装饰，不要动文档内容。
- 解析统一用 `mdwisp_text::parser()`；大纲（`text/src/outline.rs`）与编辑器对"什么算标题"的判断必须一致。
- 排版按行缓存、可见区域优先、其余行后台补齐。新增装饰不要让一次按键触发全文重新排版。

## 表格

`crates/table/src/model.rs` 与 `ops.rs` 是纯函数，不碰界面。新增表格能力时：

- 逻辑放引擎里，加测试；界面只负责调用。
- `parse_table` 返回的矩阵是**表头 + 数据行**，不含分隔行；分隔行由 `aligns` 在序列化时重新生成。

## 样式

- 所有颜色、间距、圆角、时长、缓动都取自 `crates/app/src/theme.rs`（从原 `tokens.css` 移植）。**不允许**在其他文件里写颜色字面量或临时时长。
- 动效用 `Dur` 与 `Ease`，尊重 `Motion`（减弱 / 关闭动画设置）。
- 深色是主主题。加浅色适配时，补 `LIGHT` 里的值，不要在绘制代码里写主题分支。
- 导出用的 CSS 在 `crates/export/assets/`，与界面主题各自维护。

## 语言检测

`crates/text/src/detect.rs` 的规则表是数据驱动的。加规则时：

- **强信号必须是该语言独有的**。`function name(` 不是任何语言的强信号——每个 C 系语言都有，它会把差距压垮，让判定退化成"未确定"。
- 拿不准宁可返回 `None`。错误的高亮比没有高亮更糟。
- 改完跑 `cargo test -p mdwisp-text`，它同时验证"该认出来的"和"不该猜的"。

## 代码风格

- 4 空格缩进，单行放得下的表达式不拆行，与周围代码保持一致。
- 不给不可能出现的输入写防御性检查。要的是领域校验。
- 注释只写"为什么"，不写"做了什么"。读代码就知道的事不要注释。
- 修改已有文件用最小 diff，不要顺手重构无关代码。

## 测试与验证

```bash
cargo test --workspace          # 单元测试
cargo build --release -p mdwisp # 发布构建，target/release/mdwisp.exe
claude plugin validate plugin   # 改过 plugin/ 之后
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dist.ps1   # 安装包（需 Inno Setup 6）
python scripts/icon.py          # 改过 build/icon.svg 后重新生成图标与资源
```

改完界面后的视觉验证用脚本驱动真实窗口，命令格式见 `crates/app/src/script.rs`：

```bash
MDWISP_USER_DATA=<临时目录> MDWISP_SCRIPT=<脚本> target/release/mdwisp.exe <临时文档副本>
```

- 脚本只打开临时目录里的文档副本：自动保存开着，打字会写回磁盘。
- `shot` 截的是应用自己的帧，不是屏幕。截图按轮次存到 `design-review/`（已忽略），用于对比是否退化。
- 测量或结束进程只针对自己启动的 PID，本机可能正开着安装版 MDWisp。

## Git

- `main` 不接受直接提交。
- 功能分支命名：`feat_SC_<简述>` / `fix_C_<简述>`（S 后端、C 前端、SC 前后端）。
- 提交信息用英文，正文说明**为什么**这样改，不是罗列改了哪些文件。
