// SPDX-License-Identifier: GPL-3.0-or-later
//! VISUAL-4A: the Extrusion Inspector and the 2D outline (cross-section)
//! editor.
//!
//! Display and intent only. Every value shown is Rust's (`ExtrusionInfo`,
//! `ListField`): authored lexemes, or the VRML97 default for a field the
//! author did not write. Every change is sent to Rust as raw text inside a
//! list operation (`doc_edit_list`), a field edit (`doc_edit_field`) or an
//! explicit "write this field" request (`doc_add_field`); Rust validates,
//! plans, proves and applies it. Nothing here rounds, normalizes, reorders,
//! closes, opens or clamps a value.
//!
//! The outline editor works on a WORKING COPY of the authored outline and
//! records the user's operations. Apply sends the whole operation log as ONE
//! request (one undo step); Cancel drops the copy without any IPC, so the
//! source and the undo history are untouched. Moving a point drawn as the
//! closing point of a closed outline moves both of its entries (first and
//! last), so a move never opens the outline by accident; nothing else ever
//! closes or opens it.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wrlforge_desktop_protocol as p;

use crate::ui::{self, ui};

/// The section each Extrusion field is shown in.
const SECTIONS: [(&str, &str, &[&str]); 5] = [
    (
        "xs",
        "Cross-section points (crossSection, x z)",
        &["crossSection"],
    ),
    ("sp", "Spine points (spine, x y z)", &["spine"]),
    ("sc", "Per-point scale (scale, x z)", &["scale"]),
    (
        "or",
        "Per-point orientation (orientation, axis + angle)",
        &["orientation"],
    ),
    (
        "fl",
        "Caps and geometry flags",
        &[
            "beginCap",
            "endCap",
            "solid",
            "ccw",
            "convex",
            "creaseAngle",
        ],
    ),
];

/// Fields the Extrusion panel shows; the generic table skips them.
pub fn is_extrusion_field(name: &str) -> bool {
    SECTIONS.iter().any(|(_, _, f)| f.contains(&name))
}

