//! Ports of `material/app.dart` and `material/material_localizations.dart`.
//!
//! The application at the top of a Material tree, the scroll behaviour it
//! installs, and the strings it falls back to.

use crate::pickers::TimeOfDayFormat;
use crate::platform::Brightness;

/// Upstream `ScrollPlatform`, declared with the thing it describes in
/// [`crate::scroll_plumbing`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::scroll_plumbing::ScrollPlatform;

impl ScrollPlatform {
    /// The three upstream treats as desktop in `buildScrollbar`.
    pub fn is_desktop(self) -> bool {
        matches!(
            self,
            ScrollPlatform::Linux | ScrollPlatform::MacOS | ScrollPlatform::Windows
        )
    }
}

/// Upstream `ScrollAxis`, declared with the thing it describes in
/// [`crate::scroll_plumbing`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::scroll_plumbing::ScrollAxis;

/// What `buildOverscrollIndicator` wraps a scrollable in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverscrollDecoration {
    /// Upstream's `AndroidOverscrollIndicator.stretch`, the Material 3 default.
    Stretching,
    /// Upstream's `AndroidOverscrollIndicator.glow`.
    Glowing,
    /// The child, undecorated.
    None,
}

/// Upstream `MaterialScrollBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialScrollBehavior;

impl MaterialScrollBehavior {
    /// Upstream `buildScrollbar`, returning whether a `Scrollbar` is wrapped
    /// around the child.
    ///
    /// Two things fall out of the switch that are easier to see stated:
    ///
    /// * **A horizontal scrollable never gets a scrollbar**, on any platform.
    ///   The `Axis.horizontal` arm returns the child before the platform is
    ///   even consulted.
    /// * Of the vertical ones, only the three desktops get one. On a touch
    ///   platform you move a list by dragging the list, so a bar to grab would
    ///   be furniture nobody uses.
    ///
    /// The `assert(details.controller != null)` sits inside the desktop arm
    /// alone: a scrollbar needs something to attach to, and the platforms that
    /// do not build one have no such requirement.
    pub fn builds_scrollbar(axis: ScrollAxis, platform: ScrollPlatform) -> bool {
        match axis {
            ScrollAxis::Horizontal => false,
            ScrollAxis::Vertical => platform.is_desktop(),
        }
    }

    /// Whether a controller is required for this combination -- exactly the
    /// cases that build a scrollbar.
    pub fn requires_controller(axis: ScrollAxis, platform: ScrollPlatform) -> bool {
        MaterialScrollBehavior::builds_scrollbar(axis, platform)
    }

    /// Upstream `buildOverscrollIndicator`.
    ///
    /// **Only Android is decorated.** Which reads oddly until you line it up
    /// with the physics: iOS scrollables bounce, and the bounce *is* the
    /// feedback that you have reached the end. Android's clamp instead, so
    /// something has to be drawn to say so. The desktops scroll under a cursor
    /// and get nothing either.
    ///
    /// So the platforms that get an overscroll indicator are exactly the ones
    /// whose physics do not already show you the edge.
    ///
    /// `useMaterial3` chooses between the two Android treatments, and the
    /// stretch is the newer one.
    pub fn overscroll_indicator(
        platform: ScrollPlatform,
        use_material3: bool,
    ) -> OverscrollDecoration {
        match platform {
            ScrollPlatform::Android if use_material3 => OverscrollDecoration::Stretching,
            ScrollPlatform::Android => OverscrollDecoration::Glowing,
            _ => OverscrollDecoration::None,
        }
    }

    /// Both methods carry the same note -- *"When modifying this function,
    /// consider modifying the implementation in the base class `ScrollBehavior`
    /// as well."* -- which is a duplication upstream has chosen to flag rather
    /// than factor out.
    pub fn mirrors_the_base_class() -> bool {
        true
    }
}

/// Upstream `ThemeMode`: which of an app's themes applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Follow the platform.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// Upstream's `isSystem` / `isLight` / `isDark` getters.
    pub fn is_system(self) -> bool {
        matches!(self, ThemeMode::System)
    }

    pub fn is_light(self) -> bool {
        matches!(self, ThemeMode::Light)
    }

    pub fn is_dark(self) -> bool {
        matches!(self, ThemeMode::Dark)
    }
}

/// Which of an app's five theme candidates `_themeBuilder` settles on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    HighContrastDark,
    Dark,
    HighContrast,
    /// The app's ordinary `theme`.
    Given,
    /// `ThemeData()`, when the app gave none at all.
    Fallback,
}

/// Upstream `MaterialApp`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialApp {
    pub has_home: bool,
    pub has_routes: bool,
    pub has_on_generate_route: bool,
    pub has_router_delegate: bool,
    pub has_router_config: bool,
    /// Upstream's `debugShowMaterialGrid`.
    pub debug_show_material_grid: bool,
    pub debug_show_checked_mode_banner: bool,
    /// Upstream's `themeMode`, which is **nullable** even though the
    /// constructor defaults it to `system`. Passing null explicitly is legal
    /// and is not the same as passing `system` -- see
    /// [`MaterialApp::dark_flag_agrees_with_the_theme`].
    pub theme_mode: Option<ThemeMode>,
    pub has_theme: bool,
    pub has_dark_theme: bool,
    pub has_high_contrast_theme: bool,
    pub has_high_contrast_dark_theme: bool,
}

impl MaterialApp {
    /// Upstream's `GridPaper` interval: the Material 8dp grid.
    pub const MATERIAL_GRID_INTERVAL: f32 = 8.0;

    pub fn new() -> MaterialApp {
        MaterialApp {
            has_home: true,
            has_routes: false,
            has_on_generate_route: false,
            has_router_delegate: false,
            has_router_config: false,
            debug_show_material_grid: false,
            debug_show_checked_mode_banner: true,
            theme_mode: Some(ThemeMode::System),
            has_theme: false,
            has_dark_theme: false,
            has_high_contrast_theme: false,
            has_high_contrast_dark_theme: false,
        }
    }

    /// Upstream's `useDarkTheme` inside `_themeBuilder`.
    ///
    /// Dark mode always, or system mode on a dark platform. **Light mode never
    /// goes dark**, whatever the platform says -- an app that asked for light
    /// asked for light.
    pub fn uses_dark_theme(&self, platform: Brightness) -> bool {
        let mode = self.theme_mode.unwrap_or(ThemeMode::System);
        mode == ThemeMode::Dark || (mode == ThemeMode::System && platform == Brightness::Dark)
    }

    /// Upstream's `_themeBuilder` cascade.
    ///
    /// # The third arm does not ask whether it is dark
    ///
    /// The obvious rewrite -- decide the brightness, then pick the contrast
    /// within it -- gets this wrong. Upstream's third arm is
    ///
    /// ```dart
    /// } else if (highContrast && widget.highContrastTheme != null) {
    /// ```
    ///
    /// with **no test for `useDarkTheme`**. So an app in dark mode, with high
    /// contrast on, that supplied a `highContrastTheme` but no
    /// `highContrastDarkTheme` and no `darkTheme`, gets the **light**
    /// high-contrast theme. The arms are tried in order and the first that
    /// fits wins; they are not a decision tree over (brightness, contrast).
    ///
    /// Whether that is what anyone intended is a separate question. It is what
    /// the code does, and an app that supplies only some of the four can
    /// observe it.
    pub fn choose_theme(&self, platform: Brightness, high_contrast: bool) -> ThemeChoice {
        let dark = self.uses_dark_theme(platform);
        if dark && high_contrast && self.has_high_contrast_dark_theme {
            ThemeChoice::HighContrastDark
        } else if dark && self.has_dark_theme {
            ThemeChoice::Dark
        } else if high_contrast && self.has_high_contrast_theme {
            ThemeChoice::HighContrast
        } else if self.has_theme {
            ThemeChoice::Given
        } else {
            ThemeChoice::Fallback
        }
    }

