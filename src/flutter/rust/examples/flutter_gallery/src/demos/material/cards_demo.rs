// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/cards_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! `CardsDemo` is one configuration upstream and one here: the three travel
//! destination cards (standard, tappable, selectable) in a list, with the
//! selectable card's long-press selection held by the demo's own state, like
//! upstream's `_CardsDemoState`.
//!
//! Divergences, each commented at its site as well:
//!
//! * The header photos (`places/*.png` from the `flutter_gallery_assets`
//!   package) are not shipped with this port, so the 184-pixel image area is a
//!   flat placeholder and the ink splash lands on the card's own colour rather
//!   than on `Ink.image`.
//! * The framework's `Card` pads and borders its child, so the card face is
//!   drawn here (surface fill, 4-pixel corners, elevation 1, the content
//!   clipped to them) to keep the image flush with the edges the way
//!   upstream's `clipBehavior: Clip.antiAlias` does.
//! * The soft-wrap/ellipsis text behaviour of the description block
//!   (`DefaultTextStyle(softWrap: false, overflow: ellipsis)`) is not carried;
//!   the strings here are short enough that nothing clips at the demo's width.
//! * Restoration (`RestorationMixin` on `_CardsDemoState`) is not carried:
//!   nothing here restores.

use std::rc::Rc;

use rustflutter::framework::{component, leaf, many, single, stateful, BuildContext, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxFit, CrossAxisAlignment, MainAxisSize, RenderFlex, RenderStack, StackPosition,
};
use rustflutter::semantics::SemanticsProperties;
use rustflutter::widgets::{ClipRRect, FittedBox, FullWidth, Padding, Pointer};

use crate::app::ids;
use crate::data::demos as catalog;

use super::column;

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters).
const SHARE_BUTTON: u64 = ids::DEMO_LOCAL;
const EXPLORE_BUTTON: u64 = ids::DEMO_LOCAL + 1;
const TAPPABLE_INK: u64 = ids::DEMO_LOCAL + 2;
const SELECTABLE_CARD: u64 = ids::DEMO_LOCAL + 3;
const SELECTABLE_INK: u64 = ids::DEMO_LOCAL + 4;

/// `Icons.check_circle`, the selectable card's marker, in the MATERIAL_ICONS
/// family the app registers (`data/demos.rs`). The shipped font build is newer
/// than the codepoints upstream's `Icons` class names, so this is the font's
/// own `check_circle_baseline` entry, the same convention as `data/demos.rs`'s
/// icon table.
const CHECK_CIRCLE: &str = "\u{e159}";

/// The demo body for the `card` slug: upstream's `CardsDemo`, a stateful
/// widget, so one here too.
pub(super) fn cards() -> AnyWidget {
    stateful(CardsDemo)
}

/// Upstream's `CardsDemo`.
struct CardsDemo;

/// Upstream's `_CardsDemoState`: the selectable card's selection.
#[derive(Default)]
struct CardsDemoState {
    /// Upstream's `_isSelected`.
    selected: bool,
    /// Which of the standard card's buttons is held.
    pressed: Option<u64>,
}

/// Upstream's `onPressed: () {}` on the share and explore buttons.
fn noop(_state: &mut CardsDemoState) {}

impl StatefulComponent for CardsDemo {
    type State = CardsDemoState;

    fn build(
        &self,
        state: &CardsDemoState,
        handle: StateHandle<CardsDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        // Upstream's `ListView` body: the stage's page already scrolls
        // (`pages/demo.rs`), so the list is the stage's column. The list's
        // own padding (8 on the top and sides) and each item's bottom margin
        // (8) are the column's spacing here; the stage padding stands in for
        // the list's.
        let children = destinations()
            .iter()
            .map(|destination| destination_item(*destination, state, handle.clone(), &theme))
            .collect();
        column(children, 8.0)
    }
}

// -- The data (upstream's `TravelDestination` and `destinations`) --------------

/// Upstream's `CardType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardType {
    Standard,
    Tappable,
    Selectable,
}

