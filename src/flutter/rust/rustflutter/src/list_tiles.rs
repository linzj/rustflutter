//! Ports of `material/checkbox_list_tile.dart`, `material/radio_list_tile.dart`
//! and `material/switch_list_tile.dart`.
//!
//! A control with a label you can also tap, three times over. Reading the three
//! together turns up something none of them says on its own.

use std::cell::RefCell;

use crate::editable_text::TargetPlatform;
use crate::framework::{AnyWidget, BuildContext, Component, component};
use crate::gestures::PointerHandlers;
use crate::widget_state::MaterialTapTargetSize;

/// Upstream `ListTileControlAffinity`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListTileControlAffinity {
    /// Control on the leading edge, secondary widget on the trailing one.
    Leading,
    /// The other way round.
    Trailing,
    /// Documented as *"the fashion that is typical for the current platform"*.
    ///
    /// It is not that. See [`ListTileControlAffinity::resolve`].
    #[default]
    Platform,
}

/// Which control the tile is wrapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileControl {
    Checkbox,
    Radio,
    Switch,
}

/// Where the control and the secondary widget end up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSlots {
    pub control_is_leading: bool,
}

impl ListTileControlAffinity {
    /// Resolving `platform`, and this is the finding.
    ///
    /// The enum value is documented as platform-typical, and **no
    /// implementation looks at the platform.** What they look at is which
    /// control they are wrapping: `checkbox_list_tile.dart` and
    /// `switch_list_tile.dart` group `platform` with `trailing`, while
    /// `radio_list_tile.dart` groups it with `leading`.
    ///
    /// ```dart
    /// // checkbox_list_tile.dart and switch_list_tile.dart
    /// ListTileControlAffinity.trailing || ListTileControlAffinity.platform => (secondary, control),
    /// // radio_list_tile.dart
    /// ListTileControlAffinity.leading || ListTileControlAffinity.platform => (control, secondary),
    /// ```
    ///
    /// So the value is meaningful -- it means "wherever this kind of control
    /// conventionally goes", and a radio conventionally goes first while a
    /// checkbox or a switch goes last. **It is named after the wrong axis: it
    /// varies by control, not by platform.**
    ///
    /// Ported as it behaves, with the name upstream gave it.
    pub fn resolve(self, control: TileControl) -> TileSlots {
        let control_is_leading = match self {
            ListTileControlAffinity::Leading => true,
            ListTileControlAffinity::Trailing => false,
            ListTileControlAffinity::Platform => matches!(control, TileControl::Radio),
        };
        TileSlots { control_is_leading }
    }

    /// Whether resolving this value consults the platform. It never does.
    pub fn consults_the_platform(self) -> bool {
        false
    }
}

/// What the three tiles share.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlListTile {
    pub control: TileControl,
    /// `None` is the indeterminate state, which only a tristate control may
    /// have -- the same rule as [`crate::toggleable::ToggleableStateMixin`].
    pub value: Option<bool>,
    pub tristate: bool,
    pub has_subtitle: bool,
    pub is_three_line: bool,
    /// `None` falls through to the list tile theme's, then to `Platform`.
    pub control_affinity: Option<ListTileControlAffinity>,
    pub has_secondary: bool,
    /// `None` is `ShrinkWrap` here, which is **not** a bare control's default
    /// -- see [`ControlListTile::control_tap_target`].
    pub material_tap_target_size: Option<MaterialTapTargetSize>,
    /// Upstream's `.adaptive` constructor, which is the one thing about these
    /// tiles that does consult the platform -- see
    /// [`ControlListTile::adapts_away_the_theme`].
    pub adaptive: bool,
    /// Upstream's `toggleable`, which only a radio has.
    ///
    /// A toggleable radio can be un-chosen by tapping it again. Upstream
    /// makes the identity explicit in one line -- `bool get tristate =>
    /// widget.toggleable;` -- so for a radio this *is*
    /// [`ControlListTile::tristate`]: being able to un-choose and being able
    /// to hold "nothing chosen" are one capability seen from two sides.
    pub toggleable: bool,
    /// Upstream's `enabled`, a `bool?`, which this crate's comment used to
    /// say did not exist: "onChanged being null is what `enabled: false`
    /// means for these three -- they have no separate flag".
    ///
    /// `None` is the old behaviour and still the default. See
    /// [`ControlListTile::is_enabled`] for why setting it is not symmetric.
    pub enabled: Option<bool>,
    /// Upstream's `radioScaleFactor` and `checkboxScaleFactor`, which are the
    /// same field on the two tiles that have one.
    ///
    /// `SwitchListTile` has neither this nor a shape, and that absence is
    /// what names it: a switch has nothing to scale independently of its
    /// track, and the two controls that draw a mark *inside a box* do.
    ///
    /// One is not "scaled by one" -- see
    /// [`ControlListTile::scales_the_control`].
    pub radio_scale_factor: f32,
    /// Upstream's `checkboxShape`, and the radio's ring by the same route.
    ///
    /// `None` falls through to the component theme and then to
    /// [`ControlListTile::CHECKBOX_RADIUS`].
    pub control_shape: Option<crate::borders::ShapeBorder>,
    /// Upstream's `internalAddSemanticForOnTap`: whether a tile you can tap
    /// says `button: true` to a screen reader.
    ///
    /// Upstream's own doc calls it "a temporary flag to help changing the
    /// behavior of ListTile onTap semantics" -- it is mid-migration on
    /// whether a tappable row *is* a button, which is why the name says
    /// `internal` and why this is not something a caller should be reaching
    /// for.
    pub internal_add_semantic_for_on_tap: bool,
    /// Upstream's `useCupertinoCheckmarkStyle`: on Apple platforms an
    /// adaptive radio draws an iOS checkmark instead of a ring.
    ///
    /// Upstream's plain constructor does not take this at all -- it sets it
    /// in its initializer list -- so there is no assert, because there is
    /// nothing to assert. This port has one type where upstream has two
    /// constructors, so [`ControlListTile::validate_checkmark_style`] says it
    /// instead.
    pub use_cupertino_checkmark_style: bool,
}

impl ControlListTile {
    pub fn new(control: TileControl, value: Option<bool>) -> ControlListTile {
        ControlListTile {
            control,
            value,
            tristate: false,
            has_subtitle: false,
            is_three_line: false,
            control_affinity: None,
            has_secondary: false,
            material_tap_target_size: None,
            adaptive: false,
            toggleable: false,
            enabled: None,
            radio_scale_factor: 1.0,
            control_shape: None,
            internal_add_semantic_for_on_tap: true,
            use_cupertino_checkmark_style: false,
        }
    }

    /// Upstream's radio defaults, which are three different kinds of "null
    /// means something".
    ///
    /// The background is transparent, so a radio has no fill of its own and
    /// whatever is behind the row shows through the ring. The side is **a
    /// border in the fill colour** rather than no border -- a radio always
    /// has a ring, and the field replaces it rather than adding one. And the
    /// inner circle is 4.5, the dot inside that ring, which upstream states
    /// as a bare number.
    pub const INNER_RADIUS: f32 = 4.5;

    /// Upstream's checkbox shape default:
    /// `RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(1.0)))`.
    ///
    /// **One**, not two and not four. A checkbox is a square with the corners
    /// barely taken off, and a port reaching for the usual four would draw a
    /// different control.
    pub const CHECKBOX_RADIUS: f32 = 1.0;

    /// The outline the control is drawn in: the tile's, then the theme's,
    /// then upstream's near-square.
    pub fn effective_control_shape(
        &self,
        theme_shape: Option<crate::borders::ShapeBorder>,
    ) -> crate::borders::ShapeBorder {
        self.control_shape
            .clone()
            .or(theme_shape)
            .unwrap_or_else(|| {
                crate::borders::ShapeBorder::Rounded(crate::borders::RoundedRectangleBorder::new(
                    crate::borders::BorderSide::NONE,
                    crate::borders::BorderRadiusGeometry::circular(
                        ControlListTile::CHECKBOX_RADIUS,
                    ),
                ))
            })
    }

    pub fn with_control_shape(mut self, shape: crate::borders::ShapeBorder) -> Self {
        self.control_shape = Some(shape);
        self
    }

    /// Whether a tile with a tap handler announces itself as a button.
    ///
    /// Two conditions, and the flag is only half of it: upstream adds
    /// `button: true` **if onTap is provided**, so a tile with the flag and
    /// no handler is not a button either. A row that does nothing when
    /// pressed is not one, whatever a flag says.
    pub fn announces_as_a_button(&self, has_on_tap: bool) -> bool {
        self.internal_add_semantic_for_on_tap && has_on_tap
    }