/// Shortest text for a number this UI proposes (a midpoint, a dragged
/// position). Never applied to an authored value.
pub fn fmt_num(v: f64, decimals: usize) -> String {
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

fn shortest(v: f64) -> String {
    fmt_num(v, 6)
}

fn num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A proposed tuple between rows `i` and `i + 1` (or a copy of row `i`).
fn proposed_after(tuples: &[Vec<String>], i: usize, rotation: bool) -> Vec<String> {
    let a = &tuples[i];
    match (rotation, tuples.get(i + 1)) {
        (false, Some(b)) => a
            .iter()
            .zip(b)
            .map(|(x, y)| match (num(x), num(y)) {
                (Some(x), Some(y)) => shortest((x + y) / 2.0),
                _ => x.clone(),
            })
            .collect(),
        _ => a.clone(),
    }
}

fn input_value(id: &str) -> Option<String> {
    crate::element_by_id::<web_sys::HtmlInputElement>(id).map(|i| i.value())
}

fn row_id(field: &str, row: usize, c: usize) -> String {
    format!("le-{field}-{row}-{c}")
}

/// The Extrusion section of the Inspector.
pub fn panel(
    e: p::ExtrusionInfo,
    fields: &[p::EditableField],
    rev: u64,
    editable: bool,
) -> impl IntoView {
    let notes = e
        .notes
        .iter()
        .map(|n| {
            view! {
                <li class=format!("xnote {}", n.level) data-field=n.field.clone()>
                    <span class="sev">{match n.level.as_str() { "error" => "Error", "warning" => "Warning", _ => "Note" }}</span>
                    {n.text.clone()}
                </li>
            }
        })
        .collect_view();
    let sections = SECTIONS
        .iter()
        .map(|(key, title, names)| {
            let body = names
                .iter()
                .filter_map(|name| e.fields.iter().find(|f| f.name == *name).cloned())
                .map(|xf| {
                    let ef = xf
                        .field_index
                        .and_then(|i| fields.iter().find(|f| f.index == i))
                        .cloned();
                    field_block(xf, ef, rev, editable, e.outline_closed).into_any()
                })
                .collect_view();
            view! {
                <section class="xsec" id=format!("xsec-{key}") aria-label=*title>
                    <h4>{*title}</h4>
                    {body}
                </section>
            }
        })
        .collect_view();
    view! {
        <div class="extrusion" id="extrusion-panel" data-revision=rev.to_string()>
            <ul class="xnotes" id="extrusion-notes" role="status">{notes}</ul>
            {sections}
        </div>
    }
}

fn field_block(
    xf: p::ExtrusionField,
    ef: Option<p::EditableField>,
    rev: u64,
    editable: bool,
    closed: bool,
) -> impl IntoView {
    let is_list = xf.tuples.is_some()
        || matches!(xf.field_type.as_str(), "MFVec2f" | "MFVec3f" | "MFRotation");
    let status = if xf.authored {
        "authored".to_string()
    } else {
        format!(
            "default{} (not written in the source)",
            xf.default_text
                .as_ref()
                .map(|d| format!(" {d}"))
                .unwrap_or_default()
        )
    };
    let head = view! {
        <div class="xfield-head" data-field=xf.name.clone() data-authored=xf.authored.to_string()>
            <code>{xf.name.clone()}</code>
            <span class="kind">{xf.field_type.clone()}</span>
            <span class=if xf.authored { "xstatus authored" } else { "xstatus default" }>{status}</span>
        </div>
    };
    let body = match (
        is_list,
        ef.as_ref().and_then(|f| f.list.clone()),
        xf.authored,
    ) {
        // Authored, readable, editable list.
        (true, Some(list), true) if editable && ef.as_ref().is_some_and(|f| f.editable) => {
            let f = ef.clone().unwrap();
            let outline = (xf.name == "crossSection").then(|| {
                let (fi, tuples) = (f.index, list.tuples.clone());
                view! {
                    <button class="small" id="btn-outline" disabled=move || ui().revision.get() != rev
                        data-tip="Open the 2D outline editor: move, add, insert and delete points; Apply makes one undo step, Cancel changes nothing"
                        on:click=move |_| open_outline(rev, fi, tuples.clone(), closed)>"Edit outline…"</button>
                }
            });
            view! {
                <div>
                    {list_table(&xf.name, f.index, &list, rev)}
                    {outline}
                </div>
            }
            .into_any()
        }
        // Authored but not editable here: show why.
        (_, _, true) => {
            let why = xf.problem.clone().or_else(|| {
                ef.as_ref()
                    .filter(|f| !f.editable)
                    .map(|f| f.reason.clone())
            });
            match (is_list, why) {
                (_, Some(w)) => view! {
                    <p class="readonly-note" data-field=xf.name.clone()>{format!("Read-only: {w}")}</p>
                    {ef.map(|f| view! { <code class="xexcerpt">{f.value_excerpt}</code> })}
                }
                .into_any(),
                (false, None) => single_authored(xf, ef, rev).into_any(),
                (true, None) => view! { <p class="readonly-note">"Read-only."</p> }.into_any(),
            }
        }
        // Not authored: show the default; writing it is an explicit action.
        (true, _, false) => {
            let tuples = xf.tuples.clone().unwrap_or_default();
            let name = xf.name.clone();
            let write = editable.then(|| {
                let (name, tuples) = (name.clone(), tuples.clone());
                view! {
                    <button class="small" id=format!("xadd-{name}") disabled=move || ui().revision.get() != rev
                        data-tip="Write this field into the source with the values shown (one undo step)"
                        on:click=move |_| {
                            let (n, t) = (name.clone(), tuples.clone());
                            spawn_local(async move {
                                if ui::add_field(n, p::AddFieldValue::List { tuples: t }).await {
                                    ui::preview(false).await;
                                }
                            });
                        }>"Write field with these values"</button>
                }
            });
            view! {
                <table class="xlist default" data-field=xf.name.clone()>
                    <tbody>{tuples.iter().enumerate().map(|(i, t)| view! {
                        <tr><th>{(i + 1).to_string()}</th><td><code>{t.join(" ")}</code></td></tr>
                    }).collect_view()}</tbody>
                </table>
                {write}
            }
            .into_any()
        }
        (false, _, false) => single_default(xf, rev, editable).into_any(),
    };
    view! { <div class="xfield">{head}{body}</div> }
}

/// An authored SFBool / SFFloat: the existing field-edit path.
fn single_authored(xf: p::ExtrusionField, ef: Option<p::EditableField>, rev: u64) -> impl IntoView {
    let Some(f) = ef.filter(|f| f.editable) else {
        return view! { <code>{xf.single.unwrap_or_default()}</code> }.into_any();
    };
    let (idx, name) = (f.index, f.name.clone());
    let id = format!("xf-{name}");
    if f.kind.as_deref() == Some("bool") {
        let on = f.components.first().and_then(|c| c.bool_value) == Some(true);
        let id2 = id.clone();
        view! {
            <div class="controls">
                <select id=id.clone() prop:value=if on { "TRUE" } else { "FALSE" }>
                    <option value="TRUE" selected=on>"TRUE"</option>
                    <option value="FALSE" selected=!on>"FALSE"</option>
                </select>
                <button class="small" id=format!("xapply-{name}") disabled=move || ui().revision.get() != rev
                    on:click=move |_| {
                        let v = crate::element_by_id::<web_sys::HtmlSelectElement>(&id2).map(|s| s.value() == "TRUE");
                        if let Some(v) = v {
                            let n = name.clone();
                            spawn_local(async move {
                                ui::edit_field(idx, n, vec![p::FieldInput::Bool { value: v }]).await;
                                ui::preview(false).await;
                            });
                        }
                    }>"Apply"</button>
            </div>
        }
        .into_any()
    } else {
        let text = f
            .components
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default();
        let id2 = id.clone();
        view! {
            <div class="controls">
                <input id=id.clone() class="num" type="text" spellcheck="false" prop:value=text />
                <button class="small" id=format!("xapply-{name}") disabled=move || ui().revision.get() != rev
                    on:click=move |_| {
                        if let Some(v) = input_value(&id2) {
                            let n = name.clone();
                            spawn_local(async move {
                                ui::edit_field(idx, n, vec![p::FieldInput::Text { value: v }]).await;
                                ui::preview(false).await;
                            });
                        }
                    }>"Apply"</button>
                {f.bounds.map(|b| view! { <span class="muted">{format!("range {b}")}</span> })}
            </div>
        }
        .into_any()
    }
}

/// A flag the author did not write: the default is shown; choosing a value
/// and pressing Write adds the field (explicit, one undo step).
fn single_default(xf: p::ExtrusionField, rev: u64, editable: bool) -> impl IntoView {
    let name = xf.name.clone();
    let id = format!("xf-{name}");
    let def = xf.single.clone().unwrap_or_default();
    let control = if xf.field_type == "SFBool" {
        let on = def == "TRUE";
        view! {
            <select id=id.clone() prop:value=if on { "TRUE" } else { "FALSE" }>
                <option value="TRUE" selected=on>"TRUE"</option>
                <option value="FALSE" selected=!on>"FALSE"</option>
            </select>
        }
        .into_any()
    } else {
        view! { <input id=id.clone() class="num" type="text" spellcheck="false" prop:value=def.clone() /> }.into_any()
    };
    let bool_type = xf.field_type == "SFBool";
    view! {
        <div class="controls">
            {control}
            {editable.then(|| view! {
                <button class="small" id=format!("xadd-{name}") disabled=move || ui().revision.get() != rev
                    data-tip="Write this field into the source with the chosen value (one undo step)"
                    on:click=move |_| {
                        let value = if bool_type {
                            crate::element_by_id::<web_sys::HtmlSelectElement>(&id)
                                .map(|s| p::AddFieldValue::Bool { value: s.value() == "TRUE" })
                        } else {
                            input_value(&id).map(|v| p::AddFieldValue::Number { value: v })
                        };
                        if let Some(value) = value {
                            let n = name.clone();
                            spawn_local(async move {
                                if ui::add_field(n, value).await {
                                    ui::preview(false).await;
                                }
                            });
                        }
                    }>"Write"</button>
            })}
        </div>
    }
}

/// One authored list as an editable table: per row Apply / Insert after /
/// Delete; Add at end. Each button is ONE list operation, one undo step.
pub fn list_table(name: &str, field_index: u32, list: &p::ListField, rev: u64) -> impl IntoView {
    let count = list.tuples.len() as u32;
    let arity = list.labels.len();
    let rotation = list.rotation;
    let tuples = list.tuples.clone();
    let send = move |name: String, op: p::ListOp| {
        spawn_local(async move {
            if ui::edit_list(field_index, name, count, vec![op]).await {
                ui::preview(false).await;
            }
        });
    };
    let header = list
        .labels
        .iter()
        .map(|l| view! { <th scope="col">{l.clone()}</th> })
        .collect_view();
    let rows = tuples
        .iter()
        .enumerate()
        .map(|(r, t)| {
            let n = name.to_string();
            let cells = t
                .iter()
                .enumerate()
                .map(|(c, v)| {
                    let id = row_id(&n, r, c);
                    let n2 = n.clone();
                    view! {
                        <td><input id=id class="num" type="text" spellcheck="false" prop:value=v.clone()
                            aria-label=format!("{n2} point {} component {}", r + 1, c + 1)
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Enter" {
                                    ev.prevent_default();
                                    let comps: Option<Vec<String>> = (0..arity).map(|c| input_value(&row_id(&n2, r, c))).collect();
                                    if let Some(components) = comps {
                                        send(n2.clone(), p::ListOp::Set { index: r as u32, components });
                                    }
                                }
                            } /></td>
                    }
                })
                .collect_view();
            let angle = rotation
                .then(|| num(&t[3]))
                .flatten()
                .map(|a| view! { <td class="muted" data-tip="The angle in degrees (display only; the source keeps radians as written)">{format!("{}°", fmt_num(a.to_degrees(), 3))}</td> });
            let (n_set, n_ins, n_del) = (n.clone(), n.clone(), n.clone());
            let proposal = proposed_after(&tuples, r, rotation);
            view! {
                <tr data-row=r.to_string()>
                    <th>{(r + 1).to_string()}</th>
                    {cells}
                    {angle}
                    <td class="xrow-actions">
                        <button class="small" id=format!("le-{n}-{r}-set") data-tip="Apply this row"
                            disabled=move || ui().revision.get() != rev
                            on:click=move |_| {
                                let comps: Option<Vec<String>> = (0..arity).map(|c| input_value(&row_id(&n_set, r, c))).collect();
                                if let Some(components) = comps {
                                    send(n_set.clone(), p::ListOp::Set { index: r as u32, components });
                                }
                            }>"Apply"</button>
                        <button class="small secondary" id=format!("le-{n}-{r}-ins") data-tip="Insert a point after this one"
                            disabled=move || ui().revision.get() != rev
                            on:click=move |_| send(n_ins.clone(), p::ListOp::Insert { index: r as u32 + 1, components: proposal.clone() })>"+"</button>
                        <button class="small secondary" id=format!("le-{n}-{r}-del") data-tip="Delete this point"
                            aria-label=format!("Delete point {}", r + 1)
                            disabled=move || ui().revision.get() != rev
                            on:click=move |_| send(n_del.clone(), p::ListOp::Delete { index: r as u32 })>"×"</button>
                    </td>
                </tr>
            }
        })
        .collect_view();
    let n_add = name.to_string();
    let add_tuple = tuples
        .last()
        .cloned()
        .unwrap_or_else(|| vec!["0".to_string(); arity]);
    view! {
        <table class="xlist" data-field=name.to_string() data-count=count.to_string()>
            <thead><tr><th>"#"</th>{header}{rotation.then(|| view! { <th>"deg"</th> })}<th></th></tr></thead>
            <tbody>{rows}</tbody>
        </table>
        <button class="small secondary" id=format!("le-{name}-add") disabled=move || ui().revision.get() != rev
            data-tip="Add a point after the last one"
            on:click=move |_| send(n_add.clone(), p::ListOp::Insert { index: count, components: add_tuple.clone() })>"Add at end"</button>
    }
}

