//! The modal panels — settings (with shortcuts), export, organize, help and
//! the fullscreen image — sharing one chrome and one form layout: sections,
//! rows with a label and a control, notes, and line lists.

use std::path::PathBuf;
use std::time::Instant;

use mdwisp_core::i18n::{Locale, t, tf};
use mdwisp_core::organize::{OrganizeOptions, OrganizePlan};
use mdwisp_core::settings::{AppSettings, CursorStyle, ImageNaming, MotionLevel, PlainMdImagePolicy, ThemeMode};
use mdwisp_export::{ExportMode, ExportPreview, ImagePolicy, Theme as ExportTheme};
use mdwisp_text::keymap::pretty_key;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::modal::{Modal, panel_scrim};
use super::widgets::{BTN_H, Btn, Field, button, button_width};
use crate::actions::{ACTIONS, GROUPS, Scope};
use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::keys::Mods;
use crate::theme::{self, RADIUS_MD, RADIUS_SM, RADIUS_XL, S1, S2, S3, S4, S6, S8, TEXT_BASE, TEXT_LG, TEXT_SM, TEXT_XS};
use crate::ui::Hit;
use crate::window::Window;
use crate::{app, dialog, win};

// ---- form model -------------------------------------------------------------------

/// Control ids are `id * 16 + option`, so segmented options share one id.
pub enum Ctrl {
    None,
    Seg { id: u32, options: Vec<String>, current: usize },
    Switch { id: u32, on: bool },
    /// A text field kept in `Panel::fields`; `width` 0 grows.
    Field { id: u32, width: f32, unit: String, enabled: bool },
    Buttons(Vec<(u32, String, Btn, bool)>),
}

pub enum Item {
    Section { title: String, aside: Option<(u32, String)>, display: bool },
    Row { name: String, hint: Option<String>, ctrl: Ctrl },
    /// A row whose control takes the full width, with an optional sample line under it.
    Stack { name: String, hint: Option<String>, ctrl: Ctrl, sample: Option<(String, Style, Option<String>)> },
    Note(String),
    Empty(String),
    Steps(Vec<String>),
    /// Monospace numbered lines, with an optional dim reason (`.diff`); `true` strikes
    /// the text through, for lines an export removes.
    Lines(Vec<(String, String, String)>, bool),
    /// Selectable cards (`.modes`): id, name, hint, active.
    Cards(Vec<(u32, String, String, bool)>),
    Pre(String),
    /// Shortcut table rows: title, scope, keys.
    Keys(Vec<(String, String, Vec<String>)>),
    Space(f32),
}

pub enum Kind {
    Settings { section: usize },
    Help,
    Export { mode: ExportMode, preview: Option<Result<ExportPreview, String>>, pending: bool, result: Option<Result<PathBuf, String>> },
    Organize { plan: Option<Result<OrganizePlan, String>>, move_original: bool, download: bool, error: Option<String> },
    Image { path: Option<PathBuf> },
}

pub struct Panel {
    pub kind: Kind,
    pub opened: Instant,
    scroll: f32,
    fields: Vec<(u32, Field)>,
    focus: Option<u32>,
    field_rects: Vec<(u32, Rect)>,
    body: Rect,
    content_h: f32,
    install_result: Option<String>,
}

impl Panel {
    fn new(kind: Kind) -> Panel {
        Panel { kind, opened: Instant::now(), scroll: 0.0, fields: vec![], focus: None, field_rects: vec![], body: Rect::default(), content_h: 0.0, install_result: None }
    }

    pub fn settings(shortcuts: bool) -> Panel {
        let mut p = Panel::new(Kind::Settings { section: if shortcuts { 4 } else { 0 } });
        p.load_fields();
        if shortcuts {
            p.focus = Some(F_SEARCH);
        }
        p
    }

    pub fn help() -> Panel {
        Panel::new(Kind::Help)
    }

    pub fn export(w: &Window) -> Panel {
        let mut p = Panel::new(Kind::Export { mode: ExportMode::PlainMd, preview: None, pending: false, result: None });
        p.start_preview(w);
        p
    }

    pub fn organize(w: &Window) -> Panel {
        let plan = w.doc.as_ref().and_then(|d| d.path().map(|p| (p.to_path_buf(), d.view.buf.text.clone()))).map(|(p, text)| Ok(mdwisp_core::organize::plan_organize(&p, &text)));
        Panel::new(Kind::Organize { plan, move_original: false, download: false, error: None })
    }

    pub fn image(w: &Window) -> Panel {
        let path = crate::actions::image_at(w).and_then(|(_, _, rel)| crate::editor::images::resolve(w.doc.as_ref().and_then(|d| d.view.doc_dir.as_deref()), &rel));
        Panel::new(Kind::Image { path })
    }

    pub fn kind_id(&self) -> u8 {
        match self.kind {
            Kind::Settings { section } => if section == 4 { 1 } else { 0 },
            Kind::Help => 2,
            Kind::Export { .. } => 3,
            Kind::Organize { .. } => 4,
            Kind::Image { .. } => 5,
        }
    }

    pub fn text_input_at(&self, hit: Hit) -> bool {
        matches!(hit, Hit::PanelControl(id) if self.fields.iter().any(|(f, _)| *f * 16 == id))
    }

    pub fn caret(&self) -> Option<(f32, f32, f32)> {
        let id = self.focus?;
        self.fields.iter().find(|(f, _)| *f == id).and_then(|(_, f)| f.caret_box)
    }

    fn field(&mut self, id: u32) -> &mut Field {
        if let Some(i) = self.fields.iter().position(|(f, _)| *f == id) {
            return &mut self.fields[i].1;
        }
        self.fields.push((id, Field::default()));
        &mut self.fields.last_mut().unwrap().1
    }

    /// Text fields mirror settings; they are reloaded when the panel opens or a reset runs.
    fn load_fields(&mut self) {
        let s = app().s().clone();
        let num = |v: f64| if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") };
        let values = [
            (F_FONT_SIZE, num(s.font_size)),
            (F_FONT_UI, s.font_ui.clone().unwrap_or_default()),
            (F_FONT_READ, s.font_read.clone().unwrap_or_default()),
            (F_FONT_DISPLAY, s.font_display.clone().unwrap_or_default()),
            (F_FONT_CODE, s.font_code.clone().unwrap_or_default()),
            (F_CODE_SIZE, num(s.code_font_size)),
            (F_LINE_HEIGHT, num(s.line_height)),
            (F_ACCENT, s.accent_override.clone().unwrap_or_default()),
            (F_AUTOSAVE_DELAY, num(s.auto_save_delay_ms)),
            (F_HISTORY_INTERVAL, num((s.history_interval_ms / 1000.0).round())),
            (F_TAB, num(s.tab_size)),
            (F_MAX_WIDTH, num(s.image_max_width)),
        ];
        for (id, v) in values {
            *self.field(id) = Field::new(&v);
        }
    }

    fn start_preview(&mut self, w: &Window) {
        let Kind::Export { mode, preview, pending, result } = &mut self.kind else { return };
        *result = None;
        let Some((path, text, in_folder)) = w.doc.as_ref().and_then(|d| d.meta.as_ref().map(|m| (m.path.clone(), d.view.buf.text.clone(), m.in_folder))) else { return };
        // ZIP packs the document's folder; a loose file sits in a folder that is not its own.
        if *mode == ExportMode::Zip && !in_folder {
            *preview = None;
            return;
        }
        *pending = true;
        let mode = *mode;
        let s = app().s().clone();
        let hwnd = w.hwnd.0 as isize;
        // Previews read and encode images and fonts; keep that off the UI thread.
        std::thread::spawn(move || {
            let r = run_preview(mode, &path, &text, &s).map_err(|e| e.to_string());
            win::post(hwnd, move |w| {
                if let Some(Modal::Panel(Panel { kind: Kind::Export { mode: m, preview, pending, .. }, .. })) = &mut w.modal {
                    if *m == mode {
                        *preview = Some(r);
                        *pending = false;
                    }
                }
                win::request_frame(w.hwnd);
            });
        });
    }
}

fn export_theme(s: &AppSettings) -> ExportTheme {
    if s.theme == ThemeMode::Light { ExportTheme::Light } else { ExportTheme::Dark }
}

fn image_policy(s: &AppSettings) -> ImagePolicy {
    match s.plain_md_image_policy {
        PlainMdImagePolicy::Drop => ImagePolicy::Drop,
        PlainMdImagePolicy::AltPlaceholder => ImagePolicy::AltPlaceholder,
        PlainMdImagePolicy::EmptyRef => ImagePolicy::EmptyRef,
    }
}

fn run_preview(mode: ExportMode, path: &std::path::Path, text: &str, s: &AppSettings) -> std::io::Result<ExportPreview> {
    match mode {
        ExportMode::PlainMd => Ok(mdwisp_export::preview_plain_md(path, text, image_policy(s))),
        ExportMode::Zip => mdwisp_export::preview_zip(path),
        ExportMode::Html => mdwisp_export::preview_html(path, text, export_theme(s)),
        ExportMode::Pdf => mdwisp_export::preview_pdf(path, text, export_theme(s)),
    }
}

// ---- field ids ----------------------------------------------------------------------

const F_FONT_SIZE: u32 = 1;
const F_FONT_UI: u32 = 2;
const F_FONT_READ: u32 = 3;
const F_FONT_DISPLAY: u32 = 4;
const F_FONT_CODE: u32 = 5;
const F_CODE_SIZE: u32 = 6;
const F_LINE_HEIGHT: u32 = 7;
const F_ACCENT: u32 = 8;
const F_AUTOSAVE_DELAY: u32 = 9;
const F_HISTORY_INTERVAL: u32 = 10;
const F_TAB: u32 = 11;
const F_MAX_WIDTH: u32 = 12;
const F_SEARCH: u32 = 13;

