// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The studies: whole screens rather than single components.
//!
//! Upstream these are `new_gallery/lib/studies/{rally,shrine,crane,...}`, each
//! a small app with its own theme, navigation and assets. What is ported here
//! is one screen each -- the one that shows what the component library looks
//! like when it is composed rather than catalogued. Their own themes are not:
//! the gallery's theme is what a study demonstrates working under.

use rustflutter::components::theme_of;
use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, component, leaf, many};
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{Align, Container, Empty};

use crate::app::{self, GalleryState, ids};
use crate::catalog::Demo;

/// What the studies remember.
#[derive(Clone, Debug, Default)]
pub struct StudyState {
    /// Shrine's category filter.
    pub filter: usize,
    /// Shrine's cart.
    pub cart: u32,
    /// Crane's tab.
    pub tab: usize,
}

pub fn page(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let body: AnyWidget = match demo.slug {
        "rally" => rally(),
        "shrine" => shrine(state, handle.clone()),
        "crane" => crane(state, handle.clone()),
        other => {
            let slug = other.to_string();
            leaf(move || Text::new(format!("The {slug} study is not written yet.")))
        }
    };

    app::scaffold(demo.title, Some(demo.subtitle), state, handle, body)
}

// -- Rally --------------------------------------------------------------------

/// A finance dashboard: totals, accounts, bills and a budget.
///
/// Every number on screen drives a layout rather than a label -- the bars are
/// sized from their values, so a wrong figure would be visibly wrong.
fn rally() -> AnyWidget {
    // Name, the masked number upstream shows beneath it, and the balance.
    let accounts = [
        ("Checking", "••• 1234", 2215.13_f32),
        ("Home savings", "••• 5678", 8676.88),
        ("Car savings", "••• 9012", 987.48),
    ];
    let bills = [
        ("RedPay Credit", "Jan 29", 45.36_f32),
        ("Rent", "Feb 9", 1200.00),
        ("Water", "Feb 17", 46.10),
    ];

    let total: f32 = accounts.iter().map(|(_, _, amount)| amount).sum();

    let mut children: Vec<AnyWidget> = vec![component(RallyTotal { total })];

    children.push(component(Section::new(
        "Accounts",
        stack(
            accounts
                .iter()
                .map(|(name, note, amount)| {
                    component(RallyRow {
                        name: name.to_string(),
                        note: note.to_string(),
                        amount: *amount,
                        // Share of the total rather than progress towards a
                        // goal: upstream draws a donut here, and a share is
                        // the part of that the bar can honestly show without
                        // inventing a target for each account.
                        fraction: if total > 0.0 { (amount / total).clamp(0.0, 1.0) } else { 0.0 },
                    })
                })
                .collect(),
            10.0,
        ),
    )));

    children.push(component(Section::new(
        "Bills due",
        stack(
            bills
                .iter()
                .map(|(name, due, amount)| {
                    component(
                        ListTile::new(name.to_string())
                            .with_subtitle(format!("Due {due}"))
                            .with_trailing(component(Label::new(format!("${amount:.2}")))),
                    )
                })
                .collect(),
            0.0,
        ),
    )));

    app::scrolling_body(children, 18.0, 16.0)
}

struct RallyTotal {
    total: f32,
}

impl Component for RallyTotal {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let total = self.total;
        let primary = theme.primary;
        let surface = theme.surface;
        let outline = theme.outline;
        let radius = theme.radius;
        let spacing = theme.spacing;
        let muted = theme.muted();

        leaf(move || {
            Container::new()
                .with_color(surface)
                .with_corner_radius(radius)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::all(spacing * 2.25))
                .with_child(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(4.0)
                        .push(Text::new("Total balance").with_style(muted.clone()))
                        .push(
                            Text::new(format!("${total:.2}"))
                                .with_size(32.0)
                                .with_weight(700)
                                .with_color(primary),
                        ),
                )
        })
    }
}

struct RallyRow {
    name: String,
    note: String,
    amount: f32,
    fraction: f32,
}

impl Component for RallyRow {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let name = self.name.clone();
        let note = self.note.clone();
        let amount = self.amount;
        let fraction = self.fraction;
        let body = theme.body();
        let muted = theme.muted();
        let track = theme.surface_variant;
        let fill = theme.primary;