/// Upstream's `TravelDestination`, minus the asset package (always
/// `flutter_gallery_assets` upstream, and not shipped here).
#[derive(Clone, Copy)]
struct TravelDestination {
    /// Carried like upstream's, but not read: the `flutter_gallery_assets`
    /// package is not shipped, so the image area is a placeholder (see the
    /// module header).
    #[allow(dead_code)]
    asset_name: &'static str,
    title: &'static str,
    description: &'static str,
    city: &'static str,
    location: &'static str,
    card_type: CardType,
}

/// Upstream's `destinations(context)`.
fn destinations() -> [TravelDestination; 3] {
    [
        TravelDestination {
            asset_name: "places/india_thanjavur_market.png",
            title: "Top 10 Cities to Visit in Tamil Nadu",
            description: "Number 10",
            city: "Thanjavur",
            location: "Thanjavur, Tamil Nadu",
            card_type: CardType::Standard,
        },
        TravelDestination {
            asset_name: "places/india_chettinad_silk_maker.png",
            title: "Artisans of Southern India",
            description: "Silk Spinners",
            city: "Chettinad",
            location: "Sivaganga, Tamil Nadu",
            card_type: CardType::Tappable,
        },
        TravelDestination {
            asset_name: "places/india_tanjore_thanjavur_temple.png",
            title: "Brihadisvara Temple",
            description: "Temples",
            city: "Thanjavur",
            location: "Thanjavur, Tamil Nadu",
            card_type: CardType::Selectable,
        },
    ]
}

// -- The cards -----------------------------------------------------------------

/// Upstream's `TravelDestinationItem.height`.
const STANDARD_HEIGHT: f32 = 360.0;
/// Upstream's `TappableTravelDestinationItem.height` and
/// `SelectableTravelDestinationItem.height`.
const TAPPABLE_HEIGHT: f32 = 298.0;
const SELECTABLE_HEIGHT: f32 = 298.0;
/// The image area's height in `TravelDestinationContent`.
const IMAGE_HEIGHT: f32 = 184.0;

fn card_height(card_type: CardType) -> f32 {
    match card_type {
        CardType::Standard => STANDARD_HEIGHT,
        CardType::Tappable => TAPPABLE_HEIGHT,
        CardType::Selectable => SELECTABLE_HEIGHT,
    }
}

/// One list entry: the section title above the fixed-height card, inside the
/// item's all-around padding of 8 (upstream's `Padding(padding:
/// EdgeInsets.all(8), ...)` in all three item widgets).
fn destination_item(
    destination: TravelDestination,
    state: &CardsDemoState,
    handle: StateHandle<CardsDemoState>,
    theme: &Rc<Theme>,
) -> AnyWidget {
    // The section titles: upstream's `SectionTitle`s. The standard card's is
    // `settingsTextScalingNormal` ("Normal") -- an upstream quirk, kept.
    let section = match destination.card_type {
        CardType::Standard => "Normal",
        CardType::Tappable => "Tappable",
        CardType::Selectable => "Selectable (long press)",
    };
    many(
        vec![
            section_title(section, theme),
            destination_card(destination, state, handle, theme),
        ],
        |mut rendered| {
            let card = rendered.pop().expect("two children");
            let title = rendered.pop().expect("two children");
            Box::new(Padding::all(
                8.0,
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .push(title)
                    .push(card),
            ))
        },
    )
}

/// Upstream's `SectionTitle`: the title medium, left-aligned, padded 4/4/4/12.
fn section_title(title: &'static str, theme: &Theme) -> AnyWidget {
    // `textTheme.titleMedium` under `Typography.material2018` is subtitle1:
    // 16 pixels, regular weight.
    let style = TextStyle {
        font_size: 16.0,
        color: theme.text,
        ..TextStyle::default()
    };
    leaf(move || {
        Container::new()
            .with_padding(EdgeInsets::only(4.0, 4.0, 4.0, 12.0))
            .with_child(rustflutter::widgets::Align::new(
                Alignment::CENTER_LEFT,
                Text::new(title).with_style(style.clone()),
            ))
    })
}