    pub fn with_radio_scale_factor(mut self, factor: f32) -> Self {
        self.radio_scale_factor = factor;
        self
    }

    pub fn with_cupertino_checkmark_style(mut self, use_checkmark: bool) -> Self {
        self.use_cupertino_checkmark_style = use_checkmark;
        self
    }

    /// Whether the control is wrapped in a scaling transform at all.
    ///
    /// ```dart
    /// if (widget.radioScaleFactor != 1.0) {
    ///   control = Transform.scale(scale: widget.radioScaleFactor, child: control);
    /// }
    /// ```
    ///
    /// **Not `Transform.scale(scale: 1.0)`**, which would be the same
    /// picture. The default leaves the tree one widget shorter, and a port
    /// that always wrapped would be right about the pixels and wrong about
    /// the tree -- which is what anything walking it sees.
    pub fn scales_the_control(&self) -> bool {
        self.radio_scale_factor != 1.0
    }

    /// Upstream's plain constructor sets `useCupertinoCheckmarkStyle = false`
    /// in its initializer list, so only `.adaptive` can carry a true one.
    pub fn validate_checkmark_style(&self) -> Result<(), &'static str> {
        if self.use_cupertino_checkmark_style && !self.adaptive {
            return Err("useCupertinoCheckmarkStyle is only usable on an adaptive tile");
        }
        Ok(())
    }

    /// Whether the checkmark style actually changes the picture.
    ///
    /// It needs both halves: the flag *and* an Apple platform. Elsewhere an
    /// adaptive radio is the Material one, and there is no checkmark to draw
    /// -- the same shape as [`ControlListTile::adapts_away_the_theme`], and
    /// asked separately because a caller may set the flag on a tile that
    /// never reaches iOS.
    pub fn draws_a_cupertino_checkmark(&self, platform: TargetPlatform) -> bool {
        self.use_cupertino_checkmark_style
            && self.adaptive
            && matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS)
    }

    /// Upstream's `toggleable`, and with it upstream's `tristate` for a
    /// radio.
    pub fn with_toggleable(mut self, toggleable: bool) -> Self {
        self.toggleable = toggleable;
        if self.control == TileControl::Radio {
            self.tristate = toggleable;
        }
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Upstream's `_enabled`:
    ///
    /// ```dart
    /// widget.enabled ?? (widget.onChanged != null || registry != null)
    /// ```
    ///
    /// `has_somewhere_to_go` is upstream's `onChanged != null || registry !=
    /// null` -- a handler on the tile, or a `RadioGroup` above it. The two
    /// are one condition because either is somewhere for the new value to
    /// land, and a control with neither has nothing to report to.
    pub fn is_enabled(&self, has_somewhere_to_go: bool) -> bool {
        self.enabled.unwrap_or(has_somewhere_to_go)
    }

    /// Upstream's `build` assert:
    ///
    /// ```dart
    /// assert(!(widget.enabled ?? false) || widget.onChanged != null
    ///            || RadioGroup.maybeOf<T>(context) != null,
    ///        'Radio is enabled but has no RadioListTile.onChange or registry above');
    /// ```
    ///
    /// **The flag is not symmetric.** It may turn a tile off freely, and may
    /// only turn one on where there was already somewhere for the change to
    /// go. `enabled: true` with no handler and no group is the one
    /// combination refused, because it draws a live control that does
    /// nothing.
    pub fn validate_enabled(&self, has_somewhere_to_go: bool) -> Result<(), &'static str> {
        if self.enabled == Some(true) && !has_somewhere_to_go {
            return Err("enabled but has no onChanged and no RadioGroup above");
        }
        Ok(())
    }

    /// Upstream's `_handleListTileTap`:
    ///
    /// ```dart
    /// if (!widget.toggleable && checked) { return; }
    /// handleChange(checked ? null : radioValue);
    /// ```
    ///
    /// `None` means **no change is reported at all**, and that is the arm
    /// worth naming: an ordinary radio that is already chosen *swallows* the
    /// tap. It does not report the same value again -- which is what a port
    /// writing this from the outside would do, and which would fire
    /// `onChanged` on every tap of a row that was already selected.
    ///
    /// `Some(None)` is the other interesting one: a toggleable radio that was
    /// chosen reports **null**, which is how a group goes back to having
    /// nothing in it.
    pub fn tap_on_radio(&self, checked: bool) -> Option<Option<bool>> {
        if !self.toggleable && checked {
            return None;
        }
        Some(if checked { None } else { Some(true) })
    }

    /// Upstream's two constructor asserts.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.tristate && self.value.is_none() {
            return Err("only a tristate control may have a null value");
        }
        if self.is_three_line && !self.has_subtitle {
            // The third line has to come from somewhere.
            return Err("isThreeLine requires a subtitle");
        }
        Ok(())
    }

    /// Upstream's fallback chain: the widget's, then the list tile theme's,
    /// then `platform`.
    pub fn effective_affinity(
        &self,
        theme_affinity: Option<ListTileControlAffinity>,
    ) -> ListTileControlAffinity {
        self.control_affinity
            .or(theme_affinity)
            .unwrap_or(ListTileControlAffinity::Platform)
    }

    pub fn slots(&self, theme_affinity: Option<ListTileControlAffinity>) -> TileSlots {
        self.effective_affinity(theme_affinity)
            .resolve(self.control)
    }

    /// Upstream wraps the whole thing in `MergeSemantics`, which is what makes
    /// the tile **one** thing to a screen reader rather than a checkbox beside
    /// some unrelated text. It is also what makes tapping the label work: the
    /// label is not a second control, it is part of this one.
    pub fn merges_semantics() -> bool {
        true
    }

    /// Upstream wraps the control in `ExcludeFocus`, in all three tiles and in
    /// both branches of each.
    ///
    /// The tile is the focus stop; the control inside it is not. Without this
    /// Tab would stop twice on one row -- once on the row and once on the
    /// switch in it -- and the second stop would do the same thing as the
    /// first.
    pub fn control_excludes_focus() -> bool {
        true
    }

    /// Upstream's `materialTapTargetSize ?? MaterialTapTargetSize.shrinkWrap`.
    ///
    /// A bare `Switch` defaults to `Padded`, growing itself to the
    /// 48-pixel minimum. Inside a tile that default is **overridden**, because
    /// the tile is the tap target and a second 48-high target inside a 48-high
    /// row buys nothing while making the row taller.
    ///
    /// Upstream's own default differs from this one, so the tile is not
    /// deferring to the control here -- it is contradicting it.
    pub fn control_tap_target(&self) -> MaterialTapTargetSize {
        self.material_tap_target_size
            .unwrap_or(MaterialTapTargetSize::ShrinkWrap)
    }

    /// Whether an adaptive control on this platform throws the switch theme
    /// away.
    ///
    /// Upstream's `_SwitchThemeAdaptation.adapt` returns the theme unchanged
    /// on Android, Fuchsia, Linux and Windows, and **`const SwitchThemeData()`
    /// -- an empty one -- on iOS and macOS**. So "adaptive" here does not mean
    /// "use a different theme on Apple platforms"; it means *forget the one
    /// you were given*.
    ///
    /// And it reads `ThemeData.platform`, not the device. A caller who sets
    /// the theme's platform moves this with it, which is the point of that
    /// field existing.
    ///
    /// The contrast worth keeping: [`ListTileControlAffinity::Platform`] is
    /// named for the platform and never asks
    /// ([`ListTileControlAffinity::consults_the_platform`] is false), while
    /// this is not named for it and always does.
    pub fn adapts_away_the_theme(&self, platform: TargetPlatform) -> bool {
        self.adaptive && matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS)
    }
}

/// Upstream `CheckboxListTile`.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckboxListTile(pub ControlListTile);

impl CheckboxListTile {
    pub fn new(value: bool) -> CheckboxListTile {
        CheckboxListTile(ControlListTile::new(TileControl::Checkbox, Some(value)))
    }

    /// Upstream's tristate constructor, the only way to a null value.
    pub fn tristate(value: Option<bool>) -> CheckboxListTile {
        let mut tile = ControlListTile::new(TileControl::Checkbox, value);
        tile.tristate = true;
        CheckboxListTile(tile)
    }
}

/// Upstream `RadioListTile`.
///
/// The one of the three whose control sits **first** by default, because a
/// column of radios reads as a list of choices and the marks want to line up
/// down the leading edge.
#[derive(Clone, Debug, PartialEq)]
pub struct RadioListTile(pub ControlListTile);

