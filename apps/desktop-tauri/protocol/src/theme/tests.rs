// SPDX-License-Identifier: GPL-3.0-or-later
//! The theme contract, checked against the stylesheets that actually ship.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

const THEMES_CSS: &str = include_str!("../../../ui/static/themes.css");
const STYLE_CSS: &str = include_str!("../../../ui/static/style.css");
const INDEX_HTML: &str = include_str!("../../../ui/static/index.html");
const LICENSE: &str = include_str!("../../../ui/static/LICENSE-tokyo-night.txt");

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = match rest[i + 2..].find("*/") {
            Some(j) => &rest[i + 2 + j + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// (selector, body) for every top-level rule. Enough for these flat files.
fn rules(css: &str) -> Vec<(String, String)> {
    let css = strip_comments(css);
    let mut out = vec![];
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let sel = rest[..open].trim().to_string();
        let mut depth = 0;
        let mut end = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.expect("unbalanced braces");
        out.push((sel, rest[open + 1..end].to_string()));
        rest = &rest[end + 1..];
    }
    out
}

fn declarations(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// theme id -> its declared custom properties.
fn theme_blocks() -> BTreeMap<String, (String, BTreeMap<String, String>)> {
    let mut out = BTreeMap::new();
    for (sel, body) in rules(THEMES_CSS) {
        let mut ids = vec![];
        let mut s = sel.as_str();
        while let Some(i) = s.find("[data-theme=\"") {
            let r = &s[i + 13..];
            let j = r.find('"').unwrap();
            ids.push(r[..j].to_string());
            s = &r[j..];
        }
        assert!(
            !ids.is_empty(),
            "themes.css rule without a data-theme selector: {sel}"
        );
        let decls: BTreeMap<_, _> = declarations(&body)
            .into_iter()
            .filter(|(k, _)| k.starts_with("--"))
            .collect();
        for id in ids {
            assert!(
                out.insert(id.clone(), (sel.clone(), decls.clone()))
                    .is_none(),
                "theme {id} defined twice"
            );
        }
    }
    out
}

#[test]
fn registry_is_the_three_tokyo_night_themes_with_tokyo_night_default() {
    let ids: Vec<_> = THEMES.iter().map(|t| t.id).collect();
    assert_eq!(
        ids,
        ["tokyo-night", "tokyo-night-storm", "tokyo-night-light"]
    );
    assert_eq!(DEFAULT_THEME, "tokyo-night");
    assert!(is_known(DEFAULT_THEME));
    assert!(!is_known("Tokyo Night"));
    assert!(!is_known(""));
    assert!(!is_known("tokyo-night "));
    assert!(!is_known("../tokyo-night"));
    let st = ThemeState::new("tokyo-night-light", None);
    assert_eq!(st.themes.len(), 3);
}

#[test]
fn required_tokens_are_unique_and_prefixed() {
    let set: BTreeSet<_> = REQUIRED_TOKENS.iter().collect();
    assert_eq!(set.len(), REQUIRED_TOKENS.len(), "duplicate token name");
    assert!(REQUIRED_TOKENS.iter().all(|t| t.starts_with("--wf-")));
}

#[test]
fn every_theme_defines_exactly_the_required_tokens() {
    let blocks = theme_blocks();
    let css_ids: BTreeSet<_> = blocks.keys().cloned().collect();
    let reg_ids: BTreeSet<_> = THEMES.iter().map(|t| t.id.to_string()).collect();
    assert_eq!(css_ids, reg_ids, "themes.css and the registry disagree");
    let want: BTreeSet<_> = REQUIRED_TOKENS.iter().map(|s| s.to_string()).collect();
    for (id, (_, decls)) in &blocks {
        let have: BTreeSet<_> = decls.keys().cloned().collect();
        let missing: Vec<_> = want.difference(&have).collect();
        let extra: Vec<_> = have.difference(&want).collect();
        assert!(missing.is_empty(), "{id}: missing tokens {missing:?}");
        assert!(
            extra.is_empty(),
            "{id}: undeclared tokens {extra:?} (add to REQUIRED_TOKENS)"
        );
        for (k, v) in decls {
            assert!(
                !v.is_empty() && !v.contains("var("),
                "{id}: {k} must be a literal"
            );
        }
    }
}

#[test]
fn a_missing_token_fails_the_contract() {
    // The checker itself must notice an omission, not just pass on real input.
    let (_, mut decls) = theme_blocks().remove("tokyo-night-storm").unwrap();
    decls.remove("--wf-focus-ring");
    let have: BTreeSet<_> = decls.keys().map(String::as_str).collect();
    assert!(REQUIRED_TOKENS.iter().any(|t| !have.contains(t)));
}

#[test]
fn default_theme_is_the_unattributed_fallback() {
    let (sel, _) = &theme_blocks()["tokyo-night"];
    let parts: Vec<_> = sel.split(',').map(str::trim).collect();
    assert!(
        parts.contains(&":root"),
        "Tokyo Night must also match bare :root: {sel}"
    );
    for (id, (sel, _)) in theme_blocks() {
        if id != DEFAULT_THEME {
            assert!(
                !sel.split(',').any(|p| p.trim() == ":root"),
                "{id} must not claim :root"
            );
        }
    }
}

#[test]
fn every_theme_declares_its_color_scheme() {
    for (sel, body) in rules(THEMES_CSS) {
        let d = declarations(&body);
        let cs = d
            .iter()
            .find(|(k, _)| k == "color-scheme")
            .map(|x| x.1.clone());
        let id = sel.split('"').nth(1).unwrap().to_string();
        let dark = find(&id).unwrap().dark;
        assert_eq!(
            cs.as_deref(),
            Some(if dark { "dark" } else { "light" }),
            "{id}"
        );
    }
}

const NAMED_COLORS: &[&str] = &[
    "white", "black", "red", "green", "blue", "yellow", "orange", "purple", "gray", "grey",
    "silver", "navy", "teal", "maroon", "olive", "lime", "aqua", "fuchsia", "pink", "gold",
    "brown", "cyan", "magenta", "indigo", "violet",
];

#[test]
fn component_styles_use_tokens_not_raw_colors() {
    let css = strip_comments(STYLE_CSS);
    // Values only: `x3d-canvas#viewport` is a selector, not a colour. Nested
    // blocks (@media) are flattened by scanning every `prop: value` pair.
    let values: Vec<(String, String)> = rules(&css)
        .iter()
        .flat_map(|(_, b)| {
            let inner = rules(b);
            if inner.is_empty() {
                declarations(b)
            } else {
                inner.iter().flat_map(|(_, ib)| declarations(ib)).collect()
            }
        })
        .collect();
    assert!(
        values.len() > 150,
        "parsed too few declarations: {}",
        values.len()
    );
    for (prop, value) in &values {
        for bad in ["#", "rgb(", "rgba(", "hsl(", "hsla(", "color-mix("] {
            assert!(!value.contains(bad), "style.css {prop}: raw colour {value}");
        }
        if prop.starts_with("--wf-font-") {
            continue;
        }
        for word in value
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .filter(|w| !w.is_empty())
        {
            assert!(
                !NAMED_COLORS.contains(&word.to_ascii_lowercase().as_str()),
                "style.css {prop}: named colour {word}"
            );
        }
    }
    // Every token style.css reads is a contract token (or one of its own
    // non-colour font tokens).
    let own: BTreeSet<_> = rules(STYLE_CSS)
        .iter()
        .flat_map(|(_, b)| declarations(b))
        .filter(|(k, _)| k.starts_with("--"))
        .map(|(k, _)| k)
        .collect();
    assert!(
        own.iter()
            .all(|k| k == "--ui" || k.starts_with("--wf-font-")),
        "{own:?}"
    );
    let mut rest = css.as_str();
    let mut used = BTreeSet::new();
    while let Some(i) = rest.find("var(") {
        let r = &rest[i + 4..];
        let j = r.find([')', ',']).unwrap();
        used.insert(r[..j].trim().to_string());
        rest = &r[j..];
    }
    for u in &used {
        assert!(
            REQUIRED_TOKENS.contains(&u.as_str()) || own.contains(u),
            "style.css reads an undefined token {u}"
        );
    }
    assert!(
        used.len() > 50,
        "style.css should be fully tokenised ({} tokens)",
        used.len()
    );
}

#[test]
fn stylesheets_need_no_external_resources() {
    for (name, src) in [("themes.css", THEMES_CSS), ("style.css", STYLE_CSS)] {
        let s = strip_comments(src);
        for bad in ["url(", "@import", "http:", "https:", "@font-face"] {
            assert!(!s.contains(bad), "{name} references {bad}");
        }
    }
    let t = INDEX_HTML
        .find("href=\"themes.css\"")
        .expect("index links themes.css");
    let s = INDEX_HTML
        .find("href=\"style.css\"")
        .expect("index links style.css");
    assert!(t < s, "themes.css must load before style.css");
    assert!(!INDEX_HTML.contains("http:") && !INDEX_HTML.contains("https:"));
}

#[test]
fn upstream_license_ships_with_the_themes() {
    assert!(LICENSE.contains("The MIT License"));
    assert!(LICENSE.contains("Copyright (c) 2018-present Enkia"));
    let head = &THEMES_CSS[..THEMES_CSS.find("*/").unwrap()];
    assert!(head.contains("Copyright (c) 2018-present Enkia"));
    assert!(head.contains("7c0f11eaef322f293621ca7befe462214b7ea468"));
}

// ---- WCAG 2.x contrast -------------------------------------------------

fn rgb(hex: &str) -> [f64; 3] {
    let h = hex.trim_start_matches('#');
    assert_eq!(
        h.len(),
        6,
        "contrast-checked token must be opaque #rrggbb: {hex}"
    );
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64 / 255.0;
    [c(0), c(2), c(4)]
}

fn luminance(hex: &str) -> f64 {
    let lin = |v: f64| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = rgb(hex);
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// (foreground, background) pairs that carry readable text: AA 4.5:1.
const TEXT_PAIRS: &[(&str, &str)] = &[
    ("--wf-text-primary", "--wf-surface-chrome"),
    ("--wf-text-primary", "--wf-surface-panel"),
    ("--wf-text-primary", "--wf-surface-header"),
    ("--wf-text-primary", "--wf-surface-hover"),
    ("--wf-text-primary", "--wf-surface-raised"),
    ("--wf-text-editor", "--wf-surface-editor"),
    ("--wf-text-editor", "--wf-surface-input"),
    ("--wf-text-secondary", "--wf-surface-panel"),
    ("--wf-text-secondary", "--wf-surface-chrome"),
    ("--wf-text-muted", "--wf-surface-panel"),
    ("--wf-text-muted", "--wf-surface-chrome"),
    ("--wf-text-muted", "--wf-surface-header"),
    ("--wf-text-muted", "--wf-surface-hover"),
    ("--wf-text-placeholder", "--wf-surface-editor"),
    ("--wf-text-placeholder", "--wf-surface-input"),
    ("--wf-text-heading", "--wf-surface-panel"),
    ("--wf-text-brand", "--wf-surface-chrome"),
    ("--wf-text-selected", "--wf-surface-selected"),
    ("--wf-text-on-accent", "--wf-btn-primary-bg"),
    ("--wf-btn-primary-fg", "--wf-btn-primary-bg"),
    ("--wf-btn-primary-fg", "--wf-btn-primary-hover-bg"),
    ("--wf-btn-primary-fg", "--wf-btn-primary-pressed-bg"),
    ("--wf-btn-secondary-fg", "--wf-btn-secondary-bg"),
    ("--wf-btn-secondary-fg", "--wf-btn-secondary-hover-bg"),
    ("--wf-btn-secondary-fg", "--wf-btn-secondary-pressed-bg"),
    ("--wf-btn-selected-fg", "--wf-btn-selected-bg"),
    ("--wf-tab-active-fg", "--wf-tab-active-bg"),
    ("--wf-tab-inactive-fg", "--wf-tab-inactive-bg"),
    ("--wf-tree-selected-fg", "--wf-tree-selected-bg"),
    ("--wf-text-primary", "--wf-tree-hover-bg"),
    ("--wf-tree-kind", "--wf-surface-panel"),
    ("--wf-tree-kind", "--wf-tree-selected-bg"),
    ("--wf-tree-route", "--wf-surface-panel"),
    ("--wf-tree-route", "--wf-tree-selected-bg"),
    ("--wf-tree-use", "--wf-surface-panel"),
    ("--wf-tree-use", "--wf-tree-selected-bg"),
    ("--wf-tree-proto", "--wf-surface-panel"),
    ("--wf-tree-proto", "--wf-tree-selected-bg"),
    ("--wf-tree-unresolved", "--wf-surface-panel"),
    ("--wf-tree-unresolved", "--wf-tree-selected-bg"),
    ("--wf-error-fg", "--wf-error-bg"),
    ("--wf-error-fg", "--wf-surface-panel"),
    ("--wf-warning-fg", "--wf-warning-bg"),
    ("--wf-warning-fg", "--wf-surface-panel"),
    ("--wf-warning-fg", "--wf-surface-chrome"),
    ("--wf-success-fg", "--wf-success-bg"),
    ("--wf-success-fg", "--wf-surface-panel"),
    ("--wf-info-fg", "--wf-info-bg"),
    ("--wf-info-fg", "--wf-surface-panel"),
    ("--wf-badge-fg", "--wf-badge-bg"),
    ("--wf-text-primary", "--wf-viewport-header-bg"),
    ("--wf-viewport-status-fg", "--wf-viewport-bg"),
    ("--wf-editor-selection-fg", "--wf-editor-selection-bg"),
    ("--wf-syntax-comment", "--wf-surface-editor"),
    ("--wf-syntax-keyword", "--wf-surface-editor"),
    ("--wf-syntax-string", "--wf-surface-editor"),
    ("--wf-syntax-number", "--wf-surface-editor"),
    ("--wf-syntax-node-type", "--wf-surface-editor"),
    ("--wf-syntax-field", "--wf-surface-editor"),
    ("--wf-syntax-def-name", "--wf-surface-editor"),
    ("--wf-syntax-route", "--wf-surface-editor"),
    ("--wf-syntax-punctuation", "--wf-surface-editor"),
    ("--wf-syntax-invalid", "--wf-surface-editor"),
];

/// Non-text UI that identifies a control or state: 3:1.
const UI_PAIRS: &[(&str, &str)] = &[
    ("--wf-focus-ring", "--wf-surface-chrome"),
    ("--wf-focus-ring", "--wf-surface-panel"),
    ("--wf-focus-ring", "--wf-surface-header"),
    ("--wf-focus-ring", "--wf-surface-editor"),
    ("--wf-focus-ring", "--wf-surface-input"),
    ("--wf-focus-ring", "--wf-viewport-header-bg"),
    ("--wf-input-focus-border", "--wf-surface-panel"),
    ("--wf-border-control", "--wf-surface-panel"),
    ("--wf-border-control", "--wf-surface-chrome"),
    ("--wf-tree-selected-indicator", "--wf-tree-selected-bg"),
    ("--wf-tree-selected-indicator", "--wf-surface-panel"),
    ("--wf-tab-active-border", "--wf-tab-active-bg"),
    ("--wf-editor-caret", "--wf-surface-editor"),
    // UI-SYNTAX-1 diagnostic underlines in the source editor.
    ("--wf-error-fg", "--wf-surface-editor"),
    ("--wf-warning-fg", "--wf-surface-editor"),
    ("--wf-icon-active", "--wf-surface-chrome"),
    ("--wf-icon-inactive", "--wf-surface-chrome"),
    ("--wf-scrollbar-thumb", "--wf-scrollbar-track"),
    ("--wf-accent", "--wf-surface-panel"),
    ("--wf-axis-x", "--wf-viewport-header-bg"),
    ("--wf-axis-y", "--wf-viewport-header-bg"),
    ("--wf-axis-z", "--wf-viewport-header-bg"),
];

#[test]
fn every_theme_meets_wcag_aa_for_its_text_and_controls() {
    let mut fails = vec![];
    for (id, (_, t)) in theme_blocks() {
        for (pairs, min) in [(TEXT_PAIRS, 4.5), (UI_PAIRS, 3.0)] {
            for (fg, bg) in pairs {
                let r = contrast(&t[*fg], &t[*bg]);
                if r < min {
                    fails.push(format!(
                        "{id}: {fg} {} on {bg} {} = {r:.2} < {min}",
                        t[*fg], t[*bg]
                    ));
                }
            }
        }
    }
    assert!(fails.is_empty(), "contrast failures:\n{}", fails.join("\n"));
}

#[test]
fn contrast_math_matches_wcag_reference_values() {
    assert!((contrast("#ffffff", "#000000") - 21.0).abs() < 1e-9);
    assert!((contrast("#777777", "#ffffff") - 4.48).abs() < 0.01);
}
