// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The Material theme, from upstream `material/theme_data.dart` and
//! `material/theme.dart`.
//!
//! A [`ThemeData`] is what every Material control reads its paint from. Most
//! of it is derived: name a [`ColorScheme`] and the two dozen colours a
//! control might ask for fall out of it by upstream's rules, which is why
//! `ThemeData::from_color_scheme` is the constructor worth using and the
//! individual colours are there to override rather than to fill in.
//!
//! # This is the first half of the theme
//!
//! Upstream's `ThemeData` has ninety-two fields: about thirty general ones,
//! and about forty-five *component* themes -- `appBarTheme`, `chipTheme`,
//! `dialogTheme` and so on, one per control family. The general half is
//! here. The component half arrives with the controls it belongs to: a
//! component theme with no component to configure is a data class nothing
//! reads, and each control's cluster brings its own along with the fallback
//! chain that runs through here.
//!
//! # The crate already had a theme
//!
//! [`crate::components::Theme`] is fourteen fields, and every control in this
//! crate reads it. It stays, and [`ThemeData::to_component_theme`] derives
//! one -- so a caller can hold the upstream shape and hand the existing
//! controls what they expect, and the controls can migrate one at a time
//! rather than in one commit that touches everything.
//!
//! # Recorded divergences
//!
//! * `primarySwatch` and `ColorScheme.fromSwatch` are not here. They are the
//!   Material 2 way in, and upstream is phasing them out in favour of a
//!   scheme (flutter#91772); this port starts where upstream is going.
//! * `useMaterial3` is not a field. Upstream keeps it to switch between two
//!   sets of defaults during the migration; there is only one set here, the
//!   Material 3 one.
//! * `Typography`, `TextTheme` and `IconThemeData` are not here yet -- they
//!   belong with the text and icon clusters (`E5` in the plan gives the
//!   framework an icon system at all).

use crate::animation::{Animatable, ColorTween, Tween};
use crate::color_scheme::ColorScheme;
use crate::colors::Colors;
use crate::components::Theme;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, provide};
use crate::platform::Brightness;

/// Upstream `VisualDensity`: how tightly a control packs itself.
///
/// The two numbers are in upstream's density units, each worth four logical
/// pixels, and they shrink or grow a control's box without changing what is
/// drawn in it -- a touch target that is comfortable on a phone is wasteful
/// on a desktop where a mouse can hit a smaller one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualDensity {
    pub horizontal: f32,
    pub vertical: f32,
}

impl VisualDensity {
    /// Upstream `VisualDensity.minimumDensity`.
    pub const MINIMUM: f32 = -4.0;
    /// Upstream `VisualDensity.maximumDensity`.
    pub const MAXIMUM: f32 = 4.0;
    /// One density unit, in logical pixels -- upstream's
    /// `_kDensityAmountPerUnit`, applied as `4 * density`.
    pub const PIXELS_PER_UNIT: f32 = 4.0;

    /// Upstream `VisualDensity.standard`: the default, and the density every
    /// other one is measured from.
    pub const STANDARD: VisualDensity = VisualDensity {
        horizontal: 0.0,
        vertical: 0.0,
    };

    /// Upstream `VisualDensity.comfortable`.
    pub const COMFORTABLE: VisualDensity = VisualDensity {
        horizontal: -1.0,
        vertical: -1.0,
    };

    /// Upstream `VisualDensity.compact`.
    pub const COMPACT: VisualDensity = VisualDensity {
        horizontal: -2.0,
        vertical: -2.0,
    };

    pub const fn new(horizontal: f32, vertical: f32) -> VisualDensity {
        VisualDensity {
            horizontal,
            vertical,
        }
    }

    /// Upstream `baseSizeAdjustment`: what this density adds to a control's
    /// size, in logical pixels. Negative for a denser layout.
    pub fn base_size_adjustment(&self) -> (f32, f32) {
        (
            self.horizontal * VisualDensity::PIXELS_PER_UNIT,
            self.vertical * VisualDensity::PIXELS_PER_UNIT,
        )
    }

