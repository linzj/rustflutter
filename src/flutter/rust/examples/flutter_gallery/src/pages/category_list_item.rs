// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A category on the home page, and the demo rows it opens to.
//!
//! Ported from `lib/pages/category_list_item.dart` (flutter/gallery @ d12640d):
//! `CategoryListItem` and `CategoryDemoItem`, which earlier batches kept inside
//! `pages/home.rs`.
//!
//! Upstream animates the expansion over 200ms with an `AnimationController`
//! per item: the header's height, margin, image padding and corner radius
//! tween, the chevron fades in, and the children are clipped to a growing
//! height factor. The controllers live on `GalleryState` (one per category,
//! `app::GalleryState::category_expand`) and are ticked in `Gallery::advance`;
//! this file reads the eased value it is handed. The ends and the easing are
//! upstream's numbers.

use rustflutter::animation::Curve;
use rustflutter::framework::{component, leaf, many, AnyWidget, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxFit, CrossAxisAlignment, FlexChild, MainAxisSize, RenderAlign, RenderFlex,
};
use rustflutter::widgets::{ClipRRect, Container, ImageView, Pointer};

use crate::app::{ids, GalleryState};
use crate::data::demos::{self as catalog, Category};
use crate::themes::gallery_theme_data::{text, Scheme};

/// Upstream's `_expandDuration`.
pub const EXPAND_DURATION: std::time::Duration = std::time::Duration::from_millis(200);

/// Upstream's `_easeInTween`: every tween below runs on the eased value.
pub fn eased(value: f32) -> f32 {
    Curve::EASE_IN.transform(value)
}

/// The collapsed header's margin, `EdgeInsets.fromLTRB(32, 8, 32, 8)`, and the
/// expanded one's, `EdgeInsets.zero`.
fn lerp_insets(from: EdgeInsets, to: EdgeInsets, t: f32) -> EdgeInsets {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    EdgeInsets {
        left: lerp(from.left, to.left),
        top: lerp(from.top, to.top),
        right: lerp(from.right, to.right),
        bottom: lerp(from.bottom, to.bottom),
    }
}

/// A category that opens to reveal its demos. Upstream's `CategoryListItem`.
///
/// `progress` is the expansion controller's value, before easing; the widget
/// eases it the way upstream's `_easeInTween` does.
pub struct CategoryListItem {
    pub category: Category,
    pub index: usize,
    pub scheme: Scheme,
    /// The expansion controller's value: 0 collapsed, 1 open.
    pub progress: f32,
    pub pressed: Option<u64>,
    /// Upstream reads `isDisplayDesktop(context)` for the demo rows' end
    /// padding; the value is resolved at the page and passed down.
    pub is_desktop: bool,
    pub handle: StateHandle<GalleryState>,
}

impl Component for CategoryListItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let category = self.category;
        let scheme = self.scheme;
        let t = eased(self.progress);
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

        // Upstream's tweens, at this frame's value:
        //
        //            collapsed                 expanded
        //   height   80                        96
        //   margin   LTRB(32, 8, 32, 8)        zero
        //   image    all(8)                    start 16, else 8
        //   radius   10                        0
        //   chevron  transparent               opaque
        //
        // The collapsed header is inset and rounded so it reads as a card; the
        // open one runs edge to edge and squares off, so the demos below read
        // as part of it rather than as a list beside it.
        let header_height = 80.0 + (96.0 - 80.0) * t;
        let margin = lerp_insets(
            EdgeInsets::only(HORIZONTAL_MARGIN, 8.0, HORIZONTAL_MARGIN, 8.0),
            EdgeInsets::ZERO,
            t,
        );
        let image_padding = lerp_insets(
            EdgeInsets::all(8.0),
            EdgeInsets::only(16.0, 8.0, 8.0, 8.0),
            t,
        );
        let radius = 10.0 * (1.0 - t);
        let children_padding = lerp_insets(
            EdgeInsets::symmetric(HORIZONTAL_MARGIN, 0.0),
            EdgeInsets::ZERO,
            t,
        );

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
                Container::new()
                    .with_padding(EdgeInsets::only(8.0, 0.0, 0.0, 0.0))
                    .with_child(rustflutter::widgets::Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(title).with_style(title_style.clone()),
                    )),
                1,
            ));
            // Upstream's `Opacity(opacity: chevronOpacity, ...)`: the arrow is
            // in the tree from the first frame of the expansion, fading in,
            // and gone entirely while closed.
            if t > 0.0 {
                row = row.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(8.0, 0.0, 32.0, 0.0))
                        .with_child(
                            Text::new(catalog::icon::ARROW_UP)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(scheme.on_surface.with_alpha((t * 255.0) as u8)),
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

        // Upstream builds the children whenever the controller is not
        // dismissed, clipped to the eased height factor.
        if self.progress <= 0.0 {
            return header;
        }

        let is_desktop = self.is_desktop;
        let mut children: Vec<AnyWidget> = Vec::new();
        for demo in catalog::in_category(category) {
            children.push(component(CategoryDemoItem {
                demo,
                id: ids::DEMO + slug_index(demo.slug) as u64,
                scheme,
                pressed: self.pressed,
                is_desktop,
                handle: handle.clone(),
            }));
        }
        // Upstream's extra space below an open list.
        children.push(leaf(|| Container::new().with_size(1.0, 12.0)));

        let demos = many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        });

        many(vec![header, demos], move |mut rendered| {
            let demos = rendered.pop().expect("two children");
            let header = rendered.pop().expect("two children");
            // Upstream's `ClipRect(Align(heightFactor: _childrenHeightFactor))`:
            // the list is laid out whole and shown to a fraction of its
            // height. RenderAlign's height factor is the same arithmetic.
            let clipped = ClipRRect::new(
                0.0,
                RenderAlign::new(Alignment::TOP_CENTER, demos)
                    .with_factors(None, Some(t.max(f32::EPSILON))),
            );
            Box::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(header)
                    .push(
                        Container::new()
                            .with_padding(children_padding)
                            .with_child(clipped),
                    ),
            )
        })
    }
}

/// The horizontal inset of a collapsed category header, and of the expanding
/// demo list. Upstream inlines `32` in both tweens.
const HORIZONTAL_MARGIN: f32 = 32.0;

/// A demo's position in the catalogue, which is what makes its hit-test id
/// stable: the same demo gets the same id whatever category is open.
fn slug_index(slug: &str) -> usize {
    catalog::DEMOS
        .iter()
        .position(|d| d.slug == slug)
        .unwrap_or(0)
}

/// One tappable demo row. Upstream's `CategoryDemoItem`.
pub struct CategoryDemoItem {
    pub demo: &'static catalog::Demo,
    pub id: u64,
    pub scheme: Scheme,
    pub pressed: Option<u64>,
    /// Upstream reads `isDisplayDesktop(context)` for the row's end padding.
    pub is_desktop: bool,
    pub handle: StateHandle<GalleryState>,
}

impl Component for CategoryDemoItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let demo = self.demo;
        let scheme = self.scheme;
        let id = self.id;
        let held = self.pressed == Some(id);
        let handle = self.handle.clone();
        let slug = demo.slug;
        let end_padding = if self.is_desktop { 16.0 } else { 8.0 };

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
        // Upstream writes the subtitle at half opacity rather than in a
        // second colour.
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
                .push(
                    Container::new()
                        .with_height(1.0)
                        .with_color(scheme.background),
                );

            Pointer::new(
                id,
                Container::new()
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x14)
                    } else {
                        scheme.surface
                    })
                    .with_padding(EdgeInsets::only(32.0, 20.0, end_padding, 0.0))
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