impl RadioListTile {
    pub fn new(selected: bool) -> RadioListTile {
        RadioListTile(ControlListTile::new(TileControl::Radio, Some(selected)))
    }
}

/// Upstream `SwitchListTile`.
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchListTile(pub ControlListTile);

impl SwitchListTile {
    pub fn new(value: bool) -> SwitchListTile {
        SwitchListTile(ControlListTile::new(TileControl::Switch, Some(value)))
    }
}

// -- Building the three ---------------------------------------------------------

/// One of the three tiles, with everything needed to put it on screen.
///
/// [`ControlListTile`] is the decision -- where the control goes, whether the
/// value is legal, what merges into what. This is that decision plus the
/// content, and it is what upstream's three widgets are once their
/// `build` methods have chosen the slots: **a `ListTile` with the control in
/// one slot and the secondary widget in the other**.
///
/// That is worth saying plainly because it is what the three have in common
/// and none of them says: none of them lays anything out. Every one of
/// upstream's `build` methods ends in a `ListTile`, and the tile is where the
/// padding, the height, the colours and the tap all come from.
pub struct ControlTile {
    tile: ControlListTile,
    id: u64,
    title: String,
    subtitle: Option<String>,
    secondary: RefCell<Option<AnyWidget>>,
    handlers: PointerHandlers,
    enabled: bool,
    selected: bool,
    dense: Option<bool>,
    theme_affinity: Option<ListTileControlAffinity>,
}

impl ControlTile {
    pub fn new(id: u64, tile: ControlListTile, title: impl Into<String>) -> ControlTile {
        ControlTile {
            tile,
            id,
            title: title.into(),
            subtitle: None,
            secondary: RefCell::new(None),
            handlers: PointerHandlers::new(),
            enabled: true,
            selected: false,
            dense: None,
            theme_affinity: None,
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self.tile.has_subtitle = true;
        self
    }

    /// Upstream's `secondary`: the widget in whichever slot the control is not
    /// in.
    pub fn with_secondary(self, secondary: AnyWidget) -> Self {
        *self.secondary.borrow_mut() = Some(secondary);
        let mut tile = self;
        tile.tile.has_secondary = true;
        tile
    }

    /// Upstream's `onChanged`.
    ///
    /// This said "which being null is what `enabled: false` means for these
    /// three -- they have no separate flag". They have one now; see
    /// [`ControlListTile::is_enabled`], which is what a handler being present
    /// now feeds rather than decides.
    pub fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        self.handlers = handlers;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn with_dense(mut self, dense: bool) -> Self {
        self.dense = Some(dense);
        self
    }

    pub fn with_control_affinity(mut self, affinity: ListTileControlAffinity) -> Self {
        self.tile.control_affinity = Some(affinity);
        self
    }

    /// The theme's affinity, which sits between the widget's and `platform`.
    pub fn with_theme_affinity(mut self, affinity: ListTileControlAffinity) -> Self {
        self.theme_affinity = Some(affinity);
        self
    }

    pub fn with_three_line(mut self, is_three_line: bool) -> Self {
        self.tile.is_three_line = is_three_line;
        self
    }

    pub fn slots(&self) -> TileSlots {
        self.tile.slots(self.theme_affinity)
    }

    /// The control itself, as a widget.
    ///
    /// A disabled tile builds the control **without handlers**, which is how
    /// upstream's null `onChanged` reaches it: the control is drawn and does
    /// not answer, rather than being drawn and answering into nothing.
    fn control_widget(&self) -> AnyWidget {
        let handlers = if self.enabled {
            self.handlers.clone()
        } else {
            PointerHandlers::new()
        };
        let value = self.tile.value.unwrap_or(false);
        match self.tile.control {
            TileControl::Checkbox => component(
                crate::controls::Checkbox::new(self.id, value)
                    .with_enabled(self.enabled)
                    .with_handlers(handlers),
            ),
            TileControl::Radio => component(
                crate::controls::Radio::new(self.id, value)
                    .with_enabled(self.enabled)
                    .with_handlers(handlers),
            ),
            TileControl::Switch => {
                component(crate::components::Switch::new(self.id, value).with_handlers(handlers))
            }
        }
    }
}

impl ControlTile {
    /// Upstream's `effectiveActiveColor`, which the three tiles all hand to
    /// their `ListTile` as its `selectedColor`.
    ///
    /// So **a selected row is drawn in its own control's colour**, not the
    /// theme's selected colour. On a settings page whose switches are green,
    /// a selected row whose title came out in the theme's primary would put
    /// two accent colours on one line.
    ///
    /// Each control resolves it the way that control does -- a switch's active
    /// colour is not a checkbox's -- so this asks the same resolvers the
    /// controls themselves ask rather than guessing a shared default.
    ///
    /// # Which part of the control the row borrows
    ///
    /// The switch's chain is
    /// `activeThumbColor ?? activeColor ?? switchTheme.thumbColor ?? ...`:
    /// the **thumb**, not the track. This port read the track, which on the
    /// ordinary two-tone switch -- coloured track, pale thumb -- is a
    /// different colour, so a selected row's title came out in the track's
    /// colour where upstream draws it in the thumb's.
    ///
    /// # Which `selected` the state property is asked about
    ///
    /// Upstream builds the state set from the **tile's** `selected`:
    ///
    /// ```dart
    /// final states = <WidgetState>{if (selected) WidgetState.selected};
    /// ```
    ///
    /// not from the control's value. The two are separate properties and they
    /// come apart in ordinary use -- a settings page marks the row the reader
    /// arrived at with `selected: true` whether or not its switch is on. This
    /// port asked the switch, so a themed colour keyed on `selected` answered
    /// the wrong question in both directions.
    ///
    /// # The fallback
    ///
    /// Upstream ends at `theme.colorScheme.secondary` in all three chains.
    /// This crate's [`crate::components::Theme`] is a smaller palette with no
    /// secondary role at all, so the last step is its `primary`, which is the
    /// accent this port has. That is a substitution and is written down as
    /// one; it is not a claim that upstream ends at primary.
    pub(crate) fn control_active_color(&self, context: &mut BuildContext) -> crate::engine::Color {
        let theme = crate::components::theme_of(context);
        // The tile's `selected`, which is the whole of upstream's state set
        // here -- `enabled` is not in it, in any of the three.
        let states = if self.selected {
            crate::widget_state::WidgetStates::NONE.with(crate::widget_state::WidgetState::Selected)
        } else {
            crate::widget_state::WidgetStates::NONE
        };
        let resolved = match self.tile.control {
            TileControl::Switch => crate::component_themes::SwitchTheme::of(context)
                .thumb_color
                .as_ref()
                .and_then(|property| property.resolve(states)),
            TileControl::Checkbox => crate::component_themes::CheckboxTheme::of(context)
                .fill_color
                .as_ref()
                .and_then(|property| property.resolve(states)),
            TileControl::Radio => crate::component_themes::RadioTheme::of(context)
                .fill_color
                .as_ref()
                .and_then(|property| property.resolve(states)),
        };
        resolved.unwrap_or(theme.primary)
    }
}

impl Component for ControlTile {
    /// Upstream's three `build` methods, which differ only in which control
    /// they make and which way `platform` resolves.
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        debug_assert!(
            self.tile.validate().is_ok(),
            "{}",
            self.tile.validate().unwrap_err()
        );
        let slots = self.slots();
        let control = self.control_widget();
        let secondary = self.secondary.borrow().clone();

        let mut list = crate::components::ListTile::new(self.title.clone())
            .with_selected_color(self.control_active_color(_context))
            .with_selected(self.selected)
            .with_enabled(self.enabled)
            .with_three_line(self.tile.is_three_line);
        // Upstream's three all pass `onTap` down to the tile, and it is not a
        // convenience: **the label is part of the control**. A row whose
        // checkbox is the only target makes the reader aim at a 20-pixel box
        // when a 400-pixel one is sitting right there saying what it does.
        //
        // The `enabled` check here is upstream's `onTap: onChanged != null ?
        // ... : null`, and [`crate::components::ListTile`] checks `enabled`
        // again before it makes itself a target -- upstream passes both, and
        // each of them alone is enough. Neither can be removed on its own
        // without the other still refusing the tap, so the two are load-bearing
        // only as a pair; a mutation has to take both to see it.
        if self.enabled {
            list = list.tappable(self.id, self.handlers.clone());
        }
        if let Some(subtitle) = &self.subtitle {
            list = list
                .with_subtitle(subtitle.clone())
                .with_three_line(self.tile.is_three_line);
        }
        if let Some(dense) = self.dense {
            list = list.with_dense(dense);
        }
        // The two slots, filled the way `slots()` said. The secondary goes in
        // whichever one the control did not take -- and if there is no
        // secondary that slot stays empty rather than being filled with the
        // control, which is what makes affinity visible with nothing else in
        // the tile.
        if slots.control_is_leading {
            list = list.with_leading(control);
            if let Some(secondary) = secondary {
                list = list.with_trailing(secondary);
            }
        } else {
            list = list.with_trailing(control);
            if let Some(secondary) = secondary {
                list = list.with_leading(secondary);
            }
        }

