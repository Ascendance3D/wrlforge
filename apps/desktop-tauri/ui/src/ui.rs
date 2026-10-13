// SPDX-License-Identifier: GPL-3.0-or-later
//! Reactive display state and the document-level actions (open, save, reload,
//! analysis, preview). Display only: every value here is a copy of something
//! the Rust backend reported.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wrlforge_desktop_protocol as p;

use crate::editor::{self, CORE};
use crate::ipc::{self, call, NoArgs, Session};

#[derive(Clone, Copy)]
pub struct Ui {
    pub doc: RwSignal<Option<p::DocumentInfo>>,
    pub revision: RwSignal<u64>,
    pub dirty: RwSignal<bool>,
    pub can_undo: RwSignal<bool>,
    pub can_redo: RwSignal<bool>,
    pub cursor: RwSignal<(u32, u32, u32)>,
    pub message: RwSignal<String>,
    pub analysis: RwSignal<Option<p::Analysis>>,
    /// The selected Scene Tree item, valid in exactly one revision.
    pub selected: RwSignal<Option<Selected>>,
    pub inspection: RwSignal<Option<p::Inspection>>,
    /// The last Inspector refusal, shown until the next selection or edit.
    pub field_error: RwSignal<Option<String>>,
    pub conflict: RwSignal<Option<String>>,
    pub preview_status: RwSignal<String>,
    /// TEXTURE-LOCAL-1: the Rust check of the previewed revision's
    /// `ImageTexture` URLs (textures the preview cannot load).
    pub texture_warnings: RwSignal<Vec<p::TextureWarning>>,
    /// XITE-16-UPGRADE-2: the Rust-proven invalid ROUTEs of the revision ON
    /// SCREEN. X_ITE 16.5.0 drops such a ROUTE and renders the rest.
    pub route_warnings: RwSignal<Vec<p::RouteWarning>>,
    pub preview_enabled: RwSignal<bool>,
    pub last_save: RwSignal<Option<String>>,
    /// X_ITE scene loads this run (lets tests prove a theme switch never
    /// reloads the preview).
    pub preview_loads: RwSignal<u64>,
    /// The last Create refusal, shown until the next Create or document.
    pub create_error: RwSignal<Option<String>>,
    /// VISUAL-2: the viewport Select mode (a click selects, a drag navigates).
    pub pick_mode: RwSignal<bool>,
    /// B2-CAMERA: the authored viewpoints of the scene on screen (the
    /// viewport's viewpoint list). View state only.
    pub viewpoints: RwSignal<crate::ipc::ViewpointList>,
    /// The last viewport pick result: (`ok` | `none` | `refused`, text).
    pub pick_message: RwSignal<Option<(String, String)>>,
    /// VISUAL-3A: the viewport Move tool (translation gizmo). One viewport
    /// tool at a time: Select and Move exclude each other.
    pub move_mode: RwSignal<bool>,
    /// VISUAL-4B: the Rotate and Scale tools (exclusive with Select / Move).
    pub rotate_mode: RwSignal<bool>,
    pub scale_mode: RwSignal<bool>,
    /// The Move tool's status line: (`ok` | `none` | `refused`, text).
    pub gizmo_message: RwSignal<Option<(String, String)>>,
    /// B2: why the selected node has no viewport outline, if it has none.
    pub highlight_message: RwSignal<Option<String>>,
    /// B1: the shared PROTO definitions the user agreed to edit, per
    /// document session: (session, PROTO name). Asked once per definition.
    pub shared_ok: RwSignal<Vec<(u64, String)>>,
    /// D4: Create places new objects at the origin (else in view).
    pub place_origin: RwSignal<bool>,
    /// NATIVE-RENDER-1: the hidden native viewport's state (default off).
    pub native: RwSignal<p::NativeState>,
}

/// A Scene Tree selection. Item ids are SOURCE spans of one revision, so
/// the id is meaningless without the session and revision it came from.
/// Every acknowledged change carries it forward through Rust's exact change
/// mapping, or clears it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    pub session: u64,
    pub revision: u64,
    pub id: String,
}

impl Selected {
    pub fn is(&self, id: &str) -> bool {
        self.id == id
    }
}

thread_local! {
    static UI: OnceCell<Ui> = const { OnceCell::new() };
}

/// Why offsets from a projection of (`session`, `revision`) may NOT be used
/// now, or `None` when they may: the projection is of exactly the text the
/// widget shows and Rust holds (no edit in flight, no newer revision).
pub fn stale_reason(session: u64, revision: u64) -> Option<String> {
    let (cur_s, cur_r, busy) = CORE.with_borrow(|c| (c.session, c.revision, c.busy));
    if cur_s != Some(session) {
        Some("another document is open".into())
    } else if busy {
        Some("an edit is being applied".into())
    } else if cur_r != revision {
        Some(format!(
            "it shows revision {revision}, the document is at {cur_r}"
        ))
    } else {
        None
    }
}

/// Select a source span that a projection of (`session`, `revision`)
/// reported. Refused -- nothing selected, nothing changed -- when that
/// projection is not of the current text. Returns whether it selected.
pub fn select_from(what: &str, session: u64, revision: u64, from: u64, to: u64) -> bool {
    match stale_reason(session, revision) {
        None => {
            editor::select(from, to);
            true
        }
        Some(why) => {
            flash(&format!(
                "{what} is updating ({why}); selection not changed."
            ));
            false
        }
    }
}

