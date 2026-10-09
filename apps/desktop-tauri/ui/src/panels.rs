// SPDX-License-Identifier: GPL-3.0-or-later
//! Workspace panels: Scene Tree, Inspector, Diagnostics, 3D Viewport. All are
//! READ projections of the canonical document computed in Rust.

use leptos::prelude::*;
use leptos::task::spawn_local;

use wrlforge_desktop_protocol as p;

use crate::editor;
use crate::ui::{self, ui};

#[component]
pub fn SceneTree() -> impl IntoView {
    let u = ui();
    view! {
        <aside class="tree-col" aria-label="Scene Tree">
            <div class="pane-title">"Scene Tree"
                <span class="muted" title="USE resolution uses the flat, non-authoritative scope; the WD1.5 scope graph is not migrated yet">
                    {move || u.analysis.with(|a| a.as_ref().map(|a| format!(" · {} items · {} scope", a.items.len(), a.resolution_scope)).unwrap_or_default())}
                </span>
            </div>
            <ul class="tree" role="tree" aria-label="Scene items">
                {move || u.analysis.with(|a| a.as_ref().map(|a| a.items.iter().map(|it| {
                    let id = it.id.clone();
                    let id_sel = it.id.clone();
                    let (from, to) = (it.view_from, it.view_to);
                    let pad = format!("padding-left: {}rem", 0.4 + (it.depth.saturating_sub(1)) as f32 * 0.9);
                    let class = format!("tree-item kind-{}{}", it.kind.to_lowercase(),
                        if it.use_status.as_deref() == Some("unresolved") { " unresolved" } else { "" });
                    let title = it.use_status.clone().map(|s| format!("USE {s} (flat scope, non-authoritative)")).unwrap_or_default();
                    view! {
                        <li role="treeitem" class=class style=pad title=title data-id=id.clone()
                            class:selected={let id_sel = id_sel.clone(); move || u.selected.get().as_deref() == Some(id_sel.as_str())}
                            aria-selected=move || if u.selected.get().as_deref() == Some(id_sel.as_str()) { "true" } else { "false" }
                            on:click=move |_| {
                                editor::select(from, to);
                                let id = id.clone();
                                spawn_local(ui::inspect(id));
                            }>
                            <span class="kind">{it.kind.clone()}</span>
                            <span class="label">{it.label.clone()}</span>
                        </li>
                    }
                }).collect_view()))}
            </ul>
            {move || (u.doc.with(|d| d.is_none())).then(|| view! { <p class="empty">"Open a file to see its scene."</p> })}
        </aside>
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
                    let body = match i.node {
                        Some(n) => node_fields(n).into_any(),
                        None => rows(i.rows).into_any(),
                    };
                    view! {
                        <div>
                            <h3 id="inspector-title">{title}</h3>
                            {body}
                        </div>
                    }.into_any()
                }
            }}
        </section>
    }
}

fn rows(rows: Vec<p::InspectorRow>) -> impl IntoView {
    view! {
        <table class="fields">
            <tbody>
            {rows.into_iter().map(|r| {
                let (f, t) = (r.view_from, r.view_to);
                view! {
                    <tr on:click=move |_| editor::select(f, t)>
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

fn node_fields(n: p::NodeFields) -> impl IntoView {
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
            {n.fields.into_iter().map(field_row).collect_view()}
            </tbody>
        </table>
    }
}

fn field_row(f: p::EditableField) -> impl IntoView {
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
            <th title=title on:click=move |_| editor::select(from, to)>{f.name.clone()}</th>
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
            // Diagnostics and syntax colours come from the same Rust parse;
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
                            view! {
                                <li class=format!("diag {}", d.severity) on:click=move |_| editor::select(f, t)>
                                    // Severity in words too: never colour alone.
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
        <section class="viewport" aria-label="3D Viewport">
            <div class="pane-title">"3D Viewport"
                <span class="badge" title="X_ITE is a JavaScript renderer hosted through a narrow adapter">"X_ITE 15.1.10 (JavaScript) · temporary"</span>
                <label class="toggle">
                    <input type="checkbox" prop:checked=move || u.preview_enabled.get()
                        on:change=move |ev| u.preview_enabled.set(event_target_checked(&ev)) />
                    "Live"
                </label>
                <button class="secondary small" on:click=move |_| spawn_local(ui::preview(true))>"Update"</button>
            </div>
            <x3d-canvas id="viewport" splashScreen="false" contextMenu="false" notifications="false"
                timings="false" cache="false"></x3d-canvas>
            <div class="preview-status" id="preview-status">{move || format!("Preview: {}", u.preview_status.get())}</div>
        </section>
    }
}
