// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use PanelId::*;

fn wire(json_root: NodeWire) -> LayoutWire {
    LayoutWire {
        version: LAYOUT_VERSION,
        preset: "preset.source".into(),
        root: json_root,
    }
}

fn wtabs(p: &[&str], a: &str) -> NodeWire {
    NodeWire::Tabs {
        panels: p.iter().map(|s| s.to_string()).collect(),
        active: a.into(),
    }
}

fn wsplit(axis: &str, c: Vec<NodeWire>, s: Vec<f64>) -> NodeWire {
    NodeWire::Split {
        axis: axis.into(),
        children: c,
        sizes: s,
    }
}

fn base() -> NodeWire {
    wsplit(
        "row",
        vec![
            wtabs(&["panel.source"], "panel.source"),
            wtabs(&["panel.viewport"], "panel.viewport"),
        ],
        vec![0.5, 0.5],
    )
}

fn every_panel_once(l: &Layout) {
    let v = l.visible();
    let mut s = v.clone();
    s.sort();
    s.dedup();
    assert_eq!(s.len(), v.len(), "duplicate in {v:?}");
    assert!(v.contains(&Viewport) && v.contains(&Source));
    l.validate().unwrap();
}

#[test]
fn ids_are_stable_strings() {
    let keys: Vec<_> = PanelId::ALL.iter().map(|p| p.key()).collect();
    assert_eq!(
        keys,
        [
            "panel.sceneTree",
            "panel.viewport",
            "panel.source",
            "panel.inspector",
            "panel.diagnostics"
        ]
    );
    for p in PanelId::ALL {
        assert_eq!(PanelId::from_key(p.key()), Some(p));
    }
    assert_eq!(PanelId::from_key("panel.SceneTree"), None);
    assert_eq!(PresetId::from_key("preset.source"), Some(PresetId::Source));
    assert_eq!(PresetId::DEFAULT, PresetId::Source);
    assert!(!Viewport.closable() && !Source.closable());
    assert!(SceneTree.closable() && Inspector.closable() && Diagnostics.closable());
}

#[test]
fn minimum_sizes_match_app_shell_0() {
    assert_eq!(SceneTree.min_size(), (200.0, 160.0));
    assert_eq!(Viewport.min_size(), (320.0, 240.0));
    assert_eq!(Source.min_size(), (320.0, 200.0));
    assert_eq!(Inspector.min_size(), (260.0, 160.0));
    assert_eq!(Diagnostics.min_size(), (260.0, 100.0));
}

#[test]
fn presets_are_valid_and_complete() {
    for p in PresetId::ALL {
        let l = Layout::preset(p);
        every_panel_once(&l);
        assert_eq!(l.visible().len(), 5, "{p:?}");
        let (back, unknown) = Layout::from_wire(&l.to_wire()).unwrap();
        assert_eq!(back, l);
        assert!(unknown.is_empty());
    }
    // Source: the textarea and the canvas are both on screen.
    let s = Layout::preset(PresetId::Source);
    assert!(s.is_on_screen(Source) && s.is_on_screen(Viewport));
    // Modeling: the Viewport is the largest; Source is a tab.
    let m = Layout::preset(PresetId::Modeling);
    assert!(m.is_on_screen(Viewport));
    assert_eq!(m.group_of(Source).unwrap().0, vec![Diagnostics, Source]);
    let g = solve(&m, 1440.0, 800.0);
    let vp = g.group_of(Viewport).unwrap().rect;
    for gr in &g.groups {
        assert!(gr.rect.w * gr.rect.h <= vp.w * vp.h + 1e-6);
    }
    let g = solve(&s, 1440.0, 800.0);
    let src = g.group_of(Source).unwrap().rect;
    for gr in &g.groups {
        assert!(gr.rect.w * gr.rect.h <= src.w * src.h + 1e-6);
    }
}

