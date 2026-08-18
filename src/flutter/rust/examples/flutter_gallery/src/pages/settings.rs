// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The settings panel.
//!
//! Ported from `lib/pages/settings.dart` and `lib/pages/settings_list_item.dart`
//! (flutter/gallery @ d12640d) -- one file here, because the list item exists
//! only to serve this page and upstream's split is a dartdoc convenience.
//!
//! The panel is the backdrop's back layer (`pages/backdrop.rs`), not a route.
//! It offers upstream's full list: text scaling, text direction, locale,
//! platform mechanics, theme and the slow-motion toggle, then the about,
//! feedback and attribution links. What each option drives:
//!
//! * **Theme** and **text scaling** are live: they resolve through
//!   `GalleryOptions` and are applied at the root (`app::Gallery::build`).
//! * **Slow motion** is live: `time_dilation` dilates the frame time every
//!   animation is ticked with (`app::Gallery::advance`).
//! * **Text direction**, **locale** and **platform mechanics** are visible
//!   but disabled, with the reason written on the row -- the values resolve
//!   (`data/gallery_options.rs`) but nothing renders with them yet: no RTL
//!   layout, an English-only catalogue, one embedder. See PORTING.md.
//!
//! The expansion animation is upstream's: 150ms, the header's margin, padding
//! and corner radius tween away, the subtitle collapses, the chevron turns
//! half a turn, and the options are clipped to a growing height factor. The
//! controllers live on `app::GalleryState::setting_expand`.

use rustflutter::animation::Curve;
use rustflutter::framework::{component, leaf, many, AnyWidget, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderAlign,
    RenderFlex,
};
use rustflutter::widgets::{Align, ClipRRect, Container, Pointer, Transform};

use crate::app::{self, ids, GalleryState};
use crate::constants::{FIRST_HEADER_DESKTOP_TOP_PADDING, GALLERY_HEADER_HEIGHT};
use crate::data::demos as catalog;
use crate::data::gallery_options::{CustomTextDirection, ThemeMode};
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::pages::backdrop::stagger_interval;
use crate::themes::gallery_theme_data::{text, Scheme};

/// Upstream's `_expandDuration` for a settings item.
pub const EXPAND_DURATION: std::time::Duration = std::time::Duration::from_millis(150);

/// Upstream's `_ExpandableSetting`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpandableSetting {
    Theme,
    TextScale,
    TextDirection,
    Locale,
    Platform,
}

/// Upstream's `_ExpandableSetting.values`, in the order the panel lists them:
/// theme first, platform mechanics last.
pub const EXPANDABLE_SETTINGS: &[ExpandableSetting] = &[
    ExpandableSetting::Theme,
    ExpandableSetting::TextScale,
    ExpandableSetting::TextDirection,
    ExpandableSetting::Locale,
    ExpandableSetting::Platform,
];

