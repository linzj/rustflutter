// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/colors_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `ColorsDemo` is the Material 2014 palette as a swatch book: a
//! scrollable `TabBar` of the nineteen palette names over a `TabBarView`
//! whose pages (`_PaletteTabView`) list each palette's shades, one
//! `_ColorItem` row apiece -- the shade index on the left, the `#AARRGGBB`
//! value on the right, the label black or white by the palette's
//! `threshold`.
//!
//! Divergences, each also marked at its site:
//!
//! * The `Scaffold`/`AppBar` (title `demoColorsTitle`) is the demo page's own
//!   chrome (`src/pages/demo.rs`); the stage starts at the tab strip. The
//!   strip keeps the app bar's look -- the tabs sit on `primary` with
//!   `onPrimary` labels, upstream's `labelColor`.
//! * Upstream's `TabBar(isScrollable: true)` is a horizontal `ListView`
//!   here, as in the tabs demo: the framework's `TabBar` lays every tab out
//!   to an equal share of the width, unreadable at nineteen tabs.
//! * Each palette's `ListView` is height-bounded ([`PALETTE_HEIGHT`]) rather
//!   than filling the demo screen, and keyed by the palette so a tab switch
//!   starts the new palette at its top -- upstream gives each tab its own
//!   scrollable, and a fresh one starts unscrolled.
//! * The palette names resolve through the English localizations only, the
//!   port's standing l10n rule.

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, ListView, Pointer};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::column;

/// Upstream's `kColorItemHeight`.
const COLOR_ITEM_HEIGHT: f32 = 48.0;

/// How tall the palette's shade list is. Upstream the `TabBarView` fills the
/// demo screen; here the stage does not guarantee a bounded height, so the
/// list gets a fixed window it scrolls inside -- ten rows of
/// [`COLOR_ITEM_HEIGHT`].
const PALETTE_HEIGHT: f32 = COLOR_ITEM_HEIGHT * 10.0;

/// Upstream's `_PaletteTabView.primaryKeys`.
const PRIMARY_KEYS: [i32; 10] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900];
/// Upstream's `_PaletteTabView.accentKeys`.
const ACCENT_KEYS: [i32; 4] = [100, 200, 400, 700];

/// Upstream's `_Palette`: one row of the Material 2014 palette -- ten
/// primary shades and, for the hues that have them, four accent shades.
struct Palette {
    name: String,
    /// The shades at `PRIMARY_KEYS`, upstream's `MaterialColor`.
    primary: [Color; 10],
    /// The shades at `ACCENT_KEYS`, upstream's `MaterialAccentColor`.
    accent: Option<[Color; 4]>,
    /// Upstream's `threshold`: titles for indices above it are white,
    /// otherwise black.
    threshold: i32,
}

/// The shades of one `MaterialColor`, lightest to darkest.
macro_rules! swatch {
    ($($hex:literal),+) => {
        [$(Color(0xFF00_0000 | $hex)),+]
    };
}

