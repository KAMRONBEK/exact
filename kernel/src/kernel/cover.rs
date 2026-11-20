//! What a native container covers of a box (@ref LLP 1075.003 §3.5, §3.7).
//!
//! The host measures what its controller's bars take of a route — a
//! navigation bar over its top edge, a tab bar over its bottom — and the
//! kernel lays the route out inside what is left, as it lays a page out
//! inside `env(safe-area-inset-*)`: the covered edges add to the box's
//! padding. A box that a native bar replaces outright (a header whose
//! heading the bar shows) is covered whole and laid out as `display: none`.
//! Authored rows never change, and a host that reports nothing (the web,
//! Linux, the agent's presentation) lays out exactly as authored.
//!
//! A scroller whose content the platform itself insets — UIKit's adjusted
//! content inset under a navigation bar and over the home indicator — is
//! reported by those insets (`Inset`): its content box for layout excludes
//! them, so content that fills the scroller's height rests between the bar
//! and the indicator, while the content keeps its place (the platform's
//! inset moves it) and its published scroll extent leaves them out (the
//! platform adds them back). Scrolling content still passes under both.
//!
//! A covered route is a content owner (@ref LLP 1123 D3, D4): its cover is
//! its chrome's clearance, the owner's local inset on each edge its padding
//! executes. Clearance adds to the authored padding, as SwiftUI's safe area
//! adds to `padding()`, except where that padding already clears the inset
//! (it reads `env(safe-area-inset-<edge>)` or `var(--exact-safe-area-inset-
//! <edge>)`): the edge is then the larger of the two. `-exact-safe-area`
//! turns an edge's clearance off; the author pads it themselves. A header
//! the bar replaces neither clears nor is credited.
//!
//! The cells a platform's own list gives a grouped list (their width and
//! layout margins) are a claim of the same kind (`grouped::GroupedCells`),
//! set here.

use super::Kernel;
use crate::arena::NodeArena;
use crate::error::{KernelError, LayoutError};
use crate::id::ViewId;
use crate::layout::LayoutMirror;
use crate::style::{Dimension, Edge, Region};
use crate::{AlignItems, AlignSelf, FlexDirection};

/// A host container's claim on one box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostCover {
    /// Points the container's bars cover at the top, right, bottom and left
    /// edges, added to the box's padding.
    Edges([f32; 4]),
    /// Replaced by a native control: the box takes no space.
    Whole,
    /// A scroller the platform insets by these points at its top and bottom
    /// edges: subtracted from its content box's height for layout, never
    /// moving the content, and left out of its published extent.
    Inset([f32; 2]),
    /// Space occupied by native scrollers on the right and bottom edges.
    /// Overlay scrollers report zero; authored padding stays unchanged.
    Gutter([f32; 2]),
}

/// What host containers cover, by the box's slot, and the absolutely
/// positioned boxes placed past a covered box's bottom-edge chain
/// (`settle_chain`), each with the bottom clearance its inset starts past: a
/// few route nodes at most, so lists by slot rather than a column (and not
/// maps, whose code every web core would carry).
#[derive(Clone, Debug, Default)]
pub(crate) struct Covers {
    boxes: Vec<(u32, HostCover)>,
    chain: Vec<(u32, f32)>,
}

impl Covers {
    pub(crate) fn get(&self, slot: u32) -> Option<HostCover> {
        self.boxes.iter().find(|(s, _)| *s == slot).map(|(_, c)| *c)
    }

    pub(crate) fn set(&mut self, slot: u32, cover: Option<HostCover>) {
        self.boxes.retain(|(s, _)| *s != slot);
        self.boxes.extend(cover.map(|c| (slot, c)));
    }

    /// A freed slot's: its cover, and its place past a chain.
    pub(crate) fn remove(&mut self, slot: u32) {
        self.boxes.retain(|(s, _)| *s != slot);
        self.chain.retain(|(s, _)| *s != slot);
    }

    /// The bottom clearance the absolutely positioned `slot`'s inset starts
    /// past, its parent a box of a covered box's bottom-edge chain, as last
    /// settled.
    pub(crate) fn chain_past(&self, slot: u32) -> f32 {
        self.chain
            .iter()
            .find(|(s, _)| *s == slot)
            .map_or(0.0, |(_, p)| *p)
    }
}

