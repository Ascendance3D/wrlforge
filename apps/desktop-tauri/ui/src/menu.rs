// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-MENU-1: the application menu bar (File | Edit | View | Create | Tools |
//! World | Help), drawn in the window on every platform.
//!
//! Every item is a registry command (`protocol::command::MENUS`): its label,
//! shortcut text, enabled state, disabled reason and checked state come from
//! the registry and the existing UI signals, and activating it calls
//! `command::run` -- the toolbar's own path. This module holds no labels, no
//! handlers and no tool or theme state of its own.
//!
//! Keyboard (Linux / Windows): F10, or Alt pressed and released alone, focuses
//! the bar; Alt+mnemonic opens a menu. Left / Right move between menus, Down /
//! Up / Home / End move between items, Enter or Space activates, Escape closes
//! and gives the focus back to the control that had it. Disabled items stay
//! focusable and say why (`title`, `aria-disabled`). On macOS the standard
//! application menu holds About and Quit (`MAC_APP_MENU`), so this bar leaves
//! them out; F10 and the Alt keys are not used there.
//!
//! The bar is an overlay above the toolbar: opening it recreates no dock
//! panel and never touches the X_ITE canvas.

use std::cell::Cell;
use std::cell::OnceCell;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, KeyboardEvent};
use wrlforge_desktop_protocol::command::{
    self as c, Availability, CommandId, MenuDef, MenuEntry, Platform, MAC_APP_MENU, MENUS,
};

use crate::{clipboard, command, dock};

#[derive(Clone, Copy)]
struct MenuState {
    /// The open menu (index into `MENUS`).
    open: RwSignal<Option<usize>>,
    /// Bumped when the recorded focus changes (menu item states re-read it).
    focus_tick: RwSignal<u64>,
}

thread_local! {
    static STATE: OnceCell<MenuState> = const { OnceCell::new() };
    /// Alt went down with no other key since (Alt alone toggles the bar).
    static ALT_ALONE: Cell<bool> = const { Cell::new(false) };
}

fn st() -> MenuState {
    STATE.with(|s| {
        *s.get_or_init(|| MenuState {
            open: RwSignal::new(None),
            focus_tick: RwSignal::new(0),
        })
    })
}

/// Created before mount, outside any owner (like the dock state).
pub fn init() {
    let _ = st();
}

pub fn is_open() -> bool {
    st().open.get_untracked().is_some()
}

pub fn focus_tick() -> u64 {
    st().focus_tick.get()
}

pub fn bump_focus() {
    st().focus_tick.update(|t| *t += 1);
}

fn mac() -> bool {
    command::platform() == Platform::MacOs
}

fn menu_slug(m: &MenuDef) -> String {
    m.label.to_lowercase()
}

fn item_dom_id(id: CommandId) -> String {
    format!("mi-{}", id.key().replace('.', "-"))
}

fn shown_in_bar(id: CommandId) -> bool {
    !(mac() && MAC_APP_MENU.contains(&id))
}

fn by_id<T: JsCast>(id: &str) -> Option<T> {
    crate::element_by_id::<T>(id)
}

fn focus_el(id: &str) {
    if let Some(e) = by_id::<HtmlElement>(id) {
        let _ = e.focus();
    }
}

fn items(menu: usize) -> Vec<HtmlElement> {
    let Some(list) = by_id::<HtmlElement>(&format!("menu-{}-list", menu_slug(&MENUS[menu]))) else {
        return Vec::new();
    };
    let Ok(nodes) = list.query_selector_all("[role^=menuitem]") else {
        return Vec::new();
    };
    (0..nodes.length())
        .filter_map(|i| nodes.item(i)?.dyn_into::<HtmlElement>().ok())
        .collect()
}

/// Open menu `i` and focus its first (or last) item after it renders.
fn open_menu(i: usize, last: bool) {
    st().open.set(Some(i));
    leptos::task::spawn_local(async move {
        crate::ipc::sleep(0).await;
        let it = items(i);
        let pick = if last { it.last() } else { it.first() };
        if let Some(e) = pick {
            let _ = e.focus();
        }
    });
}

/// Close the open menu; `refocus` gives the focus back to the control that
/// had it before the menu bar took it.
pub fn close(refocus: bool) {
    st().open.set(None);
    if refocus {
        clipboard::restore_focus();
    }
}

