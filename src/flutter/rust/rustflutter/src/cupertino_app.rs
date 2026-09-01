//! Ports of `cupertino/app.dart`'s `CupertinoApp` and `CupertinoScrollBehavior`,
//! and `cupertino/localizations.dart`'s `CupertinoLocalizations` and
//! `DefaultCupertinoLocalizations`.
//!
//! Tick 89 ported the Material four. These are their opposite numbers, and
//! reading them side by side is most of what there is to say.

use crate::material_app::ScrollPlatform;

/// Upstream `ScrollDecelerationRate`, declared with the thing it describes in
/// [`crate::scroll_physics`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::scroll_physics::ScrollDecelerationRate;

/// Upstream `MultitouchDragStrategy`, declared with the thing it describes in
/// [`crate::scroll_plumbing`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::scroll_plumbing::MultitouchDragStrategy;

/// Upstream `CupertinoScrollBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoScrollBehavior;

impl CupertinoScrollBehavior {
    /// Upstream `buildScrollbar`, and the interesting part is what is *missing*
    /// beside its Material counterpart.
    ///
    /// [`crate::material_app::MaterialScrollBehavior::builds_scrollbar`] opens
    /// by switching on the axis and returning the child for anything
    /// horizontal, before the platform is consulted at all. **This one has no
    /// axis check.** Neither does the base `ScrollBehavior`. So a horizontal
    /// scrollable on macOS gets a Cupertino scrollbar where the same list in a
    /// Material app would get none.
    ///
    /// All three carry the same note -- *"When modifying this function, consider
    /// modifying the implementation in the base class as well"* -- and **the
    /// three have already drifted apart on exactly this point.** A comment
    /// asking three copies to stay in step is doing the work a shared helper
    /// would have done, and it has not been enough.
    ///
    /// The platform half is identical to both of them: the three desktops get a
    /// bar, the three touch platforms do not, and the
    /// `assert(details.controller != null)` sits inside the desktop arm alone.
    pub fn builds_scrollbar(platform: ScrollPlatform) -> bool {
        platform.is_desktop()
    }

    /// Upstream `buildOverscrollIndicator`, whose entire body is a comment and a
    /// return:
    ///
    /// ```dart
    /// // No overscroll indicator.
    /// return child;
    /// ```
    ///
    /// Tick 89 read Material's Android-only indicator and guessed the rule was
    /// "the platforms that get one are those whose physics do not already show
    /// you the edge". This is that guess confirmed and widened: a Cupertino app
    /// installs bouncing physics on **every** platform, so **nowhere needs one**.
    /// The rule was never about iOS; it was about the bounce.
    pub fn overscroll_indicator(_platform: ScrollPlatform) -> bool {
        false
    }

    /// Upstream `getScrollPhysics`: bouncing everywhere, and macOS alone gets
    /// `ScrollDecelerationRate.fast`.
    ///
    /// Which is the trackpad. A flick with a thumb is meant to coast; a flick
    /// with two fingers on a glass pad is a smaller, more repeatable gesture,
    /// and coasting as far would overshoot everything.
    pub fn deceleration_rate(platform: ScrollPlatform) -> ScrollDecelerationRate {
        match platform {
            ScrollPlatform::MacOS => ScrollDecelerationRate::Fast,
            _ => ScrollDecelerationRate::Normal,
        }
    }

    /// Whether the physics bounce. They always do.
    pub fn bounces(_platform: ScrollPlatform) -> bool {
        true
    }

    /// Upstream `getMultitouchDragStrategy`, which returns
    /// `averageBoundaryPointers` unconditionally: with several fingers down the
    /// scroll follows the average of the outermost two rather than whichever
    /// pointer moved last.
    pub fn multitouch_drag_strategy() -> MultitouchDragStrategy {
        MultitouchDragStrategy::AverageBoundaryPointers
    }
}

/// Upstream `CupertinoApp`.
///
/// Its `title` and `onGenerateTitle` are not here because they are not its:
/// both are forwarded to the `WidgetsApp` it builds, unchanged, on both the
/// `.router` and the navigator path. What decides between them lives at
/// [`crate::widgets_app::WidgetsApp::app_title`], and `MaterialApp` forwards
/// the identical pair -- there is no Cupertino-specific naming behaviour to
/// port, which is itself the answer to "where did the title go".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoApp {
    pub has_home: bool,
    /// Upstream's five router parameters, which `CupertinoApp.router` takes and
    /// hands straight to `WidgetsApp.router`.
    ///
    /// All five, not the two this port carried before: an application that
    /// passes a `routeInformationProvider` and no parser is refused by
    /// `WidgetsApp.router`, and an app that modelled only the delegate and the
    /// config had nowhere to put the parameters that refusal is about.
    pub router: crate::widgets_app::RouterConfiguration,
    pub debug_show_checked_mode_banner: bool,
}

impl CupertinoApp {
    pub fn new() -> CupertinoApp {
        CupertinoApp {
            has_home: true,
            router: crate::widgets_app::RouterConfiguration::default(),
            debug_show_checked_mode_banner: true,
        }
    }

    /// Upstream's only constructor assert, on the `.router` form:
    /// `assert(routerDelegate != null || routerConfig != null)` -- the same "at
    /// least one" as `MaterialApp.router`.
    ///
    /// It is **weaker** than the three `WidgetsApp.router` asserts, and that
    /// is upstream's arrangement rather than an omission here: this
    /// constructor checks the one thing it can check locally and forwards
    /// everything to `WidgetsApp.router`, which checks the rest. See
    /// [`crate::widgets_app::RouterConfiguration::validate`].
    pub fn router_is_configured(&self) -> bool {
        self.router.is_configured()
    }

    /// `MaterialApp` carries a `debugShowMaterialGrid` and wraps a `GridPaper`
    /// inside an assert block. **`CupertinoApp` has no counterpart** -- no
    /// Cupertino grid overlay exists, because the iOS design language is not
    /// specified against an 8dp grid the way Material is.
    pub fn has_a_design_grid_overlay() -> bool {
        false
    }

    /// The status bar style that goes with a brightness -- and it is the
    /// **opposite** one.
    ///
    /// Which brightness is **not** here on purpose. Upstream's build reads
    /// `effectiveThemeData.brightness ?? MediaQuery.platformBrightnessOf`, and
    /// that is `CupertinoTheme.brightnessOf` -- already ported, as
    /// [`crate::cupertino_theme::CupertinoThemeData::brightness_of`]. A second
    /// copy under a second name is two rules that can drift, and the first
    /// draft of this tick wrote exactly that before noticing.
    ///
    /// Upstream: `brightness == Brightness.dark ? SystemUiOverlayStyle.light :
    /// SystemUiOverlayStyle.dark`. The name says what the *icons* are, not
    /// what the interface behind them is: a dark app needs light icons over
    /// it. Reading the pair as "dark app, dark style" is the mistake this
    /// exists to make hard.
    pub fn overlay_style(
        brightness: crate::prelude::Brightness,
    ) -> crate::services::system::SystemUiOverlayStyle {
        match brightness {
            crate::prelude::Brightness::Dark => {
                crate::services::system::SystemUiOverlayStyle::LIGHT
            }
            crate::prelude::Brightness::Light => {
                crate::services::system::SystemUiOverlayStyle::DARK
            }
        }
    }

    /// Upstream's `_buildWidgetApp`: `widget.color ?? effectiveThemeData
    /// .primaryColor`, resolved.
    ///
    /// The colour an app hands the operating system for its task switcher
    /// entry, which is why it falls back to the theme rather than to a
    /// constant: an application that themed itself and said nothing about
    /// `color` still wants to be recognised by its own colour.
    pub fn app_color(
        given: Option<u32>,
        theme: &crate::cupertino_theme::CupertinoThemeData,
    ) -> u32 {
        given.unwrap_or_else(|| theme.primary_color())
    }

    /// Upstream's `DefaultSelectionStyle`: the cursor is the primary colour
    /// and the selection is the same colour at a fifth.
    ///
    /// One colour and one number, so that a theme changing its primary colour
    /// moves both -- upstream writes them as two lines of the same widget for
    /// the same reason.
    pub fn selection_style(
        theme: &crate::cupertino_theme::CupertinoThemeData,
    ) -> (crate::prelude::Color, crate::prelude::Color) {
        let cursor = crate::prelude::Color(theme.primary_color());
        (
            crate::elevation_overlay::with_opacity(cursor, CupertinoApp::SELECTION_OPACITY),
            cursor,
        )
    }

    /// Upstream's `withOpacity(0.2)` on the selection colour.
    pub const SELECTION_OPACITY: f32 = 0.2;

    /// Upstream's `_localizationsDelegates`, and the order is the rule:
    ///
    /// ```dart
    /// return <LocalizationsDelegate<dynamic>>[
    ///   ...?widget.localizationsDelegates,
    ///   DefaultCupertinoLocalizations.delegate,
    /// ];
    /// ```
    ///
    /// The application's own come **first**, and upstream's comment says why:
    /// *"Only the first delegate of a particular LocalizationsDelegate.type is
    /// loaded so the localizationsDelegate parameter can be used to override
    /// _CupertinoLocalizationsDelegate."* Appending the framework's rather than
    /// prepending it is the whole of an application's ability to replace the
    /// framework's strings -- see [`crate::localizations`], which is where the
    /// first-per-type rule lives.
    pub fn localizations_delegates(
        app: Vec<std::rc::Rc<dyn crate::localizations::LocalizationsDelegate>>,
    ) -> Vec<std::rc::Rc<dyn crate::localizations::LocalizationsDelegate>> {
        let mut delegates = app;
        delegates.push(std::rc::Rc::new(DefaultCupertinoLocalizationsDelegate));
        delegates
    }
}

impl Default for CupertinoApp {
    fn default() -> Self {
        CupertinoApp::new()
    }
}

/// Upstream `CupertinoLocalizations`: the strings the Cupertino widgets say.
///
/// # A bundle rather than a table
///
/// This was an empty struct with an `of` on it, and the strings sat as
/// constants on [`DefaultCupertinoLocalizations`] with every widget reading
/// them from there. That works and it is not what the class is *for*: reading
/// a constant off the implementation means an application can never put its
/// own bundle in front of it, which is the whole point of having an interface.
///
/// [`crate::material_app::MaterialLocalizations`] went through exactly this a
/// while ago, and its own docs say why. Two localization layers in one crate
/// modelled two different ways was the thing to fix; this is the second half.
///
/// # Not shared with the Material bundle
///
/// Four of these names also exist on `MaterialLocalizations` --
/// `alert_dialog_label`, `cancel_button_label`, `menu_dismiss_label` and
/// `modal_barrier_dismiss_label` -- and four out of forty-odd is not a shared
/// interface, it is a coincidence of English. Upstream declares both classes
/// independently, and a locale is free to word "Cancel" differently in a
/// Cupertino alert than in a Material dialog. Sharing them here would make
/// that impossible to express.
///
/// The constants stay, and are what the default implementation returns. They
/// are the values; this is the interface.
pub trait CupertinoLocalizations {
    /// Upstream's `datePickerYear`.
    fn date_picker_year(&self, year_index: i32) -> String;

    /// Upstream's `datePickerMonth`.
    fn date_picker_month(&self, month_index: usize) -> &str;

    /// Upstream's `datePickerStandaloneMonth`.
    fn date_picker_standalone_month(&self, month_index: usize) -> &str;

    /// Upstream's `datePickerDayOfMonth`, whose `weekDay` is optional -- a
    /// picker showing the weekday alongside the day passes it and one that
    /// does not, does not.
    fn date_picker_day_of_month(&self, day_index: u32, week_day: Option<u32>) -> String;

    /// Upstream's `datePickerMediumDate`.
    fn date_picker_medium_date(&self, week_day: u32, month: usize, day: u32) -> String;

    /// Upstream's `datePickerHour`.
    fn date_picker_hour(&self, hour: u32) -> String;

    /// Upstream's `datePickerHourSemanticsLabel`.
    fn date_picker_hour_semantics_label(&self, hour: u32) -> String;

    /// Upstream's `datePickerMinute`.
    fn date_picker_minute(&self, minute: u32) -> String;

    /// Upstream's `datePickerMinuteSemanticsLabel`.
    fn date_picker_minute_semantics_label(&self, minute: u32) -> String;

    /// Upstream's `datePickerDateOrder`.
    fn date_picker_date_order(&self) -> DatePickerDateOrder;

    /// Upstream's `datePickerDateTimeOrder`.
    fn date_picker_date_time_order(&self) -> DatePickerDateTimeOrder;

    /// Upstream's `anteMeridiemAbbreviation`.
    fn ante_meridiem_abbreviation(&self) -> &str;

    /// Upstream's `postMeridiemAbbreviation`.
    fn post_meridiem_abbreviation(&self) -> &str;

    /// Upstream's `todayLabel`.
    fn today_label(&self) -> &str;

    /// Upstream's `alertDialogLabel`.
    fn alert_dialog_label(&self) -> &str;

    /// Upstream's `tabSemanticsLabel`.
    /// Upstream's `tabSemanticsLabel`, answering `None` for a tab index or
    /// count that cannot be spoken -- this port's usual grounds, stated where
    /// the implementation is: a wrong number is a bug to catch in a test, not
    /// a reason to take the application down in front of somebody using a
    /// screen reader.
    fn tab_semantics_label(&self, tab_index: u32, tab_count: u32) -> Option<String>;

