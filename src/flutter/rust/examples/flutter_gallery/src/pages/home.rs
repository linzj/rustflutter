// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The home page.
//!
//! Ported from `lib/pages/home.dart` (flutter/gallery @ d12640d), both
//! layouts. Upstream picks by `isDisplayDesktop(context)`; the value is
//! resolved at the root (`app::Gallery::build`) and handed down.
//!
//! Mobile, upstream's `_AnimatedHomePage`:
//!
//! ```text
//!   Header "Gallery"        headlineMedium, in primaryContainer
//!   the carousel            six study cards, scrolled sideways
//!   Header "Categories"     the same header again, in primary
//!   Material                a category that opens to its demos
//!   Cupertino
//!   STYLES & OTHER
//! ```
//!
//! Desktop, upstream's desktop branch: the same headers over a carousel with
//! page buttons, then the three categories as columns of demo rows, then the
//! footer with the Flutter logo and the about/feedback/attribution links.
//!
//! The entrance animation is upstream's: an 800ms controller that slides the
//! carousel in from the right and staggers each category down from a 60
//! logical pixel offset. At this commit the splash starts dismissed, so
//! upstream's `_AnimatedHomePage.initState` sets the controller to 1.0
//! immediately and the entrance does not visibly play; the state and the math
//! are here (`app::GalleryState::entrance`), faithful to that.
//!
//! The carousel cards scale with their distance from the current page --
//! upstream's parallax in `_MobileCarousel.builder` -- and the category
//! headers animate open; the controllers are on `GalleryState` and ticked in
//! `Gallery::advance`. What is not ported: the desktop carousel's snapping
//! scroll physics (the framework's `Scroll` has no snapping simulation; the
//! page buttons cover the same navigation), and the restoration ids, which
//! have no counterpart. Both are logged in PORTING.md.

use rustflutter::animation::Curve;
use rustflutter::framework::{component, leaf, many, single, AnyWidget, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, Axis, BoxConstraints, BoxFit, CrossAxisAlignment, FlexChild, MainAxisAlignment,
    MainAxisSize, RenderBox, RenderFlex, RenderRef, Size, StackPosition,
};
use rustflutter::widgets::{
    Align, ClipRRect, Container, ImageView, ListView, Pointer, Stack, Transform,
};

use crate::app::{self, ids, GalleryState};
use crate::constants::DESKTOP_DISPLAY1_FONT_DELTA;
use crate::data::demos::{self as catalog, Category};
use crate::pages::adaptive_layout::MAX_HOME_ITEM_WIDTH;
use crate::pages::category_list_item::{CategoryDemoItem, CategoryListItem};
use crate::pages::splash::{FLUTTER_LOGO, FLUTTER_LOGO_COLOR};
use crate::themes::gallery_theme_data::{text, Scheme};

/// Upstream's `_horizontalPadding` / `_horizontalDesktopPadding`.
const HORIZONTAL_PADDING: f32 = 32.0;
const HORIZONTAL_DESKTOP_PADDING: f32 = 81.0;
/// Upstream's `_carouselItemWidth`.
const CARD_WIDTH: f32 = 296.0;
/// Upstream's `_carouselHeightMin`, and the band the mobile carousel sits in.
const CAROUSEL_HEIGHT_MIN: f32 = 240.0;
/// Upstream's `_carouselItemMobileMargin` / `_carouselItemDesktopMargin`, and
/// the vertical margin both share.
const CARD_MARGIN_MOBILE: f32 = 4.0;
const CARD_MARGIN_DESKTOP: f32 = 8.0;
const CARD_MARGIN_VERTICAL: f32 = 16.0;
/// Upstream's card corner.
const CARD_RADIUS: f32 = 10.0;
/// The desktop categories row's fixed height: upstream's `SizedBox(height:
/// 585)`.
const DESKTOP_CATEGORY_HEIGHT: f32 = 585.0;
/// Upstream's `spaceBetween(28, ...)` between the desktop category columns.
const DESKTOP_CATEGORY_SPACING: f32 = 28.0;