// ---------------------------------------------------------------------------
// The 2D outline editor
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub session: u64,
    pub revision: u64,
    pub item: String,
    pub field_index: u32,
    pub base_count: u32,
    /// The working copy: (x, z) lexemes.
    pub points: Vec<[String; 2]>,
    pub ops: Vec<p::ListOp>,
    pub selected: Option<usize>,
    /// The view: min x, min z, size (square).
    pub view: (f64, f64, f64),
    pub message: Option<String>,
    pub dragging: bool,
    pub snap: bool,
}

thread_local! {
    static OUTLINE: OnceCell<RwSignal<Option<Outline>>> = const { OnceCell::new() };
}

pub fn outline() -> RwSignal<Option<Outline>> {
    OUTLINE.with(|o| *o.get_or_init(|| RwSignal::new(None)))
}

const W: f64 = 320.0;

fn fit_view(points: &[[String; 2]]) -> (f64, f64, f64) {
    let xs: Vec<(f64, f64)> = points
        .iter()
        .filter_map(|p| Some((num(&p[0])?, num(&p[1])?)))
        .collect();
    if xs.is_empty() {
        return (-2.0, -2.0, 4.0);
    }
    let (mut x0, mut x1, mut z0, mut z1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (x, z) in &xs {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        z0 = z0.min(*z);
        z1 = z1.max(*z);
    }
    let size = ((x1 - x0).max(z1 - z0) * 1.5).max(1.0);
    let (cx, cz) = ((x0 + x1) / 2.0, (z0 + z1) / 2.0);
    (cx - size / 2.0, cz - size / 2.0, size)
}

/// Re-fit the view when a point lies outside its inner 90%.
fn refit(o: &mut Outline) {
    let (x0, z0, size) = o.view;
    let m = size * 0.05;
    let out = o.points.iter().any(|p| match (num(&p[0]), num(&p[1])) {
        (Some(x), Some(z)) => x < x0 + m || x > x0 + size - m || z < z0 + m || z > z0 + size - m,
        _ => false,
    });
    if out {
        o.view = fit_view(&o.points);
    }
}

fn nice_step(size: f64) -> f64 {
    let raw = size / 8.0;
    let p = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|s| *s >= raw)
        .unwrap_or(p * 10.0)
}

