// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/data_table_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `DataTableDemo` is a `PaginatedDataTable` over
//! `_DessertDataSource`: thirty `_Dessert`s, sortable columns, row selection
//! with select-all, a rows-per-page menu and page controls. All of that is
//! here, on a per-demo StatefulComponent (upstream's StatefulWidget shape),
//! because the framework's `DataTable` is a static text table with none of
//! the interaction. The stage dispatch hands this slug no `DemoState`, which
//! suits it: the demo's state is its own, as upstream's is.
//!
//! Divergences, each marked at its site as well:
//!
//! * **no restoration** -- upstream's `RestorationMixin`/`RestorableProperty`
//!   machinery (`restorationId: 'data_table_demo'`) has no counterpart; the
//!   state lives in the component and dies with the page, which is what a
//!   restored-then-left demo amounts to.
//! * **rows-per-page is two buttons** -- upstream's footer is a
//!   `DropdownButton` over `PaginatedDataTable.defaultAvailableRowsPerPage`
//!   ([10, 20, 40]); there is no dropdown that can open from inside a demo
//!   (the overlay slot belongs to modal demos), so the page sizes are two
//!   text buttons, and 40 is dropped because it exceeds the 30-row source.
//! * **the header checkbox is binary** -- upstream's select-all checkbox is
//!   tristate; the framework's `Checkbox` has no indeterminate state, so a
//!   partial selection draws unchecked.
//! * **sort arrows are text** -- upstream draws an animated arrow icon per
//!   sorted column; here the sorted column's label carries a ↑/↓ glyph.
//! * **no `Card` of its own** -- upstream's `PaginatedDataTable` wraps itself
//!   in a `Card`; the stage's own bordered container is that card here.

use rustflutter::framework::{
    component, many, stateful, AnyWidget, BuildContext, StateHandle, StatefulComponent,
};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderBox, RenderFlex,
};
use rustflutter::widgets::{Align, Pointer};

use crate::app::ids;
use crate::data::demos::MATERIAL_ICONS;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// Upstream's `PaginatedDataTable.defaultRowsPerPage`.
const DEFAULT_ROWS_PER_PAGE: usize = 10;
/// The rows-per-page choices: upstream's `defaultAvailableRowsPerPage`
/// ([10, 20, 40]) without the 40 -- see the module header.
const AVAILABLE_ROWS_PER_PAGE: [usize; 2] = [10, 20];

/// Upstream's `_Dessert`.
#[derive(Clone, Debug, PartialEq)]
struct Dessert {
    name: String,
    calories: i32,
    fat: f32,
    carbs: i32,
    protein: f32,
    sodium: i32,
    calcium: i32,
    iron: i32,
    selected: bool,
}

impl Dessert {
    fn new(
        name: String,
        calories: i32,
        fat: f32,
        carbs: i32,
        protein: f32,
        sodium: i32,
        calcium: i32,
        iron: i32,
    ) -> Dessert {
        Dessert {
            name,
            calories,
            fat,
            carbs,
            protein,
            sodium,
            calcium,
            iron,
            selected: false,
        }
    }
}