#[test]
fn validation_rejects_bad_trees() {
    let cases: Vec<(NodeWire, LayoutError)> = vec![
        (
            wsplit(
                "row",
                vec![
                    wtabs(&["panel.source"], "panel.source"),
                    wtabs(&["panel.source", "panel.viewport"], "panel.viewport"),
                ],
                vec![0.5, 0.5],
            ),
            LayoutError::DuplicatePanel(Source),
        ),
        (
            wtabs(&["panel.source"], "panel.source"),
            LayoutError::MissingRequired(Viewport),
        ),
        (
            wsplit("row", vec![base()], vec![1.0]),
            LayoutError::SplitTooSmall,
        ),
        (
            wsplit(
                "row",
                vec![wtabs(&[], "panel.source"), base()],
                vec![0.5, 0.5],
            ),
            LayoutError::EmptyTabGroup,
        ),
        (
            wsplit(
                "row",
                vec![
                    wtabs(&["panel.source"], "panel.viewport"),
                    wtabs(&["panel.viewport"], "panel.viewport"),
                ],
                vec![0.5, 0.5],
            ),
            LayoutError::ActiveNotInGroup,
        ),
        (
            wsplit("diagonal", vec![base(), base()], vec![0.5, 0.5]),
            LayoutError::UnknownAxis("diagonal".into()),
        ),
        (
            wsplit(
                "row",
                vec![
                    wtabs(&["panel.source"], "panel.source"),
                    wtabs(&["panel.viewport"], "panel.viewport"),
                ],
                vec![0.5],
            ),
            LayoutError::SizeCountMismatch,
        ),
    ];
    for (root, want) in cases {
        assert_eq!(
            Layout::from_wire(&wire(root.clone())).unwrap_err(),
            want,
            "{root:?}"
        );
    }
}

#[test]
fn validation_rejects_bad_sizes() {
    let two = |a: f64, b: f64| {
        wire(wsplit(
            "row",
            vec![
                wtabs(&["panel.source"], "panel.source"),
                wtabs(&["panel.viewport"], "panel.viewport"),
            ],
            vec![a, b],
        ))
    };
    for (a, b) in [
        (-0.5, 1.5),
        (0.0, 1.0),
        (f64::NAN, 0.5),
        (f64::INFINITY, 0.0),
        (0.01, 0.99),
        (1.5, -0.5),
    ] {
        assert_eq!(
            Layout::from_wire(&two(a, b)).unwrap_err(),
            LayoutError::BadSize,
            "{a} {b}"
        );
    }
    for (a, b) in [(0.5, 0.6), (0.3, 0.3), (0.9, 0.9)] {
        assert_eq!(
            Layout::from_wire(&two(a, b)).unwrap_err(),
            LayoutError::BadSizeSum,
            "{a} {b}"
        );
    }
    assert!(Layout::from_wire(&two(0.3, 0.7)).is_ok());
    assert!(Layout::from_wire(&two(0.3, 0.7005)).is_ok());
}

#[test]
fn validation_limits_depth_and_node_count() {
    // A chain deeper than MAX_DEPTH, padded with unknown IDs: the depth
    // budget counts what was WRITTEN, before unknown IDs are dropped.
    let mut n = base();
    for i in 0..MAX_DEPTH {
        let axis = if i % 2 == 0 { "column" } else { "row" };
        let pad = format!("panel.pad{i}");
        n = wsplit(axis, vec![n, wtabs(&[&pad], &pad)], vec![0.5, 0.5]);
    }
    assert_eq!(
        Layout::from_wire(&wire(n)).unwrap_err(),
        LayoutError::TooDeep
    );
    // One level less is accepted (and collapses to the base row).
    let mut ok = base();
    for i in 0..MAX_DEPTH - 2 {
        let pad = format!("panel.pad{i}");
        ok = wsplit("column", vec![ok, wtabs(&[&pad], &pad)], vec![0.5, 0.5]);
    }
    let (l, unknown) = Layout::from_wire(&wire(ok)).unwrap();
    assert_eq!(unknown.len(), MAX_DEPTH - 2);
    assert_eq!(l.visible(), vec![Source, Viewport]);
    // Too many nodes.
    let many: Vec<NodeWire> = (0..MAX_NODES)
        .map(|i| wtabs(&[&format!("panel.x{i}")], &format!("panel.x{i}")))
        .chain([base()])
        .collect();
    let k = many.len();
    let e = Layout::from_wire(&wire(wsplit("row", many, vec![1.0 / k as f64; k]))).unwrap_err();
    assert_eq!(e, LayoutError::TooManyNodes);
    // A constructed tree is held to the same depth rule.
    let mut deep = tabs(&[Source]);
    for _ in 0..MAX_DEPTH {
        deep = Node::Split {
            axis: SplitAxis::Row,
            children: vec![deep, tabs(&[Viewport])],
            sizes: vec![0.5, 0.5],
        };
    }
    let l = Layout {
        preset: PresetId::Source,
        root: deep,
    };
    assert_eq!(l.validate().unwrap_err(), LayoutError::TooDeep);
}