    /// Upstream's `_isDarkTheme`, which is handed to the text-selection
    /// controls as `isDarkTheme`.
    ///
    /// It computes the same idea as [`MaterialApp::uses_dark_theme`] and
    /// **spells it differently**:
    ///
    /// ```dart
    /// return widget.themeMode == ThemeMode.dark ||
    ///     widget.themeMode == ThemeMode.system &&
    ///         MediaQuery.platformBrightnessOf(context) == Brightness.dark;
    /// ```
    ///
    /// No `?? ThemeMode.system`. So with `themeMode` explicitly null on a dark
    /// platform, `_themeBuilder` reads the null as system and picks the dark
    /// theme, while this returns **false** -- a dark app whose selection
    /// handles are told they are on a light one.
    ///
    /// Ported as it is rather than corrected, for the reason
    /// [`crate::cupertino::CupertinoFormRow::error_color`] gives: a difference
    /// copied on purpose stays comparable.
    pub fn is_dark_theme_flag(&self, platform: Brightness) -> bool {
        self.theme_mode == Some(ThemeMode::Dark)
            || (self.theme_mode == Some(ThemeMode::System) && platform == Brightness::Dark)
    }

    /// Whether the two spellings agree, for this app and this platform.
    pub fn dark_flag_agrees_with_the_theme(&self, platform: Brightness) -> bool {
        self.is_dark_theme_flag(platform) == self.uses_dark_theme(platform)
    }

    /// Upstream's `MaterialApp.router` constructor:
    /// `assert(routerDelegate != null || routerConfig != null)`.
    ///
    /// An **at least one**, which is a third shape again after the exclusions
    /// and the implication -- a router with neither has no way to turn a URL
    /// into a screen.
    pub fn router_is_configured(&self) -> bool {
        self.has_router_delegate || self.has_router_config
    }

    /// Upstream wraps the app in a `GridPaper` **inside an `assert(() { ... }())`
    /// block**, so the overlay is not merely defaulted off in release -- the
    /// code that draws it is not there at all.
    pub fn shows_material_grid(&self, debug_build: bool) -> bool {
        debug_build && self.debug_show_material_grid
    }
}

impl Default for MaterialApp {
    fn default() -> Self {
        MaterialApp::new()
    }
}

/// Upstream `MaterialLocalizations`: the interface every Material widget reads
/// its strings through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialLocalizations;

impl MaterialLocalizations {
    /// Upstream's `of` asserts `debugCheckHasMaterialLocalizations` and then
    /// force-unwraps, so a missing delegate is a debug error with an
    /// explanation rather than a null in release.
    pub fn of(present: bool) -> Option<MaterialLocalizations> {
        present.then_some(MaterialLocalizations)
    }
}

/// Upstream `DefaultMaterialLocalizations`, whose own doc says what it is:
/// *"Constructs an object that defines the material widgets' localized strings
/// for **US English (only)**."*
///
/// It `implements MaterialLocalizations` rather than extending it, so every
/// member has to be written out and a new one added upstream breaks this class
/// loudly rather than inheriting something wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultMaterialLocalizations;

impl DefaultMaterialLocalizations {
    /// Upstream's `_shortWeekdays`, and above it -- twice, once for this list
    /// and once for the long one -- sits this comment:
    ///
    /// ```dart
    /// // Ordered to match DateTime.monday=1, DateTime.sunday=6
    /// ```
    ///
    /// **`DateTime.sunday` is 7.** The list is seven entries indexed by
    /// `date.weekday - DateTime.monday`, so Sunday lands at *index* 6, and the
    /// comment has written the index where it names the constant.
    ///
    /// The code is right and only the comment is wrong, which is the kind of
    /// wrong that costs a reader real time: taken at its word it gives
    /// `sunday - monday = 6 - 1 = 5`, and index 5 is Saturday.
    pub const SHORT_WEEKDAYS: [&'static str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    pub const SHORT_MONTHS: [&'static str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    /// Dart's `DateTime.monday`.
    pub const MONDAY: u32 = 1;
    /// Dart's `DateTime.sunday`, which is what the comment above got wrong.
    pub const SUNDAY: u32 = 7;

    /// Upstream's indexing, `_shortWeekdays[date.weekday - DateTime.monday]`.
    pub fn short_weekday(weekday: u32) -> &'static str {
        DefaultMaterialLocalizations::SHORT_WEEKDAYS
            [(weekday - DefaultMaterialLocalizations::MONDAY) as usize]
    }

    /// Upstream `_formatTwoDigitZeroPad`, which asserts its own range:
    /// *"Formats `number` using two digits, assuming it's in the 0-99 inclusive
    /// range. Not designed to format values outside this range."*
    /// Upstream `formatDecimal`: thousands separators, and nothing else.
    ///
    /// # Everything under a thousand leaves by the front door
    ///
    /// `if (number > -1000 && number < 1000) return number.toString();` -- and
    /// that early return is not only speed. Below a thousand there is no group
    /// to separate, so the general path would be building the same string the
    /// long way round.
    ///
    /// # The grouping is anchored at the units digit, not at the front
    ///
    /// Upstream walks left to right but tests `(maxDigitIndex - i) % 3 == 0`,
    /// which measures from the **right**. So 1234 groups as `1,234` and not
    /// `123,4`: how many digits there are in total never moves where the
    /// commas fall.
    ///
    /// # The sign is written before the digits are looked at
    ///
    /// The buffer opens with `number < 0 ? '-' : ''` and everything after it
    /// comes from `number.abs()`, so the minus sign is never part of the
    /// grouping and `-1000` cannot come out as `-,1000`.
    ///
    /// That `abs()` is where Dart and Rust part company: Dart's is on a 64-bit
    /// int that has no trapping, and Rust's would panic in debug on
    /// [`i64::MIN`], whose absolute value does not fit. Handled here rather
    /// than inherited, since a number nobody will ever format is still not a
    /// reason to abort.
    pub fn format_decimal(number: i64) -> String {
        if number > -1000 && number < 1000 {
            return number.to_string();
        }
        // `unsigned_abs` rather than `abs`: the minimum has no positive
        // counterpart in the signed range and upstream's `abs()` is on a type
        // that does not trap.
        let digits = number.unsigned_abs().to_string();
        let mut result = String::new();
        if number < 0 {
            result.push('-');
        }
        let last = digits.len() - 1;
        for (index, digit) in digits.chars().enumerate() {
            result.push(digit);
            if index < last && (last - index) % 3 == 0 {
                result.push(',');
            }
        }
        result
    }