// Other controls.
const C_LANGUAGE: u32 = 20;
const C_THEME: u32 = 21;
const C_MOTION: u32 = 22;
const C_CURSOR: u32 = 23;
const C_AUTOSAVE: u32 = 24;
const C_HISTORY: u32 = 25;
const C_DEFAULT_APP: u32 = 26;
const C_POLICY: u32 = 27;
const C_GPU: u32 = 28;
const C_TOGGLES: u32 = 30; // 30..=37: editor switches
const C_NAMING: u32 = 40;
const C_DEDUPE: u32 = 41;
const C_RESET: u32 = 42;
const C_CATEGORY: u32 = 43;
const C_MODE: u32 = 50; // 50..=53
const C_EXPORT: u32 = 54;
const C_ORGANIZE_LINK: u32 = 55;
const C_MOVE: u32 = 56;
const C_DOWNLOAD: u32 = 57;
const C_ORGANIZE: u32 = 58;
const C_WELCOME: u32 = 60;
const C_REVEAL_WELCOME: u32 = 61;
const C_COPY_PLUGIN: u32 = 62;
const C_INSTALL_SKILLS: u32 = 63;
const C_COPY_SKILL: u32 = 64; // 64..
const C_NAV: u32 = 80; // 80..=84 settings sections

const SECTIONS: [&str; 5] = ["外观与字体", "文档与保存", "编辑器", "图片", "快捷键"];
const NAV_STYLE: Style = Style::new(Stack::Ui, TEXT_BASE);
/// The nav row's inset plus the label's inset inside it, both sides.
const NAV_PAD: f32 = S2 * 2.0 + S3 * 2.0;

fn opts(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|l| t(l).to_string()).collect()
}

fn editor_toggles(s: &AppSettings) -> [(&'static str, &'static str, bool); 8] {
    [
        ("打字机模式", "光标恒定居中，视线不用跟着走", s.typewriter_mode),
        ("高亮当前行", "在当前行背后画一条极浅的色带", s.highlight_current_line),
        ("只读模式", "禁止修改文档内容，仅用于阅读", s.read_only),
        ("显示侧栏", "窄窗口下会自动收起，避免挤占正文", s.sidebar_visible),
        ("显示大纲", "左侧按 H1–H6 组织标题，支持折叠与跳转", s.outline_visible),
        ("拼写检查", "浏览器的拼写波浪线", s.spell_check),
        ("自动配对", "输入括号引号时补上另一半", s.auto_pair),
        ("智能列表", "回车自动延续列表与引用前缀", s.smart_lists),
    ]
}

fn scope_label(s: Scope) -> &'static str {
    t(match s {
        Scope::App => "全局",
        Scope::Document => "文档",
        Scope::Find => "查找",
        Scope::CodeBlock => "代码块内",
        Scope::Table => "表格内",
        Scope::Image => "图片上",
        Scope::Link => "链接上",
    })
}

fn field(id: u32, width: f32, unit: &str) -> Ctrl {
    Ctrl::Field { id, width, unit: unit.into(), enabled: true }
}

