// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/grid_list_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `GridListDemo` is a `Scaffold` with an app bar (the
//! `demoGridListsTitle`, no leading) over one `GridView.count` of the twelve
//! `_Photo`s, each a `_GridDemoPhotoItem` in the style the page's options
//! section selected -- image only, with header, or with footer. That is what
//! is built here: the selection arrives as the catalogue configuration index
//! (`Demo::configurations`, switched in `pages/demo.rs`'s options section),
//! and the stage shows exactly one grid in that style.
//!
//! Divergences, each marked at its site as well:
//!
//! * **fixed viewport height** -- upstream's `GridView` is the body of a
//!   `Scaffold` and fills the screen; the stage here is a shrink-wrapping
//!   column inside the demo page, where a viewport given an unbounded axis
//!   has no window to be, so the grid gets the height the demo card would
//!   have given it ([`grid_height`]) and scrolls inside it, on a [`Scroll`]
//!   held by the StatefulComponent (`GridListDemo`).
//! * **no `Semantics` label** -- the photo's `Semantics(label:)` has no
//!   counterpart in the framework; the title and subtitle it announced are
//!   still what the header and footer styles draw.

use std::rc::Rc;

use rustflutter::components::K_TOOLBAR_HEIGHT;
use rustflutter::framework::{
    leaf, many, single, stateful, AnyWidget, BuildContext, StateHandle, StatefulComponent,
};
use rustflutter::gestures::PointerHandlers;
use rustflutter::grid::GridView;
use rustflutter::media_query::size_of;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxFit, CrossAxisAlignment, MainAxisSize, RenderFlex, RenderRef, StackPosition,
};
use rustflutter::scrolling::Scroll;
use rustflutter::widgets::{Align, ClipRRect, ImageView, Pointer, Positioned, Stack};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

/// The `GridTileBar` heights (`grid_tile_bar.dart`'s `preferredSize`): 48
/// with a title only, 68 with a subtitle as well.
const HEADER_BAR_HEIGHT: f32 = 48.0;
const FOOTER_BAR_HEIGHT: f32 = 68.0;

/// Upstream's `Colors.black45`, the `GridTileBar` background.
const BLACK_45: Color = Color(0x73000000);

/// The chrome around the demo card: the page's own bar (56), the wrapper's
/// bottom padding (16, `pages/demo.rs`'s `demo_wrapper`) and the stage card's
/// padding (16 either end, `demos/material/mod.rs`'s `spacing * 2`).
const CHROME_HEIGHT: f32 = 56.0 + 16.0 + 32.0;

/// Upstream's `GridListDemoType` (mirrored in `material_demo_types.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridListDemoType {
    ImageOnly,
    Header,
    Footer,
}

/// The style a configuration index selects, in the order
/// `Demo::configurations` lists them: image only, with header, with footer.
fn style_for(config: usize) -> GridListDemoType {
    match config {
        1 => GridListDemoType::Header,
        2 => GridListDemoType::Footer,
        _ => GridListDemoType::ImageOnly,
    }
}

/// Upstream's `_Photo`, with the decoded asset standing in for `assetName`:
/// there is no asset bundle here, so the bytes are compiled in (the same call
/// the Shrine study made for its photographs). The PNGs are upstream's
/// `flutter_gallery_assets` `places/`, copied verbatim into `assets/places/`.
struct Photo {
    asset: &'static [u8],
    /// The image cache key, upstream's `assetName`.
    cache_key: &'static str,
    title: &'static str,
    subtitle: &'static str,
}