/// Upstream's `_DessertDataSource` constructor: the thirty rows, in order.
fn desserts() -> Vec<Dessert> {
    let l10n = GalleryLocalizations::en();
    let mut rows = Vec::new();
    let base = [
        (
            l10n.data_table_row_frozen_yogurt().to_string(),
            159,
            6.0,
            24,
            4.0,
            87,
            14,
            1,
        ),
        (
            l10n.data_table_row_ice_cream_sandwich().to_string(),
            237,
            9.0,
            37,
            4.3,
            129,
            8,
            1,
        ),
        (
            l10n.data_table_row_eclair().to_string(),
            262,
            16.0,
            24,
            6.0,
            337,
            6,
            7,
        ),
        (
            l10n.data_table_row_cupcake().to_string(),
            305,
            3.7,
            67,
            4.3,
            413,
            3,
            8,
        ),
        (
            l10n.data_table_row_gingerbread().to_string(),
            356,
            16.0,
            49,
            3.9,
            327,
            7,
            16,
        ),
        (
            l10n.data_table_row_jelly_bean().to_string(),
            375,
            0.0,
            94,
            0.0,
            50,
            0,
            0,
        ),
        (
            l10n.data_table_row_lollipop().to_string(),
            392,
            0.2,
            98,
            0.0,
            38,
            0,
            2,
        ),
        (
            l10n.data_table_row_honeycomb().to_string(),
            408,
            3.2,
            87,
            6.5,
            562,
            0,
            45,
        ),
        (
            l10n.data_table_row_donut().to_string(),
            452,
            25.0,
            51,
            4.9,
            326,
            2,
            22,
        ),
        (
            l10n.data_table_row_apple_pie().to_string(),
            518,
            26.0,
            65,
            7.0,
            54,
            12,
            6,
        ),
    ];
    for (name, calories, fat, carbs, protein, sodium, calcium, iron) in base {
        rows.push(Dessert::new(
            name, calories, fat, carbs, protein, sodium, calcium, iron,
        ));
    }
    // Upstream's `dataTableRowWithSugar(...)` and `dataTableRowWithHoney(...)`
    // rounds: the same ten with their own numbers.
    let with_sugar = [
        (
            l10n.data_table_row_frozen_yogurt().to_string(),
            168,
            6.0,
            26,
            4.0,
            87,
            14,
            1,
        ),
        (
            l10n.data_table_row_ice_cream_sandwich().to_string(),
            246,
            9.0,
            39,
            4.3,
            129,
            8,
            1,
        ),
        (
            l10n.data_table_row_eclair().to_string(),
            271,
            16.0,
            26,
            6.0,
            337,
            6,
            7,
        ),
        (
            l10n.data_table_row_cupcake().to_string(),
            314,
            3.7,
            69,
            4.3,
            413,
            3,
            8,
        ),
        (
            l10n.data_table_row_gingerbread().to_string(),
            345,
            16.0,
            51,
            3.9,
            327,
            7,
            16,
        ),
        (
            l10n.data_table_row_jelly_bean().to_string(),
            364,
            0.0,
            96,
            0.0,
            50,
            0,
            0,
        ),
        (
            l10n.data_table_row_lollipop().to_string(),
            401,
            0.2,
            100,
            0.0,
            38,
            0,
            2,
        ),
        (
            l10n.data_table_row_honeycomb().to_string(),
            417,
            3.2,
            89,
            6.5,
            562,
            0,
            45,
        ),
        (
            l10n.data_table_row_donut().to_string(),
            461,
            25.0,
            53,
            4.9,
            326,
            2,
            22,
        ),
        (
            l10n.data_table_row_apple_pie().to_string(),
            527,
            26.0,
            67,
            7.0,
            54,
            12,
            6,
        ),
    ];
    for (name, calories, fat, carbs, protein, sodium, calcium, iron) in with_sugar {
        rows.push(Dessert::new(
            l10n.data_table_row_with_sugar(&name),
            calories,
            fat,
            carbs,
            protein,
            sodium,
            calcium,
            iron,
        ));
    }
    let with_honey = [
        (
            l10n.data_table_row_frozen_yogurt().to_string(),
            223,
            6.0,
            36,
            4.0,
            87,
            14,
            1,
        ),
        (
            l10n.data_table_row_ice_cream_sandwich().to_string(),
            301,
            9.0,
            49,
            4.3,
            129,
            8,
            1,
        ),
        (
            l10n.data_table_row_eclair().to_string(),
            326,
            16.0,
            36,
            6.0,
            337,
            6,
            7,
        ),
        (
            l10n.data_table_row_cupcake().to_string(),
            369,
            3.7,
            79,
            4.3,
            413,
            3,
            8,
        ),
        (
            l10n.data_table_row_gingerbread().to_string(),
            420,
            16.0,
            61,
            3.9,
            327,
            7,
            16,
        ),
        (
            l10n.data_table_row_jelly_bean().to_string(),
            439,
            0.0,
            106,
            0.0,
            50,
            0,
            0,
        ),
        (
            l10n.data_table_row_lollipop().to_string(),
            456,
            0.2,
            110,
            0.0,
            38,
            0,
            2,
        ),
        (
            l10n.data_table_row_honeycomb().to_string(),
            472,
            3.2,
            99,
            6.5,
            562,
            0,
            45,
        ),
        (
            l10n.data_table_row_donut().to_string(),
            516,
            25.0,
            63,
            4.9,
            326,
            2,
            22,
        ),
        (
            l10n.data_table_row_apple_pie().to_string(),
            582,
            26.0,
            77,
            7.0,
            54,
            12,
            6,
        ),
    ];
    for (name, calories, fat, carbs, protein, sodium, calcium, iron) in with_honey {
        rows.push(Dessert::new(
            l10n.data_table_row_with_honey(&name),
            calories,
            fat,
            carbs,
            protein,
            sodium,
            calcium,
            iron,
        ));
    }
    rows
}