    /// Upstream `formatHour`, for the two formats this localization supports.
    ///
    /// # A twelve-hour clock has no zero
    ///
    /// `hourOfPeriod == 0 ? 12 : hourOfPeriod`. Midnight and noon are written
    /// **12**, and the hour that would be zero is the one the clock face puts
    /// at the top.
    ///
    /// # And it supports two of the six formats, refusing the rest
    ///
    /// Upstream's switch throws `AssertionError('$runtimeType does not support
    /// $format')` for the other four. `DefaultMaterialLocalizations` is the
    /// English one, not a general one: `a_space_h_colon_mm`, `frenchCanadian`,
    /// `H_colon_mm` and `HH_dot_mm` belong to localizations that speak those
    /// languages. Returned as an error here rather than thrown, so the refusal
    /// can be tested.
    ///
    /// # Why none of this goes through a date formatter
    ///
    /// Upstream says so where `formatTimeOfDay` would have called one, and the
    /// second reason is the interesting one: `DateFormat` "operates on
    /// DateTime, which is sensitive to time eras and time zones, while here we
    /// want to format hour and minute within one day no matter what date the
    /// day falls on."
    ///
    /// **A time of day is not a moment.** Putting one through a date type
    /// would drag in a date it does not have, and a zone it was never in.
    ///
    /// The first reason is about consistency rather than correctness:
    /// `DateFormat` "supports more formats than our material time picker
    /// does", and the picker and the string had better agree.
    pub fn format_hour(hour: u32, always_use_24_hour_format: bool) -> Result<String, &'static str> {
        DefaultMaterialLocalizations::format_hour_in(
            DefaultMaterialLocalizations::time_of_day_format(always_use_24_hour_format),
            hour,
        )
    }

    /// Upstream's `timeOfDayFormat`: which of the six patterns **these**
    /// localizations produce.
    ///
    /// Two of six, and that is the whole of it -- `alwaysUse24HourFormat
    /// ? HH_colon_mm : h_colon_mm_space_a`. `DefaultMaterialLocalizations` is
    /// US English only, so the dot, the Canadian `h`, the unpadded 24-hour and
    /// the leading day period all belong to locales it does not speak.
    pub fn time_of_day_format(always_use_24_hour_format: bool) -> TimeOfDayFormat {
        if always_use_24_hour_format {
            TimeOfDayFormat::HH_colon_mm
        } else {
            TimeOfDayFormat::h_colon_mm_space_a
        }
    }

    /// Upstream's `formatHour`, which **is total on only two of the six
    /// formats** and throws an `AssertionError` for the rest.
    ///
    /// That is not an oversight to be smoothed over. A localizations subclass
    /// is free to return any of the six from `timeOfDayFormat`, and this one
    /// can only write two of them; refusing is how it says so, rather than
    /// picking the nearest and being quietly wrong in four locales.
    ///
    /// The port took `always_use_24_hour_format` straight through and never
    /// formed the `TimeOfDayFormat` at all -- which happened to give the right
    /// answer, because the only two formats it can reach are the two that
    /// work, **and so the refusal had nowhere to live.** Taking the format as
    /// an argument puts the other four back within reach of a caller and of a
    /// test.
    ///
    /// The twelve-hour arm's `hourOfPeriod == 0 ? 12 : hourOfPeriod` is why
    /// midnight and noon read as 12 rather than 0.
    pub fn format_hour_in(format: TimeOfDayFormat, hour: u32) -> Result<String, &'static str> {
        if hour > 23 {
            return Err("an hour of the day is 0 to 23");
        }
        match format {
            TimeOfDayFormat::HH_colon_mm => {
                DefaultMaterialLocalizations::format_two_digit_zero_pad(hour)
                    .ok_or("two digits only")
            }
            TimeOfDayFormat::h_colon_mm_space_a => {
                let hour_of_period = hour % 12;
                Ok(DefaultMaterialLocalizations::format_decimal(
                    if hour_of_period == 0 {
                        12
                    } else {
                        hour_of_period as i64
                    },
                ))
            }
            TimeOfDayFormat::a_space_h_colon_mm
            | TimeOfDayFormat::FrenchCanadian
            | TimeOfDayFormat::H_colon_mm
            | TimeOfDayFormat::HH_dot_mm => {
                Err("DefaultMaterialLocalizations does not support this format")
            }
        }
    }

    pub fn format_two_digit_zero_pad(number: u32) -> Option<String> {
        if number >= 100 {
            return None;
        }
        Some(format!("{number:02}"))
    }

    /// Upstream `formatMinute`, which does the same padding **inline** rather
    /// than calling the helper above -- so the two-digit rule exists twice in
    /// one class, once with a range assert and once without.
    pub fn format_minute(minute: u32) -> String {
        if minute < 10 {
            format!("0{minute}")
        } else {
            minute.to_string()
        }
    }

    /// Upstream `formatCompactDate`, carrying the comment
    /// `// Assumes US mm/dd/yyyy format` -- the default localization saying out
    /// loud that it is one locale rather than a neutral one.
    ///
    /// Note the year takes a different route: month and day go through the
    /// 0-99 helper, and the year is padded with `padLeft(4, '0')` because it is
    /// not a two-digit number.
    pub fn format_compact_date(year: i32, month: u32, day: u32) -> Option<String> {
        let month = DefaultMaterialLocalizations::format_two_digit_zero_pad(month)?;
        let day = DefaultMaterialLocalizations::format_two_digit_zero_pad(day)?;
        Some(format!("{month}/{day}/{year:04}"))
    }

    /// Upstream `tabLabel`, which asserts `tabIndex >= 1` and `tabCount >= 1`.
    ///
    /// **One-based, where [`crate::tabs::TabController::index`] is zero-based.**
    /// The number a reader hears is not the number the code counts with, and
    /// the assert is what stops the two conventions being confused at the
    /// boundary: passing the controller's index straight through would announce
    /// "Tab 0 of 3" and trip on the very first tab.
    pub fn tab_label(tab_index: u32, tab_count: u32) -> Option<String> {
        if tab_index < 1 || tab_count < 1 {
            return None;
        }
        Some(format!("Tab {tab_index} of {tab_count}"))
    }

    /// Upstream's English for the strings a widget in this crate asks for.
    ///
    /// # Why these and not the other hundred and thirty
    ///
    /// `DefaultMaterialLocalizations` has a hundred and fifty-eight members
    /// and copying the table down would put a hundred and thirty unread
    /// strings in the crate. Each of these was reached from the other
    /// direction: a widget here already knew which string it wanted -- the
    /// action buttons have carried the key names since they were written --
    /// and there was nothing to resolve them against, so the widget did
    /// without.
    ///
    /// A string arrives when something asks for it. That keeps the table
    /// honest: every entry has a caller, and `unwired.py`'s whole argument is
    /// that one without is indistinguishable from one that is wrong.
    pub const BACK_BUTTON_TOOLTIP: &'static str = "Back";
    /// Upstream's `closeButtonTooltip`.
    pub const CLOSE_BUTTON_TOOLTIP: &'static str = "Close";
    /// Upstream's `openAppDrawerTooltip`. **Not "Open drawer"**: the word the
    /// reader hears is about what is inside, not about the panel.
    pub const OPEN_APP_DRAWER_TOOLTIP: &'static str = "Open navigation menu";
    /// Upstream's `drawerLabel`, which the drawer itself carries as its
    /// semantics label -- the same idea as the tooltip above, said to a
    /// different listener.
    pub const DRAWER_LABEL: &'static str = "Navigation menu";
    /// Upstream's `alertDialogLabel`, which an `AlertDialog` announces itself
    /// as when it was given no label of its own.
    pub const ALERT_DIALOG_LABEL: &'static str = "Alert";
    /// Upstream's `dialogLabel`, the same for a `SimpleDialog`. Two words for
    /// two shapes: an alert interrupts and a dialog asks, and a reader is
    /// told which before hearing the contents.
    pub const DIALOG_LABEL: &'static str = "Dialog";
    // -- An expansion tile's two sentences ---------------------------------
    //
    // Four strings that compose into two, and **two of the four are named the
    // wrong way round upstream**:
    //
    //     expandedHint  => 'Collapsed'   /// describes the expanded state
    //     collapsedHint => 'Expanded'    /// describes the collapsed state
    //
    // Read on their own they look like a straightforward bug. They are not,
    // because the pairing is crossed to match: upstream's own doc says
    // `expansionTileExpandedHint` is appended to `collapsedHint`, and
    // `expansionTileCollapsedHint` to `expandedHint`. Follow both crossings
    // and the sentences come out right --
    //
    //     "Expanded double tap to collapse"
    //     "Collapsed double tap to expand"
    //
    // -- so a port that tidied the names into agreement with their values,
    // and kept the obvious pairing, would produce two sentences that each say
    // the opposite of the truth. The names are copied as they are, and
    // [`Self::expansion_tile_hint`] is the only thing that pairs them.

    /// Upstream's `expandedHint`, which describes the **expanded** state and
    /// whose value is "Collapsed". See the note above.
    pub const EXPANDED_HINT: &'static str = "Collapsed";
    /// Upstream's `collapsedHint`, which describes the **collapsed** state and
    /// whose value is "Expanded". See the note above.
    pub const COLLAPSED_HINT: &'static str = "Expanded";
    /// Upstream's `expansionTileExpandedHint`.
    pub const EXPANSION_TILE_EXPANDED_HINT: &'static str = "double tap to collapse";
    /// Upstream's `expansionTileCollapsedHint`.
    pub const EXPANSION_TILE_COLLAPSED_HINT: &'static str = "double tap to expand";

    /// The whole hint for an expansion tile on iOS and macOS, which is the
    /// state followed by what a tap will do.
    ///
    /// The crossing lives here and nowhere else. `expanded` picks
    /// `collapsedHint` for the first half and `expansionTileExpandedHint` for
    /// the second, which reads backwards twice and comes out forwards.
    pub fn expansion_tile_hint(expanded: bool) -> String {
        if expanded {
            format!(
                "{} {}",
                Self::COLLAPSED_HINT,
                Self::EXPANSION_TILE_EXPANDED_HINT
            )
        } else {
            format!(
                "{} {}",
                Self::EXPANDED_HINT,
                Self::EXPANSION_TILE_COLLAPSED_HINT
            )
        }
    }

    // -- The date and time pickers ----------------------------------------
    //
    // Moved here from `pickers.rs`, which held them privately because the
    // crate had no localization layer when that module was written.
    //
    // Two were wrong on arrival: upstream's `dateRangeStartLabel` and
    // `dateRangeEndLabel` capitalise the D -- "Start Date", "End Date" -- next
    // to a `dateInputLabel` that also does and a `datePickerHelpText` that
    // does not. It reads like an inconsistency upstream never tidied, and
    // tidying it here is how a port stops being one.
    /// Upstream's `invalidDateFormatLabel`.
    pub const INVALID_DATE_FORMAT_LABEL: &'static str = "Invalid format.";
    /// Upstream's `dateOutOfRangeLabel`.
    pub const DATE_OUT_OF_RANGE_LABEL: &'static str = "Out of range.";
    /// Upstream's `dateHelpText`.
    pub const DATE_HELP_TEXT: &'static str = "mm/dd/yyyy";
    /// Upstream's `dateInputLabel`.
    pub const DATE_INPUT_LABEL: &'static str = "Enter Date";
    /// Upstream's `datePickerHelpText`.
    pub const DATE_PICKER_HELP_TEXT: &'static str = "Select date";
    /// Upstream's `cancelButtonLabel`.
    pub const CANCEL_BUTTON_LABEL: &'static str = "Cancel";
    /// Upstream's `okButtonLabel`.
    pub const OK_BUTTON_LABEL: &'static str = "OK";
    /// Upstream's `saveButtonLabel`.
    pub const SAVE_BUTTON_LABEL: &'static str = "Save";
    /// Upstream's `dateRangePickerHelpText`.
    pub const DATE_RANGE_PICKER_HELP_TEXT: &'static str = "Select range";
    /// Upstream's `invalidDateRangeLabel`.
    pub const INVALID_DATE_RANGE_LABEL: &'static str = "Invalid range.";
    /// Upstream's `dateRangeStartLabel`.
    pub const DATE_RANGE_START_LABEL: &'static str = "Start Date";
    /// Upstream's `dateRangeEndLabel`.
    pub const DATE_RANGE_END_LABEL: &'static str = "End Date";
    /// Upstream's `timePickerDialHelpText`.
    pub const TIME_PICKER_DIAL_HELP_TEXT: &'static str = "Select time";
    /// Upstream's `timePickerInputHelpText`.
    pub const TIME_PICKER_INPUT_HELP_TEXT: &'static str = "Enter time";
    /// Upstream's `invalidTimeLabel`.
    pub const INVALID_TIME_LABEL: &'static str = "Enter a valid time";
    /// Upstream's `timePickerHourLabel`.
    pub const TIME_PICKER_HOUR_LABEL: &'static str = "Hour";
    /// Upstream's `timePickerMinuteLabel`.
    pub const TIME_PICKER_MINUTE_LABEL: &'static str = "Minute";

    /// Upstream's `rowsPerPageTitle`, **with its colon**.
    ///
    /// The colon is part of the string rather than something the footer adds,
    /// which is how it has to be: a language that does not use one, or puts it
    /// elsewhere, changes the string and not the widget.
    pub const ROWS_PER_PAGE_TITLE: &'static str = "Rows per page:";

    /// Upstream's `pageRowsInfoTitle`: which rows of how many are showing.
    ///
    /// # The separator is an en dash
    ///
    /// `'$firstRow–$lastRow of $rowCount'` uses U+2013, not a hyphen. It is a
    /// range between two numbers, which is what an en dash is for, and it is
    /// exactly the detail a paraphrase loses -- a hyphen would read as a
    /// compound rather than a span, and nothing in a test that only checked
    /// the numbers would notice.
    ///
    /// `approximate` is upstream's `rowCountIsApproximate`, for a source that
    /// knows roughly how much it has -- a query that has not finished counting.
    /// It changes the sentence rather than the number: "of about 300" claims
    /// less than "of 300" does.
    pub fn page_rows_info_title(
        first_row: usize,
        last_row: usize,
        row_count: usize,
        approximate: bool,
    ) -> String {
        if approximate {
            format!("{first_row}\u{2013}{last_row} of about {row_count}")
        } else {
            format!("{first_row}\u{2013}{last_row} of {row_count}")
        }
    }

    /// Upstream's `selectedRowCountTitle`: how many rows are ticked.
    ///
    /// Three cases and not two. Zero is **"No items selected"** rather than
    /// "0 items selected", and one is **"1 item selected"** rather than
    /// "1 items selected" -- English has a singular, and a table that says
    /// "1 items" in its header says it every time anyone ticks a row.
    pub fn selected_row_count_title(selected: usize) -> String {
        match selected {
            0 => "No items selected".to_string(),
            1 => "1 item selected".to_string(),
            more => format!("{more} items selected"),
        }
    }

    /// Upstream's `refreshIndicatorSemanticLabel`: what the spinner at the top
    /// of a pulled-down list is called.
    ///
    /// It is the verb and not the noun -- "Refresh", not "Loading" -- because
    /// a reader meets it while the gesture is still theirs to complete or
    /// abandon, and what matters is what letting go will do.
    pub const REFRESH_INDICATOR_SEMANTIC_LABEL: &'static str = "Refresh";
    /// Upstream's `licensesPageTitle`, the heading of the page
    /// `showLicensePage` opens.
    pub const LICENSES_PAGE_TITLE: &'static str = "Licenses";
    /// Upstream's `showMenuTooltip`: what a menu button's glyph says it does.
    pub const SHOW_MENU_TOOLTIP: &'static str = "Show menu";
    /// Upstream's `popupMenuLabel`, which the opened menu carries as its own
    /// name -- so a reader hears what kind of thing has appeared before its
    /// contents are read out.
    pub const POPUP_MENU_LABEL: &'static str = "Popup menu";
    /// Upstream's `menuDismissLabel`, and **not the same string as
    /// [`Self::MODAL_BARRIER_DISMISS_LABEL`]**.
    ///
    /// A dialog's scrim says "Dismiss" and a menu's says "Dismiss menu". The
    /// extra word earns its place: a dialog's scrim is visibly dimmed and
    /// obviously belongs to the thing in front of it, while a menu's is
    /// invisible, so a reader who activates it needs to be told what it is
    /// that goes away.
    pub const MENU_DISMISS_LABEL: &'static str = "Dismiss menu";
    /// Upstream's `searchFieldLabel`, which is the search field's hint **and**
    /// the search route's name -- upstream assigns `routeName =
    /// searchFieldLabel` -- so one word does duty as the placeholder a reader
    /// sees and the announcement a reader hears on arriving.
    pub const SEARCH_FIELD_LABEL: &'static str = "Search";
    /// Upstream's `modalBarrierDismissLabel`: what the scrim behind a dialog
    /// announces.
    ///
    /// Without it a screen reader meets a full-screen region with no name and
    /// no indication that tapping it is how you leave.
    pub const MODAL_BARRIER_DISMISS_LABEL: &'static str = "Dismiss";

    /// Upstream's rule for a modal surface's own name, which its source
    /// writes out three times -- for `Drawer`, for `AlertDialog` and for
    /// `SimpleDialog` -- in the same shape each time:
    ///
    /// ```dart
    /// TargetPlatform.iOS || TargetPlatform.macOS => semanticLabel,
    /// TargetPlatform.android || ... => semanticLabel ?? theFallback,
    /// ```
    ///
    /// # The two Apple platforms get no fallback, and that is the rule
    ///
    /// An unnamed surface is announced as its kind everywhere else, and as
    /// nothing on iOS and macOS. It reads like an omission and is not:
    /// VoiceOver already says that a modal surface has appeared and that there
    /// is a way out of it, so a framework adding "Alert" on top is a second
    /// voice saying what the first one just said. TalkBack does not, so on
    /// Android the framework does.
    ///
    /// A caller's own label wins on every platform. The platforms disagree
    /// only about the unnamed case.
    ///
    /// Written once here rather than three times, because three copies of a
    /// rule are three places for it to drift, and upstream's repetition is a
    /// consequence of where the code sits rather than a distinction between
    /// the cases.
    pub fn modal_surface_label(
        platform: crate::editable_text::TargetPlatform,
        own: Option<&str>,
        fallback: &'static str,
    ) -> Option<String> {
        use crate::editable_text::TargetPlatform;
        match platform {
            TargetPlatform::IOS | TargetPlatform::MacOS => own.map(str::to_string),
            TargetPlatform::Android
            | TargetPlatform::Fuchsia
            | TargetPlatform::Linux
            | TargetPlatform::Windows => Some(own.unwrap_or(fallback).to_string()),
        }
    }

    /// Upstream's `deleteButtonTooltip`: what a chip's delete affordance
    /// says when the chip did not name something better.
    ///
    /// One word, and it is the only thing that tells a reader what the small
    /// unlabelled cross does. A chip that reaches this and gets nothing has a
    /// button a screen reader cannot describe.
    pub const DELETE_BUTTON_TOOLTIP: &'static str = "Delete";

    /// Upstream `licensesPackageDetailText`'s `assert(licenseCount >= 0)`,
    /// which in Dart is a real check and here is the type.
    pub fn license_count_is_valid(_count: u32) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ScrollPlatform; 6] = [
        ScrollPlatform::Android,
        ScrollPlatform::Fuchsia,
        ScrollPlatform::IOS,
        ScrollPlatform::Linux,
        ScrollPlatform::MacOS,
        ScrollPlatform::Windows,
    ];

    // -- What gets decorated and what does not -------------------------------------

    #[test]
    fn a_horizontal_scrollable_never_gets_a_scrollbar_on_any_platform() {
        for platform in ALL {
            assert!(
                !MaterialScrollBehavior::builds_scrollbar(ScrollAxis::Horizontal, platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn but_a_vertical_one_does_on_the_three_desktops() {
        for platform in ALL {
            assert_eq!(
                MaterialScrollBehavior::builds_scrollbar(ScrollAxis::Vertical, platform),
                platform.is_desktop(),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_controller_is_demanded_exactly_where_a_scrollbar_is_built() {
        // The assert lives inside the desktop arm, not above the switch.
        for platform in ALL {
            for axis in [ScrollAxis::Horizontal, ScrollAxis::Vertical] {
                assert_eq!(
                    MaterialScrollBehavior::requires_controller(axis, platform),
                    MaterialScrollBehavior::builds_scrollbar(axis, platform),
                    "{platform:?} {axis:?}"
                );
            }
        }
    }

    #[test]
    fn only_the_platform_whose_physics_hide_the_edge_is_told_where_the_edge_is() {
        // iOS bounces, and the bounce is the feedback. Android clamps.
        for platform in ALL {
            let decoration = MaterialScrollBehavior::overscroll_indicator(platform, true);
            if platform == ScrollPlatform::Android {
                assert_eq!(decoration, OverscrollDecoration::Stretching);
            } else {
                assert_eq!(decoration, OverscrollDecoration::None, "{platform:?}");
            }
        }
    }

    #[test]
    fn material_three_stretches_where_material_two_glowed() {
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::Android, true),
            OverscrollDecoration::Stretching
        );
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::Android, false),
            OverscrollDecoration::Glowing
        );
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::IOS, false),
            OverscrollDecoration::None,
            "and the choice does not reach the other platforms at all"
        );
    }

    // -- The comment that is wrong about a constant --------------------------------

    #[test]
    fn sunday_is_seven_and_lands_at_index_six() {
        assert_eq!(DefaultMaterialLocalizations::SUNDAY, 7);
        assert_eq!(
            DefaultMaterialLocalizations::SUNDAY - DefaultMaterialLocalizations::MONDAY,
            6
        );
        assert_eq!(DefaultMaterialLocalizations::short_weekday(7), "Sun");
    }

    #[test]
    fn reading_the_comment_literally_gives_the_wrong_day() {
        // "Ordered to match DateTime.monday=1, DateTime.sunday=6" -- taken at
        // its word, 6 - 1 = 5, and index 5 is Saturday.
        assert_eq!(DefaultMaterialLocalizations::SHORT_WEEKDAYS[5], "Sat");
        assert_eq!(DefaultMaterialLocalizations::SHORT_WEEKDAYS[6], "Sun");
        assert_ne!(
            DefaultMaterialLocalizations::SHORT_WEEKDAYS[5],
            DefaultMaterialLocalizations::short_weekday(7)
        );
    }

    #[test]
    fn every_weekday_maps_to_its_own_name() {
        let names: Vec<&str> = (1..=7)
            .map(DefaultMaterialLocalizations::short_weekday)
            .collect();
        assert_eq!(names, vec!["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
    }

    // -- One-based where the controller is zero-based -------------------------------

    #[test]
    fn the_number_a_reader_hears_is_not_the_number_the_code_counts_with() {
        assert_eq!(
            DefaultMaterialLocalizations::tab_label(1, 3).as_deref(),
            Some("Tab 1 of 3")
        );
        assert_eq!(
            DefaultMaterialLocalizations::tab_label(0, 3),
            None,
            "which is what a controller's index would have handed it"
        );
        assert_eq!(DefaultMaterialLocalizations::tab_label(1, 0), None);
    }

    // -- Formatting -----------------------------------------------------------------

    #[test]
    fn the_two_digit_helper_is_only_for_two_digit_numbers() {
        assert_eq!(
            DefaultMaterialLocalizations::format_two_digit_zero_pad(7).as_deref(),
            Some("07")
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_two_digit_zero_pad(99).as_deref(),
            Some("99")
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_two_digit_zero_pad(100),
            None
        );
    }

    #[test]
    fn the_same_padding_rule_is_written_twice_in_one_class() {
        // formatMinute inlines it rather than calling the helper, so it has no
        // range assert of its own.
        for minute in [0, 5, 9, 10, 59] {
            assert_eq!(
                DefaultMaterialLocalizations::format_minute(minute),
                DefaultMaterialLocalizations::format_two_digit_zero_pad(minute).unwrap(),
                "minute {minute}"
            );
        }
    }

    #[test]
    fn the_default_localization_is_one_locale_and_says_so() {
        assert_eq!(
            DefaultMaterialLocalizations::format_compact_date(2026, 8, 20).as_deref(),
            Some("08/20/2026"),
            "US mm/dd/yyyy, per the comment above it"
        );
    }

    #[test]
    fn the_year_is_padded_by_a_different_mechanism_because_it_is_not_two_digits() {
        assert_eq!(
            DefaultMaterialLocalizations::format_compact_date(7, 1, 2).as_deref(),
            Some("01/02/0007")
        );
    }

    // -- The application ------------------------------------------------------------

    #[test]
    fn a_router_needs_at_least_one_of_the_two_ways_to_configure_it() {
        let mut app = MaterialApp::new();
        assert!(!app.router_is_configured());

        app.has_router_delegate = true;
        assert!(app.router_is_configured());

        app.has_router_config = true;
        assert!(app.router_is_configured(), "and both is allowed");

        app.has_router_delegate = false;
        assert!(app.router_is_configured());
    }

    #[test]
    fn the_material_grid_is_not_merely_off_in_release_it_is_absent() {
        let mut app = MaterialApp::new();
        app.debug_show_material_grid = true;
        assert!(app.shows_material_grid(true));
        assert!(
            !app.shows_material_grid(false),
            "the GridPaper lives inside an assert block"
        );
        assert_eq!(MaterialApp::MATERIAL_GRID_INTERVAL, 8.0);
    }

    #[test]
    fn localizations_are_fetched_through_a_check_rather_than_a_null() {
        assert!(MaterialLocalizations::of(true).is_some());
        assert!(MaterialLocalizations::of(false).is_none());
    }
}

#[cfg(test)]
mod decimal_and_hour_tests {
    use super::*;

    #[test]
    fn everything_under_a_thousand_is_left_alone() {
        // Not only speed: below a thousand there is no group to separate, so
        // the general path would build the same string the long way round.
        assert_eq!(DefaultMaterialLocalizations::format_decimal(0), "0");
        assert_eq!(DefaultMaterialLocalizations::format_decimal(999), "999");
        assert_eq!(DefaultMaterialLocalizations::format_decimal(-999), "-999");
    }

    #[test]
    fn and_a_thousand_is_the_first_one_that_is_not() {
        // The boundary is exclusive on both sides, so 1000 and -1000 take the
        // general path and are the shortest strings with a comma in them.
        assert_eq!(DefaultMaterialLocalizations::format_decimal(1000), "1,000");
        assert_eq!(
            DefaultMaterialLocalizations::format_decimal(-1000),
            "-1,000"
        );
    }

    #[test]
    fn the_grouping_is_anchored_at_the_units_digit() {
        // Upstream walks left to right but measures `(maxDigitIndex - i) % 3`
        // from the right, so how many digits there are never moves where the
        // commas fall.
        assert_eq!(DefaultMaterialLocalizations::format_decimal(1234), "1,234");
        assert_eq!(
            DefaultMaterialLocalizations::format_decimal(12345),
            "12,345"
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_decimal(123456),
            "123,456"
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_decimal(1234567),
            "1,234,567"
        );
    }

    #[test]
    fn a_number_that_is_exactly_a_group_gets_no_leading_comma() {
        // The `i < maxDigitIndex` half of the condition: the last digit never
        // gets a comma after it, and the first never gets one before it.
        assert_eq!(
            DefaultMaterialLocalizations::format_decimal(1000000),
            "1,000,000"
        );
        assert!(!DefaultMaterialLocalizations::format_decimal(123456).starts_with(','));
    }

    #[test]
    fn the_sign_is_never_part_of_the_grouping() {
        // Written before any digit is looked at, and everything after comes
        // from the absolute value -- so `-1000` cannot come out as `-,1000`.
        for number in [-1000i64, -12345, -1234567] {
            let formatted = DefaultMaterialLocalizations::format_decimal(number);
            assert!(formatted.starts_with('-'), "{formatted}");
            assert!(!formatted.starts_with("-,"), "{formatted}");
            assert_eq!(
                formatted[1..],
                DefaultMaterialLocalizations::format_decimal(-number),
                "the digits are the same either side of zero"
            );
        }
    }

    #[test]
    fn the_smallest_integer_formats_instead_of_aborting() {
        // Dart's `abs()` is on a type that does not trap; Rust's would panic
        // in debug on a value whose absolute value does not fit. A number
        // nobody will format is still not a reason to abort.
        let formatted = DefaultMaterialLocalizations::format_decimal(i64::MIN);
        assert!(formatted.starts_with("-9,223,372,036,854,775,808"));
    }

    // -- The hour ---------------------------------------------------------------

    #[test]
    fn a_twelve_hour_clock_has_no_zero() {
        // Midnight and noon are written 12 -- the hour that would be zero is
        // the one the clock face puts at the top.
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(0, false),
            Ok(String::from("12"))
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(12, false),
            Ok(String::from("12"))
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(13, false),
            Ok(String::from("1"))
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(23, false),
            Ok(String::from("11"))
        );
    }

    #[test]
    fn and_a_twenty_four_hour_one_pads_instead() {
        // Two digits always, so a column of times lines up.
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(0, true),
            Ok(String::from("00"))
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(9, true),
            Ok(String::from("09"))
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(23, true),
            Ok(String::from("23"))
        );
    }

    #[test]
    fn the_two_formats_disagree_about_midnight_in_both_directions() {
        // Which is the whole of the difference: one writes the largest number
        // on the face, the other the smallest.
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(0, false),
            Ok(String::from("12"))
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour(0, true),
            Ok(String::from("00"))
        );
        assert_ne!(
            DefaultMaterialLocalizations::format_hour(0, false),
            DefaultMaterialLocalizations::format_hour(0, true)
        );
    }

    #[test]
    fn an_hour_outside_the_day_is_refused_rather_than_wrapped() {
        // Wrapping would turn a caller's mistake into a plausible time.
        assert!(DefaultMaterialLocalizations::format_hour(24, false).is_err());
        assert!(DefaultMaterialLocalizations::format_hour(99, true).is_err());
    }

    #[test]
    fn the_hour_goes_through_the_decimal_formatter_and_never_needs_it() {
        // Upstream writes `formatDecimal(hourOfPeriod == 0 ? 12 : ...)`, and
        // every value it can be given is under a thousand -- so the call is
        // always the early return. Pinned because it looks like a place a
        // comma could appear and is not one.
        for hour in 0..24u32 {
            let formatted = DefaultMaterialLocalizations::format_hour(hour, false).unwrap();
            assert!(!formatted.contains(','), "{hour}: {formatted}");
        }
    }
}