/// Upstream's `Interval`, which the animation module has no counterpart of:
/// the slice of the entrance controller between `begin` and `end`, eased.
fn interval(value: f32, begin: f32, end: f32) -> f32 {
    Curve::EASE.transform(((value - begin) / (end - begin)).clamp(0.0, 1.0))
}

/// The home page, mobile or desktop by the breakpoint.
pub fn page(
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
) -> AnyWidget {
    if is_desktop {
        desktop_page(state, handle)
    } else {
        mobile_page(state, handle)
    }
}

/// Upstream's `Header`: one line of headline medium, with the desktop padding
/// and font delta on desktop. Shared with the settings panel, the way upstream
/// shares it.
pub fn header(
    label: &'static str,
    color: Color,
    is_desktop: bool,
) -> rustflutter::render::BoxedRender {
    let mut style = text::HEADLINE_MEDIUM.styled(color);
    if is_desktop {
        style.font_size += DESKTOP_DISPLAY1_FONT_DELTA;
    }
    let top = if is_desktop { 63.0 } else { 15.0 };
    let bottom = if is_desktop { 21.0 } else { 11.0 };
    RenderRef::new(
        Container::new()
            .with_padding(EdgeInsets::only(0.0, top, 0.0, bottom))
            .with_child(Align::new(
                Alignment::CENTER_LEFT,
                Text::new(label).with_style(style),
            )),
    )
}

// -- Mobile --------------------------------------------------------------------

/// Upstream's `_AnimatedHomePage`.
fn mobile_page(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let scheme = state.scheme();
    let entrance = state.entrance.value();

    let mut rows: Vec<AnyWidget> = vec![
        leaf(|| Container::new().with_size(1.0, 8.0)),
        leaf(move || {
            Container::new()
                .with_margin(EdgeInsets::symmetric(HORIZONTAL_PADDING, 0.0))
                .with_child(header("Gallery", scheme.primary_container, false))
        }),
        // Upstream's `_AnimatedCarousel`: the band slides in from the right
        // over the middle three fifths of the entrance.
        component(Carousel {
            scheme,
            pressed: state.pressed,
            scroll: state.carousel.clone(),
            entrance,
            is_desktop: false,
            handle: handle.clone(),
        }),
        leaf(move || {
            Container::new()
                .with_margin(EdgeInsets::symmetric(HORIZONTAL_PADDING, 0.0))
                .with_child(header("Categories", scheme.primary, false))
        }),
    ];

    for (index, category) in catalog::CATEGORIES.iter().enumerate() {
        // Upstream's `_AnimatedCategoryItem`: each category drops from a 60
        // pixel offset, staggered by a twentieth of the entrance.
        let top = 60.0 * (1.0 - interval(entrance, 0.05 * index as f32, 0.4 + 0.05 * index as f32));
        let item = component(CategoryListItem {
            category: *category,
            index,
            scheme,
            progress: state.category_expand[index].value(),
            pressed: state.pressed,
            is_desktop: false,
            handle: handle.clone(),
        });
        rows.push(single(item, move |rendered| {
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, top, 0.0, 0.0))
                    .with_child(rendered),
            )
        }));
    }

    // The list runs edge to edge; the padding is per-row, because the carousel
    // has to be able to scroll out past it.
    let offset = state.page.offset;
    let extent = state.page.link();
    let scroll = app::scroll_handlers(handle.clone(), |s| &mut s.page, Axis::Vertical);

    // Upstream's drag-down strip: a transparent band over the top of the list
    // whose downward fling reveals the splash.
    let strip = leaf({
        let handle = handle.clone();
        move || {
            let reveal = PointerHandlers::new().with_drag_end({
                let handle = handle.clone();
                move |end| {
                    if end.velocity.dy > 200.0 {
                        handle.set_state(|state| {
                            state.splash.forward();
                        });
                    }
                }
            });
            Pointer::new(ids::SPLASH_STRIP, Container::new().with_height(40.0))
                .with_handlers(reveal.clone())
        }
    });

    let body = many(rows, move |rendered| {
        let mut list = ListView::new()
            .with_offset(offset)
            .with_link(extent.clone());
        for child in rendered {
            list = list.push(child);
        }
        // Outside the rows, so a press that lands on a row and then travels
        // scrolls the page rather than opening the demo.
        Box::new(Pointer::new(ids::PAGE_SCROLL, list).with_handlers(scroll.clone()))
    });

    let stacked = many(vec![body, strip], |mut rendered| {
        let strip = rendered.pop().expect("two children");
        let body = rendered.pop().expect("two children");
        Box::new(Stack::new().push(body).push_positioned(
            strip,
            StackPosition {
                left: Some(0.0),
                top: Some(0.0),
                right: Some(0.0),
                ..StackPosition::default()
            },
        ))
    });

    app::bare_page(state, handle, stacked)
}

