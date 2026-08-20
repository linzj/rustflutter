//! Six small widgets, one per upstream file: `image_filter.dart`,
//! `grid_paper.dart`, `keyboard_listener.dart`, `navigation_toolbar.dart`,
//! `shared_app_data.dart` and `spell_check.dart`.
//!
//! They share no subject. What they do share is being **one class each**,
//! which is usually a sign the class is a single decision written down --
//! and each of these has one worth naming.

use crate::engine::Color;

/// Upstream `ImageFiltered`: applies a filter to everything its child paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageFiltered {
    /// Which filter, by identity.
    pub image_filter: u64,
    /// Upstream's `enabled`, true by default.
    pub enabled: bool,
}

impl ImageFiltered {
    pub fn new(image_filter: u64) -> ImageFiltered {
        ImageFiltered {
            image_filter,
            enabled: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's doc gives the advice that is the reason `enabled` exists:
    /// "prefer setting enabled to `false` instead of creating a no-op filter".
    ///
    /// A no-op filter is not free. The child still gets rasterised into a
    /// layer and pushed through the filter, and a blur of radius zero costs
    /// nearly what a blur of radius ten does. `enabled: false` skips the
    /// layer entirely, which is what a caller animating a filter *in* wants
    /// for every frame before it starts.
    pub fn creates_layer(&self) -> bool {
        self.enabled
    }
}

/// Upstream `GridPaper`: graph-paper lines over a child, for lining designs up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridPaper {
    pub color: Color,
    /// The distance between the **primary** lines.
    pub interval: f32,
    /// Upstream's `divisions`: major lines within each primary cell.
    pub divisions: u32,
    /// Upstream's `subdivisions`: minor lines within each division.
    pub subdivisions: u32,
}

impl Default for GridPaper {
    fn default() -> GridPaper {
        GridPaper::new()
    }
}

impl GridPaper {
    /// Upstream's default colour: a pale translucent blue, `0x7FC3E8F3`.
    /// Half-transparent on purpose -- the grid is for measuring against, and
    /// a grid you cannot see through is measuring against itself.
    pub const DEFAULT_COLOR: Color = Color(0x7FC3_E8F3);

    pub fn new() -> GridPaper {
        GridPaper {
            color: Self::DEFAULT_COLOR,
            interval: 100.0,
            divisions: 2,
            subdivisions: 5,
        }
    }

    /// Upstream asserts **both** counts are above zero, and its message says
    /// why in each case: "if there were no divisions, the grid paper would not
    /// paint anything".
    ///
    /// Zero is refused rather than treated as "just the primary lines",
    /// because a caller who wrote zero meant something and getting a blank
    /// overlay back tells them nothing about which of the two they got wrong.
    pub fn is_valid(&self) -> bool {
        self.divisions > 0 && self.subdivisions > 0
    }

    /// The spacing of the finest lines: the interval divided by both counts.
    ///
    /// The two multiply rather than adding, which is what makes the defaults
    /// -- 100 / 2 / 5 -- give a ten-pixel finest grid rather than a
    /// fourteen-pixel one.
    pub fn smallest_interval(&self) -> f32 {
        self.interval / self.divisions as f32 / self.subdivisions as f32
    }

    /// How many lines fall in one primary cell, counting the primary line
    /// itself.
    pub fn lines_per_interval(&self) -> u32 {
        self.divisions * self.subdivisions
    }
}

/// What a key handler decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyEventResult {
    /// The key was used; nobody else should see it.
    Handled,
    /// Not mine -- keep looking.
    #[default]
    Ignored,
    /// Upstream's `skipRemainingHandlers`: stop asking, but do not claim it
    /// either. It is how a widget says "nothing below me should get this
    /// key", which is different from using it.
    SkipRemainingHandlers,
}

