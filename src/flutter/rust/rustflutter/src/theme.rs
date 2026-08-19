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
use crate::component_themes::{
    AppBarThemeData, BadgeThemeData, BottomAppBarThemeData, BottomNavigationBarThemeData,
    BottomSheetThemeData, ButtonThemeData, CardThemeData, CheckboxThemeData, ChipThemeData,
    DataTableThemeData, DatePickerThemeData, DialogThemeData, DividerThemeData, DrawerThemeData,
    DropdownMenuThemeData, ElevatedButtonThemeData, ExpansionTileThemeData, FilledButtonThemeData,
    FloatingActionButtonThemeData, IconButtonThemeData, IconThemeData, InputDecorationThemeData,
    ListTileThemeData, MaterialBannerThemeData, MenuBarThemeData, MenuButtonThemeData,
    MenuThemeData, NavigationRailThemeData, OutlinedButtonThemeData, PopupMenuThemeData,
    ProgressIndicatorThemeData, RadioThemeData, ScrollbarThemeData, SearchBarThemeData,
    SearchViewThemeData, SegmentedButtonThemeData, SnackBarThemeData, SwitchThemeData,
    TabBarThemeData, TextButtonThemeData, TextSelectionThemeData, TimePickerThemeData,
    ToggleButtonsThemeData, TooltipThemeData,
};
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

