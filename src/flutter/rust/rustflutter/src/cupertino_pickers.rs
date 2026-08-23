//! Ports of `cupertino/date_picker.dart`'s `CupertinoDatePicker` and
//! `CupertinoTimerPicker`, and `cupertino/menu_anchor.dart`'s
//! `CupertinoMenuEntry`, `CupertinoMenuAnchor`, `CupertinoMenuDivider` and
//! `CupertinoMenuItem`.
//!
//! A wheel has no ends, and a menu item's business is mostly with its
//! neighbours.

/// Upstream `CupertinoDatePickerMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CupertinoDatePickerMode {
    Time,
    Date,
    #[default]
    DateAndTime,
    MonthYear,
}

/// Why a date picker's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePickerError {
    NonPositiveItemExtent,
    MinuteIntervalNotAFactorOfSixty,
    InitialMinuteNotOnAStop,
    InitialBeforeMinimum,
    InitialAfterMaximum,
    DayOfWeekOutsideDateMode,
    TimeSeparatorOutsideTimeModes,
    ModeChangedAfterBuild,
}

/// Upstream `CupertinoDatePicker`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoDatePicker {
    pub mode: CupertinoDatePickerMode,
    pub item_extent: f32,
    pub minute_interval: u32,
    pub initial_minute: u32,
    pub show_day_of_week: bool,
    pub show_time_separator: bool,
    /// Minutes since some epoch, standing in for upstream's `DateTime`.
    pub initial: i64,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
}

impl CupertinoDatePicker {
    pub fn new(mode: CupertinoDatePickerMode) -> CupertinoDatePicker {
        CupertinoDatePicker {
            mode,
            item_extent: 32.0,
            minute_interval: 1,
            initial_minute: 0,
            show_day_of_week: false,
            show_time_separator: false,
            initial: 0,
            minimum: None,
            maximum: None,
        }
    }

    /// Upstream's twelve constructor asserts. Three things about them.
    ///
    /// **The minute interval must divide sixty**, not merely be positive:
    /// `assert(minuteInterval > 0 && 60 % minuteInterval == 0, 'minute interval
    /// is not a positive integer factor of 60')`. Because the wheel wraps. An
    /// interval of seven would run 0, 7, ... 56 and then meet 0 again four
    /// minutes later, so the one gap that is not an interval is the one you
    /// cannot see coming. **A wheel has no ends, so its step has to close the
    /// loop.**
    ///
    /// And the initial minute must land *on* a stop:
    /// `assert(initialDateTime.minute % minuteInterval == 0)`. The two together
    /// say you cannot start a wheel between two of its positions, because it has
    /// no in-between to start at.
    ///
    /// **Six of the twelve are mode-guarded, each written `mode != X || <check>`**
    /// -- an implication per mode, inline. Tick 88's `PaginatedDataTable` wrote
    /// the same idea as a nested assert inside an `assert(() { ... }())` closure.
    /// Two encodings of "this rule applies only in this situation", one per file.
    pub fn validate(&self) -> Result<(), DatePickerError> {
        if self.item_extent <= 0.0 {
            return Err(DatePickerError::NonPositiveItemExtent);
        }
        if self.minute_interval == 0 || 60 % self.minute_interval != 0 {
            return Err(DatePickerError::MinuteIntervalNotAFactorOfSixty);
        }
        if self.initial_minute % self.minute_interval != 0 {
            return Err(DatePickerError::InitialMinuteNotOnAStop);
        }
        if self.minimum.is_some_and(|min| self.initial < min) {
            return Err(DatePickerError::InitialBeforeMinimum);
        }
        if self.maximum.is_some_and(|max| self.initial > max) {
            return Err(DatePickerError::InitialAfterMaximum);
        }
        // `assert((mode == date) || !showDayOfWeek, 'showDayOfWeek is only
        // supported in date mode')`.
        if self.show_day_of_week && self.mode != CupertinoDatePickerMode::Date {
            return Err(DatePickerError::DayOfWeekOutsideDateMode);
        }
        // And the same shape for the separator, admitting two modes.
        if self.show_time_separator
            && !matches!(
                self.mode,
                CupertinoDatePickerMode::Time | CupertinoDatePickerMode::DateAndTime
            )
        {
            return Err(DatePickerError::TimeSeparatorOutsideTimeModes);
        }
        Ok(())
    }

