// SPDX-License-Identifier: GPL-3.0-or-later
//! Workspace panels: Scene Tree, Inspector, Diagnostics, 3D Viewport. All are
//! READ projections of the canonical document computed in Rust.

use leptos::prelude::*;
use leptos::task::spawn_local;

use wrlforge_desktop_protocol as p;

use crate::ui::{self, ui};

#[component]
pub fn SceneTree() -> impl IntoView {
    let u = ui();
    // Only the items whose (revision, id) key flips re-render on a selection
    // change -- not every item on every keystroke.
    let sel = Selector::new(move || {
        u.selected
            .with(|s| s.as_ref().map(|s| (s.revision, s.id.clone())))
    });
    // The tree is "current" only when its analysis is of the revision the
    // document is at; otherwise its offsets are of older text.
    let stale = move || {
        u.analysis
            .with(|a| a.as_ref().is_some_and(|a| a.revision != u.revision.get()))
    };
    view! {
        <aside class="tree-col" aria-label="Scene Tree">
            <div class="pane-title">"Scene Tree"
                <span class="muted" title="USE resolution uses the flat, non-authoritative scope; the WD1.5 scope graph is not migrated yet">
                    {move || u.analysis.with(|a| a.as_ref().map(|a| format!(" · {} items · {} scope", a.items.len(), a.resolution_scope)).unwrap_or_default())}
                </span>
                <span class="tree-updating" id="tree-updating">{move || stale().then_some(" · updating…")}</span>
            </div>
            // The analysis revision these items (and their offsets) belong to.
            <ul class="tree" role="tree" aria-label="Scene items" tabindex="0"
                class:stale=stale
                aria-busy=move || if stale() { "true" } else { "false" }
                data-stale=move || if stale() { "true" } else { "false" }
                data-revision=move || u.analysis.with(|a| a.as_ref().map(|a| a.revision.to_string()).unwrap_or_default())
                on:keydown=tree_keydown>
                {let sel = sel.clone(); move || u.analysis.with(|a| a.as_ref().map(|a| {
                    let (session, rev) = (a.session, a.revision);
                    // Keys of the previous render are gone with its items.
                    sel.clear();
                    a.items.iter().map(|it| {
                    let (sel, sel2) = (sel.clone(), sel.clone());
                    let id = it.id.clone();
                    let key = Some((rev, it.id.clone()));
                    let key2 = key.clone();
                    let (from, to) = (it.view_from, it.view_to);
                    let pad = format!("padding-left: {}rem", 0.4 + (it.depth.saturating_sub(1)) as f32 * 0.9);
                    let class = format!("tree-item kind-{}{}", it.kind.to_lowercase(),
                        if it.use_status.as_deref() == Some("unresolved") { " unresolved" } else { "" });
                    let title = it.use_status.clone().map(|s| format!("USE {s} (flat scope, non-authoritative)")).unwrap_or_default();
                    // Offsets of THIS render's revision; tree_select refuses
                    // them unless that is still the document's revision.
                    let id2 = id.clone();
                    let act = move || { ui::tree_select(session, rev, id2.clone(), from, to); };
                    let act2 = act.clone();
                    view! {
                        <li role="treeitem" class=class style=pad title=title data-id=id.clone() tabindex="-1"
                            class:selected=move || sel.selected(&key)
                            aria-selected=move || if sel2.selected(&key2) { "true" } else { "false" }
                            on:click=move |_| act()
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Enter" || ev.key() == " " {
                                    ev.prevent_default();
                                    act2();
                                }
                            }>
                            <span class="kind">{it.kind.clone()}</span>
                            <span class="label">{it.label.clone()}</span>
                        </li>
                    }
                }).collect_view()}))}
            </ul>
            {move || (u.doc.with(|d| d.is_none())).then(|| view! { <p class="empty">"Start a New World or open a file to see its scene."</p> })}
        </aside>
    }
}

/// Arrow keys move focus between items (no selection change); Enter/Space
/// on an item selects it through the same revision check as a click.
fn tree_keydown(ev: web_sys::KeyboardEvent) {
    use wasm_bindgen::JsCast;
    let down = match ev.key().as_str() {
        "ArrowDown" => true,
        "ArrowUp" => false,
        _ => return,
    };
    let Some(target) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return;
    };
    let next = if target.tag_name().eq_ignore_ascii_case("ul") {
        if down {
            target.first_element_child()
        } else {
            target.last_element_child()
        }
    } else if down {
        target.next_element_sibling()
    } else {
        target.previous_element_sibling()
    };
    if let Some(n) = next.and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
        ev.prevent_default();
        let _ = n.focus();
    }
}