fn settings_items(p: &Panel, w: &Window, section: usize) -> Vec<Item> {
    let s = app().s().clone();
    let reset = |title: &str| Item::Section { title: t(title).into(), aside: Some((C_RESET, t("恢复默认").into())), display: true };
    let row = |name: &str, hint: &str, ctrl: Ctrl| Item::Row { name: t(name).into(), hint: (!hint.is_empty()).then(|| t(hint).into()), ctrl };
    match section {
        0 => {
            let font = |name: &str, id: u32, stack: Stack, sample: &str, family: &Option<String>| Item::Stack {
                name: t(name).into(),
                hint: Some(t("留空则使用内置字体栈").into()),
                ctrl: field(id, 0.0, ""),
                sample: Some((t(sample).into(), Style::new(stack, TEXT_BASE), family.clone())),
            };
            vec![
                reset("外观"),
                Item::Row { name: if s.language == Locale::En { "Language".into() } else { "语言/Language".into() }, hint: Some(t("立即切换，并在下次启动时保留").into()), ctrl: Ctrl::Seg { id: C_LANGUAGE, options: vec!["中文/Chinese".into(), "English".into()], current: (s.language == Locale::En) as usize } },
                row("主题", "深色为基础主题，浅色由同一色相提亮推导", Ctrl::Seg { id: C_THEME, options: opts(&["深色", "浅色", "跟随系统"]), current: s.theme as usize }),
                Item::Row {
                    name: t("动效强度").into(),
                    hint: Some(t(["入场、反馈动效全部保留", "只保留表达状态变化的反馈动效", "不做过渡，但状态切换仍然可见"][s.motion as usize]).into()),
                    ctrl: Ctrl::Seg { id: C_MOTION, options: opts(&["完整", "精简", "关闭动效"]), current: s.motion as usize },
                },
                row("硬件加速", "用显卡绘制界面；显示异常时关闭，改用处理器绘制", Ctrl::Switch { id: C_GPU, on: s.hardware_acceleration }),
                row("正文字号", "阅读与编辑共用的基础字号", field(F_FONT_SIZE, 68.0, "px")),
                font("界面字体", F_FONT_UI, Stack::Ui, "菜单 · 面板 · 按钮 Aa 汉字", &s.font_ui),
                font("正文字体", F_FONT_READ, Stack::Read, "正文阅读效果 Aa 汉字排版", &s.font_read),
                font("装饰标题字体", F_FONT_DISPLAY, Stack::Display, "章节标题 · 写作与阅读", &s.font_display),
                font("代码字体", F_FONT_CODE, Stack::Code, "const value = 42 // 等宽 Aa", &s.font_code),
                row("代码字号", "只影响代码块，正文保持自己的字号", field(F_CODE_SIZE, 68.0, "px")),
                row("行高", "正文的行间距倍数，编辑与阅读同步", field(F_LINE_HEIGHT, 68.0, t("倍"))),
                row("光标样式", "编辑时插入符的形状", Ctrl::Seg { id: C_CURSOR, options: opts(&["竖线", "方块", "下划线"]), current: s.cursor_style as usize }),
                row("强调色", "留空使用内置陶土橙", field(F_ACCENT, 108.0, "")),
            ]
        }
        1 => vec![
            reset("文档"),
            Item::Note(t("新文档先在内存中编辑；首次保存才选择位置。取消保存会保留草稿。").into()),
            row("自动保存", "首次保存并选择位置后，停止输入时自动写盘", Ctrl::Switch { id: C_AUTOSAVE, on: s.auto_save }),
            row("自动保存延迟", "最后一次输入到写盘的等待时间", Ctrl::Field { id: F_AUTOSAVE_DELAY, width: 68.0, unit: "ms".into(), enabled: s.auto_save }),
            row("版本历史", "首次保存后按间隔留下快照，未命名草稿不记录", Ctrl::Switch { id: C_HISTORY, on: s.history_enabled }),
            row("快照间隔", "两次自动快照之间的最短间隔", Ctrl::Field { id: F_HISTORY_INTERVAL, width: 68.0, unit: t("秒").into(), enabled: s.history_enabled }),
            row("默认打开方式", "Windows 不允许应用自行接管文件类型：在系统设置里搜索 .md，选择 MDWisp", Ctrl::Buttons(vec![(C_DEFAULT_APP, t("打开系统设置").into(), Btn::Normal, true)])),
            Item::Row {
                name: t("导出纯 MD 时的图片").into(),
                hint: Some(t(["整行删除", "图片变成 *[图：alt]* 形式的斜体说明", "留下 ![]()，之后可重新插图"][match s.plain_md_image_policy {
                    PlainMdImagePolicy::Drop => 0,
                    PlainMdImagePolicy::AltPlaceholder => 1,
                    PlainMdImagePolicy::EmptyRef => 2,
                }])
                .into()),
                ctrl: Ctrl::Seg {
                    id: C_POLICY,
                    options: opts(&["保留占位", "整行删除", "保留空引用"]),
                    current: match s.plain_md_image_policy {
                        PlainMdImagePolicy::AltPlaceholder => 0,
                        PlainMdImagePolicy::Drop => 1,
                        PlainMdImagePolicy::EmptyRef => 2,
                    },
                },
            },
        ],
        2 => {
            let mut v = vec![reset("编辑器")];
            for (i, (name, hint, on)) in editor_toggles(&s).iter().enumerate() {
                v.push(row(name, hint, Ctrl::Switch { id: C_TOGGLES + i as u32, on: *on }));
            }
            v.push(row("Tab 宽度", "", field(F_TAB, 68.0, t("空格"))));
            v
        }
        3 => vec![
            reset("图片"),
            Item::Row {
                name: t("文件命名").into(),
                hint: Some(t(["默认。同日多图不会互相覆盖，原名仍可读", "同名文件加序号，方便对照原图", "名字最短，但看不出图片是什么"][s.image_naming as usize]).into()),
                ctrl: Ctrl::Seg { id: C_NAMING, options: opts(&["日期-哈希-原名", "保留原名", "仅哈希"]), current: s.image_naming as usize },
            },
            row("重复图片去重", "内容相同的图片只存一份，复用已有文件", Ctrl::Switch { id: C_DEDUPE, on: s.image_dedupe }),
            row("最大宽度", "超过这个宽度会缩放后写入", field(F_MAX_WIDTH, 68.0, t("px，0 = 不限制"))),
        ],
        _ => {
            let q = p.fields.iter().find(|(id, _)| *id == F_SEARCH).map_or(String::new(), |(_, f)| f.text.trim().to_lowercase());
            let category = p.fields.iter().find(|(id, _)| *id == C_CATEGORY).map_or(String::new(), |(_, f)| f.text.clone());
            let mut v = vec![
                Item::Section { title: t("快捷键").into(), aside: Some((C_CATEGORY, if category.is_empty() { t("全部功能").into() } else { GROUPS.iter().find(|(g, _)| *g == category).map_or(category.clone(), |(_, l)| t(l).into()) })), display: true },
                Item::Stack { name: String::new(), hint: None, ctrl: field(F_SEARCH, 0.0, ""), sample: None },
                Item::Space(S2),
            ];
            let mut total = 0;
            for (g, label) in GROUPS {
                if !category.is_empty() && category != g {
                    continue;
                }
                let rows: Vec<(String, String, Vec<String>)> = ACTIONS
                    .iter()
                    .filter(|a| a.group == g)
                    .filter(|a| q.is_empty() || a.title().to_lowercase().contains(&q) || a.key.to_lowercase().contains(&q) || a.keywords.iter().any(|k| k.contains(&q)))
                    .map(|a| {
                        let mut title = a.title();
                        if !(a.enabled)(w) {
                            if let Some(r) = (a.reason)(w) {
                                title = format!("{title} · {r}");
                            }
                        }
                        let keys = if a.key.is_empty() { vec![] } else { std::iter::once(a.key).chain(a.alt_keys.iter().copied()).map(pretty_key).collect() };
                        (title, scope_label(a.scope).to_string(), keys)
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                total += rows.len();
                v.push(Item::Section { title: t(label).into(), aside: Some((0, rows.len().to_string())), display: true });
                v.push(Item::Keys(rows));
            }
            if total == 0 {
                v.push(Item::Empty(t("没有匹配的命令").into()));
            }
            v.push(Item::Note(format!("{}    {}", tf("共 {0} 条", &[&total]), t("表格、代码和图片快捷键需先聚焦对应内容；没有按键的可用命令在 Ctrl+P 中执行"))));
            v
        }
    }
}

fn help_items(p: &Panel) -> Vec<Item> {
    let welcome = welcome_stock().is_some();
    let mut v = vec![
        Item::Section { title: t("从写作到保存").into(), aside: None, display: false },
        Item::Steps(
            [
                "按住 Ctrl 点击链接，可打开网址、其他 Markdown 文档或文内标题；macOS 使用 Cmd。",
                "首次启动即可在空白文档输入。点击「新建文档」或按 Ctrl+N 开始另一篇。",
                "按 Ctrl+S 选择文件名与位置。取消对话框会保留内容，未命名时不自动保存、不记录快照。",
                "首次保存成功后，按设置启用自动保存与版本历史；关闭自动保存时用 Ctrl+S 手动保存。",
                "插图前先保存文档。图片存放在文档旁，分享时携带文档及其图片文件夹。",
                "新建、打开其他文件或退出时，未保存修改可选择保存、不保存或取消；保存失败会保留当前文档。",
                "「设置」独立管理外观、文档、编辑器和图片；快捷键在其中按功能分类。F1 可直接进入快捷键页。",
                "正文和图片随窗口伸缩；拖动正文两侧细线分别调整边距，双击恢复默认。导出快捷键 Ctrl+Shift+E 在其他面板中仍可用。",
            ]
            .iter()
            .map(|s| t(s).to_string())
            .collect(),
        ),
        Item::Section { title: t("入门").into(), aside: None, display: false },
        Item::Stack {
            name: t("欢迎使用 MDWisp").into(),
            hint: Some(t("一篇用本应用写成的说明文档，包含图片、表格、代码检测和脚注的实际效果。 它同时演示了这个项目最重要的设计：文档和它的图片放在同一个文件夹里， 整个文件夹拷给别人就能正常显示。").into()),
            ctrl: Ctrl::Buttons(vec![(C_WELCOME, t("打开欢迎文档").into(), Btn::Primary, welcome), (C_REVEAL_WELCOME, t("在文件夹中显示").into(), Btn::Normal, welcome)]),
            sample: None,
        },
    ];
    if !welcome {
        v.push(Item::Note(t("这个构建没有包含欢迎文档").into()));
    }
    v.push(Item::Section { title: t("给 AI 的协作规则").into(), aside: None, display: false });
    v.push(Item::Note(t("让 AI 直接按文档文件夹格式产出文档：图片存进文档自己的图片文件夹，用相对路径引用， 可以要求不生成图片或尽量生成图片。").into()));
    v.push(Item::Row {
        name: t("Claude Code 插件").into(),
        hint: Some(format!("{}/mdwisp:doc {}", t("复制后粘进 Claude Code 执行，之后用"), t("写文档，或直接让它写一份文档"))),
        ctrl: Ctrl::Buttons(vec![(C_COPY_PLUGIN, t("复制安装命令").into(), Btn::Normal, true)]),
    });
    v.push(Item::Row {
        name: "Codex / Cursor".into(),
        hint: Some(format!("{}$mdwisp-doc{}/mdwisp-doc。", t("选择项目文件夹，安装共用技能与校验脚本。Codex 使用"), t("，Cursor 使用"))),
        ctrl: Ctrl::Buttons(vec![(C_INSTALL_SKILLS, t("安装到项目").into(), Btn::Normal, true)]),
    });
    if let Some(r) = &p.install_result {
        v.push(Item::Note(r.clone()));
    }
    for (i, (title, summary, _)) in skills().iter().enumerate() {
        v.push(Item::Row { name: t(title).into(), hint: Some(t(summary).into()), ctrl: Ctrl::Buttons(vec![(C_COPY_SKILL + i as u32, t("复制").into(), Btn::Normal, true)]) });
    }
    v.push(Item::Note(format!("{}.agents/skills/{}", t("Codex 与 Cursor 共用项目下的"), t("，规则与 Claude 插件来自同一份源文件。 安装不会修改已有的 AGENTS.md 或 Cursor 规则；也可以复制上面的内容交给其他 AI 助手。"))));
    v.push(Item::Section { title: t("数据位置").into(), aside: None, display: false });
    v.push(Item::Row { name: t("设置与会话").into(), hint: Some("%APPDATA%/mdview/".into()), ctrl: Ctrl::None });
    v.push(Item::Row { name: t("历史版本").into(), hint: Some(t("%APPDATA%/mdview/history/ — 按文档分目录，按内容哈希去重").into()), ctrl: Ctrl::None });
    v.push(Item::Note(t("文件本身永远不会离开你打开的文件夹。历史版本存在应用数据目录下， 这样你发给别人的文件夹里不会多出一个 .history 目录。").into()));
    v
}

/// The bundled skills, read from the plugin — the one text source of the rules.
fn skills() -> Vec<(&'static str, &'static str, PathBuf)> {
    let Some(root) = crate::resource_dir("plugin") else { return vec![] };
    let en = mdwisp_core::i18n::current_language() == Locale::En;
    let file = |name: &str| root.join("mdwisp/skills").join(name).join(if en { "SKILL.en.md" } else { "SKILL.md" });
    vec![
        ("按文档文件夹格式写文档", "让 AI 把文档写成自带图片的文件夹：图片存进 _img 目录、相对路径引用，可选不生成图片或尽量生成图片。", file("doc")),
        ("导出可分享的单文件文档", "剥离本地图片与本地路径，保留远程链接、内嵌图片和代码块原文，逐行处理并在写出前预览。", file("share")),
    ]
}

fn welcome_stock() -> Option<PathBuf> {
    let root = crate::resource_dir("stock")?;
    let rel = if mdwisp_core::i18n::current_language() == Locale::En { "Welcome/Welcome.md" } else { "欢迎使用/欢迎使用.md" };
    let p = root.join(rel);
    p.exists().then_some(p)
}

fn export_items(p: &Panel, w: &Window) -> Vec<Item> {
    let Kind::Export { mode, preview, pending, .. } = &p.kind else { return vec![] };
    let Some(meta) = w.doc.as_ref().and_then(|d| d.meta.as_ref()) else {
        return vec![Item::Empty(t("请先保存文档，确定文档及图片的位置后再导出。").into())];
    };
    let modes = [
        (ExportMode::PlainMd, t("纯 Markdown"), t("剥离图片与本地路径，得到一份可以直接粘贴的文本")),
        (ExportMode::Zip, t("打包 ZIP"), t("压缩整个文档文件夹里已保存的内容，链接原样不动")),
        (ExportMode::Html, t("单文件 HTML"), t("图片转 base64 内嵌，可直接发邮件")),
        (ExportMode::Pdf, "PDF", t("A4 分页排版，图片内嵌")),
    ];
    let mut v = vec![Item::Cards(modes.iter().enumerate().map(|(i, (m, n, h))| (C_MODE + i as u32, n.to_string(), h.to_string(), m == mode)).collect())];
    let needs_folder = *mode == ExportMode::Zip && !meta.in_folder;
    let plain = *mode == ExportMode::PlainMd;
    if needs_folder {
        v.push(Item::Note(t("这是一份散装文件，所在文件夹里还有别的东西，不能整个打包。").into()));
        v.push(Item::Stack { name: String::new(), hint: None, ctrl: Ctrl::Buttons(vec![(C_ORGANIZE_LINK, t("先整理为文档文件夹").into(), Btn::Normal, true)]), sample: None });
        return v;
    }
    let shown = match preview {
        Some(Ok(pv)) if pv.mode == *mode => Some(pv),
        _ => None,
    };
    if let Some(Err(e)) = preview {
        v.push(Item::Note(tf("导出失败：{0}", &[e])));
    }
    if let (Some(pv), false) = (shown, plain) {
        v.push(Item::Note(pv.output.clone()));
    }
    if plain || shown.is_some_and(|pv| !pv.removals.is_empty()) {
        let count = if *pending { t("计算中…").to_string() } else { format!("{} {}", shown.map_or(0, |pv| pv.removals.len()), if plain { t("行") } else { t("项") }) };
        v.push(Item::Section { title: if plain { t("将要移除的内容") } else { t("未包含的内容") }.into(), aside: Some((0, count)), display: false });
        match shown {
            Some(pv) if pv.removals.is_empty() => v.push(Item::Empty(t("这份文档不含图片和本地路径，导出后内容完全一致").into())),
            Some(pv) => v.push(Item::Lines(pv.removals.iter().map(|r| (r.line.to_string(), if r.text.is_empty() { " ".into() } else { r.text.clone() }, format!("— {}", r.reason))).collect(), true)),
            None => {}
        }
    }
    v.push(Item::Note(format!("{}{}", t("写出目标："), shown.map_or("—".into(), |pv| pv.target_path.display().to_string()))));
    v
}

fn organize_items(p: &Panel, w: &Window) -> Vec<Item> {
    let Kind::Organize { plan, move_original, download, .. } = &p.kind else { return vec![] };
    let Some(d) = &w.doc else { return vec![] };
    let Some(meta) = &d.meta else { return vec![Item::Empty(t("请先保存文档").into())] };
    let plan = match plan {
        Some(Ok(p)) => p,
        Some(Err(e)) => return vec![Item::Empty(e.clone())],
        None => return vec![],
    };
    if let Some(b) = &plan.blocked {
        return vec![Item::Empty(b.clone())];
    }
    let stem = &meta.stem;
    let count = plan.images.len() + if *download { plan.remote.len() } else { 0 };
    let mut v = vec![
        Item::Note(t("文档和它的图片放进同一个文件夹，链接改成相对路径。之后把这个文件夹整个发出去，图片不会断。").into()),
        Item::Pre(format!("{stem}/\n├── {stem}.md\n└── {stem}_img/{}", tf("    {0} 张图片", &[&count]))),
        Item::Note(format!("{}{}", t("位置："), plan.target_dir.display())),
        Item::Row {
            name: t("原文件").into(),
            hint: Some(if *move_original { t("原 md 移入回收站；同名 _img 文件夹里的图片全部带走时一并移入") } else { t("原文件原样保留，新文件夹是一份副本") }.into()),
            ctrl: Ctrl::Seg { id: C_MOVE, options: opts(&["保留", "移入回收站"]), current: *move_original as usize },
        },
    ];
    if !plan.remote.is_empty() {
        v.push(Item::Row { name: t("下载网络图片").into(), hint: Some(format!("{} {}", plan.remote.len(), t("张图片引用的是网址；下载后离线也能看"))), ctrl: Ctrl::Switch { id: C_DOWNLOAD, on: *download } });
    }
    if !plan.images.is_empty() {
        v.push(Item::Section { title: t("将复制的图片").into(), aside: Some((0, format!("{} {}", plan.images.len(), t("张")))), display: false });
        v.push(Item::Lines(
            plan.images
                .iter()
                .enumerate()
                .map(|(i, img)| {
                    let base = img.from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let reason = if img.name != base { format!("{}{base}", t("— 重名，原名")) } else { String::new() };
                    ((i + 1).to_string(), img.name.clone(), reason)
                })
                .collect(),
            false,
        ));
    }
    if !plan.missing.is_empty() {
        v.push(Item::Section { title: t("找不到的图片").into(), aside: Some((0, format!("{} {}", plan.missing.len(), t("张 · 链接保持原样")))), display: false });
        v.push(Item::Lines(plan.missing.iter().enumerate().map(|(i, l)| ((i + 1).to_string(), l.clone(), String::new())).collect(), false));
    }
    v
}

// ---- layout + painting ----------------------------------------------------------------

struct Painter<'a> {
    g: &'a Gfx,
    w: f32,
    x: f32,
    y: f32,
    paint: bool,
    hits: Vec<(Rect, Hit)>,
    fields: Vec<(u32, Rect, Ctrl)>,
}

impl Painter<'_> {
    fn text(&self, s: &str, style: Style, w: f32) -> windows::Win32::Graphics::DirectWrite::IDWriteTextLayout {
        self.g.layout(s, style, w.max(1.0))
    }
}