/// The card itself: upstream's `Card(clipBehavior: Clip.antiAlias)` around
/// `TravelDestinationContent`, plus the tappable and selectable overlays.
fn destination_card(
    destination: TravelDestination,
    state: &CardsDemoState,
    handle: StateHandle<CardsDemoState>,
    theme: &Rc<Theme>,
) -> AnyWidget {
    let height = card_height(destination.card_type);
    // Upstream's card colour under the demo theme (`colorScheme.surface`),
    // corner radius 4, elevation 1.
    let surface = theme.surface;
    let selected = state.selected;
    let pressed = state.pressed;
    let primary = theme.primary;
    // The splash colour is on-surface at 12% with no highlight, as upstream's
    // comments prescribe for cards.
    let splash = theme.text.with_alpha(0x1F);

    // The card face is a builder rather than a widget because the ink variants
    // rebuild it on every splash frame (a stateful component is built from the
    // same widget instance -- see `Ink`).
    let face_theme = Rc::clone(theme);
    let face_handle = handle.clone();
    let face = move || {
        let content = destination_content(destination, pressed, face_handle.clone(), &face_theme);
        single(content, move |content| {
            let content: rustflutter::render::BoxedRender = match destination.card_type {
                CardType::Selectable => {
                    // Upstream's `Stack`: the 8% primary tint under the
                    // content when selected, the check circle over it at the
                    // top right. An unselected card draws neither -- upstream
                    // draws them transparent, which is the same pixels.
                    let mut stack = RenderStack::new();
                    if selected {
                        stack = stack.push_positioned(
                            Container::new().with_color(primary.with_alpha(0x14)),
                            StackPosition::fill(),
                        );
                    }
                    stack = stack.push_boxed(content);
                    if selected {
                        stack = stack.push_positioned(
                            Padding::all(
                                8.0,
                                Text::new(CHECK_CIRCLE)
                                    .with_font_family(catalog::MATERIAL_ICONS)
                                    .with_size(24.0)
                                    .with_color(primary),
                            ),
                            StackPosition {
                                top: Some(0.0),
                                right: Some(0.0),
                                ..Default::default()
                            },
                        );
                    }
                    rustflutter::render::RenderRef::new(stack)
                }
                _ => content,
            };
            Box::new(FullWidth::new(
                Container::new()
                    .with_height(height)
                    .with_color(surface)
                    .with_corner_radius(4.0)
                    .with_elevation(1)
                    .with_child(ClipRRect::new(4.0, content)),
            ))
        })
    };

    match destination.card_type {
        // Upstream: the content in a `Semantics(label:)`, no ink of its own.
        CardType::Standard => {
            rustflutter::semantics::describe(SemanticsProperties::label(destination.title), face())
        }
        // `InkWell(onTap: () {})`: it splashes and nothing more.
        CardType::Tappable => rustflutter::semantics::describe(
            SemanticsProperties::label(destination.title),
            stateful(Ink::new(TAPPABLE_INK, move || face()).with_color(splash)),
        ),
        // `InkWell(onLongPress: onSelected, ...)`: a long press toggles the
        // selection.
        CardType::Selectable => {
            let semantics_label = if state.selected {
                format!("{}, Selected", destination.title)
            } else {
                format!("{}, Not selected", destination.title)
            };
            rustflutter::semantics::describe(
                SemanticsProperties::label(semantics_label),
                stateful(
                    Ink::new(SELECTABLE_INK, move || {
                        let long_press_handle = handle.clone();
                        single(face(), move |card| {
                            let long_press = long_press_handle.clone();
                            Box::new(Pointer::new(SELECTABLE_CARD, card).with_handlers(
                                PointerHandlers::new().with_long_press(move |_| {
                                    long_press.set_state(|s| s.selected = !s.selected);
                                }),
                            ))
                        })
                    })
                    .with_color(splash),
                ),
            )
        }
    }
}