/// The settings panel's content: the list upstream's `SettingsPage` builds.
///
/// The stagger is the backdrop panel controller's back half, the way
/// upstream's `_AnimateSettingsListItems` reads the same controller the slide
/// runs on.
pub fn panel(
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
) -> AnyWidget {
    let scheme = state.scheme();
    let l10n = GalleryLocalizations::lookup(&state.options.locale());
    let stagger = stagger_interval(state.backdrop_panel.value());

    let mut children: Vec<AnyWidget> = Vec::new();
    if is_desktop {
        children.push(leaf(move || {
            Container::new().with_size(1.0, FIRST_HEADER_DESKTOP_TOP_PADDING)
        }));
    }
    children.push(component(SettingsHeader { is_desktop, scheme }));

    for (index, setting) in EXPANDABLE_SETTINGS.iter().enumerate() {
        let item: AnyWidget = match setting {
            ExpandableSetting::TextScale => component(SettingsListItem {
                id: ids::SETTINGS_LOCAL + index as u64 * 100,
                title: l10n.settings_text_scaling(),
                subtitle: text_scale_title(state.options.text_scale_factor(true), &l10n),
                scheme,
                progress: state.setting_expand[index].value(),
                options: text_scale_options(&l10n, state.options.text_scale_factor(true)),
                pressed: state.pressed,
                enabled: true,
                reason: None,
                handle: handle.clone(),
                select: |s: &mut GalleryState, which: usize| {
                    s.options = s
                        .options
                        .clone()
                        .with_text_scale_factor(TEXT_SCALE_VALUES[which]);
                },
            }),
            ExpandableSetting::Theme => component(SettingsListItem {
                id: ids::SETTINGS_LOCAL + index as u64 * 100,
                title: l10n.settings_theme(),
                subtitle: theme_title(state.options.theme_mode, &l10n),
                scheme,
                progress: state.setting_expand[index].value(),
                options: theme_options(&l10n, state.options.theme_mode),
                pressed: state.pressed,
                enabled: true,
                reason: None,
                handle: handle.clone(),
                select: |s: &mut GalleryState, which: usize| {
                    s.options.theme_mode = THEME_VALUES[which];
                },
            }),
            ExpandableSetting::TextDirection => component(SettingsListItem {
                id: ids::SETTINGS_LOCAL + index as u64 * 100,
                title: l10n.settings_text_direction(),
                subtitle: direction_title(state.options.custom_text_direction, &l10n),
                scheme,
                progress: 0.0,
                options: Vec::new(),
                pressed: state.pressed,
                enabled: false,
                reason: Some("Resolves, but no RTL layout renders it yet"),
                handle: handle.clone(),
                select: |_, _| {},
            }),
            ExpandableSetting::Locale => component(SettingsListItem {
                id: ids::SETTINGS_LOCAL + index as u64 * 100,
                title: l10n.settings_locale(),
                subtitle: l10n.settings_system_default().to_string(),
                scheme,
                progress: 0.0,
                options: Vec::new(),
                pressed: state.pressed,
                enabled: false,
                reason: Some("The message catalogue is English-only"),
                handle: handle.clone(),
                select: |_, _| {},
            }),
            ExpandableSetting::Platform => component(SettingsListItem {
                id: ids::SETTINGS_LOCAL + index as u64 * 100,
                title: l10n.settings_platform_mechanics(),
                subtitle: "Windows".to_string(),
                scheme,
                progress: 0.0,
                options: Vec::new(),
                pressed: state.pressed,
                enabled: false,
                reason: Some("Carried, but there is one embedder so far"),
                handle: handle.clone(),
                select: |_, _| {},
            }),
        };
        // Upstream's `_AnimateSettingsListItems`: each item's dividing padding
        // tweens from 0 to 4 over the stagger interval.
        let top = 4.0 * stagger;
        children.push(rustflutter::framework::single(item, move |rendered| {
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, top, 0.0, 0.0))
                    .with_child(rendered),
            )
        }));
    }

    children.push(component(SlowMotionSetting {
        id: ids::SETTINGS_LOCAL + 500,
        scheme,
        on: state.options.time_dilation != 1.0,
        handle: handle.clone(),
    }));

    if !is_desktop {
        // Upstream's mobile-only footer: the links and the attribution, between
        // two-pixel rules.
        children.push(leaf(|| Container::new().with_size(1.0, 16.0)));
        children.push(leaf(move || rule(scheme)));
        children.push(leaf(|| Container::new().with_size(1.0, 12.0)));
        children.push(component(SettingsLink {
            id: ids::SETTINGS_LOCAL + 600,
            icon: catalog::icon::INFO_OUTLINE,
            title: l10n.settings_about(),
            reason: None,
            scheme,
            pressed: state.pressed,
            handle: handle.clone(),
        }));
        children.push(component(SettingsLink {
            id: ids::SETTINGS_LOCAL + 601,
            icon: catalog::icon::FEEDBACK,
            title: l10n.settings_feedback(),
            reason: Some("Needs a URL launcher, which the example has no counterpart of"),
            scheme,
            pressed: state.pressed,
            handle: handle.clone(),
        }));
        children.push(leaf(|| Container::new().with_size(1.0, 12.0)));
        children.push(leaf(move || rule(scheme)));
        children.push(component(Attribution { scheme, is_desktop }));
    }

    let body = app::scrolling_body(children, 0.0, 0.0, state, handle);
    let fill = scheme.secondary_container;
    let bottom = if is_desktop {
        0.0
    } else {
        GALLERY_HEADER_HEIGHT
    };
    rustflutter::framework::single(body, move |rendered| {
        // Upstream's `Material(color: secondaryContainer)` with the mobile
        // bottom padding that keeps the list clear of the home peek.
        Box::new(
            Container::new()
                .with_color(fill)
                .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, bottom))
                .with_child(rendered),
        )
    })
}