/// Upstream's `_allPalettes`, in upstream's order, with upstream's
/// thresholds. The values are the Material 2014 palette, the same constants
/// upstream's `Colors.red` and friends carry.
fn all_palettes() -> Vec<Palette> {
    let l10n = GalleryLocalizations::en();
    let name = |f: fn(&GalleryLocalizations) -> &'static str| f(&l10n).to_string();
    vec![
        Palette {
            name: name(|l| l.colors_red()),
            primary: swatch!(
                0xFFEBEE, 0xFFCDD2, 0xEF9A9A, 0xE57373, 0xEF5350, 0xF44336, 0xE53935, 0xD32F2F,
                0xC62828, 0xB71C1C
            ),
            accent: Some(swatch!(0xFF8A80, 0xFF5252, 0xFF1744, 0xD50000)),
            threshold: 300,
        },
        Palette {
            name: name(|l| l.colors_pink()),
            primary: swatch!(
                0xFCE4EC, 0xF8BBD0, 0xF48FB1, 0xF06292, 0xEC407A, 0xE91E63, 0xD81B60, 0xC2185B,
                0xAD1457, 0x880E4F
            ),
            accent: Some(swatch!(0xFF80AB, 0xFF4081, 0xF50057, 0xC51162)),
            threshold: 200,
        },
        Palette {
            name: name(|l| l.colors_purple()),
            primary: swatch!(
                0xF3E5F5, 0xE1BEE7, 0xCE93D8, 0xBA68C8, 0xAB47BC, 0x9C27B0, 0x8E24AA, 0x7B1FA2,
                0x6A1B9A, 0x4A148C
            ),
            accent: Some(swatch!(0xEA80FC, 0xE040FB, 0xD500F9, 0xAA00FF)),
            threshold: 200,
        },
        Palette {
            name: name(|l| l.colors_deep_purple()),
            primary: swatch!(
                0xEDE7F6, 0xD1C4E9, 0xB39DDB, 0x9575CD, 0x7E57C2, 0x673AB7, 0x5E35B1, 0x512DA8,
                0x4527A0, 0x311B92
            ),
            accent: Some(swatch!(0xB388FF, 0x7C4DFF, 0x651FFF, 0x6200EA)),
            threshold: 200,
        },
        Palette {
            name: name(|l| l.colors_indigo()),
            primary: swatch!(
                0xE8EAF6, 0xC5CAE9, 0x9FA8DA, 0x7986CB, 0x5C6BC0, 0x3F51B5, 0x3949AB, 0x303F9F,
                0x283593, 0x1A237E
            ),
            accent: Some(swatch!(0x8C9EFF, 0x536DFE, 0x3D5AFE, 0x304FFE)),
            threshold: 200,
        },
        Palette {
            name: name(|l| l.colors_blue()),
            primary: swatch!(
                0xE3F2FD, 0xBBDEFB, 0x90CAF9, 0x64B5F6, 0x42A5F5, 0x2196F3, 0x1E88E5, 0x1976D2,
                0x1565C0, 0x0D47A1
            ),
            accent: Some(swatch!(0x82B1FF, 0x448AFF, 0x2979FF, 0x2962FF)),
            threshold: 400,
        },
        Palette {
            name: name(|l| l.colors_light_blue()),
            primary: swatch!(
                0xE1F5FE, 0xB3E5FC, 0x81D4FA, 0x4FC3F7, 0x29B6F6, 0x03A9F4, 0x039BE5, 0x0288D1,
                0x0277BD, 0x01579B
            ),
            accent: Some(swatch!(0x80D8FF, 0x40C4FF, 0x00B0FF, 0x0091EA)),
            threshold: 500,
        },
        Palette {
            name: name(|l| l.colors_cyan()),
            primary: swatch!(
                0xE0F7FA, 0xB2EBF2, 0x80DEEA, 0x4DD0E1, 0x26C6DA, 0x00BCD4, 0x00ACC1, 0x0097A7,
                0x00838F, 0x006064
            ),
            accent: Some(swatch!(0x84FFFF, 0x18FFFF, 0x00E5FF, 0x00B8D4)),
            threshold: 600,
        },
        Palette {
            name: name(|l| l.colors_teal()),
            primary: swatch!(
                0xE0F2F1, 0xB2DFDB, 0x80CBC4, 0x4DB6AC, 0x26A69A, 0x009688, 0x00897B, 0x00796B,
                0x00695C, 0x004D40
            ),
            accent: Some(swatch!(0xA7FFEB, 0x64FFDA, 0x1DE9B6, 0x00BFA5)),
            threshold: 400,
        },
        Palette {
            name: name(|l| l.colors_green()),
            primary: swatch!(
                0xE8F5E9, 0xC8E6C9, 0xA5D6A7, 0x81C784, 0x66BB6A, 0x4CAF50, 0x43A047, 0x388E3C,
                0x2E7D32, 0x1B5E20
            ),
            accent: Some(swatch!(0xB9F6CA, 0x69F0AE, 0x00E676, 0x00C853)),
            threshold: 500,
        },
        Palette {
            name: name(|l| l.colors_light_green()),
            primary: swatch!(
                0xF1F8E9, 0xDCEDC8, 0xC5E1A5, 0xAED581, 0x9CCC65, 0x8BC34A, 0x7CB342, 0x689F38,
                0x558B2F, 0x33691E
            ),
            accent: Some(swatch!(0xCCFF90, 0xB2FF59, 0x76FF03, 0x64DD17)),
            threshold: 600,
        },
        Palette {
            name: name(|l| l.colors_lime()),
            primary: swatch!(
                0xF9FBE7, 0xF0F4C3, 0xE6EE9C, 0xDCE775, 0xD4E157, 0xCDDC39, 0xC0CA33, 0xAFB42B,
                0x9E9D24, 0x827717
            ),
            accent: Some(swatch!(0xF4FF81, 0xEEFF41, 0xC6FF00, 0xAEEA00)),
            threshold: 800,
        },
        Palette {
            name: name(|l| l.colors_yellow()),
            primary: swatch!(
                0xFFFDE7, 0xFFF9C4, 0xFFF59D, 0xFFF176, 0xFFEE58, 0xFFEB3B, 0xFDD835, 0xFBC02D,
                0xF9A825, 0xF57F17
            ),
            accent: Some(swatch!(0xFFFF8D, 0xFFFF00, 0xFFEA00, 0xFFD600)),
            threshold: 900,
        },
        Palette {
            name: name(|l| l.colors_amber()),
            primary: swatch!(
                0xFFF8E1, 0xFFECB3, 0xFFE082, 0xFFD54F, 0xFFCA28, 0xFFC107, 0xFFB300, 0xFFA000,
                0xFF8F00, 0xFF6F00
            ),
            accent: Some(swatch!(0xFFE57F, 0xFFD740, 0xFFC400, 0xFFAB00)),
            threshold: 900,
        },
        Palette {
            name: name(|l| l.colors_orange()),
            primary: swatch!(
                0xFFF3E0, 0xFFE0B2, 0xFFCC80, 0xFFB74D, 0xFFA726, 0xFF9800, 0xFB8C00, 0xF57C00,
                0xEF6C00, 0xE65100
            ),
            accent: Some(swatch!(0xFFD180, 0xFFAB40, 0xFF9100, 0xFF6D00)),
            threshold: 700,
        },
        Palette {
            name: name(|l| l.colors_deep_orange()),
            primary: swatch!(
                0xFBE9E7, 0xFFCCBC, 0xFFAB91, 0xFF8A65, 0xFF7043, 0xFF5722, 0xF4511E, 0xE64A19,
                0xD84315, 0xBF360C
            ),
            accent: Some(swatch!(0xFF9E80, 0xFF6E40, 0xFF3D00, 0xDD2C00)),
            threshold: 400,
        },
        Palette {
            name: name(|l| l.colors_brown()),
            primary: swatch!(
                0xEFEBE9, 0xD7CCC8, 0xBCAAA4, 0xA1887F, 0x8D6E63, 0x795548, 0x6D4C41, 0x5D4037,
                0x4E342E, 0x3E2723
            ),
            accent: None,
            threshold: 200,
        },
        Palette {
            name: name(|l| l.colors_grey()),
            primary: swatch!(
                0xFAFAFA, 0xF5F5F5, 0xEEEEEE, 0xE0E0E0, 0xBDBDBD, 0x9E9E9E, 0x757575, 0x616161,
                0x424242, 0x212121
            ),
            accent: None,
            threshold: 500,
        },
        Palette {
            name: name(|l| l.colors_blue_grey()),
            primary: swatch!(
                0xECEFF1, 0xCFD8DC, 0xB0BEC5, 0x90A4AE, 0x78909C, 0x607D8B, 0x546E7A, 0x455A64,
                0x37474F, 0x263238
            ),
            accent: None,
            threshold: 500,
        },
    ]
}