#[test]
fn unknown_ids_are_dropped_and_versions_checked() {
    let w = wire(wsplit(
        "row",
        vec![
            wtabs(&["panel.future", "panel.source"], "panel.future"),
            wtabs(&["panel.viewport"], "panel.viewport"),
            wtabs(&["panel.other"], "panel.other"),
        ],
        vec![0.3, 0.4, 0.3],
    ));
    let (l, unknown) = Layout::from_wire(&w).unwrap();
    assert_eq!(unknown, vec!["panel.future", "panel.other"]);
    assert_eq!(l.visible(), vec![Source, Viewport]);
    assert!(l.is_on_screen(Source));
    every_panel_once(&l);
    let mut w2 = wire(base());
    w2.version = 2;
    assert_eq!(
        Layout::from_wire(&w2).unwrap_err(),
        LayoutError::UnsupportedVersion(2)
    );
    w2.version = 0;
    assert_eq!(
        Layout::from_wire(&w2).unwrap_err(),
        LayoutError::UnsupportedVersion(0)
    );
    let mut w3 = wire(base());
    w3.preset = "preset.gaming".into();
    assert!(matches!(
        Layout::from_wire(&w3).unwrap_err(),
        LayoutError::UnknownPreset(_)
    ));
}

#[test]
fn edge_drops_create_the_right_split() {
    let l = Layout::preset(PresetId::Source);
    // Scene Tree to the right of the Viewport: a row inside the right column.
    let t = DropTarget {
        anchor: Viewport,
        zone: Zone::Right,
        index: None,
    };
    let n = l.dock(SceneTree, t).unwrap();
    every_panel_once(&n);
    let g = solve(&n, 1600.0, 900.0);
    let (st, vp) = (
        g.group_of(SceneTree).unwrap().rect,
        g.group_of(Viewport).unwrap().rect,
    );
    assert!(st.x > vp.x && (st.y - vp.y).abs() < 1e-6, "{st:?} {vp:?}");
    // Inspector above the Source: a column.
    let n2 = n
        .dock(
            Inspector,
            DropTarget {
                anchor: Source,
                zone: Zone::Top,
                index: None,
            },
        )
        .unwrap();
    every_panel_once(&n2);
    let g = solve(&n2, 1600.0, 900.0);
    let (ins, src) = (
        g.group_of(Inspector).unwrap().rect,
        g.group_of(Source).unwrap().rect,
    );
    assert!(ins.y < src.y && (ins.x - src.x).abs() < 1e-6);
    // Left and Bottom.
    for zone in [Zone::Left, Zone::Bottom] {
        let n3 = l
            .dock(
                Diagnostics,
                DropTarget {
                    anchor: Viewport,
                    zone,
                    index: None,
                },
            )
            .unwrap();
        every_panel_once(&n3);
        let g = solve(&n3, 1600.0, 900.0);
        let (d, v) = (
            g.group_of(Diagnostics).unwrap().rect,
            g.group_of(Viewport).unwrap().rect,
        );
        match zone {
            Zone::Left => assert!(d.x < v.x),
            _ => assert!(d.y > v.y),
        }
    }
}

