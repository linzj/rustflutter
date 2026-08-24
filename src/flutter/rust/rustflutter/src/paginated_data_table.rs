//! Port of `material/paginated_data_table.dart`.
//!
//! Nine asserts in one constructor, and between them they use most of the
//! shapes an argument check can have.

/// Why a paginated data table's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaginatedDataTableError {
    /// `assert(actions == null || (header != null))`.
    ActionsWithoutAHeader,
    NoColumns,
    SortColumnOutOfRange,
    MaxRowHeightBelowMin,
    /// The deprecated fixed height mixed with the range that replaced it.
    FixedHeightMixedWithRange,
    NoRowsPerPage,
    NegativeDividerThickness,
    /// The current page size is not one of the sizes on offer.
    RowsPerPageNotOffered,
    /// A controller supplied alongside `primary: true`.
    TwoWaysToTheSameController,
}

/// Upstream `PaginatedDataTable`.
#[derive(Clone, Debug, PartialEq)]
pub struct PaginatedDataTable {
    pub column_count: usize,
    pub has_header: bool,
    pub has_actions: bool,
    pub sort_column_index: Option<usize>,
    /// Upstream's deprecated single height. See [`PaginatedDataTable::validate`].
    pub data_row_height: Option<f32>,
    pub data_row_min_height: Option<f32>,
    pub data_row_max_height: Option<f32>,
    pub rows_per_page: usize,
    pub available_rows_per_page: Vec<usize>,
    /// Whether the page size can be changed from the footer's dropdown.
    pub has_on_rows_per_page_changed: bool,
    pub divider_thickness: Option<f32>,
    pub has_controller: bool,
    pub primary: Option<bool>,
}

impl PaginatedDataTable {
    /// Upstream's `defaultRowsPerPage`.
    pub const DEFAULT_ROWS_PER_PAGE: usize = 10;

    pub fn new(column_count: usize) -> PaginatedDataTable {
        PaginatedDataTable {
            column_count,
            has_header: false,
            has_actions: false,
            sort_column_index: None,
            data_row_height: None,
            data_row_min_height: None,
            data_row_max_height: None,
            rows_per_page: PaginatedDataTable::DEFAULT_ROWS_PER_PAGE,
            available_rows_per_page: vec![10, 20, 50, 100],
            has_on_rows_per_page_changed: false,
            divider_thickness: None,
            has_controller: false,
            primary: None,
        }
    }

    /// Upstream's nine constructor asserts, in order.
    ///
    /// Read together they are a small catalogue of the shapes an argument check
    /// comes in, and three of them are shapes worth separating:
    ///
    /// * `assert(actions == null || (header != null))` is an **implication**,
    ///   not an exclusion: actions require a header, a header does not require
    ///   actions. The actions are drawn *in* the header row, so without one
    ///   there is nowhere to put them.
    /// * The `availableRowsPerPage` check is **conditional** -- it only applies
    ///   `if (onRowsPerPageChanged != null)`. A page size that cannot be changed
    ///   may be any number at all; one that can must be a number the dropdown is
    ///   able to show, because it has to display its own current value.
    /// * `assert(columns.isNotEmpty)` is worth contrasting with
    ///   [`crate::tabs::TabController`], which permits a length of zero and then
    ///   carries `length == 0 ||` through every later check as an escape hatch.
    ///   **Excluding the degenerate case once, up front, is what lets the range
    ///   check below it be a plain range check.**
    pub fn validate(&self) -> Result<(), PaginatedDataTableError> {
        if self.has_actions && !self.has_header {
            return Err(PaginatedDataTableError::ActionsWithoutAHeader);
        }
        if self.column_count == 0 {
            return Err(PaginatedDataTableError::NoColumns);
        }
        if self
            .sort_column_index
            .is_some_and(|index| index >= self.column_count)
        {
            return Err(PaginatedDataTableError::SortColumnOutOfRange);
        }
        if let (Some(min), Some(max)) = (self.data_row_min_height, self.data_row_max_height) {
            if max < min {
                return Err(PaginatedDataTableError::MaxRowHeightBelowMin);
            }
        }
        if self.data_row_height.is_some()
            && (self.data_row_min_height.is_some() || self.data_row_max_height.is_some())
        {
            return Err(PaginatedDataTableError::FixedHeightMixedWithRange);
        }
        if self.rows_per_page == 0 {
            return Err(PaginatedDataTableError::NoRowsPerPage);
        }
        if self.divider_thickness.is_some_and(|value| value < 0.0) {
            return Err(PaginatedDataTableError::NegativeDividerThickness);
        }
        if self.has_on_rows_per_page_changed
            && !self.available_rows_per_page.contains(&self.rows_per_page)
        {
            return Err(PaginatedDataTableError::RowsPerPageNotOffered);
        }
        if self.has_controller && self.primary.unwrap_or(false) {
            return Err(PaginatedDataTableError::TwoWaysToTheSameController);
        }
        Ok(())
    }

