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
                <span class="muted" data-tip="USE and ROUTE node names are resolved by the lexical scope graph (ISO 4.6.2, 4.8.4, 4.10.2). ROUTE events and PROTO types are checked by typed analysis (ISO 4.8, 4.10.2). Read-only and advisory.">
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
                    // B3: a smaller step per level keeps deep names on screen.
                    let pad = format!("padding-left: {}rem", 0.35 + (it.depth.saturating_sub(1)) as f32 * 0.7);
                    let class = format!("tree-item kind-{}{}", it.kind.to_lowercase(), ref_class(&it.refs, it.typed.as_ref()));
                    let title = item_title(it, typed_title(ref_title(&it.refs, &a.items), it.typed.as_ref()));
                    // Offsets of THIS render's revision; tree_select refuses
                    // them unless that is still the document's revision.
                    let id2 = id.clone();
                    let act = move || { ui::tree_select(session, rev, id2.clone(), from, to); };
                    let act2 = act.clone();
                    view! {
                        <li role="treeitem" class=class style=pad data-tip=title data-id=id.clone() tabindex="-1"
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
                            <span class="kind" aria-hidden="true">{kind_glyph(&it.kind)}</span>
                            {label_view(it)}
                        </li>
                    }
                }).collect_view()}))}
            </ul>
            {move || (u.doc.with(|d| d.is_none())).then(|| view! { <p class="empty">"Start a New World or open a file to see its scene."</p> })}
        </aside>
    }
}

/// B3: the kind column is one compact glyph; the label already names the
/// kind (`PROTO …`, `USE …`, `ROUTE …`, the node type), and the full kind is
/// in the item's hover text.
fn kind_glyph(kind: &str) -> &'static str {
    match kind {
        "Node" => "•",
        "Use" => "↪",
        "Proto" => "◆",
        "ExternProto" => "◇",
        "Route" => "→",
        _ => "·",
    }
}

/// B3: the label, with a DEF name set apart from the node type. The text
/// content is exactly `label` (`Type DEFNAME`), so nothing that reads it
/// changes.
fn label_view(it: &p::SceneItem) -> impl IntoView {
    let def = (it.kind == "Node" && it.detail.as_deref() == Some("DEF"))
        .then(|| it.label.split_once(' '))
        .flatten();
    match def {
        Some((ty, name)) => view! {
            <span class="label"><span class="ty">{ty.to_string()}</span>" "<span class="def">{name.to_string()}</span></span>
        }
        .into_any(),
        None => view! { <span class="label">{it.label.clone()}</span> }.into_any(),
    }
}

/// B3: hover text always starts with the full label and kind, so a name
/// cut off by a narrow panel is still readable.
fn item_title(it: &p::SceneItem, rest: String) -> String {
    let head = match it.detail.as_deref() {
        Some("DEF") => format!("{} · {} (DEF)", it.label, it.kind),
        _ => format!("{} · {}", it.label, it.kind),
    };
    if rest.is_empty() {
        head
    } else {
        format!("{head}\n{rest}")
    }
}

/// B3: bring the Source panel to the front and select (reveal) the node's
/// span there. A selection only: no edit, no revision, no undo step. Refused
/// when the span is not of the text the editor shows.
fn show_in_source(session: u64, rev: u64, (from, to): (u64, u64)) {
    if ui::stale_reason(session, rev).is_some() {
        ui::flash("The Inspector is updating; Show in Source did nothing.");
        return;
    }
    crate::dock::show(p::layout::PanelId::Source);
    spawn_local(async move {
        // After the tab is shown (laid out) and focused.
        crate::ipc::sleep(30).await;
        if ui::stale_reason(session, rev).is_none() {
            crate::editor::select_quiet(from as u32, to as u32);
        }
    });
}

/// B3: the outermost Scene Tree items (nodes and USEs) inside the view span
/// [`from`, `to`] in the analysis of exactly (`session`, `rev`), as (id,
/// label). Empty when the tree holds another revision.
fn mf_children(session: u64, rev: u64, from: u64, to: u64) -> Vec<(String, String)> {
    ui().analysis.with_untracked(|a| {
        let Some(a) = a.as_ref().filter(|a| a.session == session && a.revision == rev) else {
            return Vec::new();
        };
        let inside: Vec<&p::SceneItem> = a
            .items
            .iter()
            .filter(|i| {
                (i.kind == "Node" || i.kind == "Use") && i.view_from >= from && i.view_to <= to
            })
            .collect();
        let Some(d) = inside.iter().map(|i| i.depth).min() else {
            return Vec::new();
        };
        inside
            .into_iter()
            .filter(|i| i.depth == d)
            .map(|i| (i.id.clone(), i.label.clone()))
            .collect()
    })
}

/// B3: a one-line summary of an SFNode value: `[DEF name] Type`, `USE name`
/// or `NULL`. Display only; the value is never rewritten.
pub(crate) fn node_summary(excerpt: &str) -> String {
    let mut t = excerpt.split_whitespace();
    match (t.next(), t.next(), t.next()) {
        (Some("DEF"), Some(name), Some(ty)) => {
            format!("{} {name}", ty.trim_end_matches('{'))
        }
        (Some("USE"), Some(name), _) => format!("USE {name}"),
        (Some(ty), _, _) => ty.trim_end_matches('{').to_string(),
        _ => String::new(),
    }
}