#[test]
fn center_drops_make_and_extend_tab_groups() {
    let l = Layout::preset(PresetId::Source);
    let n = l
        .dock(
            Inspector,
            DropTarget {
                anchor: Viewport,
                zone: Zone::Center,
                index: None,
            },
        )
        .unwrap();
    every_panel_once(&n);
    assert_eq!(
        n.group_of(Viewport).unwrap(),
        (vec![Viewport, Inspector], Inspector)
    );
    // Insert at the front of the existing group.
    let n2 = n
        .dock(
            Diagnostics,
            DropTarget {
                anchor: Viewport,
                zone: Zone::Center,
                index: Some(0),
            },
        )
        .unwrap();
    assert_eq!(
        n2.group_of(Viewport).unwrap(),
        (vec![Diagnostics, Viewport, Inspector], Diagnostics)
    );
    every_panel_once(&n2);
}

#[test]
fn invalid_drops_leave_the_layout_unchanged() {
    let l = Layout::preset(PresetId::Source);
    let before = l.clone();
    // Onto itself.
    for zone in Zone::ALL {
        assert_eq!(
            l.dock(
                Source,
                DropTarget {
                    anchor: Source,
                    zone,
                    index: None
                }
            )
            .unwrap_err(),
            LayoutError::SameGroup
        );
    }
    // A hidden panel cannot be dragged or targeted.
    let h = l.hide(Inspector).unwrap();
    assert_eq!(
        h.dock(
            Inspector,
            DropTarget {
                anchor: Source,
                zone: Zone::Left,
                index: None
            }
        )
        .unwrap_err(),
        LayoutError::NotVisible(Inspector)
    );
    assert_eq!(
        h.dock(
            Source,
            DropTarget {
                anchor: Inspector,
                zone: Zone::Left,
                index: None
            }
        )
        .unwrap_err(),
        LayoutError::NotVisible(Inspector)
    );
    assert_eq!(l, before);
}

#[test]
fn tab_reorder_select_and_close() {
    let l = Layout::preset(PresetId::Modeling);
    // Source is the second tab beside Diagnostics.
    let r = l.move_tab(Source, 0).unwrap();
    assert_eq!(
        r.group_of(Source).unwrap(),
        (vec![Source, Diagnostics], Source)
    );
    assert_eq!(r.move_tab(Source, 0).unwrap_err(), LayoutError::NoChange);
    // The same thing as a tab-strip drop on its own group.
    let d = l
        .dock(
            Source,
            DropTarget {
                anchor: Diagnostics,
                zone: Zone::Center,
                index: Some(0),
            },
        )
        .unwrap();
    assert_eq!(d, r);
    // A same-group Center drop without an index changes nothing.
    assert_eq!(
        l.dock(
            Source,
            DropTarget {
                anchor: Diagnostics,
                zone: Zone::Center,
                index: None
            }
        )
        .unwrap_err(),
        LayoutError::SameGroup
    );
    // Split a tab out of its group to the left.
    let s = l
        .dock(
            Source,
            DropTarget {
                anchor: Diagnostics,
                zone: Zone::Left,
                index: None,
            },
        )
        .unwrap();
    assert_eq!(s.group_of(Source).unwrap().0, vec![Source]);
    every_panel_once(&s);
    let a = l.activate(Source).unwrap();
    assert!(a.is_on_screen(Source) && !a.is_on_screen(Diagnostics));
    // Closing the active tab activates its neighbor; the group stays.
    let c = a.hide(Diagnostics).unwrap();
    assert_eq!(c.group_of(Source).unwrap(), (vec![Source], Source));
    let c2 = l.hide(Diagnostics).unwrap();
    assert_eq!(c2.group_of(Source).unwrap(), (vec![Source], Source));
}