#[cfg(test)]
mod time_format_tests {
    use crate::material_app::DefaultMaterialLocalizations;
    use crate::pickers::{HourFormat, TimeOfDayFormat};

    #[test]
    fn six_patterns_collapse_onto_three_hours() {
        // The two twelve-hour patterns differ only in which side the day
        // period sits on, and the three padded ones only in the separator --
        // neither is a fact about the hour.
        assert_eq!(
            TimeOfDayFormat::h_colon_mm_space_a.hour_format(),
            HourFormat::h
        );
        assert_eq!(
            TimeOfDayFormat::a_space_h_colon_mm.hour_format(),
            HourFormat::h
        );
        assert_eq!(TimeOfDayFormat::H_colon_mm.hour_format(), HourFormat::H);
        for padded in [
            TimeOfDayFormat::HH_colon_mm,
            TimeOfDayFormat::HH_dot_mm,
            TimeOfDayFormat::FrenchCanadian,
        ] {
            assert_eq!(padded.hour_format(), HourFormat::HH, "{padded:?}");
        }
        // And every one of the six lands somewhere: no pattern is unaccounted.
        assert_eq!(TimeOfDayFormat::ALL.len(), 6);
    }

    #[test]
    fn and_the_collapse_really_loses_something() {
        // Guards against the arms agreeing: patterns that share an hour format
        // must still differ, or `hour_format` would not be collapsing anything.
        assert_eq!(
            TimeOfDayFormat::HH_colon_mm.hour_format(),
            TimeOfDayFormat::HH_dot_mm.hour_format()
        );
        assert_ne!(
            TimeOfDayFormat::HH_colon_mm.separator(),
            TimeOfDayFormat::HH_dot_mm.separator()
        );
        assert_ne!(TimeOfDayFormat::HH_colon_mm, TimeOfDayFormat::HH_dot_mm);
    }

