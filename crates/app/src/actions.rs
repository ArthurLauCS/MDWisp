//! Every user-visible operation in the app.
//!
//! Keyboard shortcuts, the command palette and the context menu are all views
//! over this registry: `group` decides the context-menu section, `scope`
//! where an action is available, `key` its shortcut. Nothing binds a key or
//! draws a menu item anywhere else, which is what keeps the three in sync.

use std::sync::LazyLock;

use mdwisp_core::i18n::{current_language, t, tf};
use mdwisp_core::settings::ThemeMode;
use mdwisp_text::ops::{self, EditResult};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

use crate::editor::buffer::Sel;
use crate::editor::deco::Code;
use crate::keys::{self, Mods};
use crate::shell::Modal;
use crate::window::Window;
use crate::{app, clipboard, dialog};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Needs an open document.
    Document,
    /// Always available; separate from `Document` so a right-click on empty chrome still offers it.
    App,
    /// Only while a text selection exists.
    Find,
    CodeBlock,
    Table,
    Image,
    Link,
}

pub const GROUPS: [(&str, &str); 16] = [
    ("format", "格式"),
    ("convert", "转换为"),
    ("insert", "插入"),
    ("find", "查找"),
    ("clipboard", "剪贴板"),
    ("table-row", "行"),
    ("table-col", "列"),
    ("table-cell", "单元格"),
    ("table-data", "数据"),
    ("table-style", "样式"),
    ("table-struct", "结构"),
    ("code", "代码"),
    ("image", "图片"),
    ("link", "链接"),
    ("file", "文件"),
    ("view", "视图"),
];

pub struct Action {
    pub id: &'static str,
    title: &'static str,
    /// Heading level for `heading.N`, whose title is formatted.
    level: u8,
    pub keywords: &'static [&'static str],
    pub scope: Scope,
    pub key: &'static str,
    pub alt_keys: &'static [&'static str],
    pub group: &'static str,
    pub enabled: fn(&Window) -> bool,
    pub reason: fn(&Window) -> Option<String>,
    pub run: fn(&mut Window),
}

impl Action {
    pub fn title(&self) -> String {
        match (self.id, self.level) {
            ("heading.0", _) => t("正文段落").into(),
            (id, n) if id.starts_with("heading.") => tf("标题 H{0}", &[&n]),
            _ => t(self.title).into(),
        }
    }
}

fn yes(_: &Window) -> bool {
    true
}

fn no_reason(_: &Window) -> Option<String> {
    None
}

const fn a(id: &'static str, title: &'static str, key: &'static str, scope: Scope, group: &'static str, keywords: &'static [&'static str], run: fn(&mut Window)) -> Action {
    Action { id, title, level: 0, keywords, scope, key, alt_keys: &[], group, enabled: yes, reason: no_reason, run }
}

// ---- context ----------------------------------------------------------------

pub fn source(w: &Window) -> &str {
    w.doc.as_ref().map_or("", |d| &d.view.buf.text)
}

pub fn sel(w: &Window) -> Sel {
    w.doc.as_ref().map_or_else(Sel::default, |d| d.view.buf.sel)
}

pub fn in_table(w: &Window) -> bool {
    let s = sel(w);
    w.doc.as_ref().is_some_and(|d| d.view.deco.tables.iter().any(|c| c.start <= s.head && s.head <= c.end))
}

pub fn in_code(w: &Window) -> bool {
    code_at(w).is_some()
}

/// The fenced block under the caret: (block, body, lang).
pub fn code_at(w: &Window) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>, String)> {
    let d = w.doc.as_ref()?;
    let head = d.view.buf.sel.head;
    let l = d.view.deco.line_of(head);
    if d.view.deco.lines.get(l)?.code == Code::None {
        return None;
    }
    d.view.code_block_at(head).cloned()
}

pub fn link_at(w: &Window) -> Option<String> {
    let d = w.doc.as_ref()?;
    mdwisp_text::links::link_at(&d.view.buf.text, d.view.buf.sel.head)
}