        // Upstream wraps the whole thing in `MergeSemantics`, which is what
        // makes the tile *one* thing to a reader rather than a control beside
        // some unrelated text -- see [`ControlListTile::merges_semantics`].
        // This crate's [`crate::semantics_markers::MergeSemantics`] is the
        // description and not a wrapper, and nothing here turns a subtree into
        // a merged one, so the claim stands where it already stood and the
        // tree is built plainly. Saying it in a wrapper that does nothing
        // would read as though it did.
        component(list)
    }
}

impl CheckboxListTile {
    /// The widget. Upstream's `CheckboxListTile.build`.
    pub fn widget(self, id: u64, title: impl Into<String>) -> ControlTile {
        ControlTile::new(id, self.0, title)
    }
}

impl RadioListTile {
    /// The widget. Upstream's `RadioListTile.build`.
    pub fn widget(self, id: u64, title: impl Into<String>) -> ControlTile {
        ControlTile::new(id, self.0, title)
    }
}

impl SwitchListTile {
    /// The widget. Upstream's `SwitchListTile.build`.
    pub fn widget(self, id: u64, title: impl Into<String>) -> ControlTile {
        ControlTile::new(id, self.0, title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- What a checkbox tile has that a switch tile does not, tick 272 ------
    //
    // `depth.py` reports `CheckboxListTile` at 5 of 42, and checking each of
    // upstream's forty-one members against the whole crate leaves five with
    // no hit -- one of which, `_checkboxType`, is the private adaptive marker
    // this port already carries.

    #[test]
    fn a_checkbox_is_a_square_with_its_corners_barely_taken_off() {
        // One, not two and not four. A port reaching for the usual four would
        // draw a different control.
        assert_eq!(ControlListTile::CHECKBOX_RADIUS, 1.0);
        let tile = ControlListTile::new(TileControl::Checkbox, Some(true));
        match tile.effective_control_shape(None) {
            crate::borders::ShapeBorder::Rounded(rounded) => {
                let radius = rounded
                    .border_radius
                    .resolve(crate::direction::TextDirection::Ltr);
                assert_eq!(radius.top_left.x, 1.0);
            }
            other => panic!("expected a rounded rectangle, got {other:?}"),
        }
    }

    #[test]
    fn the_tiles_own_shape_beats_the_themes_and_the_theme_beats_the_default() {
        // Three steps, and the middle one is the reason the parameter exists:
        // `CheckboxThemeData.shape` sits between them.
        let stadium =
            crate::borders::ShapeBorder::Stadium(crate::borders::StadiumBorder::default());
        let circle = crate::borders::ShapeBorder::Circle(crate::borders::CircleBorder::default());
        let tile = ControlListTile::new(TileControl::Checkbox, Some(true));

        assert!(matches!(
            tile.clone()
                .with_control_shape(circle.clone())
                .effective_control_shape(Some(stadium.clone())),
            crate::borders::ShapeBorder::Circle(_)
        ));
        assert!(matches!(
            tile.effective_control_shape(Some(stadium)),
            crate::borders::ShapeBorder::Stadium(_)
        ));
    }

    #[test]
    fn a_tappable_row_is_a_button_and_a_row_with_no_handler_is_not() {
        // Upstream adds `button: true` *if onTap is provided*, so the flag is
        // only half of it. A row that does nothing when pressed is not a
        // button, whatever a flag says.
        let tile = ControlListTile::new(TileControl::Checkbox, Some(true));
        assert!(tile.internal_add_semantic_for_on_tap, "upstream's default");
        assert!(tile.announces_as_a_button(true));
        assert!(!tile.announces_as_a_button(false));

        // And the flag turns it off even with a handler -- which is what it
        // is for: upstream's doc calls it "a temporary flag to help changing
        // the behavior of ListTile onTap semantics".
        let mut migrating = tile;
        migrating.internal_add_semantic_for_on_tap = false;
        assert!(!migrating.announces_as_a_button(true));
    }

    #[test]
    fn the_scale_factor_belongs_to_the_two_controls_that_draw_a_mark_in_a_box() {
        // `SwitchListTile` has neither a scale factor nor a shape upstream,
        // and that absence is what names the shared field: a switch has
        // nothing to scale independently of its track.
        for control in [
            TileControl::Radio,
            TileControl::Checkbox,
            TileControl::Switch,
        ] {
            let tile = ControlListTile::new(control, Some(true));
            assert_eq!(tile.radio_scale_factor, 1.0, "{control:?}");
            assert!(!tile.scales_the_control(), "{control:?}");
        }
    }

    // -- What a radio in a row looks like, tick 262 --------------------------

    #[test]
    fn a_scale_factor_of_one_wraps_the_control_in_nothing_at_all() {
        // Upstream: `if (widget.radioScaleFactor != 1.0) { control =
        // Transform.scale(...) }`. Not `Transform.scale(scale: 1.0)`, which
        // would be the same picture -- the default leaves the tree one widget
        // shorter, and a port that always wrapped would be right about the
        // pixels and wrong about the tree, which is what anything walking it
        // sees.
        let plain = ControlListTile::new(TileControl::Radio, Some(true));
        assert_eq!(plain.radio_scale_factor, 1.0);
        assert!(!plain.scales_the_control());

        assert!(
            plain
                .clone()
                .with_radio_scale_factor(1.5)
                .scales_the_control()
        );
        assert!(
            plain
                .clone()
                .with_radio_scale_factor(0.5)
                .scales_the_control(),
            "shrinking is a transform too"
        );
    }

    #[test]
    fn the_cupertino_checkmark_is_only_offered_on_an_adaptive_tile() {
        // Upstream's plain constructor sets `useCupertinoCheckmarkStyle =
        // false` in its initializer list, so it is not a parameter there and
        // there is no assert -- there is nothing to assert. This port has one
        // type where upstream has two constructors, so it has to be said.
        let plain = ControlListTile::new(TileControl::Radio, Some(true));
        assert_eq!(plain.validate_checkmark_style(), Ok(()));
        assert!(
            plain
                .with_cupertino_checkmark_style(true)
                .validate_checkmark_style()
                .is_err()
        );

        let mut adaptive = ControlListTile::new(TileControl::Radio, Some(true));
        adaptive.adaptive = true;
        assert_eq!(
            adaptive
                .with_cupertino_checkmark_style(true)
                .validate_checkmark_style(),
            Ok(())
        );
    }

    #[test]
    fn the_checkmark_needs_the_flag_and_an_apple_platform_both() {
        // Off an Apple platform an adaptive radio is the Material one and
        // there is no checkmark to draw. Asked separately from the validator
        // because a caller may set the flag on a tile that never reaches iOS
        // -- that is legal and simply does nothing.
        let mut tile = ControlListTile::new(TileControl::Radio, Some(true));
        tile.adaptive = true;
        let checkmarked = tile.clone().with_cupertino_checkmark_style(true);

        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(
                checkmarked.draws_a_cupertino_checkmark(platform),
                "{platform:?}"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert!(
                !checkmarked.draws_a_cupertino_checkmark(platform),
                "{platform:?}"
            );
        }

        // And an adaptive tile without the flag draws a ring on iOS like
        // everywhere else.
        assert!(!tile.draws_a_cupertino_checkmark(TargetPlatform::IOS));

        // The same platforms `adapts_away_the_theme` picks, which is the
        // point: "adaptive" means one thing here and both readers of it agree
        // about which platforms it means.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                checkmarked.draws_a_cupertino_checkmark(platform),
                checkmarked.adapts_away_the_theme(platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_radios_inner_circle_is_four_and_a_half() {
        // Upstream states it as a bare number in the doc for
        // `radioInnerRadius`: "If null, then it defaults to 4.5 in all
        // states." The dot inside the ring.
        assert_eq!(ControlListTile::INNER_RADIUS, 4.5);
    }

    // -- Un-choosing a radio, and a comment that had expired, tick 261 -------
    //
    // `with_handlers` carried this: "Upstream's `onChanged`, which being null
    // is what `enabled: false` means for these three -- they have no separate
    // flag." They have one now, and it does not work the way a reader of that
    // sentence would guess.

    #[test]
    fn a_plain_radio_swallows_a_tap_on_the_row_it_already_chose() {
        // The arm worth naming. It does *not* report the same value again --
        // which is what writing this from the outside would produce, and
        // which would fire `onChanged` on every tap of a row that was already
        // selected.
        let plain = ControlListTile::new(TileControl::Radio, Some(true));
        assert_eq!(plain.tap_on_radio(true), None, "nothing is reported at all");
        assert_eq!(
            plain.tap_on_radio(false),
            Some(Some(true)),
            "and an unchosen row still chooses itself"
        );
    }

    #[test]
    fn a_toggleable_radio_reports_null_to_un_choose_itself() {
        // Which is how a group goes back to having nothing in it.
        let toggleable = ControlListTile::new(TileControl::Radio, Some(true)).with_toggleable(true);
        assert_eq!(toggleable.tap_on_radio(true), Some(None));
        assert_eq!(toggleable.tap_on_radio(false), Some(Some(true)));

        // The two differ only on the chosen row, which is the whole content
        // of the flag.
        let plain = ControlListTile::new(TileControl::Radio, Some(true));
        assert_eq!(
            toggleable.tap_on_radio(false),
            plain.tap_on_radio(false),
            "an unchosen row behaves the same either way"
        );
        assert_ne!(toggleable.tap_on_radio(true), plain.tap_on_radio(true));
    }

    #[test]
    fn a_toggleable_radio_is_a_tristate_control() {
        // Upstream makes the identity explicit in one line: `bool get
        // tristate => widget.toggleable;`. Being able to un-choose a radio
        // and being able to hold "nothing chosen" are one capability seen
        // from two sides -- so the validator that refuses a null value on a
        // non-tristate control lets one through here.
        let toggleable = ControlListTile::new(TileControl::Radio, None).with_toggleable(true);
        assert!(toggleable.tristate);
        assert_eq!(toggleable.validate(), Ok(()));

        let plain = ControlListTile::new(TileControl::Radio, Some(true));
        assert!(!plain.tristate);

        // And it is a radio's flag alone: a checkbox's tristate is its own
        // and `toggleable` does not touch it.
        let checkbox =
            ControlListTile::new(TileControl::Checkbox, Some(true)).with_toggleable(true);
        assert!(checkbox.toggleable);
        assert!(
            !checkbox.tristate,
            "a checkbox's tristate is a separate flag"
        );
    }

    #[test]
    fn the_enabled_flag_may_turn_a_tile_off_freely_and_on_only_conditionally() {
        // Upstream's assert:
        //
        //   assert(!(widget.enabled ?? false) || widget.onChanged != null
        //              || RadioGroup.maybeOf<T>(context) != null,
        //          'Radio is enabled but has no RadioListTile.onChange or registry above');
        //
        // `enabled: true` with nowhere for the change to go is the one
        // combination refused, because it draws a live control that does
        // nothing.
        let tile = ControlListTile::new(TileControl::Radio, Some(true));
        assert_eq!(
            tile.clone().with_enabled(false).validate_enabled(false),
            Ok(())
        );
        assert_eq!(
            tile.clone().with_enabled(false).validate_enabled(true),
            Ok(())
        );
        assert_eq!(
            tile.clone().with_enabled(true).validate_enabled(true),
            Ok(())
        );
        assert!(
            tile.clone()
                .with_enabled(true)
                .validate_enabled(false)
                .is_err()
        );

        // Unset is never refused: it *is* the condition rather than a claim
        // about it.
        assert_eq!(tile.validate_enabled(false), Ok(()));
        assert_eq!(tile.validate_enabled(true), Ok(()));
    }

    #[test]
    fn without_the_flag_a_handler_or_a_group_is_what_makes_a_tile_live() {
        // `widget.enabled ?? (widget.onChanged != null || registry != null)`.
        // The two halves of that disjunction are one condition because either
        // is somewhere for the new value to land.
        let tile = ControlListTile::new(TileControl::Radio, Some(true));
        assert!(tile.is_enabled(true));
        assert!(!tile.is_enabled(false));

        // And the flag overrides both directions.
        assert!(
            !tile.clone().with_enabled(false).is_enabled(true),
            "off despite a handler"
        );
        assert!(tile.clone().with_enabled(true).is_enabled(false));
    }

    // -- The three, actually built -----------------------------------------------------

    use crate::framework::{ElementTree, provide};
    use crate::render::{BoxConstraints, RenderBox};

    const TILE: u64 = 31;
    const SECONDARY: u64 = 32;

    /// How many targets are stacked at `x`. The row is one of them once the
    /// tile is tappable, so "is the control here" is "is there more than the
    /// row".
    fn depth_at(tile: ControlTile, x: f32) -> usize {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            crate::framework::component(tile),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::tight(400.0, 80.0));
        let mut result = crate::render::HitTestResult::new();
        root.hit_test(
            crate::render::Offset::new(x, size.height / 2.0),
            &mut result,
        );
        result.path.len()
    }

    /// Which marker is at `x`, halfway down a 400-wide tile.
    fn hit(tile: ControlTile, x: f32) -> Option<u64> {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            crate::framework::component(tile),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::tight(400.0, 80.0));
        let mut result = crate::render::HitTestResult::new();
        root.hit_test(
            crate::render::Offset::new(x, size.height / 2.0),
            &mut result,
        );
        result.path.first().map(|entry| entry.target)
    }

    fn secondary() -> crate::framework::AnyWidget {
        crate::framework::leaf(|| {
            crate::widgets::Pointer::new(
                SECONDARY,
                crate::widgets::Container::new().with_size(30.0, 24.0),
            )
        })
    }

    #[test]
    fn a_radio_tile_puts_its_control_first_and_a_switch_tile_puts_it_last() {
        // The finding, now visible on screen rather than only in the
        // resolution: `platform` varies by control.
        // Measured by depth rather than by identity: the row is a target
        // everywhere now, and it carries the same id as the control, so the
        // question "is the control here" is "is there anything above the row".
        let radio = RadioListTile::new(true).widget(TILE, "Medium");
        assert_eq!(depth_at(radio, 25.0), 2, "leading edge");

        let switch = SwitchListTile::new(true).widget(TILE, "Wi-Fi");
        assert_eq!(depth_at(switch, 375.0), 2, "trailing edge");

        let checkbox = CheckboxListTile::new(true).widget(TILE, "Remember me");
        assert_eq!(depth_at(checkbox, 375.0), 2);
    }

    #[test]
    fn an_explicit_affinity_overrides_the_control_s_habit() {
        let radio = RadioListTile::new(true)
            .widget(TILE, "Medium")
            .with_control_affinity(ListTileControlAffinity::Trailing);
        assert_eq!(depth_at(radio, 375.0), 2);

        let switch = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_control_affinity(ListTileControlAffinity::Leading);
        assert_eq!(depth_at(switch, 25.0), 2);
    }

    #[test]
    fn the_theme_sits_between_the_widget_and_the_control_s_habit() {
        // Widget, then theme, then platform.
        let themed = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_theme_affinity(ListTileControlAffinity::Leading);
        assert!(themed.slots().control_is_leading);

        let overridden = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_theme_affinity(ListTileControlAffinity::Leading)
            .with_control_affinity(ListTileControlAffinity::Trailing);
        assert!(!overridden.slots().control_is_leading);
    }

    #[test]
    fn the_secondary_takes_whichever_slot_the_control_did_not() {
        let switch = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_secondary(secondary());
        assert_eq!(hit(switch, 375.0), Some(TILE), "control trailing");

        let switch = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_secondary(secondary());
        assert_eq!(hit(switch, 25.0), Some(SECONDARY), "secondary leading");

        let radio = RadioListTile::new(true)
            .widget(TILE, "Medium")
            .with_secondary(secondary());
        assert_eq!(
            hit(radio, 25.0),
            Some(TILE),
            "and the other way for a radio"
        );
        let radio = RadioListTile::new(true)
            .widget(TILE, "Medium")
            .with_secondary(secondary());
        assert_eq!(hit(radio, 375.0), Some(SECONDARY));
    }

    #[test]
    fn a_tile_with_no_secondary_leaves_that_slot_empty() {
        // Rather than filling it with the control, which would make affinity
        // invisible in the commonest case of all. Both branches, because each
        // fills a different slot and a rule that held on one side only would
        // pass a test of the other.
        let switch = SwitchListTile::new(true).widget(TILE, "Wi-Fi");
        assert_eq!(depth_at(switch, 25.0), 1, "control trails, leading empty");

        let radio = RadioListTile::new(true).widget(TILE, "Medium");
        assert_eq!(depth_at(radio, 375.0), 1, "control leads, trailing empty");
    }

    #[test]
    fn a_disabled_tile_builds_a_control_that_does_not_answer() {
        // Upstream's null `onChanged` is what `enabled: false` means for these
        // three -- they have no flag of their own.
        let live = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_handlers(crate::gestures::PointerHandlers::new());
        assert_eq!(hit(live, 375.0), Some(TILE));

        let dead = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_handlers(crate::gestures::PointerHandlers::new())
            .with_enabled(false);
        assert_eq!(
            depth_at(dead, 200.0),
            0,
            "and the row itself is not a target either"
        );
    }

    #[test]
    fn a_disabled_controls_own_tap_goes_nowhere() {
        // The control is still drawn and still hit-testable -- it is the
        // handlers it is built without. Tapping it has to reach nothing.
        fn taps(enabled: bool) -> usize {
            let heard = std::rc::Rc::new(std::cell::Cell::new(0));
            let counter = std::rc::Rc::clone(&heard);
            let handlers = crate::gestures::PointerHandlers::new()
                .with_tap(move |_| counter.set(counter.get() + 1));
            let tile = SwitchListTile::new(true)
                .widget(TILE, "Wi-Fi")
                .with_handlers(handlers)
                .with_enabled(enabled);

            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                crate::framework::component(tile),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            let size = root.layout(BoxConstraints::tight(400.0, 80.0));
            let mut result = crate::render::HitTestResult::new();
            root.hit_test(
                crate::render::Offset::new(375.0, size.height / 2.0),
                &mut result,
            );
            for entry in &result.path {
                if let Some(handlers) = &entry.handlers {
                    if let Some(tap) = &handlers.on_tap {
                        tap(crate::gestures::TapEvent {
                            local_position: crate::render::Offset::ZERO,
                            position: crate::render::Offset::ZERO,
                            pointer_id: 0,
                        });
                    }
                }
            }
            heard.get()
        }

        // Two, and both are the point: the control answers, and so does the
        // row -- the label is part of the control, and a reader aiming at a
        // 20-pixel box when a 400-pixel one says the same thing is the bug
        // upstream's `onTap` on the tile exists to prevent.
        assert_eq!(taps(true), 2, "the control and the row it is in");
        assert_eq!(taps(false), 0, "and neither, when disabled");
    }

    #[test]
    fn a_subtitle_is_recorded_on_the_description_the_asserts_read() {
        // What a subtitle *does* was invisible to this harness twice over:
        // text measured nothing in the stub engine, and the control's own
        // height sets the row's anyway. The first of those two reasons has
        // gone -- the stub measures now -- and the second has not, which is
        // the interesting half: a switch is taller than two lines of text, so
        // the tile is the same height with a subtitle and without, and that is
        // a fact about the tile rather than about the harness.
        //
        // What is decidable here is that the description knows -- which is
        // what `validate` and the three-line rule read.
        let plain = SwitchListTile::new(true).widget(TILE, "Wi-Fi");
        assert!(!plain.tile.has_subtitle);

        let with_one = SwitchListTile::new(true)
            .widget(TILE, "Wi-Fi")
            .with_subtitle("Connected to Home");
        assert!(with_one.tile.has_subtitle);
        assert_eq!(with_one.subtitle.as_deref(), Some("Connected to Home"));
    }

    #[test]
    fn a_subtitle_makes_the_row_taller_even_under_a_switch() {
        // This test used to assert the opposite -- that the control set the
        // height and a subtitle cost nothing -- and it passed because tick
        // 341 found every tile taking the *one-line* fallback of 56. Upstream
        // chooses by line count: `_defaultTileHeight`'s `(false, true)` arm
        // is 72, so a tile with a subtitle is a taller row before its
        // contents are measured at all.
        let height = |tile: ControlTile| {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                crate::framework::component(tile),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(400.0, 400.0)).height
        };

        let plain = height(SwitchListTile::new(true).widget(TILE, "Wi-Fi"));
        let with_subtitle = height(
            SwitchListTile::new(true)
                .widget(TILE, "Wi-Fi")
                .with_subtitle("Connected to Home"),
        );
        assert_eq!(plain, 56.0, "one line");
        assert_eq!(with_subtitle, 72.0, "two lines, which is upstream's row");
        assert!(
            with_subtitle > plain,
            "the subtitle buys a taller row: {plain} then {with_subtitle}"
        );
    }

    #[test]
    fn three_lines_without_a_subtitle_is_refused() {
        // Checked at build too -- `ControlTile::build` asserts it -- but
        // asserted here rather than there, because the element tree catches a
        // panicking build and reports it instead of letting it out, so a
        // `should_panic` around the build would never see it.
        let tile = CheckboxListTile::new(true)
            .widget(TILE, "Remember me")
            .with_three_line(true);
        assert_eq!(tile.tile.validate(), Err("isThreeLine requires a subtitle"));

        let with_one = CheckboxListTile::new(true)
            .widget(TILE, "Remember me")
            .with_subtitle("and stay signed in")
            .with_three_line(true);
        assert_eq!(with_one.tile.validate(), Ok(()));
    }

    // -- The finding ------------------------------------------------------------

    #[test]
    fn platform_affinity_varies_by_control_and_not_by_platform() {
        // The enum value is documented as platform-typical, and no
        // implementation looks at the platform. A radio goes first; a checkbox
        // and a switch go last.
        let platform = ListTileControlAffinity::Platform;
        assert!(platform.resolve(TileControl::Radio).control_is_leading);
        assert!(!platform.resolve(TileControl::Checkbox).control_is_leading);
        assert!(!platform.resolve(TileControl::Switch).control_is_leading);
        assert!(!platform.consults_the_platform());
    }

    #[test]
    fn the_two_explicit_values_do_not_vary_at_all() {
        for control in [
            TileControl::Checkbox,
            TileControl::Radio,
            TileControl::Switch,
        ] {
            assert!(
                ListTileControlAffinity::Leading
                    .resolve(control)
                    .control_is_leading,
                "{control:?}"
            );
            assert!(
                !ListTileControlAffinity::Trailing
                    .resolve(control)
                    .control_is_leading,
                "{control:?}"
            );
        }
    }

    #[test]
    fn a_column_of_radios_lines_its_marks_up_down_the_leading_edge() {
        // Which is why the radio tile is the odd one out.
        let radio = RadioListTile::new(true);
        let checkbox = CheckboxListTile::new(true);
        assert!(radio.0.slots(None).control_is_leading);
        assert!(!checkbox.0.slots(None).control_is_leading);
    }

    // -- The fallback chain --------------------------------------------------------

    #[test]
    fn the_widget_beats_the_theme_and_the_theme_beats_the_default() {
        let mut tile = ControlListTile::new(TileControl::Checkbox, Some(true));
        assert_eq!(
            tile.effective_affinity(None),
            ListTileControlAffinity::Platform
        );
        assert_eq!(
            tile.effective_affinity(Some(ListTileControlAffinity::Leading)),
            ListTileControlAffinity::Leading
        );

        tile.control_affinity = Some(ListTileControlAffinity::Trailing);
        assert_eq!(
            tile.effective_affinity(Some(ListTileControlAffinity::Leading)),
            ListTileControlAffinity::Trailing
        );
    }

    #[test]
    fn a_theme_can_move_every_control_in_a_list_at_once() {
        let checkbox = CheckboxListTile::new(true);
        assert!(
            checkbox
                .0
                .slots(Some(ListTileControlAffinity::Leading))
                .control_is_leading
        );
    }

    // -- What the constructors refuse -------------------------------------------------

    #[test]
    fn only_a_tristate_control_may_be_null() {
        // The same rule as the toggleable mixin it is built on.
        assert!(
            ControlListTile::new(TileControl::Checkbox, None)
                .validate()
                .is_err()
        );
        assert_eq!(
            CheckboxListTile::tristate(None).0.validate(),
            Ok(()),
            "and a tristate one may"
        );
        assert_eq!(CheckboxListTile::new(true).0.validate(), Ok(()));
    }

    #[test]
    fn the_third_line_has_to_come_from_somewhere() {
        let mut tile = ControlListTile::new(TileControl::Checkbox, Some(true));
        tile.is_three_line = true;
        assert!(tile.validate().is_err());

        tile.has_subtitle = true;
        assert_eq!(tile.validate(), Ok(()));
    }

    // -- One thing, not two ------------------------------------------------------------

    #[test]
    fn the_label_is_part_of_the_control_rather_than_next_to_it() {
        // MergeSemantics is what makes the tile one thing to a screen reader,
        // and it is also what makes tapping the label work.
        assert!(ControlListTile::merges_semantics());
    }

    #[test]
    fn a_tristate_checkbox_tile_keeps_its_indeterminate_value() {
        let tile = CheckboxListTile::tristate(None);
        assert!(tile.0.tristate);
        assert_eq!(tile.0.value, None);
    }
}

#[cfg(test)]
mod the_tile_is_the_control_tests {
    use super::*;
    use crate::focus::ExcludeFocus;