    /// Upstream's `didUpdateWidget`:
    /// `assert(oldWidget.mode == widget.mode, "The $runtimeType's mode cannot
    /// change once it's built.")`.
    ///
    /// **A constructor argument that is part of the widget's identity** -- the
    /// second in this sweep, after the stepper's step list in tick 85, and for
    /// the same kind of reason: each mode builds a different set of wheels, and
    /// the scroll controllers behind them are made once.
    pub fn accepts_update(&self, updated: &CupertinoDatePicker) -> Result<(), DatePickerError> {
        if self.mode != updated.mode {
            return Err(DatePickerError::ModeChangedAfterBuild);
        }
        Ok(())
    }

    /// The stops a minute wheel offers, which is why the interval must divide
    /// sixty.
    pub fn minute_stops(&self) -> Vec<u32> {
        (0..60).step_by(self.minute_interval as usize).collect()
    }

    /// Whether every step between consecutive stops, **including the one that
    /// wraps past 60 back to 0**, is the same size.
    pub fn wheel_closes_evenly(&self) -> bool {
        let stops = self.minute_stops();
        stops
            .windows(2)
            .all(|pair| pair[1] - pair[0] == self.minute_interval)
            && 60 - stops.last().copied().unwrap_or(0) == self.minute_interval
    }
}

/// One column of a [`CupertinoTimerPicker`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerPickerUnit {
    Hour,
    Minute,
    Second,
}

/// Upstream `CupertinoTimerPickerMode`: which units the timer shows.
///
/// **The minute is in all three.** Upstream's `initState` says so by leaving
/// it unconditional and guarding only the other two:
///
/// ```dart
/// selectedMinute = widget.initialTimerDuration.inMinutes % 60;
/// if (widget.mode != CupertinoTimerPickerMode.ms) { selectedHour = ...; }
/// if (widget.mode != CupertinoTimerPickerMode.hm) { selectedSecond = ...; }
/// ```
///
/// So the three modes are not three lists of units; they are **which of the
/// hour and the second keep the minute company.**
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CupertinoTimerPickerMode {
    /// Hours and minutes: "16 hours | 14 min".
    Hm,
    /// Minutes and seconds: "14 min | 43 sec".
    Ms,
    /// All three. Upstream's default.
    #[default]
    Hms,
}

impl CupertinoTimerPickerMode {
    pub const ALL: [CupertinoTimerPickerMode; 3] = [
        CupertinoTimerPickerMode::Hm,
        CupertinoTimerPickerMode::Ms,
        CupertinoTimerPickerMode::Hms,
    ];

    /// The columns, left to right. Everything else here is read off this.
    pub fn columns(self) -> &'static [TimerPickerUnit] {
        match self {
            CupertinoTimerPickerMode::Hm => &[TimerPickerUnit::Hour, TimerPickerUnit::Minute],
            CupertinoTimerPickerMode::Ms => &[TimerPickerUnit::Minute, TimerPickerUnit::Second],
            CupertinoTimerPickerMode::Hms => &[
                TimerPickerUnit::Hour,
                TimerPickerUnit::Minute,
                TimerPickerUnit::Second,
            ],
        }
    }

    /// Upstream divides the width by `mode == hms ? 3 : 2`.
    pub fn column_count(self) -> usize {
        self.columns().len()
    }

    pub fn shows(self, unit: TimerPickerUnit) -> bool {
        self.columns().contains(&unit)
    }

    /// Where a unit sits, counting from the left.
    ///
    /// Upstream writes this out per unit at each call site -- the minute's
    /// off-axis fraction is `mode == ms ? 0 : 1`, the second's is
    /// `mode == ms ? 1 : 2`. **Both are just this index**, and deriving it
    /// keeps the two from disagreeing when a mode is added.
    pub fn index_of(self, unit: TimerPickerUnit) -> Option<usize> {
        self.columns().iter().position(|held| *held == unit)
    }
}