/// `![…](…)` around the caret.
pub fn image_at(w: &Window) -> Option<(usize, usize, String)> {
    let src = source(w);
    let at = sel(w).head;
    let mut from = 0;
    while let Some(p) = src[from..].find("![") {
        let start = from + p;
        let Some(close) = src[start..].find("](") else { break };
        let Some(end) = src[start + close..].find(')') else { break };
        let end = start + close + end + 1;
        if at >= start && at <= end {
            return Some((start, end, src[start + close + 2..end - 1].to_string()));
        }
        from = start + 2;
    }
    None
}

fn has_path(w: &Window) -> bool {
    w.doc.as_ref().is_some_and(|d| d.meta.is_some())
}

/// Replace the whole document as one edit, touching only what changed.
pub fn apply(w: &mut Window, r: EditResult) {
    let Some(d) = &mut w.doc else { return };
    if d.view.read_only {
        return;
    }
    let (from, to, ins) = crate::editor::buffer::minimal_change(&d.view.buf.text, &r.text);
    let s = r.selection.map_or(Sel::at(r.cursor), |s| Sel { anchor: s.start, head: s.end });
    d.view.buf.replace(from, to, &ins, s, crate::editor::buffer::Kind::Other);
    d.view.buf.isolate();
    w.after_input();
}

fn edit_range(w: &mut Window, from: usize, to: usize, text: &str, cursor: usize) {
    let Some(d) = &mut w.doc else { return };
    d.view.replace(from, to, text, cursor);
    w.after_input();
}

fn select(w: &mut Window, from: usize, to: usize) {
    if let Some(d) = &mut w.doc {
        d.view.buf.sel = Sel { anchor: from, head: to };
        d.view.scroll_into_view(to, false);
    }
}

fn span(w: &Window) -> (usize, usize) {
    let s = sel(w);
    (s.from(), s.to())
}

// ---- action bodies -----------------------------------------------------------

fn inline(w: &mut Window, wrap: &str) {
    let (start, end) = span(w);
    let r = ops::toggle_inline(source(w), start, end, wrap);
    apply(w, r);
}

fn prefix(w: &mut Window, kind: u8) {
    let (start, end) = span(w);
    let r = ops::toggle_line_prefix(source(w), start, end, |i| match kind {
        0 => "> ".into(),
        1 => "- ".into(),
        2 => format!("{}. ", i + 1),
        _ => "- [ ] ".into(),
    });
    apply(w, r);
}

fn heading(w: &mut Window, level: usize) {
    let (start, end) = span(w);
    let r = ops::set_heading(source(w), start, end, level);
    apply(w, r);
}

fn insert_block(w: &mut Window, block: &str) {
    let r = ops::insert_block(source(w), sel(w).head, block);
    apply(w, r);
}

fn table(w: &mut Window, id: &str) {
    let clip = if mdwisp_table::specs::needs_clipboard(id) { clipboard::text(w.hwnd).unwrap_or_default() } else { String::new() };
    let Some(effect) = mdwisp_table::specs::resolve(id, source(w), sel(w).head, &clip, current_language()) else {
        w.toast(t("这个操作在当前单元格不可用"));
        return;
    };
    if let Some(c) = &effect.clipboard {
        clipboard::set_text(w.hwnd, c);
    }
    if let Some((a, b)) = effect.select {
        select(w, a, b);
    } else if let Some(e) = effect.edit {
        apply(w, EditResult { text: e.text, cursor: e.cursor, selection: None });
    }
}

fn zoom(w: &mut Window, size: f64) {
    // Same bounds as the font size field in settings.
    let size = size.clamp(12.0, 26.0);
    app().patch(|s| s.font_size = size);
    w.toast(&tf("正文字号 {0}px", &[&size]));
}

macro_rules! table_action {
    ($id:literal, $title:literal, $key:literal, $group:literal) => {
        Action {
            id: $id,
            title: $title,
            level: 0,
            keywords: &[],
            scope: Scope::Table,
            key: $key,
            alt_keys: &[],
            group: $group,
            enabled: in_table,
            reason: |w| (!in_table(w)).then(|| t("光标不在表格内").to_string()),
            run: |w| table(w, $id),
        }
    };
}