#[test]
fn required_panels_never_close_and_optional_ones_return_home() {
    for preset in PresetId::ALL {
        let l = Layout::preset(preset);
        assert_eq!(
            l.hide(Viewport).unwrap_err(),
            LayoutError::NotClosable(Viewport)
        );
        assert_eq!(
            l.hide(Source).unwrap_err(),
            LayoutError::NotClosable(Source)
        );
        let mut all = l.clone();
        for p in [SceneTree, Inspector, Diagnostics] {
            all = all.hide(p).unwrap();
            every_panel_once(&all);
            assert_eq!(all.hide(p).unwrap_err(), LayoutError::NotVisible(p));
        }
        assert_eq!(all.visible().len(), 2);
        // Restored in ANY order (all six), every panel comes back exactly
        // once and the result is the preset itself.
        let opt = [SceneTree, Inspector, Diagnostics];
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut r = all.clone();
            for i in order {
                let p = opt[i];
                r = r.show(p).unwrap();
                every_panel_once(&r);
                assert!(r.is_visible(p), "{preset:?} {p:?}");
            }
            assert_eq!(r.visible().len(), 5);
            assert!(r.approx_eq(&l, 1e-9), "{preset:?} {order:?}: {r:?}");
        }
        // Any subset closed and restored is the preset again.
        for mask in 1..8u8 {
            let mut r = l.clone();
            let closed: Vec<_> = (0..3)
                .filter(|i| mask & (1 << i) != 0)
                .map(|i| opt[i])
                .collect();
            for p in &closed {
                r = r.hide(*p).unwrap();
            }
            for p in closed.iter().rev() {
                r = r.show(*p).unwrap();
            }
            assert!(r.approx_eq(&l, 1e-9), "{preset:?} mask {mask}: {r:?}");
        }
        // Single restores go to the preset position.
        for p in [SceneTree, Inspector, Diagnostics] {
            let back = l.hide(p).unwrap().show(p).unwrap();
            let g0 = solve(&l, 1600.0, 900.0);
            let g1 = solve(&back, 1600.0, 900.0);
            let (a, b) = (g0.group_of(p).unwrap().rect, g1.group_of(p).unwrap().rect);
            assert!(
                (a.x - b.x).abs() < 400.0 && (a.y - b.y).abs() < 400.0,
                "{preset:?} {p:?}: {a:?} vs {b:?}"
            );
            assert_eq!(
                l.group_of(p).unwrap().0.len(),
                back.group_of(p).unwrap().0.len()
            );
        }
    }
}

#[test]
fn solve_respects_minimums_and_fills_the_container() {
    for preset in PresetId::ALL {
        let l = Layout::preset(preset);
        for (w, h) in [
            (1440.0, 800.0),
            (960.0, 527.0),
            (1600.0, 1000.0),
            (2560.0, 1400.0),
        ] {
            let g = solve(&l, w, h);
            assert_eq!((g.width, g.height), (w, h), "{preset:?} {w}x{h}");
            for gr in &g.groups {
                for p in &gr.panels {
                    let (mw, mh) = p.min_size();
                    assert!(
                        gr.content.w + 1e-6 >= mw,
                        "{preset:?} {p:?} {w}x{h} {:?}",
                        gr.content
                    );
                    assert!(
                        gr.content.h + 1e-6 >= mh,
                        "{preset:?} {p:?} {w}x{h} {:?}",
                        gr.content
                    );
                }
                assert!(gr.rect.x >= -1e-6 && gr.rect.x + gr.rect.w <= w + 1e-6);
                assert!(gr.rect.y >= -1e-6 && gr.rect.y + gr.rect.h <= h + 1e-6);
            }
            // Total area: groups + dividers tile the canvas.
            let area: f64 = g.groups.iter().map(|g| g.rect.w * g.rect.h).sum::<f64>()
                + g.dividers.iter().map(|d| d.rect.w * d.rect.h).sum::<f64>();
            assert!((area - w * h).abs() < 1.0, "{preset:?} {w}x{h}: {area}");
        }
    }
}

#[test]
fn a_small_window_overflows_instead_of_collapsing() {
    let l = Layout::preset(PresetId::Source);
    let (mw, mh) = min_size(&l.root);
    for (w, h) in [
        (400.0, 300.0),
        (0.0, 0.0),
        (-50.0, f64::NAN),
        (mw - 1.0, mh - 1.0),
    ] {
        let g = solve(&l, w, h);
        assert_eq!((g.width, g.height), (mw, mh), "{w}x{h}");
        for gr in &g.groups {
            assert!(gr.content.w > 0.0 && gr.content.h > 0.0);
            let (pw, ph) = gr.panels.iter().fold((0.0f64, 0.0f64), |a, p| {
                (a.0.max(p.min_size().0), a.1.max(p.min_size().1))
            });
            assert!(gr.content.w + 1e-6 >= pw && gr.content.h + 1e-6 >= ph);
        }
    }
}

