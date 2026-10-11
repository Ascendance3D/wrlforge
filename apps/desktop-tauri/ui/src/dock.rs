// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-DOCK-1: the dockable workspace.
//!
//! The layout tree, its validation and its geometry are pure Rust
//! (`protocol::layout`). This module renders them and turns pointer and
//! keyboard input into whole-layout changes.
//!
//! **Panel lifetime.** Each of the five panels is mounted exactly ONCE, in
//! a fixed host element that never moves in the DOM. A layout change only
//! changes the host's absolute position, its size and its `data-state`
//! (`active`, `inactive` tab, or `hidden`); inactive and hidden hosts use
//! `visibility: hidden`, which keeps their layout boxes. No element is
//! re-parented, re-created or disposed, so Leptos keeps sole ownership and
//! the Source textarea (value, selection, caret, scroll, syntax layer), the
//! X_ITE `<x3d-canvas>` (browser, scene, preview generation, pick and gizmo
//! bindings) and any unapplied Inspector field value all survive every
//! move, tab change, close and restore. Only the tab strips, dividers and
//! drag guides are rendered from the tree; they hold no state.
//!
//! **Safety.** Every layout change first cancels an active gizmo drag
//! (`gizmo::cancel`: no source edit, no undo step, the tool stays on). A
//! drop is applied only on release and only when `Layout::dock` accepts it
//! as a whole; Escape, pointer cancel and lost capture abandon the drag.
//! Nothing here calls a document, history or preview command.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, KeyboardEvent, PointerEvent};
use wrlforge_desktop_protocol::command::{CommandId, PanelSet};
use wrlforge_desktop_protocol::layout::{
    self as lay, DropTarget, Geometry, Layout, LayoutSetOutcome, LayoutState, PanelId, PresetId,
    Rect, SplitAxis, Zone,
};

use crate::ipc::{call, NoArgs};

/// What the drag overlay shows.
#[derive(Clone, Debug, PartialEq)]
pub struct DragView {
    pub panel: PanelId,
    /// Pointer, in dock coordinates.
    pub x: f64,
    pub y: f64,
    pub over: Option<Over>,
}

/// The target under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Over {
    pub target: DropTarget,
    pub valid: bool,
    /// The snap guide: where the panel will go.
    pub guide: Rect,
    /// The group (index in the geometry) under the pointer.
    pub group: usize,
    /// A tab-strip drop (insert between tabs).
    pub strip: bool,
}

#[derive(Clone, Copy)]
pub struct Dock {
    pub layout: RwSignal<Layout>,
    /// The workspace's client size.
    pub size: RwSignal<(f64, f64)>,
    pub geo: Memo<Geometry>,
    pub drag: RwSignal<Option<DragView>>,
    /// Startup recovery or save failure, shown beside the Layout control.
    pub notice: RwSignal<Option<String>>,
    /// The divider being dragged (`path/index` key), for its style.
    pub resizing: RwSignal<Option<String>>,
}

thread_local! {
    static DOCK: OnceCell<Dock> = const { OnceCell::new() };
    static INPUT: RefCell<Input> = RefCell::new(Input::default());
    static LAST_RECT: RefCell<HashMap<PanelId, Rect>> = RefCell::new(HashMap::new());
    static SAVE: RefCell<(bool, bool)> = const { RefCell::new((false, false)) };
    pub static STATS: RefCell<Stats> = RefCell::new(Stats::default());
}

/// Counters the smoke run reads.
#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub drops: u64,
    pub refused: u64,
    pub cancels: u64,
    pub last_cancel: Option<String>,
    pub saves: u64,
    pub save_failures: u64,
    pub resizes: u64,
}

#[derive(Default)]
struct Input {
    tab: Option<TabPress>,
    divider: Option<DividerDrag>,
}

struct TabPress {
    panel: PanelId,
    pointer: i32,
    x0: f64,
    y0: f64,
    dragging: bool,
    el: Element,
}

struct DividerDrag {
    key: String,
    path: Vec<usize>,
    index: usize,
    axis: SplitAxis,
    pointer: i32,
    start: f64,
    layout: Layout,
    geo: Geometry,
    el: Element,
}

/// Create the state before the UI mounts (no render effect may own it).
pub fn init() {
    dock();
}

pub fn dock() -> Dock {
    DOCK.with(|d| {
        *d.get_or_init(|| {
            let layout = RwSignal::new(Layout::default_layout());
            let size = RwSignal::new((1200.0, 760.0));
            let geo = Memo::new(move |_| {
                let (w, h) = size.get();
                layout.with(|l| lay::solve(l, w, h))
            });
            Dock {
                layout,
                size,
                geo,
                drag: RwSignal::new(None),
                notice: RwSignal::new(None),
                resizing: RwSignal::new(None),
            }
        })
    })
}

/// The panels in the current layout (registry enabled-state).
pub fn shown() -> PanelSet {
    dock().layout.with(|l| PanelSet::of(&l.visible()))
}

pub fn shown_untracked() -> PanelSet {
    dock().layout.with_untracked(|l| PanelSet::of(&l.visible()))
}

// ------------------------------------------------------------ applying