/// A Scene Tree click / Enter on an item rendered from the analysis of
/// (`session`, `revision`). The rendered DOM may lag the newest analysis, so
/// the revision is the one the item was RENDERED with, and it must also be
/// the analysis currently held and the document's current revision.
pub fn tree_select(session: u64, revision: u64, id: String, from: u64, to: u64) -> bool {
    let held = ui()
        .analysis
        .with_untracked(|a| a.as_ref().map(|a| (a.session, a.revision)));
    if held != Some((session, revision)) {
        flash("Scene Tree is updating (a newer analysis is being shown); selection not changed.");
        return false;
    }
    if !select_from("Scene Tree", session, revision, from, to) {
        // Catch the tree up now instead of after the debounce.
        spawn_local(analyze());
        return false;
    }
    adopt_selection(Selected {
        session,
        revision,
        id,
    });
    true
}

/// THE selection authority: the Scene Tree and the viewport both select
/// through here. Never shows the previous item's fields under a new one.
pub fn adopt_selection(sel: Selected) {
    let u = ui();
    // A selection change never redirects a drag to another node.
    if u.selected.get_untracked().as_ref() != Some(&sel) {
        crate::gizmo::cancel("the selection changed");
    }
    if u.selected.get_untracked().as_ref() != Some(&sel) {
        u.field_error.set(None);
        u.inspection.set(None);
    }
    u.selected.set(Some(sel.clone()));
    // B2: the viewport outline follows the selection (view state only).
    crate::highlight::start();
    spawn_local(inspect(sel));
}

/// Select the Scene Tree item a PROVEN viewport pick named, in exactly the
/// revision Rust resolved it against. The node's source span is selected
/// (revealed) without taking the keyboard focus. The
/// Scene Tree must show that revision (it is brought up to date first) and
/// must hold the item; otherwise nothing changes. Returns the item label.
pub async fn pick_select(session: u64, revision: u64, id: String) -> Result<String, String> {
    if let Some(why) = stale_reason(session, revision) {
        return Err(why);
    }
    let held = |u: Ui| {
        u.analysis
            .with_untracked(|a| a.as_ref().map(|a| (a.session, a.revision)))
    };
    let u = ui();
    if held(u) != Some((session, revision)) {
        analyze().await;
        if let Some(why) = stale_reason(session, revision) {
            return Err(why);
        }
        if held(u) != Some((session, revision)) {
            return Err("the Scene Tree is updating".into());
        }
    }
    let found = u.analysis.with_untracked(|a| {
        a.as_ref()
            .and_then(|a| a.items.iter().find(|i| i.id == id))
            .map(|i| (i.label.clone(), i.view_from, i.view_to))
    });
    let Some((label, from, to)) = found else {
        return Err("the object is not in the Scene Tree".into());
    };
    adopt_selection(Selected {
        session,
        revision,
        id: id.clone(),
    });
    crate::panels::reveal_tree_item_soon(&id);
    // B3: reveal the node in the Source as well: its span is selected and
    // scrolled into view and Ln/Col follow it. A selection only -- no edit,
    // no revision, no undo step -- and the keyboard focus stays where it is
    // (the viewport keeps it after a pick). The span is of `revision`, which
    // `stale_reason` just proved is the text the editor shows.
    editor::select_quiet(from as u32, to as u32);
    Ok(label)
}

pub fn ui() -> Ui {
    UI.with(|u| {
        *u.get_or_init(|| Ui {
            doc: RwSignal::new(None),
            revision: RwSignal::new(0),
            dirty: RwSignal::new(false),
            can_undo: RwSignal::new(false),
            can_redo: RwSignal::new(false),
            cursor: RwSignal::new((1, 1, 0)),
            message: RwSignal::new(String::new()),
            analysis: RwSignal::new(None),
            selected: RwSignal::new(None),
            inspection: RwSignal::new(None),
            field_error: RwSignal::new(None),
            conflict: RwSignal::new(None),
            preview_status: RwSignal::new("idle".into()),
            texture_warnings: RwSignal::new(Vec::new()),
            route_warnings: RwSignal::new(Vec::new()),
            preview_enabled: RwSignal::new(true),
            last_save: RwSignal::new(None),
            preview_loads: RwSignal::new(0),
            create_error: RwSignal::new(None),
            pick_mode: RwSignal::new(false),
            viewpoints: RwSignal::new(Default::default()),
            pick_message: RwSignal::new(None),
            move_mode: RwSignal::new(false),
            rotate_mode: RwSignal::new(false),
            scale_mode: RwSignal::new(false),
            gizmo_message: RwSignal::new(None),
            highlight_message: RwSignal::new(None),
            shared_ok: RwSignal::new(Vec::new()),
            place_origin: RwSignal::new(false),
            native: RwSignal::new(p::NativeState::default()),
        })
    })
}