#[test]
fn distribute_never_goes_below_a_minimum_or_negative() {
    assert_eq!(
        distribute(1000.0, &[0.5, 0.5], &[100.0, 100.0]),
        vec![500.0, 500.0]
    );
    let d = distribute(1000.0, &[0.05, 0.95], &[300.0, 100.0]);
    assert_eq!(d[0], 300.0);
    assert!((d[1] - 700.0).abs() < 1e-9);
    // The minimums do not fit: proportional compression, never negative.
    let d = distribute(300.0, &[0.5, 0.5], &[400.0, 200.0]);
    assert!((d[0] - 200.0).abs() < 1e-9 && (d[1] - 100.0).abs() < 1e-9);
    let d = distribute(-10.0, &[0.5, 0.5], &[10.0, 10.0]);
    assert!(d.iter().all(|x| *x >= 0.0));
    // Three parts: the two larger ones give up space for the small one.
    let d = distribute(1000.0, &[0.1, 0.45, 0.45], &[200.0, 100.0, 100.0]);
    assert_eq!(d[0], 200.0);
    assert!((d[1] - 400.0).abs() < 1e-9 && (d[2] - 400.0).abs() < 1e-9);
}

#[test]
fn divider_moves_are_clamped_to_the_neighbors_minimums() {
    let l = Layout::preset(PresetId::Source);
    let g = solve(&l, 1440.0, 800.0);
    // The root row: Scene Tree | Source column | right column.
    let d = g.divider(&[], 0).unwrap().clone();
    assert_eq!(d.axis, SplitAxis::Row);
    let (lo, hi) = d.range();
    assert!(lo <= 0.0 && hi >= 0.0);
    // Far left: the Scene Tree stops at its minimum.
    let n = l.resize(&g, &[], 0, -10_000.0).unwrap();
    let g2 = solve(&n, 1440.0, 800.0);
    let st = g2.group_of(SceneTree).unwrap().rect;
    assert!((st.w - 200.0).abs() < 0.5, "{st:?}");
    // Far right: the Source column stops at its minimum.
    let n = l.resize(&g, &[], 0, 10_000.0).unwrap();
    let g3 = solve(&n, 1440.0, 800.0);
    let src = g3.group_of(Source).unwrap().rect;
    assert!((src.w - 320.0).abs() < 0.5, "{src:?}");
    // Other parts keep their length.
    let right = g.group_of(Viewport).unwrap().rect.w;
    assert!((g3.group_of(Viewport).unwrap().rect.w - right).abs() < 0.5);
    // A vertical divider: Source over Diagnostics.
    let dv = g
        .dividers
        .iter()
        .find(|d| d.axis == SplitAxis::Column && d.path == vec![1])
        .unwrap();
    let n = l.resize(&g, &dv.path, dv.index, 10_000.0).unwrap();
    let g4 = solve(&n, 1440.0, 800.0);
    assert!((g4.group_of(Diagnostics).unwrap().content.h - 100.0).abs() < 0.5);
    // Non-finite input never changes anything.
    assert_eq!(
        l.resize(&g, &[], 0, f64::NAN).unwrap_err(),
        LayoutError::NoChange
    );
    assert_eq!(
        l.resize(&g, &[9], 0, 10.0).unwrap_err(),
        LayoutError::BadPath
    );
    // The stored ratios are valid.
    n.validate().unwrap();
}

