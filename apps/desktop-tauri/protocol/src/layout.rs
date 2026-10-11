// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-DOCK-1: the dockable workspace layout. Pure data and pure functions.
//!
//! A layout is a tree of splits and tab groups over the five stable panel IDs
//! (APP-SHELL-0 §4). Every operation here returns a NEW layout that has been
//! validated as a whole, or a refusal; nothing is applied half-way. The UI
//! renders the tree, the backend persists it, and both validate it with this
//! one module. A layout is an application preference: it never names, edits
//! or reaches a document.
//!
//! Geometry (`solve`) turns a layout and a container size into rectangles in
//! CSS pixels. Split sizes are stored as ratios; the minimum panel sizes are
//! enforced when the rectangles are computed, so a stored ratio never has to
//! be rewritten because the window became smaller.
//!
//! See `docs/architecture/UI_DOCK_1.md`.

use serde::{Deserialize, Serialize};

/// Version of the persisted layout object (`settings.json` → `layout`).
pub const LAYOUT_VERSION: u32 = 1;
/// The deepest tree a layout may have (the root is depth 1).
pub const MAX_DEPTH: usize = 6;
/// The most split and tab-group nodes a layout may have.
pub const MAX_NODES: usize = 24;
/// The smallest stored split ratio. Anything smaller is not a usable size.
pub const MIN_RATIO: f64 = 0.02;
/// How far the stored ratios of one split may sum away from 1.0.
pub const RATIO_SUM_TOLERANCE: f64 = 1e-3;
/// Height of a tab strip (the panel header). `style.css` uses the same value.
pub const TAB_STRIP_PX: f64 = 30.0;
/// Thickness of a split divider. `style.css` uses the same value.
pub const DIVIDER_PX: f64 = 6.0;
/// Pointer travel before a press on a tab becomes a docking drag.
pub const DRAG_THRESHOLD_PX: f64 = 6.0;
/// The outer band of a group (fraction of its size) that docks to an edge.
pub const EDGE_BAND: f64 = 0.28;
/// Keyboard divider steps.
pub const KEY_STEP_PX: f64 = 16.0;
pub const KEY_STEP_LARGE_PX: f64 = 64.0;

/// The five dockable panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PanelId {
    SceneTree,
    Viewport,
    Source,
    Inspector,
    Diagnostics,
}

impl PanelId {
    pub const ALL: [PanelId; 5] = [
        PanelId::SceneTree,
        PanelId::Viewport,
        PanelId::Source,
        PanelId::Inspector,
        PanelId::Diagnostics,
    ];

    /// The stable, persisted ID.
    pub fn key(self) -> &'static str {
        match self {
            PanelId::SceneTree => "panel.sceneTree",
            PanelId::Viewport => "panel.viewport",
            PanelId::Source => "panel.source",
            PanelId::Inspector => "panel.inspector",
            PanelId::Diagnostics => "panel.diagnostics",
        }
    }

    pub fn from_key(key: &str) -> Option<PanelId> {
        PanelId::ALL.into_iter().find(|p| p.key() == key)
    }

    /// A short DOM-safe name (`sceneTree`).
    pub fn slug(self) -> &'static str {
        &self.key()["panel.".len()..]
    }

    pub fn title(self) -> &'static str {
        match self {
            PanelId::SceneTree => "Scene Tree",
            PanelId::Viewport => "3D Viewport",
            PanelId::Source => "Source",
            PanelId::Inspector => "Inspector",
            PanelId::Diagnostics => "Diagnostics",
        }
    }

    /// Minimum (width, height) of the panel's content, in CSS px.
    pub fn min_size(self) -> (f64, f64) {
        match self {
            PanelId::SceneTree => (200.0, 160.0),
            PanelId::Viewport => (320.0, 240.0),
            PanelId::Source => (320.0, 200.0),
            PanelId::Inspector => (260.0, 160.0),
            PanelId::Diagnostics => (260.0, 100.0),
        }
    }

    /// The Viewport and the Source never close (APP-SHELL-0 §4.1).
    pub fn closable(self) -> bool {
        !matches!(self, PanelId::Viewport | PanelId::Source)
    }
}

/// `Row`: children side by side (vertical dividers). `Column`: stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Row,
    Column,
}