/// D4: where Create puts a new object. In front of the camera, at the
/// point of its view axis nearest the world origin (at least 4 units out),
/// shifted sideways by 2.5 units per object created from the same view so
/// objects do not land inside each other. `None` (the origin) when the user
/// chose "Place at origin" or the camera cannot be read -- never a guess.
fn placement() -> Option<[f64; 3]> {
    let u = ui();
    if u.place_origin.get_untracked() {
        return None;
    }
    let cam = crate::ipc::gizmo_camera()?;
    let m = cam.view;
    // Column-major world->eye: R[r][c] = m[c*4+r], t = m[12..15].
    let r = |row: usize, col: usize| m[col * 4 + row];
    let t = [m[12], m[13], m[14]];
    let pos: Vec<f64> = (0..3)
        .map(|c| -(0..3).map(|row| r(row, c) * t[row]).sum::<f64>())
        .collect();
    let fwd = [-r(2, 0), -r(2, 1), -r(2, 2)];
    let right = [r(0, 0), r(0, 1), r(0, 2)];
    if pos.iter().chain(&fwd).chain(&right).any(|v| !v.is_finite()) {
        return None;
    }
    let along = -(pos[0] * fwd[0] + pos[1] * fwd[1] + pos[2] * fwd[2]);
    let d = if along > 4.0 { along } else { 4.0 };
    let key = (0..3).map(|i| (pos[i] * 1000.0).round() as i64).collect::<Vec<_>>();
    let k = PLACED.with_borrow_mut(|p| {
        if p.0 != key {
            *p = (key, 0);
        } else {
            p.1 += 1;
        }
        p.1
    });
    let side = 2.5 * k as f64;
    Some([0, 1, 2].map(|i| pos[i] + fwd[i] * d + right[i] * side))
}

thread_local! {
    /// D4: (camera position key, objects created from it so far).
    static PLACED: std::cell::RefCell<(Vec<i64>, u32)> = const { std::cell::RefCell::new((Vec::new(), 0)) };
}

pub fn flash(m: &str) {
    ui().message.set(m.to_string());
}

pub fn cursor(line: u32, col: u32, sel: u32) {
    ui().cursor.set((line, col, sel));
}

pub fn doc_loaded(doc: &p::DocumentInfo) {
    let u = ui();
    u.doc.set(Some(doc.clone()));
    u.conflict.set(None);
    u.selected.set(None);
    u.inspection.set(None);
    u.pick_message.set(None);
    crate::gizmo::reset("the document was replaced");
    crate::pick::retire("preview-scene-replaced");
    // The previous document's Scene Tree and diagnostics never show under
    // the new one, not even until its first analysis arrives.
    u.analysis.set(None);
    state_changed(doc.revision, doc.dirty, doc.can_undo, doc.can_redo);
    set_title(doc);
}

/// The native window title is owned by Rust (`window_title`); the UI only
/// says which session it shows.
fn set_title(doc: &p::DocumentInfo) {
    let session = doc.session;
    spawn_local(async move {
        let _ = call::<()>("window_title", Session { session }).await;
    });
}

/// Every acknowledged change lands here; schedules derived views.
pub fn state_changed(revision: u64, dirty: bool, can_undo: bool, can_redo: bool) {
    let u = ui();
    let title_changes = u.dirty.get_untracked() != dirty;
    u.revision.set(revision);
    // Unchanged flags are not re-set: no re-render per keystroke for them.
    if title_changes {
        u.dirty.set(dirty);
    }
    if u.can_undo.get_untracked() != can_undo {
        u.can_undo.set(can_undo);
    }
    if u.can_redo.get_untracked() != can_redo {
        u.can_redo.set(can_redo);
    }
    // The window title shows only the name and the dirty mark: ask Rust for
    // it when that mark changes, not on every keystroke.
    if title_changes {
        if let Some(doc) = u.doc.get_untracked() {
            set_title(&doc);
        }
    }
    crate::pick::document_changed(CORE.with_borrow(|c| c.session), revision);
    if crate::gizmo::GIZMO.with_borrow(|g| {
        g.drag
            .as_ref()
            .is_some_and(|d| d.target.revision != revision)
    }) {
        crate::gizmo::cancel("the source changed");
    }
    let generation = CORE.with_borrow(|c| c.generation);
    spawn_local(async move {
        ipc::sleep(250).await;
        if CORE.with_borrow(|c| c.generation == generation && c.analyzed != Some(revision)) {
            analyze().await;
        }
    });
    spawn_local(async move {
        ipc::sleep(700).await; // the JS preview-scheduler debounce
        if CORE.with_borrow(|c| c.generation) == generation && ui().preview_enabled.get_untracked()
        {
            preview(false).await;
        }
    });
}

pub async fn analyze() {
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    match call::<p::Analysis>("doc_analyze", Session { session }).await {
        Ok(a) => {
            if let Some(sel) = apply_analysis(a) {
                inspect(sel).await;
            }
        }
        Err(e) => flash(&format!("Analysis failed: {e}")),
    }
}

/// Adopt one analysis reply. A reply for a document that is no longer open,
/// or older than the analysis already shown, is discarded (returns `None`
/// with nothing changed). Returns the selection to re-inspect, if any.
pub fn apply_analysis(a: p::Analysis) -> Option<Selected> {
    let u = ui();
    if CORE.with_borrow(|c| c.session) != Some(a.session)
        || u.analysis.with_untracked(|cur| {
            cur.as_ref()
                .is_some_and(|c| c.session == a.session && c.revision > a.revision)
        })
    {
        CORE.with_borrow_mut(|c| c.stale_analyses += 1);
        return None;
    }
    CORE.with_borrow_mut(|c| c.analyzed = Some(a.revision));
    // Colors and underlines only if this parse is of the text on screen
    // (session, revision, view hash); otherwise a newer analysis is already
    // scheduled.
    crate::syntax::apply(&a);
    if a.highlights_truncated {
        flash("Large document: syntax colors cover the first 400,000 tokens only.");
    }
    // The selection is checked against the analysis of ITS OWN revision only:
    // an id that is absent there is gone; an analysis of another revision
    // says nothing about it.
    let sel = u
        .selected
        .get_untracked()
        .filter(|s| s.session == a.session && s.revision == a.revision);
    let keep = sel.as_ref().map(|s| a.items.iter().any(|i| s.is(&i.id)));
    u.analysis.set(Some(a));
    match keep {
        Some(true) => sel,
        Some(false) => {
            u.selected.set(None);
            u.inspection.set(None);
            None
        }
        None => None,
    }
}

