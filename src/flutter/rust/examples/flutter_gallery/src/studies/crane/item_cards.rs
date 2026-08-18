// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/item_cards.dart` (flutter/gallery @
//! d12640d): `DestinationCard` and its `_DestinationImage`.
//!
//! Two layouts, as upstream: on a desktop the card is a column -- the
//! photograph at its own aspect ratio under 4px corners, the name, the
//! subtitle, 40 of bottom padding; on mobile it is a list tile -- a 60px
//! thumbnail, the two texts, and a 1px divider. The photograph is decoded
//! once and cached under its asset name ([`Image::shared`]); when it cannot
//! be decoded the placeholder upstream draws (black at 10%, the image's
//! aspect ratio) stands in. `HighlightFocus`'s press highlight is not ported:
//! upstream's `onPressed` is empty, so the wrapper would draw nothing and do
//! nothing.
//!
//! The texts are upstream's `titleMedium` (16, w500) and `titleSmall` (12,
//! w600, craneGrey); `SelectableText` has no counterpart, and the semantic
//! labels are carried on the data (`Destination::asset_semantic_label`,
//! `Destination::subtitle_semantics`) with no semantics tree to read them.

use rustflutter::framework::{leaf, AnyWidget, BuildContext, Component};
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxFit, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{AspectRatio, ClipRRect, Container, ImageView};

use super::colors;
use super::model::destination::Destination;

/// Upstream's `mobileThumbnailSize`.
pub const MOBILE_THUMBNAIL_SIZE: f32 = 60.0;

/// Upstream's `DestinationCard`.
pub struct DestinationCard {
    pub destination: &'static dyn Destination,
    pub is_desktop: bool,
}

impl Component for DestinationCard {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let destination = self.destination;
        let is_desktop = self.is_desktop;
        // Keyed by the asset name, so the thirty-seven photographs are
        // decoded once for the life of the process rather than once per
        // frame.
        let photo = Image::shared(
            &format!("crane:{}", destination.asset_name()),
            destination.photo(),
        );

        leaf(move || {
            let ratio = destination.image_aspect_ratio();
            // `_DestinationImage`: the photograph, or upstream's placeholder.
            let image: BoxedWidget = match photo.clone() {
                Some(image) => boxed(ImageView::with_fit(image, BoxFit::Cover)),
                None => boxed(Container::new().with_color(Color::argb(0x19, 0x00, 0x00, 0x00))),
            };

            let title = Text::new(destination.name())
                .with_size(16.0)
                .with_weight(500)
                .with_color(colors::CRANE_BLACK)
                .with_font_family(super::theme::RALEWAY);
            let subtitle = Text::new(destination.subtitle())
                .with_size(12.0)
                .with_weight(600)
                .with_color(colors::CRANE_GREY)
                .with_font_family(super::theme::RALEWAY);

            if is_desktop {
                boxed(
                    Container::new()
                        .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, 40.0))
                        .with_child(
                            Column::new()
                                .with_main_axis_size(MainAxisSize::Min)
                                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                .push(ClipRRect::new(4.0, boxed(AspectRatio::new(ratio, image))))
                                .push(
                                    Container::new()
                                        .with_padding(EdgeInsets::only(0.0, 20.0, 0.0, 10.0))
                                        .with_child(title),
                                )
                                .push(subtitle),
                        ),
                )
            } else {
                boxed(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(
                            Container::new()
                                .with_padding(EdgeInsets::symmetric(0.0, 8.0))
                                .with_child(
                                    RenderFlex::row()
                                        .with_main_axis_size(MainAxisSize::Max)
                                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                        .with_spacing(16.0)
                                        .push(ClipRRect::new(
                                            4.0,
                                            boxed(
                                                Container::new()
                                                    .with_size(
                                                        MOBILE_THUMBNAIL_SIZE,
                                                        MOBILE_THUMBNAIL_SIZE,
                                                    )
                                                    .with_child(image),
                                            ),
                                        ))
                                        .push_flex(FlexChild::expanded(
                                            Column::new()
                                                .with_main_axis_size(MainAxisSize::Min)
                                                .with_cross_axis_alignment(
                                                    CrossAxisAlignment::Start,
                                                )
                                                .with_spacing(2.0)
                                                .push(title)
                                                .push(subtitle),
                                            1,
                                        )),
                                ),
                        )
                        // The divider, as `components::Divider` draws it:
                        // a 1px line of the theme's outline in a 16px strip.
                        .push(
                            Container::new().with_height(16.0).with_child(
                                rustflutter::widgets::Align::new(
                                    Alignment::CENTER,
                                    Container::new()
                                        .with_height(1.0)
                                        .with_color(Color::argb(0x1F, 0x00, 0x00, 0x00)),
                                ),
                            ),
                        ),
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::{component, provide, ElementTree};
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    use crate::studies::crane::model::data;

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(super::super::theme::crane_theme(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn a_desktop_card_is_the_image_at_its_aspect_ratio_plus_text() {
        // The card is as tall as the photograph at the offered width plus the
        // two text lines and their padding -- the exact text height is the
        // font's, so what is asserted is the shape: wider card, taller card,
        // and taller than the photograph alone.
        let destination: &'static dyn Destination = &data::fly_destinations()[0];
        let narrow = lay_out(
            component(DestinationCard {
                destination,
                is_desktop: true,
            }),
            200.0,
        );
        let wide = lay_out(
            component(DestinationCard {
                destination,
                is_desktop: true,
            }),
            400.0,
        );
        // ratio 1.0: the image is as tall as it is wide.
        assert!(narrow.height > 200.0, "{narrow:?}");
        assert!(wide.height > narrow.height, "{wide:?} vs {narrow:?}");
    }

    #[test]
    fn a_mobile_card_is_thumbnail_high() {
        let destination: &'static dyn Destination = &data::sleep_destinations()[0];
        let size = lay_out(
            component(DestinationCard {
                destination,
                is_desktop: false,
            }),
            460.0,
        );
        // The 60px thumbnail plus 8px of vertical padding either side, then
        // the divider's 16 -- whatever the texts do, they fit in 60.
        assert!(
            size.height >= MOBILE_THUMBNAIL_SIZE + 16.0 + 16.0,
            "{size:?}"
        );
    }
}