    /// Upstream's `timerPickerHour`.
    fn timer_picker_hour(&self, hour: u32) -> String;

    /// Upstream's `timerPickerMinute`.
    fn timer_picker_minute(&self, minute: u32) -> String;

    /// Upstream's `timerPickerSecond`.
    fn timer_picker_second(&self, second: u32) -> String;

    /// Upstream's `timerPickerHourLabel`.
    fn timer_picker_hour_label(&self, hour: u32) -> &str;

    /// Upstream's `timerPickerHourLabels`, which is every form the label above
    /// can take -- what a locale needs to know to size the column.
    fn timer_picker_hour_labels(&self) -> &[&'static str];

    /// Upstream's `timerPickerMinuteLabel`.
    fn timer_picker_minute_label(&self, minute: u32) -> &str;

    /// Upstream's `timerPickerMinuteLabels`.
    fn timer_picker_minute_labels(&self) -> &[&'static str];

    /// Upstream's `timerPickerSecondLabel`.
    fn timer_picker_second_label(&self, second: u32) -> &str;

    /// Upstream's `timerPickerSecondLabels`.
    fn timer_picker_second_labels(&self) -> &[&'static str];

    /// Upstream's `cutButtonLabel`.
    fn cut_button_label(&self) -> &str;

    /// Upstream's `copyButtonLabel`.
    fn copy_button_label(&self) -> &str;

    /// Upstream's `pasteButtonLabel`.
    fn paste_button_label(&self) -> &str;

    /// Upstream's `clearButtonLabel`.
    fn clear_button_label(&self) -> &str;

    /// Upstream's `noSpellCheckReplacementsLabel`.
    fn no_spell_check_replacements_label(&self) -> &str;

    /// Upstream's `selectAllButtonLabel`.
    fn select_all_button_label(&self) -> &str;

    /// Upstream's `lookUpButtonLabel`.
    fn look_up_button_label(&self) -> &str;

    /// Upstream's `searchWebButtonLabel`.
    fn search_web_button_label(&self) -> &str;

    /// Upstream's `shareButtonLabel`.
    fn share_button_label(&self) -> &str;

    /// Upstream's `searchTextFieldPlaceholderLabel`.
    fn search_text_field_placeholder_label(&self) -> &str;

    /// Upstream's `modalBarrierDismissLabel`.
    fn modal_barrier_dismiss_label(&self) -> &str;

    /// Upstream's `menuDismissLabel`.
    fn menu_dismiss_label(&self) -> &str;

    /// Upstream's `cancelButtonLabel`.
    fn cancel_button_label(&self) -> &str;

    /// Upstream's `backButtonLabel`.
    fn back_button_label(&self) -> &str;

    /// Upstream's `expansionTileExpandedHint`, one of the four that **have a
    /// body on the abstract class** -- so a bundle that says nothing about
    /// them still answers, in English. The default here is upstream's default,
    /// not the default implementation's copy of it.
    fn expansion_tile_expanded_hint(&self) -> &str {
        "double tap to collapse"
    }

    /// Upstream's `expansionTileCollapsedHint`.
    fn expansion_tile_collapsed_hint(&self) -> &str {
        "double tap to expand"
    }

    /// Upstream's `expansionTileExpandedTapHint`.
    fn expansion_tile_expanded_tap_hint(&self) -> &str {
        "Collapse"
    }

    /// Upstream's `expansionTileCollapsedTapHint`.
    fn expansion_tile_collapsed_tap_hint(&self) -> &str {
        "Expand for more details"
    }

    /// Upstream's `expandedHint`.
    ///
    /// The wording is upstream's and reads backwards on purpose: `expandedHint`
    /// is 'Collapsed'. It is what a screen reader says *about* the thing, and
    /// the two are announced from the other side.
    fn expanded_hint(&self) -> &str {
        "Collapsed"
    }

    /// Upstream's `collapsedHint`.
    fn collapsed_hint(&self) -> &str {
        "Expanded"
    }
}

/// The framework's English, as the interface.
///
/// Every member is written out rather than inherited -- upstream's
/// `DefaultCupertinoLocalizations` `implements CupertinoLocalizations` for the
/// same reason: a member added upstream breaks this loudly instead of
/// inheriting something wrong. The four with bodies on the trait are the four
/// upstream gives bodies to, and this bundle repeats them because it has its
/// own constants for them and a reader looking here should find an answer, not
/// a silence.
impl CupertinoLocalizations for DefaultCupertinoLocalizations {
    fn date_picker_year(&self, year_index: i32) -> String {
        DefaultCupertinoLocalizations::date_picker_year(year_index)
    }

    fn date_picker_month(&self, month_index: usize) -> &str {
        DefaultCupertinoLocalizations::date_picker_month(month_index)
    }

    fn date_picker_standalone_month(&self, month_index: usize) -> &str {
        DefaultCupertinoLocalizations::date_picker_standalone_month(month_index)
    }

    fn date_picker_day_of_month(&self, day_index: u32, week_day: Option<u32>) -> String {
        DefaultCupertinoLocalizations::date_picker_day_of_month(day_index, week_day)
    }

    fn date_picker_medium_date(&self, week_day: u32, month: usize, day: u32) -> String {
        DefaultCupertinoLocalizations::date_picker_medium_date(week_day, month, day)
    }

    fn date_picker_hour(&self, hour: u32) -> String {
        DefaultCupertinoLocalizations::date_picker_hour(hour)
    }

    fn date_picker_hour_semantics_label(&self, hour: u32) -> String {
        DefaultCupertinoLocalizations::date_picker_hour_semantics_label(hour)
    }

    fn date_picker_minute(&self, minute: u32) -> String {
        DefaultCupertinoLocalizations::date_picker_minute(minute)
    }

    fn date_picker_minute_semantics_label(&self, minute: u32) -> String {
        DefaultCupertinoLocalizations::date_picker_minute_semantics_label(minute)
    }

    fn date_picker_date_order(&self) -> DatePickerDateOrder {
        DefaultCupertinoLocalizations::date_picker_date_order()
    }

    fn date_picker_date_time_order(&self) -> DatePickerDateTimeOrder {
        DefaultCupertinoLocalizations::date_picker_date_time_order()
    }

    fn ante_meridiem_abbreviation(&self) -> &str {
        DefaultCupertinoLocalizations::ANTE_MERIDIEM_ABBREVIATION
    }

    fn post_meridiem_abbreviation(&self) -> &str {
        DefaultCupertinoLocalizations::POST_MERIDIEM_ABBREVIATION
    }

    fn today_label(&self) -> &str {
        DefaultCupertinoLocalizations::TODAY_LABEL
    }

    fn alert_dialog_label(&self) -> &str {
        DefaultCupertinoLocalizations::ALERT_DIALOG_LABEL
    }

    fn tab_semantics_label(&self, tab_index: u32, tab_count: u32) -> Option<String> {
        DefaultCupertinoLocalizations::tab_semantics_label(tab_index, tab_count)
    }

    fn timer_picker_hour(&self, hour: u32) -> String {
        DefaultCupertinoLocalizations::timer_picker_hour(hour)
    }

    fn timer_picker_minute(&self, minute: u32) -> String {
        DefaultCupertinoLocalizations::timer_picker_minute(minute)
    }

    fn timer_picker_second(&self, second: u32) -> String {
        DefaultCupertinoLocalizations::timer_picker_second(second)
    }

    fn timer_picker_hour_label(&self, hour: u32) -> &str {
        DefaultCupertinoLocalizations::timer_picker_hour_label(hour)
    }

    fn timer_picker_hour_labels(&self) -> &[&'static str] {
        &DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
    }

    fn timer_picker_minute_label(&self, minute: u32) -> &str {
        DefaultCupertinoLocalizations::timer_picker_minute_label(minute)
    }

    fn timer_picker_minute_labels(&self) -> &[&'static str] {
        &DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS
    }

    fn timer_picker_second_label(&self, second: u32) -> &str {
        DefaultCupertinoLocalizations::timer_picker_second_label(second)
    }

    fn timer_picker_second_labels(&self) -> &[&'static str] {
        &DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS
    }

    fn cut_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::CUT_BUTTON_LABEL
    }

    fn copy_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::COPY_BUTTON_LABEL
    }

    fn paste_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::PASTE_BUTTON_LABEL
    }

    fn clear_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::CLEAR_BUTTON_LABEL
    }

    fn no_spell_check_replacements_label(&self) -> &str {
        DefaultCupertinoLocalizations::NO_SPELL_CHECK_REPLACEMENTS_LABEL
    }

    fn select_all_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::SELECT_ALL_BUTTON_LABEL
    }

    fn look_up_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::LOOK_UP_BUTTON_LABEL
    }

    fn search_web_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::SEARCH_WEB_BUTTON_LABEL
    }

    fn share_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::SHARE_BUTTON_LABEL
    }

    fn search_text_field_placeholder_label(&self) -> &str {
        DefaultCupertinoLocalizations::SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL
    }

    fn modal_barrier_dismiss_label(&self) -> &str {
        DefaultCupertinoLocalizations::MODAL_BARRIER_DISMISS_LABEL
    }

    fn menu_dismiss_label(&self) -> &str {
        DefaultCupertinoLocalizations::MENU_DISMISS_LABEL
    }

    fn cancel_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::CANCEL_BUTTON_LABEL
    }

    fn back_button_label(&self) -> &str {
        DefaultCupertinoLocalizations::BACK_BUTTON_LABEL
    }

    fn expansion_tile_expanded_hint(&self) -> &str {
        DefaultCupertinoLocalizations::EXPANSION_TILE_EXPANDED_HINT
    }

    fn expansion_tile_collapsed_hint(&self) -> &str {
        DefaultCupertinoLocalizations::EXPANSION_TILE_COLLAPSED_HINT
    }

    fn expansion_tile_expanded_tap_hint(&self) -> &str {
        DefaultCupertinoLocalizations::EXPANSION_TILE_EXPANDED_TAP_HINT
    }

    fn expansion_tile_collapsed_tap_hint(&self) -> &str {
        DefaultCupertinoLocalizations::EXPANSION_TILE_COLLAPSED_TAP_HINT
    }

    fn expanded_hint(&self) -> &str {
        DefaultCupertinoLocalizations::EXPANDED_HINT
    }

    fn collapsed_hint(&self) -> &str {
        DefaultCupertinoLocalizations::COLLAPSED_HINT
    }
}

/// Upstream's `_CupertinoLocalizationsDelegate`, the one
/// `DefaultCupertinoLocalizations.delegate` hands out.
///
/// Modelled on [`crate::localizations::DefaultWidgetsLocalizationsDelegate`],
/// which is the same thing for the widgets layer: supports every locale,
/// loads without costing a frame, and never asks to be reloaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultCupertinoLocalizationsDelegate;

impl crate::localizations::LocalizationsDelegate for DefaultCupertinoLocalizationsDelegate {
    fn resource_type(&self) -> &'static str {
        "CupertinoLocalizations"
    }

    /// Upstream: `bool isSupported(Locale locale) => locale.languageCode ==
    /// 'en'` on `_CupertinoLocalizationsDelegate` -- the **default** bundle is
    /// US English and says so, rather than claiming every locale and answering
    /// in English.
    fn is_supported(&self, locale: &crate::platform::Locale) -> bool {
        locale.language_code == "en"
    }

    /// Upstream returns a `SynchronousFuture`: the framework's own strings are
    /// already in hand, so they never cost a frame.
    fn load(&self, _locale: &crate::platform::Locale) -> crate::localizations::LoadedResources {
        crate::localizations::LoadedResources::synchronous("CupertinoLocalizations", "en_US")
    }
}

/// One column of a Cupertino date picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePickerColumn {
    Day,
    Month,
    Year,
}

/// Upstream `DatePickerDateOrder`: which order the date columns run in, left
/// to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DatePickerDateOrder {
    /// 12 | March | 1996.
    Dmy,
    /// March | 12 | 1996. What `DefaultCupertinoLocalizations` reports.
    #[default]
    Mdy,
    /// 1996 | March | 12.
    Ymd,
    /// 1996 | 12 | March.
    Ydm,
}

impl DatePickerDateOrder {
    pub const ALL: [DatePickerDateOrder; 4] = [
        DatePickerDateOrder::Dmy,
        DatePickerDateOrder::Mdy,
        DatePickerDateOrder::Ymd,
        DatePickerDateOrder::Ydm,
    ];

    /// The three columns in order.
    pub fn columns(self) -> [DatePickerColumn; 3] {
        use DatePickerColumn::{Day, Month, Year};
        match self {
            DatePickerDateOrder::Dmy => [Day, Month, Year],
            DatePickerDateOrder::Mdy => [Month, Day, Year],
            DatePickerDateOrder::Ymd => [Year, Month, Day],
            DatePickerDateOrder::Ydm => [Year, Day, Month],
        }
    }