/// Ask Rust for the Inspector data of `sel` in `sel.revision`. Only the
/// newest request may land, and only while the selection and the document
/// are still exactly what it asked about.
pub async fn inspect(sel: Selected) {
    let seq = CORE.with_borrow_mut(|c| {
        c.inspect_seq += 1;
        c.inspect_seq
    });
    #[derive(serde::Serialize)]
    struct A {
        session: u64,
        item: String,
        revision: u64,
    }
    let r = call::<p::InspectOutcome>(
        "doc_inspect",
        A {
            session: sel.session,
            item: sel.id.clone(),
            revision: sel.revision,
        },
    )
    .await;
    match r {
        Ok(o) => {
            apply_inspection(seq, &sel, o);
        }
        Err(e) => flash(&format!("Inspector failed: {e}")),
    }
}

/// Adopt one Inspector reply, or discard it (returns false, nothing changed)
/// when a newer request was made, the selection moved, or the reply is not of
/// the selection's revision.
pub fn apply_inspection(seq: u64, sel: &Selected, o: p::InspectOutcome) -> bool {
    let u = ui();
    let newest = CORE.with_borrow(|c| c.inspect_seq == seq);
    let still = u.selected.get_untracked().as_ref() == Some(sel);
    let ok = newest
        && still
        && match &o {
            p::InspectOutcome::Found { inspection } => {
                inspection.revision == sel.revision && inspection.id == sel.id
            }
            p::InspectOutcome::Missing => true,
            p::InspectOutcome::Stale { .. } => false,
        };
    if !ok {
        CORE.with_borrow_mut(|c| c.stale_inspections += 1);
        return false;
    }
    match o {
        p::InspectOutcome::Found { inspection } => u.inspection.set(Some(inspection)),
        _ => {
            u.selected.set(None);
            u.inspection.set(None);
        }
    }
    true
}

/// B1: whether an edit of `insp` may go to Rust as a shared-definition
/// edit. `None`: the node is not in a PROTO body (no consent needed).
/// `Some(true)`: the user agreed (now, in the native confirmation, or
/// earlier for this definition and document). `Some(false)`: declined --
/// nothing is sent. Rust refuses a shared edit without consent anyway.
async fn shared_consent(session: u64, insp: &p::Inspection) -> Option<bool> {
    let shared = insp.node.as_ref()?.shared.clone()?;
    Some(confirm_shared(session, shared).await)
}

/// The one consent path (Inspector edits and the banner button).
pub async fn confirm_shared(session: u64, shared: p::SharedProto) -> bool {
    let u = ui();
    let known = u
        .shared_ok
        .with_untracked(|v| v.iter().any(|(s, n)| *s == session && *n == shared.proto));
    if known {
        return true;
    }
    #[derive(serde::Serialize)]
    struct A {
        proto: String,
        instances: u32,
    }
    let yes = call::<bool>(
        "confirm_shared_edit",
        A {
            proto: shared.proto.clone(),
            instances: shared.instances,
        },
    )
    .await
    .unwrap_or(false);
    if yes {
        u.shared_ok.update(|v| v.push((session, shared.proto.clone())));
        u.field_error.set(None);
    } else {
        u.field_error.set(Some(format!(
            "Not changed: editing the shared definition PROTO {} was not confirmed.",
            shared.proto
        )));
    }
    yes
}

/// Send one Inspector field edit to Rust. The request names the node by the
/// item id of the revision the Inspector was built from; Rust refuses it if
/// the document moved on. The UI adopts nothing until Rust applies it.
pub async fn edit_field(field_index: u32, field_name: String, components: Vec<p::FieldInput>) {
    let Some((session, insp)) = field_target(&field_name).await else {
        return;
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::FieldEditRequest,
    }
    let confirm_shared = match shared_consent(session, &insp).await {
        Some(false) => return Default::default(),
        c => c.is_some(),
    };
    let request = p::FieldEditRequest {
        confirm_shared,
        session,
        base_revision: insp.revision,
        item: insp.id.clone(),
        field_index,
        field_name: field_name.clone(),
        components,
    };
    let o = call::<p::FieldEditOutcome>("doc_edit_field", A { request }).await;
    field_outcome(session, &field_name, o).await;
}

/// VISUAL-4A: one list-field change (`ops` is one Inspector action or one
/// whole outline-editing session). Rust plans, proves and applies it as ONE
/// undo step. Returns whether the document changed.
pub async fn edit_list(
    field_index: u32,
    field_name: String,
    expected_count: u32,
    ops: Vec<p::ListOp>,
) -> bool {
    let Some((session, insp)) = field_target(&field_name).await else {
        return false;
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::ListEditRequest,
    }
    let confirm_shared = match shared_consent(session, &insp).await {
        Some(false) => return Default::default(),
        c => c.is_some(),
    };
    let request = p::ListEditRequest {
        confirm_shared,
        session,
        base_revision: insp.revision,
        item: insp.id.clone(),
        field_index,
        field_name: field_name.clone(),
        expected_count,
        ops,
    };
    let o = call::<p::FieldEditOutcome>("doc_edit_list", A { request }).await;
    field_outcome(session, &field_name, o).await
}