/// Lay out (and, when `paint`, draw) the items; returns the content height.
fn layout(ui: &mut crate::ui::Ui, g: &Gfx, items: &[Item], x: f32, y: f32, w: f32, paint: bool, fields: &mut Vec<(u32, Rect)>) -> f32 {
    let th = app().theme;
    let mut p = Painter { g, w, x, y, paint, hits: vec![], fields: vec![] };
    let sm = Style::new(Stack::Ui, TEXT_SM);
    let xs = Style::new(Stack::Ui, TEXT_XS);
    for (n, item) in items.iter().enumerate() {
        match item {
            Item::Section { title, aside, display } => {
                if n > 0 {
                    p.y += S8;
                }
                let style = if *display { Style::new(Stack::Display, TEXT_BASE).weight(700) } else { Style::new(Stack::Ui, TEXT_SM).weight(700) };
                let l = p.text(title, style, p.w);
                let m = metrics(&l);
                if p.paint {
                    g.text(&l, p.x, p.y, th.text_primary);
                }
                if let Some((id, label)) = aside.as_ref().filter(|(id, _)| *id == C_CATEGORY) {
                    let al = p.text(label, Style::new(Stack::Ui, TEXT_SM).weight(600), 400.0);
                    let am = metrics(&al);
                    let bw = am.w + S3 * 2.0 + 14.0;
                    let ar = Rect::new(p.x + p.w - bw, p.y + (m.h - am.h - S1 * 2.0) / 2.0, bw, am.h + S1 * 2.0);
                    if p.paint {
                        let hit = Hit::PanelControl(id * 16);
                        g.fill_round(ar, RADIUS_SM, th.bg_inset);
                        g.stroke_round(ar, RADIUS_SM, theme::mix(th.accent_line, th.border_soft, ui.hover_t(hit)), 1.0);
                        g.text(&al, ar.x + S3, ar.y + S1, th.text_primary);
                        let (cx, cy) = (ar.right() - S3 - 3.0, ar.y + ar.h / 2.0);
                        g.line(cx - 3.5, cy - 1.8, cx, cy + 1.8, th.text_secondary, 1.4);
                        g.line(cx, cy + 1.8, cx + 3.5, cy - 1.8, th.text_secondary, 1.4);
                        p.hits.push((ar, hit));
                    }
                } else if let Some((id, label)) = aside {
                    let al = p.text(label, xs, 400.0);
                    let am = metrics(&al);
                    let ar = Rect::new(p.x + p.w - am.w, p.y + m.h - am.h, am.w, am.h);
                    if p.paint {
                        let hit = Hit::PanelControl(id * 16);
                        let c = if *id == 0 { th.text_disabled } else { theme::mix(th.accent, th.text_disabled, ui.hover_t(hit)) };
                        g.text(&al, ar.x, ar.y, c);
                        if *id != 0 {
                            p.hits.push((ar, hit));
                        }
                    }
                }
                p.y += m.h + S2;
                if p.paint {
                    g.fill(Rect::new(p.x, p.y, p.w, 1.0), th.border_soft);
                }
                p.y += 1.0 + S4;
            }
            Item::Row { name, hint, ctrl } | Item::Stack { name, hint, ctrl, .. } => {
                let stack = matches!(item, Item::Stack { .. });
                let ctrl_w = ctrl_width(g, ctrl, p.w);
                let label_w = if stack { p.w } else { (p.w - ctrl_w - S6).max(80.0) };
                let nl = (!name.is_empty()).then(|| p.text(name, sm, label_w));
                let hl = hint.as_ref().map(|h| {
                    let l = p.text(h, xs, label_w);
                    unsafe {
                        let _ = l.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_XS * 1.6, TEXT_XS * 1.25);
                    }
                    l
                });
                let nh = nl.as_ref().map_or(0.0, |l| metrics(l).h);
                let hh = hl.as_ref().map_or(0.0, |l| metrics(l).h + 2.0);
                let label_h = nh + hh;
                let ctrl_h = ctrl_height(ctrl);
                let top = p.y + S3;
                let h = if stack { label_h + if label_h > 0.0 { S2 } else { 0.0 } + ctrl_h } else { label_h.max(ctrl_h) };
                if p.paint {
                    let ly = if stack { top } else { top + (h - label_h) / 2.0 };
                    if let Some(l) = &nl {
                        g.text(l, p.x, ly, th.text_primary);
                    }
                    if let Some(l) = &hl {
                        g.text(l, p.x, ly + nh + 2.0, th.text_disabled);
                    }
                    let cr = if stack {
                        Rect::new(p.x, top + label_h + if label_h > 0.0 { S2 } else { 0.0 }, p.w, ctrl_h)
                    } else {
                        Rect::new(p.x + p.w - ctrl_w, top + (h - ctrl_h) / 2.0, ctrl_w, ctrl_h)
                    };
                    paint_ctrl(ui, g, ctrl, cr, &mut p.hits, &mut p.fields);
                }
                p.y = top + h + S3;
                if let Item::Stack { sample: Some((text, style, family)), .. } = item {
                    let l = g.layout_with(text, Style { size: TEXT_SM, ..*style }, p.w - S3 * 2.0, family.as_deref());
                    let h = metrics(&l).h + S2 * 2.0;
                    if p.paint {
                        let r = Rect::new(p.x, p.y - S1, p.w, h);
                        g.fill_round(r, RADIUS_SM, th.bg_inset);
                        g.stroke_round(r, RADIUS_SM, th.border_soft, 1.0);
                        g.clip(r);
                        g.text(&l, r.x + S3, r.y + S2, th.text_secondary);
                        g.unclip();
                    }
                    p.y += h + S2;
                }
                let last = n + 1 == items.len() || !matches!(items[n + 1], Item::Row { .. } | Item::Stack { .. });
                if p.paint && !last {
                    g.fill(Rect::new(p.x, p.y - 1.0, p.w, 1.0), theme::with_alpha(th.border_soft, 0.55));
                }
            }
            Item::Note(s) => {
                let l = p.text(s, xs, p.w);
                unsafe {
                    let _ = l.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_XS * 1.7, TEXT_XS * 1.3);
                }
                p.y += S3;
                if p.paint {
                    g.text(&l, p.x, p.y, th.text_tertiary);
                }
                p.y += metrics(&l).h + S3;
            }
            Item::Empty(s) => {
                let l = p.text(s, sm, p.w);
                unsafe {
                    let _ = l.SetTextAlignment(windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER);
                }
                p.y += S8;
                if p.paint {
                    g.text(&l, p.x, p.y, th.text_tertiary);
                }
                p.y += metrics(&l).h + S8;
            }
            Item::Steps(steps) => {
                for (i, s) in steps.iter().enumerate() {
                    // `.help__steps` inherits the panel's base size, at line-height 1.9.
                    let base = Style::new(Stack::Ui, TEXT_BASE);
                    let num = p.text(&format!("{}.", i + 1), base, 40.0);
                    let l = p.text(s, base, p.w - S6);
                    unsafe {
                        let _ = l.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_BASE * 1.9, TEXT_BASE * 1.4);
                        let _ = num.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_BASE * 1.9, TEXT_BASE * 1.4);
                    }
                    if p.paint {
                        g.text(&num, p.x + S6 - metrics(&num).w - S2, p.y, th.text_secondary);
                        g.text(&l, p.x + S6, p.y, th.text_secondary);
                    }
                    p.y += metrics(&l).h + if i + 1 < steps.len() { S3 } else { 0.0 };
                }
            }
            Item::Lines(lines, strike) => {
                let code = Style::new(Stack::Code, TEXT_XS);
                let top = p.y + S2;
                let mut y = top;
                for (no, text, reason) in lines {
                    let tl = g.layout(&format!("{text}{}{reason}", if reason.is_empty() { "" } else { "  " }), code, p.w - S3 * 2.0 - 48.0 - S3);
                    unsafe {
                        let _ = tl.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_XS * 1.7, TEXT_XS * 1.3);
                        let len = text.encode_utf16().count() as u32;
                        if !reason.is_empty() {
                            let _ = tl.SetDrawingEffect(&g.solid(th.text_disabled), windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE { startPosition: len, length: 10000 });
                            let _ = tl.SetFontStyle(windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_ITALIC, windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE { startPosition: len, length: 10000 });
                        }
                    }
                    let h = metrics(&tl).h + S1 * 2.0;
                    if p.paint {
                        let nl = g.layout(no, code, 48.0);
                        g.text(&nl, p.x + S3 + 48.0 - metrics(&nl).w, y + S1, th.text_disabled);
                        let tx = p.x + S3 + 48.0 + S3;
                        g.text(&tl, tx, y + S1, th.text_tertiary);
                        if *strike {
                            // A line-through in the danger colour at 55%, under the text only.
                            let len = text.encode_utf16().count() as u32;
                            for r in crate::editor::view::range_rects(&tl, 0, len) {
                                let ly = y + S1 + r.y + r.h * 0.55;
                                g.line(tx + r.x, ly, tx + r.x + r.w, ly, theme::with_alpha(th.danger, 0.55), 1.0);
                            }
                        }
                        g.fill(Rect::new(p.x, y + h - 1.0, p.w, 1.0), theme::with_alpha(th.border_soft, 0.45));
                    }
                    y += h;
                }
                if p.paint {
                    g.stroke_round(Rect::new(p.x, top, p.w, y - top), RADIUS_MD, th.border_soft, 1.0);
                }
                p.y = y + S2;
            }
            Item::Cards(cards) => {
                let cols = ((p.w + S3) / (180.0 + S3)).floor().clamp(1.0, cards.len() as f32) as usize;
                let cw = (p.w - S3 * (cols as f32 - 1.0)) / cols as f32;
                let mut row_h = 0.0f32;
                let rows = cards.chunks(cols).collect::<Vec<_>>();
                for chunk in rows {
                    row_h = 0.0;
                    for c in chunk {
                        let hl = g.layout(&c.2, xs, cw - S4 * 2.0);
                        row_h = row_h.max(TEXT_SM * 1.5 + S2 + metrics(&hl).h + S4 * 2.0);
                    }
                    for (i, (id, name, hint, on)) in chunk.iter().enumerate() {
                        let r = Rect::new(p.x + (cw + S3) * i as f32, p.y, cw, row_h);
                        let hit = Hit::PanelControl(id * 16);
                        if p.paint {
                            let hv = ui.hover_t(hit);
                            let r = Rect { y: r.y - hv, ..r };
                            g.fill_round(r, RADIUS_MD, if *on { th.accent_soft } else { th.bg_inset });
                            g.stroke_round(r, RADIUS_MD, if *on { th.accent } else { theme::mix(th.accent_line, th.border_soft, hv) }, 1.0);
                            let nl = g.layout(name, sm.weight(600), r.w - S4 * 2.0);
                            g.text(&nl, r.x + S4, r.y + S4, th.text_primary);
                            let hl = g.layout(hint, xs, r.w - S4 * 2.0);
                            unsafe {
                                let _ = hl.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_XS * 1.6, TEXT_XS * 1.25);
                            }
                            g.text(&hl, r.x + S4, r.y + S4 + metrics(&nl).h + S2, th.text_tertiary);
                        }
                        p.hits.push((r, hit));
                    }
                    p.y += row_h + S3;
                }
                let _ = row_h;
            }
            Item::Pre(s) => {
                let l = g.layout(s, Style::new(Stack::Code, TEXT_SM), p.w - S4 * 2.0);
                unsafe {
                    let _ = l.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_SM * 1.7, TEXT_SM * 1.3);
                }
                let h = metrics(&l).h + S3 * 2.0;
                p.y += S2;
                if p.paint {
                    g.fill_round(Rect::new(p.x, p.y, p.w, h), RADIUS_SM, th.bg_inset);
                    g.text(&l, p.x + S4, p.y + S3, th.text_secondary);
                }
                p.y += h + S2;
            }
            Item::Keys(rows) => {
                for (title, scope, keys) in rows {
                    let tl = g.layout(title, sm, p.w * 0.46 - S2 * 2.0);
                    let h = metrics(&tl).h.max(TEXT_SM * 1.5) + S2 * 2.0;
                    let r = Rect::new(p.x, p.y, p.w, h);
                    if p.paint {
                        g.text(&tl, p.x + S2, p.y + S2, th.text_primary);
                        let sl = g.layout(scope, xs, p.w * 0.22);
                        g.text(&sl, p.x + p.w * 0.46, p.y + (h - metrics(&sl).h) / 2.0, th.text_disabled);
                        let mut kx = p.x + p.w - S2;
                        if keys.is_empty() {
                            let nl = g.layout(t("命令面板"), Style { italic: true, ..xs }, 200.0);
                            let nm = metrics(&nl);
                            g.text(&nl, kx - nm.w, p.y + (h - nm.h) / 2.0, th.text_disabled);
                        }
                        for k in keys.iter().rev() {
                            let kl = g.layout(k, Style::new(Stack::Code, TEXT_XS), 300.0);
                            let km = metrics(&kl);
                            let kb = Rect::new(kx - km.w - S2 * 2.0, p.y + (h - km.h - 4.0) / 2.0, km.w + S2 * 2.0, km.h + 4.0);
                            g.fill_round(kb, RADIUS_SM, th.bg_inset);
                            g.stroke_round(kb, RADIUS_SM, th.border_soft, 1.0);
                            g.text(&kl, kb.x + S2, kb.y + 2.0, th.text_secondary);
                            kx = kb.x - S1;
                        }
                        g.fill(Rect::new(p.x, p.y + h - 1.0, p.w, 1.0), theme::with_alpha(th.border_soft, 0.5));
                    }
                    let _ = r;
                    p.y += h;
                }
            }
            Item::Space(h) => p.y += h,
        }
    }
    for (r, h) in p.hits {
        ui.region(r, h);
    }
    fields.clear();
    for (id, r, _) in p.fields {
        fields.push((id, r));
    }
    p.y - y
}