/// Upstream `CupertinoTimerPicker`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoTimerPicker {
    /// Upstream's `initialTimerDuration`, in seconds.
    pub initial_timer_duration_secs: i64,
    pub minute_interval: u32,
    pub second_interval: u32,
    pub mode: CupertinoTimerPickerMode,
}

impl CupertinoTimerPicker {
    /// A day, in seconds. Upstream: `assert(initialTimerDuration < const
    /// Duration(days: 1))`.
    pub const ONE_DAY_SECS: i64 = 24 * 60 * 60;

    pub fn new(initial_timer_duration_secs: i64) -> CupertinoTimerPicker {
        CupertinoTimerPicker {
            initial_timer_duration_secs,
            minute_interval: 1,
            second_interval: 1,
            mode: CupertinoTimerPickerMode::Hms,
        }
    }

    /// Upstream's two duration asserts: at least zero, and **strictly less than
    /// a day**. The picker shows hours, minutes and seconds, so twenty-four
    /// hours has nowhere to be displayed -- it would read as zero.
    pub fn validate(&self) -> bool {
        self.initial_timer_duration_secs >= 0
            && self.initial_timer_duration_secs < CupertinoTimerPicker::ONE_DAY_SECS
    }
}

/// Upstream `CupertinoMenuEntry`, an `abstract interface class` with exactly two
/// members -- and **both of them are about the item's neighbours rather than
/// itself.**
pub trait CupertinoMenuEntry {
    /// Upstream: *"If `hasLeading` returns true, **siblings** of this menu item
    /// that are missing a leading widget will have leading space added to align
    /// the leading edges of all menu items."*
    ///
    /// **One item with an icon indents every other item.** The alignment belongs
    /// to the group, and the group works it out by asking each member -- so the
    /// answer to "does this item have a leading widget" is read by everybody
    /// except that item.
    fn has_leading(&self) -> bool;

    /// Upstream: *"When true, a divider will **not** be drawn above or below this
    /// menu item. Otherwise, adjacent menu items will be separated by a
    /// divider."*
    ///
    /// So the flag does not say "I am a line", it says **"do not put lines next
    /// to me"** -- which is what stops a divider you asked for from arriving
    /// between two the menu drew itself.
    fn is_divider(&self) -> bool;
}

/// What colour a menu item's label is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItemLabel {
    /// `CupertinoColors.systemGrey`, for an item that cannot be pressed.
    Disabled,
    /// `CupertinoColors.systemRed`, for one that will destroy something.
    Destructive,
    /// The ordinary label colour.
    Ordinary,
}

/// Upstream `CupertinoMenuItem`.
///
/// # Disabled beats destructive
///
/// `_resolveDefaultTextStyle` asks in this order:
///
/// ```text
/// if (onPressed == null)      color = systemGrey;
/// else if (isDestructiveAction) color = systemRed;
/// else                          color = _kDefaultTextColor;
/// ```
///
/// So a disabled destructive item is **grey and not red**. The warning colour
/// is withdrawn along with the ability to act: there is nothing to warn about
/// in a button that cannot be pressed, and a red one that does nothing would
/// be alarming for no reason.
///
/// # Pressing closes the menu before it runs the callback
///
/// `_handleSelect` closes first and calls `onPressed` after, so the callback
/// runs with the menu already going. A callback that pushes a route is not
/// then fighting the menu's own dismissal for the same frame.
///
/// And `requestCloseOnActivate: false` stops the closing, not the callback --
/// the two are separate steps and only the first is optional.
///
/// # The subtitle's colour is a blend, and an approximation of one
///
/// Upstream sets `foreground: Paint()..blendMode = isDark ? BlendMode.plus :
/// BlendMode.hardLight`, with its own note that iOS uses `linearDodge` in the
/// dark and `plusDarker` in the light, and that these are approximations of
/// those. The whole style is marked "approximated from the iOS and iPadOS 18.5
/// simulators".
///
/// Recorded because a port that reproduced these two modes exactly would be
/// reproducing an approximation, not the platform -- worth knowing before
/// anybody tunes them against a screenshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoMenuItem {
    pub leading: bool,
    /// Upstream's `onPressed == null`, which is what "disabled" means here.
    pub enabled: bool,
    /// Upstream's `isDestructiveAction`, **false** by default.
    pub is_destructive_action: bool,
    /// Upstream's `requestCloseOnActivate`, **true** by default.
    pub request_close_on_activate: bool,
    /// Upstream's `requestFocusOnHover`, **true** by default -- the same as
    /// Material's `MenuItemButton.requestFocusOnHover`.
    ///
    /// I wrote the opposite here first, on the strength of this port having it
    /// false on the Material side. Checking upstream showed the port was
    /// wrong, not the platforms: both default to true, and the Material one
    /// has been corrected.
    pub request_focus_on_hover: bool,
}

