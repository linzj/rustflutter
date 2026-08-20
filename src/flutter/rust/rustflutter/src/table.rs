//! The table -- a port of upstream's `widgets/table.dart`.
//!
//! A table is written as rows and laid out as **one flat list of cells**. That
//! conversion is where its rules come from: five assertions in the
//! constructor, and a row-matching algorithm that has to reconstruct which
//! cell belonged to which row after the flattening threw that away.
//!
//! The assertion worth reading twice is the last one. **Cell keys must be
//! unique across the whole table, not merely within a row**, because by the
//! time keys are matched the rows are gone. Two cells in different rows
//! carrying the same key look, from where the matcher stands, like the same
//! cell twice.
//!
//! ## What is not here
//!
//! `RenderTable`, the column-sizing algorithms and the border painting belong
//! to [`crate::render`]. What is ported is the row, the cell, the
//! constructor's checks, and the element's row matching.

use crate::painting::TextBaseline;

/// Upstream `TableCellVerticalAlignment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableCellVerticalAlignment {
    #[default]
    Top,
    Middle,
    Bottom,
    /// Aligns the cells' text baselines, which is the one alignment that
    /// **needs to know which baseline** -- alphabetic and ideographic sit at
    /// different heights, and there is no sensible default across scripts.
    Baseline,
    /// The cell is stretched to the row's height.
    Fill,
    /// The cell keeps its own intrinsic height.
    Intrinsic,
}

/// A cell as the checks need it: an identity and an optional key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableChild {
    pub widget: u64,
    pub key: Option<u64>,
}

impl TableChild {
    pub fn new(widget: u64) -> TableChild {
        TableChild { widget, key: None }
    }

    pub fn keyed(widget: u64, key: u64) -> TableChild {
        TableChild {
            widget,
            key: Some(key),
        }
    }
}

/// Upstream `TableRow`.
///
/// It is **not a widget** -- it has no `build`, no element, and never appears
/// in the tree. It is a way of writing down a run of cells, and the table
/// takes it apart immediately. The row's `key` survives that, and is the only
/// thing that lets the element put the row back together on the next build.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableRow {
    /// Upstream's `key`, a `LocalKey`.
    pub key: Option<u64>,
    /// Upstream's `decoration`, which "fills the horizontal and vertical
    /// extent of each row in the table, unlike decorations for individual
    /// cells, which might not fill either". A cell may be shorter than its
    /// row; a row decoration never is.
    pub decoration: Option<u64>,
    pub children: Vec<TableChild>,
}

impl TableRow {
    pub fn new(children: Vec<TableChild>) -> TableRow {
        TableRow {
            key: None,
            decoration: None,
            children,
        }
    }

    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }

    pub fn with_decoration(mut self, decoration: u64) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Upstream's `toString`, which names what is there and says "no children"
    /// rather than printing an empty list.
    pub fn describe(&self) -> String {
        let mut parts = String::from("TableRow(");
        if let Some(key) = self.key {
            parts.push_str(&format!("[{key}], "));
        }
        if let Some(decoration) = self.decoration {
            parts.push_str(&format!("{decoration}, "));
        }
        if self.children.is_empty() {
            parts.push_str("no children");
        } else {
            let widgets: Vec<String> = self
                .children
                .iter()
                .map(|child| child.widget.to_string())
                .collect();
            parts.push_str(&format!("[{}]", widgets.join(", ")));
        }
        parts.push(')');
        parts
    }
}

/// What is wrong with a table, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    /// Upstream: "textBaseline is required if you specify the
    /// defaultVerticalAlignment with TableCellVerticalAlignment.baseline".
    BaselineAlignmentWithoutBaseline,
    /// Upstream: "All the keyed TableRow children of a Table must have
    /// different Keys."
    DuplicateRowKey(u64),
    /// Upstream: "Every TableRow in a Table must have the same number of
    /// children, so that every cell is filled. Otherwise, the table will
    /// contain holes."
    IrregularRowLengths,
    /// Upstream: "Every TableRow in a Table must have at least one child, so
    /// there is no empty row."
    EmptyRow,
    /// Upstream: cells are flattened for processing, "so separate cells cannot
    /// have duplicate keys even if they are in different rows".
    DuplicateCellKey(u64),
}