    #[test]
    fn only_the_canadian_one_separates_with_a_letter() {
        assert_eq!(TimeOfDayFormat::FrenchCanadian.separator(), "h");
        assert_eq!(TimeOfDayFormat::HH_dot_mm.separator(), ".");
        for colon in [
            TimeOfDayFormat::HH_colon_mm,
            TimeOfDayFormat::H_colon_mm,
            TimeOfDayFormat::h_colon_mm_space_a,
            TimeOfDayFormat::a_space_h_colon_mm,
        ] {
            assert_eq!(colon.separator(), ":", "{colon:?}");
        }
    }

    #[test]
    fn a_day_period_is_what_makes_an_hour_run_to_twelve() {
        for format in TimeOfDayFormat::ALL {
            assert_eq!(
                format.uses_day_period(),
                !format.hour_format().is_twenty_four_hour(),
                "{format:?}"
            );
        }
        // Padding and the day period are separate questions: HH pads and runs
        // to 23, H does neither, h does not pad and runs to 12.
        assert!(HourFormat::HH.is_zero_padded());
        assert!(!HourFormat::H.is_zero_padded());
        assert!(!HourFormat::h.is_zero_padded());
        assert!(HourFormat::H.is_twenty_four_hour());
        assert!(!HourFormat::h.is_twenty_four_hour());
    }

