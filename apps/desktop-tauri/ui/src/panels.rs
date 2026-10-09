// SPDX-License-Identifier: GPL-3.0-or-later
//! Workspace panels: Scene Tree, Inspector, Diagnostics, 3D Viewport. All are
//! READ projections of the canonical document computed in Rust.

use leptos::prelude::*;
use leptos::task::spawn_local;

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
            <ul class="tree" role="tree">
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
                            class:selected=move || u.selected.get().as_deref() == Some(id_sel.as_str())
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
                <span class="badge">"read-only — field editing not migrated"</span>
            </div>
            {move || match u.inspection.get() {
                None => view! { <p class="empty">"Select a Scene Tree item."</p> }.into_any(),
                Some(i) => view! {
                    <div>
                        <h3 id="inspector-title">{i.title.clone()}</h3>
                        <table class="fields">
                            <tbody>
                            {i.rows.into_iter().map(|r| {
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
                    </div>
                }.into_any(),
            }}
        </section>
    }
}

#[component]
pub fn Diagnostics() -> impl IntoView {
    let u = ui();
    view! {
        <div class="diagnostics" aria-label="Diagnostics">
            {move || u.analysis.with(|a| match a {
                None => view! { <span class="muted">"Diagnostics: —"</span> }.into_any(),
                Some(a) if a.diagnostics.is_empty() => view! { <span class="ok">"✓ No parser diagnostics (native Rust parser)"</span> }.into_any(),
                Some(a) => view! {
                    <ul>
                        {a.diagnostics.iter().take(200).map(|d| {
                            let (f, t) = (d.view_from, d.view_to);
                            view! {
                                <li class=format!("diag {}", d.severity) on:click=move |_| editor::select(f, t)>
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