// -- The carousel ---------------------------------------------------------------

/// The six study cards, scrolled sideways. Upstream's `_MobileCarousel` and
/// `_DesktopCarousel` share this; the form factor picks margins, centring and
/// whether the page buttons show.
struct Carousel {
    scheme: Scheme,
    pressed: Option<u64>,
    scroll: app::Scroll,
    /// The entrance controller's value; the desktop carousel does not read it.
    entrance: f32,
    is_desktop: bool,
    handle: StateHandle<GalleryState>,
}

impl Component for Carousel {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        // Upstream's parallax: each card scales with its distance from the
        // current page, `(1 - |page - index| * .3)` eased out.
        let page = self.scroll.offset / CARD_WIDTH;
        // Upstream's `_AnimatedCarouselCard`: only the second card gets the
        // entrance's last-tenth start padding.
        let second_padding = 32.0 * (1.0 - interval(self.entrance, 0.9, 1.0));

        let mut cards: Vec<AnyWidget> = Vec::new();
        for (index, study) in catalog::STUDIES.iter().enumerate() {
            let scale = Curve::EASE_OUT
                .transform((1.0 - (page - index as f32).abs() * 0.3).clamp(0.0, 1.0));
            cards.push(component(CarouselCard {
                study,
                id: ids::STUDY_CARD + index as u64,
                scheme: self.scheme,
                pressed: self.pressed,
                is_desktop: self.is_desktop,
                scale: if self.is_desktop { 1.0 } else { scale },
                start_padding: if index == 1 && !self.is_desktop {
                    second_padding
                } else {
                    0.0
                },
                handle: self.handle.clone(),
            }));
        }

        let offset = self.scroll.offset;
        let extent = self.scroll.link();
        let max_extent = self.scroll.max_extent();
        let handlers =
            app::scroll_handlers(self.handle.clone(), |s| &mut s.carousel, Axis::Horizontal);
        // The band slides in from the right over the entrance's middle.
        let slide = 1.0 - interval(self.entrance, 0.2, 0.8);
        let height = CAROUSEL_HEIGHT_MIN;
        let is_desktop = self.is_desktop;
        let prev_id = ids::CAROUSEL_PREV;
        let next_id = ids::CAROUSEL_NEXT;
        let pressed = self.pressed;
        let handle = self.handle.clone();

        many(cards, move |rendered| {
            let mut list = ListView::horizontal()
                .with_offset(offset)
                .with_link(extent.clone());
            if !is_desktop {
                // Upstream's carousel is a PageView whose viewportFraction is
                // one card plus its margins, which centres whichever card is
                // current. Centring the ends is the part of that which shows
                // when nothing has been swiped yet.
                list = list.with_centred_item(CARD_WIDTH);
            }
            // Desktop's `itemExtent: _carouselItemWidth` is an optimization
            // the framework's ListView does not need: the cards are fixed
            // width either way.
            for card in rendered {
                list = list.push(card);
            }
            let list = RenderRef::new(SlideIn {
                child: RenderRef::new(list),
                fraction: if is_desktop { 0.0 } else { slide },
                size: Size::ZERO,
                child_size: Size::ZERO,
            });
            let band = Pointer::new(ids::CAROUSEL_SCROLL, list).with_handlers(handlers.clone());

            if !is_desktop {
                return RenderRef::new(Container::new().with_height(height).with_child(band));
            }

            // Desktop: the page buttons upstream shows at the edges while
            // there is content past them.
            let mut layers = Stack::new().push(band);
            if offset > 0.0 {
                layers = layers.push_positioned(
                    page_button(prev_id, false, pressed, handle.clone()),
                    StackPosition {
                        left: Some(HORIZONTAL_DESKTOP_PADDING - 29.0),
                        top: Some(height / 2.0 - 29.0),
                        ..StackPosition::default()
                    },
                );
            }
            if offset < max_extent {
                layers = layers.push_positioned(
                    page_button(next_id, true, pressed, handle.clone()),
                    StackPosition {
                        right: Some(HORIZONTAL_DESKTOP_PADDING - 29.0),
                        top: Some(height / 2.0 - 29.0),
                        ..StackPosition::default()
                    },
                );
            }
            RenderRef::new(Container::new().with_height(height).with_child(layers))
        })
    }
}