    fn tile(control: TileControl) -> ControlListTile {
        ControlListTile::new(control, Some(true))
    }

    // -- Three ways of saying the same thing -----------------------------------

    #[test]
    fn the_control_is_reachable_by_nothing_the_reader_has() {
        // Keyboard, finger and screen reader, each closed a different way, and
        // all three saying that the row is the control and the switch in it is
        // a picture of the control's state.
        assert!(ControlListTile::control_excludes_focus(), "keyboard");
        assert_eq!(
            tile(TileControl::Switch).control_tap_target(),
            MaterialTapTargetSize::ShrinkWrap,
            "finger"
        );
        assert!(ControlListTile::merges_semantics(), "screen reader");
    }

    #[test]
    fn and_the_tap_target_default_is_a_contradiction_of_the_controls_own() {
        // A bare control defaults to `Padded` -- it grows itself to the
        // 48-pixel minimum. Inside a tile that is overridden, so the tile is
        // not deferring to the control here, it is disagreeing with it.
        assert_eq!(
            MaterialTapTargetSize::default(),
            MaterialTapTargetSize::Padded
        );
        assert_ne!(
            tile(TileControl::Switch).control_tap_target(),
            MaterialTapTargetSize::default()
        );
    }

    #[test]
    fn a_caller_can_still_ask_for_the_padded_one() {
        // It is a default, not a rule: `materialTapTargetSize ?? shrinkWrap`.
        let mut padded = tile(TileControl::Switch);
        padded.material_tap_target_size = Some(MaterialTapTargetSize::Padded);
        assert_eq!(padded.control_tap_target(), MaterialTapTargetSize::Padded);
    }