    /// Upstream `effectiveConstraints`: the constraints a control should lay
    /// itself out against at this density -- the minima moved by the
    /// adjustment, never below zero and never above the maxima.
    pub fn effective_constraints(
        &self,
        constraints: crate::render::BoxConstraints,
    ) -> crate::render::BoxConstraints {
        let (horizontal, vertical) = self.base_size_adjustment();
        crate::render::BoxConstraints {
            min_width: (constraints.min_width + horizontal)
                .clamp(0.0, constraints.max_width.max(0.0)),
            min_height: (constraints.min_height + vertical)
                .clamp(0.0, constraints.max_height.max(0.0)),
            ..constraints
        }
    }

    /// Upstream `VisualDensity.lerp`.
    pub fn lerp(a: VisualDensity, b: VisualDensity, t: f32) -> VisualDensity {
        VisualDensity {
            horizontal: a.horizontal + (b.horizontal - a.horizontal) * t,
            vertical: a.vertical + (b.vertical - a.vertical) * t,
        }
    }

    /// Upstream `adaptivePlatformDensity`: dense on a desktop, standard where
    /// a finger is the pointer.
    pub fn adaptive_platform_density() -> VisualDensity {
        if cfg!(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux"
        )) {
            VisualDensity::COMPACT
        } else {
            VisualDensity::STANDARD
        }
    }
}

impl Default for VisualDensity {
    fn default() -> VisualDensity {
        VisualDensity::STANDARD
    }
}

/// Upstream `ThemeData`, general half.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeData {
    pub brightness: Brightness,
    pub color_scheme: ColorScheme,
    pub visual_density: VisualDensity,

    /// Upstream `canvasColor`: what a `Material` sits on.
    pub canvas_color: Color,
    pub card_color: Color,
    pub scaffold_background_color: Color,
    pub divider_color: Color,
    pub shadow_color: Color,

    /// Upstream `primaryColor`: the surface an app bar takes.
    pub primary_color: Color,
    pub primary_color_light: Color,
    pub primary_color_dark: Color,
    pub secondary_header_color: Color,

    pub disabled_color: Color,
    pub focus_color: Color,
    pub hover_color: Color,
    pub highlight_color: Color,
    pub splash_color: Color,
    pub hint_color: Color,
    pub unselected_widget_color: Color,

    /// Upstream `applyElevationOverlayColor`: whether a raised surface in a
    /// dark theme is tinted by its elevation rather than only shadowed.
    pub apply_elevation_overlay_color: bool,
}

impl ThemeData {
    /// Upstream `ThemeData(colorScheme: ...)`, whose derivations these are:
    /// every colour that was not named falls out of the scheme.
    ///
    /// The four lines upstream writes first -- `primaryColor`, `canvasColor`,
    /// `scaffoldBackgroundColor`, `cardColor`, `dividerColor` -- come off the
    /// scheme; the rest are the brightness-dependent constants that follow
    /// them.
    pub fn from_color_scheme(color_scheme: ColorScheme) -> ThemeData {
        let brightness = color_scheme.brightness;
        let is_dark = brightness == Brightness::Dark;
        // Upstream's `primarySurfaceColor`: a dark theme's bars take the
        // surface, a light theme's take the primary.
        let primary_surface = if is_dark {
            color_scheme.surface
        } else {
            color_scheme.primary
        };
        ThemeData {
            brightness,
            color_scheme,
            visual_density: VisualDensity::STANDARD,
            canvas_color: color_scheme.surface,
            card_color: color_scheme.surface,
            scaffold_background_color: color_scheme.surface,
            divider_color: color_scheme.outline(),
            shadow_color: Colors::BLACK,
            primary_color: primary_surface,
            primary_color_light: if is_dark {
                Colors::GREY.shade(500).expect("grey has a 500")
            } else {
                Colors::BLUE.shade(100).expect("blue has a 100")
            },
            primary_color_dark: if is_dark {
                Colors::BLACK
            } else {
                Colors::BLUE.shade(700).expect("blue has a 700")
            },
            secondary_header_color: if is_dark {
                Colors::GREY.shade(700).expect("grey has a 700")
            } else {
                Colors::BLUE.shade(50).expect("blue has a 50")
            },
            disabled_color: if is_dark {
                Colors::WHITE38
            } else {
                Colors::BLACK38
            },
            focus_color: if is_dark {
                Color::argb(31, 255, 255, 255)
            } else {
                Color::argb(31, 0, 0, 0)
            },
            hover_color: if is_dark {
                Color::argb(10, 255, 255, 255)
            } else {
                Color::argb(10, 0, 0, 0)
            },
            highlight_color: if is_dark {
                Color(0x40cccccc)
            } else {
                Color(0x66bcbcbc)
            },
            splash_color: if is_dark {
                Color(0x40cccccc)
            } else {
                Color(0x66c8c8c8)
            },
            hint_color: if is_dark {
                Colors::WHITE60
            } else {
                Color::argb(153, 0, 0, 0)
            },
            unselected_widget_color: if is_dark {
                Colors::WHITE70
            } else {
                Colors::BLACK54
            },
            // Upstream: `applyElevationOverlayColor ??= brightness == dark`.
            apply_elevation_overlay_color: is_dark,
        }
    }