/// VISUAL-4A: write one absent field because the user asked for it.
pub async fn add_field(field_name: String, value: p::AddFieldValue) -> bool {
    let Some((session, insp)) = field_target(&field_name).await else {
        return false;
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::AddFieldRequest,
    }
    let confirm_shared = match shared_consent(session, &insp).await {
        Some(false) => return Default::default(),
        c => c.is_some(),
    };
    let request = p::AddFieldRequest {
        confirm_shared,
        session,
        base_revision: insp.revision,
        item: insp.id.clone(),
        field_name: field_name.clone(),
        value,
    };
    let o = call::<p::FieldEditOutcome>("doc_add_field", A { request }).await;
    field_outcome(session, &field_name, o).await
}

/// B2: one MFString change (`url`, `info`, ...). `expected` is the list the
/// Inspector showed; Rust refuses any other. One undo step.
pub async fn edit_strings(
    field_index: u32,
    field_name: String,
    expected: Vec<String>,
    ops: Vec<p::StringOp>,
) -> bool {
    let Some((session, insp)) = field_target(&field_name).await else {
        return false;
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::StringEditRequest,
    }
    let confirm_shared = match shared_consent(session, &insp).await {
        Some(false) => return Default::default(),
        c => c.is_some(),
    };
    let request = p::StringEditRequest {
        confirm_shared,
        session,
        base_revision: insp.revision,
        item: insp.id.clone(),
        field_index,
        field_name: field_name.clone(),
        expected,
        ops,
    };
    let o = call::<p::FieldEditOutcome>("doc_edit_strings", A { request }).await;
    field_outcome(session, &field_name, o).await
}

/// B2: "Choose image…" -- a native dialog run by Rust in the document's
/// folder. Returns the RELATIVE url, or `None` (canceled, or refused with a
/// visible reason).
pub async fn choose_texture() -> Option<String> {
    let session = CORE.with_borrow(|c| c.session)?;
    match call::<p::TextureChoice>("doc_choose_texture", Session { session }).await {
        Ok(p::TextureChoice::Chosen { url }) => Some(url),
        Ok(p::TextureChoice::Canceled) => None,
        Ok(p::TextureChoice::Refused { message }) => {
            let m = format!("No texture chosen: {message}");
            ui().field_error.set(Some(m.clone()));
            flash(&m);
            None
        }
        Err(e) => {
            flash(&format!("Choose image failed: {e}"));
            None
        }
    }
}

/// B2: "Add texture" on an Appearance with no texture field: choose an image,
/// then write `texture ImageTexture { url "<url>" }` (one undo step).
pub async fn add_texture() -> bool {
    let Some(url) = choose_texture().await else {
        return false;
    };
    let Some((session, insp)) = field_target("texture").await else {
        return false;
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::AddTextureRequest,
    }
    let confirm_shared = match shared_consent(session, &insp).await {
        Some(false) => return false,
        c => c.is_some(),
    };
    let request = p::AddTextureRequest {
        confirm_shared,
        session,
        base_revision: insp.revision,
        item: insp.id.clone(),
        url,
    };
    let o = call::<p::FieldEditOutcome>("doc_add_texture", A { request }).await;
    field_outcome(session, "texture", o).await
}

/// The session and the Inspector a field change is sent against, or `None`
/// (with a visible reason) while the Inspector is not of the current text.
async fn field_target(field_name: &str) -> Option<(u64, p::Inspection)> {
    editor::idle().await;
    let u = ui();
    let (Some(session), Some(insp)) = (
        CORE.with_borrow(|c| c.session),
        u.inspection.get_untracked(),
    ) else {
        return None;
    };
    // Fields of an older revision are not sent (Rust would refuse them too).
    if let Some(why) = stale_reason(session, insp.revision) {
        let m = format!("Not changed — {field_name}: the Inspector is updating ({why}).");
        u.field_error.set(Some(m.clone()));
        flash(&m);
        return None;
    }
    Some((session, insp))
}

/// Adopt Rust's answer to a field change. Returns whether it applied.
async fn field_outcome(
    session: u64,
    field_name: &str,
    o: Result<p::FieldEditOutcome, String>,
) -> bool {
    let u = ui();
    match o {
        Ok(p::FieldEditOutcome::Applied {
            state, view, item, ..
        }) => {
            u.field_error.set(None);
            editor::adopt_change(&state, view);
            let sel = Selected {
                session,
                revision: state.revision,
                id: item,
            };
            u.selected.set(Some(sel.clone()));
            inspect(sel).await;
            flash(&format!(
                "{field_name} changed (revision {}).",
                state.revision
            ));
            true
        }
        Ok(p::FieldEditOutcome::Unchanged) => {
            flash(&format!("{field_name}: no change."));
            false
        }
        Ok(p::FieldEditOutcome::Refused {
            reason,
            message,
            component_index,
        }) => {
            let at = component_index
                .map(|i| format!(" (component {})", i + 1))
                .unwrap_or_default();
            let m = format!(
                "Not changed — {field_name}{at}: {}",
                message.unwrap_or_else(|| reason.clone())
            );
            u.field_error.set(Some(format!("{m} [{reason}]")));
            flash(&m);
            false
        }
        Err(e) => {
            u.field_error.set(Some(format!("Field edit failed: {e}")));
            flash(&format!("Field edit failed: {e}"));
            false
        }
    }
}

