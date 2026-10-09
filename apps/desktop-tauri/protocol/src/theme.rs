// SPDX-License-Identifier: GPL-3.0-or-later
//! Built-in UI themes (UI-THEME-1): the registry both sides validate against,
//! the persisted-settings shape, and the semantic token contract that every
//! theme in `ui/static/themes.css` must satisfy.
//!
//! Theme selection is an APPLICATION preference. It never touches a document,
//! its revision, its history or its source text.

use serde::{Deserialize, Serialize};

/// The default theme, and the fallback for anything unknown or unreadable.
pub const DEFAULT_THEME: &str = "tokyo-night";

/// Version of the persisted `settings.json` shape.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeDef {
    /// Stable id: persisted, and written to `<html data-theme>`.
    pub id: &'static str,
    pub label: &'static str,
    pub dark: bool,
}

/// The ONLY theme ids the backend accepts or persists.
pub const THEMES: &[ThemeDef] = &[
    ThemeDef {
        id: "tokyo-night",
        label: "Tokyo Night",
        dark: true,
    },
    ThemeDef {
        id: "tokyo-night-storm",
        label: "Tokyo Night Storm",
        dark: true,
    },
    ThemeDef {
        id: "tokyo-night-light",
        label: "Tokyo Night Light",
        dark: false,
    },
];

pub fn find(id: &str) -> Option<&'static ThemeDef> {
    THEMES.iter().find(|t| t.id == id)
}

pub fn is_known(id: &str) -> bool {
    find(id).is_some()
}

/// What the UI needs at startup: the active theme, the choices, and a
/// non-fatal note when the stored preference could not be used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeState {
    pub theme_id: String,
    pub themes: Vec<ThemeChoice>,
    /// e.g. "settings file is corrupt; using Tokyo Night".
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeChoice {
    pub id: String,
    pub label: String,
}

impl ThemeState {
    pub fn new(theme_id: &str, notice: Option<String>) -> Self {
        ThemeState {
            theme_id: theme_id.to_string(),
            themes: THEMES
                .iter()
                .map(|t| ThemeChoice {
                    id: t.id.into(),
                    label: t.label.into(),
                })
                .collect(),
            notice,
        }
    }
}

/// Reply to `theme_set`. An unknown id is refused before anything is written;
/// a failed write leaves the previous settings file untouched.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ThemeSetOutcome {
    Saved {
        theme_id: String,
    },
    /// The theme is a known id but persisting it failed. The UI may keep the
    /// theme for this run; it must show `message`.
    NotSaved {
        theme_id: String,
        message: String,
    },
    Rejected {
        message: String,
    },
}

/// Every token each built-in theme must define in `themes.css`. Stable names:
/// future panels and workspaces depend on them. Add, never rename.
pub const REQUIRED_TOKENS: &[&str] = &[
    // surfaces
    "--wf-bg-app",
    "--wf-surface-chrome",
    "--wf-surface-panel",
    "--wf-surface-header",
    "--wf-surface-editor",
    "--wf-surface-input",
    "--wf-surface-raised",
    "--wf-surface-hover",
    "--wf-surface-selected",
    // text
    "--wf-text-primary",
    "--wf-text-editor",
    "--wf-text-secondary",
    "--wf-text-muted",
    "--wf-text-disabled",
    "--wf-text-on-accent",
    "--wf-text-selected",
    "--wf-text-placeholder",
    "--wf-text-heading",
    "--wf-text-brand",
    "--wf-accent",
    "--wf-link",
    // borders
    "--wf-border",
    "--wf-border-subtle",
    "--wf-border-control",
    "--wf-separator",
    // buttons
    "--wf-btn-primary-bg",
    "--wf-btn-primary-hover-bg",
    "--wf-btn-primary-pressed-bg",
    "--wf-btn-primary-fg",
    "--wf-btn-secondary-bg",
    "--wf-btn-secondary-hover-bg",
    "--wf-btn-secondary-pressed-bg",
    "--wf-btn-secondary-fg",
    "--wf-btn-disabled-bg",
    "--wf-btn-disabled-fg",
    "--wf-btn-selected-bg",
    "--wf-btn-selected-fg",
    // toolbar icons
    "--wf-icon-active",
    "--wf-icon-inactive",
    // tabs / panel headers
    "--wf-tab-active-bg",
    "--wf-tab-active-fg",
    "--wf-tab-inactive-bg",
    "--wf-tab-inactive-fg",
    "--wf-tab-active-border",
    // tree
    "--wf-tree-hover-bg",
    "--wf-tree-selected-bg",
    "--wf-tree-selected-fg",
    "--wf-tree-selected-indicator",
    "--wf-tree-kind",
    "--wf-tree-route",
    "--wf-tree-use",
    "--wf-tree-proto",
    "--wf-tree-unresolved",
    // focus
    "--wf-focus-ring",
    "--wf-input-focus-border",
    // states
    "--wf-error-fg",
    "--wf-error-bg",
    "--wf-error-border",
    "--wf-warning-fg",
    "--wf-warning-bg",
    "--wf-warning-border",
    "--wf-success-fg",
    "--wf-success-bg",
    "--wf-success-border",
    "--wf-info-fg",
    "--wf-info-bg",
    "--wf-info-border",
    // badges
    "--wf-badge-fg",
    "--wf-badge-bg",
    "--wf-badge-border",
    // scrollbars
    "--wf-scrollbar-track",
    "--wf-scrollbar-thumb",
    "--wf-scrollbar-thumb-hover",
    "--wf-scrollbar-thumb-active",
    // source editor
    "--wf-editor-selection-bg",
    "--wf-editor-selection-fg",
    "--wf-editor-caret",
    "--wf-editor-line-number",
    "--wf-editor-active-line",
    // syntax (source editor colors, UI-SYNTAX-1)
    "--wf-syntax-comment",
    "--wf-syntax-keyword",
    "--wf-syntax-string",
    "--wf-syntax-number",
    "--wf-syntax-node-type",
    "--wf-syntax-field",
    "--wf-syntax-def-name",
    "--wf-syntax-route",
    "--wf-syntax-punctuation",
    "--wf-syntax-invalid",
    // viewport chrome
    "--wf-viewport-bg",
    "--wf-viewport-canvas-bg",
    "--wf-viewport-header-bg",
    "--wf-viewport-status-fg",
    "--wf-viewport-border",
    // transform-overlay axes (reserved)
    "--wf-axis-x",
    "--wf-axis-y",
    "--wf-axis-z",
    // elevation
    "--wf-shadow",
];

#[cfg(test)]
mod tests;