/// B3: the first line of a value, shortened to 60 characters.
pub(crate) fn first_line(excerpt: &str) -> String {
    let line = excerpt.trim().lines().next().unwrap_or("");
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(60).collect::<String>())
    } else {
        line.to_string()
    }
}

/// B3: plain words for a read-only reason id (the id stays in the tooltip).
pub(crate) fn reason_words(id: &str) -> String {
    match id {
        "field-type-not-editable-in-wd2b" => "edit in Source".into(),
        "field-is-is-bound" => "bound to the PROTO interface (IS)".into(),
        "field-value-missing" => "no value".into(),
        "field-value-shape-does-not-match-type" | "field-value-token-invalid" => {
            "value not understood; edit in Source".into()
        }
        // LV2: each says what is actually the case.
        "field-not-in-schema" => "not a field of this node type".into(),
        "field-is-x3d-only" => "X3D-only field (not in VRML97)".into(),
        "field-is-not-a-stored-field" => "an event, not a stored field".into(),
        "field-authored-more-than-once" => "written more than once".into(),
        "node-type-is-a-proto-name" => "PROTO instance field; edit in Source".into(),
        "node-type-not-standard-vrml97" => "not a standard VRML97 node; edit in Source".into(),
        "document-has-syntax-errors" => "fix the syntax errors first".into(),
        "document-parse-incomplete" => "the document is too large or deep to parse fully".into(),
        "node-incomplete" => "the node is not complete; fix it in Source".into(),
        "node-inside-proto-scope" => "inside a PROTO; edit in Source".into(),
        "proto-name-declared-more-than-once" => {
            "its PROTO name is declared more than once; edit in Source".into()
        }
        "shared-proto-definition-not-confirmed" => {
            "shared PROTO definition; allow editing first".into()
        }
        "node-identity-ambiguous" | "node-not-found-at-span" | "not-a-node-instance" => {
            "this node cannot be identified exactly; edit in Source".into()
        }
        other => other.replace('-', " "),
    }
}

/// LV2: the read-only words for a field of type `field_type`. A node-valued
/// field is not edited as a value here: its node has its own fields.
pub(crate) fn field_reason_words(id: &str, field_type: Option<&str>) -> String {
    match (id, field_type) {
        ("field-type-not-editable-in-wd2b", Some("SFNode" | "MFNode")) => {
            "open the node to edit it, or edit in Source".into()
        }
        _ => reason_words(id),
    }
}

/// LV2: what is selected, read from the SAME revision's Scene Tree and
/// Inspector (display only).
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionFacts {
    pub node_type: String,
    pub def: Option<String>,
    /// The Source line of the node (when the editor shows this revision).
    pub line: Option<u32>,
    /// A non-geometry node's owning Shape: (Scene Tree label, line).
    pub owner: Option<(String, Option<u32>)>,
    /// `inside PROTO X (shared, N instances)`.
    pub context: Option<String>,
}

/// The [`SelectionFacts`] of the current selection (reactive).
pub fn selection_facts() -> Option<SelectionFacts> {
    let u = ui();
    let sel = u.selected.get()?;
    let current = u.revision.get();
    let (node_type, context) = u
        .inspection
        .with(|i| {
            i.as_ref().filter(|i| i.id == sel.id && i.revision == sel.revision).map(|i| {
                let n = i.node.as_ref();
                (
                    n.and_then(|n| n.node_type.clone()),
                    n.and_then(|n| n.shared.as_ref())
                        .map(|s| p::highlight::proto_context(&s.proto, s.instances)),
                )
            })
        })
        .unwrap_or((None, None));
    u.analysis.with(|a| {
        let a = a
            .as_ref()
            .filter(|a| a.revision == sel.revision && a.session == sel.session)?;
        let idx = a.items.iter().position(|it| it.id == sel.id)?;
        let it = &a.items[idx];
        let def = (it.kind == "Node" && it.detail.as_deref() == Some("DEF"))
            .then(|| it.label.split_once(' ').map(|(_, d)| d.to_string()))
            .flatten();
        let ty = node_type.unwrap_or_else(|| {
            it.label.split(' ').next().unwrap_or(&it.label).to_string()
        });
        // Offsets of `sel.revision` are lines of the editor text only while
        // the document is still at that revision.
        let line_at = |pos: u64| (current == sel.revision).then(|| crate::editor::line_of(pos as u32)).flatten();
        let owner = (p::highlight::node_role(&ty) == p::highlight::NodeRole::Property)
            .then(|| {
                let depths: Vec<u32> = a.items.iter().map(|x| x.depth).collect();
                ancestors(&depths, idx).into_iter().rev().find_map(|j| {
                    let o = &a.items[j];
                    (o.kind == "Node" && o.label.split(' ').next() == Some("Shape"))
                        .then(|| (o.label.clone(), line_at(o.view_from)))
                })
            })
            .flatten();
        Some(SelectionFacts {
            node_type: ty,
            def,
            line: line_at(it.view_from),
            owner,
            context,
        })
    })
}

/// LV2: " (line N)" of the selected non-geometry node's owning Shape, for
/// the outline note; "" when unknown.
pub fn owner_line_note() -> String {
    untrack(selection_facts)
        .and_then(|f| f.owner)
        .and_then(|(_, l)| l)
        .map(|l| format!(" (line {l})"))
        .unwrap_or_default()
}