fn is_closed(points: &[[String; 2]]) -> bool {
    points.len() >= 2 && {
        let (a, b) = (&points[0], &points[points.len() - 1]);
        a == b || (num(&a[0]) == num(&b[0]) && num(&a[1]) == num(&b[1]) && num(&a[0]).is_some())
    }
}

fn open_outline(rev: u64, field_index: u32, tuples: Vec<Vec<String>>, _closed: bool) {
    let u = ui();
    let Some(sel) = u.selected.get_untracked() else {
        return;
    };
    if sel.revision != rev || u.revision.get_untracked() != rev {
        ui::flash("The Inspector is updating; try again.");
        return;
    }
    let points: Vec<[String; 2]> = tuples
        .into_iter()
        .filter(|t| t.len() == 2)
        .map(|t| [t[0].clone(), t[1].clone()])
        .collect();
    let view = fit_view(&points);
    outline().set(Some(Outline {
        session: sel.session,
        revision: rev,
        item: sel.id,
        field_index,
        base_count: points.len() as u32,
        points,
        ops: Vec::new(),
        selected: None,
        view,
        message: None,
        dragging: false,
        snap: false,
    }));
    spawn_local(async {
        crate::ipc::sleep(0).await;
        if let Some(el) = crate::element_by_id::<web_sys::HtmlElement>("outline-svg") {
            let _ = el.focus();
        }
    });
}