impl SplitAxis {
    pub fn key(self) -> &'static str {
        match self {
            SplitAxis::Row => "row",
            SplitAxis::Column => "column",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Split {
        axis: SplitAxis,
        children: Vec<Node>,
        /// Fractions of the split's free length; they sum to 1.0.
        sizes: Vec<f64>,
    },
    Tabs {
        panels: Vec<PanelId>,
        active: PanelId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetId {
    Modeling,
    Source,
}

impl PresetId {
    pub const ALL: [PresetId; 2] = [PresetId::Modeling, PresetId::Source];
    /// `preset.source`, the 1.4.5 layout.
    pub const DEFAULT: PresetId = PresetId::Source;

    pub fn key(self) -> &'static str {
        match self {
            PresetId::Modeling => "preset.modeling",
            PresetId::Source => "preset.source",
        }
    }

    pub fn from_key(key: &str) -> Option<PresetId> {
        PresetId::ALL.into_iter().find(|p| p.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            PresetId::Modeling => "Modeling",
            PresetId::Source => "Source",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub preset: PresetId,
    pub root: Node,
}

/// The five drop zones of a tab group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl Zone {
    pub const ALL: [Zone; 5] = [
        Zone::Left,
        Zone::Right,
        Zone::Top,
        Zone::Bottom,
        Zone::Center,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Right => "right",
            Zone::Top => "top",
            Zone::Bottom => "bottom",
            Zone::Center => "center",
        }
    }
}

/// Where a dragged panel goes: a zone of the tab group that holds `anchor`.
/// A tab group is named by one of its panels, because a panel appears once.
/// `index` places a Center drop between existing tabs (a tab-strip drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropTarget {
    pub anchor: PanelId,
    pub zone: Zone,
    pub index: Option<usize>,
}

/// Why a layout or an operation was refused. The current layout stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    DuplicatePanel(PanelId),
    MissingRequired(PanelId),
    EmptyTabGroup,
    ActiveNotInGroup,
    SplitTooSmall,
    SizeCountMismatch,
    BadSize,
    BadSizeSum,
    TooDeep,
    TooManyNodes,
    UnknownPreset(String),
    UnknownAxis(String),
    UnsupportedVersion(u32),
    NotVisible(PanelId),
    AlreadyVisible(PanelId),
    NotClosable(PanelId),
    SameGroup,
    NoChange,
    BadPath,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use LayoutError::*;
        match self {
            DuplicatePanel(p) => write!(f, "{} appears more than once", p.title()),
            MissingRequired(p) => write!(f, "{} is missing", p.title()),
            EmptyTabGroup => write!(f, "a tab group has no panel"),
            ActiveNotInGroup => write!(f, "a tab group's active panel is not in the group"),
            SplitTooSmall => write!(f, "a split has fewer than two parts"),
            SizeCountMismatch => write!(f, "a split's sizes do not match its parts"),
            BadSize => write!(f, "a split size is negative, too small or not a number"),
            BadSizeSum => write!(f, "a split's sizes do not add up to 1"),
            TooDeep => write!(f, "the layout is nested deeper than {MAX_DEPTH} levels"),
            TooManyNodes => write!(f, "the layout has more than {MAX_NODES} parts"),
            UnknownPreset(p) => write!(f, "unknown preset {:?}", trunc(p)),
            UnknownAxis(a) => write!(f, "unknown split axis {:?}", trunc(a)),
            UnsupportedVersion(v) => write!(f, "layout version {v} is not supported"),
            NotVisible(p) => write!(f, "{} is not shown", p.title()),
            AlreadyVisible(p) => write!(f, "{} is already shown", p.title()),
            NotClosable(p) => write!(f, "{} cannot be closed", p.title()),
            SameGroup => write!(f, "the panel is already there"),
            NoChange => write!(f, "the drop changes nothing"),
            BadPath => write!(f, "no such split or divider"),
        }
    }
}

fn trunc(s: &str) -> String {
    s.chars().take(40).collect()
}

// ---------------------------------------------------------------- wire form

/// The persisted and IPC shape. Untrusted: [`Layout::from_wire`] validates it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutWire {
    pub version: u32,
    pub preset: String,
    pub root: NodeWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NodeWire {
    Split {
        axis: String,
        children: Vec<NodeWire>,
        sizes: Vec<f64>,
    },
    Tabs {
        panels: Vec<String>,
        active: String,
    },
}

/// `layout_get`: the layout to apply, and a notice when the saved one could
/// not be used (or was repaired).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutState {
    pub layout: LayoutWire,
    pub notice: Option<String>,
}

/// `layout_set`: what happened to the persisted copy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LayoutSetOutcome {
    Saved,
    /// Applied in this session, but the file could not be written.
    NotSaved {
        message: String,
    },
    /// The layout was invalid. Nothing was written or adopted.
    Rejected {
        message: String,
    },
}

impl Layout {
    pub fn to_wire(&self) -> LayoutWire {
        fn node(n: &Node) -> NodeWire {
            match n {
                Node::Split {
                    axis,
                    children,
                    sizes,
                } => NodeWire::Split {
                    axis: axis.key().into(),
                    children: children.iter().map(node).collect(),
                    sizes: sizes.clone(),
                },
                Node::Tabs { panels, active } => NodeWire::Tabs {
                    panels: panels.iter().map(|p| p.key().into()).collect(),
                    active: active.key().into(),
                },
            }
        }
        LayoutWire {
            version: LAYOUT_VERSION,
            preset: self.preset.key().into(),
            root: node(&self.root),
        }
    }