    #[test]
    fn the_default_localizations_reach_only_two_of_the_six() {
        assert_eq!(
            DefaultMaterialLocalizations::time_of_day_format(false),
            TimeOfDayFormat::h_colon_mm_space_a
        );
        assert_eq!(
            DefaultMaterialLocalizations::time_of_day_format(true),
            TimeOfDayFormat::HH_colon_mm
        );
    }

    #[test]
    fn and_refuse_the_other_four_rather_than_guess() {
        // Upstream throws an AssertionError. A subclass may return any of the
        // six, and this one can write two; refusing is how it says so instead
        // of being quietly wrong in four locales.
        for unsupported in [
            TimeOfDayFormat::a_space_h_colon_mm,
            TimeOfDayFormat::FrenchCanadian,
            TimeOfDayFormat::H_colon_mm,
            TimeOfDayFormat::HH_dot_mm,
        ] {
            assert!(
                DefaultMaterialLocalizations::format_hour_in(unsupported, 9).is_err(),
                "{unsupported:?}"
            );
        }
        // The two it does reach work for every hour of the day.
        for hour in 0..24 {
            for supported in [
                TimeOfDayFormat::h_colon_mm_space_a,
                TimeOfDayFormat::HH_colon_mm,
            ] {
                assert!(
                    DefaultMaterialLocalizations::format_hour_in(supported, hour).is_ok(),
                    "{supported:?} {hour}"
                );
            }
        }
    }