/// Upstream `KeyboardListener`: raw key events, without focus traversal.
///
/// It is the plain sibling of `Focus`, and the difference is what it does
/// **not** do: no traversal, no shortcuts, no actions. A caller who wants "run
/// this when a key is pressed while my subtree has focus" and nothing else
/// gets it without inheriting a traversal policy they then have to switch off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardListener {
    /// Upstream's `autofocus`, **false** by default. A listener that grabbed
    /// focus on build would take it from whatever the reader was using.
    pub autofocus: bool,
    /// Upstream's `includeSemantics`, **true** by default: the subtree appears
    /// in the semantics tree as a focusable thing, because something that
    /// takes key events is something a keyboard user can reach.
    pub include_semantics: bool,
}

impl KeyboardListener {
    pub fn new() -> KeyboardListener {
        KeyboardListener {
            autofocus: false,
            include_semantics: true,
        }
    }

    /// Upstream's handler returns `KeyEventResult.ignored` when the caller
    /// supplied no `onKeyEvent`, so an empty listener is transparent rather
    /// than a hole keys fall into.
    pub fn handle(&self, has_callback: bool, callback_handled: bool) -> KeyEventResult {
        if !has_callback {
            return KeyEventResult::Ignored;
        }
        if callback_handled {
            KeyEventResult::Handled
        } else {
            KeyEventResult::Ignored
        }
    }
}

/// Upstream `NavigationToolbar`: leading, middle, trailing, laid out by hand.
///
/// It exists because a `Row` gets this wrong. A centred title in a `Row` is
/// centred **in the space left over**, so it shifts whenever the leading or
/// trailing widget changes width -- a back arrow appearing moves the title.
/// This lays the middle out against the **whole** toolbar and only gives up
/// when there is not room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationToolbar {
    pub has_leading: bool,
    pub has_middle: bool,
    pub has_trailing: bool,
    /// Upstream's `centerMiddle`, true by default.
    pub center_middle: bool,
    pub middle_spacing: f32,
}

impl Default for NavigationToolbar {
    fn default() -> NavigationToolbar {
        NavigationToolbar::new()
    }
}

impl NavigationToolbar {
    /// Upstream's `kMiddleSpacing`: 16 logical pixels on each side of the
    /// middle widget.
    pub const MIDDLE_SPACING: f32 = 16.0;

    pub fn new() -> NavigationToolbar {
        NavigationToolbar {
            has_leading: false,
            has_middle: false,
            has_trailing: false,
            center_middle: true,
            middle_spacing: Self::MIDDLE_SPACING,
        }
    }

    pub fn with_slots(mut self, leading: bool, middle: bool, trailing: bool) -> Self {
        self.has_leading = leading;
        self.has_middle = middle;
        self.has_trailing = trailing;
        self
    }

    pub fn with_center_middle(mut self, center: bool) -> Self {
        self.center_middle = center;
        self
    }

    /// Where the middle widget starts, given the widths around it.
    ///
    /// The centred case tries the **true centre of the toolbar** first, and
    /// only slides the middle across when that would overlap a side. So a
    /// title stays put as long as it can, and moves as little as it must.
    pub fn middle_start(
        &self,
        toolbar_width: f32,
        leading_width: f32,
        trailing_width: f32,
        middle_width: f32,
    ) -> f32 {
        if !self.center_middle {
            return leading_width + self.middle_spacing;
        }
        let centred = (toolbar_width - middle_width) / 2.0;
        let leading_limit = leading_width + self.middle_spacing;
        let trailing_limit = toolbar_width - trailing_width - self.middle_spacing - middle_width;
        centred
            .max(leading_limit)
            .min(trailing_limit.max(leading_limit))
    }

    /// How many of the three slots are filled. An empty slot contributes
    /// nothing rather than an empty box, which is why a toolbar with no
    /// leading widget puts its title flush against the edge.
    pub fn filled_slots(&self) -> usize {
        usize::from(self.has_leading)
            + usize::from(self.has_middle)
            + usize::from(self.has_trailing)
    }
}