/// Adopt a validated layout. An active gizmo drag is canceled FIRST, so no
/// manipulation outlives the geometry it started in.
pub fn apply(new: Layout, persist_it: bool) {
    crate::gizmo::cancel("the layout changed");
    let d = dock();
    if d.layout.with_untracked(|l| *l != new) {
        d.layout.set(new);
    }
    if persist_it {
        persist();
    }
}

/// Save the CURRENT layout; at most one write in flight, the last state wins.
fn persist() {
    let busy = SAVE.with_borrow_mut(|s| {
        if s.0 {
            s.1 = true;
            true
        } else {
            s.0 = true;
            false
        }
    });
    if busy {
        return;
    }
    spawn_local(async {
        #[derive(serde::Serialize)]
        struct A {
            layout: lay::LayoutWire,
        }
        loop {
            let wire = dock().layout.with_untracked(|l| l.to_wire());
            let out = call::<LayoutSetOutcome>("layout_set", A { layout: wire }).await;
            let d = dock();
            match out {
                Ok(LayoutSetOutcome::Saved) => {
                    STATS.with_borrow_mut(|s| s.saves += 1);
                    d.notice.set(None);
                }
                Ok(LayoutSetOutcome::NotSaved { message })
                | Ok(LayoutSetOutcome::Rejected { message }) => {
                    STATS.with_borrow_mut(|s| s.save_failures += 1);
                    d.notice.set(Some(message));
                }
                Err(e) => {
                    STATS.with_borrow_mut(|s| s.save_failures += 1);
                    d.notice.set(Some(format!(
                        "Layout applied for this session but NOT saved: {e}"
                    )));
                }
            }
            let again = SAVE.with_borrow_mut(|s| {
                let a = s.1;
                s.1 = false;
                if !a {
                    s.0 = false;
                }
                a
            });
            if !again {
                break;
            }
        }
    });
}

/// Startup: the saved layout (validated again here), or the default preset
/// with the backend's notice. Never fails.
pub async fn load() {
    let d = dock();
    match call::<LayoutState>("layout_get", NoArgs {}).await {
        Ok(st) => match Layout::from_wire(&st.layout) {
            Ok((l, _)) => {
                apply(l, false);
                d.notice.set(st.notice);
            }
            Err(e) => {
                apply(Layout::default_layout(), false);
                d.notice.set(Some(format!(
                    "The saved layout is invalid ({e}); using the Source layout."
                )));
            }
        },
        Err(e) => d.notice.set(Some(format!(
            "The saved layout could not be read ({e}); using the Source layout."
        ))),
    }
}