    /// The columns a `monthYear` picker shows.
    ///
    /// Upstream writes this as a second `switch` pairing the cases up --
    /// `mdy` with `dmy` giving month|year, `ymd` with `ydm` giving year|month
    /// -- and its doc says so outright: "both `DatePickerDateOrder.dmy` and
    /// `DatePickerDateOrder.mdy` will result in the month|year order".
    ///
    /// **It is not four cases, it is the same order with the day struck
    /// out.** Removing `Day` from `Mdy` and from `Dmy` leaves month, year
    /// either way; from `Ymd` and `Ydm` it leaves year, month. Deriving it
    /// rather than restating it is what makes the pairing a consequence
    /// instead of a coincidence two lists happen to share.
    pub fn month_year_columns(self) -> [DatePickerColumn; 2] {
        let mut kept = self
            .columns()
            .into_iter()
            .filter(|column| *column != DatePickerColumn::Day);
        [
            kept.next().expect("a month and a year remain"),
            kept.next().expect("a month and a year remain"),
        ]
    }
}

/// One column of a Cupertino date-and-time picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateTimeColumn {
    Date,
    Hour,
    Minute,
    DayPeriod,
}

/// Upstream `DatePickerDateTimeOrder`: where the date sits relative to the
/// time, and which side the am/pm marker takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DatePickerDateTimeOrder {
    /// Fri Aug 31 | 02 | 08 | PM. What `DefaultCupertinoLocalizations`
    /// reports.
    #[default]
    DateTimeDayPeriod,
    /// Fri Aug 31 | PM | 02 | 08.
    DateDayPeriodTime,
    /// 02 | 08 | PM | Fri Aug 31.
    TimeDayPeriodDate,
    /// PM | 02 | 08 | Fri Aug 31.
    DayPeriodTimeDate,
}

impl DatePickerDateTimeOrder {
    pub const ALL: [DatePickerDateTimeOrder; 4] = [
        DatePickerDateTimeOrder::DateTimeDayPeriod,
        DatePickerDateTimeOrder::DateDayPeriodTime,
        DatePickerDateTimeOrder::TimeDayPeriodDate,
        DatePickerDateTimeOrder::DayPeriodTimeDate,
    ];

    /// The four columns in order.
    ///
    /// Upstream names only four of the twenty-four arrangements, and the ones
    /// it leaves out are the point: **the hour always sits immediately before
    /// the minute**, and **the date is always at one end or the other**. A
    /// clock reads left to right and a date is not something to wade through
    /// to reach the minutes.
    pub fn columns(self) -> [DateTimeColumn; 4] {
        use DateTimeColumn::{Date, DayPeriod, Hour, Minute};
        match self {
            DatePickerDateTimeOrder::DateTimeDayPeriod => [Date, Hour, Minute, DayPeriod],
            DatePickerDateTimeOrder::DateDayPeriodTime => [Date, DayPeriod, Hour, Minute],
            DatePickerDateTimeOrder::TimeDayPeriodDate => [Hour, Minute, DayPeriod, Date],
            DatePickerDateTimeOrder::DayPeriodTimeDate => [DayPeriod, Hour, Minute, Date],
        }
    }

    /// Whether the date leads. The only other place it can be is last.
    pub fn date_comes_first(self) -> bool {
        self.columns()[0] == DateTimeColumn::Date
    }
}

/// Upstream `DefaultCupertinoLocalizations`, documented -- like its Material
/// opposite number -- as being *"for US English (only)"*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultCupertinoLocalizations;

impl DefaultCupertinoLocalizations {
    /// Upstream's `CupertinoLocalizations.of`, which asserts
    /// `debugCheckHasCupertinoLocalizations` and then force-unwraps -- so a
    /// missing delegate is a debug error with an explanation rather than a
    /// null in release.
    ///
    /// It answers with the default bundle because that is the only one
    /// registered; the signature is the shape upstream's has, so a lookup can
    /// replace the body without moving the callers. It lives here rather than
    /// on [`CupertinoLocalizations`] for the reason it lives on
    /// `DefaultMaterialLocalizations` there: the interface is a trait now, and
    /// a trait cannot hand back an instance of itself.
    pub fn of(present: bool) -> Option<DefaultCupertinoLocalizations> {
        present.then_some(DefaultCupertinoLocalizations)
    }

    /// The same seven strings as `DefaultMaterialLocalizations`, indexed the
    /// same way, by `weekDay - DateTime.monday`.
    ///
    /// And **with no comment above them at all** -- where the Material file
    /// explains the ordering twice and gets it wrong both times
    /// (`// Ordered to match DateTime.monday=1, DateTime.sunday=6`, and Sunday
    /// is 7). Two files doing the identical correct thing; only one of them
    /// explains itself, and that is the one a reader can be misled by.
    pub const SHORT_WEEKDAYS: [&'static str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    /// The seven standard context menu entries that have a label of their
    /// own on this platform, which arrived with
    /// [`crate::icon_data::ContextMenuButtonType::cupertino_label`]. Only
    /// strings something already says are here, which is the rule
    /// `tools/unread_strings.py` exists to keep.
    pub const CUT_BUTTON_LABEL: &'static str = "Cut";
    pub const COPY_BUTTON_LABEL: &'static str = "Copy";
    pub const PASTE_BUTTON_LABEL: &'static str = "Paste";
    /// Upstream `clearButtonLabel`, and it sits in this list without being
    /// one of its neighbours: the cut/copy/paste words are **painted** on
    /// toolbar buttons, and this one is never drawn at all.
    ///
    /// `CupertinoTextField`'s clear button is an icon --
    /// `CupertinoIcons.clear_thick_circled` -- and the word exists only for a
    /// screen reader:
    ///
    /// ```dart
    /// final String clearLabel =
    ///     widget.clearButtonSemanticLabel ?? CupertinoLocalizations.of(context).clearButtonLabel;
    /// return Semantics(button: true, label: clearLabel, child: ...);
    /// ```
    ///
    /// So it is a localization whose whole audience cannot see the control it
    /// names, which is why the per-widget override is a
    /// `clearButtonSemanticLabel` rather than a `clearButtonText`.
    pub const CLEAR_BUTTON_LABEL: &'static str = "Clear";

    /// Upstream's `alertDialogLabel`. It and the three below it are the four
    /// names this bundle shares with `DefaultMaterialLocalizations`, and they
    /// had never been written down here at all -- the Cupertino widgets that
    /// wanted them reached across to the Material bundle, which is a locale
    /// deciding one of these words for both designs at once. See
    /// [`CupertinoLocalizations`].
    pub const ALERT_DIALOG_LABEL: &'static str = "Alert";

    /// Upstream's `modalBarrierDismissLabel`.
    pub const MODAL_BARRIER_DISMISS_LABEL: &'static str = "Dismiss";

    /// Upstream's `menuDismissLabel`.
    pub const MENU_DISMISS_LABEL: &'static str = "Dismiss menu";

    /// Upstream's `cancelButtonLabel`.
    pub const CANCEL_BUTTON_LABEL: &'static str = "Cancel";
    /// Upstream `backButtonLabel`, and it does **two** jobs in the navigation
    /// bar that are worth keeping apart.
    ///
    /// It is the `Semantics.label` of the whole back button, under
    /// `excludeSemantics: true` -- so a screen reader hears this word and
    /// never the visible label, whatever that turned out to be. And it is the
    /// **replacement** for a previous-page title longer than twelve UTF-16
    /// units, which is a visible substitution rather than an announcement.
    ///
    /// See [`crate::cupertino::CupertinoNavigationBarBackButton::label_for`].
    pub const BACK_BUTTON_LABEL: &'static str = "Back";
    /// Upstream `noSpellCheckReplacementsLabel`, the sentence a Cupertino
    /// spell-check toolbar shows **instead of** suggestions.
    ///
    /// It is a label on a `ContextMenuButtonItem` with `onPressed: null`, so
    /// it arrives wearing a button's clothes and cannot be pressed. See
    /// [`crate::cupertino_refresh::CupertinoSpellCheckSuggestionsToolbar::build_button_items`]
    /// for the three-way choice it belongs to.
    pub const NO_SPELL_CHECK_REPLACEMENTS_LABEL: &'static str = "No Replacements Found";
    /// Upstream `searchTextFieldPlaceholderLabel`, and **the opposite case**:
    /// this one is painted.
    ///
    /// ```dart
    /// final String placeholder =
    ///     widget.placeholder ?? CupertinoLocalizations.of(context).searchTextFieldPlaceholderLabel;
    /// ```
    ///
    /// The two are written the same way -- a widget property falling back to
    /// the localizations -- and land in different halves of the widget. A
    /// search field with no placeholder set is not an empty well: it says
    /// "Search".
    pub const SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL: &'static str = "Search";
    /// Capital A, where the Material table writes "Select all". The two are
    /// translated separately, so the difference is not a typo that would be
    /// caught downstream -- it survives into every locale.
    pub const SELECT_ALL_BUTTON_LABEL: &'static str = "Select All";
    pub const LOOK_UP_BUTTON_LABEL: &'static str = "Look Up";
    pub const SEARCH_WEB_BUTTON_LABEL: &'static str = "Search Web";
    /// With the ellipsis. On iOS sharing opens a sheet, and the three dots
    /// are that platform's promise that the button will not act on its own.
    pub const SHARE_BUTTON_LABEL: &'static str = "Share...";

    pub const SHORT_MONTHS: [&'static str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    pub const MONTHS: [&'static str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    /// Dart's `DateTime.monday`.
    pub const MONDAY: u32 = 1;

    /// Upstream `datePickerMonth`, one-based like the `DateTime` constant it is
    /// fed from.
    pub fn date_picker_month(month_index: usize) -> &'static str {
        DefaultCupertinoLocalizations::MONTHS[month_index - 1]
    }

    /// Upstream `datePickerStandaloneMonth`, the month name for
    /// `CupertinoDatePickerMode.monthYear` -- where the month has no day in
    /// front of it.
    ///
    /// **In English this returns exactly what
    /// [`Self::date_picker_month`] returns**, and upstream's default class
    /// says so by writing the same line twice:
    ///
    /// ```dart
    /// String datePickerMonth(int monthIndex) => _months[monthIndex - 1];
    /// String datePickerStandaloneMonth(int monthIndex) => _months[monthIndex - 1];
    /// ```
    ///
    /// So this is a distinction that **cannot be seen in the output a port
    /// built against English would ever look at**, which is exactly why it is
    /// worth having: upstream's doc names the case it is for --
    ///
    /// > This is distinct from [datePickerMonth] because in some languages,
    /// > like Russian, the name of a month takes a different form depending on
    /// > whether it is preceded by a day or whether it stands alone.
    ///
    /// -- Russian's January is `января` after a day and `Январь` on its own.
    /// A port that called `datePickerMonth` in `monthYear` mode would not be
    /// wrong in English and would be wrong in Russian, and nothing in English
    /// would ever say so. Keeping the two names apart is the whole of it; the
    /// bodies agreeing is a fact about English, not about the port.
    ///
    /// The real class differs in **two** ways, both invisible here.
    /// `GlobalCupertinoLocalizations` reads a different symbol table *and*
    /// runs the result through a capitalisation pass:
    ///
    /// ```dart
    /// String datePickerMonth(int i) => _fullYearFormat.dateSymbols.MONTHS[i - 1];
    ///
    /// String datePickerStandaloneMonth(int i) =>
    ///     intl.toBeginningOfSentenceCase(
    ///       _fullYearFormat.dateSymbols.STANDALONEMONTHS[i - 1]) ?? ...;
    /// ```
    ///
    /// with the reason written beside it: *"Because this will be used without
    /// specifying any day of month, in most cases it should be capitalized"*.
    /// A month standing on its own begins a phrase; a month after a day is in
    /// the middle of one.
    ///
    /// No capitalisation step is written here. This crate's month names are a
    /// fixed English array that is already sentence-cased, so the pass could
    /// only ever be a no-op, and a transformation that provably never
    /// transforms anything is the sort of thing `hollow.py` exists to catch.
    /// The rule is recorded rather than performed.
    pub fn date_picker_standalone_month(month_index: usize) -> &'static str {
        DefaultCupertinoLocalizations::MONTHS[month_index - 1]
    }

    /// Upstream `datePickerYear`, which is `yearIndex.toString()` -- no padding,
    /// where the Material `formatCompactDate` pads to four digits.
    pub fn date_picker_year(year_index: i32) -> String {
        year_index.to_string()
    }

    /// Upstream `datePickerDayOfMonth`.
    ///
    /// Note the returned string: `' ${_shortWeekdays[...]} $dayIndex '` --
    /// **a leading and a trailing space baked into the localisation itself**,
    /// for the spinner it will be dropped into. A string that carries its own
    /// padding, which is layout hiding inside a translation.
    pub fn date_picker_day_of_month(day_index: u32, week_day: Option<u32>) -> String {
        match week_day {
            Some(week_day) => format!(
                " {} {day_index} ",
                DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                    [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize]
            ),
            None => day_index.to_string(),
        }
    }

    /// Upstream's `datePickerDateOrder` for US English.
    pub fn date_picker_date_order() -> DatePickerDateOrder {
        DatePickerDateOrder::Mdy
    }

    /// Upstream's `datePickerDateTimeOrder` for US English.
    pub fn date_picker_date_time_order() -> DatePickerDateTimeOrder {
        DatePickerDateTimeOrder::DateTimeDayPeriod
    }