    /// Upstream's two field initialisers, which run **after** the asserts:
    ///
    /// ```dart
    /// dataRowMinHeight = dataRowHeight ?? dataRowMinHeight,
    /// dataRowMaxHeight = dataRowHeight ?? dataRowMaxHeight,
    /// ```
    ///
    /// The assert forbids giving the deprecated single height alongside the
    /// range that replaced it, and then these collapse the single height *into*
    /// that range. **A fixed height survives as a degenerate interval**, min
    /// equal to max, and everything downstream only ever sees the pair.
    ///
    /// Which is also why the `max >= min` assert above can be read plainly: it
    /// judges what the caller wrote, and the values these produce are always
    /// equal.
    pub fn resolved_row_heights(&self) -> (Option<f32>, Option<f32>) {
        (
            self.data_row_height.or(self.data_row_min_height),
            self.data_row_height.or(self.data_row_max_height),
        )
    }
}

/// Upstream `PaginatedDataTableState`.
#[derive(Clone, Debug, PartialEq)]
pub struct PaginatedDataTableState {
    pub first_row_index: usize,
    pub rows_per_page: usize,
    pub row_count: usize,
    /// Upstream's `showEmptyRows`: whether the last page is padded out to full
    /// height.
    pub show_empty_rows: bool,
    /// Upstream's `_rowCountApproximate`.
    pub row_count_approximate: bool,
}

impl PaginatedDataTableState {
    pub fn new(row_count: usize, rows_per_page: usize) -> PaginatedDataTableState {
        PaginatedDataTableState {
            first_row_index: 0,
            rows_per_page,
            row_count,
            show_empty_rows: true,
            row_count_approximate: false,
        }
    }

    /// Upstream's `pageTo`, and the name is doing something quietly:
    ///
    /// ```dart
    /// _firstRowIndex = (rowIndex ~/ rowsPerPage) * rowsPerPage;
    /// ```
    ///
    /// **It takes a row index and gives you the page that row is on.** You
    /// cannot land halfway through a page by asking for row 7 -- the integer
    /// division rounds you back to the top of the page containing it.
    ///
    /// Returns whether `onPageChanged` fires, which upstream gates on the
    /// *snapped* index having moved. Paging to a different row of the page you
    /// are already looking at is silent.
    pub fn page_to(&mut self, row_index: usize) -> bool {
        let old = self.first_row_index;
        self.first_row_index = (row_index / self.rows_per_page) * self.rows_per_page;
        old != self.first_row_index
    }

    /// Upstream's `_handleNext`.
    pub fn next(&mut self) -> bool {
        self.page_to(self.first_row_index + self.rows_per_page)
    }

    /// Upstream's `_handlePrevious`, which clamps at zero via
    /// `math.max(_firstRowIndex - widget.rowsPerPage, 0)`.
    pub fn previous(&mut self) -> bool {
        let target = self.first_row_index.saturating_sub(self.rows_per_page);
        self.page_to(target)
    }

    /// Whether the forward button is live. Upstream's condition is
    /// `_firstRowIndex + widget.rowsPerPage < _rowCount || _rowCountApproximate`
    /// -- **an approximate count keeps the button enabled even when the
    /// arithmetic says there is nothing after this page**, because a row source
    /// that does not know its own length may still have more.
    pub fn can_go_next(&self) -> bool {
        self.first_row_index + self.rows_per_page < self.row_count || self.row_count_approximate
    }

    pub fn can_go_previous(&self) -> bool {
        self.first_row_index > 0
    }