fn change(f: impl FnOnce(&Layout) -> Result<Layout, lay::LayoutError>) -> Result<(), String> {
    let cur = dock().layout.get_untracked();
    match f(&cur) {
        // Nothing to change: no gizmo cancel, no save.
        Ok(n) if n == cur => Ok(()),
        Ok(n) => {
            apply(n, true);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// `view.panel.*`: show (or bring to the front) and focus.
pub fn show(p: PanelId) {
    if let Err(e) = change(|l| l.show(p)) {
        crate::ui::flash(&format!("{} not shown: {e}.", p.title()));
        return;
    }
    focus_panel(p);
}

/// `view.hidePanel.*`. The panel stays mounted; nothing is applied.
///
/// UI-MENU-1 (PR #142 review): when a tab of the hidden panel's group had
/// the keyboard focus, the re-keyed tab strip destroys that element. The
/// focus then goes to the tab that is now active in the same group, never to
/// the document body; if the group is gone, to the Source tab.
pub fn hide(p: PanelId) {
    let focused = focused_tab_panel();
    let siblings: Vec<PanelId> = dock()
        .layout
        .with_untracked(|l| l.group_of(p).map(|(g, _)| g).unwrap_or_default());
    let had_focus = focused.is_some_and(|f| siblings.contains(&f));
    if let Err(e) = change(|l| l.hide(p)) {
        crate::ui::flash(&format!("{} not hidden: {e}.", p.title()));
        return;
    }
    if had_focus {
        let next = dock().layout.with_untracked(|l| {
            siblings
                .iter()
                .filter(|s| **s != p)
                .find_map(|s| l.group_of(*s).map(|(_, active)| active))
        });
        focus_panel_tab(next.unwrap_or(PanelId::Source));
    }
}

/// The panel whose dock tab has the keyboard focus, if any.
fn focused_tab_panel() -> Option<PanelId> {
    let id = web_sys::window()?.document()?.active_element()?.id();
    let slug = id.strip_prefix("dock-tab-")?;
    PanelId::ALL.into_iter().find(|p| p.slug() == slug)
}

/// `view.focus.*`: bring the panel's tab to the front and focus it.
pub fn focus(p: PanelId) {
    let _ = change(|l| {
        if l.is_on_screen(p) {
            Ok(l.clone())
        } else {
            l.activate(p)
        }
    });
    focus_panel(p);
}

fn focus_panel(p: PanelId) {
    spawn_local(async move {
        crate::ipc::sleep(0).await;
        // The Source textarea keeps its selection when it takes focus.
        let id = match p {
            PanelId::Source => "source".to_string(),
            _ => format!("dock-tab-{}", p.slug()),
        };
        if let Some(el) = crate::element_by_id::<HtmlElement>(&id) {
            let _ = el.focus();
        }
    });
}

/// `view.preset.*`: switch to a preset (document and selection unchanged).
pub fn set_preset(p: PresetId) {
    apply(Layout::preset(p), true);
}

/// `view.resetLayout`: the active preset, as defined.
pub fn reset() {
    let p = dock().layout.with_untracked(|l| l.preset);
    apply(Layout::preset(p), true);
}

// ------------------------------------------------------------ geometry

fn px(v: f64) -> String {
    format!("{v:.2}px")
}

fn dock_origin() -> (f64, f64) {
    crate::element_by_id::<Element>("dock")
        .map(|e| {
            let r = e.get_bounding_client_rect();
            (r.left(), r.top())
        })
        .unwrap_or_default()
}

fn divider_key(path: &[usize], index: usize) -> String {
    let p: Vec<String> = path.iter().map(|i| i.to_string()).collect();
    format!("{}/{index}", p.join("."))
}

fn group_key(panels: &[PanelId]) -> String {
    panels
        .iter()
        .map(|p| p.slug())
        .collect::<Vec<_>>()
        .join("+")
}

/// Measure the workspace and follow its size. A size change cancels an
/// active gizmo drag before the viewport moves.
pub fn install() {
    let Some(ws) = crate::element_by_id::<HtmlElement>("workspace") else {
        return;
    };
    let measure = |ws: &HtmlElement| {
        let (w, h) = (ws.client_width() as f64, ws.client_height() as f64);
        let d = dock();
        if d.size.get_untracked() != (w, h) && w > 0.0 && h > 0.0 {
            crate::gizmo::cancel("the workspace was resized");
            d.size.set((w, h));
        }
    };
    measure(&ws);
    let ws2 = ws.clone();
    let cb = Closure::<dyn FnMut(JsValue)>::new(move |_| measure(&ws2));
    if let Ok(ro) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
        ro.observe(&ws);
        std::mem::forget(ro);
    }
    cb.forget();
    let Some(w) = web_sys::window() else { return };
    // Escape cancels a docking or divider drag (capture: before the panels).
    let esc = Closure::<dyn Fn(KeyboardEvent)>::new(|ev: KeyboardEvent| {
        if ev.key() == "Escape" && dragging() {
            ev.prevent_default();
            ev.stop_propagation();
            cancel_all("Escape");
        }
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
        "keydown",
        esc.as_ref().unchecked_ref(),
        &opts,
    );
    esc.forget();
    let blur = Closure::<dyn Fn(web_sys::Event)>::new(|_| {
        if dragging() {
            cancel_all("the window lost focus");
        }
    });
    let _ = w.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    blur.forget();
}

/// A docking or divider drag is in progress.
pub fn dragging() -> bool {
    INPUT.with_borrow(|i| i.tab.as_ref().is_some_and(|t| t.dragging) || i.divider.is_some())
}

/// Abandon any drag: the layout before it stays (a divider drag is undone).
pub fn cancel_all(reason: &str) {
    let (tab, div) = INPUT.with_borrow_mut(|i| (i.tab.take(), i.divider.take()));
    let mut any = false;
    if let Some(t) = tab {
        let _ = t.el.release_pointer_capture(t.pointer);
        any |= t.dragging;
        dock().drag.set(None);
    }
    if let Some(dv) = div {
        let _ = dv.el.release_pointer_capture(dv.pointer);
        dock().resizing.set(None);
        apply(dv.layout, false);
        any = true;
    }
    if any {
        STATS.with_borrow_mut(|s| {
            s.cancels += 1;
            s.last_cancel = Some(reason.to_string());
        });
    }
}

// ------------------------------------------------------------ tab drag

fn over_at(panel: PanelId, cx: f64, cy: f64) -> Option<Over> {
    let d = dock();
    let (ox, oy) = dock_origin();
    let (x, y) = (cx - ox, cy - oy);
    let geo = d.geo.get_untracked();
    let layout = d.layout.get_untracked();
    for (gi, g) in geo.groups.iter().enumerate() {
        if !g.rect.contains(x, y) {
            continue;
        }
        let anchor = g
            .panels
            .iter()
            .copied()
            .find(|p| *p != panel)
            .unwrap_or(panel);
        if g.strip.contains(x, y) {
            // Between tabs: the measured tab centers decide the index.
            let tabs: Vec<Rect> = g
                .panels
                .iter()
                .filter_map(|p| {
                    let r = crate::element_by_id::<Element>(&format!("dock-tab-{}", p.slug()))?
                        .get_bounding_client_rect();
                    Some(Rect {
                        x: r.left() - ox,
                        y: r.top() - oy,
                        w: r.width(),
                        h: r.height(),
                    })
                })
                .collect();
            let centers: Vec<f64> = tabs.iter().map(|r| r.x + r.w / 2.0).collect();
            let index = lay::insertion_index(&centers, x);
            let at = match tabs.get(index) {
                Some(r) => r.x,
                None => tabs.last().map(|r| r.x + r.w).unwrap_or(g.strip.x),
            };
            let target = DropTarget {
                anchor,
                zone: Zone::Center,
                index: Some(index),
            };
            return Some(Over {
                valid: layout.can_dock(panel, target).is_ok(),
                target,
                guide: Rect {
                    x: at - 2.0,
                    w: 4.0,
                    ..g.strip
                },
                group: gi,
                strip: true,
            });
        }
        let zone = lay::zone_at(g.content, x, y)?;
        let target = DropTarget {
            anchor,
            zone,
            index: None,
        };
        return Some(Over {
            valid: layout.can_dock(panel, target).is_ok(),
            target,
            guide: lay::zone_rect(g.content, zone),
            group: gi,
            strip: false,
        });
    }
    None
}

fn on_tab_down(panel: PanelId, ev: PointerEvent) {
    if ev.button() != 0 || dragging() {
        return;
    }
    // A press on a control inside the tab never starts a drag.
    if ev
        .target()
        .and_then(|t| t.dyn_into::<Element>().ok())
        .is_some_and(|e| e.closest("button").ok().flatten().is_some())
    {
        return;
    }
    let Some(el) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<Element>().ok())
    else {
        return;
    };
    ev.prevent_default();
    let _ = el.set_pointer_capture(ev.pointer_id());
    if let Ok(h) = el.clone().dyn_into::<HtmlElement>() {
        let _ = h.focus();
    }
    INPUT.with_borrow_mut(|i| {
        i.tab = Some(TabPress {
            panel,
            pointer: ev.pointer_id(),
            x0: ev.client_x() as f64,
            y0: ev.client_y() as f64,
            dragging: false,
            el,
        })
    });
}

fn on_tab_move(ev: PointerEvent) {
    let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
    let start = INPUT.with_borrow_mut(|i| {
        let t = i.tab.as_mut().filter(|t| t.pointer == ev.pointer_id())?;
        let begin = !t.dragging && (cx - t.x0).hypot(cy - t.y0) >= lay::DRAG_THRESHOLD_PX;
        if begin {
            t.dragging = true;
        }
        Some((t.panel, t.dragging, begin))
    });
    let Some((panel, dragging, begin)) = start else {
        return;
    };
    if !dragging {
        return;
    }
    if begin {
        // Before anything else can move: no manipulation survives this.
        crate::gizmo::cancel("a panel is being moved");
    }
    let (ox, oy) = dock_origin();
    dock().drag.set(Some(DragView {
        panel,
        x: cx - ox,
        y: cy - oy,
        over: over_at(panel, cx, cy),
    }));
}

fn on_tab_up(ev: PointerEvent) {
    let Some(t) = INPUT.with_borrow_mut(|i| i.tab.take_if(|t| t.pointer == ev.pointer_id())) else {
        return;
    };
    let d = dock();
    if !t.dragging {
        // A click: bring the tab to the front.
        if d.layout.with_untracked(|l| !l.is_on_screen(t.panel)) {
            let _ = change(|l| l.activate(t.panel));
        }
        return;
    }
    let over = over_at(t.panel, ev.client_x() as f64, ev.client_y() as f64);
    d.drag.set(None);
    let result = over
        .filter(|o| o.valid)
        .map(|o| d.layout.with_untracked(|l| l.dock(t.panel, o.target)));
    match result {
        Some(Ok(n)) => {
            STATS.with_borrow_mut(|s| s.drops += 1);
            apply(n, true);
            // UI-MENU-1 (PR #142 review): the drop re-keys the tab strips and
            // destroys the tab that held the focus; give it to the moved tab.
            focus_panel_tab(t.panel);
        }
        _ => {
            // Invalid or no target: the layout is unchanged.
            STATS.with_borrow_mut(|s| s.refused += 1);
        }
    }
}

fn on_tab_cancel(ev: PointerEvent, reason: &str) {
    let mine = INPUT.with_borrow(|i| i.tab.as_ref().is_some_and(|t| t.pointer == ev.pointer_id()));
    if mine {
        cancel_all(reason);
    }
}

/// Tabs: arrows / Home / End select, Shift+arrow reorders, Delete hides.
fn on_tab_key(panel: PanelId, ev: KeyboardEvent) {
    let Some((group, _)) = dock().layout.with_untracked(|l| l.group_of(panel)) else {
        return;
    };
    let i = group.iter().position(|p| *p == panel).unwrap_or(0);
    let n = group.len();
    let key = ev.key();
    if ev.shift_key() && (key == "ArrowLeft" || key == "ArrowRight") {
        ev.prevent_default();
        let to = if key == "ArrowLeft" {
            i.saturating_sub(1)
        } else {
            (i + 2).min(n)
        };
        if change(|l| l.move_tab(panel, to)).is_ok() {
            focus_panel_tab(panel);
        }
        return;
    }
    let next = match key.as_str() {
        "ArrowLeft" => Some((i + n - 1) % n),
        "ArrowRight" => Some((i + 1) % n),
        "Home" => Some(0),
        "End" => Some(n - 1),
        "Enter" | " " => Some(i),
        "Delete" => {
            if let Some(c) = CommandId::hide_for(panel) {
                ev.prevent_default();
                crate::command::run(c);
            }
            None
        }
        _ => None,
    };
    if let Some(j) = next {
        ev.prevent_default();
        let p = group[j];
        if dock().layout.with_untracked(|l| !l.is_on_screen(p)) {
            let _ = change(|l| l.activate(p));
        }
        focus_panel_tab(p);
    }
}

fn focus_panel_tab(p: PanelId) {
    spawn_local(async move {
        crate::ipc::sleep(0).await;
        if let Some(el) = crate::element_by_id::<HtmlElement>(&format!("dock-tab-{}", p.slug())) {
            let _ = el.focus();
        }
    });
}

// ------------------------------------------------------------ dividers

fn on_div_down(path: Vec<usize>, index: usize, ev: PointerEvent) {
    if ev.button() != 0 || dragging() {
        return;
    }
    let Some(el) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<Element>().ok())
    else {
        return;
    };
    let d = dock();
    let geo = d.geo.get_untracked();
    let Some(dg) = geo.divider(&path, index).cloned() else {
        return;
    };
    ev.prevent_default();
    crate::gizmo::cancel("a panel is being resized");
    let _ = el.set_pointer_capture(ev.pointer_id());
    let key = divider_key(&path, index);
    d.resizing.set(Some(key.clone()));
    let start = match dg.axis {
        SplitAxis::Row => ev.client_x() as f64,
        SplitAxis::Column => ev.client_y() as f64,
    };
    INPUT.with_borrow_mut(|i| {
        i.divider = Some(DividerDrag {
            key,
            path,
            index,
            axis: dg.axis,
            pointer: ev.pointer_id(),
            start,
            layout: d.layout.get_untracked(),
            geo,
            el,
        })
    });
}

fn on_div_move(ev: PointerEvent) {
    let next = INPUT.with_borrow(|i| {
        let dv = i
            .divider
            .as_ref()
            .filter(|d| d.pointer == ev.pointer_id())?;
        let at = match dv.axis {
            SplitAxis::Row => ev.client_x() as f64,
            SplitAxis::Column => ev.client_y() as f64,
        };
        // Always from the drag's START layout: clamping never accumulates.
        dv.layout
            .resize(&dv.geo, &dv.path, dv.index, at - dv.start)
            .ok()
    });
    if let Some(n) = next {
        apply(n, false);
    }
}

fn on_div_up(ev: PointerEvent) {
    let Some(dv) = INPUT.with_borrow_mut(|i| i.divider.take_if(|d| d.pointer == ev.pointer_id()))
    else {
        return;
    };
    dock().resizing.set(None);
    if dock().layout.with_untracked(|l| *l != dv.layout) {
        STATS.with_borrow_mut(|s| s.resizes += 1);
        persist();
    }
    let _ = dv.key;
}

fn on_div_cancel(ev: PointerEvent, reason: &str) {
    let mine = INPUT.with_borrow(|i| {
        i.divider
            .as_ref()
            .is_some_and(|d| d.pointer == ev.pointer_id())
    });
    if mine {
        cancel_all(reason);
    }
}

/// Keyboard resizing: arrows (Shift = larger step), Home / End = limits.
fn on_div_key(path: Vec<usize>, index: usize, ev: KeyboardEvent) {
    let d = dock();
    let geo = d.geo.get_untracked();
    let Some(dg) = geo.divider(&path, index) else {
        return;
    };
    let step = if ev.shift_key() {
        lay::KEY_STEP_LARGE_PX
    } else {
        lay::KEY_STEP_PX
    };
    let (lo, hi) = dg.range();
    let delta = match (dg.axis, ev.key().as_str()) {
        (SplitAxis::Row, "ArrowLeft") | (SplitAxis::Column, "ArrowUp") => -step,
        (SplitAxis::Row, "ArrowRight") | (SplitAxis::Column, "ArrowDown") => step,
        (_, "Home") => lo,
        (_, "End") => hi,
        _ => return,
    };
    ev.prevent_default();
    if let Ok(n) = d.layout.get_untracked().resize(&geo, &path, index, delta) {
        if d.layout.with_untracked(|l| *l != n) {
            STATS.with_borrow_mut(|s| s.resizes += 1);
            apply(n, true);
        }
    }
}

// ------------------------------------------------------------ view

fn panel_state(l: &Layout, p: PanelId) -> &'static str {
    match l.group_of(p) {
        None => "hidden",
        Some((_, a)) if a == p => "active",
        Some(_) => "inactive",
    }
}