    /// Upstream `datePickerMediumDate`, the other use of the weekday list.
    ///
    /// The day is `padRight(2)`, not `padLeft`: a single-digit day carries a
    /// **trailing** space, so "Fri Aug 1 " is as wide as "Fri Aug 31" and the
    /// column does not shuffle as the wheel turns. The same layout-inside-a-
    /// translation as [`DefaultCupertinoLocalizations::date_picker_day_of_month`].
    pub fn date_picker_medium_date(week_day: u32, month: usize, day: u32) -> String {
        format!(
            "{} {} {:<2}",
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize],
            DefaultCupertinoLocalizations::SHORT_MONTHS[month - 1],
            day
        )
    }

    /// Upstream `datePickerHour`, which is `hour.toString()` -- **unpadded**,
    /// where the minute beside it is padded to two. A twelve-hour clock reads
    /// "9:05", not "09:05".
    pub fn date_picker_hour(hour: u32) -> String {
        hour.to_string()
    }

    /// Upstream `datePickerMinute`: `padLeft(2, '0')`.
    pub fn date_picker_minute(minute: u32) -> String {
        format!("{minute:02}")
    }

    pub const ANTE_MERIDIEM_ABBREVIATION: &'static str = "AM";
    pub const POST_MERIDIEM_ABBREVIATION: &'static str = "PM";

    /// Upstream `todayLabel`, which the `dateAndTime` date column shows in
    /// place of today's date.
    pub const TODAY_LABEL: &'static str = "Today";

    /// Upstream `timerPickerHour`/`timerPickerMinute`/`timerPickerSecond`:
    /// all three are `toString()`, **none of them padded** -- a countdown's
    /// columns carry their own unit labels, so there is nothing to line up
    /// against.
    pub fn timer_picker_hour(hour: u32) -> String {
        hour.to_string()
    }

    pub fn timer_picker_minute(minute: u32) -> String {
        minute.to_string()
    }

    pub fn timer_picker_second(second: u32) -> String {
        second.to_string()
    }

    /// Upstream `timerPickerHourLabel`, **the only one of the three that is
    /// plural-sensitive**: `hour == 1 ? 'hour' : 'hours'`. The other two are
    /// abbreviations with a full stop and do not inflect.
    pub fn timer_picker_hour_label(hour: u32) -> &'static str {
        if hour == 1 { "hour" } else { "hours" }
    }

    pub fn timer_picker_minute_label(_minute: u32) -> &'static str {
        "min."
    }

    pub fn timer_picker_second_label(_second: u32) -> &'static str {
        "sec."
    }

    /// Upstream `timerPickerHourLabels` / `timerPickerMinuteLabels` /
    /// `timerPickerSecondLabels`: **every** form the label above can take,
    /// not the one it is taking.
    ///
    /// They exist for one job, and it is a layout job. `CupertinoTimerPicker`
    /// measures its label column with
    ///
    /// ```dart
    /// hourLabelWidth = _measureLabelsMaxWidth(localizations.timerPickerHourLabels, textStyle);
    /// ```
    ///
    /// -- the widest of *all* of them. Measuring the label currently on screen
    /// instead would size the column to "hour" while the wheel sits on 1, and
    /// then the column would have to grow the moment the reader spun to 2:
    /// the number beside it would shift sideways as they scrolled, which is
    /// the one thing a spinner must not do.
    ///
    /// So the singular list is not an oversight. English minutes and seconds
    /// are abbreviations that do not inflect, so their lists hold one entry;
    /// hours inflect, so that list holds two. A language whose hour has six
    /// forms would return six here and the column would be sized for the
    /// longest of them.
    ///
    /// [`DefaultCupertinoLocalizations::labels_cover_every_form`] is the
    /// invariant that keeps these lists honest.
    pub const TIMER_PICKER_HOUR_LABELS: [&'static str; 2] = ["hour", "hours"];
    pub const TIMER_PICKER_MINUTE_LABELS: [&'static str; 1] = ["min."];
    pub const TIMER_PICKER_SECOND_LABELS: [&'static str; 1] = ["sec."];

    // -- What a screen reader is told about a spinner ----------------------
    //
    // A date picker's columns show numbers, and a number read out on its own
    // says nothing: "3" in a column of hours and "3" in a column of minutes
    // are the same utterance for two different things. Upstream gives each
    // column a `semanticsLabel` distinct from the text it paints.

    /// Upstream `datePickerHourSemanticsLabel`, `"$hour o'clock"`.
    ///
    /// **The number is the displayed hour, not the wheel's index.** Upstream
    /// converts first --
    ///
    /// ```dart
    /// final int displayHour = widget.use24hFormat ? hour : (hour + 11) % 12 + 1;
    /// ...
    /// semanticsLabel: localizations.datePickerHourSemanticsLabel(displayHour),
    /// ```
    ///
    /// -- so in twelve-hour mode the row that paints "1" is heard as "1
    /// o'clock" even though it stands for 13:00. Handing it the raw hour would
    /// make the reader and the screen disagree by twelve.
    ///
    /// It does not inflect. The generated English class carries the plural
    /// machinery anyway and fills both categories with the same string
    /// (`datePickerHourSemanticsLabelOne` and `...Other` are both
    /// `"$hour o'clock"`), which is the same shape as the timer picker's
    /// one-entry label lists: the categories exist for languages that need
    /// them, not because English does.
    pub fn date_picker_hour_semantics_label(display_hour: u32) -> String {
        format!("{display_hour} o'clock")
    }

    /// Upstream `datePickerMinuteSemanticsLabel`, and **the one in this group
    /// that does inflect**:
    ///
    /// ```dart
    /// if (minute == 1) {
    ///   return '1 minute';
    /// }
    /// return '$minute minutes';
    /// ```
    ///
    /// Zero takes the plural -- "0 minutes" -- which is English's rule and not
    /// every language's.
    pub fn date_picker_minute_semantics_label(minute: u32) -> String {
        if minute == 1 {
            "1 minute".to_string()
        } else {
            format!("{minute} minutes")
        }
    }

    /// Upstream `tabSemanticsLabel`, `"Tab $tabIndex of $tabCount"`, with both
    /// of its asserts:
    ///
    /// ```dart
    /// assert(tabIndex >= 1);
    /// assert(tabCount >= 1);
    /// ```
    ///
    /// **The index is one-based and the caller's is not.** `CupertinoTabBar`
    /// passes `tabIndex: index + 1` from a zero-based loop, and the assert is
    /// what stands between a forgotten `+ 1` and a reader hearing "Tab 0 of
    /// 3". Returns `None` rather than asserting, on this port's usual grounds
    /// -- a wrong number is a bug to catch in a test, not a reason to take the
    /// application down in front of somebody using a screen reader.
    pub fn tab_semantics_label(tab_index: u32, tab_count: u32) -> Option<String> {
        if tab_index < 1 || tab_count < 1 {
            return None;
        }
        Some(format!("Tab {tab_index} of {tab_count}"))
    }

    // -- What a screen reader is told about an expansion tile --------------
    //
    // Upstream declares these six on `CupertinoLocalizations` **and** on
    // `MaterialLocalizations`, with the same English values, and
    // `CupertinoExpansionTile` reads the Cupertino ones:
    //
    //     final CupertinoLocalizations localizations = CupertinoLocalizations.of(context);
    //
    // They are the same words today and two independent contracts, because a
    // locale supplies each class separately -- so a translation could give the
    // Cupertino tile a different phrasing from the Material one and neither
    // class would know. Reading the Material copy from a Cupertino widget
    // happens to be right in English and is not the thing upstream wrote.
    //
    // The crossing between the names and the values is explained where the
    // pairing happens, at
    // [`crate::material_app::DefaultMaterialLocalizations::expansion_tile_hint`].

    /// Upstream's `expandedHint`, which describes the **expanded** state and
    /// whose value is "Collapsed".
    pub const EXPANDED_HINT: &'static str = "Collapsed";
    /// Upstream's `collapsedHint`, which describes the **collapsed** state and
    /// whose value is "Expanded".
    pub const COLLAPSED_HINT: &'static str = "Expanded";
    pub const EXPANSION_TILE_EXPANDED_HINT: &'static str = "double tap to collapse";
    pub const EXPANSION_TILE_COLLAPSED_HINT: &'static str = "double tap to expand";
    /// Not crossed -- see
    /// [`crate::material_app::DefaultMaterialLocalizations::EXPANSION_TILE_EXPANDED_TAP_HINT`].
    pub const EXPANSION_TILE_EXPANDED_TAP_HINT: &'static str = "Collapse";
    pub const EXPANSION_TILE_COLLAPSED_TAP_HINT: &'static str = "Expand for more details";

    /// Upstream's `'${state}\n ${action}'`, on iOS and macOS only.
    pub fn expansion_tile_hint(expanded: bool) -> String {
        if expanded {
            format!(
                "{}\n {}",
                Self::COLLAPSED_HINT,
                Self::EXPANSION_TILE_EXPANDED_HINT
            )
        } else {
            format!(
                "{}\n {}",
                Self::EXPANDED_HINT,
                Self::EXPANSION_TILE_COLLAPSED_HINT
            )
        }
    }

    /// Upstream's `onTapHint`, which is a separate semantics field and is
    /// **not** crossed.
    pub fn expansion_tile_tap_hint(expanded: bool) -> &'static str {
        if expanded {
            Self::EXPANSION_TILE_EXPANDED_TAP_HINT
        } else {
            Self::EXPANSION_TILE_COLLAPSED_TAP_HINT
        }
    }

    /// Whether the three lists above really cover what the three functions
    /// above can return, over `0..=count`.
    ///
    /// Upstream cannot check this -- the list and the function are separate
    /// overrides in every one of its eighty-odd locale classes -- and this
    /// port had the same pair written twice with nothing between them: the
    /// picker's own metrics carried a private `["hour", "hours"]` of their
    /// own, so renaming a label would have left the column measured for the
    /// old word and nothing would have said so.
    pub fn labels_cover_every_form(count: u32) -> bool {
        (0..=count).all(|n| {
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
                .contains(&DefaultCupertinoLocalizations::timer_picker_hour_label(n))
                && DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS
                    .contains(&DefaultCupertinoLocalizations::timer_picker_minute_label(n))
                && DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS
                    .contains(&DefaultCupertinoLocalizations::timer_picker_second_label(n))
        })
    }
}

/// Upstream `CupertinoLocalizationEn`, the English member of
/// `flutter_localizations`' `GlobalCupertinoLocalizations`.
///
/// # Why there are two of these
///
/// [`DefaultCupertinoLocalizations`] above is the class in `packages/flutter`,
/// and it is what an application gets when it installs **no** localisation
/// delegates. Every real application installs `GlobalCupertinoLocalizations`,
/// and the strings differ where it matters most to a picker: the default class
/// writes an hour as `hour.toString()`, and this one runs it through
/// `intl.DateFormat('HH')` -- so a date picker shows `01` under one and `1`
/// under the other. The gallery installs the delegate, so these are the
/// strings on its screen, and they are the ones [`crate::cupertino_pickers`]
/// reads.
///
/// The formats are the skeletons `_GlobalCupertinoLocalizationsDelegate.
/// loadFormats` builds, resolved for `en_US`. This crate compiles one locale,
/// so they are resolved once here rather than looked up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoLocalizationEn;

impl CupertinoLocalizationEn {
    /// `intl.DateFormat.y` -- the year, unpadded.
    pub fn date_picker_year(year_index: i32) -> String {
        year_index.to_string()
    }

    /// `_fullYearFormat.dateSymbols.MONTHS[monthIndex - 1]`, which for English
    /// is the same list [`DefaultCupertinoLocalizations`] carries.
    pub fn date_picker_month(month_index: usize) -> &'static str {
        DefaultCupertinoLocalizations::MONTHS[month_index - 1]
    }

    /// The generated English class inherits `datePickerStandaloneMonth` from
    /// `GlobalCupertinoLocalizations`, which reads `STANDALONEMONTHS` and
    /// sentence-cases it. English's two symbol tables hold the same twelve
    /// already-capitalised names, so both paths land on the same word --
    /// see [`DefaultCupertinoLocalizations::date_picker_standalone_month`]
    /// for the two differences and why neither is written out here.
    pub fn date_picker_standalone_month(month_index: usize) -> &'static str {
        DefaultCupertinoLocalizations::MONTHS[month_index - 1]
    }

    /// `intl.DateFormat.d` for the day, with `intl.DateFormat.E` in front of
    /// it when a weekday is asked for -- **no leading or trailing spaces**,
    /// where the default class bakes them into the string.
    pub fn date_picker_day_of_month(day_index: u32, week_day: Option<u32>) -> String {
        match week_day {
            Some(week_day) => format!(
                "{} {day_index}",
                DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                    [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize]
            ),
            None => day_index.to_string(),
        }
    }

    /// `intl.DateFormat.MMMEd`, which for `en_US` is `EEE, MMM d` -- **with a
    /// comma**, where the default class's hand-written version has none and
    /// pads the day instead.
    pub fn date_picker_medium_date(week_day: u32, month: usize, day: u32) -> String {
        format!(
            "{}, {} {day}",
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS
                [(week_day - DefaultCupertinoLocalizations::MONDAY) as usize],
            DefaultCupertinoLocalizations::SHORT_MONTHS[month - 1]
        )
    }

    /// `intl.DateFormat('HH')`: **two digits**. Upstream's own comment on the
    /// field explains the odd skeleton -- "We don't want any additional
    /// decoration here. The am/pm is handled in the date picker. We just want
    /// an hour number localized" -- with a TODO pointing at the intl issue
    /// that would let it ask for the hour alone.
    pub fn date_picker_hour(hour: u32) -> String {
        format!("{hour:02}")
    }

    /// `intl.DateFormat('mm')`: two digits.
    pub fn date_picker_minute(minute: u32) -> String {
        format!("{minute:02}")
    }

    /// `intl.DateFormat('HH')` again -- a countdown's hours are padded even
    /// though its minutes and seconds are not.
    pub fn timer_picker_hour(hour: u32) -> String {
        format!("{hour:02}")
    }

    /// `intl.DateFormat.m`: the minute alone, unpadded.
    pub fn timer_picker_minute(minute: u32) -> String {
        minute.to_string()
    }

    /// `intl.DateFormat.s`: the second alone, unpadded.
    pub fn timer_picker_second(second: u32) -> String {
        second.to_string()
    }

    /// The four strings the generated `CupertinoLocalizationEn` carries
    /// verbatim, which are the same as the default class's.
    pub const ANTE_MERIDIEM_ABBREVIATION: &'static str = "AM";
    pub const POST_MERIDIEM_ABBREVIATION: &'static str = "PM";
    pub const TODAY_LABEL: &'static str = "Today";

    pub fn timer_picker_hour_label(hour: u32) -> &'static str {
        DefaultCupertinoLocalizations::timer_picker_hour_label(hour)
    }

    pub fn timer_picker_minute_label(minute: u32) -> &'static str {
        DefaultCupertinoLocalizations::timer_picker_minute_label(minute)
    }

    pub fn timer_picker_second_label(second: u32) -> &'static str {
        DefaultCupertinoLocalizations::timer_picker_second_label(second)
    }

    /// The generated English class overrides these three the same way it
    /// overrides the singular labels: with the same words. Forwarded rather
    /// than repeated, so the pair cannot drift apart here either.
    pub const TIMER_PICKER_HOUR_LABELS: [&'static str; 2] =
        DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS;
    pub const TIMER_PICKER_MINUTE_LABELS: [&'static str; 1] =
        DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS;
    pub const TIMER_PICKER_SECOND_LABELS: [&'static str; 1] =
        DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS;

    /// Both orders are the generated class's, and both are the default
    /// class's too.
    pub fn date_picker_date_order() -> DatePickerDateOrder {
        DatePickerDateOrder::Mdy
    }

    pub fn date_picker_date_time_order() -> DatePickerDateTimeOrder {
        DatePickerDateTimeOrder::DateTimeDayPeriod
    }

    // -- The strings `CupertinoLocalizationEn` declares -------------------
    //
    // Upstream generates this class from `cupertino_en.arb`, and it writes
    // every one of these out even though `DefaultCupertinoLocalizations`
    // already says the same words. That is not a redundancy to remove: the two
    // are independently sourced, and they agree **in English only**. Writing
    // them out here keeps that true rather than assumed -- see
    // `the_two_bundles_agree_on_words_and_differ_on_numbers`, which is what
    // says so out loud.

    /// Upstream's `alertDialogLabel`.
    pub const ALERT_DIALOG_LABEL: &'static str = "Alert";
    /// Upstream's `backButtonLabel`.
    pub const BACK_BUTTON_LABEL: &'static str = "Back";
    /// Upstream's `cancelButtonLabel`.
    pub const CANCEL_BUTTON_LABEL: &'static str = "Cancel";
    /// Upstream's `clearButtonLabel`.
    pub const CLEAR_BUTTON_LABEL: &'static str = "Clear";
    /// Upstream's `copyButtonLabel`.
    pub const COPY_BUTTON_LABEL: &'static str = "Copy";
    /// Upstream's `cutButtonLabel`.
    pub const CUT_BUTTON_LABEL: &'static str = "Cut";
    /// Upstream's `pasteButtonLabel`.
    pub const PASTE_BUTTON_LABEL: &'static str = "Paste";
    /// Upstream's `lookUpButtonLabel`.
    pub const LOOK_UP_BUTTON_LABEL: &'static str = "Look Up";
    /// Upstream's `menuDismissLabel`.
    pub const MENU_DISMISS_LABEL: &'static str = "Dismiss menu";
    /// Upstream's `modalBarrierDismissLabel`.
    pub const MODAL_BARRIER_DISMISS_LABEL: &'static str = "Dismiss";
    /// Upstream's `noSpellCheckReplacementsLabel`.
    pub const NO_SPELL_CHECK_REPLACEMENTS_LABEL: &'static str = "No Replacements Found";
    /// Upstream's `searchTextFieldPlaceholderLabel`.
    pub const SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL: &'static str = "Search";
    /// Upstream's `searchWebButtonLabel`.
    pub const SEARCH_WEB_BUTTON_LABEL: &'static str = "Search Web";
    /// Upstream's `selectAllButtonLabel`.
    pub const SELECT_ALL_BUTTON_LABEL: &'static str = "Select All";
    /// Upstream's `shareButtonLabel`.
    pub const SHARE_BUTTON_LABEL: &'static str = "Share...";
    /// Upstream's `expandedHint` -- and it reads backwards on purpose, here as
    /// there: what a screen reader says *about* the thing.
    pub const EXPANDED_HINT: &'static str = "Collapsed";
    /// Upstream's `collapsedHint`.
    pub const COLLAPSED_HINT: &'static str = "Expanded";
    /// Upstream's `expansionTileExpandedHint`.
    pub const EXPANSION_TILE_EXPANDED_HINT: &'static str = "double tap to collapse";
    /// Upstream's `expansionTileCollapsedHint`.
    pub const EXPANSION_TILE_COLLAPSED_HINT: &'static str = "double tap to expand";
    /// Upstream's `expansionTileExpandedTapHint`.
    pub const EXPANSION_TILE_EXPANDED_TAP_HINT: &'static str = "Collapse";
    /// Upstream's `expansionTileCollapsedTapHint`.
    pub const EXPANSION_TILE_COLLAPSED_TAP_HINT: &'static str = "Expand for more details";

    /// Upstream's `datePickerHourSemanticsLabelOne` and `...Other`, which are
    /// **the same string** in English -- so the plural has one arm here and
    /// the count is still what chooses it in a locale where they differ.
    pub fn date_picker_hour_semantics_label(hour: u32) -> String {
        format!("{hour} o'clock")
    }

    /// Upstream's `datePickerMinuteSemanticsLabelOne` ('1 minute') and
    /// `...Other` (r'$minute minutes'), which in English are **not** the same.
    pub fn date_picker_minute_semantics_label(minute: u32) -> String {
        if minute == 1 {
            "1 minute".to_string()
        } else {
            format!("{minute} minutes")
        }
    }

    /// Upstream's `tabSemanticsLabelRaw`, r'Tab $tabIndex of $tabCount', with
    /// the two numbers put in -- and `None` where the numbers cannot be
    /// spoken, on the grounds `DefaultCupertinoLocalizations` states.
    pub fn tab_semantics_label(tab_index: u32, tab_count: u32) -> Option<String> {
        if tab_index < 1 || tab_count < 1 {
            return None;
        }
        Some(format!("Tab {tab_index} of {tab_count}"))
    }
}

/// The bundle a real application runs, as the interface.
///
/// Written out member by member like the default one, and for the same reason:
/// upstream's generated class `extends GlobalCupertinoLocalizations` and
/// supplies every string, so a member added upstream has to be answered here
/// rather than inherited from somewhere that happens to compile.
///
/// The date and time half is **this bundle's own**, and that is the whole
/// point of there being two: `date_picker_hour` is `01` here and `1` on
/// [`DefaultCupertinoLocalizations`]. The words agree, in English.
impl CupertinoLocalizations for CupertinoLocalizationEn {
    fn date_picker_year(&self, year_index: i32) -> String {
        CupertinoLocalizationEn::date_picker_year(year_index)
    }

    fn date_picker_month(&self, month_index: usize) -> &str {
        CupertinoLocalizationEn::date_picker_month(month_index)
    }

    fn date_picker_standalone_month(&self, month_index: usize) -> &str {
        CupertinoLocalizationEn::date_picker_standalone_month(month_index)
    }

    fn date_picker_day_of_month(&self, day_index: u32, week_day: Option<u32>) -> String {
        CupertinoLocalizationEn::date_picker_day_of_month(day_index, week_day)
    }

    fn date_picker_medium_date(&self, week_day: u32, month: usize, day: u32) -> String {
        CupertinoLocalizationEn::date_picker_medium_date(week_day, month, day)
    }

    fn date_picker_hour(&self, hour: u32) -> String {
        CupertinoLocalizationEn::date_picker_hour(hour)
    }

    fn date_picker_hour_semantics_label(&self, hour: u32) -> String {
        CupertinoLocalizationEn::date_picker_hour_semantics_label(hour)
    }

    fn date_picker_minute(&self, minute: u32) -> String {
        CupertinoLocalizationEn::date_picker_minute(minute)
    }

    fn date_picker_minute_semantics_label(&self, minute: u32) -> String {
        CupertinoLocalizationEn::date_picker_minute_semantics_label(minute)
    }

    fn date_picker_date_order(&self) -> DatePickerDateOrder {
        CupertinoLocalizationEn::date_picker_date_order()
    }

    fn date_picker_date_time_order(&self) -> DatePickerDateTimeOrder {
        CupertinoLocalizationEn::date_picker_date_time_order()
    }

    fn ante_meridiem_abbreviation(&self) -> &str {
        CupertinoLocalizationEn::ANTE_MERIDIEM_ABBREVIATION
    }

    fn post_meridiem_abbreviation(&self) -> &str {
        CupertinoLocalizationEn::POST_MERIDIEM_ABBREVIATION
    }

    fn today_label(&self) -> &str {
        CupertinoLocalizationEn::TODAY_LABEL
    }

    fn alert_dialog_label(&self) -> &str {
        CupertinoLocalizationEn::ALERT_DIALOG_LABEL
    }

    fn tab_semantics_label(&self, tab_index: u32, tab_count: u32) -> Option<String> {
        CupertinoLocalizationEn::tab_semantics_label(tab_index, tab_count)
    }

    fn timer_picker_hour(&self, hour: u32) -> String {
        CupertinoLocalizationEn::timer_picker_hour(hour)
    }

    fn timer_picker_minute(&self, minute: u32) -> String {
        CupertinoLocalizationEn::timer_picker_minute(minute)
    }

    fn timer_picker_second(&self, second: u32) -> String {
        CupertinoLocalizationEn::timer_picker_second(second)
    }

    fn timer_picker_hour_label(&self, hour: u32) -> &str {
        CupertinoLocalizationEn::timer_picker_hour_label(hour)
    }

    fn timer_picker_hour_labels(&self) -> &[&'static str] {
        &CupertinoLocalizationEn::TIMER_PICKER_HOUR_LABELS
    }

    fn timer_picker_minute_label(&self, minute: u32) -> &str {
        CupertinoLocalizationEn::timer_picker_minute_label(minute)
    }

    fn timer_picker_minute_labels(&self) -> &[&'static str] {
        &CupertinoLocalizationEn::TIMER_PICKER_MINUTE_LABELS
    }

    fn timer_picker_second_label(&self, second: u32) -> &str {
        CupertinoLocalizationEn::timer_picker_second_label(second)
    }

    fn timer_picker_second_labels(&self) -> &[&'static str] {
        &CupertinoLocalizationEn::TIMER_PICKER_SECOND_LABELS
    }

    fn cut_button_label(&self) -> &str {
        CupertinoLocalizationEn::CUT_BUTTON_LABEL
    }

    fn copy_button_label(&self) -> &str {
        CupertinoLocalizationEn::COPY_BUTTON_LABEL
    }

    fn paste_button_label(&self) -> &str {
        CupertinoLocalizationEn::PASTE_BUTTON_LABEL
    }

    fn clear_button_label(&self) -> &str {
        CupertinoLocalizationEn::CLEAR_BUTTON_LABEL
    }

    fn no_spell_check_replacements_label(&self) -> &str {
        CupertinoLocalizationEn::NO_SPELL_CHECK_REPLACEMENTS_LABEL
    }

    fn select_all_button_label(&self) -> &str {
        CupertinoLocalizationEn::SELECT_ALL_BUTTON_LABEL
    }

    fn look_up_button_label(&self) -> &str {
        CupertinoLocalizationEn::LOOK_UP_BUTTON_LABEL
    }

    fn search_web_button_label(&self) -> &str {
        CupertinoLocalizationEn::SEARCH_WEB_BUTTON_LABEL
    }

    fn share_button_label(&self) -> &str {
        CupertinoLocalizationEn::SHARE_BUTTON_LABEL
    }

    fn search_text_field_placeholder_label(&self) -> &str {
        CupertinoLocalizationEn::SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL
    }

    fn modal_barrier_dismiss_label(&self) -> &str {
        CupertinoLocalizationEn::MODAL_BARRIER_DISMISS_LABEL
    }

    fn menu_dismiss_label(&self) -> &str {
        CupertinoLocalizationEn::MENU_DISMISS_LABEL
    }

    fn cancel_button_label(&self) -> &str {
        CupertinoLocalizationEn::CANCEL_BUTTON_LABEL
    }

    fn back_button_label(&self) -> &str {
        CupertinoLocalizationEn::BACK_BUTTON_LABEL
    }

    fn expansion_tile_expanded_hint(&self) -> &str {
        CupertinoLocalizationEn::EXPANSION_TILE_EXPANDED_HINT
    }

    fn expansion_tile_collapsed_hint(&self) -> &str {
        CupertinoLocalizationEn::EXPANSION_TILE_COLLAPSED_HINT
    }

    fn expansion_tile_expanded_tap_hint(&self) -> &str {
        CupertinoLocalizationEn::EXPANSION_TILE_EXPANDED_TAP_HINT
    }

    fn expansion_tile_collapsed_tap_hint(&self) -> &str {
        CupertinoLocalizationEn::EXPANSION_TILE_COLLAPSED_TAP_HINT
    }

    fn expanded_hint(&self) -> &str {
        CupertinoLocalizationEn::EXPANDED_HINT
    }

    fn collapsed_hint(&self) -> &str {
        CupertinoLocalizationEn::COLLAPSED_HINT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_app::{MaterialScrollBehavior, ScrollAxis};

    const ALL: [ScrollPlatform; 6] = [
        ScrollPlatform::Android,
        ScrollPlatform::Fuchsia,
        ScrollPlatform::IOS,
        ScrollPlatform::Linux,
        ScrollPlatform::MacOS,
        ScrollPlatform::Windows,
    ];

    // -- Three copies of one function, already out of step --------------------------

    // -- Two English localisations, and where they disagree --------------------------

    #[test]
    fn the_delegate_pads_a_picker_hour_and_the_default_class_does_not() {
        // The difference an application sees the moment it installs
        // `GlobalCupertinoLocalizations`, and the reason
        // `cupertino_pickers` reads the delegate's strings: a date picker
        // shows `01` under one and `1` under the other.
        assert_eq!(DefaultCupertinoLocalizations::date_picker_hour(1), "1");
        assert_eq!(CupertinoLocalizationEn::date_picker_hour(1), "01");
        assert_eq!(CupertinoLocalizationEn::date_picker_hour(12), "12");

        // A countdown's hours are padded and its minutes and seconds are not,
        // in the delegate; the default class pads none of the three.
        assert_eq!(CupertinoLocalizationEn::timer_picker_hour(5), "05");
        assert_eq!(CupertinoLocalizationEn::timer_picker_minute(5), "5");
        assert_eq!(CupertinoLocalizationEn::timer_picker_second(5), "5");
        assert_eq!(DefaultCupertinoLocalizations::timer_picker_hour(5), "5");
    }

    #[test]
    fn the_delegates_medium_date_has_a_comma_and_the_default_classs_has_padding() {
        // `intl.DateFormat.MMMEd` against a string written out by hand: the
        // one place the two Englishes differ by punctuation rather than by
        // width.
        assert_eq!(
            CupertinoLocalizationEn::date_picker_medium_date(4, 8, 27),
            "Thu, Aug 27"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_medium_date(4, 8, 27),
            "Thu Aug 27"
        );
        // And the default class's `padRight(2)`, which is what keeps its own
        // column from shuffling: a one-digit day carries a trailing space.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_medium_date(4, 8, 1),
            "Thu Aug 1 "
        );
    }

    #[test]
    fn a_spinners_number_is_said_as_a_quantity_not_as_a_digit() {
        // "3" alone is the same utterance in a column of hours and a column
        // of minutes. The label is what tells them apart.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_hour_semantics_label(3),
            "3 o'clock"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(3),
            "3 minutes"
        );
    }

    #[test]
    fn the_minute_inflects_and_the_hour_does_not() {
        // The minute is the only one in this group that changes shape. The
        // hour keeps "o'clock" at every value -- the generated English class
        // fills both plural categories with the same string, which is the
        // same shape as the timer picker's one-entry label lists.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(1),
            "1 minute"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(2),
            "2 minutes"
        );
        // Zero takes the plural, which is English's rule and not every
        // language's.
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(0),
            "0 minutes"
        );
        for hour in [1, 2, 12, 23] {
            assert!(
                DefaultCupertinoLocalizations::date_picker_hour_semantics_label(hour)
                    .ends_with("o'clock"),
                "hour {hour} keeps its shape"
            );
        }
    }

    #[test]
    fn a_tab_is_counted_from_one_because_it_is_said_out_loud() {
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(1, 3).as_deref(),
            Some("Tab 1 of 3")
        );
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(3, 3).as_deref(),
            Some("Tab 3 of 3")
        );
        // Upstream's two asserts. A zero index means a caller forgot the
        // `+ 1`; answering None is this port's usual choice over taking the
        // application down in front of somebody using a screen reader.
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(0, 3),
            None
        );
        assert_eq!(
            DefaultCupertinoLocalizations::tab_semantics_label(1, 0),
            None
        );
    }

    #[test]
    fn only_the_hour_label_of_a_countdown_inflects() {
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_hour_label(1),
            "hour"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_hour_label(2),
            "hours"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_hour_label(0),
            "hours"
        );
        // The other two are abbreviations with a full stop, and do not.
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_minute_label(1),
            "min."
        );
        assert_eq!(
            DefaultCupertinoLocalizations::timer_picker_second_label(1),
            "sec."
        );
    }

    #[test]
    fn the_label_lists_cover_every_form_the_labels_can_take() {
        // The invariant the picker's column width rests on. Upstream cannot
        // check it -- the list and the function are separate overrides in
        // every locale class -- and this port had the pair written twice with
        // nothing between them, so renaming a label would have left the
        // column measured for the old word.
        assert!(DefaultCupertinoLocalizations::labels_cover_every_form(99));
        assert!(CupertinoLocalizationEn::TIMER_PICKER_HOUR_LABELS.len() == 2);
    }

    #[test]
    fn the_singular_lists_are_a_fact_about_english_not_an_omission() {
        // English minutes and seconds are abbreviations that do not inflect,
        // so one entry each is the whole truth; hours inflect, so two. A
        // language whose hour had six forms would list six and the column
        // would be sized for the longest.
        assert_eq!(
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS,
            ["hour", "hours"]
        );
        assert_eq!(
            DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS,
            ["min."]
        );
        assert_eq!(
            DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS,
            ["sec."]
        );
        // The generated English class forwards rather than repeating them.
        assert_eq!(
            CupertinoLocalizationEn::TIMER_PICKER_HOUR_LABELS,
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
        );
    }

    #[test]
    fn a_horizontal_list_gets_a_scrollbar_here_and_none_in_a_material_app() {
        // Material's implementation opens with an axis check; this one and the
        // base class have none.
        assert!(CupertinoScrollBehavior::builds_scrollbar(
            ScrollPlatform::MacOS
        ));
        assert!(!MaterialScrollBehavior::builds_scrollbar(
            ScrollAxis::Horizontal,
            ScrollPlatform::MacOS
        ));
    }

    #[test]
    fn on_the_vertical_axis_the_two_agree_platform_for_platform() {
        for platform in ALL {
            assert_eq!(
                CupertinoScrollBehavior::builds_scrollbar(platform),
                MaterialScrollBehavior::builds_scrollbar(ScrollAxis::Vertical, platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn only_the_desktops_get_a_bar_in_either_design() {
        for platform in ALL {
            assert_eq!(
                CupertinoScrollBehavior::builds_scrollbar(platform),
                platform.is_desktop(),
                "{platform:?}"
            );
        }
    }

    // -- The guess from tick 89, confirmed and widened -------------------------------

    #[test]
    fn nowhere_gets_an_overscroll_indicator_because_everywhere_bounces() {
        for platform in ALL {
            assert!(!CupertinoScrollBehavior::overscroll_indicator(platform));
            assert!(
                CupertinoScrollBehavior::bounces(platform),
                "{platform:?} bounces, so it needs none"
            );
        }
    }

    #[test]
    fn android_is_the_case_that_shows_the_rule_is_about_physics_not_platform() {
        use crate::material_app::OverscrollDecoration;
        // The same platform, decorated in a Material app and bare in a Cupertino
        // one, because only one of the two gives it bouncing physics.
        assert_eq!(
            MaterialScrollBehavior::overscroll_indicator(ScrollPlatform::Android, true),
            OverscrollDecoration::Stretching
        );
        assert!(!CupertinoScrollBehavior::overscroll_indicator(
            ScrollPlatform::Android
        ));
    }

    // -- The trackpad ----------------------------------------------------------------

    #[test]
    fn macos_alone_gives_up_faster() {
        for platform in ALL {
            let expected = if platform == ScrollPlatform::MacOS {
                ScrollDecelerationRate::Fast
            } else {
                ScrollDecelerationRate::Normal
            };
            assert_eq!(
                CupertinoScrollBehavior::deceleration_rate(platform),
                expected,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn several_fingers_are_averaged_rather_than_the_last_one_winning() {
        assert_eq!(
            CupertinoScrollBehavior::multitouch_drag_strategy(),
            MultitouchDragStrategy::AverageBoundaryPointers
        );
    }

    // -- The application ---------------------------------------------------------------

    #[test]
    fn the_router_needs_at_least_one_of_its_two_configurations() {
        let mut app = CupertinoApp::new();
        assert!(!app.router_is_configured());
        app.router.has_router_config = true;
        assert!(app.router_is_configured());
    }

    #[test]
    fn there_is_no_cupertino_grid_overlay_to_match_the_material_one() {
        assert!(!CupertinoApp::has_a_design_grid_overlay());
    }

    // -- The same list, one of them explained wrongly ---------------------------------

    #[test]
    fn the_weekday_lists_of_the_two_defaults_are_identical() {
        use crate::material_app::DefaultMaterialLocalizations;
        assert_eq!(
            DefaultCupertinoLocalizations::SHORT_WEEKDAYS,
            DefaultMaterialLocalizations::SHORT_WEEKDAYS
        );
    }

    #[test]
    fn and_are_indexed_the_same_way_from_the_same_constant() {
        use crate::material_app::DefaultMaterialLocalizations;
        for weekday in 1..=7u32 {
            let cupertino =
                DefaultCupertinoLocalizations::date_picker_day_of_month(1, Some(weekday));
            assert!(
                cupertino.contains(DefaultMaterialLocalizations::short_weekday(weekday)),
                "weekday {weekday}"
            );
        }
    }

    #[test]
    fn sunday_lands_where_it_should_in_the_file_that_says_nothing_about_it() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_day_of_month(9, Some(7)),
            " Sun 9 "
        );
    }

    #[test]
    fn the_string_carries_its_own_padding() {
        // A leading and trailing space baked into the localisation, for the
        // spinner it lands in.
        let padded = DefaultCupertinoLocalizations::date_picker_day_of_month(1, Some(1));
        assert!(padded.starts_with(' '));
        assert!(padded.ends_with(' '));
        assert_eq!(padded.trim(), "Mon 1");

        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_day_of_month(1, None),
            "1",
            "and the form without a weekday does not"
        );
    }

    #[test]
    fn the_months_are_one_based_like_the_datetime_constants_that_feed_them() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_month(1),
            "January"
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_month(12),
            "December"
        );
    }

    #[test]
    fn the_year_is_not_padded_here_where_the_material_compact_date_pads_to_four() {
        use crate::material_app::DefaultMaterialLocalizations;
        assert_eq!(DefaultCupertinoLocalizations::date_picker_year(7), "7");
        assert_eq!(
            DefaultMaterialLocalizations::format_compact_date(7, 1, 2).as_deref(),
            Some("01/02/0007")
        );
    }

    #[test]
    fn a_medium_date_names_both_the_day_and_the_month() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_medium_date(3, 9, 27),
            "Wed Sep 27",
            "the example from the doc comment"
        );
    }

    #[test]
    fn localizations_are_fetched_through_a_check_rather_than_a_null() {
        assert!(DefaultCupertinoLocalizations::of(true).is_some());
        assert!(DefaultCupertinoLocalizations::of(false).is_none());
    }
}

