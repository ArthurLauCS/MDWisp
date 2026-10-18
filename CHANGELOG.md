# 更新记录

## 0.5.0 · 2026-10-18

- 用 Rust 重写为 Windows 原生应用（Win32 + Direct2D + DirectWrite）：窗口约 90 毫秒出现（原约 850 毫秒），单进程内存约 60MB（原约 450MB、4 个进程），安装包 14MB（原 102MB）。界面布局、字体与动效保持一致，设置、会话与历史记录直接沿用。
- 安装包改为按当前用户安装、无需管理员权限，安装时自动移除 0.4.x。
- 精简 AI 插件规则文本（约 14.5KB → 10.4KB），插件版本 0.3.2。
- 暂未补齐：编辑器内公式渲染、拼写检查、全选匹配；详见项目状态文档。
- Rewrite as a native Windows app in Rust (Win32 + Direct2D + DirectWrite): the window appears in about 90 ms instead of 850 ms, memory is about 60 MB in one process instead of 450 MB in four, and the installer is 14 MB instead of 102 MB. Layout, fonts and motion are unchanged; settings, sessions and history carry over.
- Install per user without administrator rights; the installer removes 0.4.x.
- Trim the AI plugin rules (about 14.5 KB → 10.4 KB); plugin version 0.3.2.
- Not yet restored: math rendering in the editor, spell check and select-all-matches; see the project status document.

## 0.4.1 · 2026-10-15

- 侧栏支持拖动调整大纲/文件树与图片区域的高度，拖到边缘可折叠，双击恢复默认比例；支持方向键与重启后保留设置。
- 同步中英文 README，补充公式、源码模式、正文缩放、字数统计和外部修改处理说明。
- Resize the sidebar navigation/image split, collapse either section at the edges, reset with a double-click, and preserve the split across restarts with keyboard support.
- Update both READMEs with math, source mode, text zoom, word counts and external-change handling.

## 0.4.0 · 2026-10-15

- 新增行内与块级数学公式渲染；HTML/PDF 导出包含公式字体，支持离线阅读。
- 新增源码模式（Ctrl+/）、正文缩放（Ctrl+=、Ctrl+-、Ctrl+0）和中英文字数及选区统计。
- 自动载入外部程序对未修改文档的更新；存在未保存编辑时提示冲突，可用 Ctrl+Shift+R 载入磁盘版本并撤销。
- 选中文字后粘贴网址可生成链接；表格支持 Enter / Shift+Enter 上下移动，长单元格编辑保持多行高度。
- Add inline and display math rendering, with formula fonts embedded in HTML/PDF exports for offline reading.
- Add source mode (Ctrl+/), reading zoom (Ctrl+=, Ctrl+-, Ctrl+0), and Chinese/English word and selection counts.
- Reload external changes to clean documents automatically; warn when unsaved edits exist and offer undoable disk reload with Ctrl+Shift+R.
- Paste a URL over selected text to create a link; navigate table cells with Enter / Shift+Enter and keep long cells multi-line while editing.

## 0.3.5 · 2026-10-09

- 修复长表格中的查找跳转：按匹配文字的实际显示坐标定位，不再按整个表格组件跳转。
- 支持超高单元格中的重复匹配，以及链接路径、图片路径的可见目标；保持预览和查找框焦点。
- 布局和字号变化后校正查找位置；主动滚动后停止校正，避免被拉回。
- 新增 54 次实际位置检查，覆盖真实大文档、三种窗口/字号、双向循环查找、隐藏路径、横向溢出与只读状态。
- Fix search navigation inside long preview tables by scrolling to the rendered match rather than the enclosing table widget.
- Locate repeated matches in tall cells and visible targets for hidden link/image paths while retaining preview and search-field focus.
- Keep matches visible after layout/font changes while respecting manual scrolling.
- Add 54 actual-position checks across a real large document, three window/font layouts, bidirectional wraparound, hidden paths, horizontal overflow and read-only mode.

## 0.3.4 · 2026-10-09