        leaf(move || {
            let bar = Container::new()
                .with_height(6.0)
                .with_color(track)
                .with_corner_radius(3.0)
                .with_child(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        // Two flexible children whose factors are the split:
                        // the bar's fill is a layout, not a drawn rectangle.
                        .push_flex(FlexChild::expanded(
                            Container::new().with_color(fill).with_corner_radius(3.0),
                            ((fraction * 1000.0) as u32).max(1),
                        ))
                        .push_flex(FlexChild::expanded(
                            Empty,
                            (((1.0 - fraction) * 1000.0) as u32).max(1),
                        )),
                );

            Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(6.0)
                .push(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push_flex(FlexChild::expanded(
                            Column::new()
                                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                .with_spacing(2.0)
                                .push(
                                    Text::new(name.clone()).with_style(TextStyle {
                                        font_weight: 700,
                                        ..body.clone()
                                    }),
                                )
                                .push(Text::new(note.clone()).with_style(muted.clone())),
                            1,
                        ))
                        .push(
                            Text::new(format!("${amount:.2}"))
                                .with_style(TextStyle { font_weight: 700, ..body.clone() }),
                        ),
                )
                .push(bar)
        })
    }
}

// -- Shrine -------------------------------------------------------------------

/// A shop: a filter row, a product grid and a cart count.
fn shrine(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let filters = ["All", "Accessories", "Clothing", "Home"];
    let products: &[(&str, &str, f32, usize)] = &[
        ("Vagabond sack", "Accessories", 120.0, 1),
        ("Stella sunglasses", "Accessories", 58.0, 1),
        ("Whitney belt", "Accessories", 35.0, 1),
        ("Garden strand", "Clothing", 98.0, 2),
        ("Strut earrings", "Accessories", 34.0, 1),
        ("Varsity socks", "Clothing", 12.0, 2),
        ("Weave keyring", "Home", 16.0, 3),
        ("Gatsby hat", "Clothing", 40.0, 2),
        ("Shrug bag", "Home", 198.0, 3),
    ];

    let filter = state.study.filter;
    let cart = state.study.cart;

    let mut chips: Vec<AnyWidget> = Vec::new();
    for (index, label) in filters.iter().enumerate() {
        let chip = Chip::new(ids::STUDY_LOCAL + index as u64, *label).selected(filter == index);
        // A fn pointer cannot capture the index, so each arm names its own.
        chips.push(component(match index {
            0 => chip.wired(handle.clone(), |s| s.study.filter = 0),
            1 => chip.wired(handle.clone(), |s| s.study.filter = 1),
            2 => chip.wired(handle.clone(), |s| s.study.filter = 2),
            _ => chip.wired(handle.clone(), |s| s.study.filter = 3),
        }));
    }

    let mut grid = GridList::new(3).with_spacing(10.0).with_aspect_ratio(0.78);
    let mut shown = 0;
    for (name, category, price, group) in products {
        if filter != 0 && *group != filter {
            continue;
        }
        shown += 1;
        grid = grid.push(component(ProductTile {
            name: name.to_string(),
            category: category.to_string(),
            price: *price,
        }));
    }

    let add_handle = handle;
    let body = vec![
        component(ShrineHeader { cart, shown }),
        many(chips, |rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0);
            for child in rendered {
                row = row.push(child);
            }
            Box::new(row)
        }),
        component(grid),
        component(
            Button::new(ids::STUDY_LOCAL + 20, "Add to cart")
                .wired(add_handle, |s| &mut s.pressed, |s| s.study.cart += 1),
        ),
    ];

    app::scrolling_body(body, 14.0, 16.0)
}

struct ShrineHeader {
    cart: u32,
    shown: usize,
}

impl Component for ShrineHeader {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let cart = self.cart;
        let shown = self.shown;
        let title = theme.title();
        let muted = theme.muted();
        let accent = theme.primary;