/// Load the current revision into X_ITE. Without `force`, a revision already
/// on screen is not reloaded (each reload builds a whole new X_ITE scene).
pub async fn preview(force: bool) {
    // NATIVE-RENDER-1: while the native viewport is on, it shows the text.
    if crate::native::on() {
        return crate::native::preview(force).await;
    }
    let Some(session) = CORE.with_borrow(|c| c.session) else {
        return;
    };
    let (rev, done) = CORE.with_borrow(|c| (c.revision, c.previewed));
    if !force && done == Some(rev) {
        return;
    }
    let u = ui();
    // A newer preview starts: the generation on screen is retired NOW, so a
    // pick in flight can never land on it.
    crate::pick::retire("preview-scene-replaced");
    // Only the newest load may report: a superseded X_ITE load ends with
    // "Replacing world aborted" and must not overwrite a newer status.
    let seq = CORE.with_borrow_mut(|c| {
        c.preview_seq += 1;
        c.preview_seq
    });
    let newest = move || CORE.with_borrow(|c| c.preview_seq == seq);
    u.preview_status.set("updating…".into());
    match call::<p::PreviewSource>("doc_preview_source", Session { session }).await {
        Ok(src) => {
            CORE.with_borrow_mut(|c| c.previewed = Some(src.revision));
            let gen = crate::pick::Gen {
                session,
                revision: src.revision,
                hash: src.hash,
                seq,
            };
            // VISUAL-3A1: keep the user's camera only onto a PROVEN same
            // viewpoint (crate::camera); otherwise X_ITE's own binding stands.
            let (camera, why) = crate::camera::want(session, src.revision).await;
            // B2-CAMERA: a document's first shown scene is framed once when
            // it has no authored Viewpoint bound.
            let auto_fit = crate::camera::auto_fit_wanted(session);
            let t0 = crate::editor::now();
            let warnings = src.texture_warnings.clone();
            let routes = src.route_warnings.clone();
            let status = ipc::preview_load(
                &src.text,
                Some(&gen),
                camera.clone(),
                auto_fit,
                &src.resource_base,
            )
            .await;
            let load_ms = crate::editor::now() - t0;
            crate::camera::LOAD_MS.with_borrow_mut(|v| v.push(load_ms));
            u.preview_loads.update(|n| *n += 1);
            if newest() {
                // Only the newest successful load is the picking generation.
                if status.starts_with("loaded") {
                    crate::pick::loaded(gen);
                    // Only a scene that is now on screen changes the ROUTE
                    // notice; a refused load keeps the last valid scene and
                    // so keeps that scene's notice.
                    u.route_warnings.set(routes);
                }
                u.texture_warnings.set(warnings);
                let mut note = crate::camera::note(why);
                if status.starts_with("loaded") {
                    note = crate::camera::after_load(session, seq, auto_fit, camera, note);
                }
                u.preview_status
                    .set(format!("rev {} · {status}{note}", src.revision));
            }
        }
        Err(e) if newest() => u.preview_status.set(format!("error: {e}")),
        Err(_) => {}
    }
}

pub fn open() {
    spawn_local(async {
        if confirm_discard().await {
            match call::<p::OpenOutcome>("open_document", NoArgs {}).await {
                Ok(o) => {
                    // B2-CAMERA: an opened file is framed once (no authored
                    // Viewpoint bound).
                    crate::camera::arm_auto_fit();
                    apply_open(o, "Opened").await
                }
                Err(e) => flash(&format!("Open failed: {e}")),
            }
        }
    });
}

async fn confirm_discard() -> bool {
    // A dirty buffer is never discarded silently. Without a modal dialog
    // (WebView alerts block the IPC), Open and Close simply refuse.
    if ui().dirty.get_untracked() {
        flash("Save or undo your changes before opening or closing a file.");
        return false;
    }
    true
}

pub async fn apply_open(o: p::OpenOutcome, verb: &str) {
    match o {
        p::OpenOutcome::Opened { doc } => {
            if let Some(old) = CORE
                .with_borrow(|c| c.session)
                .filter(|&s| s != doc.session)
            {
                let _ = call::<()>("close_document", Session { session: old }).await;
            }
            crate::camera::opened(Some(doc.session));
            editor::adopt(&doc);
            flash(&format!(
                "{verb} {} ({}, {}{}{})",
                doc.name,
                doc.format,
                doc.eol,
                if doc.eol_mixed {
                    ", mixed line endings"
                } else {
                    ""
                },
                if doc.bom { ", BOM" } else { "" }
            ));
        }
        p::OpenOutcome::Canceled => {
            crate::camera::opened(None);
            flash("Open canceled — nothing changed.")
        }
        p::OpenOutcome::Failed { message } => {
            crate::camera::opened(None);
            flash(&format!("Could not open: {message}"))
        }
    }
}

/// File → New World. Rust decides whether the open document holds unsaved
/// changes and, if so, asks with a native confirmation; Cancel leaves it
/// exactly as it is.
pub fn new_world() {
    spawn_local(async {
        editor::idle().await;
        #[derive(serde::Serialize)]
        struct A {
            replace: Option<u64>,
        }
        let replace = CORE.with_borrow(|c| c.session);
        match call::<p::OpenOutcome>("new_document", A { replace }).await {
            Ok(p::OpenOutcome::Canceled) => {
                flash("New World canceled — your document is unchanged.")
            }
            Ok(o) => {
                apply_open(o, "New world:").await;
                ui().create_error.set(None);
            }
            Err(e) => flash(&format!("New World failed: {e}")),
        }
    });
}