- 修复输入下划线等 Markdown 标记时自动重复的问题；自动配对和格式操作只更新改动范围，避免长文档滚动跳转。
- 预览中的分隔线可用一次 Backspace 整行删除，并可撤销；代码示例、YAML 和 Setext 标题不受影响。
- 格式快捷键处理中文/英文标点边界、选区空白、嵌套格式、多段文本和含反引号的行内代码；必要时使用兼容的行内 HTML 保持文字不变。
- 修复标题/列表选区误包含下一行的问题。增加多类文本单元测试和真实 Electron 快捷键回归脚本。
- Fix duplicated Markdown delimiters and scroll jumps by typing delimiters literally and applying local edits for pairing and formatting.
- Delete a previewed horizontal rule with one Backspace, with undo support, while preserving code samples, YAML and Setext headings.
- Handle punctuation boundaries, whitespace, nested formatting, multiple paragraphs and backticks in formatting shortcuts; use inline HTML where needed to preserve exact text.
- Keep line-format shortcuts from touching the next unselected line and add broad unit and Electron keyboard regression coverage.

## 0.3.3 · 2026-10-09

- 恢复实时预览中行内代码和列表标记原有的陶土色强调；保留 0.3.2 的排版、搜索和性能优化。
- Restore the original terracotta accent for inline code and list markers in live preview, preserving the layout, search and performance improvements from 0.3.2.

## 0.3.2 · 2026-10-08

### 简体中文

- 查找和替换保持实时预览；表格、链接目标、图片路径及隐藏标记的匹配仍有可见提示。
- 增大默认正文行距，调整标题和段落留白；列表按层级缩进，换行与正文对齐。
- 独立图片居中并增加上下间距；修复分隔符只有空白、没有横线的问题。
- 历史列表只传输摘要，选中后按需读取全文；屏幕外内容延迟排版，侧栏拖动按帧更新。
- 优化历史差异计算的内存占用，减少光标移动时的重复解析，避免 Windows 换行符导致打开文档时重复加载。
- 427 项单元测试、双端类型检查、真实 Electron 编辑与搜索回归、76 个快捷键检查通过。

### English

- Find and replace preserve live preview, with visible matches in tables, link destinations, image paths and hidden Markdown markers.
- Increased default prose line spacing, improved heading and paragraph spacing, and added nested list indentation with aligned wrapped lines.
- Centered standalone images with more surrounding space and fixed invisible horizontal rules.
- History lists transfer summaries and load full text on demand. Offscreen content defers layout, and sidebar resizing updates once per frame.
- Reduced history diff memory usage and repeated parsing during caret movement; avoided reloading CRLF documents unnecessarily on open.
- Verified 427 unit tests, both TypeScript projects, real Electron editing/search regressions and 76 keyboard shortcuts.

## 0.3.1 · 2026-10-07

- GitHub 仓库、AI 插件与新安装的技能统一使用 MDWisp / `mdwisp`；保留已有数据目录、文档内部标记和旧版技能。
- Unified the GitHub repository, AI plugin and newly installed skills under MDWisp / `mdwisp`, preserving existing data, document markers and previously installed skills.

## 0.3.0 · 2026-10-07

### 简体中文

- 新增文件内查找与替换：`Ctrl+F` 查找，`Ctrl+H` 替换，支持逐个替换、全部替换、大小写、全字和正则匹配。
- `F3` / `Shift+F3` 循环切换匹配，`Esc` 关闭搜索；搜索时临时显示 Markdown 源码，确保表格、代码和链接路径中的匹配可见，关闭后恢复实时预览。
- 查找操作同步接入右键菜单和命令面板；历史版本快捷键调整为 `Ctrl+Alt+Y`。
- 替换支持撤销与只读保护，搜索输入框中的编辑快捷键不会误改文档；搜索面板支持中英文及深浅主题。
- 420 项单元测试、类型检查、真实 Electron 搜索与编辑回归通过。

### English

- Added in-document find and replace: `Ctrl+F` to find and `Ctrl+H` to replace, with replace-next, replace-all, case-sensitive, whole-word and regular expression matching.
- `F3` / `Shift+F3` cycle through matches; `Esc` closes search. Search temporarily reveals Markdown source so matches in tables, code and link paths remain visible, then restores live preview when closed.
- Search actions also appear in the context menu and command palette. Version history now uses `Ctrl+Alt+Y`.
- Replacement supports undo and read-only protection. Editing shortcuts in search fields do not change the document. The panel supports both interface languages and themes.
- Verified 420 unit tests, both TypeScript projects and real Electron search/editing regressions.