#[component]
pub fn Inspector() -> impl IntoView {
    let u = ui();
    view! {
        <section class="inspector" aria-label="Inspector">
            <div class="pane-title">"Inspector"
                <span class="badge" title="Field edits are planned, validated and applied by Rust (wrlforge-vrml field_edit)">
                    "SF fields editable · Rust-validated"
                </span>
            </div>
            {move || u.field_error.get().map(|e| view! {
                <p class="field-error" id="inspector-error" role="alert">{e}</p>
            })}
            {move || match u.inspection.get() {
                None => view! { <p class="empty">"Select a Scene Tree item."</p> }.into_any(),
                Some(i) => {
                    let title = i.title.clone();
                    let rev = i.revision;
                    let session = crate::editor::CORE.with_borrow(|c| c.session).unwrap_or(0);
                    let body = match i.node {
                        Some(n) => node_fields(n, session, rev).into_any(),
                        None => rows(i.rows, session, rev).into_any(),
                    };
                    view! {
                        <div data-revision=rev.to_string()>
                            {move || (u.revision.get() != rev).then(|| view! {
                                <p class="inspector-stale" id="inspector-stale" role="status">
                                    {format!("Updating… these fields are of revision {rev}.")}
                                </p>
                            })}
                            <h3 id="inspector-title">{title}</h3>
                            {body}
                        </div>
                    }.into_any()
                }
            }}
        </section>
    }
}

fn rows(rows: Vec<p::InspectorRow>, session: u64, rev: u64) -> impl IntoView {
    view! {
        <table class="fields">
            <tbody>
            {rows.into_iter().map(|r| {
                let (f, t) = (r.view_from, r.view_to);
                view! {
                    <tr on:click=move |_| { ui::select_from("Inspector", session, rev, f, t); }>
                        <th>{r.name}</th>
                        <td class="kind">{r.kind}</td>
                        <td><code>{r.source}{if r.elided { "…" } else { "" }}</code></td>
                    </tr>
                }
            }).collect_view()}
            </tbody>
        </table>
    }
}

fn input_id(field: u32, component: usize) -> String {
    format!("fe-{field}-{component}")
}

/// Read the field's controls and hand the RAW values to Rust. No value is
/// parsed, clamped or validated here: Rust is the only validator.
fn submit(field: u32, name: String, kind: String, arity: usize) {
    let mut out = Vec::with_capacity(arity);
    for c in 0..arity {
        let id = input_id(field, c);
        let v = if kind == "bool" {
            let Some(sel) = crate::element_by_id::<web_sys::HtmlSelectElement>(&id) else {
                return;
            };
            p::FieldInput::Bool {
                value: sel.value() == "TRUE",
            }
        } else {
            let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&id) else {
                return;
            };
            p::FieldInput::Text { value: inp.value() }
        };
        out.push(v);
    }
    spawn_local(ui::edit_field(field, name, out));
}

fn node_fields(n: p::NodeFields, session: u64, rev: u64) -> impl IntoView {
    let banner = (!n.editable).then(|| {
        view! {
            <p class="readonly-note" id="inspector-readonly">
                {format!("Read-only node: {}", n.reason)}
            </p>
        }
    });
    let empty = n.fields.is_empty().then(|| {
        view! { <p class="empty">"No explicitly authored fields (defaults are not listed)."</p> }
    });
    view! {
        {banner}
        {empty}
        <table class="fields editable">
            <tbody>
            {n.fields.into_iter().map(|f| field_row(f, session, rev)).collect_view()}
            </tbody>
        </table>
    }
}

