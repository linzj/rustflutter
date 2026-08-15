// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The home page.
//!
//! Ported from `new_gallery/lib/pages/home.dart`, mobile layout. Upstream's
//! order, and the reason the page reads the way it does:
//!
//! ```text
//!   Header "Gallery"        headlineMedium, in primaryContainer
//!   the carousel            six study cards, scrolled sideways
//!   Header "Categories"     the same header again
//!   Material                a category that opens to twenty-four demos
//!   Cupertino               thirteen
//!   STYLES & OTHER          four
//! ```
//!
//! The cards are upstream's own artwork, and the demo icons are upstream's own
//! icon font -- see `catalog.rs`. What is not ported: the staggered entrance
//! animation each category plays on first paint, the carousel's parallax, and
//! the splash page in front of it all.
//!
//! Upstream animates a category open, which needs the expanding list laid out
//! at two heights at once. Here the chevron turns and the list appears. The
//! animation is a render object rather than a composition, and is worth doing
//! when something else needs it too.

use rustflutter::framework::{AnyWidget, StateHandle, component, leaf, many};
use rustflutter::gestures::PointerHandlers;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxFit, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex, Size,
    StackPosition,
};
use rustflutter::widgets::{
    Align, ClipRRect, Container, Empty, ImageView, ListView, Pointer, Stack,
};

use crate::app::{self, GalleryState, ids};
use crate::catalog::{self, Category};
use crate::theme::{Scheme, text};

/// Upstream's `_horizontalPadding`.
const HORIZONTAL_PADDING: f32 = 32.0;
/// Upstream's `_carouselItemWidth`, and the band it sits in.
///
/// The band is `_carouselHeight(.4, context)`, which at a text scale of one is
/// `_carouselHeightMin` -- 240. The card asks for 240 as well, but it also
/// carries a vertical margin of 16, and the page view it lives in clips rather
/// than grows: what is left for the card body is 240 less the margins.
const CARD_WIDTH: f32 = 296.0;
const CAROUSEL_HEIGHT: f32 = 240.0;
/// Upstream's `_carouselItemMobileMargin`, and its vertical counterpart.
const CARD_MARGIN: f32 = 4.0;
const CARD_MARGIN_VERTICAL: f32 = 16.0;
const CARD_HEIGHT: f32 = CAROUSEL_HEIGHT - CARD_MARGIN_VERTICAL * 2.0;
/// Upstream's card corner.
const CARD_RADIUS: f32 = 10.0;

pub fn page(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let scheme = state.scheme();

    let mut rows: Vec<AnyWidget> = vec![
        component(Header { text: "Gallery", scheme }),
        component(Carousel { scheme, pressed: state.pressed, handle: handle.clone() }),
        component(Header { text: "Categories", scheme }),
    ];

    for (index, category) in catalog::CATEGORIES.iter().enumerate() {
        rows.push(component(CategoryListItem {
            category: *category,
            index,
            scheme,
            expanded: state.is_expanded(*category),
            pressed: state.pressed,
            handle: handle.clone(),
        }));
    }

    // The list runs edge to edge; the padding is per-row, because the carousel
    // has to be able to scroll out past it.
    let body = many(rows, move |rendered| {
        let mut list = ListView::new().with_offset(0.0);
        list = list.push(Container::new().with_size(1.0, 8.0));
        for child in rendered {
            list = list.push(child);
        }
        Box::new(list.push(Container::new().with_size(1.0, 32.0)))
    });

    app::bare_page(state, handle, body)
}

/// Upstream's `Header`: one line, in `primaryContainer`, with fixed padding.
struct Header {
    text: &'static str,
    scheme: Scheme,
}

impl Component for Header {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let label = self.text;
        let color = self.scheme.primary_container;
        let style = text::HEADLINE_MEDIUM.styled(color);

        leaf(move || {
            Container::new()
                .with_padding(EdgeInsets::only(HORIZONTAL_PADDING, 15.0, HORIZONTAL_PADDING, 11.0))
                .with_child(Align::new(
                    Alignment::CENTER_LEFT,
                    Text::new(label).with_style(style.clone()),
                ))
        })
    }
}