/// Upstream `ThemeData`: the general half, plus the component themes whose
/// controls are here.
#[derive(Clone, Debug, PartialEq)]
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

    // -- Component themes -------------------------------------------------
    //
    // One per control family, and the fallback every `*Theme::of` lands on
    // when nobody installed a nearer one. They start empty: an unset field
    // means "whatever the control's own default is".
    pub divider_theme: DividerThemeData,
    pub card_theme: CardThemeData,
    pub badge_theme: BadgeThemeData,
    pub tooltip_theme: TooltipThemeData,
    pub progress_indicator_theme: ProgressIndicatorThemeData,
    pub checkbox_theme: CheckboxThemeData,
    pub radio_theme: RadioThemeData,
    pub switch_theme: SwitchThemeData,
    pub app_bar_theme: AppBarThemeData,
    pub bottom_sheet_theme: BottomSheetThemeData,
    pub snack_bar_theme: SnackBarThemeData,
    pub list_tile_theme: ListTileThemeData,
    pub dialog_theme: DialogThemeData,
    pub chip_theme: ChipThemeData,
    pub tab_bar_theme: TabBarThemeData,
    pub data_table_theme: DataTableThemeData,
    pub navigation_rail_theme: NavigationRailThemeData,
    pub bottom_navigation_bar_theme: BottomNavigationBarThemeData,
    pub drawer_theme: DrawerThemeData,
    pub elevated_button_theme: ElevatedButtonThemeData,
    pub filled_button_theme: FilledButtonThemeData,
    pub text_button_theme: TextButtonThemeData,
    pub outlined_button_theme: OutlinedButtonThemeData,
    pub icon_button_theme: IconButtonThemeData,
    pub banner_theme: MaterialBannerThemeData,
    pub expansion_tile_theme: ExpansionTileThemeData,
    /// The Material 2 button theme, which is a different set of questions
    /// from the five `*ButtonThemeData` above.
    pub button_theme: ButtonThemeData,
    pub scrollbar_theme: ScrollbarThemeData,
    pub menu_theme: MenuThemeData,
    pub menu_bar_theme: MenuBarThemeData,
    pub menu_button_theme: MenuButtonThemeData,
    pub segmented_button_theme: SegmentedButtonThemeData,
    pub floating_action_button_theme: FloatingActionButtonThemeData,
    pub toggle_buttons_theme: ToggleButtonsThemeData,
    pub search_bar_theme: SearchBarThemeData,
    pub search_view_theme: SearchViewThemeData,
    pub time_picker_theme: TimePickerThemeData,
    pub date_picker_theme: DatePickerThemeData,
    pub input_decoration_theme: InputDecorationThemeData,
    /// Upstream's `iconTheme`, which is the general one every icon under the
    /// theme starts from -- the component themes' own icon themes merge over
    /// it.
    pub icon_theme: IconThemeData,
    pub text_selection_theme: TextSelectionThemeData,
    pub popup_menu_theme: PopupMenuThemeData,
    pub dropdown_menu_theme: DropdownMenuThemeData,
    pub bottom_app_bar_theme: BottomAppBarThemeData,
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
            divider_theme: DividerThemeData::new(),
            card_theme: CardThemeData::new(),
            badge_theme: BadgeThemeData::new(),
            tooltip_theme: TooltipThemeData::new(),
            progress_indicator_theme: ProgressIndicatorThemeData::new(),
            checkbox_theme: CheckboxThemeData::new(),
            radio_theme: RadioThemeData::new(),
            switch_theme: SwitchThemeData::new(),
            app_bar_theme: AppBarThemeData::new(),
            bottom_sheet_theme: BottomSheetThemeData::new(),
            snack_bar_theme: SnackBarThemeData::new(),
            list_tile_theme: ListTileThemeData::new(),
            dialog_theme: DialogThemeData::new(),
            chip_theme: ChipThemeData::new(),
            tab_bar_theme: TabBarThemeData::new(),
            data_table_theme: DataTableThemeData::new(),
            navigation_rail_theme: NavigationRailThemeData::new(),
            bottom_navigation_bar_theme: BottomNavigationBarThemeData::new(),
            drawer_theme: DrawerThemeData::new(),
            elevated_button_theme: ElevatedButtonThemeData::new(),
            filled_button_theme: FilledButtonThemeData::new(),
            text_button_theme: TextButtonThemeData::new(),
            outlined_button_theme: OutlinedButtonThemeData::new(),
            icon_button_theme: IconButtonThemeData::new(),
            banner_theme: MaterialBannerThemeData::new(),
            expansion_tile_theme: ExpansionTileThemeData::new(),
            button_theme: ButtonThemeData::new(),
            scrollbar_theme: ScrollbarThemeData::new(),
            menu_theme: MenuThemeData::new(),
            menu_bar_theme: MenuBarThemeData::new(),
            menu_button_theme: MenuButtonThemeData::new(),
            segmented_button_theme: SegmentedButtonThemeData::new(),
            floating_action_button_theme: FloatingActionButtonThemeData::new(),
            toggle_buttons_theme: ToggleButtonsThemeData::new(),
            search_bar_theme: SearchBarThemeData::new(),
            search_view_theme: SearchViewThemeData::new(),
            time_picker_theme: TimePickerThemeData::new(),
            date_picker_theme: DatePickerThemeData::new(),
            input_decoration_theme: InputDecorationThemeData::new(),
            icon_theme: IconThemeData::new(),
            text_selection_theme: TextSelectionThemeData::new(),
            popup_menu_theme: PopupMenuThemeData::new(),
            dropdown_menu_theme: DropdownMenuThemeData::new(),
            bottom_app_bar_theme: BottomAppBarThemeData::new(),
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

    pub fn with_divider_theme(mut self, divider_theme: DividerThemeData) -> ThemeData {
        self.divider_theme = divider_theme;
        self
    }

    pub fn with_card_theme(mut self, card_theme: CardThemeData) -> ThemeData {
        self.card_theme = card_theme;
        self
    }

    pub fn with_badge_theme(mut self, badge_theme: BadgeThemeData) -> ThemeData {
        self.badge_theme = badge_theme;
        self
    }

    pub fn with_tooltip_theme(mut self, tooltip_theme: TooltipThemeData) -> ThemeData {
        self.tooltip_theme = tooltip_theme;
        self
    }

    pub fn with_scrollbar_theme(mut self, scrollbar_theme: ScrollbarThemeData) -> ThemeData {
        self.scrollbar_theme = scrollbar_theme;
        self
    }

    pub fn with_menu_theme(mut self, menu_theme: MenuThemeData) -> ThemeData {
        self.menu_theme = menu_theme;
        self
    }

    pub fn with_filled_button_theme(
        mut self,
        filled_button_theme: FilledButtonThemeData,
    ) -> ThemeData {
        self.filled_button_theme = filled_button_theme;
        self
    }

    pub fn with_text_button_theme(mut self, text_button_theme: TextButtonThemeData) -> ThemeData {
        self.text_button_theme = text_button_theme;
        self
    }

    pub fn with_outlined_button_theme(
        mut self,
        outlined_button_theme: OutlinedButtonThemeData,
    ) -> ThemeData {
        self.outlined_button_theme = outlined_button_theme;
        self
    }

    pub fn with_navigation_rail_theme(
        mut self,
        navigation_rail_theme: NavigationRailThemeData,
    ) -> ThemeData {
        self.navigation_rail_theme = navigation_rail_theme;
        self
    }

    pub fn with_bottom_navigation_bar_theme(
        mut self,
        bottom_navigation_bar_theme: BottomNavigationBarThemeData,
    ) -> ThemeData {
        self.bottom_navigation_bar_theme = bottom_navigation_bar_theme;
        self
    }

    pub fn with_drawer_theme(mut self, drawer_theme: DrawerThemeData) -> ThemeData {
        self.drawer_theme = drawer_theme;
        self
    }

    pub fn with_chip_theme(mut self, chip_theme: ChipThemeData) -> ThemeData {
        self.chip_theme = chip_theme;
        self
    }

    pub fn with_tab_bar_theme(mut self, tab_bar_theme: TabBarThemeData) -> ThemeData {
        self.tab_bar_theme = tab_bar_theme;
        self
    }

    pub fn with_data_table_theme(mut self, data_table_theme: DataTableThemeData) -> ThemeData {
        self.data_table_theme = data_table_theme;
        self
    }

    pub fn with_list_tile_theme(mut self, list_tile_theme: ListTileThemeData) -> ThemeData {
        self.list_tile_theme = list_tile_theme;
        self
    }

    pub fn with_dialog_theme(mut self, dialog_theme: DialogThemeData) -> ThemeData {
        self.dialog_theme = dialog_theme;
        self
    }

    pub fn with_app_bar_theme(mut self, app_bar_theme: AppBarThemeData) -> ThemeData {
        self.app_bar_theme = app_bar_theme;
        self
    }

    pub fn with_bottom_sheet_theme(
        mut self,
        bottom_sheet_theme: BottomSheetThemeData,
    ) -> ThemeData {
        self.bottom_sheet_theme = bottom_sheet_theme;
        self
    }

    pub fn with_snack_bar_theme(mut self, snack_bar_theme: SnackBarThemeData) -> ThemeData {
        self.snack_bar_theme = snack_bar_theme;
        self
    }

    pub fn with_checkbox_theme(mut self, checkbox_theme: CheckboxThemeData) -> ThemeData {
        self.checkbox_theme = checkbox_theme;
        self
    }

    pub fn with_radio_theme(mut self, radio_theme: RadioThemeData) -> ThemeData {
        self.radio_theme = radio_theme;
        self
    }

    pub fn with_switch_theme(mut self, switch_theme: SwitchThemeData) -> ThemeData {
        self.switch_theme = switch_theme;
        self
    }

    pub fn with_progress_indicator_theme(
        mut self,
        progress_indicator_theme: ProgressIndicatorThemeData,
    ) -> ThemeData {
        self.progress_indicator_theme = progress_indicator_theme;
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
            divider_theme: DividerThemeData::lerp(&a.divider_theme, &b.divider_theme, t),
            card_theme: CardThemeData::lerp(&a.card_theme, &b.card_theme, t),
            badge_theme: BadgeThemeData::lerp(&a.badge_theme, &b.badge_theme, t),
            tooltip_theme: TooltipThemeData::lerp(&a.tooltip_theme, &b.tooltip_theme, t),
            progress_indicator_theme: ProgressIndicatorThemeData::lerp(
                &a.progress_indicator_theme,
                &b.progress_indicator_theme,
                t,
            ),
            checkbox_theme: CheckboxThemeData::lerp(&a.checkbox_theme, &b.checkbox_theme, t),
            radio_theme: RadioThemeData::lerp(&a.radio_theme, &b.radio_theme, t),
            switch_theme: SwitchThemeData::lerp(&a.switch_theme, &b.switch_theme, t),
            app_bar_theme: AppBarThemeData::lerp(&a.app_bar_theme, &b.app_bar_theme, t),
            bottom_sheet_theme: BottomSheetThemeData::lerp(
                &a.bottom_sheet_theme,
                &b.bottom_sheet_theme,
                t,
            ),
            snack_bar_theme: SnackBarThemeData::lerp(&a.snack_bar_theme, &b.snack_bar_theme, t),
            list_tile_theme: ListTileThemeData::lerp(&a.list_tile_theme, &b.list_tile_theme, t),
            dialog_theme: DialogThemeData::lerp(&a.dialog_theme, &b.dialog_theme, t),
            chip_theme: ChipThemeData::lerp(&a.chip_theme, &b.chip_theme, t),
            tab_bar_theme: TabBarThemeData::lerp(&a.tab_bar_theme, &b.tab_bar_theme, t),
            data_table_theme: DataTableThemeData::lerp(&a.data_table_theme, &b.data_table_theme, t),
            navigation_rail_theme: NavigationRailThemeData::lerp(
                &a.navigation_rail_theme,
                &b.navigation_rail_theme,
                t,
            ),
            bottom_navigation_bar_theme: BottomNavigationBarThemeData::lerp(
                &a.bottom_navigation_bar_theme,
                &b.bottom_navigation_bar_theme,
                t,
            ),
            drawer_theme: DrawerThemeData::lerp(&a.drawer_theme, &b.drawer_theme, t),
            elevated_button_theme: ElevatedButtonThemeData::lerp(
                &a.elevated_button_theme,
                &b.elevated_button_theme,
                t,
            ),
            filled_button_theme: FilledButtonThemeData::lerp(
                &a.filled_button_theme,
                &b.filled_button_theme,
                t,
            ),
            text_button_theme: TextButtonThemeData::lerp(
                &a.text_button_theme,
                &b.text_button_theme,
                t,
            ),
            outlined_button_theme: OutlinedButtonThemeData::lerp(
                &a.outlined_button_theme,
                &b.outlined_button_theme,
                t,
            ),
            icon_button_theme: IconButtonThemeData::lerp(
                &a.icon_button_theme,
                &b.icon_button_theme,
                t,
            ),
            banner_theme: MaterialBannerThemeData::lerp(&a.banner_theme, &b.banner_theme, t),
            expansion_tile_theme: ExpansionTileThemeData::lerp(
                &a.expansion_tile_theme,
                &b.expansion_tile_theme,
                t,
            ),
            // Upstream's `ButtonThemeData` has no `lerp`: its fields are
            // metrics rather than paint, and a button bar does not animate
            // between two widths.
            button_theme: nearer.button_theme.clone(),
            scrollbar_theme: ScrollbarThemeData::lerp(&a.scrollbar_theme, &b.scrollbar_theme, t),
            menu_theme: MenuThemeData::lerp(&a.menu_theme, &b.menu_theme, t),
            menu_bar_theme: MenuBarThemeData::lerp(&a.menu_bar_theme, &b.menu_bar_theme, t),
            menu_button_theme: MenuButtonThemeData::lerp(
                &a.menu_button_theme,
                &b.menu_button_theme,
                t,
            ),
            segmented_button_theme: SegmentedButtonThemeData::lerp(
                &a.segmented_button_theme,
                &b.segmented_button_theme,
                t,
            ),
            floating_action_button_theme: FloatingActionButtonThemeData::lerp(
                &a.floating_action_button_theme,
                &b.floating_action_button_theme,
                t,
            ),
            toggle_buttons_theme: ToggleButtonsThemeData::lerp(
                &a.toggle_buttons_theme,
                &b.toggle_buttons_theme,
                t,
            ),
            search_bar_theme: SearchBarThemeData::lerp(&a.search_bar_theme, &b.search_bar_theme, t),
            search_view_theme: SearchViewThemeData::lerp(
                &a.search_view_theme,
                &b.search_view_theme,
                t,
            ),
            time_picker_theme: TimePickerThemeData::lerp(
                &a.time_picker_theme,
                &b.time_picker_theme,
                t,
            ),
            date_picker_theme: DatePickerThemeData::lerp(
                &a.date_picker_theme,
                &b.date_picker_theme,
                t,
            ),
            // Upstream's `InputDecorationThemeData` has no `lerp`: its
            // borders are shapes and its flags are flags, and a field does
            // not animate between two of them.
            input_decoration_theme: nearer.input_decoration_theme.clone(),
            icon_theme: IconThemeData::lerp(&a.icon_theme, &b.icon_theme, t),
            text_selection_theme: TextSelectionThemeData::lerp(
                &a.text_selection_theme,
                &b.text_selection_theme,
                t,
            ),
            popup_menu_theme: PopupMenuThemeData::lerp(&a.popup_menu_theme, &b.popup_menu_theme, t),
            dropdown_menu_theme: DropdownMenuThemeData::lerp(
                &a.dropdown_menu_theme,
                &b.dropdown_menu_theme,
                t,
            ),
            bottom_app_bar_theme: BottomAppBarThemeData::lerp(
                &a.bottom_app_bar_theme,
                &b.bottom_app_bar_theme,
                t,
            ),
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
            .map(|data| (*data).clone())
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
        let component_theme = data.to_component_theme();
        provide(data, provide(component_theme, child))
    }
}