#[cfg(test)]
mod date_order_tests {
    use super::{
        DatePickerColumn, DatePickerDateOrder, DatePickerDateTimeOrder, DateTimeColumn,
        DefaultCupertinoLocalizations,
    };

    #[test]
    fn every_order_shows_each_column_exactly_once() {
        for order in DatePickerDateOrder::ALL {
            let columns = order.columns();
            for wanted in [
                DatePickerColumn::Day,
                DatePickerColumn::Month,
                DatePickerColumn::Year,
            ] {
                assert_eq!(
                    columns.iter().filter(|c| **c == wanted).count(),
                    1,
                    "{order:?} {wanted:?}"
                );
            }
        }
        // And the four are four different arrangements.
        let mut seen: Vec<[DatePickerColumn; 3]> = DatePickerDateOrder::ALL
            .iter()
            .map(|o| o.columns())
            .collect();
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn a_month_year_picker_is_the_same_order_with_the_day_struck_out() {
        for order in DatePickerDateOrder::ALL {
            let expected: Vec<DatePickerColumn> = order
                .columns()
                .into_iter()
                .filter(|c| *c != DatePickerColumn::Day)
                .collect();
            assert_eq!(order.month_year_columns().to_vec(), expected, "{order:?}");
        }
    }

    #[test]
    fn and_that_is_why_two_pairs_of_orders_collapse() {
        // Upstream's second switch pairs mdy with dmy and ymd with ydm. The
        // pairing is a consequence of removing the day, not two lists that
        // happen to agree.
        assert_eq!(
            DatePickerDateOrder::Mdy.month_year_columns(),
            DatePickerDateOrder::Dmy.month_year_columns()
        );
        assert_eq!(
            DatePickerDateOrder::Ymd.month_year_columns(),
            DatePickerDateOrder::Ydm.month_year_columns()
        );
        // The two pairs are still different from each other, or the collapse
        // would have flattened everything.
        assert_ne!(
            DatePickerDateOrder::Mdy.month_year_columns(),
            DatePickerDateOrder::Ymd.month_year_columns()
        );
        assert_eq!(
            DatePickerDateOrder::Mdy.month_year_columns(),
            [DatePickerColumn::Month, DatePickerColumn::Year]
        );
        assert_eq!(
            DatePickerDateOrder::Ymd.month_year_columns(),
            [DatePickerColumn::Year, DatePickerColumn::Month]
        );
        // And the full orders they came from were not equal to begin with.
        assert_ne!(
            DatePickerDateOrder::Mdy.columns(),
            DatePickerDateOrder::Dmy.columns()
        );
    }

    #[test]
    fn the_hour_always_sits_immediately_before_the_minute() {
        // Upstream names four of the twenty-four arrangements. What it leaves
        // out is the rule.
        for order in DatePickerDateTimeOrder::ALL {
            let columns = order.columns();
            let hour = columns
                .iter()
                .position(|c| *c == DateTimeColumn::Hour)
                .expect("an hour");
            let minute = columns
                .iter()
                .position(|c| *c == DateTimeColumn::Minute)
                .expect("a minute");
            assert_eq!(minute, hour + 1, "{order:?}");
        }
    }

    #[test]
    fn and_the_date_is_always_at_one_end() {
        for order in DatePickerDateTimeOrder::ALL {
            let columns = order.columns();
            let date = columns
                .iter()
                .position(|c| *c == DateTimeColumn::Date)
                .expect("a date");
            assert!(date == 0 || date == columns.len() - 1, "{order:?} {date}");
            assert_eq!(order.date_comes_first(), date == 0, "{order:?}");
        }
        // Both ends are actually used, so the rule is not vacuous.
        assert!(DatePickerDateTimeOrder::DateTimeDayPeriod.date_comes_first());
        assert!(!DatePickerDateTimeOrder::TimeDayPeriodDate.date_comes_first());
    }

    #[test]
    fn and_every_date_time_order_shows_four_distinct_columns() {
        for order in DatePickerDateTimeOrder::ALL {
            let columns = order.columns();
            for wanted in [
                DateTimeColumn::Date,
                DateTimeColumn::Hour,
                DateTimeColumn::Minute,
                DateTimeColumn::DayPeriod,
            ] {
                assert_eq!(
                    columns.iter().filter(|c| **c == wanted).count(),
                    1,
                    "{order:?} {wanted:?}"
                );
            }
        }
    }

    #[test]
    fn us_english_puts_the_month_first_and_the_marker_last() {
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_date_order(),
            DatePickerDateOrder::Mdy
        );
        assert_eq!(
            DefaultCupertinoLocalizations::date_picker_date_time_order(),
            DatePickerDateTimeOrder::DateTimeDayPeriod
        );
        assert_eq!(DatePickerDateOrder::default(), DatePickerDateOrder::Mdy);
        assert_eq!(
            DatePickerDateTimeOrder::default(),
            DatePickerDateTimeOrder::DateTimeDayPeriod
        );
    }
}