/// The demo body for the `colors` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(ColorsDemo)
}

/// Upstream's `ColorsDemo`: the tab strip over the selected palette.
struct ColorsDemo;

/// What the demo remembers: the `DefaultTabController`'s index, and the tab
/// strip's own scroll position (see the header for why the strip is a
/// `ListView`).
#[derive(Default)]
struct ColorsDemoState {
    selected: usize,
    strip: Scroll,
}

impl StatefulComponent for ColorsDemo {
    type State = ColorsDemoState;

    fn advance(&self, state: &mut ColorsDemoState, frame_time_micros: i64) -> bool {
        // A fling on the strip plays out on the frame clock.
        state.strip.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &ColorsDemoState,
        handle: StateHandle<ColorsDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let palettes = all_palettes();
        let selected = state.selected.min(palettes.len() - 1);

        column(
            vec![
                strip(state, handle, context, &palettes),
                // Upstream's `TabBarView`: the selected palette's shades.
                // Keyed by the tab so a switch replaces the element and its
                // scroll state, starting the new palette at its top.
                palette_view(selected, &palettes[selected]),
            ],
            0.0,
        )
    }
}

/// The palette names, upstream's `TabBar(isScrollable: true)`.
///
/// The strip keeps its upstream home's look: the tabs sit on the app bar's
/// `primary` fill, the selected label and its indicator are `onPrimary`
/// (upstream's `labelColor`; the indicator is upstream's
/// `TabBarTheme.indicatorColor`, the demo theme's `onPrimary`), the rest a
/// quieter `onPrimary`.
fn strip(
    state: &ColorsDemoState,
    handle: StateHandle<ColorsDemoState>,
    context: &mut BuildContext,
    palettes: &[Palette],
) -> AnyWidget {
    let theme = theme_of(context);
    let selected = state.selected;
    let offset = state.strip.offset;
    let extent = state.strip.link();
    let bar = theme.primary;
    let label = theme.on_primary;
    let unselected = theme.on_primary.with_alpha(0x99);
    let size = theme.body_size;

    let tabs: Vec<AnyWidget> = palettes
        .iter()
        .enumerate()
        .map(|(index, palette)| {
            let active = index == selected;
            let name = palette.name.clone();
            let tap = PointerHandlers::new().with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(move |s| s.selected = index);
                }
            });
            leaf(move || {
                // The indicator is a positioned layer at the tab's bottom
                // edge rather than a flex child, as in the tabs demo's strip.
                let indicator = rustflutter::widgets::Container::new()
                    .with_height(2.0)
                    .with_color(if active { label } else { Color::TRANSPARENT });
                Pointer::new(
                    ids::DEMO_LOCAL + 10 + index as u64,
                    rustflutter::widgets::Container::new()
                        .with_height(46.0)
                        .with_padding(EdgeInsets::symmetric(16.0, 0.0))
                        .with_child(
                            rustflutter::render::RenderStack::new()
                                .push(Align::new(
                                    Alignment::CENTER,
                                    Text::new(name.clone())
                                        .with_size(size)
                                        .with_weight(if active { 700 } else { 500 })
                                        .with_color(if active { label } else { unselected }),
                                ))
                                .push_positioned(
                                    indicator,
                                    rustflutter::render::StackPosition {
                                        left: Some(0.0),
                                        right: Some(0.0),
                                        bottom: Some(0.0),
                                        ..Default::default()
                                    },
                                ),
                        ),
                )
                .with_handlers(tap.clone())
            })
        })
        .collect();

    // The strip's drag and wheel, the horizontal case of the list demo's
    // scroll wiring.
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle;
    let handlers = PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(|s| s.strip.stop());
        })
        .with_drag_update(move |drag| {
            let delta = drag.delta.dx;
            drag_handle.set_state(move |s| s.strip.scroll_by(-delta));
        })
        .with_drag_end(move |end| {
            let velocity = end.velocity.dx;
            end_handle.set_state(move |s| s.strip.fling(-velocity));
        })
        // A wheel is a vertical delta even over a horizontal strip;
        // upstream's scrollables map it onto the scroll axis, so this does
        // too.
        .with_scroll(move |scroll| {
            let along = scroll.delta.dy;
            wheel_handle.set_state(move |s| s.strip.scroll_by(along));
        });

    many(tabs, move |rendered| {
        let mut list = ListView::horizontal()
            .with_offset(offset)
            .with_link(extent.clone());
        for tab in rendered {
            list = list.push(tab);
        }
        Box::new(
            rustflutter::widgets::Container::new()
                .with_color(bar)
                .with_child(Pointer::new(ids::DEMO_LOCAL, list).with_handlers(handlers.clone())),
        )
    })
}