/// Advertised in the menus, but with no engine implementation yet: greyed out with a reason.
macro_rules! table_stub {
    ($id:literal, $title:literal, $group:literal) => {
        Action {
            id: $id,
            title: $title,
            level: 0,
            keywords: &[],
            scope: Scope::Table,
            key: "",
            alt_keys: &[],
            group: $group,
            enabled: |_| false,
            reason: |_| Some(t("这个操作还没实现").to_string()),
            run: |_| {},
        }
    };
}

macro_rules! heading_action {
    ($n:literal, $key:literal) => {
        Action {
            id: concat!("heading.", $n),
            title: "",
            level: $n,
            keywords: &["heading", "title"],
            scope: Scope::Document,
            key: $key,
            alt_keys: &[],
            group: "convert",
            enabled: yes,
            reason: no_reason,
            run: |w| heading(w, $n),
        }
    };
}

pub static ACTIONS: LazyLock<Vec<Action>> = LazyLock::new(|| {
    use Scope::*;
    vec![
        a("document.new", "新建文档", "Ctrl+N", App, "file", &["new", "blank", "草稿"], |w| w.open_in_new_window(None, String::new())),
        a("document.open", "打开文档", "Ctrl+O", App, "file", &["open"], |w| w.open_dialog()),
        a("workspace.open", "打开目录", "Ctrl+Shift+O", App, "file", &[], |w| {
            if let Some(p) = dialog::pick_folder(w.hwnd, t("打开文档目录")) {
                w.open_workspace(&p);
            }
        }),
        a("view.settings", "设置", "Ctrl+,", App, "view", &[], |w| Modal::toggle(w, Modal::settings())),
        a("view.shortcuts", "快捷键", "F1", App, "view", &[], |w| Modal::toggle(w, Modal::shortcuts())),
        a("view.help", "使用说明", "", App, "view", &[], |w| Modal::toggle(w, Modal::help())),
        a("view.palette", "命令面板", "Ctrl+P", App, "view", &[], |w| Modal::toggle(w, Modal::palette(w))),
        a("document.export", "导出文档", "Ctrl+Shift+E", App, "file", &[], |w| Modal::toggle(w, Modal::export(w))),
        a("view.history", "历史版本", "Ctrl+Alt+Y", App, "view", &["history", "快照", "还原"], |w| w.toggle_history()),
        a("document.save", "保存文档", "Ctrl+S", Document, "file", &[], |w| {
            w.save(true);
        }),
        Action {
            enabled: has_path,
            reason: |w| (!has_path(w)).then(|| t("请先保存文档").to_string()),
            ..a("document.reload", "重新载入磁盘版本", "Ctrl+Shift+R", Document, "file", &["reload", "revert", "disk", "还原", "外部修改"], |w| w.reload_from_disk())
        },
        Action {
            enabled: |w| w.doc.as_ref().and_then(|d| d.meta.as_ref()).is_some_and(|m| !m.in_folder),
            reason: |w| match w.doc.as_ref().and_then(|d| d.meta.as_ref()) {
                Some(m) if m.in_folder => Some(t("这份文档已经在自己的文档文件夹里").to_string()),
                Some(_) => None,
                None => Some(t("请先保存文档").to_string()),
            },
            ..a("document.organize", "整理为文档文件夹", "", Document, "file", &["organize", "folder", "文件夹", "散装", "转换"], |w| Modal::toggle(w, Modal::organize(w)))
        },
        Action {
            enabled: has_path,
            reason: |w| (!has_path(w)).then(|| t("请先保存文档").to_string()),
            ..a("document.reveal", "在文件管理器中显示", "", Document, "file", &["reveal", "explorer", "资源管理器", "文件夹"], |w| {
                if let Some(p) = w.doc.as_ref().and_then(|d| d.path()) {
                    dialog::reveal(p);
                }
            })
        },
        Action {
            enabled: has_path,
            reason: |w| (!has_path(w)).then(|| t("请先保存文档").to_string()),
            ..a("document.copyPath", "复制文档路径", "", Document, "file", &["copy", "path", "路径", "绝对路径"], |w| {
                if let Some(p) = w.doc.as_ref().and_then(|d| d.path()).map(|p| p.display().to_string()) {
                    clipboard::set_text(w.hwnd, &p);
                    w.toast(t("路径已复制"));
                }
            })
        },
        a("document.undo", "撤销", "Ctrl+Z", Document, "file", &[], |w| {
            if let Some(d) = &mut w.doc {
                if !d.view.read_only {
                    d.view.buf.undo();
                }
            }
            w.after_input();
        }),
        Action {
            alt_keys: &["Ctrl+Y"],
            ..a("document.redo", "重做", "Ctrl+Shift+Z", Document, "file", &[], |w| {
                if let Some(d) = &mut w.doc {
                    if !d.view.read_only {
                        d.view.buf.redo();
                    }
                }
                w.after_input();
            })
        },
        // ---- inline formatting
        a("format.bold", "加粗", "Ctrl+B", Document, "format", &["bold", "strong"], |w| inline(w, "**")),
        a("format.italic", "斜体", "Ctrl+I", Document, "format", &["italic", "em"], |w| inline(w, "*")),
        a("format.underline", "下划线", "Ctrl+U", Document, "format", &["underline"], |w| inline(w, "<u>")),
        a("format.strike", "删除线", "Ctrl+Alt+S", Document, "format", &["strikethrough", "del"], |w| inline(w, "~~")),
        a("format.code", "行内代码", "Ctrl+E", Document, "format", &["code", "inline"], |w| inline(w, "`")),
        a("format.mark", "高亮", "Ctrl+Shift+H", Document, "format", &["mark", "highlight"], |w| inline(w, "==")),
        // ---- block conversion
        a("prefix.quote", "引用块", "Ctrl+Shift+Digit8", Document, "convert", &["list", "quote", "bullet", "ordered", "task"], |w| prefix(w, 0)),
        a("prefix.ul", "无序列表", "Ctrl+Shift+Digit7", Document, "convert", &["list", "quote", "bullet", "ordered", "task"], |w| prefix(w, 1)),
        a("prefix.ol", "有序列表", "Ctrl+Shift+Digit9", Document, "convert", &["list", "quote", "bullet", "ordered", "task"], |w| prefix(w, 2)),
        a("prefix.task", "任务列表", "Ctrl+Shift+U", Document, "convert", &["list", "quote", "bullet", "ordered", "task"], |w| prefix(w, 3)),
        heading_action!(1, "Ctrl+Shift+Digit1"),
        heading_action!(2, "Ctrl+Shift+Digit2"),
        heading_action!(3, "Ctrl+Shift+Digit3"),
        heading_action!(4, "Ctrl+Shift+Digit4"),
        heading_action!(5, "Ctrl+Shift+Digit5"),
        heading_action!(6, "Ctrl+Shift+Digit6"),
        heading_action!(0, "Ctrl+Shift+Digit0"),
        // ---- insert
        a("insert.link", "链接", "Ctrl+K", Document, "insert", &["link", "url"], |w| {
            let (start, end) = span(w);
            let src = source(w);
            let label = &src[start..end];
            let text = format!("{}[{label}](url){}", &src[..start], &src[end..]);
            let url = start + label.len() + 3;
            apply(w, EditResult { text, cursor: url, selection: Some(url..url + 3) });
        }),
        a("insert.image", "插入图片", "Ctrl+Shift+I", Document, "insert", &["image", "picture", "paste"], |w| {
            if !has_path(w) {
                w.toast(t("请先保存文档，再插入图片；图片会放在文档旁边。"));
                return;
            }
            let filter = vec![(t("图片").to_string(), "*.png;*.jpg;*.jpeg;*.gif;*.webp;*.svg;*.avif".to_string())];
            let files = dialog::open_file(w.hwnd, t("插入图片"), &filter, true);
            if !files.is_empty() {
                crate::shell::insert_images(w, &files);
            }
        }),
        a("insert.codeblock", "代码块", "Ctrl+Shift+C", Document, "insert", &["code", "fence"], |w| {
            let r = ops::insert_code_block(source(w), sel(w).head, "");
            apply(w, r);
        }),
        a("insert.table", "插入表格", "Ctrl+Alt+T", Document, "insert", &["table", "grid"], |w| {
            let r = ops::insert_table(source(w), sel(w).head, 3, 3);
            apply(w, r);
        }),
        a("insert.footnote", "插入脚注", "Ctrl+Shift+W", Document, "insert", &["footnote", "note"], |w| insert_block(w, "[^1]\n\n[^1]: ")),
        a("insert.math", "插入数学块", "Ctrl+Alt+M", Document, "insert", &["math", "latex", "katex", "formula"], |w| {
            let mut r = ops::insert_block(source(w), sel(w).head, "$$\n\n$$");
            r.cursor -= 3;
            apply(w, r);
        }),
        a("insert.mermaid", "插入 Mermaid 图", "Ctrl+Alt+K", Document, "insert", &["mermaid", "diagram", "chart", "flowchart"], |w| {
            insert_block(w, t("```mermaid\ngraph TD\n  A[开始] --> B[结束]\n```"))
        }),
        a("insert.toc", "插入目录", "Ctrl+Alt+O", Document, "insert", &["toc", "outline", "contents"], |w| insert_block(w, "[TOC]")),
        a("insert.hr", "插入分隔线", "Ctrl+Alt+H", Document, "insert", &["hr", "divider", "rule", "separator"], |w| insert_block(w, "---")),
        a("insert.details", "插入折叠块", "", Document, "insert", &["details", "collapse", "fold"], |w| insert_block(w, t("<details>\n<summary>展开</summary>\n\n\n</details>"))),
        a("insert.pagebreak", "插入分页符", "", Document, "insert", &["page break", "pdf"], |w| insert_block(w, "<div style=\"page-break-after: always\"></div>")),
        // ---- clipboard / find
        a("find.open", "文件内查找", "Ctrl+F", Document, "find", &["find", "search"], |w| w.search_open(false)),
        Action {
            enabled: |_| !app().s().read_only,
            ..a("find.replace", "文件内替换", "Ctrl+H", Document, "find", &["replace"], |w| w.search_open(true))
        },
        a("find.next", "下一个匹配", "F3", Document, "find", &["find", "next"], |w| w.search_step(1)),
        a("find.previous", "上一个匹配", "Shift+F3", Document, "find", &["find", "previous"], |w| w.search_step(-1)),
        Action { enabled: |w| w.search_is_open(), ..a("find.close", "关闭查找", "Escape", Document, "find", &[], |w| w.search_close()) },
        a("clipboard.copyPlain", "复制为纯文本", "Ctrl+Shift+Alt+C", Document, "clipboard", &["copy", "plain"], |w| {
            let s = sel(w);
            let src = source(w);
            let text = if s.empty() { src } else { &src[s.from()..s.to()] };
            clipboard::set_text(w.hwnd, &ops::to_plain_text(text));
            w.toast(t("已复制为纯文本"));
        }),
        a("clipboard.copyAll", "复制全文", "", Document, "clipboard", &[], |w| {
            clipboard::set_text(w.hwnd, source(w));
        }),
        Action {
            enabled: |w| !sel(w).empty(),
            ..a("find.selection", "查找选中内容", "Ctrl+Shift+F", Find, "find", &["find", "search"], |w| {
                let s = sel(w);
                if s.empty() {
                    return;
                }
                let src = source(w);
                let text = &src[s.from()..s.to()];
                let at = src[s.to()..].find(text).map(|i| i + s.to()).or_else(|| src.find(text)).unwrap_or(s.from());
                let len = text.len();
                select(w, at, at + len);
                if at == s.from() {
                    w.toast(t("没有其他匹配内容"));
                }
            })
        },
        a("view.toggleTheme", "切换深浅主题", "Ctrl+Shift+T", App, "view", &["theme", "dark", "light"], |w| {
            let light = app().s().theme == ThemeMode::Light;
            app().patch(|s| s.theme = if light { ThemeMode::Dark } else { ThemeMode::Light });
            w.toast(if light { t("已切换到深色") } else { t("已切换到浅色") });
        }),
        a("view.toggleSidebar", "显示 / 隐藏侧栏", "Ctrl+Backslash", App, "view", &["sidebar", "panel"], |_| {
            let v = !app().s().sidebar_visible;
            app().patch(|s| s.sidebar_visible = v);
        }),
        a("view.sourceMode", "切换源码模式", "Ctrl+Slash", App, "view", &["source", "raw", "markdown", "源码"], |w| {
            let v = !app().s().source_mode;
            app().patch(|s| s.source_mode = v);
            w.toast(if v { t("已切换到源码模式") } else { t("已切换到排版编辑") });
        }),
        a("view.zoomIn", "放大正文", "Ctrl+Equal", App, "view", &["zoom", "font size", "字号", "缩放"], |w| zoom(w, app().s().font_size + 1.0)),
        a("view.zoomOut", "缩小正文", "Ctrl+Minus", App, "view", &["zoom", "font size", "字号", "缩放"], |w| zoom(w, app().s().font_size - 1.0)),
        a("view.zoomReset", "恢复默认字号", "Ctrl+Digit0", App, "view", &["zoom", "font size", "字号", "缩放"], |w| zoom(w, mdwisp_core::settings::AppSettings::default().font_size)),
        a("view.showOutline", "大纲", "", App, "view", &["outline", "heading", "标题层级"], |_| app().patch(|s| {
            s.sidebar_visible = true;
            s.outline_visible = true;
        })),
        a("view.showFiles", "文件", "", App, "view", &["files", "tree", "文件导航"], |_| app().patch(|s| {
            s.sidebar_visible = true;
            s.outline_visible = false;
        })),
        // ---- document hygiene
        a("document.tidy", "格式化文档", "Ctrl+Alt+F", Document, "file", &["format", "tidy", "clean", "normalise"], |w| {
            let next = ops::tidy_markdown(source(w));
            if next == source(w) {
                w.toast(t("文档已是规范格式"));
                return;
            }
            let cursor = sel(w).head.min(next.len());
            let cursor = (0..=cursor).rev().find(|c| next.is_char_boundary(*c)).unwrap_or(0);
            apply(w, EditResult { text: next, cursor, selection: None });
        }),
        // ---- table: rows
        Action { keywords: &["row", "insert"], ..table_action!("table.row.insertAbove", "上方插入行", "Ctrl+Enter", "table-row") },
        table_action!("table.row.insertBelow", "下方插入行", "Ctrl+Alt+N", "table-row"),
        Action { keywords: &["delete", "row"], ..table_action!("table.row.delete", "删除本行", "Ctrl+Shift+Backspace", "table-row") },
        table_action!("table.row.moveUp", "上移本行", "Alt+ArrowUp", "table-row"),
        table_action!("table.row.moveDown", "下移本行", "Alt+ArrowDown", "table-row"),
        table_action!("table.row.copy", "复制本行", "Ctrl+Shift+D", "table-row"),
        table_action!("table.row.cut", "剪切本行", "Ctrl+Shift+X", "table-row"),
        // ---- table: columns
        table_action!("table.col.insertLeft", "左侧插入列", "Ctrl+Shift+Enter", "table-col"),
        table_action!("table.col.insertRight", "右侧插入列", "Ctrl+Alt+Enter", "table-col"),
        table_action!("table.col.delete", "删除本列", "Ctrl+Alt+Backspace", "table-col"),
        table_action!("table.col.moveLeft", "左移本列", "Alt+ArrowLeft", "table-col"),
        table_action!("table.col.moveRight", "右移本列", "Alt+ArrowRight", "table-col"),
        table_action!("table.col.alignLeft", "本列左对齐", "Ctrl+Alt+L", "table-col"),
        table_action!("table.col.alignCenter", "本列居中", "Ctrl+Alt+C", "table-col"),
        table_action!("table.col.alignRight", "本列右对齐", "Ctrl+Alt+R", "table-col"),
        // ---- table: cells
        table_action!("table.cell.merge", "合并单元格", "Ctrl+M", "table-cell"),
        table_action!("table.cell.split", "拆分单元格", "Ctrl+Shift+M", "table-cell"),
        table_action!("table.cell.clear", "清空单元格", "Ctrl+Delete", "table-cell"),
        table_action!("table.cell.newline", "单元格内换行", "Alt+Enter", "table-cell"),
        // ---- table: data
        Action { keywords: &["sort"], ..table_action!("table.sort.asc", "按本列升序", "Ctrl+Alt+ArrowUp", "table-data") },
        table_action!("table.sort.desc", "按本列降序", "Ctrl+Alt+ArrowDown", "table-data"),
        table_action!("table.select.all", "选中整张表", "Ctrl+Shift+Space", "table-data"),
        table_action!("table.stats.sum", "本列求和", "", "table-data"),
        table_action!("table.stats.avg", "本列均值", "", "table-data"),
        table_action!("table.stats.count", "本列计数", "", "table-data"),
        table_stub!("table.stats.min", "本列最小值", "table-data"),
        table_stub!("table.stats.max", "本列最大值", "table-data"),
        table_action!("table.paste.fromClipboard", "从剪贴板建表", "Ctrl+Alt+V", "table-data"),
        table_stub!("table.paste.fromExcel", "从 Excel 粘贴", "table-data"),
        // ---- table: styling
        table_action!("table.style.pad", "整表美化（对齐竖线）", "Ctrl+Alt+P", "table-style"),
        table_stub!("table.style.zebra", "斑马纹", "table-style"),
        table_stub!("table.style.compact", "紧凑样式", "table-style"),
        table_stub!("table.style.borderless", "无边框", "table-style"),
        table_stub!("table.style.card", "卡片样式", "table-style"),
        table_stub!("table.style.center", "表格居中", "table-style"),
        // ---- table: structure
        table_action!("table.struct.transpose", "转置表格", "Ctrl+Alt+X", "table-struct"),
        table_action!("table.struct.headerOn", "首行设为表头", "", "table-struct"),
        table_action!("table.struct.headerOff", "取消表头", "", "table-struct"),
        table_stub!("table.struct.toCsv", "表格转 CSV", "table-struct"),
        table_stub!("table.struct.fromCsv", "CSV 转表格", "table-struct"),
        table_stub!("table.struct.toJson", "表格转 JSON", "table-struct"),
        table_stub!("table.struct.toList", "表格转列表", "table-struct"),
        table_stub!("table.struct.exportCsv", "导出本表为 CSV", "table-struct"),
        table_stub!("table.struct.delete", "删除整张表格", "table-struct"),
        // ---- code block
        a("code.copy", "复制代码", "Ctrl+Shift+Alt+E", CodeBlock, "code", &["copy", "code"], |w| {
            if let Some((_, body, _)) = code_at(w) {
                let text = source(w)[body].to_string();
                clipboard::set_text(w.hwnd, &text);
                w.toast(t("已复制代码"));
            }
        }),
        a("code.selectAll", "全选代码块", "Ctrl+Shift+A", CodeBlock, "code", &[], |w| {
            if let Some((_, body, _)) = code_at(w) {
                select(w, body.start, body.end);
            }
        }),
        a("code.unwrap", "解包为纯文本", "", CodeBlock, "code", &["unwrap", "strip", "fence"], |w| {
            if let Some((block, body, _)) = code_at(w) {
                let text = source(w)[body].to_string();
                edit_range(w, block.start, block.end, &text, block.start);
            }
        }),
        a("code.toInline", "转换为行内代码", "", CodeBlock, "code", &[], |w| {
            if let Some((block, body, _)) = code_at(w) {
                let text = format!("`{}`", source(w)[body].split('\n').collect::<Vec<_>>().join(" "));
                edit_range(w, block.start, block.end, &text, block.start);
            }
        }),
        a("code.pickLanguage", "指定语言", "", CodeBlock, "code", &["language", "lang", "syntax"], |w| crate::shell::pick_code_language(w)),
        // ---- image / link
        a("image.fullscreen", "全屏查看", "Z", Image, "image", &["zoom", "preview"], |w| Modal::toggle(w, Modal::image(w))),
        a("image.copyPath", "复制图片路径", "", Image, "image", &[], |w| {
            let p = image_at(w).map(|i| i.2).unwrap_or_default();
            clipboard::set_text(w.hwnd, &p);
            w.toast(t("路径已复制"));
        }),
        a("image.reveal", "在文件管理器中显示", "", Image, "image", &[], |w| {
            if let (Some((_, _, rel)), Some(dir)) = (image_at(w), w.doc.as_ref().and_then(|d| d.view.doc_dir.clone())) {
                if let Some(p) = crate::editor::images::resolve(Some(&dir), &rel) {
                    dialog::reveal(&p);
                }
            }
        }),
        a("image.delete", "删除图片", "", Image, "image", &[], |w| {
            if let Some((a, b, _)) = image_at(w) {
                edit_range(w, a, b, "", a);
            }
        }),
        a("link.open", "打开链接", "", Link, "link", &[], |w| {
            if let Some(h) = link_at(w) {
                w.follow_link(&h);
            }
        }),
        a("link.copy", "复制链接地址", "", Link, "link", &[], |w| {
            let h = link_at(w).unwrap_or_default();
            clipboard::set_text(w.hwnd, &h);
            w.toast(t("链接已复制"));
        }),
        Action {
            enabled: |w| mdwisp_text::links::unlink_at(source(w), sel(w).head).is_some(),
            ..a("link.remove", "移除链接（保留文字）", "", Link, "link", &[], |w| {
                if let Some((r, text)) = mdwisp_text::links::unlink_at(source(w), sel(w).head).map(|(r, s)| (r, s.to_string())) {
                    edit_range(w, r.start, r.end, &text, r.start);
                }
            })
        },
    ]
});