/// Upstream `ThemeDataTween`.
#[derive(Clone, Debug)]
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
/// Upstream is an `ImplicitlyAnimatedWidget` over a [`ThemeDataTween`], and
/// so is this: a target that changes mid-flight restarts the walk *from where
/// it is now* rather than from the old target, which is the rule that keeps
/// two theme changes in quick succession from jumping backwards. The crate's
/// general implicit animation ([`crate::implicit::animated`]) wants a `Copy`
/// value and a `ThemeData` is not one, so the same rule is written out here.
pub struct AnimatedTheme<F> {
    data: ThemeData,
    duration: std::time::Duration,
    build: F,
}

/// What an [`AnimatedTheme`] is part-way through.
#[derive(Default)]
pub struct AnimatedThemeState {
    from: Option<ThemeData>,
    to: Option<ThemeData>,
    started_micros: Option<i64>,
    now_micros: i64,
    restart_pending: bool,
}

impl AnimatedThemeState {
    /// How far along the walk is.
    fn progress(&self, duration: std::time::Duration) -> f32 {
        let Some(started) = self.started_micros else {
            return 1.0;
        };
        let total = duration.as_micros() as f32;
        if total <= 0.0 {
            return 1.0;
        }
        (((self.now_micros - started) as f32) / total).clamp(0.0, 1.0)
    }