    /// How many rows this page draws, before any empty padding.
    /// Upstream's footer line, from
    /// [`crate::material_app::DefaultMaterialLocalizations::page_rows_info_title`].
    ///
    /// The numbers a reader sees are **one-based and inclusive** -- "1–10 of
    /// 53" for the first page of ten -- where everything this state machine
    /// counts with is zero-based and half-open. The conversion happens here,
    /// at the boundary between the two conventions, which is the only place it
    /// can happen once.
    ///
    /// An empty table is the case that does not simply fall out of the
    /// arithmetic: there is no zeroth row to start from, so upstream's own
    /// footer would read "1–0 of 0". Left as upstream leaves it, because a
    /// table with no rows shows no footer to read it in.
    /// `row_count_approximate` is read from the state rather than passed in,
    /// because it is a fact about the source and not about the moment: a query
    /// that has not finished counting is still uncounted on the next page.
    pub fn page_rows_info(&self) -> String {
        crate::material_app::DefaultMaterialLocalizations::page_rows_info_title(
            self.first_row_index + 1,
            self.first_row_index + self.rows_on_this_page(),
            self.row_count,
            self.row_count_approximate,
        )
    }

    /// Upstream's `rowsPerPageTitle`, which the footer puts before the
    /// dropdown.
    pub fn rows_per_page_title(&self) -> &'static str {
        crate::material_app::DefaultMaterialLocalizations::ROWS_PER_PAGE_TITLE
    }

    pub fn rows_on_this_page(&self) -> usize {
        self.row_count
            .saturating_sub(self.first_row_index)
            .min(self.rows_per_page)
    }

    /// How many blank rows are added to keep the card from changing height,
    /// which `showEmptyRows` turns off.
    pub fn empty_rows(&self) -> usize {
        if !self.show_empty_rows {
            return 0;
        }
        self.rows_per_page - self.rows_on_this_page()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> PaginatedDataTable {
        PaginatedDataTable::new(4)
    }

    // -- Three shapes of check, in one constructor ---------------------------------

    #[test]
    fn actions_need_a_header_but_a_header_needs_no_actions() {
        // An implication, not an exclusion: the actions are drawn in the header
        // row, so without one there is nowhere to put them.
        let mut plain = table();
        assert_eq!(plain.validate(), Ok(()), "neither is fine");

        plain.has_header = true;
        assert_eq!(plain.validate(), Ok(()), "a header alone is fine");

        plain.has_header = false;
        plain.has_actions = true;
        assert_eq!(
            plain.validate(),
            Err(PaginatedDataTableError::ActionsWithoutAHeader)
        );

        plain.has_header = true;
        assert_eq!(plain.validate(), Ok(()), "and both together are fine");
    }

    #[test]
    fn the_page_size_must_be_one_the_dropdown_can_show_only_if_there_is_a_dropdown() {
        let mut fixed = table();
        fixed.rows_per_page = 7;
        assert!(!fixed.available_rows_per_page.contains(&7));
        assert_eq!(
            fixed.validate(),
            Ok(()),
            "a page size nobody can change may be any number"
        );

        fixed.has_on_rows_per_page_changed = true;
        assert_eq!(
            fixed.validate(),
            Err(PaginatedDataTableError::RowsPerPageNotOffered),
            "but a dropdown has to be able to display its own value"
        );

        fixed.rows_per_page = 20;
        assert_eq!(fixed.validate(), Ok(()));
    }

    #[test]
    fn excluding_the_empty_case_once_is_what_keeps_the_range_check_plain() {
        // TabController takes the other road: it allows a length of zero and
        // then carries `length == 0 ||` through every later assert.
        let empty = PaginatedDataTable::new(0);
        assert_eq!(empty.validate(), Err(PaginatedDataTableError::NoColumns));

        let mut sorted = table();
        sorted.sort_column_index = Some(3);
        assert_eq!(sorted.validate(), Ok(()));
        sorted.sort_column_index = Some(4);
        assert_eq!(
            sorted.validate(),
            Err(PaginatedDataTableError::SortColumnOutOfRange),
            "no escape hatch needed, because there is always at least one column"
        );
    }

    // -- The deprecated parameter as a degenerate range -----------------------------

    #[test]
    fn a_fixed_row_height_becomes_a_range_with_no_width() {
        let mut fixed = table();
        fixed.data_row_height = Some(50.0);
        assert_eq!(fixed.validate(), Ok(()));
        assert_eq!(fixed.resolved_row_heights(), (Some(50.0), Some(50.0)));
    }

    #[test]
    fn but_saying_it_both_ways_at_once_is_refused() {
        let mut mixed = table();
        mixed.data_row_height = Some(50.0);
        mixed.data_row_min_height = Some(40.0);
        assert_eq!(
            mixed.validate(),
            Err(PaginatedDataTableError::FixedHeightMixedWithRange)
        );

        mixed.data_row_height = None;
        mixed.data_row_max_height = Some(80.0);
        assert_eq!(mixed.validate(), Ok(()), "the range alone is the new way");
        assert_eq!(mixed.resolved_row_heights(), (Some(40.0), Some(80.0)));
    }

    #[test]
    fn a_range_has_to_face_the_right_way() {
        let mut backwards = table();
        backwards.data_row_min_height = Some(80.0);
        backwards.data_row_max_height = Some(40.0);
        assert_eq!(
            backwards.validate(),
            Err(PaginatedDataTableError::MaxRowHeightBelowMin)
        );
    }

    #[test]
    fn two_ways_to_reach_the_same_scroll_controller_is_one_too_many() {
        let mut both = table();
        both.has_controller = true;
        assert_eq!(both.validate(), Ok(()));
        both.primary = Some(true);
        assert_eq!(
            both.validate(),
            Err(PaginatedDataTableError::TwoWaysToTheSameController)
        );
        both.primary = Some(false);
        assert_eq!(both.validate(), Ok(()));
    }

    #[test]
    fn a_page_has_to_hold_at_least_one_row() {
        let mut none = table();
        none.rows_per_page = 0;
        assert_eq!(none.validate(), Err(PaginatedDataTableError::NoRowsPerPage));
    }

    #[test]
    fn a_divider_may_be_invisible_but_not_negative() {
        let mut table = table();
        table.divider_thickness = Some(0.0);
        assert_eq!(table.validate(), Ok(()));
        table.divider_thickness = Some(-1.0);
        assert_eq!(
            table.validate(),
            Err(PaginatedDataTableError::NegativeDividerThickness)
        );
    }

    // -- Paging ---------------------------------------------------------------------

    #[test]
    fn the_last_page_is_short_and_padded_out_to_keep_the_card_still() {
        let state = PaginatedDataTableState::new(23, 10);
        let mut last = state.clone();
        last.first_row_index = 20;

        assert_eq!(last.rows_on_this_page(), 3);
        assert_eq!(last.empty_rows(), 7, "so the card does not change height");
        assert_eq!(state.rows_on_this_page(), 10);
        assert_eq!(state.empty_rows(), 0);
    }

    #[test]
    fn turning_the_padding_off_lets_the_card_shrink() {
        let mut last = PaginatedDataTableState::new(23, 10);
        last.first_row_index = 20;
        last.show_empty_rows = false;
        assert_eq!(last.empty_rows(), 0);
        assert_eq!(last.rows_on_this_page(), 3, "and the rows are unchanged");
    }

    #[test]
    fn a_row_source_that_does_not_know_its_length_keeps_the_forward_button_live() {
        // The arithmetic says this is the last page.
        let mut state = PaginatedDataTableState::new(23, 10);
        state.first_row_index = 20;
        assert!(!state.can_go_next());

        state.row_count_approximate = true;
        assert!(
            state.can_go_next(),
            "but an approximate count may still have more"
        );
    }

    #[test]
    fn paging_to_a_row_pages_to_the_page_that_row_is_on() {
        let mut state = PaginatedDataTableState::new(100, 10);
        assert!(state.page_to(37));
        assert_eq!(state.first_row_index, 30, "not 37");

        assert!(
            !state.page_to(35),
            "already on that page, so nothing is said"
        );
        assert_eq!(state.first_row_index, 30);

        assert!(state.page_to(30 + 10));
        assert_eq!(state.first_row_index, 40);
    }

    #[test]
    fn paging_backwards_stops_at_the_first_row_rather_than_going_under() {
        let mut state = PaginatedDataTableState::new(23, 10);
        assert!(state.next());
        assert_eq!(state.first_row_index, 10);
        assert!(state.can_go_previous());

        assert!(state.previous());
        assert_eq!(state.first_row_index, 0);
        assert!(!state.can_go_previous());

        assert!(
            !state.previous(),
            "and again moves nothing and says nothing"
        );
        assert_eq!(state.first_row_index, 0);
    }
}