fn field_row(f: p::EditableField, session: u64, rev: u64) -> impl IntoView {
    let (from, to) = (f.view_from, f.view_to);
    let ty = f.field_type.clone().unwrap_or_else(|| "?".into());
    let title = [
        f.declaration.clone(),
        f.bounds.clone().map(|b| format!("range {b}")),
        f.constraint_note.clone().map(|n| format!("note {n}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let value = if f.editable {
        let idx = f.index;
        let kind = f.kind.clone().unwrap_or_default();
        let arity = f.components.len();
        let controls = f
            .components
            .iter()
            .enumerate()
            .map(|(c, comp)| {
                let id = input_id(idx, c);
                let label = comp.label.clone();
                if kind == "bool" {
                    let on = comp.bool_value == Some(true);
                    view! {
                        <label class="comp">{label}
                            <select id=id prop:value=if on { "TRUE" } else { "FALSE" }>
                                <option value="TRUE" selected=on>"TRUE"</option>
                                <option value="FALSE" selected=!on>"FALSE"</option>
                            </select>
                        </label>
                    }
                    .into_any()
                } else {
                    let (name, kind) = (f.name.clone(), kind.clone());
                    let class = if kind == "string" { "text" } else { "num" };
                    view! {
                        <label class="comp">{label}
                            <input id=id class=class type="text" spellcheck="false" prop:value=comp.text.clone()
                                on:keydown=move |ev: web_sys::KeyboardEvent| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        submit(idx, name.clone(), kind.clone(), arity);
                                    }
                                } />
                        </label>
                    }
                    .into_any()
                }
            })
            .collect_view();
        let (name, kind2) = (f.name.clone(), kind.clone());
        view! {
            <div class="controls">
                {controls}
                <button class="small" id=format!("fe-apply-{idx}")
                    disabled=move || ui().revision.get() != rev
                    on:click=move |_| submit(idx, name.clone(), kind2.clone(), arity)>"Apply"</button>
            </div>
        }
        .into_any()
    } else {
        view! {
            <div>
                <code>{f.value_excerpt.clone()}</code>
                <span class="reason" title="Why this field is read-only">{f.reason.clone()}</span>
            </div>
        }
        .into_any()
    };
    view! {
        <tr class:ro=!f.editable data-field=f.name.clone()>
            <th title=title on:click=move |_| { ui::select_from("Inspector", session, rev, from, to); }>{f.name.clone()}</th>
            <td class="kind">{ty}</td>
            <td>{value}</td>
        </tr>
    }
}