    /// Validate an untrusted layout. The tree's shape is checked as written
    /// (no repair). Unknown panel IDs are then dropped (APP-SHELL-0 §4.1) and
    /// named in the returned list; the result is validated as a whole.
    pub fn from_wire(w: &LayoutWire) -> Result<(Layout, Vec<String>), LayoutError> {
        if w.version != LAYOUT_VERSION {
            return Err(LayoutError::UnsupportedVersion(w.version));
        }
        let preset = PresetId::from_key(&w.preset)
            .ok_or_else(|| LayoutError::UnknownPreset(w.preset.clone()))?;
        let mut count = 0;
        let mut unknown = Vec::new();
        let root = wire_node(&w.root, 1, &mut count, &mut unknown)?;
        let root = normalize(root.ok_or(LayoutError::MissingRequired(PanelId::Source))?)
            .ok_or(LayoutError::MissingRequired(PanelId::Source))?;
        let l = Layout { preset, root };
        l.validate()?;
        Ok((l, unknown))
    }

    /// Every rule of a valid layout.
    pub fn validate(&self) -> Result<(), LayoutError> {
        let mut seen = Vec::new();
        let mut count = 0;
        check(&self.root, 1, &mut count, &mut seen)?;
        for p in [PanelId::Viewport, PanelId::Source] {
            if !seen.contains(&p) {
                return Err(LayoutError::MissingRequired(p));
            }
        }
        Ok(())
    }
}

fn wire_node(
    n: &NodeWire,
    depth: usize,
    count: &mut usize,
    unknown: &mut Vec<String>,
) -> Result<Option<Node>, LayoutError> {
    if depth > MAX_DEPTH {
        return Err(LayoutError::TooDeep);
    }
    *count += 1;
    if *count > MAX_NODES {
        return Err(LayoutError::TooManyNodes);
    }
    match n {
        NodeWire::Split {
            axis,
            children,
            sizes,
        } => {
            let axis = match axis.as_str() {
                "row" => SplitAxis::Row,
                "column" => SplitAxis::Column,
                other => return Err(LayoutError::UnknownAxis(other.into())),
            };
            check_sizes(children.len(), sizes)?;
            let mut kept = Vec::new();
            let mut kept_sizes = Vec::new();
            for (c, s) in children.iter().zip(sizes) {
                if let Some(c) = wire_node(c, depth + 1, count, unknown)? {
                    kept.push(c);
                    kept_sizes.push(*s);
                }
            }
            Ok(match kept.len() {
                0 => None,
                1 => kept.pop(),
                _ => Some(Node::Split {
                    axis,
                    children: kept,
                    sizes: renormalize(&kept_sizes),
                }),
            })
        }
        NodeWire::Tabs { panels, active } => {
            if panels.is_empty() {
                return Err(LayoutError::EmptyTabGroup);
            }
            if !panels.contains(active) {
                return Err(LayoutError::ActiveNotInGroup);
            }
            let mut known = Vec::new();
            for k in panels {
                match PanelId::from_key(k) {
                    Some(p) if known.contains(&p) => return Err(LayoutError::DuplicatePanel(p)),
                    Some(p) => known.push(p),
                    None => unknown.push(trunc(k)),
                }
            }
            let Some(first) = known.first().copied() else {
                return Ok(None);
            };
            let active = PanelId::from_key(active).unwrap_or(first);
            Ok(Some(Node::Tabs {
                panels: known,
                active,
            }))
        }
    }
}

fn check_sizes(children: usize, sizes: &[f64]) -> Result<(), LayoutError> {
    if children < 2 {
        return Err(LayoutError::SplitTooSmall);
    }
    if sizes.len() != children {
        return Err(LayoutError::SizeCountMismatch);
    }
    if sizes
        .iter()
        .any(|s| !s.is_finite() || *s < MIN_RATIO || *s > 1.0)
    {
        return Err(LayoutError::BadSize);
    }
    if (sizes.iter().sum::<f64>() - 1.0).abs() > RATIO_SUM_TOLERANCE {
        return Err(LayoutError::BadSizeSum);
    }
    Ok(())
}

fn check(
    n: &Node,
    depth: usize,
    count: &mut usize,
    seen: &mut Vec<PanelId>,
) -> Result<(), LayoutError> {
    if depth > MAX_DEPTH {
        return Err(LayoutError::TooDeep);
    }
    *count += 1;
    if *count > MAX_NODES {
        return Err(LayoutError::TooManyNodes);
    }
    match n {
        Node::Split {
            children, sizes, ..
        } => {
            check_sizes(children.len(), sizes)?;
            for c in children {
                check(c, depth + 1, count, seen)?;
            }
        }
        Node::Tabs { panels, active } => {
            if panels.is_empty() {
                return Err(LayoutError::EmptyTabGroup);
            }
            if !panels.contains(active) {
                return Err(LayoutError::ActiveNotInGroup);
            }
            for p in panels {
                if seen.contains(p) {
                    return Err(LayoutError::DuplicatePanel(*p));
                }
                seen.push(*p);
            }
        }
    }
    Ok(())
}

fn renormalize(sizes: &[f64]) -> Vec<f64> {
    let sum: f64 = sizes.iter().sum();
    if sum.is_nan() || sum <= 0.0 {
        return vec![1.0 / sizes.len() as f64; sizes.len()];
    }
    sizes.iter().map(|s| s / sum).collect()
}