impl Kernel {
    /// The bottom padding `key`'s cover adds to its layout as it is applied
    /// (`apply`): a bar's clearance, or none where the scroller at the box's
    /// bottom edge takes it as its platform inset.
    pub fn cover_bottom(&self, key: crate::NodeKey) -> f32 {
        applied_padding(&self.arena, key.index)[2]
    }

    /// Set or clear (`None`) what a host container covers of `view`.
    /// Nonfinite or negative edges are refused.
    pub fn set_host_cover(
        &mut self,
        view: ViewId,
        cover: Option<HostCover>,
    ) -> Result<(), KernelError> {
        let slot = self
            .arena
            .slot_of(view)
            .ok_or(LayoutError::UnknownView(view))?;
        let edges: &[f32] = match &cover {
            Some(HostCover::Edges(e)) => e,
            Some(HostCover::Inset(e) | HostCover::Gutter(e)) => e,
            _ => &[],
        };
        if !edges.iter().all(|e| e.is_finite() && *e >= 0.0) {
            return Err(LayoutError::InvalidIntrinsicSize(view).into());
        }
        let cover =
            cover.filter(|c| *c != HostCover::Edges([0.0; 4]) && *c != HostCover::Inset([0.0; 2]));
        let before = self.arena.cover(slot);
        if before == cover {
            return Ok(());
        }
        self.arena.set_cover(slot, cover);
        // PROTO: a block root's clearance moves its frames' origin whether or
        // not its layout changes (`clearance`): publish it again. Its cover is
        // its owner's band until a host gives one (`owner_band`), so what its
        // boxes observe and take is measured again.
        if self.arena.blocks.holds(slot) {
            self.arena.layout_dirty.insert(slot);
            if self.arena.observations.band(slot).is_none() {
                self.arena.observations.invalidate_band(Some(slot));
            }
        }
        if let Some(r) = &mut self.region {
            r.intrinsic(slot);
        }
        // A scroller the platform insets at the bottom edge, or stops
        // insetting, changes what the covered box it ends pads there
        // (`scroller_insets_bottom`), however deep.
        let whole = |c: Option<HostCover>| c == Some(HostCover::Whole);
        let bottom = |c: Option<HostCover>| matches!(c, Some(HostCover::Inset([_, b])) if b > 0.0);
        // A box replaced or given back also leaves the chain or joins it.
        // So does a box that comes to be covered, or stops: the walk stops
        // at it (`scroller_insets_bottom`).
        // And one inset at its top alone, where the walk stops too.
        let edges = |c: Option<HostCover>| matches!(c, Some(HostCover::Edges(_)));
        let inset = |c: Option<HostCover>| matches!(c, Some(HostCover::Inset(_)));
        let chain = (self.layout.is_some()
            && (bottom(before) != bottom(cover)
                || inset(before) != inset(cover)
                || whole(before) != whole(cover)
                || edges(before) != edges(cover)))
        .then(|| covering(&self.arena, slot))
        .flatten();
        // And a block under it is hidden or shown with it (`block::apply`).
        let blocks: Vec<u32> = if whole(before) != whole(cover) {
            let arena = &self.arena;
            (arena.blocks.roots())
                .filter(|b| *b != slot && arena.is_ancestor(slot, *b))
                .collect()
        } else {
            Vec::new()
        };
        // And a scroller that reaches past its sides, as far as they are covered (`side_reach`).
        let reaching = if edges(before) || edges(cover) {
            reaching(&self.arena, slot)
        } else {
            Vec::new()
        };
        for slot in std::iter::once(slot)
            .chain(chain)
            .chain(blocks)
            .chain(reaching)
        {
            if let (Some(node), Some(layout)) = (self.arena.taffy(slot), self.layout.as_deref_mut())
            {
                layout.restyle(&self.arena, slot, node);
                layout.mark_dirty(node);
            }
        }
        // And the absolute boxes a box of its bottom-edge chain holds.
        settle_chain(&mut self.arena, self.layout.as_deref_mut());
        Ok(())
    }
}

