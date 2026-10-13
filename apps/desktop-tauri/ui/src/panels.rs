// SPDX-License-Identifier: GPL-3.0-or-later
//! Workspace panels: Scene Tree, Inspector, Diagnostics, 3D Viewport. All are
//! READ projections of the canonical document computed in Rust.

use leptos::prelude::*;
use leptos::task::spawn_local;

use wrlforge_desktop_protocol as p;
use wrlforge_desktop_protocol::command::CommandId;

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
                <span class="muted" title="USE and ROUTE node names are resolved by the lexical scope graph (ISO 4.6.2, 4.8.4, 4.10.2). ROUTE events and PROTO types are checked by typed analysis (ISO 4.8, 4.10.2). Read-only and advisory.">
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
                    let class = format!("tree-item kind-{}{}", it.kind.to_lowercase(), ref_class(&it.refs, it.typed.as_ref()));
                    let title = typed_title(ref_title(&it.refs, &a.items), it.typed.as_ref());
                    // Offsets of THIS render's revision; tree_select refuses
                    // them unless that is still the document's revision.
                    let id2 = id.clone();
                    let act = move || { ui::tree_select(session, rev, id2.clone(), from, to); };
                    let act2 = act.clone();
                    view! {
                        <li role="treeitem" class=class style=pad title=title data-id=id.clone() tabindex="-1"
                            data-ref=ref_data(&it.refs)
                            data-typed=typed_data(it.typed.as_ref())
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

/// The weakest reference class on an item, typed answer included: "" when
/// proven or not a reference.
fn ref_class(refs: &[p::SceneRef], typed: Option<&p::TypedCheck>) -> &'static str {
    let classes = || {
        refs.iter()
            .map(|r| r.class.as_str())
            .chain(typed.map(|t| t.class.as_str()))
    };
    if classes().any(|c| c == "unresolved") {
        " ref-unresolved"
    } else if classes().any(|c| c == "uncertain") {
        " ref-uncertain"
    } else {
        ""
    }
}