/// UI-MENU-1: a point drag in the outline editor is in progress.
pub fn dragging() -> bool {
    outline().with_untracked(|o| o.as_ref().is_some_and(|o| o.dragging))
}

/// Drop the session without any IPC: source and history stay untouched.
pub fn cancel_outline(why: &str) {
    if outline().get_untracked().is_some() {
        outline().set(None);
        ui::flash(&format!(
            "Outline editing canceled ({why}); the source is unchanged."
        ));
    }
}

/// Collapse runs of Set operations: within a run (no Insert / Delete in
/// between) only the last Set of each index matters.
pub fn compact(ops: &[p::ListOp]) -> Vec<p::ListOp> {
    let mut out: Vec<p::ListOp> = Vec::new();
    let mut run_start = 0;
    for op in ops {
        match op {
            p::ListOp::Set { index, .. } => {
                if let Some(pos) = out[run_start..]
                    .iter()
                    .position(|o| matches!(o, p::ListOp::Set { index: i, .. } if i == index))
                {
                    out.remove(run_start + pos);
                }
                out.push(op.clone());
            }
            _ => {
                out.push(op.clone());
                run_start = out.len();
            }
        }
    }
    out
}

/// Local mirror of Rust's outline rules, for immediate feedback only (Rust
/// re-checks every one of them).
fn delete_refusal(points: &[[String; 2]], i: usize) -> Option<String> {
    let n = points.len();
    let closed = is_closed(points);
    if closed && (i == 0 || i + 1 == n) {
        return Some(
            "This is the closing point of a closed outline; deleting it would open the outline."
                .into(),
        );
    }
    let min = if closed { 4 } else { 2 };
    (n <= min).then(|| {
        format!(
            "A {} outline needs at least {min} points.",
            if closed { "closed" } else { "open" }
        )
    })
}

fn insert_refusal(points: &[[String; 2]], at: usize) -> Option<String> {
    (is_closed(points) && (at == 0 || at == points.len()))
        .then(|| "A point before the first or after the last point would open the closed outline; insert it between two points.".into())
}

fn update(f: impl FnOnce(&mut Outline)) {
    outline().update(|o| {
        if let Some(o) = o {
            f(o)
        }
    });
}

/// Set point `i` (and its closing partner) to (x, z) lexemes.
fn set_point(o: &mut Outline, i: usize, x: String, z: String) {
    let n = o.points.len();
    let partner = (is_closed(&o.points) && n >= 2)
        .then(|| match i {
            0 => Some(n - 1),
            k if k + 1 == n => Some(0),
            _ => None,
        })
        .flatten();
    for k in std::iter::once(i).chain(partner) {
        o.points[k] = [x.clone(), z.clone()];
        o.ops.push(p::ListOp::Set {
            index: k as u32,
            components: vec![x.clone(), z.clone()],
        });
    }
}

fn insert_point(o: &mut Outline, at: usize, x: String, z: String) {
    if let Some(m) = insert_refusal(&o.points, at) {
        o.message = Some(m);
        return;
    }
    o.points.insert(at, [x.clone(), z.clone()]);
    o.ops.push(p::ListOp::Insert {
        index: at as u32,
        components: vec![x, z],
    });
    o.selected = Some(at);
    o.message = None;
    refit(o);
}

fn delete_point(o: &mut Outline, i: usize) {
    if let Some(m) = delete_refusal(&o.points, i) {
        o.message = Some(m);
        return;
    }
    o.points.remove(i);
    o.ops.push(p::ListOp::Delete { index: i as u32 });
    o.selected = None;
    o.message = None;
}

fn midpoint(o: &Outline, i: usize) -> Option<(String, String)> {
    let a = &o.points[i];
    let (ax, az) = (num(&a[0])?, num(&a[1])?);
    match o.points.get(i + 1) {
        Some(b) => Some((
            shortest((ax + num(&b[0])?) / 2.0),
            shortest((az + num(&b[1])?) / 2.0),
        )),
        None if i > 0 => {
            let p = &o.points[i - 1];
            Some((
                shortest(2.0 * ax - num(&p[0])?),
                shortest(2.0 * az - num(&p[1])?),
            ))
        }
        None => Some((shortest(ax + 0.5), shortest(az))),
    }
}