impl Kernel {
    /// Set or clear (`None`) the margin the platform's cell gives the rows of
    /// the cells list `view` at its left and right (LLP 1138). Nonfinite or
    /// negative lengths are refused.
    pub fn set_list_margins(
        &mut self,
        view: ViewId,
        margins: Option<[f32; 2]>,
    ) -> Result<(), KernelError> {
        let slot = self
            .arena
            .slot_of(view)
            .ok_or(LayoutError::UnknownView(view))?;
        if !margins
            .unwrap_or([0.0; 2])
            .iter()
            .all(|l| l.is_finite() && *l >= 0.0)
        {
            return Err(LayoutError::InvalidIntrinsicSize(view).into());
        }
        if self.arena.list_margins(slot) == margins {
            return Ok(());
        }
        self.arena.set_list_margins(slot, margins);
        if let Some(r) = &mut self.region {
            r.intrinsic(slot);
            r.within(&self.arena, slot);
        }
        if let Some(layout) = self.layout.as_deref_mut() {
            for &row in self.arena.children(slot) {
                crate::grouped::restyle(&self.arena, layout, row);
            }
        }
        Ok(())
    }

    /// Set or clear (`None`) the cells the platform's list gives the
    /// grouped list `view`. Nonfinite or negative lengths are refused.
    pub fn set_grouped_cells(
        &mut self,
        view: ViewId,
        margins: Option<crate::grouped::GroupedCells>,
    ) -> Result<(), KernelError> {
        let slot = self
            .arena
            .slot_of(view)
            .ok_or(LayoutError::UnknownView(view))?;
        let lengths = margins.map_or([0.0; 3], |m| [m.width, m.cell[0], m.cell[1]]);
        if !lengths.iter().all(|l| l.is_finite() && *l >= 0.0) {
            return Err(LayoutError::InvalidIntrinsicSize(view).into());
        }
        if self.arena.grouped_cells(slot) == margins {
            return Ok(());
        }
        self.arena.set_grouped_cells(slot, margins);
        if let Some(r) = &mut self.region {
            r.intrinsic(slot);
        }
        if let Some(layout) = self.layout.as_deref_mut() {
            for &section in self.arena.children(slot) {
                crate::grouped::restyle(&self.arena, layout, section);
            }
        }
        Ok(())
    }
}