/// Focus the bar's first menu button (F10 / Alt) without opening it.
fn focus_bar() {
    focus_el(&format!("menu-{}", menu_slug(&MENUS[0])));
}

fn bar_has_focus() -> bool {
    let Some(a) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
    else {
        return false;
    };
    a.closest("#menubar").ok().flatten().is_some()
}

fn activate(id: CommandId) {
    if id.availability(command::context_now()) != Availability::Enabled {
        // A disabled item says why and stays open.
        if let Availability::Disabled(why) = id.availability(command::context_now()) {
            crate::ui::flash(&format!("{} is not available: {why}", id.spec().label));
        }
        return;
    }
    close(true);
    command::run(id);
}

fn on_bar_button_key(i: usize, ev: KeyboardEvent) {
    let n = MENUS.len();
    match ev.key().as_str() {
        "ArrowRight" => {
            ev.prevent_default();
            focus_el(&format!("menu-{}", menu_slug(&MENUS[(i + 1) % n])));
        }
        "ArrowLeft" => {
            ev.prevent_default();
            focus_el(&format!("menu-{}", menu_slug(&MENUS[(i + n - 1) % n])));
        }
        "Home" => {
            ev.prevent_default();
            focus_el(&format!("menu-{}", menu_slug(&MENUS[0])));
        }
        "End" => {
            ev.prevent_default();
            focus_el(&format!("menu-{}", menu_slug(&MENUS[n - 1])));
        }
        "ArrowDown" | "Enter" | " " => {
            ev.prevent_default();
            open_menu(i, false);
        }
        "ArrowUp" => {
            ev.prevent_default();
            open_menu(i, true);
        }
        "Escape" => {
            ev.prevent_default();
            close(true);
        }
        _ => {}
    }
    ev.stop_propagation();
}

fn on_list_key(i: usize, ev: KeyboardEvent) {
    let it = items(i);
    let n = it.len();
    let cur = ev
        .target()
        .and_then(|t| t.dyn_into::<Element>().ok())
        .and_then(|e| it.iter().position(|x| x.id() == e.id()));
    let go = |k: usize| {
        if let Some(e) = it.get(k) {
            let _ = e.focus();
        }
    };
    let m = MENUS.len();
    match ev.key().as_str() {
        "ArrowDown" if n > 0 => go(cur.map_or(0, |c| (c + 1) % n)),
        "ArrowUp" if n > 0 => go(cur.map_or(n - 1, |c| (c + n - 1) % n)),
        "Home" if n > 0 => go(0),
        "End" if n > 0 => go(n - 1),
        "ArrowRight" => open_menu((i + 1) % m, false),
        "ArrowLeft" => open_menu((i + m - 1) % m, false),
        "Escape" => {
            close(false);
            focus_el(&format!("menu-{}", menu_slug(&MENUS[i])));
        }
        "Tab" => {
            close(true);
            ev.prevent_default();
        }
        "Enter" | " " => {
            if let Some(id) = cur
                .and_then(|c| it.get(c))
                .and_then(|e| e.get_attribute("data-command"))
                .and_then(|k| CommandId::from_key(&k))
            {
                activate(id);
            }
        }
        _ => {
            ev.stop_propagation();
            return;
        }
    }
    ev.prevent_default();
    ev.stop_propagation();
}

fn label_with_mnemonic(m: &MenuDef) -> impl IntoView {
    let (first, rest) = m.label.split_at(1);
    view! { <span class="mnemonic">{first.to_string()}</span>{rest.to_string()} }
}

fn shortcut_text(id: CommandId) -> String {
    let s = id.spec();
    s.shortcuts
        .first()
        .or(s.native_keys.first())
        .map(|k| k.label(command::platform()))
        .unwrap_or_default()
}

