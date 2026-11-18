//! What the platform's lists give the rows the kernel lays out: a grouped
//! list's cells and a cells list's margins.

use super::NodeArena;

impl NodeArena {
    /// The margins the platform's list gives the grouped list `slot`.
    pub(crate) fn grouped_cells(&self, slot: u32) -> Option<crate::grouped::GroupedCells> {
        self.grouped_cells
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, m)| *m)
    }

    pub(crate) fn has_grouped_cells(&self) -> bool {
        !self.grouped_cells.is_empty()
    }

    /// The margin the platform's cell gives the cells list `slot`'s rows.
    pub(crate) fn list_margins(&self, slot: u32) -> Option<[f32; 2]> {
        self.list_margins
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, m)| *m)
    }

    pub(crate) fn set_list_margins(&mut self, slot: u32, margins: Option<[f32; 2]>) {
        self.list_margins.retain(|(s, _)| *s != slot);
        self.list_margins.extend(margins.map(|m| (slot, m)));
    }

    pub(crate) fn set_grouped_cells(
        &mut self,
        slot: u32,
        margins: Option<crate::grouped::GroupedCells>,
    ) {
        self.grouped_cells.retain(|(s, _)| *s != slot);
        self.grouped_cells.extend(margins.map(|m| (slot, m)));
    }
}