fn ctrl_height(c: &Ctrl) -> f32 {
    match c {
        Ctrl::None => 0.0,
        Ctrl::Seg { .. } => TEXT_XS * 1.5 + S1 * 2.0 + 4.0,
        Ctrl::Switch { .. } => 16.0,
        Ctrl::Field { .. } => TEXT_SM * 1.5 + S1 * 2.0 + 2.0,
        Ctrl::Buttons(_) => BTN_H,
    }
}

fn ctrl_width(g: &Gfx, c: &Ctrl, full: f32) -> f32 {
    match c {
        Ctrl::None => 0.0,
        Ctrl::Seg { options, .. } => options.iter().map(|o| metrics(&g.layout(o, Style::new(Stack::Ui, TEXT_XS), 1000.0)).w + S3 * 2.0 + 2.0).sum::<f32>() + 4.0,
        Ctrl::Switch { .. } => 16.0,
        Ctrl::Field { width, unit, .. } => {
            if *width == 0.0 {
                full
            } else {
                width + if unit.is_empty() { 0.0 } else { S2 + metrics(&g.layout(unit, Style::new(Stack::Ui, TEXT_XS), 400.0)).w }
            }
        }
        Ctrl::Buttons(b) => b.iter().map(|(_, l, _, _)| button_width(g, l)).sum::<f32>() + S2 * (b.len().saturating_sub(1)) as f32,
    }
}