#[cfg(test)]
mod row_height_direction_tests {
    use super::*;

    #[test]
    fn the_exact_height_wins_over_both_bounds() {
        // `dataRowHeight ?? dataRowMinHeight` and `?? dataRowMaxHeight`. With
        // only one of the two set on either line, which side is asked first
        // cannot be seen -- `tools/order_sweep.py` found both by swapping them.
        let mut table = PaginatedDataTable::new(0);
        table.data_row_height = Some(50.0);
        table.data_row_min_height = Some(10.0);
        table.data_row_max_height = Some(90.0);
        assert_eq!(table.resolved_row_heights(), (Some(50.0), Some(50.0)));
    }

    #[test]
    fn without_an_exact_height_the_two_bounds_answer_separately() {
        let mut table = PaginatedDataTable::new(0);
        table.data_row_min_height = Some(10.0);
        table.data_row_max_height = Some(90.0);
        assert_eq!(table.resolved_row_heights(), (Some(10.0), Some(90.0)));
    }

    #[test]
    fn an_exact_height_makes_the_two_equal_which_is_what_the_assert_relies_on() {
        let mut table = PaginatedDataTable::new(0);
        table.data_row_height = Some(50.0);
        let (min, max) = table.resolved_row_heights();
        assert_eq!(min, max);
    }
}