/// Create one primitive object through Rust (`doc_create`). The UI sends
/// only the primitive choice and the revision it shows; Rust generates,
/// inserts and proves the source. The new Transform is selected by the id
/// Rust returned for the NEW revision, then shown in every view at once.
pub async fn create(prim: p::Primitive) {
    editor::idle().await;
    let u = ui();
    let (Some(session), revision, busy) = CORE.with_borrow(|c| (c.session, c.revision, c.busy))
    else {
        return;
    };
    if busy {
        flash("Not created — an edit is being applied; try again.");
        return;
    }
    #[derive(serde::Serialize)]
    struct A {
        request: p::CreateRequest,
    }
    let request = p::CreateRequest {
        session,
        base_revision: revision,
        primitive: prim,
        at: placement(),
    };
    match call::<p::CreateOutcome>("doc_create", A { request }).await {
        Ok(p::CreateOutcome::Applied {
            state,
            view,
            item,
            def_name,
        }) => {
            u.create_error.set(None);
            u.field_error.set(None);
            u.inspection.set(None);
            u.selected.set(Some(Selected {
                session,
                revision: state.revision,
                id: item,
            }));
            editor::adopt_change(&state, view);
            flash(&format!(
                "Created {} {def_name} (revision {}).",
                prim.label(),
                state.revision
            ));
            // A Create is one discrete command, not typing: show it in the
            // Scene Tree, Inspector and viewport now, not after the debounce.
            analyze().await;
            // Show the generated source, selected, in the editor.
            let span = u.analysis.with_untracked(|a| {
                a.as_ref().and_then(|a| {
                    let sel = u.selected.get_untracked()?;
                    (a.revision == sel.revision)
                        .then(|| a.items.iter().find(|i| sel.is(&i.id)))
                        .flatten()
                        .map(|i| (a.session, a.revision, i.view_from, i.view_to))
                })
            });
            if let Some((s, r, from, to)) = span {
                select_from("Create", s, r, from, to);
            }
            preview(false).await;
        }
        Ok(p::CreateOutcome::Refused { reason, message }) => {
            let m = format!("Not created — {}: {message}", prim.label());
            u.create_error.set(Some(format!("{m} [{reason}]")));
            flash(&m);
        }
        Err(e) => {
            u.create_error.set(Some(format!("Create failed: {e}")));
            flash(&format!("Create failed: {e}"));
        }
    }
}

/// B2: Delete / Duplicate the selected node through Rust
/// (`doc_structure_edit`). The UI sends only the selection's item id and the
/// revision it belongs to; Rust plans, proves and applies the edit as ONE
/// undo step. After Delete nothing is selected; after Duplicate the copy is.
pub async fn structure_edit(op: p::StructureOp) {
    editor::idle().await;
    let u = ui();
    let verb = match op {
        p::StructureOp::Delete => "Delete",
        p::StructureOp::Duplicate => "Duplicate",
    };
    let Some(sel) = u.selected.get_untracked() else {
        flash(&format!("Not changed — {verb}: no node is selected."));
        return;
    };
    if let Some(why) = stale_reason(sel.session, sel.revision) {
        flash(&format!("Not changed — {verb}: the selection is updating ({why})."));
        return;
    }
    // A PROTO body node: the same consent path as an Inspector edit, read
    // from the Inspector of exactly this selection. Rust refuses without it.
    let shared = u.inspection.with_untracked(|i| {
        i.as_ref()
            .filter(|i| i.id == sel.id && i.revision == sel.revision)
            .and_then(|i| i.node.as_ref()?.shared.clone())
    });
    let confirm = match shared {
        Some(sh) => {
            if !confirm_shared(sel.session, sh).await {
                flash(&format!("Not changed — {verb}: the shared PROTO definition was not confirmed."));
                return;
            }
            true
        }
        None => false,
    };
    #[derive(serde::Serialize)]
    struct A {
        request: p::StructureEditRequest,
    }
    let request = p::StructureEditRequest {
        session: sel.session,
        base_revision: sel.revision,
        item: sel.id.clone(),
        op,
        confirm_shared: confirm,
        extent: match op {
            p::StructureOp::Duplicate => crate::highlight::selection_extent(),
            p::StructureOp::Delete => None,
        },
    };
    match call::<p::StructureEditOutcome>("doc_structure_edit", A { request }).await {
        Ok(p::StructureEditOutcome::Applied {
            state,
            view,
            item,
            renamed,
            placement,
        }) => {
            crate::gizmo::cancel("the selected node changed");
            u.field_error.set(None);
            u.inspection.set(None);
            u.selected.set(item.map(|id| Selected {
                session: sel.session,
                revision: state.revision,
                id,
            }));
            editor::adopt_change(&state, view);
            let names = renamed
                .iter()
                .map(|(a, b)| format!("{a} → {b}"))
                .collect::<Vec<_>>()
                .join(", ");
            let done = match op {
                p::StructureOp::Delete => "Deleted the node",
                p::StructureOp::Duplicate => "Duplicated the node",
            };
            let placed = placement.map(|p| format!("; {p}")).unwrap_or_default();
            if names.is_empty() {
                flash(&format!("{done} (revision {}){placed}.", state.revision));
            } else {
                flash(&format!(
                    "{done} (revision {}){placed}; DEF names in the copy: {names}.",
                    state.revision
                ));
            }
            // One discrete command: show it everywhere now.
            analyze().await;
            let span = u.analysis.with_untracked(|a| {
                a.as_ref().and_then(|a| {
                    let sel = u.selected.get_untracked()?;
                    (a.revision == sel.revision)
                        .then(|| a.items.iter().find(|i| sel.is(&i.id)))
                        .flatten()
                        .map(|i| (a.session, a.revision, i.view_from, i.view_to))
                })
            });
            if let Some((s, r, from, to)) = span {
                select_from(verb, s, r, from, to);
            }
            preview(false).await;
        }
        Ok(p::StructureEditOutcome::Refused { reason, message }) => {
            let m = format!("Not changed — {verb}: {message}");
            u.field_error.set(Some(format!("{m} [{reason}]")));
            flash(&m);
        }
        Err(e) => flash(&format!("{verb} failed: {e}")),
    }
}