    /// Upstream `ThemeData.light()`: the Material 3 baseline light scheme and
    /// everything that follows from it.
    pub fn light() -> ThemeData {
        ThemeData::from_color_scheme(ColorScheme::light_m3())
    }

    /// Upstream `ThemeData.dark()`.
    pub fn dark() -> ThemeData {
        ThemeData::from_color_scheme(ColorScheme::dark_m3())
    }

    /// Upstream `ThemeData.fallback`: what a tree with no theme in it gets.
    pub fn fallback() -> ThemeData {
        ThemeData::light()
    }

    pub fn with_visual_density(mut self, visual_density: VisualDensity) -> ThemeData {
        self.visual_density = visual_density;
        self
    }

    pub fn with_primary_color(mut self, primary_color: Color) -> ThemeData {
        self.primary_color = primary_color;
        self
    }

    pub fn with_canvas_color(mut self, canvas_color: Color) -> ThemeData {
        self.canvas_color = canvas_color;
        self
    }

    pub fn with_scaffold_background_color(mut self, color: Color) -> ThemeData {
        self.scaffold_background_color = color;
        self
    }

    pub fn with_card_color(mut self, card_color: Color) -> ThemeData {
        self.card_color = card_color;
        self
    }

    pub fn with_divider_color(mut self, divider_color: Color) -> ThemeData {
        self.divider_color = divider_color;
        self
    }

    /// Upstream `ThemeData.lerp`: every colour interpolated, the scheme with
    /// them, and the flags taken from whichever end is nearer.
    pub fn lerp(a: &ThemeData, b: &ThemeData, t: f32) -> ThemeData {
        let mix = |first: Color, second: Color| {
            ColorTween {
                begin: first,
                end: second,
            }
            .lerp(t)
        };
        let nearer = if t < 0.5 { a } else { b };
        ThemeData {
            brightness: nearer.brightness,
            color_scheme: ColorScheme::lerp(&a.color_scheme, &b.color_scheme, t),
            visual_density: VisualDensity::lerp(a.visual_density, b.visual_density, t),
            canvas_color: mix(a.canvas_color, b.canvas_color),
            card_color: mix(a.card_color, b.card_color),
            scaffold_background_color: mix(
                a.scaffold_background_color,
                b.scaffold_background_color,
            ),
            divider_color: mix(a.divider_color, b.divider_color),
            shadow_color: mix(a.shadow_color, b.shadow_color),
            primary_color: mix(a.primary_color, b.primary_color),
            primary_color_light: mix(a.primary_color_light, b.primary_color_light),
            primary_color_dark: mix(a.primary_color_dark, b.primary_color_dark),
            secondary_header_color: mix(a.secondary_header_color, b.secondary_header_color),
            disabled_color: mix(a.disabled_color, b.disabled_color),
            focus_color: mix(a.focus_color, b.focus_color),
            hover_color: mix(a.hover_color, b.hover_color),
            highlight_color: mix(a.highlight_color, b.highlight_color),
            splash_color: mix(a.splash_color, b.splash_color),
            hint_color: mix(a.hint_color, b.hint_color),
            unselected_widget_color: mix(a.unselected_widget_color, b.unselected_widget_color),
            apply_elevation_overlay_color: nearer.apply_elevation_overlay_color,
        }
    }