/// Remove empty groups, collapse one-child splits, merge a child split into
/// a parent of the same axis, and keep ratios summing to 1. `None` = empty.
fn normalize(n: Node) -> Option<Node> {
    match n {
        Node::Tabs { panels, active } => {
            let active = if panels.contains(&active) {
                active
            } else {
                *panels.first()?
            };
            Some(Node::Tabs { panels, active })
        }
        Node::Split {
            axis,
            children,
            sizes,
        } => {
            let mut kids = Vec::new();
            let mut ks = Vec::new();
            for (c, s) in children.into_iter().zip(renormalize(&sizes)) {
                match normalize(c) {
                    Some(Node::Split {
                        axis: a,
                        children: gc,
                        sizes: gs,
                    }) if a == axis => {
                        for (g, gsz) in gc.into_iter().zip(gs) {
                            kids.push(g);
                            ks.push(s * gsz);
                        }
                    }
                    Some(c) => {
                        kids.push(c);
                        ks.push(s);
                    }
                    None => {}
                }
            }
            match kids.len() {
                0 => None,
                1 => kids.pop(),
                _ => {
                    // Keep every part usable after a removal.
                    let mut ks = renormalize(&ks);
                    for s in ks.iter_mut() {
                        *s = s.max(MIN_RATIO * 1.5);
                    }
                    Some(Node::Split {
                        axis,
                        children: kids,
                        sizes: renormalize(&ks),
                    })
                }
            }
        }
    }
}

// ---------------------------------------------------------------- presets

fn tabs(panels: &[PanelId]) -> Node {
    Node::Tabs {
        panels: panels.to_vec(),
        active: panels[0],
    }
}

fn split(axis: SplitAxis, parts: Vec<(Node, f64)>) -> Node {
    let (children, sizes) = parts.into_iter().unzip();
    Node::Split {
        axis,
        children,
        sizes,
    }
}

impl Layout {
    /// The two built-in arrangements (APP-SHELL-0 §4.5).
    pub fn preset(p: PresetId) -> Layout {
        use PanelId::*;
        use SplitAxis::*;
        let root = match p {
            // Scene Tree · Source over Diagnostics · Viewport over Inspector.
            PresetId::Source => split(
                Row,
                vec![
                    (tabs(&[SceneTree]), 0.18),
                    (
                        split(
                            Column,
                            vec![(tabs(&[Source]), 0.76), (tabs(&[Diagnostics]), 0.24)],
                        ),
                        0.48,
                    ),
                    (
                        split(
                            Column,
                            vec![(tabs(&[Viewport]), 0.56), (tabs(&[Inspector]), 0.44)],
                        ),
                        0.34,
                    ),
                ],
            ),
            // Scene Tree · large Viewport · Inspector over Diagnostics, with
            // Source as a tab beside Diagnostics.
            PresetId::Modeling => split(
                Row,
                vec![
                    (tabs(&[SceneTree]), 0.17),
                    (tabs(&[Viewport]), 0.55),
                    (
                        split(
                            Column,
                            vec![
                                (tabs(&[Inspector]), 0.55),
                                (tabs(&[Diagnostics, Source]), 0.45),
                            ],
                        ),
                        0.28,
                    ),
                ],
            ),
        };
        Layout { preset: p, root }
    }

    pub fn default_layout() -> Layout {
        Layout::preset(PresetId::DEFAULT)
    }

    /// Where a closed panel returns in the active preset: the first
    /// candidate whose anchor is shown, with the share of the split it gets
    /// (the preset's own ratio). Viewport and Source are always shown.
    fn home(&self, p: PanelId) -> Vec<Home> {
        use PanelId::*;
        let at = |anchor, zone, share| {
            Home::At(
                DropTarget {
                    anchor,
                    zone,
                    index: None,
                },
                share,
            )
        };
        match (self.preset, p) {
            (PresetId::Source, SceneTree) => vec![Home::RootLeft(0.18)],
            (PresetId::Source, Diagnostics) => vec![at(Source, Zone::Bottom, 0.24)],
            (PresetId::Source, Inspector) => vec![at(Viewport, Zone::Bottom, 0.44)],
            (PresetId::Modeling, SceneTree) => vec![Home::RootLeft(0.17)],
            (PresetId::Modeling, Inspector) => vec![
                at(Diagnostics, Zone::Top, 0.55),
                at(Source, Zone::Top, 0.55),
            ],
            (PresetId::Modeling, Diagnostics) => vec![Home::At(
                DropTarget {
                    anchor: Source,
                    zone: Zone::Center,
                    index: Some(0),
                },
                0.0,
            )],
            (_, Viewport) | (_, Source) => vec![],
        }
    }
}

/// A closed panel's place in a preset.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Home {
    /// A drop target, and the new panel's share of the split it creates.
    At(DropTarget, f64),
    /// The far left of the workspace, full height, with this share.
    RootLeft(f64),
}

// ---------------------------------------------------------------- queries

