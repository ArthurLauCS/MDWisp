//! Immediate-mode interaction: every frame paints and registers hit regions;
//! input is resolved against the regions of the last frame. Hover and press
//! fade through tweens so state changes animate like the CSS transitions.

use std::collections::HashMap;
use std::time::Instant;

use crate::gfx::Rect;
use crate::theme::{Dur, Ease, Motion};

/// Everything the pointer can land on.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Hit {
    Caption,
    Minimize,
    Maximize,
    Close,
    SidebarToggle,
    TitleAction(u8),
    SidebarTab(u8),
    TreeRow(u32),
    OutlineRow(u32),
    AssetFilter(u8),
    AssetRow(u32),
    SidebarResize,
    SidebarSplit,
    HistoryToggle,
    HistoryResize,
    HistoryRow(u32),
    HistoryButton(u8),
    Editor,
    EditorScrollbar,
    MarginLeft,
    MarginRight,
    Breadcrumb(u8),
    Status(u8),
    Palette,
    PaletteRow(u32),
    Menu(u32),
    MenuItem(u32),
    Panel,
    PanelClose,
    PanelControl(u32),
    Backdrop,
    /// Controls drawn inside the editor: code-language chips, table toolbar buttons.
    Widget(u32),
}

pub enum Cursor {
    Arrow,
    Hand,
    IBeam,
    ResizeH,
    ResizeV,
}

#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    dur: f32,
    ease: Ease,
}

impl Tween {
    fn value(&self, now: Instant) -> f32 {
        let t = (now - self.start).as_secs_f32() * 1000.0 / self.dur;
        if t >= 1.0 { self.to } else { self.from + (self.to - self.from) * self.ease.at(t) }
    }
    fn done(&self, now: Instant) -> bool {
        (now - self.start).as_secs_f32() * 1000.0 >= self.dur
    }
}

/// The CSS transition each hover state used.
fn hover_dur(hit: Hit) -> Dur {
    match hit {
        Hit::Minimize | Hit::Maximize | Hit::Close | Hit::MenuItem(_) | Hit::PaletteRow(_) | Hit::Widget(_) => Dur::Instant,
        _ => Dur::Fast,
    }
}

pub struct Ui {
    pub mx: f32,
    pub my: f32,
    pub inside: bool,
    pub hover: Option<Hit>,
    pub press: Option<Hit>,
    regions: Vec<(Rect, Hit)>,
    next: Vec<(Rect, Hit)>,
    tweens: HashMap<(Hit, u8), Tween>,
    pub motion: Motion,
    pub now: Instant,
}

impl Ui {
    pub fn new() -> Ui {
        Ui { mx: -1.0, my: -1.0, inside: false, hover: None, press: None, regions: vec![], next: vec![], tweens: HashMap::new(), motion: Motion::Full, now: Instant::now() }
    }

    pub fn begin_frame(&mut self) {
        self.now = Instant::now();
        self.next.clear();
    }

    pub fn end_frame(&mut self) {
        std::mem::swap(&mut self.regions, &mut self.next);
    }

    /// Later regions win: they were painted on top.
    pub fn region(&mut self, r: Rect, hit: Hit) {
        self.next.push((r, hit));
    }

    pub fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.regions.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }

    /// Recompute hover from the pointer; true when it changed.
    pub fn update_hover(&mut self) -> bool {
        let h = if self.inside { self.hit_at(self.mx, self.my) } else { None };
        if h == self.hover {
            return false;
        }
        if let Some(old) = self.hover {
            self.animate((old, 0), 0.0, hover_dur(old), Ease::Out);
        }
        if let Some(new) = h {
            self.animate((new, 0), 1.0, hover_dur(new), Ease::Out);
        }
        self.hover = h;
        true
    }

    /// Start (or retarget) a tween from wherever the value is now.
    pub fn animate(&mut self, key: (Hit, u8), to: f32, dur: Dur, ease: Ease) {
        let now = Instant::now();
        let from = self.tweens.get(&key).map_or(1.0 - to, |t| t.value(now));
        self.tweens.insert(key, Tween { from, to, start: now, dur: dur.ms(self.motion), ease });
    }

    /// Jump without animating.
    pub fn set(&mut self, key: (Hit, u8), v: f32) {
        self.tweens.insert(key, Tween { from: v, to: v, start: self.now, dur: 1.0, ease: Ease::Out });
    }

    pub fn value(&self, key: (Hit, u8), default: f32) -> f32 {
        self.tweens.get(&key).map_or(default, |t| t.value(self.now))
    }

    /// 0→1 hover fade for `hit`.
    pub fn hover_t(&self, hit: Hit) -> f32 {
        self.value((hit, 0), 0.0)
    }

    pub fn pressed(&self, hit: Hit) -> bool {
        self.press == Some(hit) && self.hover == Some(hit)
    }

    /// True while anything is still moving; finished tweens are dropped.
    pub fn animating(&mut self) -> bool {
        let now = Instant::now();
        self.tweens.retain(|_, t| !(t.done(now) && t.to == 0.0 && t.from == 0.0));
        self.tweens.values().any(|t| !t.done(now))
    }
}