/// A cover's part of a box's engine style (@ref LLP 1075.003 §3.5): a
/// container's bars add to the padding of the route they cover, a box a bar
/// replaces takes no space, and a scroller the platform insets ends its
/// content box that much sooner (LLP 1116.000 D1.2): both insets are a
/// gutter below its padding box, as a horizontal scrollbar's is, so its
/// children keep their start and its scroll extent ends where the
/// platform's own inset begins.
pub(crate) fn apply(arena: &NodeArena, slot: u32, s: &mut taffy::style::Style) {
    // A popover is the host's to lift, out of flow, open or closed (`style::top_layer`).
    crate::style::top_layer::apply(arena, slot, s);
    let style = arena.effective_style(slot);
    match arena.cover(slot) {
        Some(HostCover::Whole) => s.display = taffy::style::Display::None,
        Some(HostCover::Edges(_)) => {
            let cleared = applied_padding(arena, slot);
            let [top, right, bottom, left] = added_padding(arena, slot);
            s.rare.absolute_clearance = taffy::geometry::Rect {
                top,
                right,
                bottom,
                left,
            };
            // The authored padding as layout reads it (whole cells under a
            // terminal's space rule), with the clearance (LLP 1123 D3).
            let env = crate::kernel::owner::env_for(arena, slot, &style);
            let (down, across) = (crate::style::cells::ROW, crate::style::cells::COLUMN);
            let pad = |authored: &Dimension, cell: f32, edge: Edge| {
                let clearance = cleared[edge as usize];
                let spaced = authored.spaced(&env, cell);
                match clearance > 0.0 && credited(authored, edge, &env) {
                    true => spaced.floor_lp_at_least(&env, clearance),
                    false => spaced.floor_lp(&env, clearance),
                }
            };
            s.padding = taffy::geometry::Rect {
                top: pad(&style.padding_top, down, Edge::Top),
                right: pad(&style.padding_right, across, Edge::Right),
                bottom: pad(&style.padding_bottom, down, Edge::Bottom),
                left: pad(&style.padding_left, across, Edge::Left),
            };
        }
        Some(HostCover::Inset([top, bottom])) => {
            // A vertical scroller's hidden `overflow-x` lays out as `Scroll`,
            // the same box but for the bottom gutter, which Taffy reserves
            // only on a `Scroll` axis. A definite size, flex basis or aspect
            // ratio leaves the gutter out; a size taken from content counts
            // it, as a scrollbar's.
            if s.overflow.x == taffy::style::Overflow::Hidden {
                s.overflow.x = taffy::style::Overflow::Scroll;
            }
            lay_gutter(s, [0.0, top + bottom]);
        }
        Some(HostCover::Gutter(gutter)) => lay_gutter(s, gutter),
        None => {}
    }
    // A scroller under the covered box's side controls, its content where it was.
    let [right, left] = side_reach(arena, slot);
    let across = crate::style::cells::COLUMN;
    let env = &*crate::kernel::owner::env_for(arena, slot, &style);
    if right > 0.0 {
        s.margin.right = taffy::style::LengthPercentageAuto::length(-right);
        s.padding.right = style.padding_right.spaced(env, across).floor_lp(env, right);
    }
    if left > 0.0 {
        s.margin.left = taffy::style::LengthPercentageAuto::length(-left);
        s.padding.left = style.padding_left.spaced(env, across).floor_lp(env, left);
    }
    // The solver sizes and places absolute descendants inside the cleared
    // containing block (LLP 1123 D3). Mark matching inset references so they
    // clear the same edge once, after percentages and calculations resolve.
    if s.position == taffy::style::Position::Absolute {
        let env = crate::kernel::owner::env_for(arena, slot, &style);
        for (authored, edge) in [
            (&style.top, Edge::Top),
            (&style.right, Edge::Right),
            (&style.bottom, Edge::Bottom),
            (&style.left, Edge::Left),
        ] {
            if credited(authored, edge, &env) {
                s.rare.inset_credit |= 1 << edge as u8;
            }
        }
    }
    // An absolute child of a box in the bottom edge's chain, which reaches
    // under the bar (`applied_padding`), starts its bottom inset past the
    // clearance that box reaches over (`settle_chain`), as a view pinned to
    // the safe-area guide of UIKit's lower split view stays above the tab
    // bar. A percentage inset stays as written. A subtree in its own block
    // has no covered parent edge.
    if s.position == taffy::style::Position::Absolute && !arena.blocks.holds(slot) {
        let past = arena.covers.chain_past(slot);
        let raw = s.inset.bottom.into_raw();
        if past > 0.0 && raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
            s.inset.bottom = taffy::style::LengthPercentageAuto::length(raw.value() + past);
        }
    }
}

/// Whether the scroller at `slot`'s bottom edge is one the platform insets
/// there (`Inset`), the bar's own clearance for the box's bottom. It is
/// found as the host finds it: the last in-flow child, or that child's last,
/// down to the first scroller (a sheet component's root column holding its
/// header and then its scroll). A marked scroller off that chain counts for
/// nothing: the box keeps its padding (an inset twice at worst, never the
/// scroller lifted above the bar for good).
fn scroller_insets_bottom(arena: &NodeArena, slot: u32) -> bool {
    let mut at = slot;
    loop {
        let Some(last) = last_flowing(arena, at) else {
            return false;
        };
        match arena.cover(last) {
            Some(HostCover::Inset([_, bottom])) => return bottom > 0.0,
            // Another covered box decides its own bottom: what is inset
            // below it is its scroller, not this box's.
            Some(HostCover::Edges(_)) => return false,
            _ => {}
        }
        if scrolls(arena, last) {
            return false;
        }
        at = last;
    }
}

/// `slot`'s last child in flow, where its bottom-edge chain goes on
/// (`scroller_insets_bottom`).
fn last_flowing(arena: &NodeArena, slot: u32) -> Option<u32> {
    arena
        .children(slot)
        .iter()
        .rev()
        .copied()
        .find(|&c| flowing(arena, c))
}

/// Whether `c` is in flow for the edges a covered box's scrollers take:
/// shown, not absolutely positioned and not replaced by a native control.
fn flowing(arena: &NodeArena, c: u32) -> bool {
    arena.style(c).display != crate::Display::None
        // A popover is out of flow, open or closed (`style::top_layer`).
        && crate::style::top_layer::layout_position(arena, c) != crate::PositionType::Absolute
        && arena.cover(c) != Some(HostCover::Whole)
}