pub fn find(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

pub fn title(id: &str) -> String {
    find(id).map_or_else(|| id.to_string(), Action::title)
}

/// Whether `action` applies where the caret is right now.
pub fn in_scope(w: &Window, action: &Action) -> bool {
    match action.scope {
        Scope::App => true,
        Scope::Document => w.doc.is_some(),
        Scope::Find => !sel(w).empty(),
        Scope::CodeBlock => in_code(w),
        Scope::Table => in_table(w),
        Scope::Image => image_at(w).is_some(),
        Scope::Link => link_at(w).is_some(),
    }
}

pub fn run(w: &mut Window, id: &str) {
    if let Some(a) = find(id) {
        if (a.enabled)(w) {
            (a.run)(w);
        } else if let Some(r) = (a.reason)(w) {
            w.toast(&r);
        }
    }
}

/// Dispatch a key press to the registry. App actions win over the editor;
/// document actions are blocked while an overlay is open.
pub fn run_shortcut(w: &mut Window, vk: VIRTUAL_KEY, m: Mods) -> bool {
    let Some(name) = keys::key_name(vk) else { return false };
    let hit = ACTIONS.iter().find(|a| {
        (!a.key.is_empty() && keys::matches(a.key, m, name) || a.alt_keys.iter().any(|k| keys::matches(k, m, name))) && in_scope(w, a)
    });
    let Some(a) = hit else { return false };
    let app_level = a.scope == Scope::App;
    if w.modal.is_some() && !app_level {
        return false;
    }
    // A bare `Z` only means "fullscreen" on an image, never while typing.
    if a.id == "image.fullscreen" && !w.image_focused() {
        return false;
    }
    if !(a.enabled)(w) {
        if let Some(r) = (a.reason)(w) {
            w.toast(&r);
        }
        return true;
    }
    (a.run)(w);
    true
}
