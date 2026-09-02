// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/rally/home.dart` (flutter/gallery @ d12640d).
//!
//! This is the current aggregate Rally implementation, re-homed from
//! `src/studies/mod.rs` in the M-G split; per-file alignment with upstream is
//! in flight. Upstream's `RallyHomePage` is the whole tabbed dashboard; what
//! is here is one representative screen -- totals, accounts and bills -- as
//! the gallery's one-screen-per-study scope decision allows (PORTING.md).
//! The donut chart is drawn as bars (PORTING.md: "rally donut → bars").

use rustflutter::components::theme_of;
use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, component, leaf, many};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Container, Empty};

use crate::app::{self, GalleryState};

/// The body `studies::page` wraps in the study scaffold.
pub(crate) fn screen(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
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
                        fraction: if total > 0.0 {
                            (amount / total).clamp(0.0, 1.0)
                        } else {
                            0.0
                        },
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

    app::scrolling_body(children, 18.0, 16.0, state, handle)
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
                                .push(Text::new(name.clone()).with_style(TextStyle {
                                    font_weight: 700,
                                    ..body.clone()
                                }))
                                .push(Text::new(note.clone()).with_style(muted.clone())),
                            1,
                        ))
                        .push(Text::new(format!("${amount:.2}")).with_style(TextStyle {
                            font_weight: 700,
                            ..body.clone()
                        })),
                )
                .push(bar)
        })
    }
}

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