/// Settle which absolutely positioned boxes start past a covered box's
/// bottom clearance, and by how much (`apply`): the children of each box of
/// a bottom-edge chain whose scroller the platform insets there
/// (`scroller_insets_bottom`), from the covered box down to that scroller,
/// as those boxes reach under the bar with it. It runs after each batch and
/// each cover change, and derives again each box whose distance changed.
/// `apply` reads the settled distance rather than walking the chain, so a
/// box restyled partway through a batch is corrected here, and one that
/// leaves the chain, however it left, gives the distance back.
pub(crate) fn settle_chain<'a>(
    arena: &mut NodeArena,
    layout: Option<&mut (dyn LayoutMirror + 'a)>,
) {
    let mut now = Vec::new();
    for &(covered, cover) in &arena.covers.boxes {
        let HostCover::Edges([_, _, bottom, _]) = cover else {
            continue;
        };
        // PROTO: a block root's clearance is no chain's (`clearance`).
        if arena.blocks.holds(covered) {
            continue;
        }
        if bottom <= 0.0 || !arena.is_live(covered) || !scroller_insets_bottom(arena, covered) {
            continue;
        }
        let mut at = last_flowing(arena, covered);
        let on = |&c: &u32| {
            !matches!(
                arena.cover(c),
                Some(HostCover::Inset(_) | HostCover::Edges(_))
            ) && !scrolls(arena, c)
        };
        while let Some(chain) = at.filter(on) {
            now.extend(
                positioned_children(arena, chain)
                    .into_iter()
                    .map(|c| (c, bottom)),
            );
            at = last_flowing(arena, chain);
        }
    }
    if now == arena.covers.chain {
        return;
    }
    let before = std::mem::replace(&mut arena.covers.chain, now);
    let Some(layout) = layout else {
        return;
    };
    let arena = &*arena;
    let changed = |&&(slot, past): &&(u32, f32)| arena.covers.chain_past(slot) != past;
    let gained = |&&(slot, _): &&(u32, f32)| before.iter().all(|(s, _)| *s != slot);
    let now = &arena.covers.chain;
    for &(slot, _) in before
        .iter()
        .filter(changed)
        .chain(now.iter().filter(gained))
    {
        if let Some(node) = arena.is_live(slot).then(|| arena.taffy(slot)).flatten() {
            layout.restyle(arena, slot, node);
            layout.mark_dirty(node);
        }
    }
}

/// How far a `scroll` reaches past the sides of the box a container's bars
/// cover (`Edges`), right and left: that box's
/// side cover, on each side where the scroller lies on the box's content box
/// (LLP 1123 D3, "A scroller reaches an edge"). It lies there when it and
/// every box between them fill their parents across, in flow and with no
/// width or maximum width, and have no margin on that side, and the boxes above it no
/// padding or border there, the covered box included. The scroller is laid
/// out that much wider, and its content that much further in (`apply`), so
/// the content keeps its place while the scroller's box, and the scroll
/// indicator UIKit draws at its edge, run under the side controls (the
/// iPhone Duo's side strip), as a hand-built screen's scroll view does. A
/// list is left out: the platform's own list lays its rows out, and a
/// covered box's own scroller is it.
pub(crate) fn side_reach(arena: &NodeArena, slot: u32) -> [f32; 2] {
    use crate::style::Dimension::{Auto, Points};
    if arena.node_type(slot) != crate::NodeType::ScrollView
        || matches!(arena.cover(slot), Some(HostCover::Edges(_)))
    {
        return [0.0; 2];
    }
    let (mut right, mut left) = (true, true);
    let mut at = slot;
    while let Some(p) = arena.parent(at) {
        let (s, parent) = (arena.style(at), arena.style(p));
        let fills = match parent.display {
            crate::Display::Block => true,
            crate::Display::Flex => {
                matches!(
                    parent.flex_direction,
                    FlexDirection::Column | FlexDirection::ColumnReverse
                ) && match s.align_self {
                    AlignSelf::Auto => {
                        matches!(parent.align_items, AlignItems::Normal | AlignItems::Stretch)
                    }
                    AlignSelf::Stretch => true,
                    _ => false,
                }
            }
            _ => false,
        };
        if !fills || positioned(arena, at) || s.width != Auto || s.max_width != Auto {
            return [0.0; 2];
        }
        let [_, border_right, _, border_left] = parent.border_widths();
        right &= s.margin_right == Points(0.0)
            && parent.padding_right == Points(0.0)
            && border_right == 0.0;
        left &= s.margin_left == Points(0.0)
            && parent.padding_left == Points(0.0)
            && border_left == 0.0;
        if matches!(arena.cover(p), Some(HostCover::Edges(_))) {
            let [_, cover_right, _, cover_left] = added_padding(arena, p);
            return [
                if right { cover_right } else { 0.0 },
                if left { cover_left } else { 0.0 },
            ];
        }
        if scrolls(arena, p) {
            return [0.0; 2];
        }
        at = p;
    }
    [0.0; 2]
}