#[test]
fn zones_and_guides() {
    let r = Rect {
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 200.0,
    };
    assert_eq!(zone_at(r, 300.0, 200.0), Some(Zone::Center));
    assert_eq!(zone_at(r, 105.0, 200.0), Some(Zone::Left));
    assert_eq!(zone_at(r, 495.0, 200.0), Some(Zone::Right));
    assert_eq!(zone_at(r, 300.0, 105.0), Some(Zone::Top));
    assert_eq!(zone_at(r, 300.0, 295.0), Some(Zone::Bottom));
    assert_eq!(zone_at(r, 50.0, 200.0), None);
    assert_eq!(zone_at(r, 300.0, 300.0), None);
    assert_eq!(zone_rect(r, Zone::Left).w, 200.0);
    assert_eq!(zone_rect(r, Zone::Right).x, 300.0);
    assert_eq!(zone_rect(r, Zone::Bottom).y, 200.0);
    assert_eq!(zone_rect(r, Zone::Top).h, 100.0);
    assert_eq!(insertion_index(&[10.0, 50.0, 90.0], 0.0), 0);
    assert_eq!(insertion_index(&[10.0, 50.0, 90.0], 60.0), 2);
    assert_eq!(insertion_index(&[10.0, 50.0, 90.0], 100.0), 3);
}

