// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The home page.
//!
//! Ported from `new_gallery/lib/pages/home.dart`, minus the carousel's parallax
//! and the deferred loading. What remains is its shape: a header, a row of
//! study cards, and category sections that open to reveal their demos.
//!
//! Upstream the categories are an animated expansion with a rotating chevron.
//! Here they are a tap that changes a list -- the animation would need the
//! expanding list to be laid out at two sizes at once, which is a render object
//! rather than a composition and is worth doing when something else needs it
//! too.

use rustflutter::components::theme_of;
use rustflutter::framework::{AnyWidget, StateHandle, component, leaf, many};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{Align, Container, Empty, Pointer};

use crate::app::{self, GalleryState, ids};
use crate::catalog::{self, Category};

pub fn page(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let mut sections: Vec<AnyWidget> = vec![component(Header)];

    for (index, category) in catalog::CATEGORIES.iter().enumerate() {
        sections.push(component(CategorySection {
            category: *category,
            index,
            expanded: state.is_expanded(*category),
            pressed: state.pressed,
            handle: handle.clone(),
        }));
    }

    app::scaffold(
        "rustflutter Gallery",
        Some("A port of Flutter Gallery"),
        state,
        handle,
        app::scrolling_body(sections, 16.0, 16.0),
    )
}

/// The block of text at the top, upstream's `_GalleryHeader`.
struct Header;

impl Component for Header {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = theme.title();
        let muted = theme.muted();
        let primary = theme.primary;
        let spacing = theme.spacing;

        leaf(move || {
            Container::new()
                .with_padding(EdgeInsets::only(0.0, spacing, 0.0, spacing))
                .with_child(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(6.0)
                        .push(
                            Text::new("Design beautiful apps")
                                .with_style(TextStyle { font_size: 26.0, ..title.clone() }),
                        )
                        .push(
                            Text::new(
                                "Every screen below is laid out by the Rust framework and \
                                 drawn by the Flutter engine.",
                            )
                            .with_style(muted.clone()),
                        )
                        .push(
                            Container::new()
                                .with_size(48.0, 3.0)
                                .with_color(primary)
                                .with_corner_radius(2.0),
                        ),
                )
        })
    }
}

/// A category header that opens to reveal its demos.
struct CategorySection {
    category: Category,
    index: usize,
    expanded: bool,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for CategorySection {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let category = self.category;
        let expanded = self.expanded;
        let header_id = ids::CATEGORY + self.index as u64;
        let handle = self.handle.clone();
        let pressed = self.pressed == Some(header_id);

        let header_handle = handle.clone();
        let header_handlers = PointerHandlers::new()
            .with_tap(move |_| {
                header_handle.set_state(move |state| state.toggle_category(category));
            })
            .with_press_change({
                let handle = handle.clone();
                move |down| {
                    handle.set_state(move |state| {
                        state.pressed = if down { Some(header_id) } else { None };
                    });
                }
            });

        let title = theme.title();
        let muted = theme.muted();
        let surface = theme.surface;
        let outline = theme.outline;
        let radius = theme.radius;
        let spacing = theme.spacing;
        let accent = theme.primary;
        let count = catalog::count(category);

        let header = leaf(move || {
            let chevron = Container::new()
                .with_size(26.0, 26.0)
                .with_color(accent.with_alpha(0x28))
                .with_corner_radius(13.0)
                .with_child(Align::new(
                    Alignment::CENTER,
                    // A triangle glyph rather than a rotating chevron: rotating
                    // one would need the transform to animate, and the section
                    // does not animate yet.
                    Text::new(if expanded { "\u{25BE}" } else { "\u{25B8}" })
                        .with_size(13.0)
                        .with_weight(700)
                        .with_color(accent),
                ));

            let text = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(3.0)
                .push(Text::new(category.title()).with_style(title.clone()))
                .push(Text::new(category.subtitle()).with_style(muted.clone()));

            Pointer::new(
                header_id,
                Container::new()
                    .with_color(if pressed { accent.with_alpha(0x18) } else { surface })
                    .with_corner_radius(radius)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::all(spacing * 1.75))
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(spacing * 1.5)
                            .push(chevron)
                            .push_flex(FlexChild::expanded(text, 1))
                            .push(
                                Text::new(format!("{count}"))
                                    .with_size(13.0)
                                    .with_weight(700)
                                    .with_color(accent),
                            ),
                    ),
            )
            .with_handlers(header_handlers.clone())
        });

        if !expanded {
            return header;
        }

        let mut children = vec![header];
        for (index, demo) in catalog::in_category(category).enumerate() {
            children.push(component(DemoRow {
                demo,
                id: ids::DEMO + slug_index(demo.slug) as u64,
                pressed: self.pressed,
                handle: handle.clone(),
                first: index == 0,
            }));
        }

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(8.0);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        })
    }
}

/// A demo's position in the catalogue, which is what makes its hit-test id
/// stable: the same demo gets the same id whatever category is open.
fn slug_index(slug: &str) -> usize {
    catalog::DEMOS.iter().position(|d| d.slug == slug).unwrap_or(0)
}

/// One tappable row, upstream's `CategoryListItem` child.
struct DemoRow {
    demo: &'static catalog::Demo,
    id: u64,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
    first: bool,
}

impl Component for DemoRow {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let demo = self.demo;
        let id = self.id;
        let held = self.pressed == Some(id);
        let indent = if self.first { 0.0 } else { 0.0 };
        let handle = self.handle.clone();
        let slug = demo.slug;

        let handlers = PointerHandlers::new()
            .with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(move |state| state.open(slug));
                }
            })
            .with_press_change(move |down| {
                handle.set_state(move |state| {
                    state.pressed = if down { Some(id) } else { None };
                });
            });

        let body = theme.body();
        let muted = theme.muted();
        let surface = theme.surface;
        let outline = theme.outline;
        let radius = theme.radius;
        let spacing = theme.spacing;
        let accent = demo.accent;

        leaf(move || {
            let mark = Container::new()
                .with_size(36.0, 36.0)
                .with_color(accent.with_alpha(0x2A))
                .with_corner_radius(10.0)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(demo.mark)
                        .with_size(12.0)
                        .with_weight(700)
                        .with_color(accent),
                ));

            let text = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(3.0)
                .push(
                    Text::new(demo.title)
                        .with_style(TextStyle { font_weight: 700, ..body.clone() }),
                )
                .push(
                    Text::new(demo.subtitle)
                        .with_style(TextStyle { font_size: 12.0, ..muted.clone() }),
                );

            Pointer::new(
                id,
                Container::new()
                    .with_margin(EdgeInsets::only(indent, 0.0, 0.0, 0.0))
                    .with_color(if held { accent.with_alpha(0x1A) } else { surface })
                    .with_corner_radius(radius)
                    .with_border(1.0, if held { accent } else { outline })
                    .with_padding(EdgeInsets::symmetric(spacing * 1.5, spacing * 1.25))
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(spacing * 1.5)
                            .push(mark)
                            .push_flex(FlexChild::expanded(text, 1))
                            .push(
                                Text::new("\u{203A}")
                                    .with_size(20.0)
                                    .with_weight(700)
                                    .with_color(accent),
                            ),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// Kept so the module compiles standalone in tests that only need the frame.
#[allow(dead_code)]
fn unused() -> AnyWidget {
    leaf(|| Empty)
}