fn command_item(id: CommandId) -> AnyView {
    let reason = move || match id.availability(command::context()) {
        Availability::Enabled => None,
        Availability::Disabled(why) => Some(why),
    };
    let role = match id.spec().check {
        c::Check::None => "menuitem",
        c::Check::Theme(_) => "menuitemradio",
        _ => "menuitemcheckbox",
    };
    let checked = move || command::checked(id);
    view! {
        <button role=role class="create-item menu-item" id=item_dom_id(id) data-command=id.key()
            tabindex="-1"
            aria-disabled=move || if reason().is_some() { "true" } else { "false" }
            aria-checked=move || checked().map(|c| if c { "true" } else { "false" })
            title=move || match reason() {
                Some(why) => format!("{} — not available: {why}", id.spec().hint),
                None => command::title(id),
            }
            on:click=move |_| activate(id)>
            <span class="check" aria-hidden="true">{move || match (checked(), role) {
                (Some(true), "menuitemradio") => "●",
                (Some(true), _) => "✓",
                _ => "",
            }}</span>
            <span class="menu-label">{id.spec().label}</span>
            <span class="menu-key" aria-hidden="true">{shortcut_text(id)}</span>
        </button>
    }
    .into_any()
}

/// A panel as one checkbox item: Show when hidden, Hide when shown.
fn panel_item(show: CommandId, hide: CommandId) -> AnyView {
    let p = show.panel().expect("panel command");
    let on = move || dock::shown().contains(p);
    let cur = move || if on() { hide } else { show };
    view! {
        <button role="menuitemcheckbox" class="create-item menu-item" tabindex="-1"
            id=format!("mi-panel-{}", p.slug())
            aria-checked=move || if on() { "true" } else { "false" }
            aria-disabled="false"
            data-command=move || cur().key()
            title=move || command::title(cur())
            on:click=move |_| activate(cur())>
            <span class="check" aria-hidden="true">{move || if on() { "✓" } else { "" }}</span>
            <span class="menu-label">{p.title()}</span>
            <span class="menu-key"></span>
        </button>
    }
    .into_any()
}

fn preset_item(id: CommandId) -> AnyView {
    let preset = id.preset().expect("preset command");
    let on = move || dock::dock().layout.with(|l| l.preset == preset);
    view! {
        <button role="menuitemradio" class="create-item menu-item" tabindex="-1" id=item_dom_id(id)
            data-command=id.key() aria-disabled="false"
            aria-checked=move || if on() { "true" } else { "false" }
            title=command::title(id)
            on:click=move |_| activate(id)>
            <span class="check" aria-hidden="true">{move || if on() { "●" } else { "" }}</span>
            <span class="menu-label">{id.spec().label}</span>
            <span class="menu-key"></span>
        </button>
    }
    .into_any()
}

fn entries_view(m: &'static MenuDef) -> Vec<AnyView> {
    let visible: Vec<&MenuEntry> = m
        .entries
        .iter()
        .filter(|e| match e {
            MenuEntry::Command(id) => shown_in_bar(*id),
            _ => true,
        })
        .collect();
    let mut out = Vec::new();
    for (k, e) in visible.iter().enumerate() {
        out.push(match **e {
            MenuEntry::Command(id) => command_item(id),
            MenuEntry::Panel(show, hide) => panel_item(show, hide),
            MenuEntry::Preset(id) => preset_item(id),
            MenuEntry::Group(g) => {
                view! { <div class="menu-group" role="presentation">{g}</div> }.into_any()
            }
            MenuEntry::Separator => {
                // No separator at an edge (macOS drops About / Quit).
                if k == 0 || k + 1 == visible.len() {
                    continue;
                }
                view! { <div class="menu-sep" role="separator"></div> }.into_any()
            }
        });
    }
    out
}

#[component]
pub fn MenuBar() -> impl IntoView {
    let s = st();
    view! {
        <nav id="menubar" class="menubar" role="menubar" aria-label="Application menu"
            // A pointer press here never takes the focus from the editor or
            // a field: the Edit commands act on that control.
            on:mousedown=move |ev: web_sys::MouseEvent| ev.prevent_default()>
            {MENUS.iter().enumerate().map(|(i, m)| {
                let slug = menu_slug(m);
                let open = move || s.open.get() == Some(i);
                view! {
                    <div class="menubar-menu">
                        <button id=format!("menu-{slug}") role="menuitem" class="menubar-button"
                            tabindex=if i == 0 { "0" } else { "-1" }
                            aria-haspopup="menu"
                            aria-expanded=move || if open() { "true" } else { "false" }
                            aria-controls=format!("menu-{slug}-list")
                            aria-keyshortcuts=if mac() { String::new() } else { format!("Alt+{}", m.mnemonic.to_uppercase()) }
                            on:click=move |_| if open() { close(true) } else { open_menu(i, false) }
                            on:mouseenter=move |_| if s.open.get_untracked().is_some_and(|o| o != i) { open_menu(i, false) }
                            on:keydown=move |ev| on_bar_button_key(i, ev)>
                            {label_with_mnemonic(m)}
                        </button>
                        {move || open().then(|| view! {
                            <div id=format!("menu-{}-list", menu_slug(m)) class="create-list menubar-list" role="menu"
                                aria-label=m.label
                                on:keydown=move |ev| on_list_key(i, ev)>
                                {entries_view(m)}
                            </div>
                        })}
                    </div>
                }
            }).collect_view()}
        </nav>
    }
}