/// The six study cards, scrolled sideways. Upstream's `_MobileCarousel`.
struct Carousel {
    scheme: Scheme,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for Carousel {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let mut cards: Vec<AnyWidget> = Vec::new();
        for (index, study) in catalog::STUDIES.iter().enumerate() {
            cards.push(component(CarouselCard {
                study,
                id: ids::STUDY_CARD + index as u64,
                scheme: self.scheme,
                pressed: self.pressed,
                handle: self.handle.clone(),
            }));
        }

        many(cards, move |rendered| {
            // Upstream's carousel is a PageView whose viewportFraction is one
            // card plus its margins, which centres whichever card is current.
            // Centring the ends is the part of that which shows when nothing
            // has been swiped yet.
            let mut list = ListView::horizontal().with_centred_item(CARD_WIDTH);
            for card in rendered {
                list = list.push(card);
            }
            Box::new(Container::new().with_height(CAROUSEL_HEIGHT).with_child(list))
        })
    }
}

/// One study card: upstream's artwork, with the title over the bottom of it.
struct CarouselCard {
    study: &'static catalog::Study,
    id: u64,
    scheme: Scheme,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for CarouselCard {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let study = self.study;
        let scheme = self.scheme;
        let id = self.id;
        let held = self.pressed == Some(id);
        let handle = self.handle.clone();
        let slug = study.slug;

        let handlers = PointerHandlers::new()
            .with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(move |state| state.open_study(slug));
                }
            })
            .with_press_change(move |down| {
                handle.set_state(move |state| {
                    state.pressed = if down { Some(id) } else { None };
                });
            });

        // Upstream picks the dark artwork and writes the title in white at 87%
        // when the theme is dark; in light it uses the study's own brand colour.
        let (bytes, fill, ink) = if scheme.is_dark {
            (study.card_dark, study.fill_dark, Color::WHITE.with_alpha(0xDE))
        } else {
            (study.card, study.fill, study.text)
        };
        let artwork = Image::shared(&format!("{slug}:{}", scheme.is_dark), bytes);

        let title_style = text::BODY_SMALL.styled(ink);
        let sub_style = text::LABEL_SMALL.styled(ink);

        leaf(move || {
            let mut layers = Stack::new();
            if let Some(artwork) = artwork.clone() {
                layers = layers.push_positioned(
                    ImageView::with_fit(artwork, BoxFit::Cover),
                    StackPosition::fill(),
                );
            }
            layers = layers.push_positioned(
                Container::new().with_padding(EdgeInsets::only(16.0, 0.0, 16.0, 16.0)).with_child(
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(rustflutter::render::MainAxisAlignment::End)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .push(Text::new(study.title).with_style(title_style.clone()))
                        .push(Text::new(study.subtitle).with_style(sub_style.clone())),
                ),
                StackPosition::fill(),
            );

            Pointer::new(
                id,
                Container::new()
                    .with_margin(EdgeInsets::symmetric(CARD_MARGIN, CARD_MARGIN_VERTICAL))
                    .with_size(CARD_WIDTH - CARD_MARGIN * 2.0, CARD_HEIGHT)
                    .with_color(if held { fill.darkened(0.12) } else { fill })
                    .with_corner_radius(CARD_RADIUS)
                    // Upstream's `clipBehavior: Clip.antiAlias`. Without it the
                    // artwork keeps its own aspect under BoxFit::Cover and
                    // spills out over the cards on either side -- and the
                    // rounded corners are painted under a square image.
                    .with_child(ClipRRect::new(CARD_RADIUS, layers)),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// A category that opens to reveal its demos. Upstream's `CategoryListItem`.
struct CategoryListItem {
    category: Category,
    index: usize,
    scheme: Scheme,
    expanded: bool,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for CategoryListItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let category = self.category;
        let scheme = self.scheme;
        let expanded = self.expanded;
        let header_id = ids::CATEGORY + self.index as u64;
        let handle = self.handle.clone();
        let held = self.pressed == Some(header_id);

        let header_handlers = PointerHandlers::new()
            .with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(move |state| state.toggle_category(category));
                }
            })
            .with_press_change({
                let handle = handle.clone();
                move |down| {
                    handle.set_state(move |state| {
                        state.pressed = if down { Some(header_id) } else { None };
                    });
                }
            });

        let title = category.title().unwrap_or("");
        let icon = category
            .icon()
            .and_then(|bytes| Image::shared(category.title().unwrap_or("?"), bytes));
        let title_style = text::HEADLINE_SMALL.styled(scheme.on_surface);

        // Upstream animates between these two over 200ms. The ends are its
        // numbers; only the travel between them is missing.
        //
        //            collapsed                 expanded
        //   height   80                        96
        //   margin   LTRB(32, 8, 32, 8)        zero
        //   image    all(8)                    start 16, else 8
        //   radius   10                        0
        //   chevron  hidden                    shown
        //
        // The collapsed header is inset and rounded so it reads as a card; the
        // open one runs edge to edge and squares off, so the demos below read
        // as part of it rather than as a list beside it.
        let header_height = if expanded { 96.0 } else { 80.0 };
        let margin = if expanded {
            EdgeInsets::ZERO
        } else {
            EdgeInsets::symmetric(HORIZONTAL_PADDING, 8.0)
        };
        let image_padding = if expanded {
            EdgeInsets::only(16.0, 8.0, 8.0, 8.0)
        } else {
            EdgeInsets::all(8.0)
        };
        let radius = if expanded { 0.0 } else { 10.0 };

        let header = leaf(move || {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if let Some(icon) = icon.clone() {
                row = row.push(
                    Container::new().with_padding(image_padding).with_child(
                        Container::new()
                            .with_size(64.0, 64.0)
                            .with_child(ImageView::with_fit(icon, BoxFit::Contain)),
                    ),
                );
            }
            row = row.push_flex(FlexChild::expanded(
                Container::new().with_padding(EdgeInsets::only(8.0, 0.0, 0.0, 0.0)).with_child(
                    Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(title).with_style(title_style.clone()),
                    ),
                ),
                1,
            ));
            // Only when open. Upstream fades it in with the expansion, so a
            // closed header carries no arrow at all -- the inset card shape is
            // what says it can be tapped.
            if expanded {
                row = row.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(8.0, 0.0, 32.0, 0.0))
                        .with_child(
                            Text::new(catalog::icon::ARROW_UP)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(scheme.on_surface),
                        ),
                );
            }

            Pointer::new(
                header_id,
                Container::new()
                    .with_margin(margin)
                    .with_height(header_height)
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x1F)
                    } else {
                        scheme.on_background
                    })
                    .with_corner_radius(radius)
                    .with_child(row),
            )
            .with_handlers(header_handlers.clone())
        });

        if !expanded {
            return header;
        }

        let mut children = vec![header];
        for demo in catalog::in_category(category) {
            children.push(component(CategoryDemoItem {
                demo,
                id: ids::DEMO + slug_index(demo.slug) as u64,
                scheme,
                pressed: self.pressed,
                handle: handle.clone(),
            }));
        }
        // Upstream's extra space below an open list.
        children.push(leaf(|| Container::new().with_size(1.0, 12.0)));

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
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