    #[test]
    fn all_three_tiles_agree_about_all_of_it() {
        // Upstream writes it out three times, in both branches of each.
        for control in [
            TileControl::Checkbox,
            TileControl::Radio,
            TileControl::Switch,
        ] {
            assert_eq!(
                tile(control).control_tap_target(),
                MaterialTapTargetSize::ShrinkWrap,
                "{control:?}"
            );
        }
    }

    // -- Excluding focus --------------------------------------------------------

    #[test]
    fn excluding_focus_closes_four_doors_and_only_one_is_the_flag() {
        // `canRequestFocus: false`, `skipTraversal: true` and
        // `includeSemantics: false` are constant; only
        // `descendantsAreFocusable` is what `excluding` decides.
        let excluding = ExcludeFocus::new();
        let not = ExcludeFocus::excluding(false);

        assert!(excluding.skips_traversal() && not.skips_traversal());
        assert!(!excluding.includes_semantics() && !not.includes_semantics());
        assert!(!excluding.can_request_focus());
        assert!(
            !not.can_request_focus(),
            "not excluding is still not itself a stop"
        );

        assert!(!excluding.descendants_are_focusable());
        assert!(not.descendants_are_focusable());
    }

    #[test]
    fn it_defaults_to_excluding() {
        assert!(ExcludeFocus::new().excluding);
        assert!(ExcludeFocus::default().excluding);
    }