/// The scrollers that may reach past `slot`'s sides when it is covered
/// (`side_reach`): the first `scroll` on each path down from it, which
/// `apply` derives again when the cover changes.
pub(super) fn reaching(arena: &NodeArena, slot: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack: Vec<u32> = arena.children(slot).to_vec();
    while let Some(c) = stack.pop() {
        if positioned(arena, c) {
            continue;
        }
        if arena.node_type(c) == crate::NodeType::ScrollView {
            out.push(c);
        } else if !scrolls(arena, c) {
            stack.extend(arena.children(c).iter().copied());
        }
    }
    out
}

/// Whether `slot` scrolls vertically, as its engine style says (`style.rs`).
fn scrolls(arena: &NodeArena, slot: u32) -> bool {
    let s = arena.style(slot);
    let y = if !s.mask.has(crate::StyleId::OverflowY) && arena.node_type(slot).scrolls_by_default()
    {
        crate::Overflow::Auto
    } else {
        s.overflow_y
    };
    // As the engine computes it: `visible` beside a clipping axis scrolls.
    matches!(
        y.computed(s.overflow_x),
        crate::Overflow::Auto | crate::Overflow::Scroll
    )
}

/// The nearest box above `slot` a container's bars cover (`Edges`), whose
/// bottom padding a scroller at its edge decides (`scroller_insets_bottom`):
/// none when a scroller lies between, where that search stops.
fn covering(arena: &NodeArena, slot: u32) -> Option<u32> {
    let mut at = arena.parent(slot);
    while let Some(p) = at {
        if matches!(arena.cover(p), Some(HostCover::Edges(_))) {
            return Some(p);
        }
        if scrolls(arena, p) {
            return None;
        }
        at = arena.parent(p);
    }
    None
}

/// `slot`'s style changed (`txn::style_changed`, a layout row): its
/// `display`, `position` or `overflow-y` is its place on the bottom edge's
/// chain.
pub(crate) fn style_changed(
    arena: &NodeArena,
    layout: &mut dyn LayoutMirror,
    slot: u32,
    mask: &crate::StyleMask,
) {
    use crate::StyleId;
    if [
        StyleId::Display,
        StyleId::PositionType,
        StyleId::OverflowX,
        StyleId::OverflowY,
    ]
    .iter()
    .any(|&id| mask.has(id))
    {
        chain_changed(arena, layout, slot);
    }
    if [
        StyleId::Display,
        StyleId::FlexDirection,
        StyleId::AlignItems,
        StyleId::AlignSelf,
        StyleId::PositionType,
        StyleId::Width,
        StyleId::MaxWidth,
        StyleId::MarginLeft,
        StyleId::MarginRight,
        StyleId::PaddingLeft,
        StyleId::PaddingRight,
        StyleId::BorderWidthLeft,
        StyleId::BorderWidthRight,
        StyleId::BorderStyleLeft,
        StyleId::BorderStyleRight,
        StyleId::OverflowX,
        StyleId::OverflowY,
    ]
    .iter()
    .any(|&id| mask.has(id))
    {
        reach_changed(arena, layout, slot);
    }
}

/// `slot`'s place on the bottom edge's chain may have changed (its
/// `display`, `position` or `overflow-y`): the covered box above it is
/// derived again (`scroller_insets_bottom`).
pub(crate) fn chain_changed(arena: &NodeArena, layout: &mut dyn LayoutMirror, slot: u32) {
    if layout.tree_ref().is_none() {
        return;
    }
    if let Some((above, node)) = covering(arena, slot).and_then(|a| Some((a, arena.taffy(a)?))) {
        layout.restyle(arena, above, node);
        layout.mark_dirty(node);
    }
}