impl Layout {
    pub fn visible(&self) -> Vec<PanelId> {
        fn walk(n: &Node, out: &mut Vec<PanelId>) {
            match n {
                Node::Split { children, .. } => children.iter().for_each(|c| walk(c, out)),
                Node::Tabs { panels, .. } => out.extend(panels),
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    pub fn is_visible(&self, p: PanelId) -> bool {
        self.visible().contains(&p)
    }

    /// The panels of the group that holds `p`, and its active panel.
    pub fn group_of(&self, p: PanelId) -> Option<(Vec<PanelId>, PanelId)> {
        fn walk(n: &Node, p: PanelId) -> Option<(Vec<PanelId>, PanelId)> {
            match n {
                Node::Split { children, .. } => children.iter().find_map(|c| walk(c, p)),
                Node::Tabs { panels, active } => {
                    panels.contains(&p).then(|| (panels.clone(), *active))
                }
            }
        }
        walk(&self.root, p)
    }

    /// Shown AND the active tab of its group (its content is on screen).
    pub fn is_on_screen(&self, p: PanelId) -> bool {
        self.group_of(p).is_some_and(|(_, a)| a == p)
    }
}

// ---------------------------------------------------------------- operations

fn map_group(n: &mut Node, anchor: PanelId, f: &mut dyn FnMut(&mut Node)) -> bool {
    match n {
        Node::Split { children, .. } => children.iter_mut().any(|c| map_group(c, anchor, f)),
        Node::Tabs { panels, .. } if panels.contains(&anchor) => {
            f(n);
            true
        }
        Node::Tabs { .. } => false,
    }
}

fn remove_panel(n: Node, p: PanelId) -> Option<Node> {
    let n = match n {
        Node::Tabs { mut panels, active } => {
            let i = panels.iter().position(|x| *x == p);
            if let Some(i) = i {
                panels.remove(i);
                // The neighbor that slides into the removed tab's place.
                let next = panels.get(i.min(panels.len().saturating_sub(1))).copied();
                let active = if active == p { next? } else { active };
                Node::Tabs { panels, active }
            } else {
                Node::Tabs { panels, active }
            }
        }
        Node::Split {
            axis,
            children,
            sizes,
        } => {
            let mut kids = Vec::new();
            let mut ks = Vec::new();
            for (c, s) in children.into_iter().zip(sizes) {
                if let Some(c) = remove_panel(c, p) {
                    kids.push(c);
                    ks.push(s);
                }
            }
            if kids.is_empty() {
                return None;
            }
            Node::Split {
                axis,
                children: kids,
                sizes: ks,
            }
        }
    };
    normalize(n)
}

impl Layout {
    fn with_root(&self, root: Node) -> Result<Layout, LayoutError> {
        let root = normalize(root).ok_or(LayoutError::MissingRequired(PanelId::Source))?;
        let l = Layout {
            preset: self.preset,
            root,
        };
        l.validate()?;
        // An operation never loses or invents a panel by accident.
        Ok(l)
    }

    /// Is this drop allowed? The same check `dock` makes.
    pub fn can_dock(&self, panel: PanelId, t: DropTarget) -> Result<(), LayoutError> {
        self.dock(panel, t).map(|_| ())
    }

    /// Move a shown panel to a drop target. Refused drops change nothing.
    pub fn dock(&self, panel: PanelId, t: DropTarget) -> Result<Layout, LayoutError> {
        if !self.is_visible(panel) {
            return Err(LayoutError::NotVisible(panel));
        }
        if !self.is_visible(t.anchor) {
            return Err(LayoutError::NotVisible(t.anchor));
        }
        if t.anchor == panel {
            return Err(LayoutError::SameGroup);
        }
        let (group, _) = self.group_of(t.anchor).ok_or(LayoutError::BadPath)?;
        let same = group.contains(&panel);
        if same && t.zone == Zone::Center {
            // Only a tab-strip drop reorders inside the group.
            let Some(to) = t.index else {
                return Err(LayoutError::SameGroup);
            };
            return self.move_tab(panel, to);
        }
        let removed = remove_panel(self.root.clone(), panel).ok_or(LayoutError::BadPath)?;
        let root = self.insert(removed, panel, t)?;
        let l = self.with_root(root)?;
        if l == *self {
            return Err(LayoutError::NoChange);
        }
        Ok(l)
    }

    fn insert(&self, root: Node, panel: PanelId, t: DropTarget) -> Result<Node, LayoutError> {
        self.insert_share(root, panel, t, 0.5)
    }

    fn insert_share(
        &self,
        mut root: Node,
        panel: PanelId,
        t: DropTarget,
        share: f64,
    ) -> Result<Node, LayoutError> {
        let share = share.clamp(0.1, 0.9);
        let found = map_group(&mut root, t.anchor, &mut |g| {
            if t.zone == Zone::Center {
                if let Node::Tabs { panels, active } = g {
                    let i = t.index.unwrap_or(panels.len()).min(panels.len());
                    panels.insert(i, panel);
                    *active = panel;
                }
                return;
            }
            let axis = match t.zone {
                Zone::Left | Zone::Right => SplitAxis::Row,
                _ => SplitAxis::Column,
            };
            let new = tabs(&[panel]);
            let old = std::mem::replace(g, tabs(&[panel]));
            let children = if matches!(t.zone, Zone::Left | Zone::Top) {
                vec![new, old]
            } else {
                vec![old, new]
            };
            let sizes = if matches!(t.zone, Zone::Left | Zone::Top) {
                vec![share, 1.0 - share]
            } else {
                vec![1.0 - share, share]
            };
            *g = Node::Split {
                axis,
                children,
                sizes,
            };
        });
        if found {
            Ok(root)
        } else {
            Err(LayoutError::NotVisible(t.anchor))
        }
    }

    /// Reorder a tab inside its own group (`to` counts the other tabs).
    pub fn move_tab(&self, panel: PanelId, to: usize) -> Result<Layout, LayoutError> {
        let (group, _) = self.group_of(panel).ok_or(LayoutError::NotVisible(panel))?;
        let from = group.iter().position(|p| *p == panel).unwrap();
        // `to` is an insertion index among the CURRENT tabs (0..=len).
        let to = to.min(group.len());
        let dest = if to > from { to - 1 } else { to };
        if dest == from {
            return Err(LayoutError::NoChange);
        }
        let mut root = self.root.clone();
        map_group(&mut root, panel, &mut |g| {
            if let Node::Tabs { panels, active } = g {
                panels.remove(from);
                panels.insert(dest, panel);
                *active = panel;
            }
        });
        self.with_root(root)
    }

    /// Make `panel` the active tab of its group.
    pub fn activate(&self, panel: PanelId) -> Result<Layout, LayoutError> {
        if !self.is_visible(panel) {
            return Err(LayoutError::NotVisible(panel));
        }
        let mut root = self.root.clone();
        map_group(&mut root, panel, &mut |g| {
            if let Node::Tabs { active, .. } = g {
                *active = panel;
            }
        });
        self.with_root(root)
    }

    /// Close an optional panel. Its component stays mounted (hidden).
    pub fn hide(&self, panel: PanelId) -> Result<Layout, LayoutError> {
        if !panel.closable() {
            return Err(LayoutError::NotClosable(panel));
        }
        if !self.is_visible(panel) {
            return Err(LayoutError::NotVisible(panel));
        }
        let root = remove_panel(self.root.clone(), panel).ok_or(LayoutError::BadPath)?;
        self.with_root(root)
    }

    /// Show a closed panel at its preset position, or activate a shown one.
    pub fn show(&self, panel: PanelId) -> Result<Layout, LayoutError> {
        if self.is_visible(panel) {
            return self.activate(panel);
        }
        let home = self
            .home(panel)
            .into_iter()
            .find(|h| match h {
                Home::At(t, _) => self.is_visible(t.anchor),
                Home::RootLeft(_) => true,
            })
            .unwrap_or(Home::At(
                DropTarget {
                    anchor: PanelId::Source,
                    zone: Zone::Right,
                    index: None,
                },
                0.5,
            ));
        let root = match home {
            Home::At(t, share) => self.insert_share(self.root.clone(), panel, t, share)?,
            Home::RootLeft(share) => {
                let share = share.clamp(0.1, 0.9);
                Node::Split {
                    axis: SplitAxis::Row,
                    children: vec![tabs(&[panel]), self.root.clone()],
                    sizes: vec![share, 1.0 - share],
                }
            }
        };
        self.with_root(root)
    }

    /// Replace the sizes of the split at `path` (child indexes from the root).
    pub fn set_sizes(&self, path: &[usize], sizes: Vec<f64>) -> Result<Layout, LayoutError> {
        fn at<'a>(n: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
            match path.split_first() {
                None => Some(n),
                Some((i, rest)) => match n {
                    Node::Split { children, .. } => at(children.get_mut(*i)?, rest),
                    Node::Tabs { .. } => None,
                },
            }
        }
        let mut root = self.root.clone();
        match at(&mut root, path) {
            Some(Node::Split {
                children, sizes: s, ..
            }) => {
                check_sizes(children.len(), &sizes)?;
                *s = sizes;
            }
            _ => return Err(LayoutError::BadPath),
        }
        let l = Layout {
            preset: self.preset,
            root,
        };
        l.validate()?;
        Ok(l)
    }
}

impl Layout {
    /// The same tree, with every split ratio within `tol` (float noise from
    /// a restore is not a different layout).
    pub fn approx_eq(&self, other: &Layout, tol: f64) -> bool {
        fn node(a: &Node, b: &Node, tol: f64) -> bool {
            match (a, b) {
                (
                    Node::Split {
                        axis: x,
                        children: c,
                        sizes: s,
                    },
                    Node::Split {
                        axis: y,
                        children: d,
                        sizes: t,
                    },
                ) => {
                    x == y
                        && c.len() == d.len()
                        && s.iter().zip(t).all(|(p, q)| (p - q).abs() <= tol)
                        && c.iter().zip(d).all(|(p, q)| node(p, q, tol))
                }
                (Node::Tabs { .. }, Node::Tabs { .. }) => a == b,
                _ => false,
            }
        }
        self.preset == other.preset && node(&self.root, &other.root, tol)
    }
}

// ---------------------------------------------------------------- geometry

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupGeom {
    pub panels: Vec<PanelId>,
    pub active: PanelId,
    pub rect: Rect,
    /// The tab strip (panel header).
    pub strip: Rect,
    /// Where the active panel's content is placed.
    pub content: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DividerGeom {
    /// The split's path from the root.
    pub path: Vec<usize>,
    /// The divider between child `index` and `index + 1`.
    pub index: usize,
    pub axis: SplitAxis,
    pub rect: Rect,
    /// Every child's solved length along the axis, and its minimum.
    pub lengths: Vec<f64>,
    pub mins: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    /// The solved canvas: the container, or the layout's minimum when the
    /// container is smaller (the workspace then scrolls; nothing collapses).
    pub width: f64,
    pub height: f64,
    pub groups: Vec<GroupGeom>,
    pub dividers: Vec<DividerGeom>,
}

impl Geometry {
    pub fn group_of(&self, p: PanelId) -> Option<&GroupGeom> {
        self.groups.iter().find(|g| g.panels.contains(&p))
    }

    pub fn divider(&self, path: &[usize], index: usize) -> Option<&DividerGeom> {
        self.dividers
            .iter()
            .find(|d| d.path == path && d.index == index)
    }
}

/// Minimum (width, height) of a subtree, including tab strips and dividers.
pub fn min_size(n: &Node) -> (f64, f64) {
    match n {
        Node::Tabs { panels, .. } => {
            let w = panels.iter().map(|p| p.min_size().0).fold(0.0, f64::max);
            let h = panels.iter().map(|p| p.min_size().1).fold(0.0, f64::max);
            (w, h + TAB_STRIP_PX)
        }
        Node::Split { axis, children, .. } => {
            let mins: Vec<_> = children.iter().map(min_size).collect();
            let gaps = DIVIDER_PX * (children.len() - 1) as f64;
            match axis {
                SplitAxis::Row => (
                    mins.iter().map(|m| m.0).sum::<f64>() + gaps,
                    mins.iter().map(|m| m.1).fold(0.0, f64::max),
                ),
                SplitAxis::Column => (
                    mins.iter().map(|m| m.0).fold(0.0, f64::max),
                    mins.iter().map(|m| m.1).sum::<f64>() + gaps,
                ),
            }
        }
    }
}

/// Share `avail` among parts by `ratios`, never giving a part less than its
/// minimum: a part below its minimum is fixed AT it and the others shrink in
/// proportion (the largest gives the most). When even the minimums do not
/// fit, every part is compressed in proportion to its minimum (no negative
/// or zero length).
pub fn distribute(avail: f64, ratios: &[f64], mins: &[f64]) -> Vec<f64> {
    let n = ratios.len();
    let avail = avail.max(0.0);
    let need: f64 = mins.iter().sum();
    if need >= avail {
        if need <= 0.0 {
            return vec![avail / n as f64; n];
        }
        return mins.iter().map(|m| m * avail / need).collect();
    }
    let mut fixed = vec![false; n];
    loop {
        let fixed_len: f64 = (0..n).filter(|i| fixed[*i]).map(|i| mins[i]).sum();
        let free_ratio: f64 = (0..n).filter(|i| !fixed[*i]).map(|i| ratios[i]).sum();
        let rest = avail - fixed_len;
        let out: Vec<f64> = (0..n)
            .map(|i| {
                if fixed[i] {
                    mins[i]
                } else if free_ratio > 0.0 {
                    ratios[i] / free_ratio * rest
                } else {
                    rest / (0..n).filter(|j| !fixed[*j]).count() as f64
                }
            })
            .collect();
        let mut changed = false;
        for i in 0..n {
            if !fixed[i] && out[i] < mins[i] {
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}

/// Rectangles for every group and divider in a `width` × `height` container.
pub fn solve(layout: &Layout, width: f64, height: f64) -> Geometry {
    let (mw, mh) = min_size(&layout.root);
    let w = if width.is_finite() { width.max(mw) } else { mw };
    let h = if height.is_finite() {
        height.max(mh)
    } else {
        mh
    };
    let mut g = Geometry {
        width: w,
        height: h,
        ..Geometry::default()
    };
    place(
        &layout.root,
        Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        },
        &mut Vec::new(),
        &mut g,
    );
    g
}

fn place(n: &Node, r: Rect, path: &mut Vec<usize>, g: &mut Geometry) {
    match n {
        Node::Tabs { panels, active } => {
            let strip_h = TAB_STRIP_PX.min(r.h);
            g.groups.push(GroupGeom {
                panels: panels.clone(),
                active: *active,
                rect: r,
                strip: Rect { h: strip_h, ..r },
                content: Rect {
                    y: r.y + strip_h,
                    h: (r.h - strip_h).max(0.0),
                    ..r
                },
            });
        }
        Node::Split {
            axis,
            children,
            sizes,
        } => {
            let total = match axis {
                SplitAxis::Row => r.w,
                SplitAxis::Column => r.h,
            };
            let avail = total - DIVIDER_PX * (children.len() - 1) as f64;
            let mins: Vec<f64> = children
                .iter()
                .map(|c| match axis {
                    SplitAxis::Row => min_size(c).0,
                    SplitAxis::Column => min_size(c).1,
                })
                .collect();
            let lengths = distribute(avail, sizes, &mins);
            let mut at = match axis {
                SplitAxis::Row => r.x,
                SplitAxis::Column => r.y,
            };
            for (i, c) in children.iter().enumerate() {
                let len = lengths[i];
                let cr = match axis {
                    SplitAxis::Row => Rect { x: at, w: len, ..r },
                    SplitAxis::Column => Rect { y: at, h: len, ..r },
                };
                path.push(i);
                place(c, cr, path, g);
                path.pop();
                at += len;
                if i + 1 < children.len() {
                    let dr = match axis {
                        SplitAxis::Row => Rect {
                            x: at,
                            w: DIVIDER_PX,
                            ..r
                        },
                        SplitAxis::Column => Rect {
                            y: at,
                            h: DIVIDER_PX,
                            ..r
                        },
                    };
                    g.dividers.push(DividerGeom {
                        path: path.clone(),
                        index: i,
                        axis: *axis,
                        rect: dr,
                        lengths: lengths.clone(),
                        mins: mins.clone(),
                    });
                    at += DIVIDER_PX;
                }
            }
        }
    }
}

impl DividerGeom {
    /// The (clamped) lengths of the two neighbors after moving the divider
    /// by `delta` px. `None` when the neighbors cannot both keep their
    /// minimums (the divider then stays where it is).
    pub fn clamp_move(&self, delta: f64) -> Option<(f64, f64)> {
        let (a, b) = (self.lengths[self.index], self.lengths[self.index + 1]);
        let (ma, mb) = (self.mins[self.index], self.mins[self.index + 1]);
        let pair = a + b;
        if !delta.is_finite() || ma + mb > pair + 1e-6 {
            return None;
        }
        let na = (a + delta).clamp(ma, pair - mb);
        Some((na, pair - na))
    }

    /// The split's new ratios after moving this divider by `delta` px.
    pub fn ratios_after(&self, delta: f64) -> Option<Vec<f64>> {
        let (na, nb) = self.clamp_move(delta)?;
        let mut l = self.lengths.clone();
        l[self.index] = na;
        l[self.index + 1] = nb;
        let sum: f64 = l.iter().sum();
        if sum.is_nan() || sum <= 0.0 {
            return None;
        }
        let r: Vec<f64> = l.iter().map(|x| (x / sum).max(MIN_RATIO)).collect();
        Some(renormalize(&r))
    }

    /// How far the divider can move: (most negative, most positive) px.
    pub fn range(&self) -> (f64, f64) {
        let (a, b) = (self.lengths[self.index], self.lengths[self.index + 1]);
        let (ma, mb) = (self.mins[self.index], self.mins[self.index + 1]);
        ((ma - a).min(0.0), (b - mb).max(0.0))
    }

    /// The divider position as a percentage of its two neighbors (ARIA).
    pub fn percent(&self) -> f64 {
        let (a, b) = (self.lengths[self.index], self.lengths[self.index + 1]);
        if a + b > 0.0 {
            (a / (a + b) * 100.0).round()
        } else {
            50.0
        }
    }
}

impl Layout {
    /// Move the divider `index` of the split at `path` by `delta` px, with
    /// the minimum sizes enforced against the solved geometry `geo`.
    pub fn resize(
        &self,
        geo: &Geometry,
        path: &[usize],
        index: usize,
        delta: f64,
    ) -> Result<Layout, LayoutError> {
        let d = geo.divider(path, index).ok_or(LayoutError::BadPath)?;
        let r = d.ratios_after(delta).ok_or(LayoutError::NoChange)?;
        self.set_sizes(path, r)
    }
}

/// The zone of a group's content rectangle under the pointer: the outer
/// [`EDGE_BAND`] docks to the nearest edge, the middle makes a tab.
pub fn zone_at(content: Rect, x: f64, y: f64) -> Option<Zone> {
    if !content.contains(x, y) || content.w <= 0.0 || content.h <= 0.0 {
        return None;
    }
    let fx = (x - content.x) / content.w;
    let fy = (y - content.y) / content.h;
    let edges = [
        (fx, Zone::Left),
        (1.0 - fx, Zone::Right),
        (fy, Zone::Top),
        (1.0 - fy, Zone::Bottom),
    ];
    let (d, z) = edges
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    Some(if d < EDGE_BAND { z } else { Zone::Center })
}

/// The area a drop into `zone` of `content` will occupy (the snap guide).
pub fn zone_rect(content: Rect, zone: Zone) -> Rect {
    let Rect { x, y, w, h } = content;
    match zone {
        Zone::Left => Rect {
            w: w / 2.0,
            ..content
        },
        Zone::Right => Rect {
            x: x + w / 2.0,
            w: w / 2.0,
            ..content
        },
        Zone::Top => Rect {
            h: h / 2.0,
            ..content
        },
        Zone::Bottom => Rect {
            y: y + h / 2.0,
            h: h / 2.0,
            ..content
        },
        Zone::Center => Rect {
            x: x + 4.0,
            y: y + 4.0,
            w: (w - 8.0).max(0.0),
            h: (h - 8.0).max(0.0),
        },
    }
}

/// The tab-strip insertion index for pointer `x`, given each tab's
/// horizontal center (left to right).
pub fn insertion_index(centers: &[f64], x: f64) -> usize {
    centers.iter().take_while(|c| x > **c).count()
}

#[cfg(test)]
mod tests;