#[cfg(test)]
mod bundle_tests {
    use super::{
        CupertinoLocalizationEn, CupertinoLocalizations, DatePickerDateOrder,
        DatePickerDateTimeOrder, DefaultCupertinoLocalizations,
    };

    /// An application's own bundle, which is the whole reason the interface
    /// exists. It answers for two strings and defers for the rest -- every
    /// member still has to be written out, as upstream's `implements` forces
    /// too, except the six the abstract class gives bodies to.
    struct Loud;

    impl CupertinoLocalizations for Loud {
        fn date_picker_year(&self, year_index: i32) -> String {
            DefaultCupertinoLocalizations::date_picker_year(year_index)
        }
        fn date_picker_month(&self, month_index: usize) -> &str {
            DefaultCupertinoLocalizations::date_picker_month(month_index)
        }
        fn date_picker_standalone_month(&self, month_index: usize) -> &str {
            DefaultCupertinoLocalizations::date_picker_standalone_month(month_index)
        }
        fn date_picker_day_of_month(&self, day_index: u32, week_day: Option<u32>) -> String {
            DefaultCupertinoLocalizations::date_picker_day_of_month(day_index, week_day)
        }
        fn date_picker_medium_date(&self, week_day: u32, month: usize, day: u32) -> String {
            DefaultCupertinoLocalizations::date_picker_medium_date(week_day, month, day)
        }
        fn date_picker_hour(&self, hour: u32) -> String {
            DefaultCupertinoLocalizations::date_picker_hour(hour)
        }
        fn date_picker_hour_semantics_label(&self, hour: u32) -> String {
            DefaultCupertinoLocalizations::date_picker_hour_semantics_label(hour)
        }
        fn date_picker_minute(&self, minute: u32) -> String {
            DefaultCupertinoLocalizations::date_picker_minute(minute)
        }
        fn date_picker_minute_semantics_label(&self, minute: u32) -> String {
            DefaultCupertinoLocalizations::date_picker_minute_semantics_label(minute)
        }
        fn date_picker_date_order(&self) -> DatePickerDateOrder {
            DatePickerDateOrder::Ymd
        }
        fn date_picker_date_time_order(&self) -> DatePickerDateTimeOrder {
            DefaultCupertinoLocalizations::date_picker_date_time_order()
        }
        fn ante_meridiem_abbreviation(&self) -> &str {
            DefaultCupertinoLocalizations::ANTE_MERIDIEM_ABBREVIATION
        }
        fn post_meridiem_abbreviation(&self) -> &str {
            DefaultCupertinoLocalizations::POST_MERIDIEM_ABBREVIATION
        }
        fn today_label(&self) -> &str {
            DefaultCupertinoLocalizations::TODAY_LABEL
        }
        fn alert_dialog_label(&self) -> &str {
            DefaultCupertinoLocalizations::ALERT_DIALOG_LABEL
        }
        fn tab_semantics_label(&self, tab_index: u32, tab_count: u32) -> Option<String> {
            DefaultCupertinoLocalizations::tab_semantics_label(tab_index, tab_count)
        }
        fn timer_picker_hour(&self, hour: u32) -> String {
            DefaultCupertinoLocalizations::timer_picker_hour(hour)
        }
        fn timer_picker_minute(&self, minute: u32) -> String {
            DefaultCupertinoLocalizations::timer_picker_minute(minute)
        }
        fn timer_picker_second(&self, second: u32) -> String {
            DefaultCupertinoLocalizations::timer_picker_second(second)
        }
        fn timer_picker_hour_label(&self, hour: u32) -> &str {
            DefaultCupertinoLocalizations::timer_picker_hour_label(hour)
        }
        fn timer_picker_hour_labels(&self) -> &[&'static str] {
            &DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
        }
        fn timer_picker_minute_label(&self, minute: u32) -> &str {
            DefaultCupertinoLocalizations::timer_picker_minute_label(minute)
        }
        fn timer_picker_minute_labels(&self) -> &[&'static str] {
            &DefaultCupertinoLocalizations::TIMER_PICKER_MINUTE_LABELS
        }
        fn timer_picker_second_label(&self, second: u32) -> &str {
            DefaultCupertinoLocalizations::timer_picker_second_label(second)
        }
        fn timer_picker_second_labels(&self) -> &[&'static str] {
            &DefaultCupertinoLocalizations::TIMER_PICKER_SECOND_LABELS
        }
        fn cut_button_label(&self) -> &str {
            "CUT"
        }
        fn copy_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::COPY_BUTTON_LABEL
        }
        fn paste_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::PASTE_BUTTON_LABEL
        }
        fn clear_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::CLEAR_BUTTON_LABEL
        }
        fn no_spell_check_replacements_label(&self) -> &str {
            DefaultCupertinoLocalizations::NO_SPELL_CHECK_REPLACEMENTS_LABEL
        }
        fn select_all_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::SELECT_ALL_BUTTON_LABEL
        }
        fn look_up_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::LOOK_UP_BUTTON_LABEL
        }
        fn search_web_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::SEARCH_WEB_BUTTON_LABEL
        }
        fn share_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::SHARE_BUTTON_LABEL
        }
        fn search_text_field_placeholder_label(&self) -> &str {
            DefaultCupertinoLocalizations::SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL
        }
        fn modal_barrier_dismiss_label(&self) -> &str {
            DefaultCupertinoLocalizations::MODAL_BARRIER_DISMISS_LABEL
        }
        fn menu_dismiss_label(&self) -> &str {
            DefaultCupertinoLocalizations::MENU_DISMISS_LABEL
        }
        fn cancel_button_label(&self) -> &str {
            DefaultCupertinoLocalizations::CANCEL_BUTTON_LABEL
        }
        fn back_button_label(&self) -> &str {
            "BACK"
        }
    }

    #[test]
    fn an_application_can_put_its_own_bundle_in_front_of_the_frameworks() {
        // The point of the interface, and what an empty struct with an `of` on
        // it could not do: the strings were constants on the implementation,
        // so there was nothing for an application to stand in front of.
        let framework: &dyn CupertinoLocalizations = &DefaultCupertinoLocalizations;
        let theirs: &dyn CupertinoLocalizations = &Loud;
        assert_eq!(framework.back_button_label(), "Back");
        assert_eq!(theirs.back_button_label(), "BACK");
        assert_eq!(framework.cut_button_label(), "Cut");
        assert_eq!(theirs.cut_button_label(), "CUT");
    }

    #[test]
    fn a_bundle_decides_the_order_the_date_columns_run_in() {
        // Not only words. `datePickerDateOrder` is a locale's answer too, and
        // it is what a date picker lays its columns out by -- so a bundle in
        // front of the framework's moves the columns, not just their labels.
        let framework: &dyn CupertinoLocalizations = &DefaultCupertinoLocalizations;
        let theirs: &dyn CupertinoLocalizations = &Loud;
        assert_eq!(framework.date_picker_date_order(), DatePickerDateOrder::Mdy);
        assert_eq!(theirs.date_picker_date_order(), DatePickerDateOrder::Ymd);
        assert_eq!(
            theirs.date_picker_date_order().columns(),
            [
                super::DatePickerColumn::Year,
                super::DatePickerColumn::Month,
                super::DatePickerColumn::Day
            ]
        );
    }

    #[test]
    fn the_default_bundle_answers_with_its_own_constants() {
        // The constants are the values and the trait is the interface; a
        // caller wanting the framework's English can still name either, and
        // the two must not drift.
        let bundle: &dyn CupertinoLocalizations = &DefaultCupertinoLocalizations;
        assert_eq!(
            bundle.search_text_field_placeholder_label(),
            DefaultCupertinoLocalizations::SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL
        );
        assert_eq!(
            bundle.today_label(),
            DefaultCupertinoLocalizations::TODAY_LABEL
        );
        assert_eq!(
            bundle.timer_picker_hour_labels(),
            DefaultCupertinoLocalizations::TIMER_PICKER_HOUR_LABELS
        );
    }

    #[test]
    fn the_four_strings_it_shares_a_name_with_material_are_its_own() {
        // Four names out of forty-odd also exist on `MaterialLocalizations`,
        // and four is a coincidence of English rather than a shared interface:
        // upstream declares both classes independently, and a locale may word
        // one of these differently in a Cupertino alert than in a Material
        // dialog. They had never been written down on this side at all.
        let bundle: &dyn CupertinoLocalizations = &DefaultCupertinoLocalizations;
        assert_eq!(bundle.alert_dialog_label(), "Alert");
        assert_eq!(bundle.modal_barrier_dismiss_label(), "Dismiss");
        assert_eq!(bundle.menu_dismiss_label(), "Dismiss menu");
        assert_eq!(bundle.cancel_button_label(), "Cancel");
    }

    #[test]
    fn the_hints_an_abstract_class_answers_for_itself_are_answered_here_too() {
        // Six of upstream's members have bodies on the abstract class, so a
        // bundle that says nothing about them still answers. This one says
        // nothing about them -- `Loud` never mentions them -- and the answers
        // are upstream's own defaults.
        let theirs: &dyn CupertinoLocalizations = &Loud;
        assert_eq!(
            theirs.expansion_tile_expanded_hint(),
            "double tap to collapse"
        );
        assert_eq!(
            theirs.expansion_tile_collapsed_hint(),
            "double tap to expand"
        );
        assert_eq!(theirs.expansion_tile_expanded_tap_hint(), "Collapse");
        assert_eq!(
            theirs.expansion_tile_collapsed_tap_hint(),
            "Expand for more details"
        );
        // And the pair that read backwards on purpose: what a screen reader
        // says *about* the thing, announced from the other side.
        assert_eq!(theirs.expanded_hint(), "Collapsed");
        assert_eq!(theirs.collapsed_hint(), "Expanded");
    }

    #[test]
    fn the_two_bundles_agree_on_words_and_differ_on_numbers() {
        // Why there are two of these at all. `DefaultCupertinoLocalizations`
        // is what an application gets with **no** delegates installed;
        // `CupertinoLocalizationEn` is what every real one runs, and the
        // gallery installs it.
        //
        // The words are the same in English -- upstream generates one set from
        // `cupertino_en.arb` and writes the other by hand, and they agree
        // because English is English. Stating it here is what keeps it a
        // checked fact rather than an assumption: a locale is free to make
        // them differ, and this test is what would notice if one of the two
        // were edited alone.
        let framework: &dyn CupertinoLocalizations = &DefaultCupertinoLocalizations;
        let global: &dyn CupertinoLocalizations = &CupertinoLocalizationEn;
        for (what, one, other) in [
            (
                "alert",
                framework.alert_dialog_label(),
                global.alert_dialog_label(),
            ),
            (
                "back",
                framework.back_button_label(),
                global.back_button_label(),
            ),
            (
                "cancel",
                framework.cancel_button_label(),
                global.cancel_button_label(),
            ),
            (
                "cut",
                framework.cut_button_label(),
                global.cut_button_label(),
            ),
            (
                "copy",
                framework.copy_button_label(),
                global.copy_button_label(),
            ),
            (
                "paste",
                framework.paste_button_label(),
                global.paste_button_label(),
            ),
            (
                "clear",
                framework.clear_button_label(),
                global.clear_button_label(),
            ),
            (
                "look up",
                framework.look_up_button_label(),
                global.look_up_button_label(),
            ),
            (
                "share",
                framework.share_button_label(),
                global.share_button_label(),
            ),
            (
                "search web",
                framework.search_web_button_label(),
                global.search_web_button_label(),
            ),
            (
                "select all",
                framework.select_all_button_label(),
                global.select_all_button_label(),
            ),
            (
                "placeholder",
                framework.search_text_field_placeholder_label(),
                global.search_text_field_placeholder_label(),
            ),
            (
                "no replacements",
                framework.no_spell_check_replacements_label(),
                global.no_spell_check_replacements_label(),
            ),
            (
                "dismiss",
                framework.modal_barrier_dismiss_label(),
                global.modal_barrier_dismiss_label(),
            ),
            (
                "dismiss menu",
                framework.menu_dismiss_label(),
                global.menu_dismiss_label(),
            ),
            ("today", framework.today_label(), global.today_label()),
            (
                "am",
                framework.ante_meridiem_abbreviation(),
                global.ante_meridiem_abbreviation(),
            ),
            (
                "pm",
                framework.post_meridiem_abbreviation(),
                global.post_meridiem_abbreviation(),
            ),
            (
                "expanded",
                framework.expanded_hint(),
                global.expanded_hint(),
            ),
            (
                "collapsed",
                framework.collapsed_hint(),
                global.collapsed_hint(),
            ),
        ] {
            assert_eq!(one, other, "the two bundles say the same {what} in English");
        }

        // And the numbers, which are the reason to install the delegate at
        // all: an hour is padded under the global bundle and bare under the
        // framework's, so a date picker reads `01` on one and `1` on the other.
        assert_eq!(framework.date_picker_hour(1), "1");
        assert_eq!(global.date_picker_hour(1), "01");
        assert_eq!(framework.date_picker_minute(5), "05");
        assert_eq!(global.date_picker_minute(5), "05");
        assert_eq!(framework.timer_picker_hour(3), "3");
        assert_eq!(global.timer_picker_hour(3), "03");
    }

    #[test]
    fn the_global_bundle_speaks_the_semantics_labels_too() {
        // The three the generated class supplies as raw templates --
        // r"$hour o'clock", '1 minute' / r'$minute minutes', and
        // r'Tab $tabIndex of $tabCount' -- which `GlobalCupertinoLocalizations`
        // fills in. They had never been ported to this bundle: anything
        // wanting them had to reach across to the framework's, which is a
        // different locale's answer wearing this one's name.
        let global: &dyn CupertinoLocalizations = &CupertinoLocalizationEn;
        assert_eq!(global.date_picker_hour_semantics_label(4), "4 o'clock");
        assert_eq!(
            global.date_picker_minute_semantics_label(1),
            "1 minute",
            "the `one` arm, which English does spell differently"
        );
        assert_eq!(global.date_picker_minute_semantics_label(2), "2 minutes");
        assert_eq!(
            global.tab_semantics_label(2, 5),
            Some("Tab 2 of 5".to_string())
        );
        assert_eq!(
            global.tab_semantics_label(0, 5),
            None,
            "a tab index nobody can speak is not spoken"
        );
    }

    #[test]
    fn either_bundle_can_stand_where_the_interface_is_asked_for() {
        // The point of last tick's trait, now with two real implementations
        // behind it rather than one and a test double.
        fn placeholder(bundle: &dyn CupertinoLocalizations) -> String {
            bundle.search_text_field_placeholder_label().to_string()
        }
        assert_eq!(placeholder(&DefaultCupertinoLocalizations), "Search");
        assert_eq!(placeholder(&CupertinoLocalizationEn), "Search");

        // The three label lists, which a picker sizes its columns by: each is
        // its own, and handing back a neighbour's would size the minutes
        // column for the word "hours".
        let global: &dyn CupertinoLocalizations = &CupertinoLocalizationEn;
        assert_eq!(global.timer_picker_hour_labels(), ["hour", "hours"]);
        assert_eq!(global.timer_picker_minute_labels(), ["min."]);
        assert_eq!(global.timer_picker_second_labels(), ["sec."]);

        // And the column order, which is a bundle's answer and not a word.
        let both = [
            DefaultCupertinoLocalizations.date_picker_date_order(),
            CupertinoLocalizationEn.date_picker_date_order(),
        ];
        assert_eq!(
            both,
            [DatePickerDateOrder::Mdy, DatePickerDateOrder::Mdy],
            "`mdy` in English, from both -- the generated one says so in \
             `datePickerDateOrderString`"
        );
    }
}