/// Model coordinates of a pointer event, as lexemes formatted to the
/// screen's precision (or snapped to the grid when Snap is on).
fn pointer_model(o: &Outline, ev: &web_sys::PointerEvent) -> Option<(String, String)> {
    let svg = crate::element_by_id::<web_sys::Element>("outline-svg")?;
    let r = svg.get_bounding_client_rect();
    if r.width() <= 0.0 {
        return None;
    }
    let (x0, z0, size) = o.view;
    let x = x0 + (ev.client_x() as f64 - r.left()) / r.width() * size;
    let z = z0 + (ev.client_y() as f64 - r.top()) / r.height() * size;
    if o.snap {
        let s = nice_step(size);
        let d = (-s.log10()).ceil().clamp(0.0, 6.0) as usize;
        return Some((
            fmt_num((x / s).round() * s, d),
            fmt_num((z / s).round() * s, d),
        ));
    }
    let upp = size / r.width();
    let d = (-upp.log10()).ceil().clamp(0.0, 6.0) as usize;
    Some((fmt_num(x, d), fmt_num(z, d)))
}

async fn apply_outline() {
    let Some(o) = outline().get_untracked() else {
        return;
    };
    if o.ops.is_empty() {
        outline().set(None);
        ui::flash("Outline: no change.");
        return;
    }
    let u = ui();
    let still = u
        .selected
        .get_untracked()
        .is_some_and(|s| s.id == o.item && s.revision == o.revision)
        && u.revision.get_untracked() == o.revision;
    if !still {
        update(|o| {
            o.message = Some("The document changed since editing started; Apply is refused. Cancel and edit again.".into())
        });
        return;
    }
    let ops = compact(&o.ops);
    if ui::edit_list(o.field_index, "crossSection".into(), o.base_count, ops).await {
        outline().set(None);
        ui::preview(false).await;
    } else {
        let m = u
            .field_error
            .get_untracked()
            .unwrap_or_else(|| "Not applied.".into());
        update(|o| o.message = Some(m));
    }
}