/// The eight column headers, upstream's `columns:` list.
fn column_headers() -> [&'static str; 8] {
    let l10n = GalleryLocalizations::en();
    [
        l10n.data_table_column_dessert(),
        l10n.data_table_column_calories(),
        l10n.data_table_column_fat(),
        l10n.data_table_column_carbs(),
        l10n.data_table_column_protein(),
        l10n.data_table_column_sodium(),
        l10n.data_table_column_calcium(),
        l10n.data_table_column_iron(),
    ]
}

/// Upstream's `_DessertDataSource._sort`: order by column, ascending or not.
fn sort_desserts(desserts: &mut [Dessert], column: usize, ascending: bool) {
    let compare = |a: &Dessert, b: &Dessert| {
        let ordering = match column {
            0 => a.name.cmp(&b.name),
            1 => a.calories.cmp(&b.calories),
            2 => a
                .fat
                .partial_cmp(&b.fat)
                .unwrap_or(std::cmp::Ordering::Equal),
            3 => a.carbs.cmp(&b.carbs),
            4 => a
                .protein
                .partial_cmp(&b.protein)
                .unwrap_or(std::cmp::Ordering::Equal),
            5 => a.sodium.cmp(&b.sodium),
            6 => a.calcium.cmp(&b.calcium),
            _ => a.iron.cmp(&b.iron),
        };
        if ascending {
            ordering
        } else {
            ordering.reverse()
        }
    };
    desserts.sort_by(compare);
}

/// The value a cell shows, upstream's `DataRow` cells: integers as
/// themselves, fat and protein at one decimal, calcium and iron as whole
/// percents (`NumberFormat.decimalPercentPattern(decimalDigits: 0)` of the
/// value over 100, which reads as the value with a `%`).
fn cell_text(dessert: &Dessert, column: usize) -> String {
    match column {
        0 => dessert.name.clone(),
        1 => format!("{}", dessert.calories),
        2 => format!("{:.1}", dessert.fat),
        3 => format!("{}", dessert.carbs),
        4 => format!("{:.1}", dessert.protein),
        5 => format!("{}", dessert.sodium),
        6 => format!("{}%", dessert.calcium),
        _ => format!("{}%", dessert.iron),
    }
}

/// Upstream's `_DataTableDemoState`: the data and what the reader has done
/// to it. (`RestorableInt` and friends are plain fields here -- no
/// restoration, see the module header.)
struct DataTableDemoState {
    desserts: Vec<Dessert>,
    /// Upstream's `_sortColumnIndex`.
    sort_column_index: Option<usize>,
    /// Upstream's `_sortAscending`.
    sort_ascending: bool,
    /// Upstream's `_rowsPerPage`.
    rows_per_page: usize,
    /// Upstream's `_rowIndex`.
    first_row_index: usize,
    /// The held button, for press feedback; the demo's own because no
    /// `GalleryState` reaches this slug's stage.
    pressed: Option<u64>,
}

impl Default for DataTableDemoState {
    fn default() -> DataTableDemoState {
        DataTableDemoState {
            desserts: desserts(),
            sort_column_index: None,
            sort_ascending: true,
            rows_per_page: DEFAULT_ROWS_PER_PAGE,
            first_row_index: 0,
            pressed: None,
        }
    }
}