/// Upstream `SharedAppData`: a small pot of values a package's own widgets can
/// share.
///
/// Its documentation is unusually careful about what it is **not**: "not
/// intended to be a substitute for Provider or any of the other general
/// purpose application state systems". It exists so a package can ship widgets
/// that share a value or two "without requiring the developer to add a
/// package-specific umbrella widget to their application" -- `WidgetsApp`
/// creates one automatically, so it is always there.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SharedAppData {
    values: Vec<(String, String)>,
    /// Which keys a dependency was created on, in order.
    dependencies: Vec<String>,
    rebuilds: usize,
}

impl SharedAppData {
    pub fn new() -> SharedAppData {
        SharedAppData::default()
    }

    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }

    pub fn rebuilds(&self) -> usize {
        self.rebuilds
    }

    pub fn peek(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(held, _)| held == key)
            .map(|(_, value)| value.as_str())
    }

    /// Upstream's `getValue`, which **creates a dependency on that key** --
    /// through `InheritedModel.inheritFrom` with the key as the aspect, so a
    /// widget reading `foo` is not rebuilt when `bar` changes.
    ///
    /// The `init` callback is called only when the key is absent, which is
    /// what makes the values lazily initialised: a package's widget can ask
    /// for its default without anybody having set one up first.
    pub fn get_value(&mut self, key: &str, init: impl FnOnce() -> String) -> String {
        if !self.dependencies.iter().any(|held| held == key) {
            self.dependencies.push(key.to_string());
        }
        if let Some(value) = self.peek(key) {
            return value.to_string();
        }
        let value = init();
        self.values.push((key.to_string(), value.clone()));
        value
    }

    /// Upstream's `setValue`, and its doc states the contrast outright:
    /// "unlike `SharedAppData.getValue`, this method does _not_ create a
    /// dependency between `context` and `key`".
    ///
    /// It reaches for the model with `getInheritedWidgetOfExactType` rather
    /// than depending on it. A widget that writes a value should not be
    /// rebuilt by its own write -- it already knows.
    ///
    /// And a value `==` to the current one rebuilds nothing, which is why the
    /// values are expected to be **immutable**: a mutated object compares
    /// equal to itself, so nothing would happen.
    pub fn set_value(&mut self, key: &str, value: &str) {
        if self.peek(key) == Some(value) {
            return;
        }
        match self.values.iter_mut().find(|(held, _)| held == key) {
            Some((_, existing)) => *existing = value.to_string(),
            None => self.values.push((key.to_string(), value.to_string())),
        }
        self.rebuilds += 1;
    }

    /// Which dependents a change to `key` would rebuild.
    pub fn rebuilds_for(&self, key: &str) -> bool {
        self.dependencies.iter().any(|held| held == key)
    }
}

/// Upstream `SpellCheckConfiguration`: how a text field checks spelling.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpellCheckConfiguration {
    pub has_service: bool,
    pub has_misspelled_style: bool,
    pub has_toolbar_builder: bool,
    enabled: bool,
}

impl SpellCheckConfiguration {
    pub fn new() -> SpellCheckConfiguration {
        SpellCheckConfiguration {
            has_service: false,
            has_misspelled_style: false,
            has_toolbar_builder: false,
            enabled: true,
        }
    }

    /// Upstream's `SpellCheckConfiguration.disabled` constructor.
    ///
    /// A **separate constructor** rather than an `enabled: false` argument,
    /// and the difference shows in `copyWith` below: disabled is not a field a
    /// caller can flip, it is a kind of configuration.
    pub fn disabled() -> SpellCheckConfiguration {
        SpellCheckConfiguration {
            has_service: false,
            has_misspelled_style: false,
            has_toolbar_builder: false,
            enabled: false,
        }
    }

    pub fn spell_check_enabled(&self) -> bool {
        self.enabled
    }