/// One palette's shade list, upstream's `_PaletteTabView`: a `Scrollbar`
/// around a bounded `ListView` of [`COLOR_ITEM_HEIGHT`] rows. Keyed by the
/// tab so a switch replaces the element, and with it the scroll state --
/// upstream's per-tab `ListView`s each start unscrolled.
fn palette_view(index: usize, palette: &Palette) -> AnyWidget {
    // The palette's rows as data: the shade index and the color, primary
    // shades then accents, in upstream's key order.
    let mut rows: Vec<(i32, Color)> = PRIMARY_KEYS.into_iter().zip(palette.primary).collect();
    if let Some(accent) = palette.accent {
        rows.extend(ACCENT_KEYS.into_iter().zip(accent));
    }
    let threshold = palette.threshold;

    let view = scrollbar(move || {
        stateful(PaletteView {
            index,
            rows: rows.clone(),
            threshold,
        })
    });
    single(view, move |inner| {
        Box::new(
            Container::new()
                .with_height(PALETTE_HEIGHT)
                .with_child(inner),
        )
    })
}

/// The scrollable list itself, with its own `Scroll` for a state.
struct PaletteView {
    /// The tab this is the view for; also the element's key (see above).
    index: usize,
    rows: Vec<(i32, Color)>,
    threshold: i32,
}