/// The points a cover adds to the box's padding as layout pads it (`apply`):
/// `added_padding`, but nothing at the top or the bottom where the scroller
/// at that edge takes the bar's clearance as its platform inset (LLP
/// 1116.000 D1.2), so it reaches under the bar. An absolutely positioned
/// child is still placed past the whole cover (`added_padding`).
pub(crate) fn applied_padding(arena: &NodeArena, slot: u32) -> [f32; 4] {
    let [top, right, bottom, left] = added_padding(arena, slot);
    let bottom = if bottom > 0.0 && scroller_insets_bottom(arena, slot) {
        0.0
    } else {
        bottom
    };
    [top, right, bottom, left]
}

/// What the clearance adds to the box's padding beyond its authored padding,
/// as layout pads it (`apply`): `applied_padding`, less what a credited
/// padding already clears (LLP 1123 D3), so the padding a host reads back is
/// the authored one. A percentage is of the parent's width as last laid out.
pub(crate) fn contributed_padding(arena: &NodeArena, slot: u32) -> [f32; 4] {
    let applied = applied_padding(arena, slot);
    if applied == [0.0; 4] {
        return applied;
    }
    let style = arena.effective_style(slot);
    let env = crate::kernel::owner::env_for(arena, slot, &style);
    let basis = arena.parent(slot).map_or(0.0, |p| arena.frame(p).width);
    let pads = [
        &style.padding_top,
        &style.padding_right,
        &style.padding_bottom,
        &style.padding_left,
    ];
    std::array::from_fn(|i| {
        let clearance = applied[i];
        match clearance > 0.0 && credited(pads[i], Edge::ALL[i], &env) {
            true => (clearance - pads[i].at_basis(&env, basis).max(0.0)).max(0.0),
            false => clearance,
        }
    })
}

/// The absolutely positioned children of `slot`: derived again whenever the
/// cover's points change (a cover cleared included, so the caller says when).
pub(crate) fn positioned_children(arena: &NodeArena, slot: u32) -> Vec<u32> {
    (arena.children(slot).iter().copied())
        .filter(|&c| positioned(arena, c))
        .collect()
}

/// Whether `slot` lays out absolute: authored so, or a popover (`style::top_layer`).
fn positioned(arena: &NodeArena, slot: u32) -> bool {
    crate::style::top_layer::layout_position(arena, slot) == crate::PositionType::Absolute
}

/// The points a cover adds to the box's padding for layout (`apply`), top,
/// right, bottom and left: the host's sizing, not the box's CSS padding.
pub(crate) fn added_padding(arena: &NodeArena, slot: u32) -> [f32; 4] {
    match arena.cover(slot) {
        // PROTO (safe-area design §2.2): a block root's `Edges` is its
        // clearance, the safe area its root is laid out inside
        // (`clearance`), never padding added to it.
        Some(HostCover::Edges(_)) if arena.blocks.holds(slot) => [0.0; 4],
        Some(HostCover::Edges(edges)) => {
            let off = arena.style(slot).rare.safe_area;
            std::array::from_fn(|i| if off & (1 << i) != 0 { 0.0 } else { edges[i] })
        }
        _ => [0.0; 4],
    }
}

/// PROTO (safe-area design §2.2): the band of the content owner rooted at
/// `slot`, top right bottom left in its block's space: the band its host
/// gives (`Kernel::set_owner_band`), else its root's cover (`Edges`), the safe
/// area the iOS host reports for a route. Every reader of the band takes it
/// from here: the owner's clearance (`block::cleared`), the area its boxes
/// observe and take (`area::band_at`, LLP 1123 D4, D12) and their margins (D8).
pub(crate) fn owner_band(arena: &NodeArena, slot: u32) -> Option<[f32; 4]> {
    arena
        .observations
        .band(slot)
        .or_else(|| match arena.cover(slot) {
            Some(HostCover::Edges(e)) if arena.blocks.holds(slot) => Some(e),
            _ => None,
        })
}