/// Upstream's `GridListDemo._photos`, in order.
fn photos() -> Vec<Photo> {
    let l10n = GalleryLocalizations::en();
    vec![
        Photo {
            asset: include_bytes!("../../../assets/places/india_chennai_flower_market.png"),
            cache_key: "places/india_chennai_flower_market.png",
            title: l10n.place_chennai(),
            subtitle: l10n.place_flower_market(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_tanjore_bronze_works.png"),
            cache_key: "places/india_tanjore_bronze_works.png",
            title: l10n.place_tanjore(),
            subtitle: l10n.place_bronze_works(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_tanjore_market_merchant.png"),
            cache_key: "places/india_tanjore_market_merchant.png",
            title: l10n.place_tanjore(),
            subtitle: l10n.place_market(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_tanjore_thanjavur_temple.png"),
            cache_key: "places/india_tanjore_thanjavur_temple.png",
            title: l10n.place_tanjore(),
            subtitle: l10n.place_thanjavur_temple(),
        },
        Photo {
            asset: include_bytes!(
                "../../../assets/places/india_tanjore_thanjavur_temple_carvings.png"
            ),
            cache_key: "places/india_tanjore_thanjavur_temple_carvings.png",
            title: l10n.place_tanjore(),
            subtitle: l10n.place_thanjavur_temple(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_pondicherry_salt_farm.png"),
            cache_key: "places/india_pondicherry_salt_farm.png",
            title: l10n.place_pondicherry(),
            subtitle: l10n.place_salt_farm(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_chennai_highway.png"),
            cache_key: "places/india_chennai_highway.png",
            title: l10n.place_chennai(),
            subtitle: l10n.place_scooters(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_chettinad_silk_maker.png"),
            cache_key: "places/india_chettinad_silk_maker.png",
            title: l10n.place_chettinad(),
            subtitle: l10n.place_silk_maker(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_chettinad_produce.png"),
            cache_key: "places/india_chettinad_produce.png",
            title: l10n.place_chettinad(),
            subtitle: l10n.place_lunch_prep(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_tanjore_market_technology.png"),
            cache_key: "places/india_tanjore_market_technology.png",
            title: l10n.place_tanjore(),
            subtitle: l10n.place_market(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_pondicherry_beach.png"),
            cache_key: "places/india_pondicherry_beach.png",
            title: l10n.place_pondicherry(),
            subtitle: l10n.place_beach(),
        },
        Photo {
            asset: include_bytes!("../../../assets/places/india_pondicherry_fisherman.png"),
            cache_key: "places/india_pondicherry_fisherman.png",
            title: l10n.place_pondicherry(),
            subtitle: l10n.place_fisherman(),
        },
    ]
}

/// The demo body for the `grid-lists` slug: upstream's `GridListDemo` for the
/// configuration the page's options section selected.
pub(super) fn grid_lists(config: usize) -> AnyWidget {
    stateful(GridListDemo {
        style: style_for(config),
    })
}

/// How tall the grid's viewport is. See the module header: upstream's grid
/// fills the demo body; here it gets the height the card would have given it
/// -- the window less the chrome and the demo's own app bar -- so the demo
/// reads as a screen and still scrolls.
fn grid_height(context: &BuildContext) -> f32 {
    (size_of(context).height - CHROME_HEIGHT - K_TOOLBAR_HEIGHT).max(200.0)
}

/// Upstream's `GridListDemo`: the app bar over the `GridView.count`. The
/// grid's scroll offset lives here (upstream's `restorationId:
/// 'grid_view_demo_grid_offset'` position).
struct GridListDemo {
    style: GridListDemoType,
}

/// The grid's scroll offset.
#[derive(Default)]
struct GridListDemoState {
    scroll: Scroll,
}

impl StatefulComponent for GridListDemo {
    type State = GridListDemoState;

    fn advance(&self, state: &mut Self::State, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &Self::State,
        handle: StateHandle<Self::State>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let style = self.style;
        let placeholder = theme_of(context).surface_variant;
        let height = grid_height(context);
        let photos = Rc::new(photos());
        // Ask for every decode up front: the grid builds its tiles when the
        // render tree is laid out, which is after a headless render's one
        // `wait_for_images` (main.rs), so a tile asking there would never get
        // a second build. Asking here queues the decodes with the first build.
        for photo in photos.iter() {
            let _ = Image::shared(photo.cache_key, photo.asset);
        }
        let offset = state.scroll.offset;
        let extent = state.scroll.extent.clone();

        // The handlers app::scroll_handlers gives a page, on the grid's own
        // Scroll: touch it, drag it, throw it, or turn the wheel over it. The
        // grid's region is the innermost one on the hit path that wants
        // drags, so the grid -- not the page behind it -- moves.
        let handlers = {
            let down = handle.clone();
            let drag = handle.clone();
            let fling = handle.clone();
            PointerHandlers::new()
                .with_pointer_down(move |_| {
                    down.set_state(|s| s.scroll.stop());
                })
                .with_drag_update(move |event| {
                    drag.set_state(move |s| s.scroll.scroll_by(-event.delta.dy));
                })
                .with_drag_end(move |event| {
                    fling.set_state(move |s| s.scroll.fling(-event.velocity.dy));
                })
                .with_scroll(move |event| {
                    handle.set_state(move |s| s.scroll.scroll_by(event.delta.dy));
                })
        };

        // Upstream's app bar: `AppBar(automaticallyImplyLeading: false,
        // title: Text(demoGridListsTitle))` on the demo theme's app-bar colors
        // (`MaterialDemoThemeData.appBarTheme`) -- primary fill, on-primary
        // title. With no leading, the title sits 16 from the start edge.
        let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
        let title = GalleryLocalizations::en().demo_grid_lists_title();
        let bar = leaf(move || {
            Container::new()
                .with_height(K_TOOLBAR_HEIGHT)
                .with_color(bar_fill)
                .with_padding(EdgeInsets::only(16.0, 0.0, 0.0, 0.0))
                .with_child(Align::new(
                    Alignment::CENTER_LEFT,
                    Text::new(title)
                        .with_size(20.0)
                        .with_weight(500)
                        .with_color(bar_ink),
                ))
        });

        // Upstream's `GridView.count(restorationId: 'grid_view_demo_grid_offset',
        // crossAxisCount: 2, mainAxisSpacing: 8, crossAxisSpacing: 8,
        // padding: EdgeInsets.all(8), childAspectRatio: 1)`.
        let grid = GridView::count(2, photos.len(), move |index| {
            RenderRef::new(build_tile(&photos[index], style, placeholder))
        })
        .with_main_axis_spacing(8.0)
        .with_cross_axis_spacing(8.0)
        .with_padding(EdgeInsets::all(8.0))
        .with_child_aspect_ratio(1.0)
        .with_offset(offset)
        .with_extent_sink(extent);

        let grid = single(rustflutter::framework::component(grid), move |rendered| {
            Box::new(Container::new().with_height(height).with_child(
                Pointer::new(ids::DEMO_LOCAL, rendered).with_handlers(handlers.clone()),
            ))
        });

        many(vec![bar, grid], |mut rendered| {
            let grid = rendered.pop().expect("the grid");
            let bar = rendered.pop().expect("the app bar");
            Box::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(bar)
                    .push(grid),
            )
        })
    }
}

/// One tile: upstream's `_GridDemoPhotoItem`. The photograph decodes on a
/// worker (`Image::shared`), so a tile whose picture has not landed yet draws
/// the placeholder upstream's first frame would have drawn over.
fn build_tile(photo: &Photo, style: GridListDemoType, placeholder: Color) -> RenderRef {
    // Upstream: `Material(shape: RoundedRectangleBorder(borderRadius:
    // BorderRadius.circular(4)), clipBehavior: Clip.antiAlias, child:
    // Image.asset(photo.assetName, fit: BoxFit.cover))`.
    let picture: RenderRef = match Image::shared(photo.cache_key, photo.asset) {
        Some(image) => rustflutter::widgets::boxed(ImageView::with_fit(image, BoxFit::Cover)),
        None => rustflutter::widgets::boxed(Container::new().with_color(placeholder)),
    };
    let image = ClipRRect::new(4.0, picture);

    match style {
        GridListDemoType::ImageOnly => rustflutter::widgets::boxed(image),
        GridListDemoType::Header => rustflutter::widgets::boxed(
            Stack::new()
                .push_positioned(image, Positioned::fill())
                // The header bar: upstream's `GridTile(header: Material(...,
                // child: GridTileBar(title: ..., backgroundColor:
                // Colors.black45)))`. Its rounded top corners are the whole
                // bar clipped at 4 here -- `ClipRRect` takes one radius.
                .push_positioned(
                    ClipRRect::new(4.0, tile_bar(photo.title, None)),
                    StackPosition {
                        left: Some(0.0),
                        top: Some(0.0),
                        right: Some(0.0),
                        ..Default::default()
                    },
                ),
        ),
        GridListDemoType::Footer => rustflutter::widgets::boxed(
            Stack::new()
                .push_positioned(image, Positioned::fill())
                .push_positioned(
                    ClipRRect::new(4.0, tile_bar(photo.title, Some(photo.subtitle))),
                    StackPosition {
                        left: Some(0.0),
                        right: Some(0.0),
                        bottom: Some(0.0),
                        ..Default::default()
                    },
                ),
        ),
    }
}

/// Upstream's `GridTileBar`: a black45 strip with the title (and subtitle)
/// in white, padded 16 from the sides.
fn tile_bar(
    title: &'static str,
    subtitle: Option<&'static str>,
) -> impl rustflutter::render::RenderBox {
    let height = if subtitle.is_some() {
        FOOTER_BAR_HEIGHT
    } else {
        HEADER_BAR_HEIGHT
    };
    let mut texts = Column::new()
        .with_main_axis_size(rustflutter::render::MainAxisSize::Min)
        .with_cross_axis_alignment(rustflutter::render::CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .push(grid_title_text(title, 14.0));
    if let Some(subtitle) = subtitle {
        texts = texts.push(grid_title_text(subtitle, 12.0));
    }
    Container::new()
        .with_height(height)
        .with_color(BLACK_45)
        .with_padding(EdgeInsets::symmetric(16.0, 0.0))
        .with_child(Align::new(Alignment::CENTER_LEFT, texts))
}

/// Upstream's `_GridTitleText`: the text may shrink to fit its bar, never
/// grow -- `FittedBox(fit: BoxFit.scaleDown, alignment:
/// AlignmentDirectional.centerStart, child: Text(text))`.
fn grid_title_text(text: &'static str, size: f32) -> impl rustflutter::render::RenderBox {
    rustflutter::render::RenderFittedBox::new(
        Text::new(text).with_size(size).with_color(Color::WHITE),
    )
    .with_fit(BoxFit::ScaleDown)
    .with_alignment(Alignment::CENTER_LEFT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_photo_list_is_upstreams() {
        // Twelve photos, in upstream's order, with upstream's titles.
        let photos = photos();
        assert_eq!(photos.len(), 12);
        let pairs: Vec<(&str, &str)> = photos.iter().map(|p| (p.title, p.subtitle)).collect();
        assert_eq!(
            pairs,
            vec![
                ("Chennai", "Flower Market"),
                ("Tanjore", "Bronze Works"),
                ("Tanjore", "Market"),
                ("Tanjore", "Thanjavur Temple"),
                ("Tanjore", "Thanjavur Temple"),
                ("Pondicherry", "Salt Farm"),
                ("Chennai", "Scooters"),
                ("Chettinad", "Silk Maker"),
                ("Chettinad", "Lunch Prep"),
                ("Tanjore", "Market"),
                ("Pondicherry", "Beach"),
                ("Pondicherry", "Fisherman"),
            ]
        );
    }

    #[test]
    fn the_configurations_pick_the_style() {
        // In `Demo::configurations`'s order: image only, header, footer.
        assert_eq!(style_for(0), GridListDemoType::ImageOnly);
        assert_eq!(style_for(1), GridListDemoType::Header);
        assert_eq!(style_for(2), GridListDemoType::Footer);
        // An out-of-range index is the first configuration, as an upstream
        // restore of a dropped index would be.
        assert_eq!(style_for(3), GridListDemoType::ImageOnly);
        assert_eq!(
            crate::data::demos::find("grid-lists")
                .expect("the grid-lists demo")
                .configurations(),
            &["Image only", "With header", "With footer"]
        );
    }

    #[test]
    fn the_assets_are_pngs() {
        for photo in photos() {
            assert_eq!(&photo.asset[..4], b"\x89PNG", "{}", photo.cache_key);
        }
    }

    #[test]
    fn the_assets_decode() {
        for photo in photos() {
            let image = Image::shared_now(photo.cache_key, photo.asset);
            assert!(image.is_some(), "{}", photo.cache_key);
        }
    }

    #[test]
    fn the_bar_heights_are_upstreams() {
        // `GridTileBar.preferredSize`: 48 with a title, 68 with a subtitle.
        assert_eq!(HEADER_BAR_HEIGHT, 48.0);
        assert_eq!(FOOTER_BAR_HEIGHT, 68.0);
    }
}