    // -- The one place these tiles ask about the platform ----------------------

    #[test]
    fn the_affinity_named_for_the_platform_never_asks_and_adaptive_always_does() {
        // The contrast worth keeping. One is called `platform` and resolves
        // from the control; the other is not, and reads `ThemeData.platform`.
        assert!(!ListTileControlAffinity::Platform.consults_the_platform());

        let mut adaptive = tile(TileControl::Switch);
        adaptive.adaptive = true;
        assert!(adaptive.adapts_away_the_theme(TargetPlatform::IOS));
        assert!(!adaptive.adapts_away_the_theme(TargetPlatform::Android));
    }

    #[test]
    fn a_tile_that_is_not_adaptive_ignores_the_platform_entirely() {
        let plain = tile(TileControl::Switch);
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::IOS,
            TargetPlatform::MacOS,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
            TargetPlatform::Fuchsia,
        ] {
            assert!(!plain.adapts_away_the_theme(platform), "{platform:?}");
        }
    }

    #[test]
    fn the_two_apple_platforms_are_the_two_that_throw_the_theme_away() {
        // `_SwitchThemeAdaptation.adapt` returns the theme unchanged on
        // Android, Fuchsia, Linux and Windows and an empty one on iOS and
        // macOS -- "adaptive" means forget what you were given, not use
        // something else.
        let mut adaptive = tile(TileControl::Switch);
        adaptive.adaptive = true;
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(adaptive.adapts_away_the_theme(platform), "{platform:?}");
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert!(!adaptive.adapts_away_the_theme(platform), "{platform:?}");
        }
    }

    #[test]
    fn the_affinity_still_resolves_from_the_control_and_not_from_the_platform() {
        // Which is the half of the contrast that was already ported, checked
        // here so the two halves sit together.
        assert!(
            ListTileControlAffinity::Platform
                .resolve(TileControl::Radio)
                .control_is_leading
        );
        for control in [TileControl::Checkbox, TileControl::Switch] {
            assert!(
                !ListTileControlAffinity::Platform
                    .resolve(control)
                    .control_is_leading,
                "{control:?}"
            );
        }
    }
}

// -- Whose colour a selected control row is drawn in --------------------------

#[cfg(test)]
mod control_colour_tests {
    //! Upstream's three tiles hand their `ListTile` a `selectedColor` taken
    //! from their own control, so a selected row is drawn in the colour of the
    //! thing that made it selected rather than in the theme's accent.
    //!
    //! # The hand-over, which took two ticks to be able to ask about
    //!
    //! Tick 175 left the line in `build` that hands the colour over uncovered,
    //! because a `ListTile`'s resolved text colour was not observable: the
    //! title goes out as a paragraph and `Drawn::Paragraph` recorded the text
    //! and where it landed but not its colour. Tick 176 recorded the colour --
    //! the builder was handed an `argb` all along and the stub dropped it --
    //! and the last test below is the one that could not be written before.

    use super::{CheckboxListTile, ControlTile, RadioListTile, SwitchListTile, TileControl};
    use crate::component_themes::{
        CheckboxTheme, CheckboxThemeData, RadioTheme, RadioThemeData, SwitchTheme, SwitchThemeData,
    };
    use crate::components::Theme;
    use crate::engine::Color;
    use crate::framework::{
        AnyWidget, BuildContext, Component, ElementTree, component, leaf, provide,
    };
    use crate::widget_state::StateProperty;
    use std::cell::Cell;
    use std::rc::Rc;

    const GREEN: Color = Color(0xff00cc44);
    const ORANGE: Color = Color(0xffee7722);

    /// The colour a tile would hand its `ListTile`, under a switch theme whose
    /// thumb says what it says.
    fn active_colour(tile: ControlTile, thumb: Option<Color>) -> (Color, Color) {
        active_colour_with(tile, thumb, None)
    }

