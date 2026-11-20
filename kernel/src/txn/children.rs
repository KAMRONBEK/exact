//! A parent's children pushed into the layout engine.

use crate::arena::NodeArena;
use crate::id::NodeKey;
use crate::layout::LayoutMirror;

/// Push the arena's child list for `parent` into the layout engine. A `Text`
/// parent's children are inline runs: they are measured with it, not laid out.
/// The nodes a joining child brings that read the environment go to `touched`.
pub(super) fn sync_children(
    arena: &mut NodeArena,
    layout: &mut dyn LayoutMirror,
    parent: u32,
    touched: &mut Vec<NodeKey>,
) {
    let Some(node) = arena.taffy(parent) else {
        return;
    };
    // Children joining here from elsewhere, or built detached: their styles
    // were converted under another owner's content root, which container
    // units read, or none (LLP 1123 D16).
    let joining: Vec<u32> = match layout.tree_ref() {
        Some(tree) if arena.blocks.any_given() => arena
            .children(parent)
            .iter()
            .copied()
            .filter(|c| {
                arena
                    .taffy(*c)
                    .is_some_and(|n| tree.engine_parent(n) != Some(node))
            })
            .collect(),
        _ => Vec::new(),
    };
    let _entries = layout.sync_children(arena, parent, node);
    restyle_environment_readers(arena, layout, joining, touched);
    // Styles that read a box's children.
    arena.alert_children_changed(layout, parent, node);
    crate::segmented::children_changed(arena, layout, parent);
    crate::kernel::bar::children_changed(arena, layout, parent, node);
    #[cfg(test)]
    super::count(_entries, 0);
}

/// Converts again the styles under `roots` that read the environment, which
/// a subtree reads from the content owner it is now in, or from the document
/// in the top layer (LLP 1123 D16). An image among them is measured again with
/// its paragraph, and each is touched: a host sends its style again though
/// nothing it inherits changed.
pub(crate) fn restyle_environment_readers(
    arena: &mut NodeArena,
    layout: &mut dyn LayoutMirror,
    mut roots: Vec<u32>,
    touched: &mut Vec<NodeKey>,
) {
    while let Some(slot) = roots.pop() {
        if crate::style::uses_env(arena.style(slot)) {
            if let Some(node) = arena.taffy(slot) {
                layout.restyle(arena, slot, node);
                layout.mark_dirty(node);
            }
            if arena.is_inline_image(slot) {
                super::invalidate_text(arena, layout, slot);
            }
            touched.push(arena.key(slot));
        }
        roots.extend_from_slice(arena.children(slot));
    }
}