impl DataTableDemoState {
    /// Upstream's `_DataTableDemoState._sort`: the tap on a column header.
    /// Sorting an unsorted column sorts it ascending; tapping the sorted
    /// column turns it around -- the `onSort(columnIndex, _sortColumnIndex !=
    /// columnIndex || !_sortAscending)` upstream's `DataTable` computes.
    fn sort(&mut self, column: usize) {
        let ascending = self.sort_column_index != Some(column) || !self.sort_ascending;
        sort_desserts(&mut self.desserts, column, ascending);
        self.sort_column_index = Some(column);
        self.sort_ascending = ascending;
    }

    /// Upstream's `DataRow.onSelectChanged`.
    fn toggle_row(&mut self, index: usize) {
        if let Some(dessert) = self.desserts.get_mut(index) {
            dessert.selected = !dessert.selected;
        }
    }

    /// Upstream's `_DessertDataSource._selectAll`.
    fn select_all(&mut self, checked: bool) {
        for dessert in &mut self.desserts {
            dessert.selected = checked;
        }
    }

    /// Upstream's `selectedRowCount`.
    fn selected_count(&self) -> usize {
        self.desserts
            .iter()
            .filter(|dessert| dessert.selected)
            .count()
    }

    /// Upstream's `onRowsPerPageChanged`, with `PaginatedDataTable`'s own
    /// follow-up: the first row is realigned to the new page boundary.
    fn set_rows_per_page(&mut self, rows_per_page: usize) {
        self.rows_per_page = rows_per_page;
        self.first_row_index = self.first_row_index / rows_per_page * rows_per_page;
    }

    /// The highest first-row index: the start of the last page.
    fn last_page_start(&self) -> usize {
        let count = self.desserts.len();
        if count == 0 {
            return 0;
        }
        (count - 1) / self.rows_per_page * self.rows_per_page
    }

    /// Upstream's `handlePrevious` / `handleNext`.
    fn previous_page(&mut self) {
        self.first_row_index = self.first_row_index.saturating_sub(self.rows_per_page);
    }

    fn next_page(&mut self) {
        self.first_row_index =
            (self.first_row_index + self.rows_per_page).min(self.last_page_start());
    }

    /// The rows on the current page, as the half-open range into `desserts`.
    fn page_range(&self) -> (usize, usize) {
        let first = self.first_row_index.min(self.last_page_start());
        (first, (first + self.rows_per_page).min(self.desserts.len()))
    }
}

/// The demo body for the `data-table` slug.
pub(super) fn data_table() -> AnyWidget {
    stateful(DataTableDemo)
}

/// Upstream's `DataTableDemo`.
struct DataTableDemo;

impl StatefulComponent for DataTableDemo {
    type State = DataTableDemoState;