#[derive(Default)]
struct PaletteViewState {
    scroll: Scroll,
}

impl StatefulComponent for PaletteView {
    type State = PaletteViewState;

    fn key(&self) -> rustflutter::framework::Key {
        Some(self.index as u64)
    }

    fn advance(&self, state: &mut PaletteViewState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &PaletteViewState,
        handle: StateHandle<PaletteViewState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // Dispatched from this element so the `Scrollbar` above hears the
        // list's movements -- upstream's notification bubbling.
        state
            .scroll
            .set_notification_sink(context.notification_sink());
        let offset = state.scroll.offset;
        let extent = state.scroll.link();

        // The same handlers the list demo gives its lists, against this
        // list's own `Scroll`.
        let down_handle = handle.clone();
        let drag_handle = handle.clone();
        let end_handle = handle.clone();
        let wheel_handle = handle;
        let handlers = PointerHandlers::new()
            .with_pointer_down(move |_| {
                down_handle.set_state(|state| state.scroll.stop());
            })
            .with_drag_update(move |drag| {
                let delta = drag.delta.dy;
                drag_handle.set_state(move |state| state.scroll.scroll_by(-delta));
            })
            .with_drag_end(move |end| {
                let velocity = end.velocity.dy;
                end_handle.set_state(move |state| state.scroll.fling(-velocity));
            })
            .with_scroll(move |scroll| {
                let delta = scroll.delta.dy;
                wheel_handle.set_state(move |state| state.scroll.scroll_by(delta));
            });

        let threshold = self.threshold;
        let is_accent = |position: usize| position >= PRIMARY_KEYS.len();
        let items: Vec<AnyWidget> = self
            .rows
            .iter()
            .enumerate()
            .map(|(position, &(key, color))| {
                color_item(
                    key,
                    color,
                    // Upstream's `prefix: 'A'` for the accent shades.
                    if is_accent(position) { "A" } else { "" },
                    threshold,
                )
            })
            .collect();

        many(items, move |rendered| {
            let mut flex = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for item in rendered {
                flex = flex.push(item);
            }
            let list = ListView::new()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(flex);
            Box::new(Pointer::new(ids::DEMO_LOCAL + 1, list).with_handlers(handlers.clone()))
        })
    }
}

/// One shade row, upstream's `_ColorItem`: the shade index on the left, the
/// `#AARRGGBB` value on the right, on the shade itself. The label is white
/// above the palette's threshold, black otherwise -- upstream's
/// `DefaultTextStyle` switch.
fn color_item(key: i32, color: Color, prefix: &'static str, threshold: i32) -> AnyWidget {
    // Upstream's `_colorString`.
    let hex = format!("#{:08X}", color.0);
    let label = format!("{prefix}{key}");
    let ink = if key > threshold {
        Color::WHITE
    } else {
        Color::BLACK
    };
    leaf(move || {
        Container::new()
            .with_height(COLOR_ITEM_HEIGHT)
            .with_color(color)
            .with_padding(EdgeInsets::symmetric(16.0, 0.0))
            .with_child(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(rustflutter::render::MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(Text::new(label.clone()).with_size(14.0).with_color(ink))
                    .push(Text::new(hex.clone()).with_size(14.0).with_color(ink)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palettes_are_upstreams_nineteen() {
        let palettes = all_palettes();
        assert_eq!(palettes.len(), 19);
        // Upstream's accents: every palette but brown, grey and blue grey.
        let without_accent: Vec<&str> = palettes
            .iter()
            .filter(|palette| palette.accent.is_none())
            .map(|palette| palette.name.as_str())
            .collect();
        assert_eq!(without_accent, ["BROWN", "GREY", "BLUE GREY"]);
    }

    #[test]
    fn the_shade_values_are_the_2014_palettes() {
        let palettes = all_palettes();
        // The 500 shade is the one the palette is named for.
        assert_eq!(palettes[0].primary[5], Color(0xFFF44336)); // red
        assert_eq!(palettes[8].primary[5], Color(0xFF009688)); // teal
        assert_eq!(palettes[18].primary[5], Color(0xFF607D8B)); // blue grey
        assert_eq!(palettes[0].accent.unwrap()[1], Color(0xFFFF5252)); // red A200
    }
}