#[component]
pub fn OutlineEditor() -> impl IntoView {
    let o = outline();
    let u = ui();
    // The session belongs to one document and one revision. Any edit
    // elsewhere makes it stale (Apply refuses); another document cancels it.
    Effect::new(move |_| {
        let s = u.selected.get();
        if let Some(cur) = o.get_untracked() {
            if s.as_ref().is_none_or(|s| s.session != cur.session) {
                cancel_outline("the document or selection changed");
            }
        }
    });
    let shown = move || o.with(|o| o.is_some());
    let w = move |f: &dyn Fn(&Outline) -> String| o.with(|o| o.as_ref().map(f).unwrap_or_default());
    view! {
        <div class="outline-editor" id="outline-editor" role="dialog" aria-modal="false"
            aria-labelledby="outline-title" hidden=move || !shown()>
            <div class="outline-head">
                <h3 id="outline-title">"Outline editor (crossSection)"</h3>
                <span class="muted">"x → right · z ↓ down (VRML97 outline plane)"</span>
            </div>
            <svg id="outline-svg" class="outline-svg" tabindex="0" role="img"
                aria-label="Outline points. Arrow keys move the selected point; Delete removes it."
                viewBox=format!("0 0 {W} {W}")
                on:keydown=move |ev: web_sys::KeyboardEvent| outline_key(ev)
                on:pointermove=move |ev: web_sys::PointerEvent| {
                    let Some(cur) = o.get_untracked() else { return };
                    if !cur.dragging { return; }
                    let (Some(i), Some((x, z))) = (cur.selected, pointer_model(&cur, &ev)) else { return };
                    update(|o| set_point(o, i, x, z));
                }
                on:pointerup=move |_| update(|o| { o.dragging = false; refit(o); })
                on:pointercancel=move |_| update(|o| { o.dragging = false; refit(o); })>
                {move || o.with(|o| o.as_ref().map(draw))}
            </svg>
            <div class="outline-status" id="outline-status" role="status" aria-live="polite">
                {move || w(&|o| format!(
                    "{} points · {} · {} change(s) pending{}",
                    o.points.len(),
                    if is_closed(&o.points) { "closed" } else { "open" },
                    o.ops.len(),
                    o.selected.map(|i| format!(" · point {} selected{}", i + 1,
                        if is_closed(&o.points) && (i == 0 || i + 1 == o.points.len()) { " (closing point: first = last)" } else { "" })).unwrap_or_default()
                ))}
            </div>
            <div class="outline-message" id="outline-message" role="alert"
                hidden=move || o.with(|o| o.as_ref().is_none_or(|o| o.message.is_none()))>
                {move || w(&|o| o.message.clone().unwrap_or_default())}
            </div>
            <div class="outline-stale" id="outline-stale" role="status"
                hidden=move || o.with(|o| o.as_ref().is_none_or(|o| o.revision == u.revision.get()))>
                "The document changed since editing started; Apply will be refused."
            </div>
            <div class="controls outline-point">
                <label class="comp">"x"<input id="outline-x" class="num" type="text" spellcheck="false"
                    prop:value=move || w(&|o| o.selected.and_then(|i| o.points.get(i)).map(|p| p[0].clone()).unwrap_or_default()) /></label>
                <label class="comp">"z"<input id="outline-z" class="num" type="text" spellcheck="false"
                    prop:value=move || w(&|o| o.selected.and_then(|i| o.points.get(i)).map(|p| p[1].clone()).unwrap_or_default()) /></label>
                <button class="small" id="outline-set" disabled=move || o.with(|o| o.as_ref().is_none_or(|o| o.selected.is_none()))
                    data-tip="Set the selected point to these coordinates (exact text, validated by Rust on Apply)"
                    on:click=move |_| {
                        let (Some(x), Some(z)) = (input_value("outline-x"), input_value("outline-z")) else { return };
                        update(|o| if let Some(i) = o.selected { set_point(o, i, x.trim().to_string(), z.trim().to_string()); o.message = None; refit(o); });
                    }>"Set point"</button>
                <label class="toggle"><input id="outline-snap" type="checkbox"
                    prop:checked=move || o.with(|o| o.as_ref().is_some_and(|o| o.snap))
                    on:change=move |ev| { let on = event_target_checked(&ev); update(|o| o.snap = on); } />"Snap drags to grid"</label>
            </div>
            <div class="controls outline-actions">
                <button class="small secondary" id="outline-add" data-tip="Add a point: after the last point of an open outline, or between the last two points of a closed one"
                    on:click=move |_| update(|o| {
                        let n = o.points.len();
                        if n == 0 { insert_point(o, 0, "0".into(), "0".into()); return; }
                        let (at, from) = if is_closed(&o.points) && n >= 2 { (n - 1, n - 2) } else { (n, n - 1) };
                        if let Some((x, z)) = midpoint(o, from) { insert_point(o, at, x, z); }
                    })>"Add point"</button>
                <button class="small secondary" id="outline-insert" disabled=move || o.with(|o| o.as_ref().is_none_or(|o| o.selected.is_none()))
                    data-tip="Insert a point halfway between the selected point and the next one (or click a segment)"
                    on:click=move |_| update(|o| {
                        if let Some(i) = o.selected {
                            if let Some((x, z)) = midpoint(o, i) { insert_point(o, i + 1, x, z); }
                        }
                    })>"Insert after selected"</button>
                <button class="small secondary" id="outline-delete" disabled=move || o.with(|o| o.as_ref().is_none_or(|o| o.selected.is_none()))
                    on:click=move |_| update(|o| if let Some(i) = o.selected { delete_point(o, i) })>"Delete point"</button>
                <span class="sep"></span>
                <button class="small" id="outline-apply" on:click=move |_| spawn_local(apply_outline())
                    data-tip="Write all changes to the source as one undo step">"Apply"</button>
                <button class="small secondary" id="outline-cancel" on:click=move |_| cancel_outline("Cancel")
                    data-tip="Discard all changes; the source and undo history stay unchanged">"Cancel"</button>
            </div>
        </div>
    }
}

fn outline_key(ev: web_sys::KeyboardEvent) {
    let Some(o) = outline().get_untracked() else {
        return;
    };
    let Some(i) = o.selected else {
        return;
    };
    let step = nice_step(o.view.2);
    let (dx, dz) = match ev.key().as_str() {
        "ArrowLeft" => (-step, 0.0),
        "ArrowRight" => (step, 0.0),
        "ArrowUp" => (0.0, -step),
        "ArrowDown" => (0.0, step),
        "Delete" | "Backspace" => {
            ev.prevent_default();
            update(|o| delete_point(o, i));
            return;
        }
        _ => return,
    };
    ev.prevent_default();
    let p = &o.points[i];
    let (Some(x), Some(z)) = (num(&p[0]), num(&p[1])) else {
        return;
    };
    let d = (-step.log10()).ceil().clamp(0.0, 6.0) as usize;
    let (x, z) = (fmt_num(x + dx, d.max(1)), fmt_num(z + dz, d.max(1)));
    update(|o| set_point(o, i, x, z));
}