#[test]
fn every_single_drop_from_every_preset_is_valid_or_refused_whole() {
    // Exhaustive: every shown panel onto every zone of every group.
    for preset in PresetId::ALL {
        let l = Layout::preset(preset);
        for p in l.visible() {
            for anchor in l.visible() {
                for zone in Zone::ALL {
                    for index in [None, Some(0), Some(5)] {
                        let t = DropTarget {
                            anchor,
                            zone,
                            index,
                        };
                        match l.dock(p, t) {
                            Ok(n) => {
                                every_panel_once(&n);
                                assert_eq!(n.visible().len(), l.visible().len());
                                let (back, _) = Layout::from_wire(&n.to_wire()).unwrap();
                                assert_eq!(back, n);
                            }
                            Err(_) => assert_eq!(l, Layout::preset(preset)),
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn wire_json_shape_is_stable() {
    let w = Layout::preset(PresetId::Modeling).to_wire();
    assert_eq!(w.version, 1);
    assert_eq!(w.preset, "preset.modeling");
    match &w.root {
        NodeWire::Split { axis, children, .. } => {
            assert_eq!(axis, "row");
            assert_eq!(children.len(), 3);
        }
        other => panic!("{other:?}"),
    }
}

// ---- STACK-REVIEW-1 (PR #142 r4242637970): the snap guide is the result

fn tgt(anchor: PanelId, zone: Zone) -> DropTarget {
    DropTarget {
        anchor,
        zone,
        index: None,
    }
}

/// The moved panel's content box after the drop is really applied.
fn accepted(l: &Layout, p: PanelId, t: DropTarget, w: f64, h: f64) -> Rect {
    let n = l.dock(p, t).unwrap();
    solve(&n, w, h).group_of(p).unwrap().content
}

fn diff(a: Rect, b: Rect) -> f64 {
    [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
        .iter()
        .fold(0.0_f64, |m, d| m.max(d.abs()))
}

#[test]
fn drop_preview_is_the_accepted_content_box_for_every_drop() {
    let (mut ok, mut refused) = (0, 0);
    for preset in PresetId::ALL {
        let l = Layout::preset(preset);
        for (w, h) in [(1200.0, 760.0), (700.0, 420.0), (2400.0, 1400.0)] {
            for panel in l.visible() {
                for anchor in l.visible() {
                    for zone in Zone::ALL {
                        let t = tgt(anchor, zone);
                        match l.dock(panel, t) {
                            Ok(_) => {
                                let got = drop_preview(&l, panel, t, w, h).unwrap();
                                assert_eq!(
                                    got,
                                    accepted(&l, panel, t, w, h),
                                    "{preset:?} {panel:?} {t:?}"
                                );
                                ok += 1;
                            }
                            Err(e) => {
                                assert_eq!(drop_preview(&l, panel, t, w, h), Err(e));
                                refused += 1;
                            }
                        }
                    }
                }
            }
        }
        // Previews never change the live layout.
        assert_eq!(l, Layout::preset(preset));
    }
    assert!(ok > 200 && refused > 0, "ok={ok} refused={refused}");
}

#[test]
fn drop_preview_edges_in_all_four_directions_differ_from_the_zone_half() {
    // Source preset: Inspector onto the Viewport's four edges. The Inspector
    // leaves the Viewport's column (the split collapses), so the Viewport
    // group grows before it is split; the new group also has a tab strip.
    let l = Layout::preset(PresetId::Source);
    let (w, h) = (1200.0, 760.0);
    let vp = solve(&l, w, h).group_of(Viewport).unwrap().content;
    for zone in [Zone::Left, Zone::Right, Zone::Top, Zone::Bottom] {
        let t = tgt(Viewport, zone);
        let got = drop_preview(&l, Inspector, t, w, h).unwrap();
        assert_eq!(got, accepted(&l, Inspector, t, w, h), "{zone:?}");
        let old = zone_rect(vp, zone);
        assert!(
            diff(got, old) > TAB_STRIP_PX - 1.0,
            "{zone:?}: {got:?} vs {old:?}"
        );
    }
    // A Top / Bottom drop in a group that stays in place: the old guide
    // still ignored the new tab strip and the divider.
    let t = tgt(Viewport, Zone::Bottom);
    let got = drop_preview(&l, Diagnostics, t, w, h).unwrap();
    let old = zone_rect(vp, Zone::Bottom);
    assert!(got.h < old.h - TAB_STRIP_PX + 1.0, "{got:?} vs {old:?}");
}

#[test]
fn drop_preview_respects_minimum_sizes() {
    // A narrow workspace: half of the Viewport is below the Scene Tree's
    // minimum width, so the solver widens it (or the canvas scrolls).
    let l = Layout::preset(PresetId::Source);
    let (w, h) = (1000.0, 600.0);
    let t = tgt(Viewport, Zone::Left);
    let got = drop_preview(&l, SceneTree, t, w, h).unwrap();
    assert_eq!(got, accepted(&l, SceneTree, t, w, h));
    assert!(got.w >= SceneTree.min_size().0 - 1e-6, "{got:?}");
    let vp = solve(&l, w, h).group_of(Viewport).unwrap().content;
    assert!(
        zone_rect(vp, Zone::Left).w < SceneTree.min_size().0,
        "the case must bind"
    );
    let n = l.dock(SceneTree, t).unwrap();
    let g = solve(&n, w, h);
    assert!(g.group_of(Viewport).unwrap().content.w >= Viewport.min_size().0 - 1e-6);
}

#[test]
fn drop_preview_merge_split_and_one_tab_left_behind() {
    let (w, h) = (1200.0, 760.0);
    // Merge: Diagnostics joins the Source group as a tab. Its own group goes,
    // the Source column collapses, so the merged content box is taller than
    // the Source box before the drop.
    let l = Layout::preset(PresetId::Source);
    let src = solve(&l, w, h).group_of(Source).unwrap().content;
    let t = tgt(Source, Zone::Center);
    let got = drop_preview(&l, Diagnostics, t, w, h).unwrap();
    assert_eq!(got, accepted(&l, Diagnostics, t, w, h));
    assert!(got.h > src.h + 50.0, "{got:?} vs {src:?}");
    // Split: Scene Tree onto the Source group's right edge.
    let t = tgt(Source, Zone::Right);
    assert_eq!(
        drop_preview(&l, SceneTree, t, w, h).unwrap(),
        accepted(&l, SceneTree, t, w, h)
    );
    // Out of a two-tab group: Modeling's [Diagnostics, Source] keeps one tab.
    let m = Layout::preset(PresetId::Modeling);
    for zone in [
        Zone::Left,
        Zone::Right,
        Zone::Top,
        Zone::Bottom,
        Zone::Center,
    ] {
        let t = tgt(Viewport, zone);
        let got = drop_preview(&m, Source, t, w, h).unwrap();
        assert_eq!(got, accepted(&m, Source, t, w, h), "{zone:?}");
        let n = m.dock(Source, t).unwrap();
        assert_eq!(
            n.group_of(Diagnostics).unwrap().0,
            vec![Diagnostics],
            "{zone:?}"
        );
    }
    // A refused drop (onto itself) has no preview.
    assert!(drop_preview(&l, Source, tgt(Source, Zone::Left), w, h).is_err());
}