#[cfg(test)]
mod app_rules_tests {
    use super::{CupertinoApp, DefaultCupertinoLocalizationsDelegate};
    use crate::cupertino_theme::CupertinoThemeData;
    use crate::localizations::LocalizationsDelegate;
    use crate::platform::Locale;
    use crate::prelude::Brightness;

    #[test]
    fn the_brightness_question_is_the_themes_to_answer() {
        // Upstream's build reads `effectiveThemeData.brightness ??
        // MediaQuery.platformBrightnessOf(context)`, which is
        // `CupertinoTheme.brightnessOf` -- and that is already ported. This
        // says so rather than restating it: a second copy under a second name
        // is two rules that can drift, and the first draft of this tick wrote
        // one before noticing.
        let unstated = CupertinoThemeData::new();
        assert_eq!(unstated.brightness(), None, "nothing stated");
        assert_eq!(
            unstated.brightness_of(Brightness::Dark),
            Brightness::Dark,
            "so the platform answers"
        );
    }

    #[test]
    fn a_dark_app_asks_for_light_status_bar_icons() {
        // `brightness == Brightness.dark ? SystemUiOverlayStyle.light :
        // SystemUiOverlayStyle.dark` -- **the opposite one**. The name says
        // what the icons are, not what is behind them, and reading the pair as
        // "dark app, dark style" would leave black icons on a black bar.
        use crate::services::system::SystemUiOverlayStyle;
        assert_eq!(
            CupertinoApp::overlay_style(Brightness::Dark),
            SystemUiOverlayStyle::LIGHT,
            "a dark app is topped by the light style"
        );
        assert_eq!(
            CupertinoApp::overlay_style(Brightness::Light),
            SystemUiOverlayStyle::DARK
        );

        // And the inversion said a second time, one level down: the style's
        // own `statusBarBrightness` describes the **background** it expects,
        // so the light style -- light icons -- expects a dark bar behind them.
        // A test that read this field as "which style is it" would have the
        // whole thing backwards and still pass.
        assert_eq!(
            SystemUiOverlayStyle::LIGHT.status_bar_brightness,
            Some(Brightness::Dark)
        );
        assert_eq!(
            SystemUiOverlayStyle::DARK.status_bar_brightness,
            Some(Brightness::Light)
        );
    }