    fn build(
        &self,
        state: &Self::State,
        handle: StateHandle<Self::State>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let headers = column_headers();
        let selected_count = state.selected_count();
        let (first, last) = state.page_range();
        let row_count = state.desserts.len();
        let on_first_page = first == 0;
        let on_last_page = last >= row_count;

        // The rows-per-page buttons: upstream's dropdown, as two text buttons
        // (see the module header). The active one reads pressed. Hand-wired
        // rather than `Button::wired`, whose action is a plain `fn` and could
        // not know its own page size.
        let mut page_size_buttons: Vec<AnyWidget> = Vec::new();
        for (index, size) in AVAILABLE_ROWS_PER_PAGE.iter().enumerate() {
            let id = ids::DEMO_LOCAL + index as u64;
            let size = *size;
            page_size_buttons.push(component(
                Button::new(id, format!("{size}"))
                    .with_style(ButtonStyle::Text)
                    .with_pressed(state.pressed == Some(id) || state.rows_per_page == size)
                    .with_handlers(
                        PointerHandlers::new()
                            .with_tap({
                                let handle = handle.clone();
                                move |_| {
                                    handle.set_state(move |s| s.set_rows_per_page(size));
                                }
                            })
                            .with_press_change({
                                let handle = handle.clone();
                                move |down| {
                                    handle.set_state(move |s| {
                                        s.pressed = if down { Some(id) } else { None };
                                    });
                                }
                            }),
                    ),
            ));
        }

        // Everything below assembles the table: header line, heading row,
        // data rows, footer -- upstream's PaginatedDataTable layout, minus its
        // Card (the stage's container is that card).
        let outline = theme.outline;
        let text_color = theme.text;
        let muted = theme.text_muted;
        let primary = theme.primary;
        // Upstream's selected-row overlay, `onSurface` at 8%.
        let selected_fill = text_color.with_alpha(0x14);
        let sort_column_index = state.sort_column_index;
        let sort_ascending = state.sort_ascending;
        let desserts: Vec<Dessert> = state.desserts[first..last].to_vec();

        // A tap on a heading sorts by it; a tap on a row toggles its
        // selection (upstream's row-wide `onSelectChanged`).
        let sort_handle = handle.clone();
        let toggle_handle = handle.clone();

        many(page_size_buttons, move |mut rendered| {
            let mut table = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

            // The header: the title, or the selection count in its place --
            // upstream's `selectedRowCountTitle` swap.
            let header_text = if selected_count == 0 {
                GalleryLocalizations::en().data_table_header().to_string()
            } else if selected_count == 1 {
                "1 item selected".to_string()
            } else {
                format!("{selected_count} items selected")
            };
            table = table.push(
                Container::new()
                    .with_padding(EdgeInsets::symmetric(12.0, 12.0))
                    .with_child(Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(header_text)
                            .with_size(16.0)
                            .with_weight(700)
                            .with_color(text_color),
                    )),
            );

            // The heading row: the select-all checkbox, then the eight
            // tappable labels, the sorted one carrying its arrow.
            let all_selected = selected_count == row_count && row_count > 0;
            let mut heading = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            heading = heading.push({
                // Upstream's `onSelectAll: _dessertsDataSource!._selectAll`.
                let select_all = sort_handle.clone();
                let handlers = PointerHandlers::new().with_tap(move |_| {
                    select_all.set_state(move |s| s.select_all(!all_selected));
                });
                Pointer::new(
                    ids::DEMO_LOCAL + 20,
                    Container::new().with_width(40.0).with_child(Align::new(
                        Alignment::CENTER,
                        // Drawn by hand rather than with `Checkbox`: the row
                        // checkboxes and this one share the look, and their
                        // taps are the row's and the header's regions.
                        checkbox_mark(all_selected, primary, text_color),
                    )),
                )
                .with_handlers(handlers)
            });
            for (column, header) in headers.iter().enumerate() {
                let label = if sort_column_index == Some(column) {
                    // The sort arrow, upstream's animated icon as a glyph.
                    format!(
                        "{header} {}",
                        if sort_ascending {
                            "\u{2191}"
                        } else {
                            "\u{2193}"
                        }
                    )
                } else {
                    header.to_string()
                };
                let id = ids::DEMO_LOCAL + 10 + column as u64;
                let sort = sort_handle.clone();
                let handlers = PointerHandlers::new().with_tap(move |_| {
                    sort.set_state(move |s| s.sort(column));
                });
                heading = heading.push_flex(FlexChild::expanded(
                    Pointer::new(
                        id,
                        Container::new()
                            .with_padding(EdgeInsets::symmetric(4.0, 10.0))
                            .with_child(Align::new(
                                if column == 0 {
                                    Alignment::CENTER_LEFT
                                } else {
                                    // Upstream's `numeric: true`.
                                    Alignment::CENTER_RIGHT
                                },
                                Text::new(label)
                                    .with_size(11.0)
                                    .with_weight(700)
                                    .with_color(muted),
                            )),
                    )
                    .with_handlers(handlers),
                    if column == 0 { 3 } else { 2 },
                ));
            }
            table = table.push(heading);
            table = table.push(Container::new().with_height(1.0).with_color(outline));

            // The page's rows.
            for (position, dessert) in desserts.iter().enumerate() {
                let index = first + position;
                let id = ids::DEMO_LOCAL + 40 + index as u64;
                let selected = dessert.selected;
                let toggle = toggle_handle.clone();
                let handlers = PointerHandlers::new().with_tap(move |_| {
                    toggle.set_state(move |s| s.toggle_row(index));
                });
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                row = row.push(Container::new().with_width(40.0).with_child(Align::new(
                    Alignment::CENTER,
                    checkbox_mark(selected, primary, text_color),
                )));
                for column in 0..headers.len() {
                    row = row.push_flex(FlexChild::expanded(
                        Container::new()
                            .with_padding(EdgeInsets::symmetric(4.0, 10.0))
                            .with_child(Align::new(
                                if column == 0 {
                                    Alignment::CENTER_LEFT
                                } else {
                                    Alignment::CENTER_RIGHT
                                },
                                Text::new(cell_text(dessert, column))
                                    .with_size(12.0)
                                    .with_color(text_color),
                            )),
                        if column == 0 { 3 } else { 2 },
                    ));
                }
                table = table.push(
                    Pointer::new(
                        id,
                        Container::new()
                            .with_color(if selected {
                                selected_fill
                            } else {
                                Color::TRANSPARENT
                            })
                            .with_child(row),
                    )
                    .with_handlers(handlers),
                );
                table = table.push(
                    Container::new()
                        .with_height(1.0)
                        .with_color(outline.with_alpha(0x60)),
                );
            }

            // The footer: rows per page on the left, the page position and
            // turners on the right -- upstream's footer row.
            let mut footer = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            footer = footer.push(
                Container::new()
                    .with_padding(EdgeInsets::only(12.0, 0.0, 4.0, 0.0))
                    .with_child(
                        Text::new("Rows per page:")
                            .with_size(12.0)
                            .with_color(muted),
                    ),
            );
            for button in rendered.drain(..) {
                footer = footer.push(button);
            }
            footer = footer.push_flex(FlexChild::expanded(rustflutter::widgets::Empty, 1));
            footer = footer.push(
                Text::new(format!("{}\u{2013}{} of {}", first + 1, last, row_count))
                    .with_size(12.0)
                    .with_color(muted),
            );
            footer = footer.push(page_turner(
                ids::DEMO_LOCAL + 30,
                "\u{e5e0}",
                !on_first_page,
                {
                    let handle = sort_handle.clone();
                    move |_| {
                        handle.set_state(|s| s.previous_page());
                    }
                },
                muted,
                text_color,
            ));
            footer = footer.push(page_turner(
                ids::DEMO_LOCAL + 31,
                "\u{e5e1}",
                !on_last_page,
                {
                    let handle = toggle_handle.clone();
                    move |_| {
                        handle.set_state(|s| s.next_page());
                    }
                },
                muted,
                text_color,
            ));
            table = table.push(footer);

            Box::new(table)
        })
    }
}