    fn value(&self, duration: std::time::Duration) -> Option<ThemeData> {
        let from = self.from.as_ref()?;
        let to = self.to.as_ref()?;
        Some(ThemeData::lerp(from, to, self.progress(duration)))
    }
}

impl<F> AnimatedTheme<F>
where
    F: Fn(ThemeData) -> AnyWidget + 'static,
{
    pub fn new(data: ThemeData, duration: std::time::Duration, build: F) -> AnimatedTheme<F> {
        AnimatedTheme {
            data,
            duration,
            build,
        }
    }

    pub fn into_widget(self) -> AnyWidget {
        crate::framework::stateful(self)
    }
}

impl<F> crate::framework::StatefulComponent for AnimatedTheme<F>
where
    F: Fn(ThemeData) -> AnyWidget + 'static,
{
    type State = AnimatedThemeState;

    fn did_update_widget(&self, _old: &Self, state: &mut Self::State) {
        state.restart_pending = state.to.as_ref().is_some_and(|to| *to != self.data);
    }

    fn advance(&self, state: &mut Self::State, frame_time_micros: i64) -> bool {
        if state.to.is_none() {
            // First frame: sitting on the target. An implicit animation does
            // not animate its way in from nowhere.
            state.from = Some(self.data.clone());
            state.to = Some(self.data.clone());
            state.now_micros = frame_time_micros;
            return false;
        }
        if state.restart_pending {
            state.restart_pending = false;
            let current = state
                .value(self.duration)
                .unwrap_or_else(|| self.data.clone());
            state.from = Some(current);
            state.to = Some(self.data.clone());
            state.started_micros = Some(frame_time_micros);
        }
        state.now_micros = frame_time_micros;
        let was_walking = state.started_micros.is_some();
        let walking = was_walking && state.progress(self.duration) < 1.0;
        if was_walking && !walking {
            // The frame that lands is the frame that shows the target.
            state.started_micros = None;
            return true;
        }
        walking
    }

    fn build(
        &self,
        state: &Self::State,
        _handle: crate::framework::StateHandle<Self::State>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let current = state
            .value(self.duration)
            .unwrap_or_else(|| self.data.clone());
        (self.build)(current)
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