fn paint_ctrl(ui: &mut crate::ui::Ui, g: &Gfx, c: &Ctrl, r: Rect, hits: &mut Vec<(Rect, Hit)>, fields: &mut Vec<(u32, Rect, Ctrl)>) {
    let th = app().theme;
    match c {
        Ctrl::None => {}
        Ctrl::Seg { id, options, current } => {
            g.fill_round(r, RADIUS_MD, th.bg_inset);
            g.stroke_round(r, RADIUS_MD, th.border_soft, 1.0);
            let mut x = r.x + 2.0;
            for (i, o) in options.iter().enumerate() {
                let l = g.layout(o, Style::new(Stack::Ui, TEXT_XS), 1000.0);
                let m = metrics(&l);
                let cell = Rect::new(x, r.y + 2.0, m.w + S3 * 2.0, r.h - 4.0);
                let hit = Hit::PanelControl(id * 16 + i as u32);
                let on = i == *current;
                if on {
                    g.fill_round(cell, RADIUS_SM, th.bg_base);
                    g.fill(Rect::new(cell.x, cell.bottom() - 2.0, cell.w, 2.0), th.accent);
                }
                let c = if on { th.text_primary } else { theme::mix(th.text_secondary, th.text_tertiary, ui.hover_t(hit)) };
                g.text(&l, cell.x + S3, cell.y + (cell.h - m.h) / 2.0, c);
                hits.push((cell, hit));
                x += cell.w + 2.0;
            }
        }
        Ctrl::Switch { id, on } => {
            let hit = Hit::PanelControl(id * 16);
            let k = ui.value((hit, 3), if *on { 1.0 } else { 0.0 });
            // A checkbox: the style has no toggle switch.
            let track = Rect::new(r.x, r.y, 16.0, 16.0);
            g.fill(track, theme::with_alpha(th.accent, k));
            g.stroke_round(track, 0.0, theme::mix(th.accent, th.border_strong, k), 1.5);
            let tick = theme::with_alpha(th.on_accent, k);
            g.line(track.x + 4.0, track.y + 8.4, track.x + 6.8, track.y + 11.0, tick, 2.0);
            g.line(track.x + 6.8, track.y + 11.0, track.x + 12.0, track.y + 5.0, tick, 2.0);
            hits.push((track, hit));
        }
        Ctrl::Field { id, width, unit, enabled, .. } => {
            let fw = if *width == 0.0 { r.w } else { *width };
            let fr = Rect::new(r.x, r.y, fw, r.h);
            g.fill_round(fr, RADIUS_SM, th.bg_inset);
            fields.push((*id, fr, Ctrl::None));
            if *enabled {
                hits.push((fr, Hit::PanelControl(id * 16)));
            }
            if !unit.is_empty() {
                let l = g.layout(unit, Style::new(Stack::Ui, TEXT_XS), 400.0);
                g.text(&l, fr.right() + S2, r.y + (r.h - metrics(&l).h) / 2.0, th.text_disabled);
            }
        }
        Ctrl::Buttons(bs) => {
            let mut x = r.x;
            for (id, label, kind, enabled) in bs {
                let bw = button_width(g, label);
                button(ui, g, Rect::new(x, r.y, bw, r.h), Hit::PanelControl(id * 16), label, *kind, *enabled);
                x += bw + S2;
            }
        }
    }
}

pub fn paint(w: &mut Window, g: &Gfx) {
    let th = app().theme;
    let Some(Modal::Panel(p)) = &w.modal else { return };
    let opened = p.opened;
    let (fade, _) = panel_scrim(w, g, opened);
    if let Some(Modal::Panel(Panel { kind: Kind::Image { path }, .. })) = &w.modal {
        let path = path.clone();
        image_view(w, g, path, fade);
        return;
    }
    let (ww, wh) = (w.width, w.height);
    let Some(Modal::Panel(p)) = &w.modal else { return };
    let (title, wide, settings) = match &p.kind {
        Kind::Settings { .. } => (t("设置"), 960.0, true),
        Kind::Help => (t("帮助"), 760.0, false),
        Kind::Export { .. } => (t("导出"), if w.doc.as_ref().is_some_and(|d| d.meta.is_some()) { 960.0 } else { 760.0 }, false),
        Kind::Organize { .. } => (t("整理为文档文件夹"), 760.0, false),
        Kind::Image { .. } => ("", 0.0, false),
    };
    let pw = (ww - S6 * 2.0).min(wide);
    let max_h = wh * 0.78;
    let x = (ww - pw) / 2.0;
    let y = wh * 0.1;
    let head_h = S4 * 2.0 + TEXT_LG * 1.5;
    // Footer content.
    let (foot_l, foot_r, foot_btn) = footer(p, w);
    let foot_h = if foot_btn.is_some() { S3 * 2.0 + BTN_H } else { S3 * 2.0 + TEXT_XS * 1.5 };
    let has_foot = !foot_l.is_empty() || foot_btn.is_some();
    // Items and their height decide the panel height (settings is fixed at 78vh).
    // At least 160, and wide enough that no section name wraps (the English ones run longest).
    let nav_w = if settings { SECTIONS.iter().map(|l| metrics(&g.layout(t(l), NAV_STYLE, 1.0e4)).w + NAV_PAD).fold(160.0, f32::max) } else { 0.0 };
    let body_w = pw - nav_w - S6 * 2.0;
    let items = items_for(p, w);
    let mut scratch = vec![];
    let content_h = layout(&mut w.ui, g, &items, 0.0, 0.0, body_w, false, &mut scratch) + S6 * 2.0;
    let ph = if settings { max_h } else { (head_h + content_h + if has_foot { foot_h } else { 0.0 }).min(max_h) };
    let r = Rect::new(x, y, pw, ph);
    let focused = w.focused;
    g.with_opacity(fade, || {
        g.fill_round(r, RADIUS_XL, th.bg_overlay);
        g.stroke_round(r, RADIUS_XL, th.border_strong, 1.0);
        // Head.
        let tl = g.layout(title, Style::new(Stack::Display, TEXT_LG).weight(620), r.w);
        let tm = metrics(&tl);
        g.text(&tl, r.x + S6, r.y + (head_h - tm.h) / 2.0, th.text_primary);
        g.fill(Rect::new(r.x, r.y + head_h - 1.0, r.w, 1.0), th.border_soft);
    });
    // Esc button.
    let esc = g.layout("Esc", Style::new(Stack::Code, TEXT_XS), 100.0);
    let em = metrics(&esc);
    let eb = Rect::new(r.right() - S6 - em.w - S2 * 2.0, r.y + (head_h - em.h - 4.0) / 2.0, em.w + S2 * 2.0, em.h + 4.0);
    let eh = w.ui.hover_t(Hit::PanelClose);
    g.with_opacity(fade, || {
        g.fill_round(eb, RADIUS_SM, th.bg_inset);
        g.stroke_round(eb, RADIUS_SM, theme::mix(th.accent_line, th.border_soft, eh), 1.0);
        g.text(&esc, eb.x + S2, eb.y + 2.0, theme::mix(th.text_primary, th.text_tertiary, eh));
    });
    w.ui.region(r, Hit::Panel);
    w.ui.region(eb, Hit::PanelClose);
    // Foot.
    let foot = if has_foot { Rect::new(r.x, r.bottom() - foot_h, r.w, foot_h) } else { Rect::new(r.x, r.bottom(), r.w, 0.0) };
    if has_foot {
        let fl = g.layout(&foot_l, Style::new(Stack::Ui, TEXT_XS), foot.w * 0.6);
        let fr = g.layout(&foot_r, Style::new(Stack::Ui, TEXT_XS), foot.w * 0.4);
        let ui = &mut w.ui;
        g.with_opacity(fade, || {
            g.fill(Rect::new(foot.x, foot.y, foot.w, 1.0), th.border_soft);
            g.text(&fl, foot.x + S6, foot.y + (foot.h - metrics(&fl).h) / 2.0, th.text_tertiary);
            let mut right = foot.right() - S6;
            if let Some((id, label, enabled)) = &foot_btn {
                let bw = button_width(g, label);
                button(ui, g, Rect::new(right - bw, foot.y + S3, bw, BTN_H), Hit::PanelControl(id * 16), label, Btn::Primary, *enabled);
                right -= bw + S4;
            }
            g.text(&fr, right - metrics(&fr).w, foot.y + (foot.h - metrics(&fr).h) / 2.0, th.text_tertiary);
        });
    }
    // Settings navigation.
    let mut body = Rect::new(r.x, r.y + head_h, r.w, foot.y - r.y - head_h);
    if let Some(Modal::Panel(Panel { kind: Kind::Settings { section }, .. })) = &w.modal {
        let section = *section;
        let nav = body.cut_left(nav_w);
        let ui = &mut w.ui;
        g.with_opacity(fade, || {
            g.clip(nav);
            g.fill(nav, th.bg_raised);
            g.unclip();
            g.fill(Rect::new(nav.right() - 1.0, nav.y, 1.0, nav.h), th.border_soft);
            let mut y = nav.y + S4;
            for (i, label) in SECTIONS.iter().enumerate() {
                let l = g.layout(t(label), NAV_STYLE, nav.w - NAV_PAD);
                let row = Rect::new(nav.x + S2, y, nav.w - S2 * 2.0, metrics(&l).h + S3 * 2.0);
                let hit = Hit::PanelControl((C_NAV + i as u32) * 16);
                let on = i == section;
                if on {
                    g.fill_round(row, RADIUS_SM, th.accent_soft);
                    g.fill(Rect::new(row.x, row.y, 2.0, row.h), th.accent);
                } else {
                    g.fill_round(row, RADIUS_SM, theme::with_alpha(th.bg_hover, ui.hover_t(hit)));
                }
                g.text(&l, row.x + S3, row.y + S3, if on { th.text_primary } else { th.text_secondary });
                ui.region(row, hit);
                y += row.h;
            }
        });
    }
    // Body.
    let inner = body.pad(S6, S6);
    let Some(Modal::Panel(p)) = &mut w.modal else { return };
    p.body = body;
    p.content_h = content_h;
    p.scroll = p.scroll.clamp(0.0, (content_h - body.h).max(0.0));
    let scroll = p.scroll;
    let mut field_rects = vec![];
    let ui = &mut w.ui;
    g.clip(body);
    g.with_opacity(fade, || {
        layout(ui, g, &items, inner.x, inner.y - scroll, inner.w, true, &mut field_rects);
    });
    // Text fields draw over their boxes.
    let focus = p.focus;
    for (id, fr) in &field_rects {
        let on = focus == Some(*id);
        let f = p.field(*id);
        let style = if *id == F_SEARCH || *id >= F_FONT_UI && *id <= F_FONT_CODE { Style::new(Stack::Code, TEXT_SM) } else { Style::new(Stack::Code, TEXT_SM) };
        let numeric = matches!(*id, F_FONT_SIZE | F_CODE_SIZE | F_LINE_HEIGHT | F_AUTOSAVE_DELAY | F_HISTORY_INTERVAL | F_TAB | F_MAX_WIDTH);
        if numeric && !on {
            // Right-aligned while at rest; while editing, the caret logic needs the left-aligned field.
            let l = g.layout(&f.text, style, 1.0e4);
            let m = metrics(&l);
            g.with_opacity(fade, || {
                g.stroke_round(*fr, RADIUS_SM, th.border_soft, 1.0);
                g.text(&l, fr.right() - S3 - m.w, fr.y + (fr.h - m.h) / 2.0, th.text_primary);
            });
            continue;
        }
        g.with_opacity(fade, || {
            g.stroke_round(*fr, RADIUS_SM, if on { th.accent_line } else { th.border_soft }, 1.0);
            let placeholder = match *id {
                F_ACCENT => "#6a4c9c",
                F_SEARCH => t("搜索命令或按键…"),
                F_FONT_UI..=F_FONT_CODE => t("输入本机字体名称；留空使用默认"),
                _ => "",
            };
            f.paint(g, style, fr.pad(S3, 0.0), th.text_primary, placeholder, on && focused);
        });
    }
    p.field_rects = field_rects;
    g.unclip();
}