/// Upstream's `_DesktopPageButton`: a 58px half-black circle that drives the
/// carousel by one card.
fn page_button(
    id: u64,
    forward: bool,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> rustflutter::render::BoxedRender {
    let held = pressed == Some(id);
    let handlers = PointerHandlers::new()
        .with_tap({
            let handle = handle.clone();
            move |_| {
                handle.set_state(move |state| {
                    let target = if forward {
                        state.carousel.offset + CARD_WIDTH
                    } else {
                        state.carousel.offset - CARD_WIDTH
                    };
                    state
                        .carousel
                        .animate_to(target, 200_000, Curve::EASE_IN_OUT);
                });
            }
        })
        .with_press_change({
            let handle = handle.clone();
            move |down| {
                handle.set_state(move |state| {
                    state.pressed = if down { Some(id) } else { None };
                });
            }
        });
    let glyph = if forward {
        catalog::icon::ARROW_FORWARD_IOS
    } else {
        catalog::icon::ARROW_BACK_IOS
    };
    RenderRef::new(
        Pointer::new(
            id,
            Container::new()
                .with_size(58.0, 58.0)
                .with_corner_radius(29.0)
                .with_color(if held {
                    Color::argb(0x8A, 0, 0, 0)
                } else {
                    Color::argb(0x80, 0, 0, 0)
                })
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(glyph)
                        .with_font_family(catalog::MATERIAL_ICONS)
                        .with_size(24.0)
                        .with_color(Color::WHITE),
                )),
        )
        .with_handlers(handlers),
    )
}

/// One study card: upstream's artwork, with the title over the bottom of it.
struct CarouselCard {
    study: &'static catalog::Study,
    id: u64,
    scheme: Scheme,
    pressed: Option<u64>,
    is_desktop: bool,
    /// The parallax scale: 1 centred, easing to 0.7 a page away.
    scale: f32,
    /// The second card's entrance padding.
    start_padding: f32,
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
        let margin = if self.is_desktop {
            CARD_MARGIN_DESKTOP
        } else {
            CARD_MARGIN_MOBILE
        };
        let start_padding = self.start_padding;
        let scale = self.scale;

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
            (
                study.card_dark,
                study.fill_dark,
                Color::WHITE.with_alpha(0xDE),
            )
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
                Container::new()
                    .with_padding(EdgeInsets::only(16.0, 0.0, 16.0, 16.0))
                    .with_child(
                        RenderFlex::column()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::End)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .push(Text::new(study.title).with_style(title_style.clone()))
                            .push(Text::new(study.subtitle).with_style(sub_style.clone())),
                    ),
                StackPosition::fill(),
            );

            let card = Container::new()
                .with_margin(EdgeInsets::symmetric(margin, CARD_MARGIN_VERTICAL))
                .with_size(
                    CARD_WIDTH - margin * 2.0,
                    CAROUSEL_HEIGHT_MIN - CARD_MARGIN_VERTICAL * 2.0,
                )
                .with_color(if held { fill.darkened(0.12) } else { fill })
                .with_corner_radius(CARD_RADIUS)
                // Upstream's `clipBehavior: Clip.antiAlias`. Without it the
                // artwork keeps its own aspect under BoxFit::Cover and spills
                // out over the cards on either side.
                .with_child(ClipRRect::new(CARD_RADIUS, layers));

            // The parallax scale, about the card's centre as upstream's
            // `Transform.scale(alignment: Alignment.center)` does.
            let mut wrapped = if scale < 1.0 {
                RenderRef::new(Transform::scale(scale, card).with_origin(Alignment::CENTER))
            } else {
                RenderRef::new(card)
            };

            if start_padding > 0.0 {
                wrapped = RenderRef::new(
                    Container::new()
                        .with_padding(EdgeInsets::only(start_padding, 0.0, 0.0, 0.0))
                        .with_child(wrapped),
                );
            }

            Pointer::new(id, wrapped).with_handlers(handlers.clone())
        })
    }
}