## 0.2.0 · 2026-10-04

### 简体中文

- 产品更名为 **MDWisp**，保留图标、既有设置、历史数据和 AI 命令标识。
- 修复 Ctrl+点击跳转：支持相对 Markdown 文件、网址、邮箱、标题锚点、参考式链接、表格内链接和带链接图片。
- 按住 Ctrl 时链接显示手形指针，松开或窗口失焦后恢复文本光标。
- 跨文档跳转、新建、打开文件及再次双击快捷方式均打开新窗口，保留当前草稿；窗口独立管理目录和关闭确认，并阻止过期内容覆盖其他窗口的保存。
- 新增 551 条完整配对的中英界面文案；设置中即时切换，重启后保留，切换时保留编辑内容、光标和撤销记录。
- 提供中英 README、欢迎文档、AI 规则和安装器；技能包包含两种语言。
- 修复移除链接误改相邻文字、表格参考式链接和图片无法渲染，以及图片侧栏误读代码示例的问题。
- 补充 [Markdown 语法支持清单](docs/markdown-support.md)，明确数学公式、Mermaid 等扩展目前仅显示源码。
- 420 项单元测试、类型检查、71 个快捷键及真实 Electron 多窗口、编辑和打包导出回归通过。

### English

- Renamed the product to **MDWisp**, preserving its icon, existing settings, history and AI command identifiers.
- Fixed Ctrl+click navigation for relative Markdown files, websites, email, heading anchors, reference links, table links and linked images.
- Links show a hand cursor while Ctrl is held, and return to the text cursor on release or window blur.
- Cross-document navigation, new/open commands and repeated shortcut launches use new windows and preserve drafts. Workspaces and close confirmations are isolated; stale saves cannot overwrite another window's changes.
- Added 551 paired English/Simplified Chinese messages, immediate language switching and persistence without resetting text, cursor or undo history.
- Added bilingual READMEs, welcome documents, AI rules and installer languages; the skills archive includes both languages.
- Fixed unlinking adjacent text, reference links/images inside tables, and image examples incorrectly appearing in the asset sidebar.
- Added a [Markdown support matrix](docs/markdown-support.md); math and Mermaid remain source-only.
- Verified 420 unit tests, both TypeScript projects, 71 keyboard shortcuts, native Electron multiple windows, editing and packaged exports.

## 0.1.2 · 2026-10-03

首次公开发行，提供 Windows x64 安装包和独立 AI 技能包。

### 写作与界面

- 单页实时编辑；启动即可输入，首次保存时选择位置，未命名草稿不自动保存。
- 正文和图片随窗口伸缩；左右边距和侧栏宽度可拖动调整。
- 大纲按标题层级折叠，点击定位到编辑区域顶部；历史版本在右侧展开。
- 修复三个反引号输入后的光标问题，回车创建代码块，代码块下方可继续输入正文。
- 代码语言自动检测，悬停或编辑时显示语言选择器，支持手动选择和撤销。
- 右键二级菜单独立展开，靠近窗口边缘时自动调整方向，支持鼠标与键盘导航。

### 快捷键、导出与 AI

- 修复 Windows 中文输入法下部分组合键无响应，以及 Shift 加数字键的匹配问题。
- 导出快捷键覆盖设置、历史等面板获得焦点的场景。
- 修复安装包缺少导出样式和字体资源的问题；验证 HTML、PDF、Markdown 与 ZIP 导出。
- Codex 与 Cursor 可从应用安装同源的项目级技能；Claude Code 支持本地或 GitHub 插件市场安装。

### 验证

- 357 个单元测试与主进程、渲染进程类型检查通过。
- 71 个快捷键完成实际按键回归；代码块、鼠标定位、右键菜单和侧栏调整完成交互验证。
- 打包后的应用完成四种导出与技能安装检查。

已知限制和未接入的表格动作见[项目状态](docs/STATUS.md)。