/// File → Close. Like Open, a dirty document is never discarded silently.
pub fn close() {
    spawn_local(async {
        editor::idle().await;
        if !confirm_discard().await {
            return;
        }
        let Some(session) = CORE.with_borrow(|c| c.session) else {
            return;
        };
        let _ = call::<()>("close_document", Session { session }).await;
        editor::clear();
        let u = ui();
        u.doc.set(None);
        u.analysis.set(None);
        u.selected.set(None);
        u.inspection.set(None);
        u.field_error.set(None);
        u.create_error.set(None);
        u.conflict.set(None);
        u.revision.set(0);
        u.dirty.set(false);
        u.can_undo.set(false);
        u.can_redo.set(false);
        u.pick_message.set(None);
        crate::gizmo::reset("the document was closed");
        crate::pick::retire("preview-scene-replaced");
        // No document: the viewport shows an empty world, not the old one.
        u.texture_warnings.set(Vec::new());
        u.route_warnings.set(Vec::new());
        u.viewpoints.set(Default::default());
        let status = ipc::preview_load("#VRML V2.0 utf8\n", None, None, false, "").await;
        u.preview_loads.update(|n| *n += 1);
        u.preview_status.set(format!("no document · {status}"));
        flash("Closed.");
    });
}

pub fn save(as_new: bool) {
    spawn_local(async move {
        let _ = save_now(as_new).await;
    });
}

pub async fn save_now(as_new: bool) -> Option<p::SaveOutcome> {
    editor::idle().await;
    let session = CORE.with_borrow(|c| c.session)?;
    // A new world has no file yet: its first save is a Save As.
    let untitled = ui()
        .doc
        .with_untracked(|d| d.as_ref().is_some_and(|d| d.untitled));
    let cmd = if as_new || untitled {
        "doc_save_as"
    } else {
        "doc_save"
    };
    let r = call::<p::SaveOutcome>(cmd, Session { session }).await;
    let u = ui();
    match &r {
        Ok(p::SaveOutcome::Saved {
            bytes_written,
            backup,
            preserved,
            doc,
        }) => {
            u.doc.set(Some(doc.clone()));
            state_changed(doc.revision, doc.dirty, doc.can_undo, doc.can_redo);
            let m = if *preserved {
                "Already saved ✓ — the gzip artifact already holds this text; nothing written."
                    .to_string()
            } else {
                format!(
                    "Saved {} ({bytes_written} B, {}). Backup: {}",
                    doc.name,
                    doc.format,
                    backup.clone().unwrap_or_else(|| "none (new file)".into())
                )
            };
            u.last_save.set(Some(m.clone()));
            flash(&m);
            // TEXTURE-LOCAL-1: a Save As can give the document a (new)
            // folder. Textures the preview could not load may load now, so
            // the preview re-resolves them; later loads always use the
            // session's current folder.
            if cmd == "doc_save_as" && u.texture_warnings.with_untracked(|w| !w.is_empty()) {
                preview(true).await;
            }
        }
        Ok(p::SaveOutcome::Conflict { reason }) => {
            u.conflict.set(Some(reason.clone()));
            flash(&format!("Not saved: the file changed on disk ({reason})."));
        }
        Ok(p::SaveOutcome::Canceled) => flash("Save canceled — nothing written."),
        Ok(p::SaveOutcome::Failed { message }) => flash(&format!("Not saved: {message}")),
        Err(e) => flash(&format!("Save failed: {e}")),
    }
    r.ok()
}

pub fn reload() {
    spawn_local(async {
        let Some(session) = CORE.with_borrow(|c| c.session) else {
            return;
        };
        match call::<p::OpenOutcome>("doc_reload", Session { session }).await {
            Ok(o) => apply_open(o, "Reloaded").await,
            Err(e) => flash(&format!("Reload failed: {e}")),
        }
    });
}

/// Poll for external changes (the JS editor polls too).
pub fn start_external_watch() {
    spawn_local(async {
        loop {
            ipc::sleep(3000).await;
            let Some(session) = CORE.with_borrow(|c| c.session) else {
                continue;
            };
            if ui().conflict.get_untracked().is_some() {
                continue;
            }
            if let Ok(st) =
                call::<p::ExternalStatus>("doc_check_external", Session { session }).await
            {
                if st.changed {
                    ui().conflict.set(Some(st.reason));
                }
            }
        }
    });
}