/// One tappable demo row. Upstream's `CategoryDemoItem`.
struct CategoryDemoItem {
    demo: &'static catalog::Demo,
    id: u64,
    scheme: Scheme,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for CategoryDemoItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let demo = self.demo;
        let scheme = self.scheme;
        let id = self.id;
        let held = self.pressed == Some(id);
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

        let title_style = text::TITLE_MEDIUM.styled(scheme.on_surface);
        let sub_style = text::LABEL_SMALL.styled(scheme.muted());

        leaf(move || {
            let text_column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .push(Text::new(demo.title).with_style(title_style.clone()))
                .push(Text::new(demo.subtitle).with_style(sub_style.clone()))
                .push(Container::new().with_size(1.0, 20.0))
                // Upstream's one-pixel rule, in the background colour so it
                // reads as a gap in the surface rather than as a drawn line.
                .push(Container::new().with_height(1.0).with_color(scheme.background));

            Pointer::new(
                id,
                Container::new()
                    .with_color(if held { scheme.on_surface.with_alpha(0x14) } else { scheme.surface })
                    .with_padding(EdgeInsets::only(32.0, 20.0, 8.0, 0.0))
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .push(
                                Text::new(demo.icon)
                                    .with_font_family(demo.icon_family)
                                    .with_size(24.0)
                                    .with_color(scheme.primary),
                            )
                            .push(Container::new().with_size(40.0, 1.0))
                            .push_flex(FlexChild::expanded(text_column, 1)),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// Kept so the module compiles standalone in tests that only need the frame.
#[allow(dead_code)]
fn unused() -> (AnyWidget, Size) {
    (leaf(|| Empty), Size::ZERO)
}