    #[test]
    fn midnight_and_noon_read_as_twelve() {
        let twelve_hour = TimeOfDayFormat::h_colon_mm_space_a;
        assert_eq!(
            DefaultMaterialLocalizations::format_hour_in(twelve_hour, 0),
            Ok("12".to_string())
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour_in(twelve_hour, 12),
            Ok("12".to_string())
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour_in(twelve_hour, 13),
            Ok("1".to_string())
        );
        // Where the padded 24-hour one writes the hour it was given.
        assert_eq!(
            DefaultMaterialLocalizations::format_hour_in(TimeOfDayFormat::HH_colon_mm, 0),
            Ok("00".to_string())
        );
        assert_eq!(
            DefaultMaterialLocalizations::format_hour_in(TimeOfDayFormat::HH_colon_mm, 13),
            Ok("13".to_string())
        );
    }

    #[test]
    fn and_the_old_boolean_entry_point_still_agrees() {
        // `format_hour` is now the composition of the two steps, so it must
        // answer exactly as it did before the format came between them.
        for hour in 0..24 {
            for always in [false, true] {
                assert_eq!(
                    DefaultMaterialLocalizations::format_hour(hour, always),
                    DefaultMaterialLocalizations::format_hour_in(
                        DefaultMaterialLocalizations::time_of_day_format(always),
                        hour
                    )
                );
            }
        }
        assert!(DefaultMaterialLocalizations::format_hour(24, false).is_err());
    }
}