/// B3: the ancestors of item `idx` in a depth-first item list, outermost
/// first: each is the nearest earlier item one level shallower.
pub(crate) fn ancestors(depths: &[u32], idx: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let Some(&d0) = depths.get(idx) else {
        return out;
    };
    let mut want = d0;
    for j in (0..idx).rev() {
        if want <= 1 {
            break;
        }
        if depths[j] < want {
            out.push(j);
            want = depths[j];
        }
    }
    out.reverse();
    out
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
                <span class="badge" data-tip="Field edits are planned, validated and applied by Rust (wrlforge-vrml field_edit)">
                    "Edits validated by Rust"
                </span>
            </div>
            {move || u.field_error.get().map(|e| view! {
                <p class="field-error" id="inspector-error" role="alert">{e}</p>
            })}
            {move || match u.inspection.get() {
                None => view! { <p class="empty">"Select a Scene Tree item, or click an object in the viewport with Select."</p> }.into_any(),
                Some(i) => {
                    let rev = i.revision;
                    let session = crate::editor::CORE.with_borrow(|c| c.session).unwrap_or(0);
                    let head = inspector_head(&i, session);
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
                            {head}
                            {body}
                        </div>
                    }.into_any()
                }
            }}
        </section>
    }
}

/// B3: what is selected, where it is, and how to get back up. The path is
/// the Scene Tree's own ancestry of this item in the SAME revision; a crumb
/// selects through `pick_select` (the Open-node path), so a stale tree
/// refuses instead of selecting a different node. Display and navigation
/// only: nothing here edits.
fn inspector_head(i: &p::Inspection, session: u64) -> impl IntoView {
    let u = ui();
    let (id, rev) = (i.id.clone(), i.revision);
    let node_type = i.node.as_ref().and_then(|n| n.node_type.clone());
    let shared = i.node.as_ref().and_then(|n| n.shared.clone());
    let title = i.title.clone();
    // (DEF name, ancestors as (id, label)), from the analysis of `rev`.
    let found = move || {
        u.analysis.with(|a| {
            let a = a
                .as_ref()
                .filter(|a| a.revision == rev && a.session == session)?;
            let idx = a.items.iter().position(|it| it.id == id)?;
            let it = &a.items[idx];
            let def = (it.kind == "Node" && it.detail.as_deref() == Some("DEF"))
                .then(|| it.label.split_once(' ').map(|(_, d)| d.to_string()))
                .flatten();
            let depths: Vec<u32> = a.items.iter().map(|x| x.depth).collect();
            let path: Vec<(String, String)> = ancestors(&depths, idx)
                .into_iter()
                .map(|j| (a.items[j].id.clone(), a.items[j].label.clone()))
                .collect();
            Some((def, path, it.label.clone(), (it.view_from, it.view_to)))
        })
    };
    let found2 = found.clone();
    let go = move |target: String| {
        spawn_local(async move {
            if let Err(e) = ui::navigate_select(session, rev, target).await {
                ui::flash(&format!("Cannot open the node: {e}."));
            }
        });
    };
    let go2 = go.clone();
    let kind = node_type.clone().unwrap_or(title);
    let context = shared.map(|s| {
        let n = s.instances;
        format!(
            "inside PROTO {} (shared, {} instance{})",
            s.proto,
            n,
            if n == 1 { "" } else { "s" }
        )
    });
    view! {
        <div class="inspector-head" id="inspector-head">
            {move || found().map(|(_, path, label, span)| {
                let parent = path.last().cloned();
                let none = parent.is_none();
                let up_title = parent.as_ref().map(|(_, l)| format!("Select the parent node: {l}")).unwrap_or_else(|| "This node is at the top level".into());
                let go = go.clone();
                let go2 = go2.clone();
                view! {
                    <nav class="crumbs" id="inspector-path" aria-label="Node path">
                        <button type="button" class="small secondary crumb-up" id="inspector-up"
                            disabled=move || none || ui().revision.get() != rev
                            data-tip=up_title
                            on:click={let parent = parent.clone(); move |_| if let Some((p, _)) = parent.clone() { go(p) }}>
                            "↑ Parent"
                        </button>
                        <button type="button" class="small secondary" id="inspector-show-source"
                            data-tip="Show this node in the Source panel (selects its text; no edit)"
                            disabled=move || ui().revision.get() != rev
                            on:click=move |_| show_in_source(session, rev, span)>
                            "Show in Source"
                        </button>
                        {path.into_iter().map(|(pid, l)| {
                            let go = go2.clone();
                            view! {
                                <button type="button" class="crumb" data-item=pid.clone() data-tip=format!("Select {l}")
                                    disabled=move || ui().revision.get() != rev
                                    on:click=move |_| go(pid.clone())>{l.clone()}</button>
                                <span class="crumb-sep" aria-hidden="true">"›"</span>
                            }
                        }).collect_view()}
                        <span class="visually-hidden" aria-current="true">{label}</span>
                    </nav>
                }
            })}
            <h3 id="inspector-title">
                <span class="ty">{kind}</span>
                {move || found2().and_then(|(def, _, _, _)| def).map(|d| view! {
                    " "<span class="def" data-tip="DEF name">{d}</span>
                })}
                // LV2: where it is, always visible.
                {move || selection_facts().and_then(|f| f.line).map(|l| view! {
                    " "<span class="line-badge" id="inspector-line">{format!("line {l}")}</span>
                })}
            </h3>
            // LV2: a non-geometry node is not a rendered object of its own:
            // say what owns it (the viewport outlines that Shape, dotted).
            {move || selection_facts().and_then(|f| f.owner.map(|(label, line)| (f.node_type, label, line))).map(|(ty, label, line)| view! {
                <p class="inspector-owner" id="inspector-owner">
                    <span class="owner-badge">{p::highlight::owner_badge(&ty, &label, line)}</span>
                    " · not drawn by itself; the viewport outlines its Shape"
                </p>
            })}
            {context.map(|c| view! { <p class="inspector-context" id="inspector-context">{c}</p> })}
        </div>
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

/// B1: the "Shared PROTO definition" indicator. Every change of a shared
/// definition asks for confirmation (PR #146 review): there is no
/// document-wide "allow" state.
fn shared_banner(s: p::SharedProto, _session: u64) -> impl IntoView {
    let proto = s.proto.clone();
    let instances = s.instances;
    let text = format!(
        "Shared PROTO definition: this node is part of PROTO {}. It is used by {} instance(s) in this document; a change here changes every one of them.",
        s.proto, instances
    );
    view! {
        <div class="shared-proto" id="inspector-shared" role="note" data-proto=proto.clone()>
            <p data-tip=text><strong>"Shared definition"</strong>{format!(" · a change here changes every instance of PROTO {} ({} in this document)", proto, instances)}</p>
            <p class="shared-ok" id="inspector-shared-ask" data-tip="WRL Forge asks before every change to a shared PROTO definition; nothing is remembered.">"Each change asks for confirmation."</p>
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
                {format!("Read-only node: {}", reason_words(&n.reason))}
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
                        data-tip="Choose an image in the document's folder and add texture ImageTexture { url \"…\" } (one edit, one undo step)"
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
                        data-tip="Choose an image in the document's folder (written as a relative url)"
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
                    <button type="button" class="small" id=format!("fs-{field}-{i}-set") data-tip="Apply this entry"
                        disabled=stale
                        on:click=move |_| {
                            if let Some(inp) = crate::element_by_id::<web_sys::HtmlInputElement>(&id_set) {
                                s_set(p::StringOp::Set { index: i as u32, value: inp.value() });
                            }
                        }>"Apply"</button>
                    {pick}
                    <button type="button" class="small" id=format!("fs-{field}-{i}-del") data-tip="Remove this entry"
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
                data-tip="Choose an image in the document's folder and append it"
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
                <button type="button" class="small" id=format!("fs-{field}-add") data-tip="Append this entry"
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
                    data-tip="Pick a color (applies as one edit, one undo step)"
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
                    data-tip="Select this node in the Scene Tree and show its fields"
                    on:click=move |_| {
                        let id = id.clone();
                        spawn_local(async move {
                            if let Err(e) = ui::navigate_select(session, rev, id).await {
                                ui::flash(&format!("Cannot open the node: {e}."));
                            }
                        });
                    }>{label}</button>
            }
        });
        // B3: an MFNode value gets one "Open <Type> ›" per child node. The
        // children are the Scene Tree items of THIS revision lying inside
        // the field's value span, outermost only; each opens through the
        // same `pick_select` path as an SFNode's button (exact item id).
        let kids = if f.field_type.as_deref() == Some("MFNode") {
            mf_children(session, rev, f.view_from, f.view_to)
        } else {
            Vec::new()
        };
        let kid_buttons = (!kids.is_empty()).then(|| {
            view! {
                <span class="mf-open">
                {kids.into_iter().map(|(id, label)| {
                    view! {
                        <button class="small node-open" data-item=id.clone()
                            on:click=move |_| {
                                let id = id.clone();
                                spawn_local(async move {
                                    if let Err(e) = ui::navigate_select(session, rev, id).await {
                                        ui::flash(&format!("Cannot open the node: {e}."));
                                    }
                                });
                            }>{format!("Open {label} ›")}</button>
                    }
                }).collect_view()}
                </span>
            }
        });
        // B3: one line per field. An SFNode value shows its node type (and
        // DEF name); any long or multi-line value shows its first line, with
        // the full text one click away. The reason id is shown in words; the
        // id itself stays in the hover text.
        let excerpt = f.value_excerpt.clone();
        let summary = if f.node_item.is_some() || f.field_type.as_deref() == Some("SFNode") {
            node_summary(&excerpt)
        } else {
            first_line(&excerpt)
        };
        let full = (summary != excerpt.trim()).then(|| {
            view! {
                <details class="value-full">
                    <summary data-tip="Show the full value as written in the source">"Show source"</summary>
                    <code>{excerpt.clone()}</code>
                </details>
            }
        });
        let reason_title = format!("Why this field is read-only here ({})", f.reason);
        view! {
            <div class="ro-value">
                {open}
                {kid_buttons}
                <code class="value-summary" data-tip=excerpt.clone()>{summary}</code>
                <span class="reason" data-reason=f.reason.clone() data-tip=reason_title>{field_reason_words(&f.reason, f.field_type.as_deref())}</span>
                {full}
            </div>
        }
        .into_any()
    };
    view! {
        <tr class:ro=!f.editable data-field=f.name.clone()>
            <th data-tip=title on:click=move |_| { ui::select_from("Inspector", session, rev, from, to); }>{f.name.clone()}</th>
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
                        data-tip="Advisory scope findings: node names SCOPE0xx (ISO 4.6.2, 4.8.4, 4.10.2) and typed references SCOPE1xx (ISO 4.8, Table 4.4, 4.10.2). Never block Save; uncertain cases are not listed.">
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
                    data-tip="X_ITE is a JavaScript renderer hosted through a narrow adapter">"X_ITE 16.5.0 (JavaScript) · temporary"</span>
                // NATIVE-RENDER-1 (hidden): the native viewport is the pane
                // beside this window. Click selects, drag orbits, wheel zooms,
                // Escape clears, Home resets the view. Selection only.
                <span class="badge native-badge" hidden=move || !u.native.with(|s| s.native_shown())
                    data-tip=move || u.native.with(|s| s.info.clone().unwrap_or_default())>"Native (wgpu) · test · beside →"</span>
                <button class="secondary small" hidden=move || !u.native.with(|s| s.native_shown())
                    data-tip="Reset the native view to frame the scene (Home)"
                    on:click=move |_| crate::native::reset_camera()>"Reset view"</button>
                // UI-MENU-1: the same registry commands as View → Live
                // Preview / Update Preview.
                <label class="toggle" data-tip=crate::command::title(CommandId::ViewLivePreview)>
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
                    data-tip=crate::command::title(CommandId::ViewUpdatePreview)
                    disabled=move || !crate::command::enabled(CommandId::ViewUpdatePreview)
                    on:click=move |_| crate::command::run(CommandId::ViewUpdatePreview)>"Update"</button>
                <span class="sep" hidden=move || u.native.with(|s| s.native_shown())></span>
                // UI-DOCK-1: the four tools wrap as one group (never clipped).
                <span class="tool-group" role="group" aria-label="Viewport tools">
                // VISUAL-2: an explicit selection mode. A click selects the
                // exact authored object; a drag still moves the camera.
                <button id="btn-select" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolSelect.key() data-tip=crate::command::title(CommandId::ToolSelect)
                    aria-pressed=move || if u.pick_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolSelect)
                    on:click=move |_| crate::command::run(CommandId::ToolSelect)>"Select"</button>
                // VISUAL-3A: the Move tool. Drag an axis handle to move the
                // selected top-level Transform; a click still selects, a drag
                // elsewhere still moves the camera.
                <button id="btn-move" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolMove.key() data-tip=crate::command::title(CommandId::ToolMove)
                    aria-pressed=move || if u.move_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolMove)
                    on:click=move |_| crate::command::run(CommandId::ToolMove)>"Move"</button>
                // VISUAL-4B: Rotate (rings about the world axes through the
                // object's center) and Scale (its own scale axes + uniform).
                <button id="btn-rotate" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolRotate.key() data-tip=crate::command::title(CommandId::ToolRotate)
                    aria-pressed=move || if u.rotate_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolRotate)
                    on:click=move |_| crate::command::run(CommandId::ToolRotate)>"Rotate"</button>
                <button id="btn-scale" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolScale.key() data-tip=crate::command::title(CommandId::ToolScale)
                    aria-pressed=move || if u.scale_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolScale)
                    on:click=move |_| crate::command::run(CommandId::ToolScale)>"Scale"</button>
                // LINUX-EDIT-INPUT-SAFETY: the world's own sensors get the
                // pointer only here; every other state edits.
                <button id="btn-interact" class="secondary small" hidden=move || u.native.with(|s| s.native_shown())
                    data-command=CommandId::ToolInteract.key() data-tip=crate::command::title(CommandId::ToolInteract)
                    aria-pressed=move || if u.interact_mode.get() { "true" } else { "false" }
                    disabled=move || !crate::command::enabled(CommandId::ToolInteract)
                    on:click=move |_| crate::command::run(CommandId::ToolInteract)>"Interact"</button>
                </span>
                <span class="sep" hidden=move || u.native.with(|s| s.native_shown())></span>
                // B2-CAMERA: the camera controls (View menu → Camera). The
                // camera is view state: no source change, no dirty flag, no
                // undo step.
                <span class="tool-group" role="group" aria-label="Camera" hidden=move || u.native.with(|s| s.native_shown())>
                <button id="btn-fit" class="secondary small"
                    data-command=CommandId::ViewFitAll.key() data-tip=crate::command::title(CommandId::ViewFitAll)
                    disabled=move || !crate::command::enabled(CommandId::ViewFitAll)
                    on:click=move |_| crate::command::run(CommandId::ViewFitAll)>"Fit"</button>
                <select id="sel-view" class="small" aria-label="Standard view"
                    data-tip="Front, Back, Left, Right, Top or Bottom view of the whole scene, or Reset View (camera only)"
                    disabled=move || !crate::command::enabled(CommandId::ViewFront)
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        if let Some(sel) = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlSelectElement>(t).ok()) {
                            sel.set_value("");
                        }
                        if let Some(id) = CommandId::from_key(&v) {
                            crate::command::run(id);
                        }
                    }>
                    <option value="" selected>"View…"</option>
                    {[CommandId::ViewFront, CommandId::ViewBack, CommandId::ViewLeft, CommandId::ViewRight,
                      CommandId::ViewTop, CommandId::ViewBottom, CommandId::ViewResetCamera]
                        .into_iter()
                        .map(|c| view! { <option value=c.key() title=c.spec().hint>{c.spec().label}</option> })
                        .collect_view()}
                </select>
                <select id="sel-viewpoint" class="small" aria-label="Viewpoint"
                    data-tip="Bind an authored Viewpoint (its description, else its DEF name). Camera only."
                    disabled=move || !crate::command::enabled(CommandId::ViewFitAll)
                    on:focus=move |_| crate::camera::refresh_viewpoints()
                    on:pointerdown=move |_| crate::camera::refresh_viewpoints()
                    on:change=move |ev| {
                        if let Ok(i) = event_target_value(&ev).parse::<usize>() {
                            crate::camera::choose_viewpoint(i);
                        }
                    }>
                    <option value="" prop:selected=move || u.viewpoints.with(|v| !v.items.iter().any(|i| i.bound))>
                        {move || u.viewpoints.with(|v| if v.items.is_empty() { "No viewpoints" } else { "Viewpoints…" })}
                    </option>
                    {move || u.viewpoints.with(|v| v.items.iter().enumerate().map(|(i, it)| {
                        let label = crate::camera::viewpoint_label(it, i);
                        let bound = it.bound;
                        view! { <option value=i.to_string() prop:selected=bound>{label}</option> }
                    }).collect_view())}
                </select>
                </span>
            </div>
            <div class="viewport-stage" hidden=move || u.native.with(|s| s.native_shown())>
                <x3d-canvas id="viewport" class:picking=move || u.pick_mode.get() || u.move_mode.get() || u.rotate_mode.get() || u.scale_mode.get() splashScreen="false" contextMenu="false" notifications="false"
                    timings="false" cache="false"></x3d-canvas>
                // B3 (LV2): the OTHER instances of a selected shared PROTO
                // body node -- an edit of the definition changes them too.
                // Dashed and amber, so it never reads as the selection.
                <svg id="shared-outline" class="shared-outline" data-state="hidden" data-count="0" aria-hidden="true">
                    <path id="hl-shared-halo" class="hl-shared-halo"></path>
                    <path id="hl-shared-line" class="hl-shared-line"></path>
                </svg>
                // ... and which PROTO / how many other instances (set by
                // `highlight.rs` while the dashed outline is shown).
                <div class="shared-outline-note" id="shared-outline-note" role="status" hidden></div>
                // B2: the selection outline (view state only), under the
                // gizmo. Drawn from the renderer's camera by `highlight.rs`;
                // it never takes pointer input.
                <svg id="selection-outline" class="selection-outline" data-state="hidden" aria-hidden="true">
                    // LV2-C: edges the object's own faces hide (faint, dashed).
                    <path id="hl-hidden" class="hl-hidden"></path>
                    <path id="hl-halo" class="hl-halo"></path>
                    <path id="hl-line" class="hl-line"></path>
                </svg>
                // B2: why the selection has no outline. Floats over the
                // canvas so it never changes the viewport's size. LV2: what
                // the outline shows when it is not the node itself.
                <div class="highlight-notes">
                    <div class="highlight-info" id="highlight-info" role="status" aria-live="polite"
                        hidden=move || u.highlight_info.with(|m| m.is_none())>
                        {move || u.highlight_info.get().unwrap_or_default()}
                    </div>
                    <div class="highlight-status" id="highlight-status" role="status" aria-live="polite"
                        hidden=move || u.highlight_message.with(|m| m.is_none())>
                        {move || u.highlight_message.get().unwrap_or_default()}
                    </div>
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
                data-tip=move || u.texture_warnings.with(|w| w.iter().map(|t| format!("line {} · {}: {}", t.line, t.node, t.detail)).collect::<Vec<_>>().join("\n"))>
                {move || u.texture_warnings.with(|w| match w.first() {
                    None => String::new(),
                    Some(t) => format!("⚠ {} texture{} not loaded · line {} · {}: {}{}", w.len(), if w.len() == 1 { "" } else { "s" },
                        t.line, t.node, t.detail, if w.len() > 1 { " · hover for all" } else { "" }),
                })}
            </div>
            // XITE-16-UPGRADE-2: Rust diagnostics, not the picture, decide this.
            <div class="preview-status route-warning" id="route-warning" role="status" aria-live="polite"
                hidden=move || u.route_warnings.with(|w| w.is_empty()) || u.native.with(|s| s.native_shown())
                data-tip=move || u.route_warnings.with(|w| w.iter().map(|r| format!("line {} · {}", r.line, r.detail)).collect::<Vec<_>>().join("\n"))>
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
                            data-tip=format!("Write the VRML97 default {f} value into this Transform (one undo step)")
                            on:click=move |_| spawn_local(crate::gizmo::write_missing_field())>
                            {format!("Write {f}")}
                        </button>
                    })
                }}
            </div>
            <div class="pick-status" id="pick-status" role="status" aria-live="polite"
                data-kind=move || u.pick_message.with(|m| m.as_ref().map(|m| m.0.clone()).unwrap_or_default())
                // P0-A: while Select is on, the line is always laid out (empty
                // until the first pick), so the first pick message does not
                // shrink the viewport and move the scene under the pointer --
                // "click again at the same place" must mean the same ray.
                hidden=move || u.pick_message.with(|m| m.is_none()) && !u.pick_mode.get()>
                {move || u.pick_message.with(|m| m.as_ref().map(|m| m.1.clone()).unwrap_or_else(|| "\u{a0}".into()))}
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
    // B3: a deep item's name is on screen too (the tree scrolls sideways).
    let left = li.offset_left() - ul.offset_left();
    let pad = li
        .query_selector(".label")
        .ok()
        .flatten()
        .and_then(|l| l.dyn_into::<web_sys::HtmlElement>().ok())
        .map_or(left, |l| l.offset_left() - ul.offset_left());
    let (sl, vw) = (ul.scroll_left(), ul.client_width());
    if pad < sl || pad > sl + vw / 2 {
        ul.set_scroll_left((pad - 24).max(0));
    }
}