fn draw(o: &Outline) -> AnyView {
    let (x0, z0, size) = o.view;
    let sx = move |x: f64| (x - x0) / size * W;
    let sz = move |z: f64| (z - z0) / size * W;
    let step = nice_step(size);
    let mut grid = Vec::new();
    let mut g = (x0 / step).ceil() * step;
    while g <= x0 + size {
        grid.push((sx(g), 0.0, sx(g), W, g.abs() < step / 1e6));
        g += step;
    }
    let mut g = (z0 / step).ceil() * step;
    while g <= z0 + size {
        grid.push((0.0, sz(g), W, sz(g), g.abs() < step / 1e6));
        g += step;
    }
    let pts: Vec<Option<(f64, f64)>> = o
        .points
        .iter()
        .map(|p| Some((sx(num(&p[0])?), sz(num(&p[1])?))))
        .collect();
    let closed = is_closed(&o.points);
    let n = pts.len();
    type Seg = (usize, (f64, f64), (f64, f64));
    let segs: Vec<Seg> = (0..n.saturating_sub(1))
        .filter_map(|i| Some((i, pts[i]?, pts[i + 1]?)))
        .collect();
    let sel = o.selected;
    view! {
        <g class="outline-grid">
            {grid.into_iter().map(|(a, b, c, d, axis)| view! {
                <line x1=a y1=b x2=c y2=d class=if axis { "axis" } else { "grid" } />
            }).collect_view()}
        </g>
        <text x=W - 4.0 y=sz(0.0).clamp(12.0, W - 4.0) - 4.0 class="axis-label x" text-anchor="end">"+x →"</text>
        <text x=sx(0.0).clamp(4.0, W - 40.0) + 4.0 y=W - 6.0 class="axis-label z">"+z ↓"</text>
        <g class="outline-segs">
            {segs.into_iter().map(|(i, a, b)| view! {
                <line x1=a.0 y1=a.1 x2=b.0 y2=b.1 class="seg" />
                <line x1=a.0 y1=a.1 x2=b.0 y2=b.1 class="seg-hit" data-seg=i.to_string()
                    on:pointerdown=move |ev: web_sys::PointerEvent| {
                        ev.prevent_default();
                        let Some(cur) = outline().get_untracked() else { return };
                        if let Some((x, z)) = pointer_model(&cur, &ev) {
                            update(|o| insert_point(o, i + 1, x, z));
                        }
                    } />
            }).collect_view()}
        </g>
        <g class="outline-pts">
            {pts.iter().enumerate().filter_map(|(i, p)| p.map(|p| (i, p)))
                // The closing duplicate is drawn once (as point 1).
                .filter(|(i, _)| !(closed && *i + 1 == n && n > 1))
                .map(|(i, (x, y))| {
                    let is_sel = sel == Some(i) || (closed && i == 0 && sel == Some(n - 1));
                    view! {
                        <circle cx=x cy=y r="6" class=if is_sel { "pt selected" } else { "pt" } data-index=i.to_string()
                            on:pointerdown=move |ev: web_sys::PointerEvent| {
                                ev.prevent_default();
                                if let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                                    if let Some(svg) = t.closest("svg").ok().flatten() {
                                        let _ = svg.set_pointer_capture(ev.pointer_id());
                                    }
                                }
                                update(|o| { o.selected = Some(i); o.dragging = true; o.message = None; });
                            } />
                        <text x=x + 8.0 y=y - 8.0 class="pt-label">{(i + 1).to_string()}</text>
                    }
                }).collect_view()}
        </g>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_keeps_the_last_set_per_index_within_a_run() {
        let s = |i: u32, x: &str| p::ListOp::Set {
            index: i,
            components: vec![x.into(), "0".into()],
        };
        let ins = p::ListOp::Insert {
            index: 1,
            components: vec!["0".into(), "0".into()],
        };
        let ops = vec![
            s(1, "a"),
            s(2, "b"),
            s(1, "c"),
            ins.clone(),
            s(1, "d"),
            s(1, "e"),
        ];
        assert_eq!(compact(&ops), vec![s(2, "b"), s(1, "c"), ins, s(1, "e")]);
    }

    #[test]
    fn formatting_never_shows_negative_zero_or_trailing_zeros() {
        assert_eq!(fmt_num(-0.0001, 2), "0");
        assert_eq!(fmt_num(0.5, 4), "0.5");
        assert_eq!(fmt_num(2.0, 3), "2");
        assert_eq!(fmt_num(-1.25, 1), "-1.2");
    }

    #[test]
    fn local_rules_mirror_rust() {
        let pts = |v: &[(&str, &str)]| {
            v.iter()
                .map(|(a, b)| [a.to_string(), b.to_string()])
                .collect::<Vec<_>>()
        };
        let closed = pts(&[("1", "1"), ("1", "-1"), ("-1", "-1"), ("1", "1")]);
        assert!(is_closed(&closed));
        assert!(delete_refusal(&closed, 0).is_some());
        assert!(delete_refusal(&closed, 1).is_some()); // 4 is the closed minimum
        assert!(insert_refusal(&closed, 0).is_some() && insert_refusal(&closed, 4).is_some());
        assert!(insert_refusal(&closed, 2).is_none());
        let open = pts(&[("0", "0"), ("1", "0"), ("1", "1")]);
        assert!(delete_refusal(&open, 0).is_none());
        assert!(delete_refusal(&pts(&[("0", "0"), ("1", "0")]), 0).is_some());
    }
}