#[cfg(test)]
mod theme_mode_tests {
    use super::{MaterialApp, ThemeChoice, ThemeMode};
    use crate::platform::Brightness;

    /// An app with every theme supplied, so the cascade is free to pick any.
    fn fully_themed() -> MaterialApp {
        MaterialApp {
            has_theme: true,
            has_dark_theme: true,
            has_high_contrast_theme: true,
            has_high_contrast_dark_theme: true,
            ..MaterialApp::new()
        }
    }

    #[test]
    fn light_mode_stays_light_on_a_dark_platform() {
        let mut app = fully_themed();
        app.theme_mode = Some(ThemeMode::Light);
        assert!(!app.uses_dark_theme(Brightness::Dark));
        // And dark mode stays dark on a light one.
        app.theme_mode = Some(ThemeMode::Dark);
        assert!(app.uses_dark_theme(Brightness::Light));
    }

    #[test]
    fn and_system_mode_is_the_only_one_the_platform_moves() {
        let mut app = fully_themed();
        app.theme_mode = Some(ThemeMode::System);
        assert!(!app.uses_dark_theme(Brightness::Light));
        assert!(app.uses_dark_theme(Brightness::Dark));
        // Which is what `isSystem` is for.
        assert!(ThemeMode::System.is_system());
        assert!(!ThemeMode::Light.is_system());
        assert!(ThemeMode::Light.is_light());
        assert!(ThemeMode::Dark.is_dark());
    }

    #[test]
    fn the_high_contrast_light_theme_can_win_in_dark_mode() {
        // The third arm does not test useDarkTheme. An app in dark mode with
        // high contrast on, carrying a highContrastTheme but neither
        // highContrastDarkTheme nor darkTheme, gets the light one.
        let app = MaterialApp {
            has_theme: true,
            has_dark_theme: false,
            has_high_contrast_theme: true,
            has_high_contrast_dark_theme: false,
            theme_mode: Some(ThemeMode::Dark),
            ..MaterialApp::new()
        };
        assert!(app.uses_dark_theme(Brightness::Light));
        assert_eq!(
            app.choose_theme(Brightness::Light, true),
            ThemeChoice::HighContrast,
            "a decision tree over (brightness, contrast) would have said Given"
        );
        // Give it a dark theme and the second arm takes it first.
        let with_dark = MaterialApp {
            has_dark_theme: true,
            ..app
        };
        assert_eq!(
            with_dark.choose_theme(Brightness::Light, true),
            ThemeChoice::Dark
        );
    }

    #[test]
    fn and_the_arms_are_tried_in_order() {
        let app = fully_themed();
        // Dark and high contrast: the first arm.
        assert_eq!(
            app.choose_theme(Brightness::Dark, true),
            ThemeChoice::HighContrastDark
        );
        // Dark without contrast: the second.
        assert_eq!(app.choose_theme(Brightness::Dark, false), ThemeChoice::Dark);
        // Light with contrast: the third.
        assert_eq!(
            app.choose_theme(Brightness::Light, true),
            ThemeChoice::HighContrast
        );
        // Light without: the ordinary theme.
        assert_eq!(
            app.choose_theme(Brightness::Light, false),
            ThemeChoice::Given
        );
    }

    #[test]
    fn an_app_with_no_themes_falls_all_the_way_through() {
        let bare = MaterialApp::new();
        for platform in [Brightness::Light, Brightness::Dark] {
            for contrast in [false, true] {
                assert_eq!(
                    bare.choose_theme(platform, contrast),
                    ThemeChoice::Fallback,
                    "{platform:?} {contrast}"
                );
            }
        }
        // Dark mode with no dark theme falls back to the light one, rather
        // than to anything dark.
        let light_only = MaterialApp {
            has_theme: true,
            theme_mode: Some(ThemeMode::Dark),
            ..MaterialApp::new()
        };
        assert!(light_only.uses_dark_theme(Brightness::Light));
        assert_eq!(
            light_only.choose_theme(Brightness::Light, false),
            ThemeChoice::Given
        );
    }

    #[test]
    fn a_null_theme_mode_is_not_the_same_as_system() {
        // `_themeBuilder` reads null as system; `_isDarkTheme` does not read
        // it at all. On a dark platform the two disagree, and the selection
        // controls are told they are on a light theme while the app is dark.
        let app = MaterialApp {
            has_theme: true,
            has_dark_theme: true,
            theme_mode: None,
            ..MaterialApp::new()
        };
        assert!(app.uses_dark_theme(Brightness::Dark));
        assert!(!app.is_dark_theme_flag(Brightness::Dark));
        assert!(!app.dark_flag_agrees_with_the_theme(Brightness::Dark));
        assert_eq!(app.choose_theme(Brightness::Dark, false), ThemeChoice::Dark);
    }

    #[test]
    fn and_they_agree_everywhere_else() {
        // Which is what makes the disagreement above worth pinning rather than
        // being noise: it happens for exactly one value of themeMode, and only
        // on a dark platform.
        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
            for platform in [Brightness::Light, Brightness::Dark] {
                let app = MaterialApp {
                    theme_mode: Some(mode),
                    ..MaterialApp::new()
                };
                assert!(
                    app.dark_flag_agrees_with_the_theme(platform),
                    "{mode:?} {platform:?}"
                );
            }
        }
        let null_mode = MaterialApp {
            theme_mode: None,
            ..MaterialApp::new()
        };
        assert!(
            null_mode.dark_flag_agrees_with_the_theme(Brightness::Light),
            "on a light platform even null agrees, because both say false"
        );
    }

    #[test]
    fn the_constructor_default_is_system_rather_than_null() {
        // Upstream writes `this.themeMode = ThemeMode.system`, so the
        // disagreement above needs someone to pass null on purpose.
        assert_eq!(MaterialApp::new().theme_mode, Some(ThemeMode::System));
        assert_eq!(ThemeMode::default(), ThemeMode::System);
    }
}