/// Upstream `Table`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub children: Vec<TableRow>,
    pub default_vertical_alignment: TableCellVerticalAlignment,
    pub text_baseline: Option<TextBaseline>,
    /// Upstream's `_rowDecorations`, which is **null unless some row has a
    /// decoration**. Most tables have none, and a list of nulls per row is a
    /// list the painter would have to walk every frame to learn nothing.
    row_decorations: Option<Vec<Option<u64>>>,
}

impl Table {
    /// Upstream's constructor, whose five assertions all run before the table
    /// exists.
    pub fn new(children: Vec<TableRow>) -> Result<Table, TableError> {
        Table::with_alignment(children, TableCellVerticalAlignment::Top, None)
    }

    pub fn with_alignment(
        children: Vec<TableRow>,
        default_vertical_alignment: TableCellVerticalAlignment,
        text_baseline: Option<TextBaseline>,
    ) -> Result<Table, TableError> {
        if default_vertical_alignment == TableCellVerticalAlignment::Baseline
            && text_baseline.is_none()
        {
            return Err(TableError::BaselineAlignmentWithoutBaseline);
        }

        let mut seen_row_keys: Vec<u64> = Vec::new();
        for row in children.iter() {
            if let Some(key) = row.key {
                if seen_row_keys.contains(&key) {
                    return Err(TableError::DuplicateRowKey(key));
                }
                seen_row_keys.push(key);
            }
        }

        if let Some(first) = children.first() {
            let cell_count = first.children.len();
            if children.iter().any(|row| row.children.len() != cell_count) {
                return Err(TableError::IrregularRowLengths);
            }
            if children.iter().any(|row| row.children.is_empty()) {
                return Err(TableError::EmptyRow);
            }
        }

        // The cells are flattened for processing, so a key repeated across
        // rows is a repeated key.
        let mut seen_cell_keys: Vec<u64> = Vec::new();
        for row in children.iter() {
            for child in row.children.iter() {
                if let Some(key) = child.key {
                    if seen_cell_keys.contains(&key) {
                        return Err(TableError::DuplicateCellKey(key));
                    }
                    seen_cell_keys.push(key);
                }
            }
        }

        let row_decorations = if children.iter().any(|row| row.decoration.is_some()) {
            Some(children.iter().map(|row| row.decoration).collect())
        } else {
            None
        };

        Ok(Table {
            children,
            default_vertical_alignment,
            text_baseline,
            row_decorations,
        })
    }

    pub fn row_decorations(&self) -> Option<&[Option<u64>]> {
        self.row_decorations.as_deref()
    }

    pub fn column_count(&self) -> usize {
        self.children
            .first()
            .map(|row| row.children.len())
            .unwrap_or(0)
    }

    /// Upstream's `setFlatChildren`: the cells in row-major order, which is
    /// the form the render object actually holds them in.
    pub fn flat_children(&self) -> Vec<u64> {
        self.children
            .iter()
            .flat_map(|row| row.children.iter().map(|child| child.widget))
            .collect()
    }
}

/// Upstream `TableCell`: per-cell configuration.
///
/// It is a `StatelessWidget` that wraps a `ParentDataWidget`, and the wrapping
/// is not ceremony -- it is also where the cell's **semantics role** is
/// attached, so a screen reader announces a table cell as one whether or not
/// the caller wrapped their child.
///
/// A child of a `TableRow` need **not** be wrapped in one. Wrapping is how a
/// cell overrides the table's default alignment, and nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableCell {
    /// `None` means "use the table's default", which is why the field is
    /// nullable rather than defaulting to `Top` here.
    pub vertical_alignment: Option<TableCellVerticalAlignment>,
}

impl TableCell {
    pub fn new(vertical_alignment: Option<TableCellVerticalAlignment>) -> TableCell {
        TableCell { vertical_alignment }
    }

    /// The alignment this cell ends up with, given the table's default.
    pub fn resolve(&self, table_default: TableCellVerticalAlignment) -> TableCellVerticalAlignment {
        self.vertical_alignment.unwrap_or(table_default)
    }