fn host_rect(g: &Geometry, p: PanelId) -> Rect {
    match g.group_of(p) {
        Some(gr) => {
            LAST_RECT.with_borrow_mut(|m| m.insert(p, gr.content));
            gr.content
        }
        // A closed panel keeps its last box (hidden), so nothing reflows.
        None => LAST_RECT
            .with_borrow(|m| m.get(&p).copied())
            .unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                w: p.min_size().0,
                h: p.min_size().1,
            }),
    }
}

fn host(p: PanelId, content: AnyView) -> impl IntoView {
    let d = dock();
    let rect = Memo::new(move |_| d.geo.with(|g| host_rect(g, p)));
    let state = move || d.layout.with(|l| panel_state(l, p));
    view! {
        <div class="dock-panel" id=format!("dock-panel-{}", p.slug()) data-panel=p.key()
            role="tabpanel" aria-label=p.title()
            data-state=state
            aria-hidden=move || (state() != "active").then_some("true")
            style:left=move || px(rect.get().x)
            style:top=move || px(rect.get().y)
            style:width=move || px(rect.get().w)
            style:height=move || px(rect.get().h)>
            {content}
        </div>
    }
}

fn strip(key: String) -> impl IntoView {
    let d = dock();
    let k2 = key.clone();
    let geo = Memo::new(move |_| {
        d.geo.with(|g| {
            g.groups
                .iter()
                .find(|gr| group_key(&gr.panels) == k2)
                .map(|gr| (gr.strip, gr.active))
        })
    });
    let panels: Vec<PanelId> = key
        .split('+')
        .filter_map(|s| PanelId::from_key(&format!("panel.{s}")))
        .collect();
    let label = format!(
        "{} panel tabs",
        panels
            .iter()
            .map(|p| p.title())
            .collect::<Vec<_>>()
            .join(", ")
    );
    view! {
        <div class="dock-strip" role="tablist" aria-label=label data-group=key.clone()
            style:left=move || px(geo.get().map(|g| g.0.x).unwrap_or_default())
            style:top=move || px(geo.get().map(|g| g.0.y).unwrap_or_default())
            style:width=move || px(geo.get().map(|g| g.0.w).unwrap_or_default())
            style:height=move || px(geo.get().map(|g| g.0.h).unwrap_or_default())>
            {panels.into_iter().map(|p| {
                let active = move || geo.get().is_some_and(|g| g.1 == p);
                let close = CommandId::hide_for(p).map(|c| view! {
                    <button class="dock-tab-close" tabindex="-1" data-command=c.key()
                        aria-label=c.spec().label title=crate::command::title(c)
                        on:pointerdown=|ev: PointerEvent| ev.stop_propagation()
                        on:click=move |ev| { ev.stop_propagation(); crate::command::run(c); }>"×"</button>
                });
                view! {
                    <div class="dock-tab" role="tab" id=format!("dock-tab-{}", p.slug())
                        data-panel=p.key()
                        aria-selected=move || if active() { "true" } else { "false" }
                        aria-controls=format!("dock-panel-{}", p.slug())
                        tabindex=move || if active() { "0" } else { "-1" }
                        title=format!("{}: drag to dock (left, right, top, bottom or onto a tab group); Shift+Left/Right reorders", p.title())
                        on:pointerdown=move |ev: PointerEvent| on_tab_down(p, ev)
                        on:pointermove=on_tab_move
                        on:pointerup=on_tab_up
                        on:pointercancel=|ev: PointerEvent| on_tab_cancel(ev, "the pointer was canceled")
                        on:lostpointercapture=|ev: PointerEvent| on_tab_cancel(ev, "pointer capture was lost")
                        on:keydown=move |ev: KeyboardEvent| on_tab_key(p, ev)>
                        <span class="dock-tab-title">{p.title()}</span>
                        {close}
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

fn divider(key: String) -> impl IntoView {
    let d = dock();
    let k2 = key.clone();
    let geo = Memo::new(move |_| {
        d.geo.with(|g| {
            g.dividers
                .iter()
                .find(|dv| divider_key(&dv.path, dv.index) == k2)
                .cloned()
        })
    });
    let (path, index) = {
        let (p, i) = key.rsplit_once('/').unwrap_or(("", "0"));
        let path: Vec<usize> = p.split('.').filter_map(|s| s.parse().ok()).collect();
        (path, i.parse::<usize>().unwrap_or(0))
    };
    let (p1, p2) = (path.clone(), path.clone());
    let k3 = key.clone();
    let names = move || {
        let g = d.geo.get();
        let dv = g.divider(&p1, index)?;
        // The panels whose groups end at this divider (its first side).
        let before: Vec<&str> = g
            .groups
            .iter()
            .filter(|gr| match dv.axis {
                SplitAxis::Row => (gr.rect.x + gr.rect.w - dv.rect.x).abs() < 1.0,
                SplitAxis::Column => (gr.rect.y + gr.rect.h - dv.rect.y).abs() < 1.0,
            })
            .map(|gr| gr.active.title())
            .collect();
        Some(before.join(" / "))
    };
    view! {
        <div class="dock-divider" role="separator" tabindex="0"
            id=format!("dock-div-{}", key.replace(['/', '.'], "-"))
            data-key=key.clone()
            aria-orientation=move || match geo.get().map(|g| g.axis) {
                Some(SplitAxis::Column) => "horizontal",
                _ => "vertical",
            }
            aria-valuemin="0" aria-valuemax="100"
            aria-valuenow=move || geo.get().map(|g| g.percent().to_string()).unwrap_or_default()
            aria-label=move || format!("Resize {}", names().unwrap_or_else(|| "panels".into()))
            title="Drag to resize; arrow keys move it (Shift: larger steps), Home / End go to the limits"
            data-dragging=move || if d.resizing.get().as_deref() == Some(k3.as_str()) { "true" } else { "false" }
            style:left=move || px(geo.get().map(|g| g.rect.x).unwrap_or_default())
            style:top=move || px(geo.get().map(|g| g.rect.y).unwrap_or_default())
            style:width=move || px(geo.get().map(|g| g.rect.w).unwrap_or_default())
            style:height=move || px(geo.get().map(|g| g.rect.h).unwrap_or_default())
            on:pointerdown=move |ev: PointerEvent| on_div_down(path.clone(), index, ev)
            on:pointermove=on_div_move
            on:pointerup=on_div_up
            on:pointercancel=|ev: PointerEvent| on_div_cancel(ev, "the pointer was canceled")
            on:lostpointercapture=|ev: PointerEvent| on_div_cancel(ev, "pointer capture was lost")
            on:keydown=move |ev: KeyboardEvent| on_div_key(p2.clone(), index, ev)>
        </div>
    }
}

fn zone_label(z: Zone) -> &'static str {
    match z {
        Zone::Left => "left of",
        Zone::Right => "right of",
        Zone::Top => "above",
        Zone::Bottom => "below",
        Zone::Center => "as a tab with",
    }
}

/// The drag overlay: every group's drop area, the zone compass of the group
/// under the pointer (invalid zones dimmed), and the snap guide.
fn overlay() -> impl IntoView {
    let d = dock();
    move || {
        let drag = d.drag.get()?;
        let geo = d.geo.get_untracked();
        let layout = d.layout.get_untracked();
        let hovered = drag.over.as_ref().map(|o| o.group);
        let targets = geo
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let c = g.content;
                view! {
                    <div class="dock-target" data-hover=if hovered == Some(i) { "true" } else { "false" }
                        style:left=px(c.x) style:top=px(c.y) style:width=px(c.w) style:height=px(c.h)></div>
                }
            })
            .collect_view();
        let compass = hovered.and_then(|i| geo.groups.get(i)).map(|g| {
            let c = g.content;
            let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h / 2.0);
            let anchor = g
                .panels
                .iter()
                .copied()
                .find(|p| *p != drag.panel)
                .unwrap_or(drag.panel);
            Zone::ALL
                .into_iter()
                .map(|z| {
                    let (dx, dy) = match z {
                        Zone::Left => (-40.0, 0.0),
                        Zone::Right => (40.0, 0.0),
                        Zone::Top => (0.0, -40.0),
                        Zone::Bottom => (0.0, 40.0),
                        Zone::Center => (0.0, 0.0),
                    };
                    let valid = layout
                        .can_dock(
                            drag.panel,
                            DropTarget {
                                anchor,
                                zone: z,
                                index: None,
                            },
                        )
                        .is_ok();
                    let selected = drag
                        .over
                        .as_ref()
                        .is_some_and(|o| !o.strip && o.target.zone == z);
                    view! {
                        <div class="dock-zone-tile" data-zone=z.key()
                            data-valid=if valid { "true" } else { "false" }
                            data-selected=if selected { "true" } else { "false" }
                            style:left=px(cx + dx - 16.0) style:top=px(cy + dy - 16.0)></div>
                    }
                })
                .collect_view()
        });
        let guide = drag.over.as_ref().map(|o| {
            let g = o.guide;
            view! {
                <div class="dock-guide" id="dock-guide" data-valid=if o.valid { "true" } else { "false" }
                    data-zone=o.target.zone.key() data-anchor=o.target.anchor.key()
                    data-strip=if o.strip { "true" } else { "false" }
                    style:left=px(g.x) style:top=px(g.y) style:width=px(g.w) style:height=px(g.h)></div>
            }
        });
        let status = match &drag.over {
            Some(o) if o.valid => format!(
                "Release to dock {} {} {}.",
                drag.panel.title(),
                zone_label(o.target.zone),
                o.target.anchor.title()
            ),
            Some(_) => format!("{} cannot be docked here.", drag.panel.title()),
            None => format!("Move {} over a panel. Esc cancels.", drag.panel.title()),
        };
        Some(view! {
            <div class="dock-overlay" id="dock-overlay" data-panel=drag.panel.key()>
                {targets}
                {compass}
                {guide}
                <div class="dock-ghost" style:left=px(drag.x) style:top=px(drag.y)>{drag.panel.title()}</div>
                <div class="visually-hidden" role="status" aria-live="polite">{status}</div>
            </div>
        })
    }
}

/// The dock: five permanent panel hosts, plus the tab strips, dividers and
/// drag guides of the current tree.
#[component]
pub fn Workspace() -> impl IntoView {
    let d = dock();
    let groups = move || {
        d.geo.with(|g| {
            g.groups
                .iter()
                .map(|gr| group_key(&gr.panels))
                .collect::<Vec<_>>()
        })
    };
    let dividers = move || {
        d.geo.with(|g| {
            g.dividers
                .iter()
                .map(|dv| divider_key(&dv.path, dv.index))
                .collect::<Vec<_>>()
        })
    };
    view! {
        <main class="workspace" id="workspace" aria-label="Workspace">
            <div class="dock" id="dock" data-preset=move || d.layout.with(|l| l.preset.key())
                style:width=move || px(d.geo.with(|g| g.width))
                style:height=move || px(d.geo.with(|g| g.height))>
                // Mounted once, never moved: see the module comment.
                {host(PanelId::SceneTree, view! { <crate::panels::SceneTree/> }.into_any())}
                {host(PanelId::Source, view! { <crate::SourcePanel/> }.into_any())}
                {host(PanelId::Diagnostics, view! { <crate::panels::Diagnostics/> }.into_any())}
                {host(PanelId::Viewport, view! { <crate::panels::Viewport/> }.into_any())}
                {host(PanelId::Inspector, view! { <crate::panels::Inspector/> }.into_any())}
                <For each=groups key=|k| k.clone() children=strip/>
                <For each=dividers key=|k| k.clone() children=divider/>
                {overlay()}
            </div>
        </main>
    }
}

/// The toolbar Workspace control: panels, focus, presets and reset, every
/// item a registry command. Keyboard: as the Create menu.
#[component]
pub fn LayoutMenu() -> impl IntoView {
    let d = dock();
    let open = RwSignal::new(false);
    let focus_id = |id: &str| {
        if let Some(el) = crate::element_by_id::<HtmlElement>(id) {
            let _ = el.focus();
        }
    };
    let close = move |refocus: bool| {
        open.set(false);
        if refocus {
            focus_id("btn-layout");
        }
    };
    let toggle = move || {
        let now = !open.get_untracked();
        open.set(now);
        if now {
            spawn_local(async move {
                crate::ipc::sleep(0).await;
                if let Some(el) = crate::element_by_id::<HtmlElement>("layout-list")
                    .and_then(|l| l.query_selector("[role^=menuitem]").ok().flatten())
                    .and_then(|e| e.dyn_into::<HtmlElement>().ok())
                {
                    let _ = el.focus();
                }
            });
        }
    };
    let on_menu_key = move |ev: KeyboardEvent| {
        let Some(list) = crate::element_by_id::<HtmlElement>("layout-list") else {
            return;
        };
        let Ok(nodes) = list.query_selector_all("[role^=menuitem]") else {
            return;
        };
        let items: Vec<HtmlElement> = (0..nodes.length())
            .filter_map(|i| nodes.item(i)?.dyn_into::<HtmlElement>().ok())
            .collect();
        let n = items.len();
        if n == 0 {
            return;
        }
        let cur = ev
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|e| items.iter().position(|i| i.id() == e.id()))
            .unwrap_or(0);
        let next = match ev.key().as_str() {
            "ArrowDown" => Some((cur + 1) % n),
            "ArrowUp" => Some((cur + n - 1) % n),
            "Home" => Some(0),
            "End" => Some(n - 1),
            "Escape" => {
                ev.prevent_default();
                close(true);
                None
            }
            "Tab" => {
                close(false);
                None
            }
            _ => None,
        };
        if let Some(i) = next {
            ev.prevent_default();
            let _ = items[i].focus();
        }
    };
    let run = move |c: CommandId| {
        close(true);
        crate::command::run(c);
    };
    let panel_item = move |p: PanelId, show: CommandId, hide: CommandId| {
        let on = move || shown().contains(p);
        view! {
            <button role="menuitemcheckbox" class="create-item" id=format!("layout-panel-{}", p.slug())
                aria-checked=move || if on() { "true" } else { "false" }
                data-command=move || if on() { hide.key() } else { show.key() }
                title=move || crate::command::title(if on() { hide } else { show })
                on:click=move |_| run(if on() { hide } else { show })>
                <span class="check" aria-hidden="true">{move || if on() { "✓" } else { "" }}</span>
                {p.title()}
            </button>
        }
    };
    let preset_item = move |c: CommandId, p: PresetId| {
        let on = move || d.layout.with(|l| l.preset == p);
        view! {
            <button role="menuitemradio" class="create-item" id=format!("layout-{}", c.key().replace('.', "-"))
                aria-checked=move || if on() { "true" } else { "false" }
                data-command=c.key() title=crate::command::title(c)
                on:click=move |_| run(c)>
                <span class="check" aria-hidden="true">{move || if on() { "●" } else { "" }}</span>
                {c.spec().label}
            </button>
        }
    };
    let plain_item = move |c: CommandId| {
        view! {
            <button role="menuitem" class="create-item" id=format!("layout-{}", c.key().replace('.', "-"))
                data-command=c.key() title=crate::command::title(c)
                on:click=move |_| run(c)>
                <span class="check" aria-hidden="true"></span>
                {c.spec().label}
            </button>
        }
    };
    view! {
        <div class="create-menu layout-menu">
            <button id="btn-layout" class="secondary" aria-haspopup="menu"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-controls="layout-list"
                title="Panels and workspace layouts (saved for next launch)"
                on:click=move |_| toggle()
                on:keydown=move |ev: KeyboardEvent| {
                    if ev.key() == "ArrowDown" && !open.get_untracked() {
                        ev.prevent_default();
                        toggle();
                    }
                }>
                {move || format!("Layout: {} ▾", d.layout.with(|l| l.preset.label()))}
            </button>
            {move || open.get().then(|| view! {
                <div id="layout-list" class="create-list" role="menu" aria-label="Panels and layouts"
                    on:keydown=on_menu_key>
                    <div class="menu-group" role="presentation">"Panels"</div>
                    {panel_item(PanelId::SceneTree, CommandId::ViewShowSceneTree, CommandId::ViewHideSceneTree)}
                    {panel_item(PanelId::Inspector, CommandId::ViewShowInspector, CommandId::ViewHideInspector)}
                    {panel_item(PanelId::Diagnostics, CommandId::ViewShowDiagnostics, CommandId::ViewHideDiagnostics)}
                    {plain_item(CommandId::ViewFocusViewport)}
                    {plain_item(CommandId::ViewFocusSource)}
                    <div class="menu-group" role="presentation">"Layout"</div>
                    {preset_item(CommandId::ViewPresetSource, PresetId::Source)}
                    {preset_item(CommandId::ViewPresetModeling, PresetId::Modeling)}
                    {plain_item(CommandId::ViewResetLayout)}
                </div>
            })}
        </div>
        {move || d.notice.get().map(|n| view! {
            <span class="theme-error" id="layout-notice" role="alert" title=n.clone()>{n.clone()}</span>
        })}
    }
}
