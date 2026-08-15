// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The settings page.
//!
//! Ported from `new_gallery/lib/pages/settings.dart`, which upstream is a
//! sliding panel over the home page. Here it is a route, because a panel that
//! covers part of a screen and takes the taps is a route with a different
//! transition -- and the navigator already has one.
//!
//! Upstream also offers text scale, locale, platform and directionality. Only
//! the theme is real here; the rest are listed as what is missing rather than
//! offered as controls that do nothing.

use rustflutter::components::theme_of;
use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, component, leaf};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::Container;

use crate::app::{self, GalleryState, ids};
use crate::catalog;

pub fn page(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let light = state.light;
    let theme_handle = handle.clone();

    let body = app::scrolling_body(
        vec![
            component(Card::new(app::spread(
                component(
                    ListTile::new("Theme")
                        .with_subtitle(if light { "Light" } else { "Dark" }),
                ),
                component(
                    Switch::new(ids::THEME, light)
                        .wired(theme_handle, |s| s.light = !s.light),
                ),
            ))),
            component(Card::new(component(Inventory))),
            component(Card::new(component(NotYet))),
        ],
        14.0,
        16.0,
    );

    app::scaffold("Settings", None, state, handle, body)
}

/// What the gallery contains, counted from the catalogue rather than written
/// down -- a number that is typed in goes stale the first time a demo is added.
struct Inventory;

impl Component for Inventory {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let rows: Vec<(String, String)> = catalog::CATEGORIES
            .iter()
            .map(|category| {
                (
                    category.title().unwrap_or("Studies").to_string(),
                    format!("{}", catalog::count(*category)),
                )
            })
            .collect();
        let total = catalog::DEMOS.len() + catalog::STUDIES.len();
        let title = theme.title();
        let body = theme.body();
        let muted = theme.muted();
        let accent = theme.primary;

        leaf(move || {
            let mut column = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(8.0)
                .push(Text::new("In this gallery").with_style(title.clone()));
            for (name, count) in &rows {
                column = column.push(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push_flex(FlexChild::expanded(
                            Text::new(name.clone()).with_style(body.clone()),
                            1,
                        ))
                        .push(
                            Text::new(count.clone())
                                .with_size(13.0)
                                .with_weight(700)
                                .with_color(accent),
                        ),
                );
            }
            column.push(
                Text::new(format!("{total} screens in total")).with_style(muted.clone()),
            )
        })
    }
}

/// What upstream's settings page offers that this one does not, and why.
///
/// Listing them beats a row of switches that change nothing: a control that
/// does not work is worse than an absent one, because it has to be tried before
/// it can be ruled out.
struct NotYet;

impl Component for NotYet {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = theme.title();
        let muted = theme.muted();

        let entries = [
            ("Text scale", "Needs a scale factor threaded through TextStyle"),
            ("Locale", "Needs a message catalogue; the strings are inline"),
            ("Text direction", "Needs RTL in the flex and the paragraph"),
            ("Platform", "Only one embedder exists so far"),
            ("Slow motion", "Needs a global multiplier on the ticker"),
        ];

        leaf(move || {
            let mut column = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(9.0)
                .push(Text::new("Not here yet").with_style(title.clone()));
            for (name, reason) in entries {
                column = column.push(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(1.0)
                        .push(Text::new(name).with_size(13.0).with_weight(700))
                        .push(Text::new(reason).with_style(muted.clone())),
                );
            }
            Container::new().with_child(column)
        })
    }
}