    /// The theme the controls in this crate read, derived from this one.
    ///
    /// Not an upstream method -- upstream has one theme type. It is the seam
    /// that lets the two live side by side while the controls move over one
    /// cluster at a time, and it maps role by role rather than guessing:
    /// `surface` is the scheme's, `outline` is the scheme's, `text` is
    /// `onSurface`, and the two sizes and the spacing keep the values the
    /// crate's controls were built against.
    pub fn to_component_theme(&self) -> Theme {
        let base = if self.brightness == Brightness::Dark {
            Theme::dark()
        } else {
            Theme::light()
        };
        Theme {
            background: self.scaffold_background_color,
            surface: self.color_scheme.surface,
            surface_variant: self.color_scheme.surface_container_highest(),
            outline: self.color_scheme.outline(),
            primary: self.color_scheme.primary,
            on_primary: self.color_scheme.on_primary,
            danger: self.color_scheme.error,
            text: self.color_scheme.on_surface,
            text_muted: self.color_scheme.on_surface_variant(),
            ..base
        }
    }

    /// Upstream `Theme.of(context)`, which is this type rather than the
    /// widget: the nearest theme, or the fallback where nobody installed one.
    pub fn of(context: &mut BuildContext) -> ThemeData {
        context
            .inherited::<ThemeData>()
            .map(|data| *data)
            .unwrap_or_else(ThemeData::fallback)
    }
}

impl Default for ThemeData {
    fn default() -> ThemeData {
        ThemeData::fallback()
    }
}

/// Upstream `Theme`: the widget that installs a [`ThemeData`] for a subtree.
///
/// The crate's own [`crate::components::Theme`] is a value rather than a
/// widget, and is installed with `provide`; this installs both, so a subtree
/// under it can be read by either.
pub struct MaterialTheme;

impl MaterialTheme {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(data: ThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, provide(data.to_component_theme(), child))
    }
}

/// Upstream `ThemeDataTween`.
#[derive(Clone, Copy, Debug)]
pub struct ThemeDataTween {
    pub begin: ThemeData,
    pub end: ThemeData,
}

impl Tween for ThemeDataTween {
    type Output = ThemeData;

    fn lerp(&self, t: f32) -> ThemeData {
        ThemeData::lerp(&self.begin, &self.end, t)
    }
}

impl Animatable for ThemeDataTween {
    type Output = ThemeData;

    fn transform(&self, t: f32) -> ThemeData {
        self.lerp(t)
    }
}

/// Upstream `AnimatedTheme`: a theme that moves to its new value rather than
/// snapping to it.
///
/// Upstream is an `ImplicitlyAnimatedWidget`; here it is the crate's own
/// implicit animation ([`crate::implicit::animated`]) over the same tween,
/// which is the same "target changed, walk there" rule.
pub struct AnimatedTheme;

impl AnimatedTheme {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F>(data: ThemeData, duration: std::time::Duration, child: F) -> AnyWidget
    where
        F: Fn(ThemeData) -> AnyWidget + 'static,
    {
        crate::implicit::animated(
            data,
            duration,
            crate::animation::Curve::Linear,
            move |current| child(current),
        )
    }
}