        leaf(move || {
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push_flex(FlexChild::expanded(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.0)
                        .push(Text::new("Shrine").with_style(title.clone()))
                        .push(
                            Text::new(format!("{shown} items")).with_style(muted.clone()),
                        ),
                    1,
                ))
                .push(
                    Container::new()
                        .with_color(accent.with_alpha(0x2A))
                        .with_corner_radius(14.0)
                        .with_padding(EdgeInsets::symmetric(12.0, 7.0))
                        .with_child(
                            Text::new(format!("Cart {cart}"))
                                .with_size(12.0)
                                .with_weight(700)
                                .with_color(accent),
                        ),
                )
        })
    }
}

struct ProductTile {
    name: String,
    category: String,
    price: f32,
}

impl Component for ProductTile {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let name = self.name.clone();
        let category = self.category.clone();
        let price = self.price;
        let surface = theme.surface_variant;
        let outline = theme.outline;
        let radius = theme.radius;
        let text = theme.text;
        let muted = theme.text_muted;
        let accent = theme.primary;

        leaf(move || {
            Container::new()
                .with_color(surface)
                .with_corner_radius(radius)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::all(10.0))
                .with_child(
                    Column::expanded()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(4.0)
                        // The swatch takes whatever height is left after the
                        // three text lines, which is what makes the tiles line
                        // up whatever their names do.
                        .push_flex(FlexChild::expanded(
                            Container::new()
                                .with_color(accent.with_alpha(0x22))
                                .with_corner_radius(8.0),
                            1,
                        ))
                        .push(
                            Text::new(name.clone())
                                .with_size(11.5)
                                .with_weight(700)
                                .with_color(text),
                        )
                        .push(Text::new(category.clone()).with_size(10.0).with_color(muted))
                        .push(
                            Text::new(format!("${price:.0}"))
                                .with_size(11.0)
                                .with_weight(700)
                                .with_color(accent),
                        ),
                )
        })
    }
}

// -- Crane --------------------------------------------------------------------

/// A travel app: tabs over a list of destinations.
fn crane(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let tab = state.study.tab;
    let flights: &[(&str, &str, &str)] = &[
        ("Aspen", "United States", "$580"),
        ("Big Sur", "United States", "$410"),
        ("Khumbu Valley", "Nepal", "$1,240"),
        ("Machu Picchu", "Peru", "$980"),
    ];
    let sleeps: &[(&str, &str, &str)] = &[
        ("Malé", "Maldives", "$320 / night"),
        ("Beirut", "Lebanon", "$180 / night"),
        ("Tokyo", "Japan", "$240 / night"),
    ];
    let eats: &[(&str, &str, &str)] = &[
        ("Supernova", "Nashville", "2 tables"),
        ("Watercourt", "Lima", "6 tables"),
        ("The Cutting Board", "Nairobi", "4 tables"),
    ];

    let entries: &[(&str, &str, &str)] = match tab {
        1 => sleeps,
        2 => eats,
        _ => flights,
    };

    let mut rows: Vec<AnyWidget> = Vec::new();
    for (index, (place, region, note)) in entries.iter().enumerate() {
        rows.push(component(
            ListTile::new(place.to_string())
                .with_subtitle(region.to_string())
                .with_accent(if index % 2 == 0 {
                    Color::rgb(0x9B, 0x8C, 0xF0)
                } else {
                    Color::rgb(0x4F, 0xC8, 0xB0)
                })
                .with_trailing(component(Label::new(note.to_string()))),
        ));
        if index + 1 < entries.len() {
            rows.push(component(Divider));
        }
    }

    let body = vec![
        component(
            TabBar::new(
                ids::STUDY_LOCAL,
                vec!["Fly".into(), "Sleep".into(), "Eat".into()],
                tab,
            )
            .wired(handle, |s, index| s.study.tab = index),
        ),
        // A key on the list means switching tabs replaces it rather than
        // updating it in place, which is what upstream's cross-fade animates.
        rustflutter::framework::keyed_many(tab as u64, rows, |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        }),
    ];

    app::scrolling_body(body, 12.0, 16.0)
}

// -- Helpers ------------------------------------------------------------------

fn stack(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(column)
    })
}

/// Kept referenced so the imports document the whole surface.
#[allow(dead_code)]
fn unused() -> AnyWidget {
    leaf(|| Align::new(Alignment::CENTER, Empty))
}

#[allow(dead_code)]
const UNUSED_ALIGNMENT: MainAxisAlignment = MainAxisAlignment::Center;
