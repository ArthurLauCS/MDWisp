# Markdown 语法检查 / Markdown syntax review

MDWisp parses Markdown with pulldown-cmark both for live-editing decorations and for HTML/PDF output. Opening a file, following a link and switching interface language preserve its Markdown source.

实时编辑通过装饰显示效果，不重写原文。以下是当前解析器和已安装扩展的检查结果，不代表覆盖所有第三方 Markdown 方言。

| 语法 / Syntax | 实时编辑 / Live editor | HTML / PDF |
| --- | --- | --- |
| H1–H6、Setext 标题 / Headings | 显示、大纲与跳转 / Styled, outline and navigation | 支持 / Supported |
| 段落、换行、转义、实体 / Paragraphs, breaks, escapes, entities | 保留源码，部分标记可见 / Source preserved; some markers stay visible | 支持 / Supported |
| 粗体、斜体、删除线、行内代码 / Bold, emphasis, strike, code | 支持 / Supported | 支持 / Supported |
| 高亮、上下标、下划线 / Highlight, sub/sup, underline | 支持 / Supported | 支持 / Supported |
| 引用、有序/无序列表 / Quotes and lists | 支持 / Supported | 支持 / Supported |
| 任务列表 / Tasks | 可勾选；只读时禁用 / Interactive; disabled when read-only | 显示 / Rendered |
| 分隔线 / Horizontal rules | 支持 / Supported | 支持 / Supported |
| 围栏与缩进代码 / Fenced and indented code | 支持；围栏代码高亮 / Supported; highlighting for fences | 支持 / Supported |
| GFM 表格 / Tables | 显示与编辑 / Rendered and editable | 支持 / Supported |
| 行内、参考式链接 / Inline and reference links | Ctrl+点击 / Ctrl+click | 原生链接 / Native links |
| 自动链接、裸网址、邮箱 / Autolinks, bare URLs, email | Ctrl+点击 / Ctrl+click | 支持 / Supported |
| 相对文件、标题锚点 / Relative files and heading anchors | 应用内跳转 / In-app navigation | 保留 href / href preserved |
| 表格内链接、带链接图片 / Table links and linked images | Ctrl+点击 / Ctrl+click | 支持 / Supported |
| Markdown 图片、参考式图片 / Inline and reference images | 显示；相对文档定位 / Rendered relative to the document | 本地图片内嵌 / Local images embedded |
| YAML front matter | 元数据源码 / Metadata source | 不显示 / Omitted |
| 脚注、定义列表 / Footnotes and definition lists | 保留源码 / Source view | 支持 / Supported |
| HTML、details、分页标签 / HTML, details, page breaks | 部分支持；其余源码 / Partial support; other tags stay as source | 按 HTML 渲染 / Rendered as HTML |
| 数学公式 `$…$`、`$$…$$` / Math | 暂显示带样式的 TeX 源码 / Styled TeX source for now | MathML，浏览器原生渲染 / MathML, rendered natively by the browser |
| Mermaid、TOC | 仅源码 / Source only | 仅源码或代码块 / Source or code block |

## Links / 链接

- Ctrl+click follows links. Ordinary clicks edit text.
- Holding Ctrl shows a hand cursor over links; releasing it or leaving the window restores the text cursor. 按住 Ctrl 时链接显示手形指针，松开或窗口失焦后恢复文本光标。
- Relative paths support spaces, Unicode, percent encoding, `..`, queries and heading fragments. Other documents open in new windows and preserve the current draft. Same-document anchors stay in the current window. 跨文档链接新开窗口，保留当前草稿；同文档锚点在原窗口定位。
- HTTP, HTTPS and mailto use the system handler. Other external protocols and non-Markdown local files are rejected in the main process.
- `[English]\(README.md)`, unresolved reference labels and links inside code are literal text, not navigable links.
- 标题跳转复用大纲标识；重复标题使用 `-1`、`-2` 后缀。找不到标题时显示提示。
- Arbitrary HTML anchors/IDs, live-editor footnote jumps and non-Markdown attachments are outside current navigation support.

## Checks / 验证入口

- `crates/text/src/links.rs`: links, references, autolinks and code exclusions.
- `crates/app/src/editor/deco.rs`: live-editing decorations, front matter and inline HTML styles.
- `crates/core/src/paths.rs`: file resolution and protocol restrictions.
- `crates/export/src/markdown.rs`: HTML output for standard syntax, extensions, math and code blocks.
- `crates/core/src/assets.rs`: fenced and inline image examples are excluded from the sidebar.
- `MDWISP_SCRIPT` runs (`crates/app/src/script.rs`): scripted editing, links, windows and layout on a real window, with self-captured screenshots.

图片整理和资源扫描仍使用项目现有的行内图片扫描规则；复杂参考式图片的整理能力需单独扩展。Image organization and asset scanning still use the existing inline-image rules; complex reference-image organization needs separate work.