    /// Upstream's `applyParentData`, which marks the **parent** for layout
    /// rather than the cell. A cell's alignment is decided against its row's
    /// height, so the row is what has to be measured again.
    pub fn apply_parent_data(&self, current: &mut Option<TableCellVerticalAlignment>) -> bool {
        if *current == self.vertical_alignment {
            return false;
        }
        *current = self.vertical_alignment;
        true
    }
}

/// A row of elements, as the element keeps them between builds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableElementRow {
    pub key: Option<u64>,
    pub children: Vec<u64>,
}

/// Upstream's `_TableElement.update`, as the matching it performs.
///
/// Keyed and unkeyed rows are matched from **two separate pools**, which is
/// the design: a keyed row is looked up by key wherever it moved to, and an
/// unkeyed row is matched **positionally against the sequence of old unkeyed
/// rows only**. An unkeyed row is therefore never matched against a keyed one,
/// so inserting a keyed row at the top does not shift every unkeyed row's
/// identity by one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableRowMatcher {
    rows: Vec<TableElementRow>,
    next_element: u64,
    deactivated: Vec<u64>,
}

impl TableRowMatcher {
    pub fn new() -> TableRowMatcher {
        TableRowMatcher {
            rows: Vec::new(),
            next_element: 1,
            deactivated: Vec::new(),
        }
    }

    pub fn rows(&self) -> &[TableElementRow] {
        &self.rows
    }

    pub fn deactivated(&self) -> &[u64] {
        &self.deactivated
    }