    /// The same, with the track set separately, so the two can disagree.
    fn active_colour_with(
        tile: ControlTile,
        thumb: Option<Color>,
        track: Option<Color>,
    ) -> (Color, Color) {
        struct Reader {
            tile: std::cell::RefCell<Option<ControlTile>>,
            seen: Rc<Cell<(Color, Color)>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let tile = self.tile.borrow_mut().take().expect("built once");
                let colour = tile.control_active_color(context);
                let theme = crate::components::theme_of(context);
                self.seen.set((colour, theme.primary));
                leaf(|| crate::widgets::Empty)
            }
        }
        let seen = Rc::new(Cell::new((Color(0), Color(0))));
        let mut data = SwitchThemeData::new();
        data.thumb_color = thumb.map(|colour| StateProperty::all(Some(colour)));
        data.track_color = track.map(|colour| StateProperty::all(Some(colour)));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            SwitchTheme::new(
                data,
                component(Reader {
                    tile: std::cell::RefCell::new(Some(tile)),
                    seen: Rc::clone(&seen),
                }),
            ),
        ));
        seen.get()
    }

    fn switch_tile() -> ControlTile {
        ControlTile::new(1, SwitchListTile::new(true).0, "Wi-Fi")
    }

    // -- Which part of the switch the row borrows ---------------------------

    #[test]
    fn the_row_borrows_the_thumbs_colour_and_not_the_tracks() {
        // Upstream's chain is `activeThumbColor ?? activeColor ??
        // switchTheme.thumbColor ?? ...`. This port read the track, and on the
        // ordinary two-tone switch -- coloured track, pale thumb -- those are
        // different colours, so the row's title came out wrong in every theme
        // that set both.
        let (colour, _) = active_colour_with(switch_tile(), Some(GREEN), Some(ORANGE));
        assert_eq!(colour, GREEN, "the thumb's");
        assert_ne!(colour, ORANGE, "not the track's");
    }

    #[test]
    fn a_theme_that_only_paints_the_track_does_not_recolour_the_row() {
        // The other direction, and the one a single-colour test cannot see:
        // with only the track set, the row falls all the way through to the
        // accent rather than picking the track up.
        let (colour, primary) = active_colour_with(switch_tile(), None, Some(ORANGE));
        assert_eq!(colour, primary);
        assert_ne!(colour, ORANGE);
    }

    #[test]
    fn a_switch_row_answers_with_its_switchs_colour_and_not_the_accent() {
        // The whole point of upstream's `effectiveActiveColor`: a page of
        // green switches whose selected row's title came out in the theme's
        // primary would carry two accent colours on one line.
        let (colour, primary) = active_colour(switch_tile(), Some(GREEN));
        assert_eq!(colour, GREEN);
        assert_ne!(colour, primary);
    }

    #[test]
    fn and_falls_back_to_the_accent_when_the_switch_has_no_colour_of_its_own() {
        let (colour, primary) = active_colour(switch_tile(), None);
        assert_eq!(colour, primary, "the scheme's, which is the switch's too");
    }

    /// The colour the tile's title was actually drawn in, under a switch
    /// theme whose thumb says what it says.
    fn painted_title_colour(selected: bool, thumb: Option<Color>) -> u32 {
        let mut data = SwitchThemeData::new();
        data.thumb_color = thumb.map(|colour| StateProperty::all(Some(colour)));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            SwitchTheme::new(
                data,
                component(
                    ControlTile::new(1, SwitchListTile::new(true).0, "Wi-Fi")
                        .with_selected(selected),
                ),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 200.0),
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Paragraph { text, argb, .. }
                    if text == "Wi-Fi" =>
                {
                    Some(*argb)
                }
                _ => None,
            })
            .expect("the title was drawn")
    }

    #[test]
    fn and_the_title_really_is_drawn_in_it() {
        // The hand-over. Deleting `.with_selected_color(..)` from `build` left
        // the whole suite green at tick 175, because no test could see what
        // colour any text was drawn in. This is that test.
        assert_eq!(painted_title_colour(true, Some(GREEN)), GREEN.0);
    }

    #[test]
    fn and_an_unselected_row_is_not_drawn_in_it() {
        // The other half, and the one that says the colour is *for* being
        // selected rather than for being a switch row.
        assert_ne!(painted_title_colour(false, Some(GREEN)), GREEN.0);
    }

    #[test]
    fn a_checkbox_row_does_not_read_the_switch_theme() {
        // Each control resolves its own way. A checkbox tile taking a switch
        // theme's colour would recolour half a settings page from a theme that
        // has nothing to do with it.
        let mut tile = switch_tile();
        tile.tile.control = TileControl::Checkbox;
        let (colour, primary) = active_colour(tile, Some(GREEN));
        assert_eq!(colour, primary);
        assert_ne!(colour, GREEN);
    }

    // -- The other two controls, which had no resolver at all ----------------

    /// What a checkbox or radio tile hands over, under its own theme.
    fn control_colour(tile: ControlTile, fill: Option<Color>) -> (Color, Color) {
        struct Reader {
            tile: std::cell::RefCell<Option<ControlTile>>,
            seen: Rc<Cell<(Color, Color)>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let tile = self.tile.borrow_mut().take().expect("built once");
                let colour = tile.control_active_color(context);
                let theme = crate::components::theme_of(context);
                self.seen.set((colour, theme.primary));
                leaf(|| crate::widgets::Empty)
            }
        }
        let seen = Rc::new(Cell::new((Color(0), Color(0))));
        let mut checkbox = CheckboxThemeData::new();
        checkbox.fill_color = fill.map(|colour| StateProperty::all(Some(colour)));
        let mut radio = RadioThemeData::new();
        radio.fill_color = fill.map(|colour| StateProperty::all(Some(colour)));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            CheckboxTheme::new(
                checkbox,
                RadioTheme::new(
                    radio,
                    component(Reader {
                        tile: std::cell::RefCell::new(Some(tile)),
                        seen: Rc::clone(&seen),
                    }),
                ),
            ),
        ));
        seen.get()
    }

    #[test]
    fn a_checkbox_row_reads_its_own_themes_fill_colour() {
        // Upstream is `checkboxTheme.fillColor?.resolve(states)`. This port
        // returned a bare accent for both checkbox and radio and never asked
        // either theme, so a page that recoloured its checkboxes left every
        // selected row's title behind at the default.
        let tile =
            ControlTile::new(1, CheckboxListTile::new(true).0, "Remember me").with_selected(true);
        let (colour, primary) = control_colour(tile, Some(GREEN));
        assert_eq!(colour, GREEN);
        assert_ne!(colour, primary);
    }

    #[test]
    fn and_a_radio_row_reads_its_own() {
        let tile = ControlTile::new(1, RadioListTile::new(true).0, "Medium").with_selected(true);
        let (colour, primary) = control_colour(tile, Some(GREEN));
        assert_eq!(colour, GREEN);
        assert_ne!(colour, primary);
    }

    #[test]
    fn and_both_fall_back_to_the_accent_with_no_theme_to_read() {
        for tile in [
            ControlTile::new(1, CheckboxListTile::new(true).0, "Remember me"),
            ControlTile::new(1, RadioListTile::new(true).0, "Medium"),
        ] {
            let control = tile.tile.control;
            let (colour, primary) = control_colour(tile.with_selected(true), None);
            assert_eq!(colour, primary, "{control:?}");
        }
    }

    // -- Whose `selected` the state property is asked about ------------------

    #[test]
    fn the_state_property_is_asked_about_the_rows_selection_not_the_controls() {
        // Upstream builds `{if (selected) WidgetState.selected}` from the
        // tile's `selected`. This port asked the control's *value*, which is a
        // different property and comes apart in ordinary use: a settings page
        // marks the row the reader arrived at whether or not its switch is on.
        //
        // A property that answers only when selected, so the two questions
        // give different answers.
        let only_selected = || {
            StateProperty::resolve_with(|states| {
                states
                    .contains(crate::widget_state::WidgetState::Selected)
                    .then_some(GREEN)
            })
        };

        struct Reader {
            tile: std::cell::RefCell<Option<ControlTile>>,
            seen: Rc<Cell<Color>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let tile = self.tile.borrow_mut().take().expect("built once");
                self.seen.set(tile.control_active_color(context));
                leaf(|| crate::widgets::Empty)
            }
        }

        let answer = |row_selected: bool, switch_on: bool| {
            let mut data = SwitchThemeData::new();
            data.thumb_color = Some(only_selected());
            let seen = Rc::new(Cell::new(Color(0)));
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                Theme::dark(),
                SwitchTheme::new(
                    data,
                    component(Reader {
                        tile: std::cell::RefCell::new(Some(
                            ControlTile::new(1, SwitchListTile::new(switch_on).0, "Wi-Fi")
                                .with_selected(row_selected),
                        )),
                        seen: Rc::clone(&seen),
                    }),
                ),
            ));
            seen.get()
        };

        // A selected row whose switch is off still resolves as selected...
        assert_eq!(answer(true, false), GREEN);
        // ...and an unselected row whose switch is on does not.
        assert_ne!(answer(false, true), GREEN);
    }
}