/// `role:status:reason` per reference, for tests and assistive tools.
fn ref_data(refs: &[p::SceneRef]) -> String {
    refs.iter()
        .map(|r| format!("{}:{}:{}", r.role, r.status, r.reason))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `subject:status:reason` of the typed answer, for tests and assistive tools.
fn typed_data(t: Option<&p::TypedCheck>) -> String {
    t.map(|t| format!("{}:{}:{}", t.subject, t.status, t.reason))
        .unwrap_or_default()
}

/// Adds the typed answer to the hover text. Display only.
fn typed_title(base: String, t: Option<&p::TypedCheck>) -> String {
    let Some(t) = t else { return base };
    let what = if t.subject == "route" {
        "ROUTE events"
    } else {
        "Node type"
    };
    let line = match (t.class.as_str(), &t.summary) {
        ("proven", Some(s)) => format!("{what}: proven ({s})"),
        (c, Some(s)) => format!("{what}: {c}: {} ({}; {s})", t.status, t.reason),
        (c, None) => format!("{what}: {c}: {} ({})", t.status, t.reason),
    };
    if base.is_empty() {
        line
    } else {
        format!("{base}\n{line}")
    }
}

/// Hover text. Display only: a target is named, never selected or edited.
fn ref_title(refs: &[p::SceneRef], items: &[p::SceneItem]) -> String {
    let label = |id: &Option<String>| {
        id.as_ref()
            .and_then(|id| items.iter().find(|i| &i.id == id))
            .map(|i| i.label.clone())
    };
    refs.iter()
        .map(|r| {
            let who = match r.role.as_str() {
                "route-source" => "ROUTE source",
                "route-destination" => "ROUTE destination",
                _ => "USE",
            };
            let name = r.name.as_deref().unwrap_or("?");
            let tail = match (
                r.class.as_str(),
                label(&r.target),
                label(&r.language_target),
            ) {
                ("proven", Some(t), _) => format!("proven → {t}"),
                (_, _, Some(l)) if r.status == "ambiguous" => {
                    format!(
                        "ambiguous ({}); VRML97 uses the closest: {l}; not an edit target",
                        r.reason
                    )
                }
                (c, _, _) => format!("{c}: {} ({})", r.status, r.reason),
            };
            format!("{who} {name}: {tail}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Arrow keys move focus between items (no selection change); Enter/Space
/// on an item selects it through the same revision check as a click; Delete
/// deletes the selected node (B2).
fn tree_keydown(ev: web_sys::KeyboardEvent) {
    use wasm_bindgen::JsCast;
    // B2: Delete removes the SELECTED node (the registry command; Rust
    // re-proves it). A modified Delete is not this command.
    if ev.key() == "Delete" && !(ev.ctrl_key() || ev.meta_key() || ev.alt_key() || ev.shift_key()) {
        ev.prevent_default();
        crate::command::run(CommandId::EditDeleteNode);
        return;
    }
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

/// B1: the "Shared PROTO definition" indicator, with the one explicit
/// consent per definition and document.
fn shared_banner(s: p::SharedProto, session: u64) -> impl IntoView {
    let u = ui();
    let proto = s.proto.clone();
    let allowed = {
        let proto = proto.clone();
        move || u.shared_ok.with(|v| v.iter().any(|(x, n)| *x == session && *n == proto))
    };
    let instances = s.instances;
    let text = format!(
        "Shared PROTO definition: this node is part of PROTO {}. It is used by {} instance(s) in this document; a change here changes every one of them.",
        s.proto, instances
    );
    let allow = {
        let s = s.clone();
        move |_: web_sys::MouseEvent| {
            let s = s.clone();
            spawn_local(async move {
                ui::confirm_shared(session, s).await;
            });
        }
    };
    let allowed2 = allowed.clone();
    view! {
        <div class="shared-proto" id="inspector-shared" role="note" data-proto=proto.clone()
            data-allowed=move || allowed().to_string()>
            <p><strong>"Shared PROTO definition"</strong>" · "{text}</p>
            {move || if allowed2() {
                view! { <p class="shared-ok" id="inspector-shared-ok">"Editing the shared definition is allowed for this document."</p> }.into_any()
            } else {
                view! {
                    <button type="button" id="inspector-shared-allow" on:click=allow.clone()
                        title="Allow edits to this PROTO definition (all instances) for this document">
                        "Edit shared definition"
                    </button>
                }.into_any()
            }}
        </div>
    }
}

/// `#rrggbb` for three SFColor components (clamped for display only).
fn color_hex(c: &[p::FieldComponent]) -> String {
    let byte = |t: &str| -> u8 {
        let v: f64 = t.trim().parse().unwrap_or(0.0);
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(&c[0].text),
        byte(&c[1].text),
        byte(&c[2].text)
    )
}

fn parse_hex(v: &str) -> Option<[f64; 3]> {
    let h = v.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|x| x as f64 / 255.0);
    Some([b(0)?, b(2)?, b(4)?])
}

/// A short VRML float for a 0..1 channel (3 decimals, no trailing zeros).
fn color_text(x: f64) -> String {
    let t = format!("{:.3}", (x * 1000.0).round() / 1000.0);
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() { "0".into() } else { t.to_string() }
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
    let shared = n.shared.clone().map(|s| shared_banner(s, session));
    // VISUAL-4A: an Extrusion gets its own sections (authored values and
    // VRML97 defaults); the generic table lists only its other fields.
    let extrusion = n
        .extrusion
        .clone()
        .map(|e| crate::extrusion::panel(e, &n.fields, rev, n.editable));
    let is_x = n.extrusion.is_some();
    let fields: Vec<p::EditableField> = n
        .fields
        .into_iter()
        .filter(|f| !(is_x && crate::extrusion::is_extrusion_field(&f.name)))
        .collect();
    // B2 (Spazz3D Texture page / Flux texture panel): an Appearance with
    // no texture field offers "Add texture…" (choose an image, then ONE edit).
    let node_type = n.node_type.clone().unwrap_or_default();
    let add_texture = (node_type == "Appearance"
        && n.editable
        && !fields.iter().any(|f| f.name == "texture"))
        .then(|| {
            view! {
                <p class="add-texture">
                    <button type="button" class="small" id="inspector-add-texture"
                        title="Choose an image in the document's folder and add texture ImageTexture { url \"…\" } (one edit, one undo step)"
                        disabled=move || ui().revision.get() != rev
                        on:click=move |_| {
                            spawn_local(async move {
                                if ui::add_texture().await {
                                    ui::preview(false).await;
                                }
                            });
                        }>"Add texture…"</button>
                </p>
            }
        });
    let empty = (fields.is_empty() && !is_x).then(|| {
        view! { <p class="empty">"No explicitly authored fields (defaults are not listed)."</p> }
    });
    view! {
        {shared}
        {banner}
        {extrusion}
        {empty}
        {add_texture}
        <table class="fields editable">
            <tbody>
            {fields.into_iter().map(|f| field_row(f, &node_type, session, rev)).collect_view()}
            </tbody>
        </table>
    }
}

fn string_id(field: u32, i: usize) -> String {
    format!("fs-{field}-{i}")
}

/// B2: an MFString list editor. Each entry is edited, removed or (for an
/// `ImageTexture.url`) replaced by a chosen image; a new entry is appended.
/// Every action is ONE request naming the exact list shown; Rust encodes,
/// proves and applies it as one undo step.
fn string_list(
    name: &str,
    field: u32,
    values: Vec<String>,
    image: bool,
    rev: u64,
) -> impl IntoView {
    let stale = move || ui().revision.get() != rev;
    let send = {
        let (name, values) = (name.to_string(), values.clone());
        move |op: p::StringOp| {
            let (name, values) = (name.clone(), values.clone());
            spawn_local(async move {
                if ui::edit_strings(field, name, values, vec![op]).await {
                    ui::preview(false).await;
                }
            });
        }
    };
    let choose = {
        let send = send.clone();
        move |op: fn(u32, String) -> p::StringOp, index: u32| {
            let send = send.clone();
            spawn_local(async move {
                if let Some(url) = ui::choose_texture().await {
                    send(op(index, url));
                }
            });
        }
    };
    let n = values.len();
    let rows = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let id = string_id(field, i);
            let (s_enter, s_set, s_del) = (send.clone(), send.clone(), send.clone());
            let (id_enter, id_set) = (id.clone(), id.clone());
            let pick = image.then(|| {
                let choose = choose.clone();
                view! {
                    <button type="button" class="small" id=format!("fs-{field}-{i}-choose")
                        title="Choose an image in the document's folder (written as a relative url)"
                        disabled=stale
                        on:click=move |_| choose(|index, value| p::StringOp::Set { index, value }, i as u32)>
                        "Choose image…"</button>
                }
            });
            view! {
                <li data-entry=i.to_string()>
                    <input id=id class="text" type="text" spellcheck="false" prop:value=v.clone()
                        aria-label=format!("{name} entry {}", i + 1)
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Enter" {
                                ev.prevent_default();
                                if let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&id_enter) {
                                    s_enter(p::StringOp::Set { index: i as u32, value: inp.value() });
                                }
                            }
                        } />
                    <button type="button" class="small" id=format!("fs-{field}-{i}-set") title="Apply this entry"
                        disabled=stale
                        on:click=move |_| {
                            if let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&id_set) {
                                s_set(p::StringOp::Set { index: i as u32, value: inp.value() });
                            }
                        }>"Apply"</button>
                    {pick}
                    <button type="button" class="small" id=format!("fs-{field}-{i}-del") title="Remove this entry"
                        disabled=stale
                        on:click=move |_| s_del(p::StringOp::Delete { index: i as u32 })>"Remove"</button>
                </li>
            }
        })
        .collect_view();
    let new_id = format!("fs-{field}-new");
    let (s_add, s_enter, id_add, id_enter) = (send.clone(), send.clone(), new_id.clone(), new_id.clone());
    let pick_new = image.then(|| {
        let choose = choose.clone();
        view! {
            <button type="button" class="small" id=format!("fs-{field}-new-choose")
                title="Choose an image in the document's folder and append it"
                disabled=stale
                on:click=move |_| choose(|index, value| p::StringOp::Insert { index, value }, n as u32)>
                "Choose image…"</button>
        }
    });
    view! {
        <div class="strings" data-count=n.to_string()>
            <ol class="string-list">{rows}</ol>
            <div class="string-add">
                <input id=new_id class="text" type="text" spellcheck="false" placeholder="new entry"
                    aria-label=format!("{name}: new entry")
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            if let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&id_enter) {
                                s_enter(p::StringOp::Insert { index: n as u32, value: inp.value() });
                                inp.set_value("");
                            }
                        }
                    } />
                <button type="button" class="small" id=format!("fs-{field}-add") title="Append this entry"
                    disabled=stale
                    on:click=move |_| {
                        if let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&id_add) {
                            s_add(p::StringOp::Insert { index: n as u32, value: inp.value() });
                            inp.set_value("");
                        }
                    }>"Add"</button>
                {pick_new}
            </div>
        </div>
    }
}