/// B3: reveal now, and again once the tree has rendered this revision's
/// items (a viewport pick may have just brought the tree up to date).
pub fn reveal_tree_item_soon(id: &str) {
    reveal_tree_item(id);
    let id = id.to_string();
    spawn_local(async move {
        crate::ipc::sleep(60).await;
        reveal_tree_item(&id);
    });
}

/// UI-MENU-1: the Select tool toggle (toolbar button, Tools menu). Moved
/// unchanged from the button's handler: one viewport tool at a time.
pub fn toggle_select() {
    let u = ui();
    if !u.pick_mode.get_untracked() && refuse_unsafe_tool("Select") {
        return;
    }
    u.pick_mode.update(|m| *m = !*m);
    if u.pick_mode.get_untracked() {
        leave_interact();
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
    let (mine, others, name) = match tool {
        Tool::Move => (u.move_mode, [u.rotate_mode, u.scale_mode], "Move"),
        Tool::Rotate => (u.rotate_mode, [u.move_mode, u.scale_mode], "Rotate"),
        Tool::Scale => (u.scale_mode, [u.move_mode, u.rotate_mode], "Scale"),
    };
    if !mine.get_untracked() && refuse_unsafe_tool(name) {
        return;
    }
    mine.update(|m| *m = !*m);
    if mine.get_untracked() {
        leave_interact();
        u.pick_mode.set(false);
        for o in others {
            o.set(false);
        }
        crate::gizmo::start();
    } else {
        u.gizmo_message.set(None);
    }
}

/// LINUX-EDIT-INPUT-SAFETY: the Interact toggle (toolbar button, Tools
/// menu). Interact is the one state in which the world's TouchSensors, drag
/// sensors and Anchors get the pointer; it ends every editing tool, and no
/// click selects. Leaving it closes the sensor gate first, then reloads the
/// preview from the source, so state the world changed while it ran (an
/// open door, a moved PlaneSensor target) is never edited against. The
/// runtime never writes the source.
///
/// PR #149 review: entering Interact ends the edit session. Any gesture is
/// canceled, the selection, outline, gizmo, pick cycle and pick status are
/// cleared, and from then on every source edit is refused (the registry,
/// and `ipc::call` for anything begun earlier and still pending) and the
/// Source pane is read-only, until the user leaves Interact. The gate opens
/// only after all of that, and only when it verifies.
pub fn toggle_interact() {
    let u = ui();
    if u.interact_mode.get_untracked() {
        leave_interact();
        return;
    }
    let state = crate::ipc::preview_scene_input(None);
    if !gate_safe(&state) {
        gate_unavailable(&state);
        crate::ui::flash(&format!(
            "Interact is not available: the scene input gate is {state}. Viewport input stays blocked until the gate passes its check."
        ));
        return;
    }
    crate::gizmo::cancel("Interact started");
    u.pick_mode.set(false);
    u.move_mode.set(false);
    u.rotate_mode.set(false);
    u.scale_mode.set(false);
    // Refuse every source edit from here on (also one already pending).
    u.interact_mode.set(true);
    crate::ui::clear_selection();
    crate::pick::forget_selection();
    crate::pick::non_pick_selection();
    u.pick_message.set(None);
    u.gizmo_message.set(None);
    let state = crate::ipc::preview_scene_input(Some(true));
    if state != "interact" {
        // Fail closed and say why: the gate stays closed, the editor keeps
        // the pointer, and editing comes back as it was (nothing selected).
        u.interact_mode.set(false);
        let state = crate::ipc::preview_scene_input(Some(false));
        gate_unavailable(&state);
        crate::ui::flash(&format!("Interact is not available: the scene input gate is {state}."));
    }
}

/// Leave Interact (no-op when it is off): sensors lose the pointer at once,
/// then the preview is reloaded from the source text.
pub fn leave_interact() {
    let u = ui();
    if !u.interact_mode.get_untracked() {
        return;
    }
    u.interact_mode.set(false);
    crate::ipc::preview_scene_input(Some(false));
    spawn_local(crate::ui::preview(true));
}

/// A verified gate: `edit` or `interact`. Anything else is `unavailable:*`.
pub fn gate_safe(state: &str) -> bool {
    state == "edit" || state == "interact"
}

/// PR #149 review: refuse an editing tool while the gate is not verified
/// (the adapter blocks every pointer event to the canvas then; the tool
/// could not work and would only look available). True when refused.
fn refuse_unsafe_tool(tool: &str) -> bool {
    let state = crate::ipc::preview_scene_input(None);
    if gate_safe(&state) {
        return false;
    }
    gate_unavailable(&state);
    crate::ui::flash(&format!(
        "{tool} is not available: the scene input gate is {state}. Viewport input is blocked until the gate passes its check."
    ));
    true
}

/// PR #149 review: the gate is not verified. The adapter already blocks
/// every pointer event to the canvas; here every viewport tool and Interact
/// end (Interact reloads the preview from the source), any gesture is
/// canceled, and the reason is shown. Nothing is selected or edited, and
/// Interact is never entered by itself.
pub fn gate_unavailable(state: &str) {
    let u = ui();
    crate::gizmo::cancel("the scene input gate is unavailable");
    u.pick_mode.set(false);
    u.move_mode.set(false);
    u.rotate_mode.set(false);
    u.scale_mode.set(false);
    if u.interact_mode.get_untracked() {
        // Interact ends with the gate: the world's runtime state is replaced
        // by the source-backed preview before any edit can see it.
        u.interact_mode.set(false);
        spawn_local(crate::ui::preview(true));
    }
    u.pick_message.set(Some((
        "refused".into(),
        format!(
            "{BLOCKED_NOTE}: the scene input gate is {state}. The world's sensors cannot be kept from editing clicks, so the 3D Viewport takes no clicks or drags until the gate passes its check."
        ),
    )));
}

/// Read the gate after a load (or a blocked click) and act on it.
pub fn check_scene_input() -> String {
    let state = crate::ipc::preview_scene_input(None);
    if !gate_safe(&state) {
        gate_unavailable(&state);
    } else if ui()
        .pick_message
        .with_untracked(|m| m.as_ref().is_some_and(|(_, m)| m.starts_with(BLOCKED_NOTE)))
    {
        // Verified again: the warning goes; the tools stay off until chosen.
        ui().pick_message.set(None);
    }
    state
}

const BLOCKED_NOTE: &str = "Viewport input blocked";

/// PR #149 review: the adapter blocked a press on the canvas because the
/// gate is not verified: say so (once per press) and end the tools.
pub fn install_scene_input_warning() {
    use wasm_bindgen::{closure::Closure, JsCast};
    let Some(w) = web_sys::window() else { return };
    let f = Closure::<dyn Fn(web_sys::Event)>::new(|_| {
        let state = check_scene_input();
        if !gate_safe(&state) {
            crate::ui::flash(&format!(
                "Viewport input blocked: the scene input gate is {state}."
            ));
        }
    });
    let _ = w.add_event_listener_with_callback(
        "wrlforge-scene-input-blocked",
        f.as_ref().unchecked_ref(),
    );
    f.forget();
}

/// The viewport status suffix for the sensor gate: nothing while editing
/// (the normal state), a reminder in Interact, a warning when the gate is
/// missing (the viewport then takes no pointer input at all).
pub fn scene_input_note() -> String {
    let state = check_scene_input();
    match state.as_str() {
        "edit" => String::new(),
        "interact" => " · Interact: the world responds to clicks; source edits are refused".into(),
        _ => format!(" · WARNING: viewport input blocked, the scene sensor gate is {state}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ancestors, kind_glyph};

    #[test]
    fn ancestors_walk_up_a_depth_first_list() {
        // PROTO P { Group { Transform { Shape { Appearance { Material } } } } }, then a sibling.
        let d = [1, 2, 3, 4, 5, 6, 4, 1];
        assert_eq!(ancestors(&d, 5), vec![0, 1, 2, 3, 4]);
        assert_eq!(ancestors(&d, 6), vec![0, 1, 2]);
        assert!(ancestors(&d, 0).is_empty());
        assert!(ancestors(&d, 7).is_empty());
        assert!(ancestors(&d, 99).is_empty());
    }

    #[test]
    fn sfnode_values_summarize_to_one_line() {
        use super::{first_line, node_summary, reason_words};
        let x = "IndexedFaceSet {creaseAngle 3\n ccw FALSE coord Coordinate { point IS point } }";
        assert_eq!(node_summary(x), "IndexedFaceSet");
        assert_eq!(node_summary("DEF Mat Material { }"), "Material Mat");
        assert_eq!(node_summary("Appearance{ material NULL }"), "Appearance");
        assert_eq!(node_summary("USE Wheel"), "USE Wheel");
        assert_eq!(node_summary("NULL"), "NULL");
        assert_eq!(first_line("a b\nc"), "a b");
        assert_eq!(reason_words("field-type-not-editable-in-wd2b"), "edit in Source");
    }

    #[test]
    fn inspector_reasons_say_what_is_actually_the_case() {
        use super::{field_reason_words, reason_words};
        // Distinct facts get distinct words (they were one sentence).
        let ids = [
            "field-not-in-schema",
            "field-is-x3d-only",
            "field-is-not-a-stored-field",
            "node-type-is-a-proto-name",
            "node-type-not-standard-vrml97",
            "document-has-syntax-errors",
            "document-parse-incomplete",
        ];
        let w = ids.map(reason_words);
        for (i, a) in w.iter().enumerate() {
            assert_ne!(*a, ids[i].replace('-', " "), "{a}: a raw reason code");
            for b in &w[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(!reason_words("node-type-not-standard-vrml97").contains("PROTO"));
        assert!(!reason_words("document-parse-incomplete").contains("syntax"));
        // A node-valued field points at its Open button.
        assert!(field_reason_words("field-type-not-editable-in-wd2b", Some("SFNode")).starts_with("open the node"));
        assert!(field_reason_words("field-type-not-editable-in-wd2b", Some("MFNode")).starts_with("open the node"));
        assert_eq!(field_reason_words("field-type-not-editable-in-wd2b", Some("MFVec3f")), "edit in Source");
        for id in [
            "node-incomplete",
            "node-inside-proto-scope",
            "proto-name-declared-more-than-once",
            "shared-proto-definition-not-confirmed",
            "node-identity-ambiguous",
        ] {
            assert_ne!(reason_words(id), id.replace('-', " "), "{id} has words");
        }
    }

    #[test]
    fn kind_glyph_is_one_character_for_every_kind() {
        let kinds = [
            "Node",
            "Use",
            "Proto",
            "ExternProto",
            "Route",
            "Document",
            "?",
        ];
        for k in kinds {
            assert_eq!(kind_glyph(k).chars().count(), 1, "{k}");
        }
    }
}