// -- Desktop ---------------------------------------------------------------------

/// Upstream's desktop branch of `HomePage.build`.
fn desktop_page(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let scheme = state.scheme();

    let mut children: Vec<AnyWidget> = vec![
        desktop_home_item(leaf(move || {
            header("Gallery", scheme.primary_container, true)
        })),
        component(Carousel {
            scheme,
            pressed: state.pressed,
            scroll: state.carousel.clone(),
            entrance: 1.0,
            is_desktop: true,
            handle: handle.clone(),
        }),
        desktop_home_item(leaf(move || header("Categories", scheme.primary, true))),
    ];

    // The three categories as columns: upstream's `_DesktopCategoryItem`s in a
    // fixed-height row with `spaceBetween(28, ...)`.
    let mut columns: Vec<AnyWidget> = Vec::new();
    for (index, category) in catalog::CATEGORIES.iter().enumerate() {
        columns.push(component(DesktopCategoryItem {
            category: *category,
            index,
            scheme,
            pressed: state.pressed,
            scroll: state.category_columns[index].clone(),
            handle: handle.clone(),
        }));
    }
    children.push(many(columns, move |rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);
        let count = rendered.len();
        for (index, child) in rendered.into_iter().enumerate() {
            row = row.push_flex(FlexChild::flexible(child, 1));
            if index + 1 < count {
                row = row.push(Container::new().with_size(DESKTOP_CATEGORY_SPACING, 1.0));
            }
        }
        Box::new(
            Container::new()
                .with_height(DESKTOP_CATEGORY_HEIGHT)
                .with_child(desktop_home_item_render(row)),
        )
    }));

    children.push(leaf(|| Container::new().with_size(1.0, 81.0)));
    children.push(component(Footer {
        scheme,
        pressed: state.pressed,
        handle: handle.clone(),
    }));
    children.push(leaf(|| Container::new().with_size(1.0, 109.0)));

    let offset = state.page.offset;
    let extent = state.page.link();
    let handlers = app::scroll_handlers(handle.clone(), |s| &mut s.page, Axis::Vertical);

    let body = many(children, move |rendered| {
        let mut list = ListView::new()
            .with_offset(offset)
            .with_link(extent.clone());
        for child in rendered {
            list = list.push(child);
        }
        Box::new(Pointer::new(ids::PAGE_SCROLL, list).with_handlers(handlers.clone()))
    });

    app::bare_page(state, handle, body)
}

/// Upstream's `_DesktopHomeItem`: centred, at most `maxHomeItemWidth` wide,
/// padded 81 in from the sides.
fn desktop_home_item(child: AnyWidget) -> AnyWidget {
    single(child, |rendered| desktop_home_item_render(rendered))
}

fn desktop_home_item_render(child: impl RenderBox + 'static) -> rustflutter::render::BoxedRender {
    RenderRef::new(DesktopHomeItem {
        child: RenderRef::new(child),
        size: Size::ZERO,
        child_size: Size::ZERO,
    })
}

/// The render half of `_DesktopHomeItem`: the width cap exists at layout, not
/// at build.
struct DesktopHomeItem {
    child: rustflutter::render::BoxedRender,
    size: Size,
    child_size: Size,
}