/// Upstream's `TravelDestinationContent`: the 184-pixel image with the title
/// over its bottom, then the three-line description block, then -- for the
/// standard card only -- the share and explore buttons.
fn destination_content(
    destination: TravelDestination,
    pressed: Option<u64>,
    handle: StateHandle<CardsDemoState>,
    theme: &Theme,
) -> AnyWidget {
    // The header image. Upstream: `Ink.image(AssetImage(destination.assetName,
    // package: 'flutter_gallery_assets'), fit: BoxFit.cover)` -- and the
    // `flutter_gallery_assets` package is not shipped with this port, so the
    // area is a flat slate the white title still reads against.
    let title_style = TextStyle {
        // `headlineSmall` under `Typography.material2018` is headline6:
        // 20 pixels, regular weight, in white over the image.
        font_size: 20.0,
        color: Color::WHITE,
        ..TextStyle::default()
    };
    let title = destination.title;
    let image_area: AnyWidget = leaf(move || {
        Container::new()
            .with_height(IMAGE_HEIGHT)
            .with_color(Color::rgb(0x60, 0x72, 0x85))
            // `Positioned(bottom: 16, left: 16, right: 16, child:
            // FittedBox(fit: scaleDown, alignment: centerLeft, ...))`.
            .with_alignment(Alignment::BOTTOM_LEFT)
            .with_child(Padding::all(
                16.0,
                FittedBox::new(Text::new(title).with_style(title_style.clone()))
                    .with_fit(BoxFit::ScaleDown)
                    .with_alignment(Alignment::CENTER_LEFT),
            ))
    });

    // The description block: `Padding(fromLTRB(16, 16, 16, 0))` around the
    // three lines, the first in black54 with 8 below it. `titleMedium` is
    // 16 pixels under the 2018 typography.
    let description_style = TextStyle {
        font_size: 16.0,
        color: theme.text,
        ..TextStyle::default()
    };
    let muted_style = TextStyle {
        color: theme.text.with_alpha(0x8A), // Colors.black54
        ..description_style.clone()
    };
    let description = destination.description;
    let city = destination.city;
    let location = destination.location;
    let text_block: AnyWidget = leaf(move || {
        Container::new()
            .with_padding(EdgeInsets::only(16.0, 16.0, 16.0, 0.0))
            .with_child(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .push(
                        Container::new()
                            .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, 8.0))
                            .with_child(Text::new(description).with_style(muted_style.clone())),
                    )
                    .push(Text::new(city).with_style(description_style.clone()))
                    .push(Text::new(location).with_style(description_style.clone())),
            )
    });

    let mut children = vec![image_area, text_block];
    if destination.card_type == CardType::Standard {
        // The `OverflowBar(alignment: start, spacing: 8)` with the share and
        // explore text buttons, padded 8.
        children.push(single(
            super::row(
                vec![
                    component(
                        Button::new(SHARE_BUTTON, "Share")
                            .with_style(ButtonVariant::Text)
                            .with_pressed(pressed == Some(SHARE_BUTTON))
                            .wired(handle.clone(), |s| &mut s.pressed, noop),
                    ),
                    component(
                        Button::new(EXPLORE_BUTTON, "Explore")
                            .with_style(ButtonVariant::Text)
                            .with_pressed(pressed == Some(EXPLORE_BUTTON))
                            .wired(handle.clone(), |s| &mut s.pressed, noop),
                    ),
                ],
                8.0,
            ),
            |row| {
                Box::new(
                    Container::new()
                        .with_padding(EdgeInsets::all(8.0))
                        .with_child(row),
                )
            },
        ));
    }

    many(children, |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(column)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_destinations_are_upstreams() {
        let destinations = destinations();
        assert_eq!(destinations.len(), 3);
        assert_eq!(
            destinations[0].title,
            "Top 10 Cities to Visit in Tamil Nadu"
        );
        assert_eq!(destinations[0].card_type, CardType::Standard);
        assert_eq!(destinations[1].title, "Artisans of Southern India");
        assert_eq!(destinations[1].description, "Silk Spinners");
        assert_eq!(destinations[1].city, "Chettinad");
        assert_eq!(destinations[1].card_type, CardType::Tappable);
        assert_eq!(destinations[2].title, "Brihadisvara Temple");
        assert_eq!(destinations[2].location, "Thanjavur, Tamil Nadu");
        assert_eq!(destinations[2].card_type, CardType::Selectable);
    }

    #[test]
    fn the_card_heights_are_upstreams() {
        // `TravelDestinationItem.height` is 360; the tappable and selectable
        // items are 298.
        assert_eq!(card_height(CardType::Standard), 360.0);
        assert_eq!(card_height(CardType::Tappable), 298.0);
        assert_eq!(card_height(CardType::Selectable), 298.0);
        assert_eq!(IMAGE_HEIGHT, 184.0);
    }

    #[test]
    fn nothing_starts_selected() {
        // Upstream's `RestorableBool(false)`.
        assert!(!CardsDemoState::default().selected);
    }
}