/// Upstream's `Divider(thickness: 2, height: 0)`.
fn rule(scheme: Scheme) -> Container {
    let color = scheme.on_surface.with_alpha(0x1F);
    Container::new().with_height(2.0).with_color(color)
}

/// The panel's "Settings" header. Upstream builds it with `home.dart`'s
/// `Header`, padded 32 in and recoloured `onSurface`.
struct SettingsHeader {
    is_desktop: bool,
    scheme: Scheme,
}

impl Component for SettingsHeader {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let is_desktop = self.is_desktop;
        let color = self.scheme.on_surface;
        leaf(move || {
            Container::new()
                .with_padding(EdgeInsets::symmetric(32.0, 0.0))
                .with_child(crate::pages::home::header("Settings", color, is_desktop))
        })
    }
}

// -- One expandable setting ----------------------------------------------------

/// One row of an option list: the value, its display strings, and whether it
/// is the selected one. Upstream's `DisplayOption` plus the selection mark.
struct OptionRow {
    title: String,
    subtitle: Option<String>,
    selected: bool,
}

/// An expandable setting: a header row that opens to its options. Upstream's
/// `SettingsListItem`.
///
/// `select` applies the option at an index to the gallery's options; the
/// header tap toggles the expansion through `GalleryState`.
struct SettingsListItem {
    id: u64,
    title: &'static str,
    subtitle: String,
    scheme: Scheme,
    /// The expansion controller's value: 0 closed, 1 open.
    progress: f32,
    options: Vec<OptionRow>,
    pressed: Option<u64>,
    /// False for the options that resolve but cannot take effect yet: the row
    /// stays visible, the reason replaces the expansion.
    enabled: bool,
    reason: Option<&'static str>,
    handle: StateHandle<GalleryState>,
    select: fn(&mut GalleryState, usize),
}

impl SettingsListItem {
    fn index(&self) -> usize {
        ((self.id - ids::SETTINGS_LOCAL) / 100) as usize
    }
}