/// The edges, as bits top 1, right 2, bottom 4 and left 8, whose padding in
/// `style` clears its own inset ([`credited`]): what a host that executes an
/// owner's clearance itself (a scroller's content inset) credits.
pub fn credited_edges(style: &crate::StyleProps, env: &crate::style::Env) -> u8 {
    let pads = [
        &style.padding_top,
        &style.padding_right,
        &style.padding_bottom,
        &style.padding_left,
    ];
    (0..4).fold(0, |bits, i| match credited(pads[i], Edge::ALL[i], env) {
        true => bits | 1 << i,
        false => bits,
    })
}

/// Whether an owner root's authored padding on `edge` clears that edge's
/// inset itself (LLP 1123 D3, "How credit is defined"): it references
/// `env(safe-area-inset-<edge>)`, the corner-adapted safe area's, which is
/// never less (D10), or `var(--exact-safe-area-inset-<edge>)`, anywhere in it.
pub(crate) fn credited(padding: &Dimension, edge: Edge, env: &crate::style::Env) -> bool {
    match padding {
        Dimension::Env(e, _) | Dimension::Region(Region::SafeAreaCorners, e, _) => *e == edge,
        Dimension::Compare(c) => c.reads(edge, env),
        _ => false,
    }
}

/// Whether the platform insets this scroller (`Inset`).
pub(crate) fn inset(arena: &NodeArena, slot: u32) -> bool {
    matches!(arena.cover(slot), Some(HostCover::Inset(_)))
}

/// `s` as authored, without the platform's inset (`apply`).
pub(crate) fn without_inset(arena: &NodeArena, slot: u32, s: &mut taffy::style::Style) {
    if inset(arena, slot) {
        s.scrollbar_width = 0.0;
    }
}

/// Reserve native scrollbar gutters beside and below the padding box (CSS
/// 2.1 §11.1.1). Taffy reserves one width, the larger of `right` and
/// `bottom`, on each `Scroll` axis, and `Hidden` keeps the same scroll
/// container without reserving that axis's gutter.
fn lay_gutter(s: &mut taffy::style::Style, [right, bottom]: [f32; 2]) {
    s.scrollbar_width = right.max(bottom);
    if right == 0.0 && s.overflow.y == taffy::style::Overflow::Scroll {
        s.overflow.y = taffy::style::Overflow::Hidden;
    }
    if bottom == 0.0 && s.overflow.x == taffy::style::Overflow::Scroll {
        s.overflow.x = taffy::style::Overflow::Hidden;
    }
}

/// A box's children changed: a covered box's bottom padding follows the
/// scroller at its bottom edge (`scroller_insets_bottom`), so its engine
/// style is derived again.
pub(crate) fn children_changed(
    arena: &NodeArena,
    layout: &mut dyn LayoutMirror,
    slot: u32,
    touched: &mut Vec<crate::NodeKey>,
) {
    if let (Some(HostCover::Edges(_)), Some(node)) = (arena.cover(slot), arena.taffy(slot)) {
        layout.restyle(arena, slot, node);
        layout.mark_dirty(node);
    }
    if layout.tree_ref().is_some() {
        // And the scroller at the covered box's bottom edge may be another
        // (`scroller_insets_bottom`), however deep the change, unless the
        // box is that covered box itself (above) or a scroller, where the
        // search stops.
        if !matches!(arena.cover(slot), Some(HostCover::Edges(_))) && !scrolls(arena, slot) {
            chain_changed(arena, layout, slot);
        }
        reach_changed(arena, layout, slot);
    }
    crate::rows::children_changed(arena, layout, slot, touched);
}

/// A box at or under a covered one changed what decides how far a scroller
/// reaches past the covered box's sides (`side_reach`): its fill, width,
/// margins, padding, borders or children. The scrollers that may reach
/// (`reaching`) are derived again.
fn reach_changed(arena: &NodeArena, layout: &mut dyn LayoutMirror, slot: u32) {
    let mut at = Some(slot);
    while let Some(s) = at {
        if matches!(arena.cover(s), Some(HostCover::Edges(_))) {
            for r in reaching(arena, s) {
                if let Some(node) = arena.taffy(r) {
                    layout.restyle(arena, r, node);
                    layout.mark_dirty(node);
                }
            }
            return;
        }
        at = arena.parent(s);
    }
}
