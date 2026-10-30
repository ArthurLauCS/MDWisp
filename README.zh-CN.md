<div align="center">
  <img src="build/icon.svg" width="112" height="112" alt="MDWisp 应用徽标" />

# MDWisp

[English](README.md) · [简体中文](README.zh-CN.md)

**打开就写，图片随文档一起走。**

一款支持简体中文和 English 的 Markdown 桌面编辑器，面向写作、代码笔记与表格。

原名 MDView。旧设置与历史记录继续使用，仓库名称与新安装的 AI 技能统一使用 MDWisp。

0.5 起 MDWisp 是用 Rust 编写的 Windows 原生应用：窗口约 90 毫秒出现，单进程内存约 60MB（0.4 的 Electron 版约 850 毫秒、450MB、4 个进程）。截图来自 0.4 版，0.5 保持相同布局。

[![Release](https://img.shields.io/github/v/release/ArthurLauCS/MDWisp?color=6a4c9c&label=release)](https://github.com/ArthurLauCS/MDWisp/releases/latest)
![Windows x64](https://img.shields.io/badge/Windows-x64-111111)
[![License: MIT](https://img.shields.io/badge/license-MIT-111111)](LICENSE)

[下载应用](https://github.com/ArthurLauCS/MDWisp/releases/latest) · [安装 AI 插件](#安装-ai-插件) · [开始写作](#开始写作) · [更新记录](CHANGELOG.md)

</div>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/editor-light.png" />
  <img src="docs/images/editor-dark.png" alt="MDWisp 实际界面：层级大纲、直接编辑、代码高亮与表格" width="100%" />
</picture>

## 为写作留出空间

| 写作 | 整理 | 交付 |
| :--- | :--- | :--- |
| 在排版后的正文中直接编辑 | 按标题层级折叠和跳转的大纲 | Markdown、ZIP、HTML、PDF 导出 |
| 正文边距、侧栏宽度和大纲/图片高度可拖动调整 | 图片使用相对路径，跟随文档保存 | HTML 内嵌图片与字体，便于离线阅读 |
| 代码语言自动检测，也可手动选择 | 右侧历史面板，查看差异与恢复版本 | Codex、Cursor、Claude Code 协作规则 |
| 深浅主题，中文圆体与等宽代码字体 | 表格编辑、排序、统计与转置 | 本地文件，可用其他编辑器继续打开 |

## 下载与安装

前往 **[Releases](https://github.com/ArthurLauCS/MDWisp/releases/latest)**，下载 `MDWisp-0.6.1-setup.exe`，也可自行构建（见[开发与验证](#开发与验证)）。安装时可以选择中英文、安装目录，并创建桌面快捷方式。按当前用户安装，不需要管理员权限；已装 0.4.x 时会先移除旧版，设置与历史记录保留。

需要 **Windows 10 及以上 x64**；不支持 macOS 和 Linux。安装包暂未进行代码签名，Windows SmartScreen 可能提示「Windows 已保护你的电脑」，点击 **更多信息 → 仍要运行** 即可。

只需要 AI 协作规则，可以下载同一发行版中的 `MDWisp-AI-skills-<版本>.zip`，无需安装桌面应用。中文规则位于 `.agents`，英文规则位于 `en/.agents`；选择一份复制到项目根目录。`SHA256SUMS.txt` 提供下载文件的校验值。

## 语言与链接

按 **Ctrl+F** 查找当前文档，**Ctrl+H** 打开替换。支持逐个替换、全部替换、区分大小写、全字和正则匹配；**F3 / Shift+F3** 切换匹配，**Esc** 关闭。查找和替换保持排版预览，表格、链接目标、图片路径及隐藏标记中的匹配仍有可见提示。替换可以撤销，只读文档只能查找。历史版本快捷键为 **Ctrl+Alt+Y**。

在「设置 → 外观与字体 → 语言/Language」中切换「中文/Chinese」和「English」，即时生效并在重启后保留，不会翻译文档内容。

按住 **Ctrl 点击**链接跳转。按住时链接显示手形指针。相对 Markdown 文档在新窗口打开，当前文档内的锚点在原窗口定位。支持网址、邮箱、参考式链接，以及表格中的链接。普通点击仍用于编辑。粘贴的裸网址和邮箱即使紧贴中文也会立即变成链接：`网站：https://a.cn。` 链接的是 `https://a.cn`，网址在第一个非 ASCII 字符处结束；含中文路径的网址请从浏览器地址栏复制，浏览器会把中文转成百分号编码。`[English](README.md)` 是链接；`[English]\(README.md)` 是转义后的普通文本。

标题栏文档名右侧的两个小按钮分别在资源管理器中显示当前文档、复制它的绝对路径，命令面板中也有这两条命令。

语法覆盖和现有限制见 [Markdown 支持情况](docs/markdown-support.md)。

## 开始写作

1. **打开就写。** 启动后直接输入，`Ctrl+N` 在新窗口新建，`Ctrl+O` 在新窗口打开已有文档。
2. **第一次保存再选位置。** `Ctrl+S` 选择文件名和目录；取消保存仍保留当前内容。未命名草稿不自动保存、不生成历史快照。
3. **图片跟着文档走。** 保存文档后，粘贴、拖入或插入图片；应用复制图片并生成相对路径。本文图片集中在左侧栏底部。
4. **整理与分享。** 可将散装 Markdown 整理成文档文件夹，再按 `Ctrl+Shift+E` 导出。

新建、打开文档和再次双击应用快捷方式都会打开独立窗口，保留当前文档与未保存内容。已打开的文档不会重复打开，而是把它所在的窗口切到最前。新窗口出现在最前面，并相对上一个窗口向右下错开一个标题栏的距离，方便拖动下面的窗口。关闭窗口时可选择保存、不保存或取消；保存失败会保留该窗口。保存时会检查文件是否已被其他窗口修改，避免覆盖。

保存后的自动保存和历史记录由「设置 → 文档与保存」控制。未命名草稿不会在退出后恢复，关闭前请保存。

输入三个反引号后按回车即可开始代码块，无需先填写语言。悬停或在代码块内编辑时显示语言选择器；点击代码块下方可以继续写正文。正文两侧边距可直接拖动，双击恢复默认。

## 编辑与布局

按 **Ctrl+/** 切换排版编辑与 Markdown 源码；**Ctrl+= / Ctrl+- / Ctrl+0** 放大、缩小或重置正文字号。状态栏显示中文字数、英文词数及选区统计。

行内公式使用 `$...$`，独立公式使用 `$$...$$`，编辑器内暂显示带样式的 TeX 源码；HTML/PDF 导出以 MathML 渲染。选中文字后粘贴网址可直接生成链接。表格中按 **Enter / Shift+Enter** 移到下一行或上一行，长单元格编辑时保持多行高度。

保存文档后，拖动图片区域上方的分隔线，可调整大纲或文件树与图片区域的高度。拖到上下边缘可折叠对应区域，反向拖回即可恢复；双击恢复默认比例，分隔线获得焦点后可用 **上 / 下方向键** 调整。比例会在重启后保留。

其他程序修改文档时，没有未保存编辑的窗口会自动载入更新；有未保存编辑时显示冲突提示。**Ctrl+Shift+R** 可载入磁盘版本，并支持撤销。

## 文档与图片，一起保存

MDWisp 使用普通文件，推荐每篇文档独占一个文件夹：

```text
我的笔记/
├── 我的笔记.md
└── 我的笔记_img/
    ├── 架构图.svg
    └── 操作截图.png
```

图片引用始终相对于 Markdown 文件：

```markdown
![架构图](./我的笔记_img/架构图.svg)
```

下面的 MDWisp 应用架构图就是这个相对路径的实际图片：

![MDWisp 应用架构：原生界面与窗口层调用本地服务，共享解析与表格逻辑，保存文档文件夹并导出多种格式](./我的笔记_img/架构图.svg)

把整个文件夹拷走，图片仍然可用。整理前可预览变更，并选择是否保留原文件。

| 格式 | 适合什么场景 |
| :--- | :--- |
| Markdown | 继续编辑；纯文本导出，可在导出面板预览图片等内容的处理方式 |
| ZIP | 携带完整文档文件夹及图片，交给他人继续编辑 |
| HTML | 单文件离线阅读，内嵌图片、样式与字体 |
| PDF | 固定版式阅读与打印 |

## 安装 AI 插件

提供两项能力：**doc** 生成符合文档文件夹格式的文档，**share** 预览并移除本地图片与链接，整理可单独分享的 Markdown。三种客户端使用[同一份规则](plugin/mdwisp/skills)，无需分别维护提示词。

可要求「不生成图片」或「尽量生成图片」；实际图片生成能力取决于所用客户端已连接的工具。

### Codex 与 Cursor：通过应用安装

1. 在 MDWisp 中打开 **使用说明 → 给 AI 的协作规则**。
2. 找到 **Codex / Cursor**，点击 **安装到项目**。
3. 选择准备写文档的项目文件夹。
4. 在 Codex 或 Cursor 中打开同一文件夹，开始新会话；若未出现技能，重启客户端。

安装后目录如下。安装器会保留已有自定义内容，遇到不同版本时提示先备份处理。

```text
你的项目/
└── .agents/
    └── skills/
        ├── mdwisp-doc/
        │   ├── SKILL.md
        │   └── scripts/check.mjs
        └── mdwisp-share/
            └── SKILL.md
```

| 客户端 | 创建文档 | 整理交付 |
| :--- | :--- | :--- |
| Codex | `$mdwisp-doc 为这个项目写一份使用手册` | `$mdwisp-share 整理可分享的单文件 Markdown` |
| Cursor | `/mdwisp-doc 为这个项目写一份使用手册` | `/mdwisp-share 整理可分享的单文件 Markdown` |

**不使用桌面应用：** 解压发行版中的 AI 技能包，将 `.agents` 文件夹复制到项目根目录。如果目标已有同名技能，先比较内容，保留自己的修改。希望所有项目可用，可将两个 `mdwisp-*` 文件夹放入用户目录 `~/.agents/skills/`。

这里使用客户端的本地 Agent Skills 支持；它不依赖应用商店上架。路径与加载方式见 [Codex Skills 文档](https://learn.chatgpt.com/docs/build-skills)和 [Cursor Skills 文档](https://cursor.com/docs/skills)。

### Claude Code：从 GitHub 安装

在终端执行：

```bash
claude plugin marketplace add ArthurLauCS/MDWisp
claude plugin install mdwisp@mdwisp
```

在新的 Claude Code 会话中使用：

```text
/mdwisp:doc 为这个项目写一份使用手册
/mdwisp:share 整理可分享的单文件 Markdown
```

也可以在 MDWisp 的「给 AI 的协作规则」中，点击 Claude Code 的 **复制安装命令**，把命令粘贴到 Claude Code 会话中，使用随应用分发的本地插件。

如果已下载源码或 AI 技能包，在解压目录运行：

```bash
claude plugin marketplace add ./plugin
claude plugin install mdwisp@mdwisp
```

用 `claude plugin list` 检查安装结果。旧版 `mdview` 技能不会被自动删除；请按上面的命令安装新版 `mdwisp` 插件。此处指 **Claude Code** 的插件系统；安装机制见 [Claude Code 插件市场文档](https://code.claude.com/docs/en/plugin-marketplaces)。

### 检查 AI 生成的文档

校验脚本需要 Node.js，无需安装依赖。在安装技能的项目根目录运行：

```bash
node .agents/skills/mdwisp-doc/scripts/check.mjs "./我的笔记"
```

从源码或 AI 技能包运行时，也可使用 `plugin/mdwisp/skills/doc/scripts/check.mjs`。它会检查文件夹命名、Markdown 主文件及图片引用等约定。

## 常用快捷键

| 操作 | 快捷键 | 操作 | 快捷键 |
| :--- | :--- | :--- | :--- |
| 新建文档 | `Ctrl+N` | 打开文档 | `Ctrl+O` |
| 保存 | `Ctrl+S` | 导出 | `Ctrl+Shift+E` |
| 命令面板 | `Ctrl+P` | 设置 | `Ctrl+,` |
| 历史版本 | `Ctrl+Alt+Y` | 插入代码块 | `Ctrl+Shift+C` |
| 插入图片 | `Ctrl+Shift+I` | 显示 / 隐藏侧栏 | `Ctrl+\` |
| 撤销 | `Ctrl+Z` | 重做 | `Ctrl+Shift+Z` / `Ctrl+Y` |

按 `F1` 查看按功能分类的完整列表。格式和表格操作取决于当前编辑位置；导出等应用级快捷键在设置、历史等面板获得焦点时仍可使用。

## 已知限制

以下与 0.4 版不一致，后续版本补齐：

- 编辑器内公式显示为 TeX 源码，导出时正常渲染。
- 「拼写检查」设置暂不生效。
- 编辑器只支持单选区，「全选匹配」不可用。
- 14 个无快捷键的表格动作（表格样式、最小值/最大值、Excel 粘贴、CSV/JSON/列表转换、导出 CSV、删除整表）列出但不可用。
- 重命名文档时可能覆盖同名文件。

详见[项目状态](docs/STATUS.md)。

## 开发与验证

使用 Rust 编写，界面基于 Win32、Direct2D、DirectWrite 与 DirectComposition。准备稳定版 Rust（MSVC 工具链）与 Git；打安装包需要 [Inno Setup 6](https://jrsoftware.org/isinfo.php)。Node.js 只用于插件 `check.mjs` 的测试。

```bash
cargo test --workspace                 # 单元测试
cargo build --release -p mdwisp        # 输出 target/release/mdwisp.exe
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dist.ps1   # 安装包，输出到 target/installer/
claude plugin validate .
claude plugin validate plugin
```

界面回归用脚本驱动真实窗口，截取应用自身的画面。请使用隔离的配置目录和文档副本，脚本命令见 `crates/app/src/script.rs`：

```bash
MDWISP_USER_DATA=<临时目录> MDWISP_SCRIPT=<脚本> target/release/mdwisp.exe <文档副本>
```

欢迎文档（`stock/欢迎使用/欢迎使用.md`、`stock/Welcome/Welcome.md`，从「帮助 → 入门」打开）同时是格式测试文档：文中有图片、表格和代码，末尾的格式速查把各类链接、文内跳转、行内格式、列表、引用和公式各写了一遍。改动编辑器后，在浅色和深色下从头看一遍，并按住 Ctrl 把链接点一遍。

开发约定见 [AGENTS.md](AGENTS.md)：资源路径集中处理；快捷键、右键菜单与命令面板共享动作注册表；颜色与动效统一取自主题。问题反馈请使用 [Issues](https://github.com/ArthurLauCS/MDWisp/issues)。

## 许可证与字体

应用代码采用 [MIT License](LICENSE)。内置字体使用各自的开源授权，授权文本随源码与安装包分发，明细见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)：[思源宋体 Noto Serif SC](crates/export/assets/fonts/OFL-NotoSerifSC.txt)、[霞鹜文楷 LXGW WenKai](crates/export/assets/fonts/OFL-LXGWWenKai.txt)（均为 SIL OFL 1.1），以及 [Latin Modern Roman](crates/export/assets/fonts/GUST-FONT-LICENSE-LatinModern.txt)（GUST 字体许可证）。字体文件不进 git，从源码运行前先执行 `scripts/fonts.ps1` 下载。