fn items_for(p: &Panel, w: &Window) -> Vec<Item> {
    match &p.kind {
        Kind::Settings { section } => settings_items(p, w, *section),
        Kind::Help => help_items(p),
        Kind::Export { .. } => export_items(p, w),
        Kind::Organize { .. } => organize_items(p, w),
        Kind::Image { .. } => vec![],
    }
}

/// (left text, right text, primary button: id, label, enabled).
fn footer(p: &Panel, w: &Window) -> (String, String, Option<(u32, String, bool)>) {
    match &p.kind {
        Kind::Settings { .. } => (t("设置即时生效并保存").into(), t("快捷键按功能分类，可搜索").into(), None),
        Kind::Help | Kind::Image { .. } => (String::new(), String::new(), None),
        Kind::Export { mode, pending, result, preview } => {
            if w.doc.as_ref().is_none_or(|d| d.meta.is_none()) {
                return (String::new(), String::new(), None);
            }
            let needs_folder = *mode == ExportMode::Zip && !w.doc.as_ref().and_then(|d| d.meta.as_ref()).is_some_and(|m| m.in_folder);
            let left = match result {
                Some(Ok(p)) => format!("{}{}", t("已导出到"), p.display()),
                Some(Err(e)) => tf("导出失败：{0}", &[e]),
                None => t("写出前会先让你确认").into(),
            };
            let _ = preview;
            (left, String::new(), Some((C_EXPORT, t("导出").into(), !*pending && !needs_folder)))
        }
        Kind::Organize { plan, error, .. } => {
            let left = match error {
                Some(e) => tf("整理失败：{0}", &[e]),
                None => t("代码块里的示例图片不会被改动").into(),
            };
            let ok = matches!(plan, Some(Ok(p)) if p.blocked.is_none());
            (left, String::new(), Some((C_ORGANIZE, t("整理").into(), ok)))
        }
    }
}

/// Fullscreen image: fit inside the window on the base colour.
fn image_view(w: &mut Window, g: &Gfx, path: Option<PathBuf>, fade: f32) {
    let th = app().theme;
    let r = w.bounds();
    g.with_opacity(fade, || g.fill(r, th.bg_base));
    let Some(path) = path else { return };
    let Some(d) = &w.doc else { return };
    let Some((iw, ih)) = d.view.images.size(None, &path.to_string_lossy()) else { return };
    let k = (r.w / iw).min(r.h / ih);
    let (dw, dh) = (iw * k, ih * k);
    let ir = Rect::new((r.w - dw) / 2.0, (r.h - dh) / 2.0, dw, dh);
    g.with_opacity(fade, || d.view.images.draw(g, &th, None, &path.to_string_lossy(), "", ir));
    w.ui.region(r, Hit::Backdrop);
}

// ---- input ----------------------------------------------------------------------

fn parse_num(s: &str, min: f64, max: f64) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    v.is_finite().then(|| v.clamp(min, max))
}

/// A field's text changed: push it into settings (numbers only once they parse).
fn field_changed(id: u32, text: &str) {
    let opt = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
    let a = app();
    match id {
        F_FONT_SIZE => {
            if let Some(v) = parse_num(text, 12.0, 26.0) {
                a.patch(|s| s.font_size = v);
            }
        }
        F_CODE_SIZE => {
            if let Some(v) = parse_num(text, 11.0, 20.0) {
                a.patch(|s| s.code_font_size = v);
            }
        }
        F_LINE_HEIGHT => {
            if let Some(v) = parse_num(text, 1.3, 2.2) {
                a.patch(|s| s.line_height = v);
            }
        }
        F_AUTOSAVE_DELAY => {
            if let Some(v) = parse_num(text, 200.0, 5000.0) {
                a.patch(|s| s.auto_save_delay_ms = v);
            }
        }
        F_HISTORY_INTERVAL => {
            if let Some(v) = parse_num(text, 5.0, 300.0) {
                a.patch(|s| s.history_interval_ms = v * 1000.0);
            }
        }
        F_TAB => {
            if let Some(v) = parse_num(text, 2.0, 8.0) {
                a.patch(|s| s.tab_size = v.round());
            }
        }
        F_MAX_WIDTH => {
            if let Some(v) = parse_num(text, 0.0, 4000.0) {
                a.patch(|s| s.image_max_width = v);
            }
        }
        F_FONT_UI => a.patch(|s| s.font_ui = opt(text)),
        F_FONT_READ => a.patch(|s| s.font_read = opt(text)),
        F_FONT_DISPLAY => a.patch(|s| s.font_display = opt(text)),
        F_FONT_CODE => a.patch(|s| s.font_code = opt(text)),
        F_ACCENT => {
            // A half-typed hex is the common case while typing; only a complete one applies.
            let v = text.trim();
            if v.is_empty() || theme::parse_hex(v).is_some() {
                a.patch(|s| s.accent_override = opt(v));
            }
        }
        _ => {}
    }
}

pub fn key(w: &mut Window, vk: VIRTUAL_KEY, m: Mods) -> bool {
    let hwnd = w.hwnd;
    let Some(Modal::Panel(p)) = &mut w.modal else { return false };
    if vk == VK_ESCAPE {
        w.modal = None;
        return true;
    }
    if let Some(id) = p.focus {
        if vk == VK_TAB || vk == VK_RETURN {
            p.focus = None;
            return true;
        }
        let f = p.field(id);
        if f.key(vk, m, hwnd) {
            let text = f.text.clone();
            field_changed(id, &text);
            return true;
        }
    }
    false
}

pub fn text(w: &mut Window, s: &str) {
    let Some(Modal::Panel(p)) = &mut w.modal else { return };
    let Some(id) = p.focus else { return };
    let f = p.field(id);
    f.insert(s);
    let text = f.text.clone();
    field_changed(id, &text);
}

pub fn pointer_down(w: &mut Window, hit: Option<Hit>, x: f32, _y: f32, _button: u8) -> bool {
    let Some(Modal::Panel(p)) = &mut w.modal else { return false };
    match hit {
        Some(Hit::Backdrop) => {
            w.modal = None;
        }
        Some(Hit::PanelControl(c)) if p.fields.iter().any(|(id, _)| id * 16 == c) => {
            let id = c / 16;
            p.focus = Some(id);
            if let Some((_, r)) = p.field_rects.iter().find(|(f, _)| *f == id).copied() {
                let g = app().gfx();
                p.field(id).click(g, Style::new(Stack::Code, TEXT_SM), r.pad(S3, 0.0), x, false);
            }
        }
        _ => p.focus = None,
    }
    true
}