#[component]
pub fn Diagnostics() -> impl IntoView {
    let u = ui();
    view! {
        <div class="diagnostics" aria-label="Diagnostics">
            // Diagnostics and syntax colors come from the same Rust parse;
            // say which revision that was.
            {move || u.analysis.with(|a| a.as_ref().map(|a| {
                let current = a.revision == u.revision.get();
                view! {
                    <div class="diag-head" id="diag-head" data-revision=a.revision.to_string()>
                        {format!("Parser · rev {}", a.revision)}
                        {(!current).then_some(" · updating…")}
                    </div>
                }
            }))}
            {move || u.analysis.with(|a| match a {
                None => view! { <span class="muted">"Diagnostics: —"</span> }.into_any(),
                Some(a) if a.diagnostics.is_empty() => view! { <span class="ok">"✓ No parser diagnostics (native Rust parser)"</span> }.into_any(),
                Some(a) => view! {
                    <ul>
                        {a.diagnostics.iter().take(200).map(|d| {
                            let (f, t) = (d.view_from, d.view_to);
                            let (session, rev) = (a.session, a.revision);
                            view! {
                                <li class=format!("diag {}", d.severity)
                                    on:click=move |_| { ui::select_from("Diagnostics", session, rev, f, t); }>
                                    // Severity in words too: never color alone.
                                    <span class="sev">{severity_label(&d.severity)}</span>
                                    {format!("{}:{} {} {}", d.line, d.column, d.code, d.message)}
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            })}
        </div>
    }
}

fn severity_label(s: &str) -> String {
    match s {
        "error" => "Error".into(),
        "warning" => "Warning".into(),
        "info" => "Info".into(),
        other => other.to_string(),
    }
}

#[component]
pub fn Viewport() -> impl IntoView {
    let u = ui();
    view! {
        <section class="viewport" class:native=move || u.native.with(|s| s.native_shown()) aria-label="3D Viewport">
            <div class="pane-title">"3D Viewport"
                <span class="badge" hidden=move || u.native.with(|s| s.native_shown())
                    title="X_ITE is a JavaScript renderer hosted through a narrow adapter">"X_ITE 15.1.10 (JavaScript) · temporary"</span>
                // NATIVE-RENDER-1 (hidden): the native viewport is the pane
                // beside this window. Click selects, drag orbits, wheel zooms,
                // Escape clears, Home resets the view. Selection only.
                <span class="badge native-badge" hidden=move || !u.native.with(|s| s.native_shown())
                    title=move || u.native.with(|s| s.info.clone().unwrap_or_default())>"Native (wgpu) · test · beside →"</span>
                <button class="secondary small" hidden=move || !u.native.with(|s| s.native_shown())
                    title="Reset the native view to frame the scene (Home)"
                    on:click=move |_| crate::native::reset_camera()>"Reset view"</button>
                <label class="toggle">
                    <input type="checkbox" prop:checked=move || u.preview_enabled.get()
                        on:change=move |ev| u.preview_enabled.set(event_target_checked(&ev)) />
                    "Live"
                </label>
                <button class="secondary small" on:click=move |_| spawn_local(ui::preview(true))>"Update"</button>
                <span class="sep" hidden=move || u.native.with(|s| s.native_shown())></span>
                // VISUAL-2: an explicit selection mode. A click selects the
                // exact authored object; a drag still moves the camera.
                <button id="btn-select" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    title="Select: click an object in the viewport to select its exact source node (drag still moves the camera)"
                    aria-pressed=move || if u.pick_mode.get() { "true" } else { "false" }
                    disabled=move || u.doc.with(|d| d.is_none())
                    on:click=move |_| {
                        u.pick_mode.update(|m| *m = !*m);
                        if u.pick_mode.get_untracked() { u.move_mode.set(false); }
                        if !u.pick_mode.get_untracked() { u.pick_message.set(None); }
                    }>"Select"</button>
                // VISUAL-3A: the Move tool. Drag an axis handle to move the
                // selected top-level Transform; a click still selects, a drag
                // elsewhere still moves the camera.
                <button id="btn-move" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    title="Move: drag the X (red), Y (green) or Z (blue) handle of the selected object. Esc cancels a drag; the Inspector takes exact values."
                    aria-pressed=move || if u.move_mode.get() { "true" } else { "false" }
                    disabled=move || u.doc.with(|d| d.is_none())
                    on:click=move |_| {
                        u.move_mode.update(|m| *m = !*m);
                        if u.move_mode.get_untracked() {
                            u.pick_mode.set(false);
                            crate::gizmo::start();
                        } else {
                            u.gizmo_message.set(None);
                        }
                    }>"Move"</button>
            </div>
            <div class="viewport-stage" hidden=move || u.native.with(|s| s.native_shown())>
                <x3d-canvas id="viewport" class:picking=move || u.pick_mode.get() || u.move_mode.get() splashScreen="false" contextMenu="false" notifications="false"
                    timings="false" cache="false"></x3d-canvas>
                // The gizmo overlay. Drawn from the renderer's camera by
                // `gizmo.rs`; only the handles take pointer input.
                <svg id="gizmo" class="gizmo" data-state="hidden" role="group" aria-label="Translation handles">
                    {[("x", "X"), ("y", "Y"), ("z", "Z")].into_iter().map(|(a, l)| view! {
                        <g id=format!("gz-{a}") class="gz-handle" data-axis=a role="button" aria-disabled="true"
                            aria-label=format!("Move along {l}: drag")>
                            <line id=format!("gz-{a}-hit") class="gz-hit"></line>
                            <line id=format!("gz-{a}-halo") class="gz-halo"></line>
                            <line id=format!("gz-{a}-line") class="gz-line"></line>
                            <circle id=format!("gz-{a}-tip") class="gz-tip" r="7"></circle>
                        </g>
                    }).collect_view()}
                    <circle id="gz-origin" class="gz-origin" r="4"></circle>
                </svg>
            </div>
            <div class="preview-status" id="preview-status">{move || format!("Preview: {}", u.preview_status.get())}</div>
            <div class="preview-status native-status" id="native-status" hidden=move || !u.native.with(|s| s.requested && s.reason.is_some())>
                {move || u.native.with(|s| s.reason.clone().unwrap_or_default())}
            </div>
            <div class="pick-status gizmo-status" id="gizmo-status" role="status" aria-live="polite"
                data-kind=move || u.gizmo_message.with(|m| m.as_ref().map(|m| m.0.clone()).unwrap_or_default())
                hidden=move || !u.move_mode.get() || u.gizmo_message.with(|m| m.is_none())>
                {move || u.gizmo_message.with(|m| m.as_ref().map(|m| m.1.clone()).unwrap_or_default())}
            </div>
            <div class="pick-status" id="pick-status" role="status" aria-live="polite"
                data-kind=move || u.pick_message.with(|m| m.as_ref().map(|m| m.0.clone()).unwrap_or_default())
                hidden=move || u.pick_message.with(|m| m.is_none())>
                {move || u.pick_message.with(|m| m.as_ref().map(|m| m.1.clone()).unwrap_or_default())}
            </div>
        </section>
    }
}

/// Scroll the Scene Tree so item `id` is visible (a viewport selection may
/// name an item far from the current scroll position). Display only.
pub fn reveal_tree_item(id: &str) {
    use wasm_bindgen::JsCast;
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(Some(li)) = doc.query_selector(&format!("li.tree-item[data-id=\"{id}\"]")) else {
        return;
    };
    let (Ok(li), Some(Ok(ul))) = (
        li.dyn_into::<web_sys::HtmlElement>(),
        doc.query_selector("ul.tree")
            .ok()
            .flatten()
            .map(|u| u.dyn_into::<web_sys::HtmlElement>()),
    ) else {
        return;
    };
    let (top, h) = (li.offset_top() - ul.offset_top(), li.offset_height());
    let (st, vh) = (ul.scroll_top(), ul.client_height());
    if top < st || top + h > st + vh {
        ul.set_scroll_top((top - vh / 3).max(0));
    }
}