#[cfg(test)]
mod footer_wording_tests {
    use super::PaginatedDataTableState;
    use crate::material_app::DefaultMaterialLocalizations as L10n;

    fn table(row_count: usize, rows_per_page: usize) -> PaginatedDataTableState {
        PaginatedDataTableState::new(row_count, rows_per_page)
    }

    #[test]
    fn the_footer_counts_from_one_where_the_state_counts_from_zero() {
        // "1 to 10 of 53" for the first page of ten, while first_row_index is
        // 0 and the range is half-open. The conversion happens once, here.
        let first = table(53, 10);
        assert_eq!(first.first_row_index, 0);
        assert_eq!(first.page_rows_info(), "1\u{2013}10 of 53");
    }

    #[test]
    fn a_short_last_page_says_how_short_it_is() {
        let mut table = table(53, 10);
        // Guarded by can_go_next, which is what the forward button is guarded
        // by. `next` itself does not clamp -- upstream's `pageTo` snaps to a
        // page boundary and nothing more, so a caller that ignores the guard
        // walks off the end into empty pages, on both sides.
        while table.can_go_next() {
            table.next();
        }
        assert_eq!(table.page_rows_info(), "51\u{2013}53 of 53");
    }

    #[test]
    fn the_separator_is_an_en_dash_and_not_a_hyphen() {
        // U+2013. It is a range between two numbers, which is what an en dash
        // is for, and it is exactly what a paraphrase loses -- a test that
        // only checked the numbers would not notice.
        let info = table(53, 10).page_rows_info();
        assert!(info.contains('\u{2013}'), "{info}");
        assert!(
            !info.contains('-'),
            "a hyphen would read as a compound rather than a span: {info}"
        );
    }

    #[test]
    fn a_source_still_counting_claims_less() {
        // "of about 300" claims less than "of 300" does, and the flag is a
        // fact about the source rather than about the page.
        let mut table = table(300, 10);
        table.row_count_approximate = true;
        assert_eq!(table.page_rows_info(), "1\u{2013}10 of about 300");
        table.next();
        assert_eq!(
            table.page_rows_info(),
            "11\u{2013}20 of about 300",
            "still uncounted on the next page"
        );
    }

    #[test]
    fn the_rows_per_page_title_keeps_its_colon() {
        // Part of the string rather than something the footer adds, so a
        // language that puts it elsewhere changes the string and not the
        // widget.
        assert_eq!(table(53, 10).rows_per_page_title(), "Rows per page:");
    }

    #[test]
    fn a_selection_is_counted_in_three_cases_and_not_two() {
        // English has a singular, and a table that says "1 items" in its
        // header says it every time anyone ticks a row.
        assert_eq!(L10n::selected_row_count_title(0), "No items selected");
        assert_eq!(L10n::selected_row_count_title(1), "1 item selected");
        assert_eq!(L10n::selected_row_count_title(2), "2 items selected");
        assert_eq!(L10n::selected_row_count_title(53), "53 items selected");
    }

    #[test]
    fn and_none_is_a_word_rather_than_a_zero() {
        // "No items selected", not "0 items selected".
        let none = L10n::selected_row_count_title(0);
        assert!(!none.contains('0'), "{none}");
    }
}