    /// Builds or rebuilds against `table`, returning the element ids per row.
    pub fn update(&mut self, table: &Table) {
        let old_rows = std::mem::take(&mut self.rows);
        let mut old_keyed: Vec<&TableElementRow> =
            old_rows.iter().filter(|row| row.key.is_some()).collect();
        let mut old_unkeyed = old_rows.iter().filter(|row| row.key.is_none());
        let mut taken: Vec<usize> = Vec::new();
        let mut new_rows: Vec<TableElementRow> = Vec::new();

        for row in table.children.iter() {
            let old_children: Vec<u64> = match row.key {
                Some(key) => match old_keyed.iter().position(|old| old.key == Some(key)) {
                    Some(at) => {
                        taken.push(at);
                        old_keyed[at].children.clone()
                    }
                    None => Vec::new(),
                },
                None => match old_unkeyed.next() {
                    Some(old) => old.children.clone(),
                    None => Vec::new(),
                },
            };

            let mut children = Vec::new();
            for (column, _) in row.children.iter().enumerate() {
                match old_children.get(column) {
                    Some(existing) => children.push(*existing),
                    None => {
                        children.push(self.next_element);
                        self.next_element += 1;
                    }
                }
            }
            // Cells beyond the new row's width are gone.
            for gone in old_children.iter().skip(row.children.len()) {
                self.deactivated.push(*gone);
            }
            new_rows.push(TableElementRow {
                key: row.key,
                children,
            });
        }

        // Old unkeyed rows the new table did not reach.
        for leftover in old_unkeyed {
            self.deactivated.extend(leftover.children.iter().copied());
        }
        // Keyed rows nothing claimed.
        for (at, old) in old_keyed.iter().enumerate() {
            if !taken.contains(&at) {
                self.deactivated.extend(old.children.iter().copied());
            }
        }
        old_keyed.clear();

        self.rows = new_rows;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(widgets: &[u64]) -> TableRow {
        TableRow::new(widgets.iter().map(|id| TableChild::new(*id)).collect())
    }

    // -- The five constructor checks ---------------------------------------

    #[test]
    fn baseline_alignment_has_to_say_which_baseline() {
        // Alphabetic and ideographic sit at different heights, and there is no
        // sensible default across scripts.
        assert_eq!(
            Table::with_alignment(vec![row(&[1])], TableCellVerticalAlignment::Baseline, None),
            Err(TableError::BaselineAlignmentWithoutBaseline)
        );
        assert!(
            Table::with_alignment(
                vec![row(&[1])],
                TableCellVerticalAlignment::Baseline,
                Some(TextBaseline::Alphabetic)
            )
            .is_ok()
        );
        assert!(
            Table::with_alignment(vec![row(&[1])], TableCellVerticalAlignment::Top, None).is_ok(),
            "and every other alignment needs no baseline"
        );
    }

    #[test]
    fn two_rows_cannot_share_a_key() {
        assert_eq!(
            Table::new(vec![row(&[1]).with_key(7), row(&[2]).with_key(7)]),
            Err(TableError::DuplicateRowKey(7))
        );
        assert!(Table::new(vec![row(&[1]).with_key(7), row(&[2])]).is_ok());
    }

    #[test]
    fn a_table_with_ragged_rows_would_contain_holes() {
        assert_eq!(
            Table::new(vec![row(&[1, 2]), row(&[3])]),
            Err(TableError::IrregularRowLengths)
        );
        assert!(Table::new(vec![row(&[1, 2]), row(&[3, 4])]).is_ok());
    }

    #[test]
    fn a_row_with_no_cells_is_an_empty_row_and_is_refused() {
        assert_eq!(
            Table::new(vec![row(&[1]), TableRow::new(Vec::new())]),
            Err(TableError::IrregularRowLengths),
            "caught by the width check first when the widths differ"
        );
        assert_eq!(
            Table::new(vec![TableRow::new(Vec::new()), TableRow::new(Vec::new())]),
            Err(TableError::EmptyRow),
            "and by the empty check when they all agree"
        );
    }

    #[test]
    fn a_table_with_no_rows_at_all_is_fine() {
        // The width and empty checks only run when there is a first row.
        assert!(Table::new(Vec::new()).is_ok());
        assert_eq!(Table::new(Vec::new()).unwrap().column_count(), 0);
    }

    #[test]
    fn cell_keys_must_be_unique_across_the_whole_table_not_just_a_row() {
        // The cells are flattened for processing, so by the time keys are
        // matched the rows are gone -- two cells in different rows with one
        // key look like the same cell twice.
        let clashing = vec![
            TableRow::new(vec![TableChild::keyed(1, 7)]),
            TableRow::new(vec![TableChild::keyed(2, 7)]),
        ];
        assert_eq!(Table::new(clashing), Err(TableError::DuplicateCellKey(7)));

        let distinct = vec![
            TableRow::new(vec![TableChild::keyed(1, 7)]),
            TableRow::new(vec![TableChild::keyed(2, 8)]),
        ];
        assert!(Table::new(distinct).is_ok());
    }

    // -- Rows and decorations ----------------------------------------------

    #[test]
    fn a_table_with_no_decorated_rows_keeps_no_decoration_list_at_all() {
        // A list of nulls per row is a list the painter would walk every frame
        // to learn nothing.
        let plain = Table::new(vec![row(&[1]), row(&[2])]).unwrap();
        assert_eq!(plain.row_decorations(), None);

        let decorated = Table::new(vec![row(&[1]).with_decoration(99), row(&[2])]).unwrap();
        assert_eq!(
            decorated.row_decorations(),
            Some([Some(99), None].as_slice())
        );
    }

    #[test]
    fn the_cells_come_out_in_row_major_order() {
        let table = Table::new(vec![row(&[1, 2]), row(&[3, 4])]).unwrap();
        assert_eq!(table.flat_children(), vec![1, 2, 3, 4]);
        assert_eq!(table.column_count(), 2);
    }

    #[test]
    fn a_row_describes_what_is_there_and_says_so_when_nothing_is() {
        assert_eq!(
            TableRow::new(Vec::new()).describe(),
            "TableRow(no children)"
        );
        assert_eq!(row(&[1, 2]).describe(), "TableRow([1, 2])");
        assert_eq!(
            row(&[1]).with_key(7).with_decoration(9).describe(),
            "TableRow([7], 9, [1])"
        );
    }

    // -- The cell ----------------------------------------------------------

    #[test]
    fn a_cell_without_an_alignment_takes_the_tables_default() {
        // Which is why the field is nullable rather than defaulting to Top.
        let inherits = TableCell::new(None);
        assert_eq!(
            inherits.resolve(TableCellVerticalAlignment::Middle),
            TableCellVerticalAlignment::Middle
        );

        let overrides = TableCell::new(Some(TableCellVerticalAlignment::Bottom));
        assert_eq!(
            overrides.resolve(TableCellVerticalAlignment::Middle),
            TableCellVerticalAlignment::Bottom
        );
    }

    #[test]
    fn setting_the_same_alignment_again_does_not_ask_for_a_layout() {
        let cell = TableCell::new(Some(TableCellVerticalAlignment::Bottom));
        let mut current = None;
        assert!(cell.apply_parent_data(&mut current));
        assert!(!cell.apply_parent_data(&mut current), "nothing changed");
    }

    // -- Row matching ------------------------------------------------------

    #[test]
    fn an_unkeyed_row_is_matched_positionally() {
        let mut matcher = TableRowMatcher::new();
        matcher.update(&Table::new(vec![row(&[1]), row(&[2])]).unwrap());
        let first = matcher.rows()[0].children[0];
        let second = matcher.rows()[1].children[0];

        matcher.update(&Table::new(vec![row(&[10]), row(&[20])]).unwrap());
        assert_eq!(matcher.rows()[0].children[0], first);
        assert_eq!(matcher.rows()[1].children[0], second);
    }

    #[test]
    fn a_keyed_row_is_found_wherever_it_moved_to() {
        let mut matcher = TableRowMatcher::new();
        matcher.update(&Table::new(vec![row(&[1]).with_key(7), row(&[2]).with_key(8)]).unwrap());
        let seven = matcher.rows()[0].children[0];
        let eight = matcher.rows()[1].children[0];

        matcher.update(&Table::new(vec![row(&[2]).with_key(8), row(&[1]).with_key(7)]).unwrap());
        assert_eq!(matcher.rows()[0].children[0], eight, "8 came up");
        assert_eq!(matcher.rows()[1].children[0], seven, "and 7 went down");
        assert!(matcher.deactivated().is_empty(), "neither was rebuilt");
    }

    #[test]
    fn inserting_a_keyed_row_does_not_shift_every_unkeyed_rows_identity() {
        // Keyed and unkeyed rows are matched from separate pools, so the
        // unkeyed ones keep lining up with each other.
        let mut matcher = TableRowMatcher::new();
        matcher.update(&Table::new(vec![row(&[1]), row(&[2])]).unwrap());
        let first = matcher.rows()[0].children[0];
        let second = matcher.rows()[1].children[0];

        matcher.update(&Table::new(vec![row(&[9]).with_key(7), row(&[1]), row(&[2])]).unwrap());
        assert_eq!(
            matcher.rows()[1].children[0],
            first,
            "the first unkeyed row is still the first unkeyed element"
        );
        assert_eq!(matcher.rows()[2].children[0], second);
    }

    #[test]
    fn a_row_that_went_away_takes_its_cells_with_it() {
        let mut matcher = TableRowMatcher::new();
        matcher.update(&Table::new(vec![row(&[1]), row(&[2])]).unwrap());
        let second = matcher.rows()[1].children[0];

        matcher.update(&Table::new(vec![row(&[1])]).unwrap());
        assert_eq!(matcher.rows().len(), 1);
        assert!(matcher.deactivated().contains(&second));
    }

    #[test]
    fn a_keyed_row_nothing_claimed_is_deactivated() {
        let mut matcher = TableRowMatcher::new();
        matcher.update(&Table::new(vec![row(&[1]).with_key(7)]).unwrap());
        let seven = matcher.rows()[0].children[0];

        matcher.update(&Table::new(vec![row(&[1]).with_key(8)]).unwrap());
        assert!(matcher.deactivated().contains(&seven));
        assert_ne!(matcher.rows()[0].children[0], seven);
    }

    #[test]
    fn a_narrowed_table_deactivates_the_cells_past_the_new_width() {
        let mut matcher = TableRowMatcher::new();
        matcher.update(&Table::new(vec![row(&[1, 2, 3])]).unwrap());
        let third = matcher.rows()[0].children[2];

        matcher.update(&Table::new(vec![row(&[1, 2])]).unwrap());
        assert_eq!(matcher.rows()[0].children.len(), 2);
        assert!(matcher.deactivated().contains(&third));
    }
}