impl Default for CupertinoMenuItem {
    fn default() -> CupertinoMenuItem {
        CupertinoMenuItem::new()
    }
}

impl CupertinoMenuItem {
    pub fn new() -> CupertinoMenuItem {
        CupertinoMenuItem {
            leading: false,
            enabled: true,
            is_destructive_action: false,
            request_close_on_activate: true,
            request_focus_on_hover: true,
        }
    }

    pub fn destructive() -> CupertinoMenuItem {
        CupertinoMenuItem {
            is_destructive_action: true,
            ..CupertinoMenuItem::new()
        }
    }

    pub fn disabled(mut self) -> CupertinoMenuItem {
        self.enabled = false;
        self
    }

    /// Upstream's `_resolveDefaultTextStyle` colour ladder -- see the type's
    /// docs for why disabled is asked first.
    pub fn label(&self) -> MenuItemLabel {
        if !self.enabled {
            return MenuItemLabel::Disabled;
        }
        if self.is_destructive_action {
            return MenuItemLabel::Destructive;
        }
        MenuItemLabel::Ordinary
    }

    /// The colour that label resolves to.
    pub fn label_color(&self) -> crate::cupertino::CupertinoDynamicColor {
        match self.label() {
            MenuItemLabel::Disabled => crate::cupertino::CupertinoColors::SYSTEM_GREY,
            MenuItemLabel::Destructive => crate::cupertino::CupertinoColors::SYSTEM_RED,
            MenuItemLabel::Ordinary => crate::cupertino::CupertinoColors::LABEL,
        }
    }

    /// Upstream's `_handleSelect`, as the two things it does and their order.
    ///
    /// Closing is conditional and calling is not, so an item that keeps the
    /// menu open still does its work.
    pub fn activation(&self) -> (bool, bool) {
        (self.request_close_on_activate, self.enabled)
    }

    /// Upstream's subtitle blend: `plus` in the dark, `hardLight` in the
    /// light. Both are approximations -- see the type's docs.
    pub fn subtitle_blend(is_dark: bool) -> crate::painting::BlendMode {
        if is_dark {
            crate::painting::BlendMode::Plus
        } else {
            crate::painting::BlendMode::HardLight
        }
    }
}

impl CupertinoMenuEntry for CupertinoMenuItem {
    fn has_leading(&self) -> bool {
        self.leading
    }

    fn is_divider(&self) -> bool {
        false
    }
}

/// Upstream `CupertinoMenuDivider`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoMenuDivider;

impl CupertinoMenuEntry for CupertinoMenuDivider {
    fn has_leading(&self) -> bool {
        false
    }

    fn is_divider(&self) -> bool {
        true
    }
}

/// Upstream `CupertinoMenuAnchor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoMenuAnchor {
    pub enable_swipe: bool,
    pub enable_long_press_to_open: bool,
}

impl CupertinoMenuAnchor {
    pub fn new() -> CupertinoMenuAnchor {
        CupertinoMenuAnchor {
            enable_swipe: true,
            enable_long_press_to_open: true,
        }
    }