fn field_row(f: p::EditableField, node_type: &str, session: u64, rev: u64) -> impl IntoView {
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
    let value = if let (true, Some(list)) = (f.editable, f.list.clone()) {
        // VISUAL-4A: an editable MFVec2f / MFVec3f / MFRotation list.
        crate::extrusion::list_table(&f.name, f.index, &list, rev).into_any()
    } else if let (true, Some(strings)) = (f.editable, f.strings.clone()) {
        // B2: an editable MFString list (url, info, ...).
        let image = node_type == "ImageTexture" && f.name == "url";
        string_list(&f.name, f.index, strings.values, image, rev).into_any()
    } else if f.editable {
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
        // B2 (Spazz3D/Flux material editing): an SFColor gets a color
        // swatch. Picking a color fills the three components and applies
        // them as ONE edit; Rust still validates every value.
        let swatch = (f.field_type.as_deref() == Some("SFColor") && arity == 3).then(|| {
            let hex = color_hex(&f.components);
            let (name, kind) = (f.name.clone(), kind.clone());
            view! {
                <input type="color" class="swatch" id=format!("fe-color-{idx}")
                    title="Pick a color (applies as one edit, one undo step)"
                    prop:value=hex
                    disabled=move || ui().revision.get() != rev
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        let Some(rgb) = parse_hex(&v) else { return };
                        for (c, x) in rgb.iter().enumerate() {
                            if let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&input_id(idx, c)) {
                                inp.set_value(&color_text(*x));
                            }
                        }
                        submit(idx, name.clone(), kind.clone(), 3);
                    }/>
            }
        });
        view! {
            <div class="controls">
                {swatch}
                {controls}
                <button class="small" id=format!("fe-apply-{idx}")
                    disabled=move || ui().revision.get() != rev
                    on:click=move |_| submit(idx, name.clone(), kind2.clone(), arity)>"Apply"</button>
            </div>
        }
        .into_any()
    } else {
        let open = f.node_item.clone().map(|id| {
            let label = format!("Open {} ›", f.name);
            view! {
                <button class="small node-open" data-item=id.clone()
                    title="Select this node in the Scene Tree and show its fields"
                    on:click=move |_| {
                        let id = id.clone();
                        spawn_local(async move {
                            if let Err(e) = ui::pick_select(session, rev, id).await {
                                ui::flash(&format!("Cannot open the node: {e}."));
                            }
                        });
                    }>{label}</button>
            }
        });
        view! {
            <div>
                {open}
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
            {move || u.analysis.with(|a| a.as_ref().filter(|a| !a.scope_diagnostics.is_empty()).map(|a| {
                let (session, rev) = (a.session, a.revision);
                view! {
                    <div class="diag-scope" id="diag-scope" data-revision=rev.to_string()
                        title="Advisory scope findings: node names SCOPE0xx (ISO 4.6.2, 4.8.4, 4.10.2) and typed references SCOPE1xx (ISO 4.8, Table 4.4, 4.10.2). Never block Save; uncertain cases are not listed.">
                        <div class="diag-head">{format!("Scope (advisory) · rev {rev}")}</div>
                        <ul>
                            {a.scope_diagnostics.iter().take(200).map(|d| {
                                let (f, t) = (d.view_from, d.view_to);
                                view! {
                                    <li class=format!("diag {}", d.severity) data-code=d.code.clone()
                                        on:click=move |_| { ui::select_from("Diagnostics", session, rev, f, t); }>
                                        <span class="sev">{severity_label(&d.severity)}</span>
                                        {format!("{}:{} {} {}", d.line, d.column, d.code, d.message)}
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
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
                    title="X_ITE is a JavaScript renderer hosted through a narrow adapter">"X_ITE 16.5.0 (JavaScript) · temporary"</span>
                // NATIVE-RENDER-1 (hidden): the native viewport is the pane
                // beside this window. Click selects, drag orbits, wheel zooms,
                // Escape clears, Home resets the view. Selection only.
                <span class="badge native-badge" hidden=move || !u.native.with(|s| s.native_shown())
                    title=move || u.native.with(|s| s.info.clone().unwrap_or_default())>"Native (wgpu) · test · beside →"</span>
                <button class="secondary small" hidden=move || !u.native.with(|s| s.native_shown())
                    title="Reset the native view to frame the scene (Home)"
                    on:click=move |_| crate::native::reset_camera()>"Reset view"</button>
                // UI-MENU-1: the same registry commands as View → Live
                // Preview / Update Preview.
                <label class="toggle" title=crate::command::title(CommandId::ViewLivePreview)>
                    <input type="checkbox" id="chk-live" data-command=CommandId::ViewLivePreview.key()
                        prop:checked=move || u.preview_enabled.get()
                        on:change=move |ev| {
                            if event_target_checked(&ev) != u.preview_enabled.get_untracked() {
                                crate::command::run(CommandId::ViewLivePreview);
                            }
                        } />
                    "Live"
                </label>
                <button class="secondary small" id="btn-update-preview" data-command=CommandId::ViewUpdatePreview.key()
                    title=crate::command::title(CommandId::ViewUpdatePreview)
                    disabled=move || !crate::command::enabled(CommandId::ViewUpdatePreview)
                    on:click=move |_| crate::command::run(CommandId::ViewUpdatePreview)>"Update"</button>
                <span class="sep" hidden=move || u.native.with(|s| s.native_shown())></span>
                // UI-DOCK-1: the four tools wrap as one group (never clipped).
                <span class="tool-group" role="group" aria-label="Viewport tools">
                // VISUAL-2: an explicit selection mode. A click selects the
                // exact authored object; a drag still moves the camera.
                <button id="btn-select" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolSelect.key() title=crate::command::title(CommandId::ToolSelect)
                    aria-pressed=move || if u.pick_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolSelect)
                    on:click=move |_| crate::command::run(CommandId::ToolSelect)>"Select"</button>
                // VISUAL-3A: the Move tool. Drag an axis handle to move the
                // selected top-level Transform; a click still selects, a drag
                // elsewhere still moves the camera.
                <button id="btn-move" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolMove.key() title=crate::command::title(CommandId::ToolMove)
                    aria-pressed=move || if u.move_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolMove)
                    on:click=move |_| crate::command::run(CommandId::ToolMove)>"Move"</button>
                // VISUAL-4B: Rotate (rings about the world axes through the
                // object's center) and Scale (its own scale axes + uniform).
                <button id="btn-rotate" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolRotate.key() title=crate::command::title(CommandId::ToolRotate)
                    aria-pressed=move || if u.rotate_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolRotate)
                    on:click=move |_| crate::command::run(CommandId::ToolRotate)>"Rotate"</button>
                <button id="btn-scale" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolScale.key() title=crate::command::title(CommandId::ToolScale)
                    aria-pressed=move || if u.scale_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolScale)
                    on:click=move |_| crate::command::run(CommandId::ToolScale)>"Scale"</button>
                </span>
            </div>
            <div class="viewport-stage" hidden=move || u.native.with(|s| s.native_shown())>
                <x3d-canvas id="viewport" class:picking=move || u.pick_mode.get() || u.move_mode.get() || u.rotate_mode.get() || u.scale_mode.get() splashScreen="false" contextMenu="false" notifications="false"
                    timings="false" cache="false"></x3d-canvas>
                // B2: the selection outline (view state only), under the
                // gizmo. Drawn from the renderer's camera by `highlight.rs`;
                // it never takes pointer input.
                <svg id="selection-outline" class="selection-outline" data-state="hidden" aria-hidden="true">
                    <path id="hl-halo" class="hl-halo"></path>
                    <path id="hl-line" class="hl-line"></path>
                </svg>
                // B2: why the selection has no outline. Floats over the
                // canvas so it never changes the viewport's size.
                <div class="highlight-status" id="highlight-status" role="status" aria-live="polite"
                    hidden=move || u.highlight_message.with(|m| m.is_none())>
                    {move || u.highlight_message.get().unwrap_or_default()}
                </div>
                // The gizmo overlay. Drawn from the renderer's camera by
                // `gizmo.rs`; only the handles take pointer input.
                <svg id="gizmo" class="gizmo" data-state="hidden" data-tool="move" role="group" aria-label="Manipulation handles">
                    {[("x", "X"), ("y", "Y"), ("z", "Z")].into_iter().map(|(a, l)| view! {
                        <g id=format!("gz-r{a}") class="gz-handle gz-ring" data-axis=a role="button" aria-disabled="true"
                            aria-label=format!("Rotate about {l}: drag")>
                            <polyline id=format!("gz-r{a}-hit") class="gz-hit"></polyline>
                            <polyline id=format!("gz-r{a}-halo") class="gz-halo"></polyline>
                            <polyline id=format!("gz-r{a}-line") class="gz-line"></polyline>
                        </g>
                    }).collect_view()}
                    {[("x", "X"), ("y", "Y"), ("z", "Z")].into_iter().map(|(a, l)| view! {
                        <g id=format!("gz-{a}") class="gz-handle gz-axis" data-axis=a role="button" aria-disabled="true"
                            aria-label=format!("Move or scale along {l}: drag")>
                            <line id=format!("gz-{a}-hit") class="gz-hit"></line>
                            <line id=format!("gz-{a}-halo") class="gz-halo"></line>
                            <line id=format!("gz-{a}-line") class="gz-line"></line>
                            <circle id=format!("gz-{a}-tip") class="gz-tip" r="7"></circle>
                            <rect id=format!("gz-{a}-box") class="gz-box" width="12" height="12"></rect>
                        </g>
                    }).collect_view()}
                    <g id="gz-u" class="gz-handle gz-uniform" role="button" aria-disabled="true" aria-label="Scale uniformly: drag up or down">
                        <rect id="gz-u-hit" class="gz-hit" width="24" height="24"></rect>
                        <rect id="gz-u-box" class="gz-box" width="14" height="14"></rect>
                    </g>
                    <circle id="gz-origin" class="gz-origin" r="4"></circle>
                </svg>
            </div>
            <div class="preview-status" id="preview-status">{move || format!("Preview: {}", u.preview_status.get())}</div>
            <div class="preview-status texture-warning" id="texture-warnings" role="status" aria-live="polite"
                hidden=move || u.texture_warnings.with(|w| w.is_empty())
                title=move || u.texture_warnings.with(|w| w.iter().map(|t| format!("line {} · {}: {}", t.line, t.node, t.detail)).collect::<Vec<_>>().join("\n"))>
                {move || u.texture_warnings.with(|w| match w.first() {
                    None => String::new(),
                    Some(t) => format!("⚠ {} texture{} not loaded · line {} · {}: {}{}", w.len(), if w.len() == 1 { "" } else { "s" },
                        t.line, t.node, t.detail, if w.len() > 1 { " · hover for all" } else { "" }),
                })}
            </div>
            // XITE-16-UPGRADE-2: Rust diagnostics, not the picture, decide this.
            <div class="preview-status route-warning" id="route-warning" role="status" aria-live="polite"
                hidden=move || u.route_warnings.with(|w| w.is_empty()) || u.native.with(|s| s.native_shown())
                title=move || u.route_warnings.with(|w| w.iter().map(|r| format!("line {} · {}", r.line, r.detail)).collect::<Vec<_>>().join("\n"))>
                {move || u.route_warnings.with(|w| match w.first() {
                    None => String::new(),
                    Some(r) => format!("⚠ The preview may be incomplete because one or more ROUTEs are invalid. · line {}{}",
                        r.line, if w.len() > 1 { format!(" · {} ROUTEs · hover for all", w.len()) } else { String::new() }),
                })}
            </div>
            <div class="preview-status native-status" id="native-status" hidden=move || !u.native.with(|s| s.requested && s.reason.is_some())>
                {move || u.native.with(|s| s.reason.clone().unwrap_or_default())}
            </div>
            <div class="pick-status gizmo-status" id="gizmo-status" role="status" aria-live="polite"
                data-kind=move || u.gizmo_message.with(|m| m.as_ref().map(|m| m.0.clone()).unwrap_or_default())
                hidden=move || !(u.move_mode.get() || u.rotate_mode.get() || u.scale_mode.get()) || u.gizmo_message.with(|m| m.is_none())>
                {move || u.gizmo_message.with(|m| m.as_ref().map(|m| m.1.clone()).unwrap_or_default())}
                // VISUAL-4B: the explicit authoring action for a missing
                // rotation / scale field (one undo step, only on request).
                {move || {
                    u.gizmo_message.track();
                    crate::gizmo::GIZMO.with_borrow(|g| g.missing_field).map(|f| view! {
                        " "
                        <button id="gz-write-field" class="secondary small"
                            title=format!("Write the VRML97 default {f} value into this Transform (one undo step)")
                            on:click=move |_| spawn_local(crate::gizmo::write_missing_field())>
                            {format!("Write {f}")}
                        </button>
                    })
                }}
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

/// UI-MENU-1: the Select tool toggle (toolbar button, Tools menu). Moved
/// unchanged from the button's handler: one viewport tool at a time.
pub fn toggle_select() {
    let u = ui();
    u.pick_mode.update(|m| *m = !*m);
    if u.pick_mode.get_untracked() {
        u.move_mode.set(false);
        u.rotate_mode.set(false);
        u.scale_mode.set(false);
    }
    if !u.pick_mode.get_untracked() {
        u.pick_message.set(None);
    }
}

/// UI-MENU-1: the Move / Rotate / Scale toggle (toolbar button, Tools menu).
/// Moved unchanged from the three button handlers.
pub fn toggle_gizmo(tool: wrlforge_desktop_protocol::gizmo::Tool) {
    use wrlforge_desktop_protocol::gizmo::Tool;
    let u = ui();
    let (mine, others) = match tool {
        Tool::Move => (u.move_mode, [u.rotate_mode, u.scale_mode]),
        Tool::Rotate => (u.rotate_mode, [u.move_mode, u.scale_mode]),
        Tool::Scale => (u.scale_mode, [u.move_mode, u.rotate_mode]),
    };
    mine.update(|m| *m = !*m);
    if mine.get_untracked() {
        u.pick_mode.set(false);
        for o in others {
            o.set(false);
        }
        crate::gizmo::start();
    } else {
        u.gizmo_message.set(None);
    }
}