    #[test]
    fn an_app_that_named_no_colour_is_known_by_its_theme_s() {
        // `widget.color ?? effectiveThemeData.primaryColor`. It is the colour
        // handed to the operating system for the task switcher, so falling
        // back to the theme rather than to a constant is what keeps a themed
        // application recognisable as itself.
        let theme = CupertinoThemeData::with_primary_color(0xFF11_2233);
        assert_eq!(CupertinoApp::app_color(None, &theme), 0xFF11_2233);
        assert_eq!(
            CupertinoApp::app_color(Some(0xFF44_5566), &theme),
            0xFF44_5566,
            "and an app that named one keeps it"
        );
    }

    #[test]
    fn the_cursor_is_the_primary_colour_and_the_selection_is_a_fifth_of_it() {
        // One colour and one number: a theme changing its primary colour moves
        // both, which is why upstream writes them as two lines of the same
        // `DefaultSelectionStyle`.
        let theme = CupertinoThemeData::with_primary_color(0xFF00_7AFF);
        let (selection, cursor) = CupertinoApp::selection_style(&theme);
        assert_eq!(cursor.0, 0xFF00_7AFF, "the cursor is the colour itself");
        assert_eq!(
            selection.0 & 0x00FF_FFFF,
            0x0000_7AFF,
            "and the selection is the same colour"
        );
        assert!(
            selection.alpha() < cursor.alpha(),
            "at a fifth of the opacity: {:?} against {:?}",
            selection.alpha(),
            cursor.alpha()
        );
        assert_eq!(CupertinoApp::SELECTION_OPACITY, 0.2);
    }

    /// An application's own bundle for the same resource type, which is the
    /// case the order exists for.
    struct TheirOwn;

    impl LocalizationsDelegate for TheirOwn {
        fn resource_type(&self) -> &'static str {
            "CupertinoLocalizations"
        }
        fn is_supported(&self, _locale: &Locale) -> bool {
            true
        }
        fn load(&self, _locale: &Locale) -> crate::localizations::LoadedResources {
            crate::localizations::LoadedResources::synchronous(
                "CupertinoLocalizations",
                "their own",
            )
        }
    }

    #[test]
    fn an_applications_own_delegates_come_first() {
        // Upstream's comment, and the whole of an application's ability to
        // replace the framework's strings: *"Only the first delegate of a
        // particular LocalizationsDelegate.type is loaded so the
        // localizationsDelegate parameter can be used to override
        // _CupertinoLocalizationsDelegate."*
        //
        // So the framework's is **appended**, not prepended. Prepending it
        // would still compile, still load, and quietly make the parameter
        // useless.
        let delegates = CupertinoApp::localizations_delegates(vec![std::rc::Rc::new(TheirOwn)]);
        assert_eq!(delegates.len(), 2);
        assert_eq!(
            delegates[0].load(&Locale::new("en")).value,
            "their own",
            "the application's own is asked first"
        );
        assert_eq!(delegates[1].load(&Locale::new("en")).value, "en_US");

        // And an application that supplied none gets the framework's alone.
        let bare = CupertinoApp::localizations_delegates(Vec::new());
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].resource_type(), "CupertinoLocalizations");
    }

    #[test]
    fn the_frameworks_cupertino_delegate_answers_for_english_only() {
        // `_CupertinoLocalizationsDelegate.isSupported` is
        // `locale.languageCode == 'en'`: the default bundle is US English and
        // says so, rather than claiming every locale and answering in English
        // anyway.
        let delegate = DefaultCupertinoLocalizationsDelegate;
        assert!(delegate.is_supported(&Locale::new("en")));
        assert!(
            delegate.is_supported(&Locale {
                country_code: Some("GB".to_string()),
                ..Locale::new("en")
            }),
            "a language, not a locale -- `en_GB` is still English"
        );
        assert!(!delegate.is_supported(&Locale::new("fr")));
        assert!(!delegate.should_reload(&DefaultCupertinoLocalizationsDelegate));
    }

    #[test]
    fn a_cupertino_app_forwards_all_five_router_parameters() {
        // `CupertinoApp.router` carries the same single assert `MaterialApp
        // .router` does -- `routerDelegate != null || routerConfig != null` --
        // and hands all five parameters to `WidgetsApp.router`, which carries
        // the other three. This port used to model two of the five, so the
        // parameters those three asserts are *about* had nowhere to live.
        use crate::widgets_app::RouterConfiguration;
        let app = CupertinoApp {
            router: RouterConfiguration {
                has_router_delegate: true,
                has_route_information_provider: true,
                ..RouterConfiguration::default()
            },
            ..CupertinoApp::new()
        };
        assert!(
            app.router_is_configured(),
            "its own assert is satisfied: there is a delegate"
        );
        assert!(
            app.router.validate().is_err(),
            "and the widgets app refuses it: a provider with no parser"
        );

        let sound = CupertinoApp {
            router: RouterConfiguration {
                has_router_delegate: true,
                has_route_information_provider: true,
                has_route_information_parser: true,
                ..RouterConfiguration::default()
            },
            ..CupertinoApp::new()
        };
        assert_eq!(sound.router.validate(), Ok(()));
    }
}