    /// Upstream's one constructor assert:
    /// `assert(enableSwipe || !enableLongPressToOpen, 'enableLongPressToOpen
    /// cannot be true if enableSwipe is false')`.
    ///
    /// Another implication, and a physical one. A long press that opens the menu
    /// leaves your finger already down on it; the gesture continues straight
    /// into a swipe to pick something. **Opening by long press with swiping
    /// turned off would strand the finger that opened it.**
    pub fn is_valid(&self) -> bool {
        self.enable_swipe || !self.enable_long_press_to_open
    }

    /// Where the separators fall between a run of entries, given each one's
    /// answer to `isDivider`. Returns the gaps that get a drawn rule.
    pub fn drawn_dividers(entries: &[&dyn CupertinoMenuEntry]) -> Vec<usize> {
        (0..entries.len().saturating_sub(1))
            .filter(|&gap| !entries[gap].is_divider() && !entries[gap + 1].is_divider())
            .collect()
    }

    /// Whether the menu indents its items, which one leading widget anywhere is
    /// enough to decide.
    pub fn aligns_leading_edges(entries: &[&dyn CupertinoMenuEntry]) -> bool {
        entries.iter().any(|entry| entry.has_leading())
    }
}

impl Default for CupertinoMenuAnchor {
    fn default() -> Self {
        CupertinoMenuAnchor::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time_picker() -> CupertinoDatePicker {
        CupertinoDatePicker::new(CupertinoDatePickerMode::Time)
    }

    // -- A wheel has no ends ---------------------------------------------------------

    #[test]
    fn a_minute_interval_has_to_divide_sixty_not_merely_be_positive() {
        let mut picker = time_picker();
        for good in [1, 2, 3, 5, 10, 15, 20, 30] {
            picker.minute_interval = good;
            assert_eq!(picker.validate(), Ok(()), "interval {good}");
        }
        for bad in [7, 8, 9, 11, 45] {
            picker.minute_interval = bad;
            assert_eq!(
                picker.validate(),
                Err(DatePickerError::MinuteIntervalNotAFactorOfSixty),
                "interval {bad}"
            );
        }
    }

    #[test]
    fn because_the_one_gap_that_is_not_an_interval_is_the_one_you_cannot_see() {
        // Seven would run 0, 7, ... 56 and then meet 0 four minutes later.
        let mut picker = time_picker();
        picker.minute_interval = 7;
        assert_eq!(picker.minute_stops().last(), Some(&56));
        assert!(!picker.wheel_closes_evenly());

        picker.minute_interval = 15;
        assert_eq!(picker.minute_stops(), vec![0, 15, 30, 45]);
        assert!(picker.wheel_closes_evenly());
    }

    #[test]
    fn and_you_cannot_start_between_two_positions_of_a_wheel_that_has_none() {
        let mut picker = time_picker();
        picker.minute_interval = 15;
        picker.initial_minute = 30;
        assert_eq!(picker.validate(), Ok(()));

        picker.initial_minute = 31;
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::InitialMinuteNotOnAStop)
        );
    }

    // -- Six implications, one per mode ------------------------------------------------

    #[test]
    fn the_day_of_week_column_belongs_to_one_mode_only() {
        let mut picker = CupertinoDatePicker::new(CupertinoDatePickerMode::Date);
        picker.show_day_of_week = true;
        assert_eq!(picker.validate(), Ok(()));

        for other in [
            CupertinoDatePickerMode::Time,
            CupertinoDatePickerMode::DateAndTime,
            CupertinoDatePickerMode::MonthYear,
        ] {
            picker.mode = other;
            assert_eq!(
                picker.validate(),
                Err(DatePickerError::DayOfWeekOutsideDateMode),
                "{other:?}"
            );
        }
    }

    #[test]
    fn and_the_time_separator_to_the_two_that_show_a_time() {
        let mut picker = time_picker();
        picker.show_time_separator = true;
        assert_eq!(picker.validate(), Ok(()));

        picker.mode = CupertinoDatePickerMode::DateAndTime;
        assert_eq!(picker.validate(), Ok(()));

        picker.mode = CupertinoDatePickerMode::Date;
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::TimeSeparatorOutsideTimeModes)
        );
    }

    #[test]
    fn a_mode_cannot_change_once_the_picker_is_built() {
        // The second widget in this sweep with an argument that is part of its
        // identity, after the stepper's step list.
        let picker = time_picker();
        assert_eq!(picker.accepts_update(&time_picker()), Ok(()));
        assert_eq!(
            picker.accepts_update(&CupertinoDatePicker::new(CupertinoDatePickerMode::Date)),
            Err(DatePickerError::ModeChangedAfterBuild)
        );
    }

    #[test]
    fn the_initial_date_has_to_be_inside_the_range_it_was_given() {
        let mut picker = time_picker();
        picker.initial = 100;
        picker.minimum = Some(200);
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::InitialBeforeMinimum)
        );

        picker.minimum = Some(50);
        picker.maximum = Some(80);
        assert_eq!(picker.validate(), Err(DatePickerError::InitialAfterMaximum));

        picker.maximum = Some(150);
        assert_eq!(picker.validate(), Ok(()));
    }

    #[test]
    fn a_row_needs_a_height() {
        let mut picker = time_picker();
        picker.item_extent = 0.0;
        assert_eq!(
            picker.validate(),
            Err(DatePickerError::NonPositiveItemExtent)
        );
    }

    // -- A timer is shorter than a day ---------------------------------------------------

    #[test]
    fn a_timer_may_run_to_one_second_short_of_a_day_and_no_further() {
        assert!(CupertinoTimerPicker::new(0).validate());
        assert!(CupertinoTimerPicker::new(CupertinoTimerPicker::ONE_DAY_SECS - 1).validate());
        assert!(
            !CupertinoTimerPicker::new(CupertinoTimerPicker::ONE_DAY_SECS).validate(),
            "twenty-four hours has no column to be displayed in"
        );
        assert!(!CupertinoTimerPicker::new(-1).validate());
    }

    // -- An interface entirely about the neighbours ---------------------------------------

    #[test]
    fn one_item_with_an_icon_indents_all_the_others() {
        let plain = CupertinoMenuItem::new();
        let with_icon = CupertinoMenuItem {
            leading: true,
            ..CupertinoMenuItem::new()
        };

        let all_plain: [&dyn CupertinoMenuEntry; 2] = [&plain, &plain];
        assert!(!CupertinoMenuAnchor::aligns_leading_edges(&all_plain));

        let one_icon: [&dyn CupertinoMenuEntry; 3] = [&plain, &with_icon, &plain];
        assert!(
            CupertinoMenuAnchor::aligns_leading_edges(&one_icon),
            "and the two plain items move for it"
        );
    }

    #[test]
    fn a_divider_stops_the_menu_drawing_its_own_beside_it() {
        // isDivider does not say "I am a line", it says "no lines next to me".
        let item = CupertinoMenuItem::default();
        let rule = CupertinoMenuDivider;

        let plain: [&dyn CupertinoMenuEntry; 3] = [&item, &item, &item];
        assert_eq!(
            CupertinoMenuAnchor::drawn_dividers(&plain),
            vec![0, 1],
            "every gap between plain items gets one"
        );

        let with_rule: [&dyn CupertinoMenuEntry; 3] = [&item, &rule, &item];
        assert_eq!(
            CupertinoMenuAnchor::drawn_dividers(&with_rule),
            Vec::<usize>::new(),
            "and an explicit rule suppresses both gaps it touches"
        );
    }

    #[test]
    fn so_two_rules_never_end_up_side_by_side() {
        let item = CupertinoMenuItem::default();
        let rule = CupertinoMenuDivider;
        let entries: [&dyn CupertinoMenuEntry; 4] = [&item, &rule, &rule, &item];
        assert_eq!(
            CupertinoMenuAnchor::drawn_dividers(&entries),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn a_divider_never_claims_a_leading_widget() {
        assert!(!CupertinoMenuDivider.has_leading());
        assert!(CupertinoMenuDivider.is_divider());
        assert!(!CupertinoMenuItem::default().is_divider());
    }

    // -- The finger that opened it --------------------------------------------------------

    #[test]
    fn opening_by_long_press_needs_swiping_to_stay_on() {
        let mut anchor = CupertinoMenuAnchor::new();
        assert!(anchor.is_valid());

        anchor.enable_swipe = false;
        assert!(
            !anchor.is_valid(),
            "the long press would leave a finger down with nowhere to go"
        );

        anchor.enable_long_press_to_open = false;
        assert!(anchor.is_valid(), "and turning both off is fine");
    }
}

#[cfg(test)]
mod cupertino_menu_item_tests {
    use super::*;
    use crate::cupertino::CupertinoColors;
    use crate::painting::BlendMode;

    #[test]
    fn a_disabled_destructive_item_is_grey_and_not_red() {
        // The warning colour is withdrawn along with the ability to act:
        // there is nothing to warn about in a button that cannot be pressed.
        let off = CupertinoMenuItem::destructive().disabled();
        assert_eq!(off.label(), MenuItemLabel::Disabled);
        assert_eq!(off.label_color(), CupertinoColors::SYSTEM_GREY);

        // And it *is* red while it can be pressed, or the test above would
        // only show that nothing is ever red.
        assert_eq!(
            CupertinoMenuItem::destructive().label(),
            MenuItemLabel::Destructive
        );
        assert_eq!(
            CupertinoMenuItem::destructive().label_color(),
            CupertinoColors::SYSTEM_RED
        );
    }

    #[test]
    fn an_ordinary_disabled_item_is_the_same_grey() {
        // Disabled is one answer, not two: the destructive flag stops
        // mattering entirely rather than tinting the grey.
        assert_eq!(
            CupertinoMenuItem::new().disabled().label_color(),
            CupertinoMenuItem::destructive().disabled().label_color()
        );
    }

    #[test]
    fn and_an_enabled_ordinary_item_is_neither() {
        let plain = CupertinoMenuItem::new();
        assert_eq!(plain.label(), MenuItemLabel::Ordinary);
        assert_ne!(plain.label_color(), CupertinoColors::SYSTEM_GREY);
        assert_ne!(plain.label_color(), CupertinoColors::SYSTEM_RED);
    }

    #[test]
    fn closing_is_optional_and_calling_is_not() {
        // `_handleSelect` closes first and calls after, and only the closing
        // is behind a flag -- an item that keeps the menu open still works.
        let mut keeps_open = CupertinoMenuItem::new();
        keeps_open.request_close_on_activate = false;
        assert_eq!(keeps_open.activation(), (false, true));
        assert_eq!(CupertinoMenuItem::new().activation(), (true, true));
    }

    #[test]
    fn the_three_defaults_are_upstreams() {
        let item = CupertinoMenuItem::new();
        assert!(item.enabled);
        assert!(!item.is_destructive_action, "destructive is opt-in");
        assert!(item.request_close_on_activate, "pressing closes the menu");
        assert!(item.request_focus_on_hover, "the pointer carries the focus");
    }

    #[test]
    fn and_the_material_item_agrees_about_hovering() {
        // Both platforms default `requestFocusOnHover` to true. This port had
        // the Material one false, which is the bug this tick found: moving the
        // mouse and then pressing Enter would have acted on whatever the
        // keyboard had left behind.
        assert!(crate::menu_anchor::MenuItemButton::new().request_focus_on_hover);
        assert_eq!(
            crate::menu_anchor::MenuItemButton::new().request_focus_on_hover,
            CupertinoMenuItem::new().request_focus_on_hover
        );
    }

    #[test]
    fn the_subtitle_blends_differently_in_the_dark() {
        // Both are upstream's own approximations of iOS's `linearDodge` and
        // `plusDarker`, which is why they are worth pinning as *these two*
        // rather than tuned against a screenshot.
        assert_eq!(CupertinoMenuItem::subtitle_blend(true), BlendMode::Plus);
        assert_eq!(
            CupertinoMenuItem::subtitle_blend(false),
            BlendMode::HardLight
        );
        assert_ne!(
            CupertinoMenuItem::subtitle_blend(true),
            CupertinoMenuItem::subtitle_blend(false)
        );
    }

    #[test]
    fn a_menu_item_is_not_a_divider_whatever_else_it_is() {
        for item in [
            CupertinoMenuItem::new(),
            CupertinoMenuItem::destructive(),
            CupertinoMenuItem::new().disabled(),
        ] {
            assert!(!item.is_divider());
        }
    }
}

#[cfg(test)]
mod timer_picker_mode_tests {
    use super::{CupertinoTimerPicker, CupertinoTimerPickerMode, TimerPickerUnit};

    #[test]
    fn the_minute_is_in_every_mode() {
        // Upstream sets selectedMinute unconditionally and guards only the
        // other two, so the modes are which of the hour and the second keep
        // the minute company.
        for mode in CupertinoTimerPickerMode::ALL {
            assert!(mode.shows(TimerPickerUnit::Minute), "{mode:?}");
        }
        assert!(!CupertinoTimerPickerMode::Ms.shows(TimerPickerUnit::Hour));
        assert!(!CupertinoTimerPickerMode::Hm.shows(TimerPickerUnit::Second));
        assert!(CupertinoTimerPickerMode::Hms.shows(TimerPickerUnit::Hour));
        assert!(CupertinoTimerPickerMode::Hms.shows(TimerPickerUnit::Second));
    }

    #[test]
    fn and_the_minute_sits_in_a_different_column_depending_on_the_mode() {
        // Upstream computes this inline as `mode == ms ? 0 : 1` for the
        // minute's off-axis fraction, and `mode == ms ? 1 : 2` for the
        // second's. Both are the column index.
        for mode in CupertinoTimerPickerMode::ALL {
            let expected = if mode == CupertinoTimerPickerMode::Ms {
                0
            } else {
                1
            };
            assert_eq!(
                mode.index_of(TimerPickerUnit::Minute),
                Some(expected),
                "{mode:?}"
            );
        }
        assert_eq!(
            CupertinoTimerPickerMode::Ms.index_of(TimerPickerUnit::Second),
            Some(1)
        );
        assert_eq!(
            CupertinoTimerPickerMode::Hms.index_of(TimerPickerUnit::Second),
            Some(2)
        );
    }

    #[test]
    fn and_a_unit_that_is_not_shown_has_no_column() {
        assert_eq!(
            CupertinoTimerPickerMode::Ms.index_of(TimerPickerUnit::Hour),
            None
        );
        assert_eq!(
            CupertinoTimerPickerMode::Hm.index_of(TimerPickerUnit::Second),
            None
        );
        // Which agrees with `shows`, since both read the same list.
        for mode in CupertinoTimerPickerMode::ALL {
            for unit in [
                TimerPickerUnit::Hour,
                TimerPickerUnit::Minute,
                TimerPickerUnit::Second,
            ] {
                assert_eq!(
                    mode.index_of(unit).is_some(),
                    mode.shows(unit),
                    "{mode:?} {unit:?}"
                );
            }
        }
    }

    #[test]
    fn only_the_full_mode_has_three_columns() {
        // Upstream divides the width by `mode == hms ? 3 : 2`.
        assert_eq!(CupertinoTimerPickerMode::Hms.column_count(), 3);
        assert_eq!(CupertinoTimerPickerMode::Hm.column_count(), 2);
        assert_eq!(CupertinoTimerPickerMode::Ms.column_count(), 2);
    }

    #[test]
    fn the_units_never_change_places() {
        // Whatever the mode leaves out, what remains stays in hour, minute,
        // second order -- a timer reading "43 sec | 14 min" would be nonsense,
        // and nothing else here would catch it.
        for mode in CupertinoTimerPickerMode::ALL {
            let order: Vec<usize> = mode
                .columns()
                .iter()
                .map(|unit| match unit {
                    TimerPickerUnit::Hour => 0,
                    TimerPickerUnit::Minute => 1,
                    TimerPickerUnit::Second => 2,
                })
                .collect();
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(order, sorted, "{mode:?}");
        }
    }

    #[test]
    fn a_timer_picker_shows_everything_unless_told_otherwise() {
        assert_eq!(
            CupertinoTimerPicker::new(0).mode,
            CupertinoTimerPickerMode::Hms
        );
        assert_eq!(
            CupertinoTimerPickerMode::default(),
            CupertinoTimerPickerMode::Hms
        );
    }
}