/// The little ticked-or-not square, the framework `Checkbox`'s own look (18
/// with 2-radius corners, a border in primary when enabled), drawn here
/// because the taps belong to the row and heading regions around it.
fn checkbox_mark(checked: bool, primary: Color, tick: Color) -> impl RenderBox + 'static {
    let mark = if checked {
        Container::new()
            .with_size(10.0, 5.0)
            .with_border(2.0, tick)
            .with_corner_radius(1.0)
    } else {
        Container::new().with_size(10.0, 5.0)
    };
    Container::new()
        .with_size(18.0, 18.0)
        .with_color(if checked { primary } else { Color::TRANSPARENT })
        .with_corner_radius(2.0)
        .with_border(2.0, primary)
        .with_child(rustflutter::widgets::Center::new(mark))
}

/// Upstream's footer arrow buttons (`Icons.arrow_back_ios` /
/// `arrow_forward_ios`), icon-sized; at the first or last page they draw
/// muted and do nothing, as upstream's disabled `IconButton`s do.
fn page_turner(
    id: u64,
    glyph: &'static str,
    enabled: bool,
    on_tap: impl Fn(rustflutter::gestures::TapEvent) + 'static,
    muted: Color,
    color: Color,
) -> impl RenderBox + 'static {
    let handlers = if enabled {
        PointerHandlers::new().with_tap(on_tap)
    } else {
        PointerHandlers::new()
    };
    Pointer::new(
        id,
        Container::new()
            .with_size(36.0, 36.0)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(glyph)
                    .with_font_family(MATERIAL_ICONS)
                    .with_size(16.0)
                    .with_color(if enabled {
                        color
                    } else {
                        muted.with_alpha(0x80)
                    }),
            )),
    )
    .with_handlers(handlers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_source_has_upstreams_thirty_rows() {
        let rows = desserts();
        assert_eq!(rows.len(), 30);
        assert_eq!(rows[0].name, "Frozen yogurt");
        assert_eq!(rows[0].calories, 159);
        assert_eq!(rows[10].name, "Frozen yogurt with sugar");
        assert_eq!(rows[20].name, "Frozen yogurt with honey");
        assert_eq!(rows[29].name, "Apple pie with honey");
        assert_eq!(rows[29].calories, 582);
    }

    #[test]
    fn sorting_orders_by_the_column() {
        let mut state = DataTableDemoState::default();
        // Calories ascending: frozen yogurt's 159 leads; descending, apple
        // pie with honey's 582 does.
        state.sort(1);
        assert_eq!(state.sort_column_index, Some(1));
        assert!(state.sort_ascending);
        assert_eq!(state.desserts[0].name, "Frozen yogurt");
        state.sort(1);
        assert!(!state.sort_ascending);
        assert_eq!(state.desserts[0].name, "Apple pie with honey");
        // A fresh column starts ascending, upstream's `_sortColumnIndex !=
        // columnIndex ||` branch.
        state.sort(5);
        assert!(state.sort_ascending);
        assert_eq!(state.desserts[0].sodium, 38);
        // Names sort alphabetically.
        state.sort(0);
        assert_eq!(state.desserts[0].name, "Apple pie");
    }

    #[test]
    fn selection_counts_and_selects_all() {
        let mut state = DataTableDemoState::default();
        assert_eq!(state.selected_count(), 0);
        state.toggle_row(2);
        state.toggle_row(2);
        assert_eq!(state.selected_count(), 0, "a second tap toggles back");
        state.toggle_row(0);
        state.select_all(true);
        assert_eq!(state.selected_count(), 30);
        state.select_all(false);
        assert_eq!(state.selected_count(), 0);
    }

    #[test]
    fn paging_walks_and_realigns() {
        let mut state = DataTableDemoState::default();
        assert_eq!(state.page_range(), (0, 10));
        state.next_page();
        state.next_page();
        // Thirty rows at ten a page: three full pages.
        assert_eq!(state.page_range(), (20, 30));
        state.next_page();
        assert_eq!(state.page_range(), (20, 30), "the last page is the end");
        state.previous_page();
        state.previous_page();
        state.previous_page();
        assert_eq!(state.page_range(), (0, 10), "the first page is the start");
        // Changing the page size realigns the first row to a page boundary,
        // upstream's `PaginatedDataTable` behaviour.
        state.next_page();
        state.set_rows_per_page(20);
        assert_eq!(state.first_row_index, 0);
        assert_eq!(state.page_range(), (0, 20));
        state.next_page();
        assert_eq!(state.page_range(), (20, 30), "the last page can be short");
    }

    #[test]
    fn the_cells_format_the_way_getrow_does() {
        let rows = desserts();
        let yogurt = &rows[0];
        assert_eq!(cell_text(yogurt, 1), "159");
        assert_eq!(cell_text(yogurt, 2), "6.0", "toStringAsFixed(1)");
        assert_eq!(cell_text(yogurt, 4), "4.0");
        assert_eq!(cell_text(yogurt, 6), "14%", "decimalPercentPattern of 0.14");
        assert_eq!(cell_text(&rows[8], 7), "22%");
    }
}