impl Component for SettingsListItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        // Upstream drives every tween off the eased controller value.
        let t = Curve::EASE_IN.transform(self.progress);
        let id = self.id;
        let index = self.index();
        let enabled = self.enabled;
        let held = self.pressed == Some(id);

        let header_handlers = if enabled {
            PointerHandlers::new()
                .with_tap({
                    let handle = self.handle.clone();
                    move |_| {
                        handle.set_state(move |state| state.toggle_setting(index));
                    }
                })
                .with_press_change({
                    let handle = self.handle.clone();
                    move |down| {
                        handle.set_state(move |state| {
                            state.pressed = if down { Some(id) } else { None };
                        });
                    }
                })
        } else {
            PointerHandlers::new()
        };

        // Upstream's tweens at this frame's value:
        //   margin   STEB(32, 0, 32, 8)   → zero
        //   padding  STEB(16, 10, 0, 10)  → STEB(32, 18, 32, 20)
        //   radius   10                   → 0
        //   subtitle height factor 1      → 0
        //   chevron  0                    → half a turn
        let margin = lerp_insets(EdgeInsets::only(32.0, 0.0, 32.0, 8.0), EdgeInsets::ZERO, t);
        let padding = lerp_insets(
            EdgeInsets::only(16.0, 10.0, 0.0, 10.0),
            EdgeInsets::only(32.0, 18.0, 32.0, 20.0),
            t,
        );
        let radius = 10.0 * (1.0 - t);
        let children_padding = lerp_insets(EdgeInsets::symmetric(32.0, 0.0), EdgeInsets::ZERO, t);
        let chevron_degrees = 180.0 * t;

        let title = self.title;
        let subtitle = if enabled {
            self.subtitle.clone()
        } else {
            String::new()
        };
        let reason = self.reason;
        let subtitle_style = text::LABEL_SMALL.styled(scheme.primary);
        let reason_style = text::LABEL_SMALL.styled(scheme.muted());
        let title_color = if enabled {
            scheme.on_surface
        } else {
            scheme.muted()
        };

        let header = leaf(move || {
            let mut texts = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .push(Text::new(title).with_style(text::TITLE_MEDIUM.styled(title_color)));
            if !subtitle.is_empty() {
                // Upstream's `SizeTransition(sizeFactor: _headerSubtitleHeight)`:
                // the subtitle shrinks out as the item opens.
                texts = texts.push(ClipRRect::new(
                    0.0,
                    RenderAlign::new(
                        Alignment::TOP_LEFT,
                        Text::new(subtitle.clone()).with_style(subtitle_style.clone()),
                    )
                    .with_factors(None, Some((1.0 - t).max(f32::EPSILON))),
                ));
            }
            if let Some(reason) = reason {
                texts = texts.push(Text::new(reason).with_style(reason_style.clone()));
            }

            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push_flex(FlexChild::expanded(
                    Container::new().with_padding(padding).with_child(texts),
                    1,
                ));
            if enabled {
                row = row.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(8.0, 0.0, 24.0, 0.0))
                        .with_child(Transform::rotate(
                            chevron_degrees,
                            Align::new(
                                Alignment::CENTER,
                                Text::new(catalog::icon::ARROW_DROP_DOWN)
                                    .with_font_family(catalog::MATERIAL_ICONS)
                                    .with_size(24.0)
                                    .with_color(scheme.on_surface),
                            ),
                        )),
                );
            }

            Pointer::new(
                id,
                Container::new()
                    .with_margin(margin)
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x1F)
                    } else {
                        scheme.secondary
                    })
                    .with_corner_radius(radius)
                    .with_child(row),
            )
            .with_handlers(header_handlers.clone())
        });

        if !enabled || self.progress <= 0.0 {
            return header;
        }

        // The open option list: upstream's border-start column of
        // `RadioListTile`s, clipped to the eased height factor.
        let mut rows: Vec<AnyWidget> = Vec::new();
        for (which, option) in self.options.iter().enumerate() {
            rows.push(component(OptionItem {
                id: id + 10 + which as u64,
                row: OptionRow {
                    title: option.title.clone(),
                    subtitle: option.subtitle.clone(),
                    selected: option.selected,
                },
                which,
                scheme,
                pressed: self.pressed,
                handle: self.handle.clone(),
                select: self.select,
            }));
        }

        let list = many(rows, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            // Upstream's two-pixel start border in the background colour.
            Box::new(
                Container::new()
                    .with_margin(EdgeInsets::only(24.0, 0.0, 0.0, 40.0))
                    .with_border(2.0, scheme.background)
                    .with_child(column),
            )
        });

        many(vec![header, list], move |mut rendered| {
            let list = rendered.pop().expect("two children");
            let header = rendered.pop().expect("two children");
            let clipped = ClipRRect::new(
                0.0,
                RenderAlign::new(Alignment::TOP_CENTER, list)
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

/// One option in an open setting. Upstream's `RadioListTile`: the mark, the
/// title, and the subtitle when there is one.
struct OptionItem {
    id: u64,
    row: OptionRow,
    which: usize,
    scheme: Scheme,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
    select: fn(&mut GalleryState, usize),
}

impl Component for OptionItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        let id = self.id;
        let which = self.which;
        let select = self.select;
        let held = self.pressed == Some(id);
        let selected = self.row.selected;
        let title = self.row.title.clone();
        let subtitle = self.row.subtitle.clone();

        let handlers = PointerHandlers::new()
            .with_tap({
                let handle = self.handle.clone();
                move |_| {
                    handle.set_state(move |state| select(state, which));
                }
            })
            .with_press_change({
                let handle = self.handle.clone();
                move |down| {
                    handle.set_state(move |state| {
                        state.pressed = if down { Some(id) } else { None };
                    });
                }
            });

        leaf(move || {
            // The radio mark: a ring in primary, filled when selected.
            let ring = Container::new()
                .with_size(18.0, 18.0)
                .with_corner_radius(9.0)
                .with_border(2.0, scheme.primary)
                .with_child(if selected {
                    Align::new(
                        Alignment::CENTER,
                        Container::new()
                            .with_size(10.0, 10.0)
                            .with_corner_radius(5.0)
                            .with_color(scheme.primary),
                    )
                } else {
                    Align::new(Alignment::CENTER, Container::new().with_size(1.0, 1.0))
                });

            let mut texts = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .push(
                    Text::new(title.clone()).with_style(text::BODY_LARGE.styled(scheme.on_primary)),
                );
            if let Some(subtitle) = &subtitle {
                texts = texts.push(Text::new(subtitle.clone()).with_style(role_at(
                    text::BODY_LARGE,
                    12.0,
                    scheme.on_primary.with_alpha(0xCC),
                )));
            }

            Pointer::new(
                id,
                Container::new()
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x14)
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_padding(EdgeInsets::symmetric(12.0, 6.0))
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .push(ring)
                            .push(Container::new().with_size(12.0, 1.0))
                            .push_flex(FlexChild::expanded(texts, 1)),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// The slow-motion toggle. Upstream's `ToggleSetting`.
struct SlowMotionSetting {
    id: u64,
    scheme: Scheme,
    on: bool,
    handle: StateHandle<GalleryState>,
}

impl Component for SlowMotionSetting {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        let id = self.id;
        let on = self.on;
        let handlers = PointerHandlers::new().with_tap({
            let handle = self.handle.clone();
            move |_| {
                handle.set_state(|state| {
                    state.options.time_dilation = if state.options.time_dilation == 1.0 {
                        5.0
                    } else {
                        1.0
                    };
                });
            }
        });

        leaf(move || {
            // The switch, drawn inline: a track and a knob, primary when on --
            // the component library's `Switch` reads the framework `Theme`,
            // and this panel is painted from the gallery's scheme.
            let track = if on {
                scheme.primary
            } else {
                scheme.on_surface.with_alpha(0x1F)
            };
            let knob = if on {
                scheme.on_primary
            } else {
                scheme.muted()
            };
            let knob_x = if on { 26.0 } else { 2.0 };
            let switch = Container::new()
                .with_size(48.0, 28.0)
                .with_corner_radius(14.0)
                .with_color(track)
                .with_child(
                    // Align by a fixed offset: the knob sits 2px in from the
                    // edge it is on.
                    rustflutter::widgets::Padding::new(
                        EdgeInsets::only(knob_x, 0.0, 0.0, 0.0),
                        Align::new(
                            Alignment::CENTER_LEFT,
                            Container::new()
                                .with_size(24.0, 24.0)
                                .with_corner_radius(12.0)
                                .with_color(knob),
                        ),
                    ),
                );

            Pointer::new(
                id,
                Container::new()
                    .with_margin(EdgeInsets::only(32.0, 0.0, 32.0, 8.0))
                    .with_color(scheme.secondary)
                    .with_corner_radius(10.0)
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .push_flex(FlexChild::expanded(
                                Container::new()
                                    .with_padding(EdgeInsets::all(16.0))
                                    .with_child(
                                        Text::new("Slow motion").with_style(
                                            text::TITLE_MEDIUM.styled(scheme.on_surface),
                                        ),
                                    ),
                                1,
                            ))
                            .push(
                                Container::new()
                                    .with_padding(EdgeInsets::only(0.0, 0.0, 8.0, 0.0))
                                    .with_child(switch),
                            ),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// A link row in the mobile footer: the icon, then the title. Upstream's
/// `_SettingsLink`. A `reason` disables the row and says why.
struct SettingsLink {
    id: u64,
    icon: &'static str,
    title: &'static str,
    reason: Option<&'static str>,
    scheme: Scheme,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for SettingsLink {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        let id = self.id;
        let icon = self.icon;
        let title = self.title;
        let reason = self.reason;
        let enabled = reason.is_none();
        let held = self.pressed == Some(id);

        let handlers = if enabled {
            PointerHandlers::new()
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
                            state.pressed = if down { Some(id) } else { None };
                        });
                    }
                })
        } else {
            PointerHandlers::new()
        };

        leaf(move || {
            // Upstream tints the icon at half the on-secondary colour.
            let ink = if enabled {
                scheme.on_secondary
            } else {
                scheme.muted()
            };
            let mut texts = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .push(Text::new(title).with_style(text::LABEL_LARGE.styled(ink)));
            if let Some(reason) = reason {
                texts = texts
                    .push(Text::new(reason).with_style(text::LABEL_SMALL.styled(scheme.muted())));
            }

            Pointer::new(
                id,
                Container::new()
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x14)
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_padding(EdgeInsets::symmetric(32.0, 0.0))
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .push(
                                Text::new(icon)
                                    .with_font_family(catalog::MATERIAL_ICONS)
                                    .with_size(24.0)
                                    .with_color(scheme.on_secondary.with_alpha(0x80)),
                            )
                            .push(
                                Container::new()
                                    .with_padding(EdgeInsets::only(16.0, 12.0, 0.0, 12.0))
                                    .with_child(texts),
                            ),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// The attribution line. Upstream's `SettingsAttribution`.
struct Attribution {
    scheme: Scheme,
    is_desktop: bool,
}

impl Component for Attribution {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        let vertical = if self.is_desktop { 0.0 } else { 28.0 };
        leaf(move || {
            Container::new()
                .with_padding(EdgeInsets::symmetric(32.0, vertical))
                .with_child(
                    Text::new("Designed by TOASTER in London").with_style(role_at(
                        text::BODY_LARGE,
                        12.0,
                        scheme.on_secondary,
                    )),
                )
        })
    }
}

// -- The option tables ----------------------------------------------------------

/// The scales upstream offers, sentinel first.
const TEXT_SCALE_VALUES: &[f64] = &[
    crate::constants::SYSTEM_TEXT_SCALE_FACTOR_OPTION,
    0.8,
    1.0,
    2.0,
    3.0,
];

fn text_scale_title(value: f64, l10n: &GalleryLocalizations) -> String {
    let index = TEXT_SCALE_VALUES
        .iter()
        .position(|v| *v == value)
        .unwrap_or(0);
    text_scale_options(l10n, value)
        .into_iter()
        .nth(index)
        .map(|row| row.title)
        .unwrap_or_default()
}

fn text_scale_options(l10n: &GalleryLocalizations, selected: f64) -> Vec<OptionRow> {
    let titles = [
        l10n.settings_system_default(),
        l10n.settings_text_scaling_small(),
        l10n.settings_text_scaling_normal(),
        l10n.settings_text_scaling_large(),
        l10n.settings_text_scaling_huge(),
    ];
    TEXT_SCALE_VALUES
        .iter()
        .zip(titles)
        .map(|(value, title)| OptionRow {
            title: title.to_string(),
            subtitle: None,
            selected: *value == selected,
        })
        .collect()
}

const THEME_VALUES: &[ThemeMode] = &[ThemeMode::System, ThemeMode::Dark, ThemeMode::Light];

fn theme_title(mode: ThemeMode, l10n: &GalleryLocalizations) -> String {
    theme_options(l10n, mode)
        .into_iter()
        .find(|row| row.selected)
        .map(|row| row.title)
        .unwrap_or_default()
}

fn theme_options(l10n: &GalleryLocalizations, selected: ThemeMode) -> Vec<OptionRow> {
    let titles = [
        l10n.settings_system_default(),
        l10n.settings_dark_theme(),
        l10n.settings_light_theme(),
    ];
    THEME_VALUES
        .iter()
        .zip(titles)
        .map(|(value, title)| OptionRow {
            title: title.to_string(),
            subtitle: None,
            selected: *value == selected,
        })
        .collect()
}

fn direction_title(direction: CustomTextDirection, l10n: &GalleryLocalizations) -> String {
    match direction {
        CustomTextDirection::LocaleBased => l10n.settings_text_direction_locale_based().to_string(),
        CustomTextDirection::Ltr => l10n.settings_text_direction_ltr().to_string(),
        CustomTextDirection::Rtl => l10n.settings_text_direction_rtl().to_string(),
    }
}

/// A text role at a different size, upstream's `apply(fontSizeDelta:)`.
fn role_at(role: text::Role, size: f32, color: Color) -> TextStyle {
    let mut style = role.styled(color);
    style.font_size = size;
    style
}

fn lerp_insets(from: EdgeInsets, to: EdgeInsets, t: f32) -> EdgeInsets {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    EdgeInsets {
        left: lerp(from.left, to.left),
        top: lerp(from.top, to.top),
        right: lerp(from.right, to.right),
        bottom: lerp(from.bottom, to.bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_expandable_setting_has_a_slot() {
        // The controllers on `GalleryState` are indexed by position in this
        // table; a row added here without a slot there panics on the first
        // tap. Five is upstream's enum size.
        assert_eq!(EXPANDABLE_SETTINGS.len(), 5);
    }

    #[test]
    fn the_text_scale_table_starts_with_the_sentinel() {
        assert_eq!(
            TEXT_SCALE_VALUES[0],
            crate::constants::SYSTEM_TEXT_SCALE_FACTOR_OPTION
        );
        assert_eq!(TEXT_SCALE_VALUES.len(), 5);
    }
}
