// SPDX-License-Identifier: GPL-3.0-or-later
//! LV2-C Duplicate placement: where the copy goes so the user SEES it.
//! Pure, std-only; no renderer, DOM or document access.
//!
//! THE RULE. The copy moves by [`step`] (about the object's rendered size)
//! along ONE axis of its parent's coordinate system, so only one translation
//! component is written. The axes are tried in a fixed order -- +X, -X, +Y,
//! -Y, +Z, -Z -- and the first one is taken whose copy
//!
//! * lies wholly inside the rendered canvas (all eight box corners in front
//!   of the camera and inside it), and
//! * sits visibly apart from the original on screen (its box center moves
//!   at least half the original's on-screen size, and at least
//!   [`MIN_APART_PX`]).
//!
//! When no axis satisfies both, the first visibly-apart axis (else +X) is
//! taken and [`Choice::in_view`] is `false`: the caller frames the camera
//! on the original and the copy (camera only) and says so.
//!
//! Nothing is guessed: the size and frame come from the node's PROVEN
//! rendered bounds ([`Bounds`]); without them there is no choice at all and
//! the copy is placed exactly over the original, with the reason reported.

use serde::{Deserialize, Serialize};

use crate::gizmo::{self as gz, Camera, V3};
use crate::highlight::{ancestor_matrix, parent_extent, world_corners, Bounds};

/// The smallest on-screen move (CSS px) that counts as "visibly apart".
pub const MIN_APART_PX: f64 = 12.0;

/// Why no placement could be chosen.
pub const NO_SIZE: &str = "the preview has no proven size for the selected node";

/// The copy's offset: `step` (signed) along parent axis `axis` (0 = X).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Offset {
    pub axis: u8,
    pub step: f64,
}

impl Offset {
    /// The offset is one an editor may write: a real axis, a finite,
    /// non-zero step within the size [`step`] can produce.
    pub fn valid(&self) -> bool {
        self.axis < 3 && self.step.is_finite() && self.step != 0.0 && self.step.abs() < 1e8
    }
    pub fn axis_name(&self) -> char {
        ['X', 'Y', 'Z'][usize::from(self.axis.min(2))]
    }
}

/// The placement chosen for one Duplicate.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub offset: Offset,
    /// The copy is wholly inside the current view, apart from the original.
    pub in_view: bool,
    /// World corners of the original's box and of the copy's box.
    pub original: [V3; 8],
    pub copy: [V3; 8],
}

/// The duplicate offset for an object whose largest rendered extent in its
/// parent's coordinates is `extent`: 1.2 x that, rounded UP to a short
/// number (1, 2, 2.5 or 5 x 10^k) so the written translation stays readable.
/// `None` without a usable extent.
pub fn step(extent: f64) -> Option<f64> {
    if !(extent.is_finite() && extent > 1e-6 && extent < 1e7) {
        return None;
    }
    let v = extent * 1.2;
    let k = 10f64.powf(v.log10().floor());
    let m = v / k;
    let n = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .find(|n| *n >= m - 1e-9)
        .unwrap_or(10.0);
    // Round away the binary noise of `n * k` (0.025, not 0.025000000000000001).
    let s = format!("{:.6}", n * k);
    s.parse::<f64>().ok().filter(|x| *x > 0.0)
}

/// The order the axes are tried in: (axis, sign).
pub const ORDER: [(u8, f64); 6] = [
    (0, 1.0),
    (0, -1.0),
    (1, 1.0),
    (1, -1.0),
    (2, 1.0),
    (2, -1.0),
];

fn screen(cam: &Camera, corners: &[V3; 8]) -> Option<[(f64, f64); 8]> {
    let mut out = [(0.0, 0.0); 8];
    for (o, c) in out.iter_mut().zip(corners) {
        *o = gz::project(cam, *c)?;
    }
    Some(out)
}

/// (center, larger side) of the screen rectangle around `px`.
fn extent_px(px: &[(f64, f64); 8]) -> ((f64, f64), f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in px {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    (((x0 + x1) / 2.0, (y0 + y1) / 2.0), (x1 - x0).max(y1 - y0))
}

/// Choose the copy's placement for a node with proven `bounds`, seen
/// through `cam` (see the module docs for the rule).
pub fn choose(cam: &Camera, bounds: &Bounds) -> Result<Choice, &'static str> {
    let extent = parent_extent(bounds).ok_or(NO_SIZE)?;
    let step = step(extent).ok_or(NO_SIZE)?;
    let parent = ancestor_matrix(&bounds.ancestors)?;
    let original = world_corners(bounds)?;
    let before = screen(cam, &original).map(|px| extent_px(&px));
    let mut fallback: Option<Choice> = None;
    for (axis, sign) in ORDER {
        let mut unit = [0.0; 3];
        unit[usize::from(axis)] = 1.0;
        let d = gz::frame_dir(Some(&parent), unit);
        let copy = original.map(|c| gz::add_scaled(c, d, sign * step));
        let after = screen(cam, &copy);
        let apart = match (before, after) {
            (Some((c0, size)), Some(px)) => {
                let (c1, _) = extent_px(&px);
                (c1.0 - c0.0).hypot(c1.1 - c0.1) >= (size / 2.0).max(MIN_APART_PX)
            }
            _ => false,
        };
        let inside = after.is_some_and(|px| px.iter().all(|p| cam.contains(*p)));
        let choice = Choice {
            offset: Offset {
                axis,
                step: sign * step,
            },
            in_view: apart && inside,
            original,
            copy,
        };
        if choice.in_view {
            return Ok(choice);
        }
        if fallback.is_none() && apart {
            fallback = Some(choice);
        }
    }
    Ok(fallback.unwrap_or_else(|| {
        let d = gz::frame_dir(Some(&parent), [1.0, 0.0, 0.0]);
        Choice {
            offset: Offset { axis: 0, step },
            in_view: false,
            original,
            copy: original.map(|c| gz::add_scaled(c, d, step)),
        }
    }))
}

#[cfg(test)]
mod tests;