pub fn click(w: &mut Window, hit: Hit) {
    if hit == Hit::PanelClose {
        w.modal = None;
        return;
    }
    let Hit::PanelControl(c) = hit else { return };
    let (id, opt) = (c / 16, (c % 16) as usize);
    let hwnd = w.hwnd;
    let section = match &w.modal {
        Some(Modal::Panel(Panel { kind: Kind::Settings { section }, .. })) => Some(*section),
        _ => None,
    };
    let a = app();
    match id {
        C_NAV..=84 => {
            if let Some(Modal::Panel(p)) = &mut w.modal {
                p.kind = Kind::Settings { section: (id - C_NAV) as usize };
                p.scroll = 0.0;
                p.focus = (id - C_NAV == 4).then_some(F_SEARCH);
            }
        }
        C_LANGUAGE => a.patch(|s| s.language = if opt == 1 { Locale::En } else { Locale::ZhCn }),
        C_THEME => a.patch(|s| s.theme = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System][opt]),
        C_MOTION => a.patch(|s| s.motion = [MotionLevel::Full, MotionLevel::Reduced, MotionLevel::Off][opt]),
        C_CURSOR => a.patch(|s| s.cursor_style = [CursorStyle::Bar, CursorStyle::Block, CursorStyle::Underline][opt]),
        C_GPU => a.patch(|s| s.hardware_acceleration = !s.hardware_acceleration),
        C_AUTOSAVE => a.patch(|s| s.auto_save = !s.auto_save),
        C_HISTORY => {
            a.patch(|s| s.history_enabled = !s.history_enabled);
            w.restart_history();
        }
        C_DEFAULT_APP => dialog::shell_open("ms-settings:defaultapps"),
        C_POLICY => a.patch(|s| s.plain_md_image_policy = [PlainMdImagePolicy::AltPlaceholder, PlainMdImagePolicy::Drop, PlainMdImagePolicy::EmptyRef][opt]),
        id if (C_TOGGLES..C_TOGGLES + 8).contains(&id) => a.patch(|s| {
            let f: &mut bool = match id - C_TOGGLES {
                0 => &mut s.typewriter_mode,
                1 => &mut s.highlight_current_line,
                2 => &mut s.read_only,
                3 => &mut s.sidebar_visible,
                4 => &mut s.outline_visible,
                5 => &mut s.spell_check,
                6 => &mut s.auto_pair,
                _ => &mut s.smart_lists,
            };
            *f = !*f;
        }),
        C_NAMING => a.patch(|s| s.image_naming = [ImageNaming::DateHashName, ImageNaming::Original, ImageNaming::Hash][opt]),
        C_DEDUPE => a.patch(|s| s.image_dedupe = !s.image_dedupe),
        C_RESET => {
            let d = AppSettings::default();
            a.patch(|s| match section.unwrap_or(0) {
                0 => {
                    (s.theme, s.motion, s.font_size, s.font_ui, s.font_read, s.font_code, s.font_display) =
                        (d.theme, d.motion, d.font_size, d.font_ui.clone(), d.font_read.clone(), d.font_code.clone(), d.font_display.clone());
                    (s.code_font_size, s.line_height, s.cursor_style, s.accent_override) = (d.code_font_size, d.line_height, d.cursor_style, d.accent_override.clone());
                    s.hardware_acceleration = d.hardware_acceleration;
                }
                1 => (s.plain_md_image_policy, s.auto_save, s.auto_save_delay_ms, s.history_enabled, s.history_interval_ms) = (d.plain_md_image_policy, d.auto_save, d.auto_save_delay_ms, d.history_enabled, d.history_interval_ms),
                2 => {
                    (s.typewriter_mode, s.highlight_current_line, s.read_only, s.sidebar_visible, s.outline_visible) = (d.typewriter_mode, d.highlight_current_line, d.read_only, d.sidebar_visible, d.outline_visible);
                    (s.spell_check, s.auto_pair, s.smart_lists, s.tab_size) = (d.spell_check, d.auto_pair, d.smart_lists, d.tab_size);
                }
                _ => (s.image_naming, s.image_dedupe, s.image_max_width) = (d.image_naming, d.image_dedupe, d.image_max_width),
            });
            if let Some(Modal::Panel(p)) = &mut w.modal {
                p.load_fields();
            }
        }
        C_CATEGORY => {
            // A choice list of the action groups, like the TS <select>.
            let x = w.ui.mx;
            let y = w.ui.my;
            let mut entries = vec![super::menu::Entry::Item { id: String::new(), title: t("全部功能").into(), key: String::new(), disabled: false }];
            entries.extend(GROUPS.iter().map(|(g, l)| super::menu::Entry::Item { id: g.to_string(), title: t(l).into(), key: String::new(), disabled: false }));
            let saved = w.modal.take();
            let mut menu = super::menu::Menu::new(x, y, entries);
            menu.pick = Some(|w, id| {
                if let Some(Modal::Menu(_)) = &w.modal {}
                if let Some(saved) = w.parked.take() {
                    w.modal = Some(saved);
                }
                if let Some(Modal::Panel(p)) = &mut w.modal {
                    *p.field(C_CATEGORY) = Field::new(id);
                }
            });
            w.parked = saved;
            w.modal = Some(Modal::Menu(menu));
        }
        id if (C_MODE..C_MODE + 4).contains(&id) => {
            let w2: &Window = w;
            let _ = w2;
            if let Some(Modal::Panel(p)) = &mut w.modal {
                if let Kind::Export { mode, .. } = &mut p.kind {
                    *mode = [ExportMode::PlainMd, ExportMode::Zip, ExportMode::Html, ExportMode::Pdf][(id - C_MODE) as usize];
                }
            }
            let mut m = w.modal.take();
            if let Some(Modal::Panel(p)) = &mut m {
                p.start_preview(w);
            }
            w.modal = m;
        }
        C_EXPORT => run_export(w),
        C_ORGANIZE_LINK => w.modal = Some(Modal::organize(w)),
        C_MOVE | C_DOWNLOAD => {
            if let Some(Modal::Panel(Panel { kind: Kind::Organize { move_original, download, .. }, .. })) = &mut w.modal {
                if id == C_MOVE {
                    *move_original = opt == 1;
                } else {
                    *download = !*download;
                }
            }
        }
        C_ORGANIZE => run_organize(w),
        C_WELCOME => {
            if let Some(stock) = welcome_stock() {
                let docs = documents_dir();
                let name = stock.strip_prefix(crate::resource_dir("stock").unwrap_or_default()).ok().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                match mdwisp_core::documents::resolve_stock(&crate::resource_dir("stock").unwrap_or_default(), &docs, &name) {
                    Ok(Some(p)) => {
                        w.modal = None;
                        w.open_in_new_window(Some(p), String::new());
                    }
                    Ok(None) => {}
                    Err(e) => w.toast(&e),
                }
            }
        }
        C_REVEAL_WELCOME => {
            if let Some(p) = welcome_stock() {
                dialog::reveal(&p);
            }
        }
        C_COPY_PLUGIN => {
            if let Some(p) = crate::resource_dir("plugin") {
                crate::clipboard::set_text(hwnd, &format!("/plugin marketplace add \"{}\"\n/plugin install mdwisp@mdwisp", p.display()));
                w.toast(t("已复制"));
            }
        }
        C_INSTALL_SKILLS => {
            let Some(project) = dialog::pick_folder(hwnd, t("选择 Codex / Cursor 使用的项目文件夹")) else { return };
            let result = crate::resource_dir("plugin").ok_or_else(|| "plugin".to_string()).and_then(|root| mdwisp_core::plugin::install_skills(&root, &project));
            let msg = match result {
                Ok(p) => tf("已安装到 {0}。在该项目中新建会话；若未出现，请重启 Codex / Cursor。", &[&p.display()]),
                Err(e) => tf("安装失败：{0}", &[&e]),
            };
            if let Some(Modal::Panel(p)) = &mut w.modal {
                p.install_result = Some(msg);
            }
        }
        id if id >= C_COPY_SKILL && id < C_COPY_SKILL + 8 => {
            if let Some((_, _, path)) = skills().get((id - C_COPY_SKILL) as usize) {
                if let Ok(body) = std::fs::read_to_string(path) {
                    // The front matter is Claude Code's; the rest is plain markdown any assistant accepts.
                    let body = body.replace("\r\n", "\n");
                    let rules = body[mdwisp_core::documents::frontmatter_end(&body)..].trim();
                    crate::clipboard::set_text(hwnd, rules);
                    w.toast(t("已复制"));
                }
            }
        }
        _ => {}
    }
}

fn documents_dir() -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
                PathBuf::from(s)
            }
            Err(_) => std::env::var("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).unwrap_or_default(),
        }
    }
}

fn run_export(w: &mut Window) {
    let Some(Modal::Panel(Panel { kind: Kind::Export { mode, preview, .. }, .. })) = &w.modal else { return };
    let mode = *mode;
    let target = match preview {
        Some(Ok(p)) => p.target_path.clone(),
        _ => PathBuf::new(),
    };
    let Some((path, text)) = w.doc.as_ref().and_then(|d| d.path().map(|p| (p.to_path_buf(), d.view.buf.text.clone()))) else { return };
    let s = app().s().clone();
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (filter, title) = match mode {
        ExportMode::PlainMd => (vec![(t("Markdown 文档").to_string(), "*.md".to_string())], t("导出为纯 Markdown")),
        ExportMode::Zip => (vec![("ZIP".to_string(), "*.zip".to_string())], t("打包文档文件夹")),
        ExportMode::Html => (vec![("HTML".to_string(), "*.html".to_string())], t("导出为单文件 HTML")),
        ExportMode::Pdf => (vec![("PDF".to_string(), "*.pdf".to_string())], t("导出为 PDF")),
    };
    let _ = title;
    let name = if mode == ExportMode::Zip { mdwisp_export::archive_name_for(&path) } else { name };
    // null means the dialog was cancelled — not a failure, so no message.
    let Some(out) = dialog::save_file(w.hwnd, &name, &filter) else { return };
    if let Some(Modal::Panel(Panel { kind: Kind::Export { pending, .. }, .. })) = &mut w.modal {
        *pending = true;
    }
    let hwnd = w.hwnd.0 as isize;
    std::thread::spawn(move || {
        let r: std::io::Result<()> = (|| match mode {
            ExportMode::PlainMd => std::fs::write(&out, mdwisp_export::preview_plain_md(&path, &text, image_policy(&s)).output),
            ExportMode::Zip => std::fs::write(&out, mdwisp_export::run_zip(&path)?),
            ExportMode::Html => std::fs::write(&out, mdwisp_export::build_html(&path, &text, export_theme(&s))?.html),
            ExportMode::Pdf => std::fs::write(&out, mdwisp_export::render_pdf(&path, &text, export_theme(&s))?.0),
        })();
        win::post(hwnd, move |w| {
            if let Some(Modal::Panel(Panel { kind: Kind::Export { pending, result, .. }, .. })) = &mut w.modal {
                *pending = false;
                *result = Some(r.map(|_| out).map_err(|e| e.to_string()));
            }
            win::request_frame(w.hwnd);
        });
    });
}

fn run_organize(w: &mut Window) {
    let Some(Modal::Panel(Panel { kind: Kind::Organize { move_original, download, .. }, .. })) = &w.modal else { return };
    let opts = OrganizeOptions { r#move: *move_original, download: *download };
    // The new copy is written from the buffer; saving first keeps the original identical to it.
    if w.doc.as_ref().is_some_and(|d| d.dirty()) && !w.save(false) {
        return;
    }
    let Some((path, text)) = w.doc.as_ref().and_then(|d| d.path().map(|p| (p.to_path_buf(), d.view.buf.text.clone()))) else { return };
    match mdwisp_core::organize::run_organize(&path, &text, opts, |p| trash::delete(p).map_err(|e| e.to_string())) {
        Ok(done) => {
            w.modal = None;
            if let Err(e) = w.load(Some(&done.doc_path)) {
                w.error = Some(e);
            }
            if !done.failed.is_empty() {
                w.error = Some(tf("{0} 张网络图片下载失败，链接保持原样", &[&done.failed.len()]));
            }
        }
        Err(e) => {
            if let Some(Modal::Panel(Panel { kind: Kind::Organize { error, .. }, .. })) = &mut w.modal {
                *error = Some(e);
            }
        }
    }
}

pub fn drag(_w: &mut Window, _x: f32, _y: f32) -> bool {
    false
}

pub fn drag_end(_w: &mut Window) {}

pub fn wheel(w: &mut Window, dy: f32) -> bool {
    if let Some(Modal::Panel(p)) = &mut w.modal {
        p.scroll = (p.scroll - dy * 60.0).clamp(0.0, (p.content_h - p.body.h).max(0.0));
    }
    true
}