impl RenderBox for DesktopHomeItem {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let width = constraints.max_width.min(MAX_HOME_ITEM_WIDTH);
        let inner = (width - HORIZONTAL_DESKTOP_PADDING * 2.0).max(0.0);
        self.child_size =
            self.child
                .layout(BoxConstraints::new(0.0, inner, 0.0, constraints.max_height));
        self.size = Size::new(constraints.max_width, self.child_size.height);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(
        &self,
        context: &mut rustflutter::render::PaintContext,
        offset: rustflutter::render::Offset,
    ) {
        let dx = (self.size.width - self.child_size.width) / 2.0;
        self.child.paint(
            context,
            rustflutter::render::Offset::new(offset.dx + dx, offset.dy),
        );
    }

    fn hit_test(
        &self,
        position: rustflutter::render::Offset,
        result: &mut rustflutter::render::HitTestResult,
    ) -> bool {
        let dx = (self.size.width - self.child_size.width) / 2.0;
        let local = rustflutter::render::Offset::new(position.dx - dx, position.dy);
        self.size.contains(position) && self.child.hit_test(local, result)
    }
}

/// Upstream's `_DesktopCategoryItem`: the header over the category's demo
/// rows, in a rounded surface card.
struct DesktopCategoryItem {
    category: Category,
    index: usize,
    scheme: Scheme,
    pressed: Option<u64>,
    scroll: app::Scroll,
    handle: StateHandle<GalleryState>,
}

impl Component for DesktopCategoryItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let category = self.category;
        let scheme = self.scheme;
        let title = category.title().unwrap_or("");
        let icon = category
            .icon()
            .and_then(|bytes| Image::shared(category.title().unwrap_or("?"), bytes));
        let title_style = text::HEADLINE_SMALL.styled(scheme.on_surface);
        let header_fill = scheme.on_background;

        let mut rows: Vec<AnyWidget> = Vec::new();
        for demo in catalog::in_category(category) {
            let id = ids::DEMO + slug_index(demo.slug) as u64;
            rows.push(component(CategoryDemoItem {
                demo,
                id,
                scheme,
                pressed: self.pressed,
                is_desktop: true,
                handle: self.handle.clone(),
            }));
        }

        let offset = self.scroll.offset;
        let extent = self.scroll.link();
        let index = self.index;
        let handlers = app::scroll_handlers(
            self.handle.clone(),
            move |s| &mut s.category_columns[index],
            Axis::Vertical,
        );

        many(rows, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }

            let mut header_row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if let Some(icon) = icon.clone() {
                header_row = header_row.push(
                    Container::new()
                        .with_padding(EdgeInsets::all(10.0))
                        .with_child(
                            Container::new()
                                .with_size(64.0, 64.0)
                                .with_child(ImageView::with_fit(icon, BoxFit::Contain)),
                        ),
                );
            }
            header_row = header_row.push_flex(FlexChild::expanded(
                Container::new()
                    .with_padding(EdgeInsets::only(8.0, 0.0, 0.0, 0.0))
                    .with_child(Text::new(title).with_style(title_style.clone())),
                1,
            ));

            let list = ListView::new()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(column);

            Box::new(
                Container::new()
                    .with_color(scheme.surface)
                    .with_corner_radius(CARD_RADIUS)
                    .with_child(ClipRRect::new(
                        CARD_RADIUS,
                        RenderFlex::column()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .push(
                                Container::new()
                                    .with_color(header_fill)
                                    .with_child(header_row),
                            )
                            .push(
                                Container::new()
                                    .with_height(2.0)
                                    .with_color(scheme.background),
                            )
                            .push_flex(FlexChild::flexible(
                                Pointer::new(ids::CATEGORY_COLUMN_SCROLL + index as u64, list)
                                    .with_handlers(handlers.clone()),
                                1,
                            )),
                    )),
            )
        })
    }
}