    /// Upstream's `copyWith`, whose first line is the interesting one:
    ///
    /// ```dart
    /// if (!_spellCheckEnabled) {
    ///   return const SpellCheckConfiguration.disabled();
    /// }
    /// ```
    ///
    /// **A disabled configuration cannot be copied into an enabled one.**
    /// Every field a caller passes is discarded. That is stricter than it
    /// looks and it is right: a theme handing a field a disabled
    /// configuration is saying spell-check is off here, and a caller adding a
    /// misspelled-text style should not turn it back on by accident.
    pub fn copy_with(
        &self,
        has_service: Option<bool>,
        has_misspelled_style: Option<bool>,
        has_toolbar_builder: Option<bool>,
    ) -> SpellCheckConfiguration {
        if !self.enabled {
            return SpellCheckConfiguration::disabled();
        }
        SpellCheckConfiguration {
            has_service: has_service.unwrap_or(self.has_service),
            has_misspelled_style: has_misspelled_style.unwrap_or(self.has_misspelled_style),
            has_toolbar_builder: has_toolbar_builder.unwrap_or(self.has_toolbar_builder),
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- ImageFiltered -------------------------------------------------------

    #[test]
    fn a_disabled_filter_skips_the_layer_where_a_no_op_filter_would_not() {
        // A blur of radius zero costs nearly what a blur of radius ten does,
        // which is why upstream tells callers to disable rather than pass a
        // no-op.
        assert!(ImageFiltered::new(1).creates_layer());
        assert!(!ImageFiltered::new(1).with_enabled(false).creates_layer());
    }

    // -- GridPaper -----------------------------------------------------------

    #[test]
    fn zero_divisions_is_refused_rather_than_meaning_none() {
        // A caller who wrote zero meant something, and a blank overlay tells
        // them nothing about which of the two they got wrong.
        let mut paper = GridPaper::new();
        assert!(paper.is_valid());

        paper.divisions = 0;
        assert!(!paper.is_valid());

        paper.divisions = 2;
        paper.subdivisions = 0;
        assert!(!paper.is_valid());
    }

    #[test]
    fn the_two_counts_multiply_rather_than_adding() {
        // Which is what makes the defaults give a ten-pixel finest grid rather
        // than a fourteen-pixel one.
        let paper = GridPaper::new();
        assert_eq!(paper.interval, 100.0);
        assert_eq!(paper.smallest_interval(), 10.0);
        assert_eq!(paper.lines_per_interval(), 10);
    }

    #[test]
    fn the_default_grid_is_half_transparent_so_you_can_measure_through_it() {
        let paper = GridPaper::new();
        assert_eq!(paper.color, Color(0x7FC3_E8F3));
        assert!(
            paper.color.alpha() < 0xFF,
            "you can see the design under it"
        );
    }

    // -- KeyboardListener ----------------------------------------------------

    #[test]
    fn a_listener_with_no_callback_is_transparent_rather_than_a_hole() {
        let listener = KeyboardListener::new();
        assert_eq!(listener.handle(false, true), KeyEventResult::Ignored);
        assert_eq!(listener.handle(true, false), KeyEventResult::Ignored);
        assert_eq!(listener.handle(true, true), KeyEventResult::Handled);
    }

    #[test]
    fn a_listener_does_not_grab_focus_but_does_appear_to_a_screen_reader() {
        // Grabbing focus on build would take it from whatever the reader was
        // using; and something taking key events is something a keyboard user
        // can reach.
        let listener = KeyboardListener::new();
        assert!(!listener.autofocus);
        assert!(listener.include_semantics);
    }

    #[test]
    fn skipping_the_remaining_handlers_is_not_the_same_as_claiming_the_key() {
        // It is how a widget says "nothing below me should get this", which is
        // different from using it.
        assert_ne!(
            KeyEventResult::SkipRemainingHandlers,
            KeyEventResult::Handled
        );
        assert_eq!(KeyEventResult::default(), KeyEventResult::Ignored);
    }

    // -- NavigationToolbar ---------------------------------------------------

    #[test]
    fn a_centred_title_stays_put_when_the_leading_widget_appears() {
        // Which is the whole reason this is not a Row: a centred title in a
        // Row is centred in the space left over, so a back arrow appearing
        // moves it.
        let toolbar = NavigationToolbar::new();
        let without_leading = toolbar.middle_start(400.0, 0.0, 0.0, 100.0);
        let with_leading = toolbar.middle_start(400.0, 56.0, 0.0, 100.0);
        assert_eq!(without_leading, 150.0, "the true centre");
        assert_eq!(with_leading, 150.0, "still the true centre");
    }

    #[test]
    fn the_title_slides_only_when_the_centre_would_overlap_a_side() {
        // It moves as little as it must.
        let toolbar = NavigationToolbar::new();
        let squeezed = toolbar.middle_start(400.0, 200.0, 0.0, 100.0);
        assert_eq!(squeezed, 216.0, "pushed clear of the leading widget");
    }

    #[test]
    fn an_uncentred_title_simply_follows_the_leading_widget() {
        let toolbar = NavigationToolbar::new().with_center_middle(false);
        assert_eq!(toolbar.middle_start(400.0, 56.0, 0.0, 100.0), 72.0);
    }

    #[test]
    fn an_empty_slot_contributes_nothing_rather_than_an_empty_box() {
        // Which is why a toolbar with no leading widget puts its title flush
        // against the edge.
        let bare = NavigationToolbar::new();
        assert_eq!(bare.filled_slots(), 0);
        assert_eq!(
            NavigationToolbar::new()
                .with_slots(true, true, false)
                .filled_slots(),
            2
        );
    }

    // -- SharedAppData -------------------------------------------------------

    #[test]
    fn reading_a_key_creates_a_dependency_and_writing_one_does_not() {
        // A widget that writes a value should not be rebuilt by its own write
        // -- it already knows.
        let mut data = SharedAppData::new();
        data.get_value("foo", || "one".to_string());
        assert!(data.rebuilds_for("foo"));

        data.set_value("bar", "two");
        assert!(
            !data.rebuilds_for("bar"),
            "writing created no dependency of its own"
        );
    }

    #[test]
    fn a_widget_reading_one_key_is_not_rebuilt_when_another_changes() {
        let mut data = SharedAppData::new();
        data.get_value("foo", || "one".to_string());
        assert!(!data.rebuilds_for("bar"));
    }

    #[test]
    fn a_value_is_initialised_lazily_by_whoever_asks_first() {
        // So a package's widget can ask for its default without anybody having
        // set one up.
        let mut data = SharedAppData::new();
        assert_eq!(data.peek("foo"), None);
        assert_eq!(data.get_value("foo", || "default".to_string()), "default");
        assert_eq!(
            data.get_value("foo", || panic!("init must not run twice")),
            "default"
        );
    }

    #[test]
    fn writing_the_same_value_rebuilds_nothing() {
        // Which is why the values are expected to be immutable: a mutated
        // object compares equal to itself, so nothing would happen.
        let mut data = SharedAppData::new();
        data.set_value("foo", "one");
        assert_eq!(data.rebuilds(), 1);

        data.set_value("foo", "one");
        assert_eq!(data.rebuilds(), 1);

        data.set_value("foo", "two");
        assert_eq!(data.rebuilds(), 2);
    }

    // -- SpellCheckConfiguration ---------------------------------------------

    #[test]
    fn a_disabled_configuration_cannot_be_copied_into_an_enabled_one() {
        // A theme handing a field a disabled configuration is saying
        // spell-check is off here, and adding a style should not turn it back
        // on by accident.
        let off = SpellCheckConfiguration::disabled();
        let attempted = off.copy_with(Some(true), Some(true), Some(true));

        assert!(!attempted.spell_check_enabled());
        assert!(!attempted.has_service, "every field was discarded");
        assert!(!attempted.has_misspelled_style);
        assert!(!attempted.has_toolbar_builder);
    }

    #[test]
    fn an_enabled_configuration_copies_normally() {
        let on = SpellCheckConfiguration::new();
        let copied = on.copy_with(Some(true), None, None);
        assert!(copied.spell_check_enabled());
        assert!(copied.has_service);
        assert!(!copied.has_misspelled_style, "left as it was");
    }

    #[test]
    fn disabled_is_a_kind_of_configuration_rather_than_a_field() {
        assert!(SpellCheckConfiguration::new().spell_check_enabled());
        assert!(!SpellCheckConfiguration::disabled().spell_check_enabled());
    }
}