/// Window listeners: the focus record, the one shortcut handler, F10 / Alt
/// menu access, and closing on an outside press or window blur.
pub fn install() {
    let Some(w) = web_sys::window() else { return };
    let Some(doc) = w.document() else { return };
    let focusin = Closure::<dyn Fn(web_sys::FocusEvent)>::new(|ev: web_sys::FocusEvent| {
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlElement>().ok()) else {
            return;
        };
        if el.closest("#menubar").ok().flatten().is_none() {
            clipboard::note_focus(Some(el));
        }
    });
    let _ = doc.add_event_listener_with_callback("focusin", focusin.as_ref().unchecked_ref());
    focusin.forget();
    // Focus leaving to nothing (a click on the canvas or the background):
    // the record is cleared, so a menu never edits a control the user left.
    // Focus moving into the bar keeps it.
    let focusout = Closure::<dyn Fn(web_sys::FocusEvent)>::new(|ev: web_sys::FocusEvent| {
        let into_bar = ev
            .related_target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .is_some_and(|e| e.closest("#menubar").ok().flatten().is_some());
        let window_blur = web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| !d.has_focus().unwrap_or(true));
        if ev.related_target().is_none() && !into_bar && !window_blur {
            clipboard::note_focus(None);
        }
    });
    let _ = doc.add_event_listener_with_callback("focusout", focusout.as_ref().unchecked_ref());
    focusout.forget();

    let keydown = Closure::<dyn Fn(KeyboardEvent)>::new(|ev: KeyboardEvent| {
        let key = ev.key();
        if key != "Alt" {
            ALT_ALONE.with(|a| a.set(false));
        }
        if !mac()
            && !ev.default_prevented()
            && !ev.is_composing()
            && command::modal_or_drag().is_none()
        {
            // F10: the menu bar (no modifiers).
            if key == "F10" && !ev.alt_key() && !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key()
            {
                ev.prevent_default();
                if bar_has_focus() {
                    close(true);
                } else {
                    focus_bar();
                }
                return;
            }
            if key == "Alt" && !ev.repeat() {
                ALT_ALONE.with(|a| a.set(true));
            }
            // Alt+mnemonic (no Ctrl: Ctrl+Alt is AltGr text).
            if ev.alt_key()
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.get_modifier_state("AltGraph")
            {
                let k = key.to_lowercase();
                if let Some(i) = MENUS.iter().position(|m| m.mnemonic == k) {
                    ev.prevent_default();
                    open_menu(i, false);
                    return;
                }
            }
        }
        command::on_window_keydown(ev);
    });
    let _ = w.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref());
    keydown.forget();

    let keyup = Closure::<dyn Fn(KeyboardEvent)>::new(|ev: KeyboardEvent| {
        if mac() || ev.key() != "Alt" || !ALT_ALONE.with(|a| a.replace(false)) {
            return;
        }
        ev.prevent_default();
        if bar_has_focus() || is_open() {
            close(true);
        } else {
            focus_bar();
        }
    });
    let _ = w.add_event_listener_with_callback("keyup", keyup.as_ref().unchecked_ref());
    keyup.forget();

    // An outside press closes the menu; the press then focuses what it hit.
    let down = Closure::<dyn Fn(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
        if !is_open() {
            return;
        }
        let inside = ev
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .is_some_and(|e| e.closest("#menubar").ok().flatten().is_some());
        if !inside {
            close(false);
        }
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
        "pointerdown",
        down.as_ref().unchecked_ref(),
        &opts,
    );
    down.forget();
    let blur = Closure::<dyn Fn(web_sys::Event)>::new(|_| {
        if is_open() {
            close(false);
        }
    });
    let _ = w.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    blur.forget();
}