/// The desktop footer: the Flutter logo, then the about/feedback/attribution
/// links right-aligned. Upstream's last `_DesktopHomeItem` row.
struct Footer {
    scheme: Scheme,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for Footer {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        // Upstream shows the monochrome logo on dark and the colour one on
        // light, and opens flutter.dev on tap; there is no URL launcher here,
        // so the logo is not tappable. Logged in PORTING.md.
        let bytes = if scheme.is_dark {
            FLUTTER_LOGO
        } else {
            FLUTTER_LOGO_COLOR
        };
        let logo = Image::shared(
            if scheme.is_dark {
                "flutter_logo"
            } else {
                "flutter_logo_color"
            },
            bytes,
        );

        let about_id = ids::SETTINGS_LOCAL + 602;
        let held = self.pressed == Some(about_id);
        let about_handlers = PointerHandlers::new()
            .with_tap({
                let handle = self.handle.clone();
                move |_| {
                    handle.set_state(|state| state.about_open = true);
                }
            })
            .with_press_change({
                let handle = self.handle.clone();
                move |down| {
                    handle.set_state(move |state| {
                        state.pressed = if down { Some(about_id) } else { None };
                    });
                }
            });

        let link_style = text::LABEL_LARGE.styled(scheme.on_secondary);
        let attribution_style = {
            let mut style = text::BODY_LARGE.styled(scheme.on_secondary);
            style.font_size = 12.0;
            style
        };

        leaf(move || {
            let logo_view: rustflutter::render::BoxedRender = match logo.clone() {
                Some(image) => RenderRef::new(ImageView::new(image)),
                None => RenderRef::new(Container::new().with_size(100.0, 100.0)),
            };

            let links = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push(
                    Pointer::new(
                        about_id,
                        Container::new()
                            .with_padding(EdgeInsets::symmetric(24.0, 12.0))
                            .with_color(if held {
                                scheme.on_surface.with_alpha(0x14)
                            } else {
                                Color::TRANSPARENT
                            })
                            .with_child(
                                Text::new("About Flutter Gallery").with_style(link_style.clone()),
                            ),
                    )
                    .with_handlers(about_handlers.clone()),
                )
                .push(
                    // Disabled: upstream opens the issue tracker through
                    // `url_launcher`, which has no counterpart here.
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(24.0, 12.0))
                        .with_child(
                            Text::new("Send feedback")
                                .with_style(text::LABEL_LARGE.styled(scheme.muted())),
                        ),
                )
                .push(
                    Container::new()
                        .with_padding(EdgeInsets::only(24.0, 0.0, 0.0, 0.0))
                        .with_child(
                            Text::new("Designed by TOASTER in London")
                                .with_style(attribution_style.clone()),
                        ),
                );

            desktop_home_item_render(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(logo_view)
                    .push_flex(FlexChild::expanded(
                        Align::new(Alignment::CENTER_RIGHT, links),
                        1,
                    )),
            )
        })
    }
}

/// The carousel's entrance slide, a render object because the travel is a
/// fraction of a width that only exists once layout has run.
struct SlideIn {
    child: rustflutter::render::BoxedRender,
    /// How far off to the right the child sits, as a fraction of the width.
    fraction: f32,
    size: Size,
    child_size: Size,
}

impl RenderBox for SlideIn {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.child_size = self.child.layout(constraints);
        self.size = constraints.constrain(self.child_size);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(
        &self,
        context: &mut rustflutter::render::PaintContext,
        offset: rustflutter::render::Offset,
    ) {
        self.child.paint(
            context,
            rustflutter::render::Offset::new(
                offset.dx + self.size.width * self.fraction,
                offset.dy,
            ),
        );
    }

    fn hit_test(
        &self,
        position: rustflutter::render::Offset,
        result: &mut rustflutter::render::HitTestResult,
    ) -> bool {
        let local = rustflutter::render::Offset::new(
            position.dx - self.size.width * self.fraction,
            position.dy,
        );
        self.child_size.contains(local) && self.child.hit_test(local, result)
    }
}

/// A demo's position in the catalogue, which is what makes its hit-test id
/// stable: the same demo gets the same id whatever layout shows it.
fn slug_index(slug: &str) -> usize {
    catalog::DEMOS
        .iter()
        .position(|d| d.slug == slug)
        .unwrap_or(0)
}