impl crate::implicit::Lerp for ThemeData {
    fn lerp(from: ThemeData, to: ThemeData, t: f32) -> ThemeData {
        ThemeData::lerp(&from, &to, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::BoxConstraints;

    #[test]
    fn a_theme_derives_its_colours_from_its_scheme() {
        let theme = ThemeData::light();
        let scheme = ColorScheme::light_m3();
        // Upstream's five lines off the scheme.
        assert_eq!(theme.canvas_color, scheme.surface);
        assert_eq!(theme.card_color, scheme.surface);
        assert_eq!(theme.scaffold_background_color, scheme.surface);
        assert_eq!(theme.divider_color, scheme.outline());
        // A light theme's bars take the primary; a dark theme's take the
        // surface -- upstream's `primarySurfaceColor`.
        assert_eq!(theme.primary_color, scheme.primary);
        assert_eq!(
            ThemeData::dark().primary_color,
            ColorScheme::dark_m3().surface
        );
    }

    #[test]
    fn the_brightness_dependent_constants_are_upstreams() {
        let light = ThemeData::light();
        assert_eq!(light.highlight_color, Color(0x66bcbcbc));
        assert_eq!(light.unselected_widget_color, Colors::BLACK54);
        assert!(!light.apply_elevation_overlay_color);

        let dark = ThemeData::dark();
        assert_eq!(dark.highlight_color, Color(0x40cccccc));
        assert_eq!(dark.unselected_widget_color, Colors::WHITE70);
        assert!(
            dark.apply_elevation_overlay_color,
            "upstream turns it on for a dark theme and leaves it off otherwise"
        );
    }

    #[test]
    fn a_density_moves_the_minima_and_leaves_the_maxima() {
        let compact = VisualDensity::COMPACT;
        assert_eq!(compact.base_size_adjustment(), (-8.0, -8.0));

        let constraints = BoxConstraints {
            min_width: 48.0,
            max_width: 200.0,
            min_height: 48.0,
            max_height: 200.0,
        };
        let tightened = compact.effective_constraints(constraints);
        assert_eq!(tightened.min_width, 40.0);
        assert_eq!(tightened.min_height, 40.0);
        assert_eq!(tightened.max_width, 200.0, "the maxima are untouched");

        // And it never drives a minimum below zero.
        let tiny = VisualDensity::new(-4.0, -4.0).effective_constraints(BoxConstraints {
            min_width: 4.0,
            max_width: 100.0,
            min_height: 4.0,
            max_height: 100.0,
        });
        assert_eq!(tiny.min_width, 0.0);
    }

    #[test]
    fn a_theme_lerps_role_by_role_and_flips_its_flags_halfway() {
        let light = ThemeData::light();
        let dark = ThemeData::dark();
        let half = ThemeData::lerp(&light, &dark, 0.5);
        assert_eq!(half.brightness, Brightness::Dark);
        assert!(half.apply_elevation_overlay_color);
        // The surface is between the two, not either of them.
        assert_ne!(half.canvas_color, light.canvas_color);
        assert_ne!(half.canvas_color, dark.canvas_color);

        let just_before = ThemeData::lerp(&light, &dark, 0.49);
        assert_eq!(just_before.brightness, Brightness::Light);
        assert!(!just_before.apply_elevation_overlay_color);
    }

    #[test]
    fn the_component_theme_it_derives_carries_the_scheme_across() {
        let data = ThemeData::dark();
        let theme = data.to_component_theme();
        assert_eq!(theme.primary, data.color_scheme.primary);
        assert_eq!(theme.text, data.color_scheme.on_surface);
        assert_eq!(theme.outline, data.color_scheme.outline());
        assert_eq!(theme.background, data.scaffold_background_color);
        // The metrics the crate's controls were built against are kept.
        assert_eq!(theme.radius, Theme::dark().radius);
        assert_eq!(theme.spacing, Theme::dark().spacing);
    }

    #[test]
    fn a_theme_widget_installs_both_themes_for_the_subtree() {
        use crate::framework::{Component, ElementTree, component, leaf};
        use crate::widgets::SizedBox;
        use std::cell::Cell;
        use std::rc::Rc;

        struct Reader(Rc<Cell<Option<(Color, Color)>>>);

        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let data = ThemeData::of(context);
                let legacy = crate::components::theme_of(context);
                self.0
                    .set(Some((data.color_scheme.primary, legacy.primary)));
                leaf(|| SizedBox::new(1.0, 1.0))
            }
        }

        let seen = Rc::new(Cell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(MaterialTheme::new(
            ThemeData::dark(),
            component(Reader(Rc::clone(&seen))),
        ));
        let (from_data, from_legacy) = seen.get().expect("built");
        assert_eq!(from_data, ColorScheme::dark_m3().primary);
        assert_eq!(
            from_legacy, from_data,
            "the derived component theme is the same colour, so a control \
             reading either sees one theme"
        );
    }

    #[test]
    fn a_tree_with_no_theme_gets_the_fallback() {
        use crate::framework::{Component, ElementTree, component, leaf};
        use crate::widgets::SizedBox;
        use std::cell::Cell;
        use std::rc::Rc;

        struct Reader(Rc<Cell<Option<Brightness>>>);

        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                self.0.set(Some(ThemeData::of(context).brightness));
                leaf(|| SizedBox::new(1.0, 1.0))
            }
        }

        let seen = Rc::new(Cell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader(Rc::clone(&seen))));
        assert_eq!(seen.get(), Some(Brightness::Light));
    }
}
