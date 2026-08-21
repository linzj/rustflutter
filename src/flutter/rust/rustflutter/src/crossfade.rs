//! Swapping one child for another over time -- ports of upstream's
//! `widgets/animated_cross_fade.dart`, `widgets/animated_switcher.dart` and
//! `widgets/fade_in_image.dart`.
//!
//! All three do the same thing and differ in what they know. A cross-fade is
//! told **both** children and which to show; a switcher is told **one** child
//! and works out for itself whether it is a new one; a fade-in image is a
//! switcher whose two children are a placeholder and the real thing, and which
//! knows when the second has arrived.
//!
//! The recurring decision is what happens to the child on its way **out**, and
//! all three answer it the same way: it keeps being painted and stops being
//! anything else. It takes no taps, is not announced to a screen reader, and
//! usually stops animating. A reader watching a fade should not be able to tap
//! a button that is halfway gone, and should certainly not hear it read out
//! alongside the one replacing it.

/// Upstream `CrossFadeState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossFadeState {
    #[default]
    ShowFirst,
    ShowSecond,
}

/// Which of the two children is on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossFadeLayers {
    /// The child fading **in**, drawn on top.
    pub top: CrossFadeState,
    /// The child fading **out**, drawn underneath.
    pub bottom: CrossFadeState,
}

/// What a layer gets while the fade runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerTreatment {
    pub tickers_enabled: bool,
    pub ignores_pointer: bool,
    pub excludes_semantics: bool,
    pub excludes_focus: bool,
}

/// Upstream `AnimatedCrossFade`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatedCrossFade {
    pub state: CrossFadeState,
    pub duration_micros: i64,
    pub reverse_duration_micros: Option<i64>,
    /// Upstream's `excludeBottomFocus`, **true** by default.
    pub exclude_bottom_focus: bool,
}

impl Default for AnimatedCrossFade {
    fn default() -> AnimatedCrossFade {
        AnimatedCrossFade::new(CrossFadeState::ShowFirst, 200_000)
    }
}

impl AnimatedCrossFade {
    pub fn new(state: CrossFadeState, duration_micros: i64) -> AnimatedCrossFade {
        AnimatedCrossFade {
            state,
            duration_micros,
            reverse_duration_micros: None,
            exclude_bottom_focus: true,
        }
    }

    /// Which child is on top, from the controller's direction.
    ///
    /// The controller runs **forward towards the second child**, so
    /// `showFirst` reverses it. That is why the two children are not
    /// symmetrical in the code even though they are in the API: one of them is
    /// where zero is.
    pub fn layers(&self, forward_or_completed: bool) -> CrossFadeLayers {
        if forward_or_completed {
            CrossFadeLayers {
                top: CrossFadeState::ShowSecond,
                bottom: CrossFadeState::ShowFirst,
            }
        } else {
            CrossFadeLayers {
                top: CrossFadeState::ShowFirst,
                bottom: CrossFadeState::ShowSecond,
            }
        }
    }

    /// Upstream's treatment of the **top** child: everything on.
    ///
    /// The comment on its semantics is explicit -- "always publish semantics
    /// for the widget that's fading in" -- and its tickers stay enabled
    /// unconditionally, because the thing arriving is the thing the reader is
    /// about to interact with.
    pub fn top_treatment(&self) -> LayerTreatment {
        LayerTreatment {
            tickers_enabled: true,
            ignores_pointer: false,
            excludes_semantics: false,
            excludes_focus: false,
        }
    }

    /// Upstream's treatment of the **bottom** child, and the asymmetry is the
    /// design.
    ///
    /// It **always ignores pointers** and **always excludes semantics** --
    /// upstream's comment is "always exclude the semantics of the widget
    /// that's fading out", so a screen reader never reads two versions of the
    /// same thing at once. Its tickers run **only while the fade is running**,
    /// which stops a settled cross-fade paying for an invisible subtree's
    /// animations for as long as it exists.
    ///
    /// Focus is the one part the caller controls, and it defaults to excluded.
    pub fn bottom_treatment(&self, animating: bool) -> LayerTreatment {
        LayerTreatment {
            tickers_enabled: animating,
            ignores_pointer: true,
            excludes_semantics: true,
            excludes_focus: self.exclude_bottom_focus,
        }
    }

    /// Upstream's `defaultLayoutBuilder`, which positions the **bottom** child
    /// with `left/top/right` set and the top child with none of them.
    ///
    /// So the outgoing child is stretched to the incoming one's width while
    /// the incoming one sizes itself. The stack takes its size from the top
    /// child, which is what makes the whole thing grow towards the new
    /// content rather than jumping to it.
    pub fn bottom_is_stretched_horizontally() -> bool {
        true
    }

    /// Whether the widget wraps its result in an `AnimatedSize`.
    ///
    /// It always does, and that is most of why the class exists: fading two
    /// differently-sized children into each other without animating the size
    /// makes the surrounding layout jump on the first frame.
    pub fn animates_size(&self) -> bool {
        true
    }
}

/// Upstream `AnimatedSwitcher`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatedSwitcher {
    pub duration_micros: i64,
    pub reverse_duration_micros: Option<i64>,
}

/// A child, reduced to what `Widget.canUpdate` compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitcherChild {
    /// Upstream compares `runtimeType`.
    pub widget_type: &'static str,
    pub key: Option<u64>,
}

impl SwitcherChild {
    pub fn new(widget_type: &'static str) -> SwitcherChild {
        SwitcherChild {
            widget_type,
            key: None,
        }
    }

    pub fn keyed(widget_type: &'static str, key: u64) -> SwitcherChild {
        SwitcherChild {
            widget_type,
            key: Some(key),
        }
    }

    /// Upstream's `Widget.canUpdate`: same runtime type **and** same key.
    pub fn can_update(&self, other: &SwitcherChild) -> bool {
        self.widget_type == other.widget_type && self.key == other.key
    }
}

/// What a rebuild did to the switcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchOutcome {
    /// A new entry was added and the old one started fading out.
    Switched,
    /// The existing entry was updated in place -- no animation.
    UpdatedInPlace,
    /// There was nothing before and nothing now.
    Nothing,
}

impl Default for AnimatedSwitcher {
    fn default() -> AnimatedSwitcher {
        AnimatedSwitcher::new(200_000)
    }
}

impl AnimatedSwitcher {
    pub fn new(duration_micros: i64) -> AnimatedSwitcher {
        AnimatedSwitcher {
            duration_micros,
            reverse_duration_micros: None,
        }
    }

    /// Upstream's `didUpdateWidget` decision, and the trap it is famous for.
    ///
    /// A switch happens only when the new child **cannot update** the old one
    /// -- a different runtime type or a different key. Two `Text` widgets with
    /// different strings and no keys can update each other, so **nothing
    /// animates**: the text simply changes.
    ///
    /// That surprises everyone once, and upstream's documentation says so at
    /// length. The fix is a key, which is why nearly every example in the
    /// wild has one.
    pub fn decide(old: Option<SwitcherChild>, new: Option<SwitcherChild>) -> SwitchOutcome {
        match (old, new) {
            (None, None) => SwitchOutcome::Nothing,
            (Some(_), None) | (None, Some(_)) => SwitchOutcome::Switched,
            (Some(old), Some(new)) => {
                if new.can_update(&old) {
                    SwitchOutcome::UpdatedInPlace
                } else {
                    SwitchOutcome::Switched
                }
            }
        }
    }

    /// Upstream's `defaultTransitionBuilder`, which keys the fade **by the
    /// child's key**.
    ///
    /// That is what stops the transition itself being reused across a switch:
    /// two children with different keys get different `FadeTransition`s, and
    /// the outgoing one keeps its own opacity animation while the incoming one
    /// starts a fresh one.
    pub fn transition_key(child: &SwitcherChild) -> Option<u64> {
        child.key
    }

    /// Upstream's `defaultLayoutBuilder`: previous children first, then the
    /// current one -- so the arriving child is painted **on top** -- all
    /// centred on each other.
    pub fn paint_order(previous: &[u64], current: Option<u64>) -> Vec<u64> {
        let mut order = previous.to_vec();
        if let Some(current) = current {
            order.push(current);
        }
        order
    }

    /// Upstream reverses the outgoing entry's controller rather than starting
    /// a separate fade-out, which is what makes `reverseDuration` and
    /// `switchOutCurve` apply to it: it is the same animation running
    /// backwards.
    pub fn outgoing_runs_in_reverse() -> bool {
        true
    }
}

/// Upstream `FadeInImage`: a placeholder that gives way to the real image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeInImage {
    /// Upstream's default: 300ms out.
    pub fade_out_micros: i64,
    /// Upstream's default: 700ms in.
    pub fade_in_micros: i64,
}

impl Default for FadeInImage {
    fn default() -> FadeInImage {
        FadeInImage::new()
    }
}

/// Which of the two images is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FadeInPhase {
    /// The placeholder, while the real image loads.
    #[default]
    Placeholder,
    /// The placeholder fading out.
    FadingOut,
    /// The real image fading in.
    FadingIn,
    /// The real image, settled.
    Complete,
}

impl FadeInImage {
    pub const DEFAULT_FADE_OUT_MICROS: i64 = 300_000;
    pub const DEFAULT_FADE_IN_MICROS: i64 = 700_000;

    pub fn new() -> FadeInImage {
        FadeInImage {
            fade_out_micros: Self::DEFAULT_FADE_OUT_MICROS,
            fade_in_micros: Self::DEFAULT_FADE_IN_MICROS,
        }
    }

    /// The two durations are **deliberately unequal**, and the ratio is the
    /// point: the placeholder leaves in 300ms and the image arrives over
    /// 700ms.
    ///
    /// A symmetric cross-fade would show both at half strength through the
    /// middle, which on a photograph over a grey box reads as a smear. Letting
    /// the placeholder go first and the image come in slowly means the reader
    /// sees the image resolve rather than two pictures overlapping.
    pub fn fade_out_is_quicker(&self) -> bool {
        self.fade_out_micros < self.fade_in_micros
    }

    /// Which phase a given moment is in, measured from the real image
    /// arriving.
    pub fn phase_at(&self, loaded: bool, micros_since_loaded: i64) -> FadeInPhase {
        if !loaded {
            return FadeInPhase::Placeholder;
        }
        if micros_since_loaded < self.fade_out_micros {
            return FadeInPhase::FadingOut;
        }
        if micros_since_loaded < self.fade_out_micros + self.fade_in_micros {
            return FadeInPhase::FadingIn;
        }
        FadeInPhase::Complete
    }

    /// Upstream's `FadeInImage.memoryNetwork` and `.assetNetwork` exist for
    /// one reason worth stating: the **placeholder must not itself be a
    /// network image**, or the widget would be waiting on two downloads to
    /// show the reader anything.
    pub fn placeholder_must_be_local() -> bool {
        true
    }
}

/// Upstream `Icon`: a glyph from an icon font.
///
/// It is a font glyph rather than a picture, which is why it takes a `size`
/// and a `color` and no `fit`: it scales like text because it *is* text, and
/// a font renders at any size without blurring.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Icon {
    /// `None` means "inherit from the ambient `IconTheme`", which is how a
    /// whole toolbar's icons change colour by one line above them. Every field
    /// below is the same three-step chain: the icon, the theme, the fallback.
    pub size: Option<f32>,
    pub color: Option<crate::engine::Color>,
    pub fill: Option<f32>,
    pub weight: Option<f32>,
    pub grade: Option<f32>,
    pub optical_size: Option<f32>,
    /// Upstream's `applyTextScaling` -- see
    /// [`crate::component_themes::ResolvedIcon`] for why it defaults to off.
    pub apply_text_scaling: Option<bool>,
    pub shadows: Option<Vec<crate::painting::BoxShadow>>,
    /// Upstream's `semanticLabel`. Absent by default, and that is right: most
    /// icons sit next to a label that already says what they are, and
    /// announcing both would say it twice.
    pub has_semantic_label: bool,
}

impl Icon {
    /// Upstream's default when no `IconTheme` supplies one.
    pub const DEFAULT_SIZE: f32 = 24.0;

    pub fn new() -> Icon {
        Icon::default()
    }

    /// The size against a theme size handed in, for a caller with no context.
    ///
    /// The fallback here is 24 and not `kDefaultFontSize`, because a caller
    /// passing a theme size explicitly has a theme -- see
    /// [`crate::component_themes::ResolvedIcon`], where the distinction lives.
    pub fn resolved_size(&self, theme_size: Option<f32>) -> f32 {
        self.size.or(theme_size).unwrap_or(Self::DEFAULT_SIZE)
    }

    /// Everything this icon is drawn with, read off the ambient `IconTheme`.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedIcon {
        crate::component_themes::ResolvedIcon::of(context, self)
    }

    /// Upstream asserts `fill` is between 0 and 1 and `weight` is above zero
    /// -- variable-font axes with real ranges, not free numbers.
    pub fn axes_are_valid(&self) -> bool {
        self.fill.is_none_or(|fill| (0.0..=1.0).contains(&fill))
            && self.weight.is_none_or(|weight| weight > 0.0)
    }
}

/// Upstream `ImageIcon`: the same shape, from an image instead of a font.
///
/// It exists for icons a font cannot carry -- a multicoloured logo, an avatar
/// -- and it takes the `IconTheme`'s size and colour so it lines up with the
/// font icons beside it. The colour is applied as a **blend**, which is why an
/// `ImageIcon` of a photograph comes out tinted rather than replaced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageIcon {
    pub icon: Icon,
}

impl ImageIcon {
    pub fn new() -> ImageIcon {
        ImageIcon::default()
    }

    /// It follows the same theme resolution as [`Icon`], which is the whole
    /// point of the class.
    pub fn resolved_size(&self, theme_size: Option<f32>) -> f32 {
        self.icon.resolved_size(theme_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The cross-fade ------------------------------------------------------

    #[test]
    fn the_controller_runs_forward_towards_the_second_child() {
        // Which is why the two are not symmetrical in the code even though
        // they are in the API: one of them is where zero is.
        let fade = AnimatedCrossFade::default();
        assert_eq!(
            fade.layers(true).top,
            CrossFadeState::ShowSecond,
            "forward means the second is arriving"
        );
        assert_eq!(fade.layers(false).top, CrossFadeState::ShowFirst);
        assert_eq!(fade.layers(true).bottom, CrossFadeState::ShowFirst);
    }

    #[test]
    fn the_child_fading_out_is_never_read_out_alongside_the_one_arriving() {
        // Upstream: "always exclude the semantics of the widget that's fading
        // out".
        let fade = AnimatedCrossFade::default();
        assert!(fade.bottom_treatment(true).excludes_semantics);
        assert!(fade.bottom_treatment(false).excludes_semantics);
        assert!(!fade.top_treatment().excludes_semantics);
    }

    #[test]
    fn a_button_halfway_gone_cannot_be_tapped() {
        let fade = AnimatedCrossFade::default();
        assert!(fade.bottom_treatment(true).ignores_pointer);
        assert!(!fade.top_treatment().ignores_pointer);
    }

    #[test]
    fn a_settled_cross_fade_stops_paying_for_the_hidden_subtrees_animations() {
        // The bottom child's tickers run only while the fade is running.
        let fade = AnimatedCrossFade::default();
        assert!(fade.bottom_treatment(true).tickers_enabled);
        assert!(!fade.bottom_treatment(false).tickers_enabled);
        assert!(
            fade.top_treatment().tickers_enabled,
            "where the top child's always are"
        );
    }

    #[test]
    fn focus_is_the_one_part_of_the_bottom_child_the_caller_controls() {
        let mut fade = AnimatedCrossFade::default();
        assert!(fade.exclude_bottom_focus, "excluded by default");
        assert!(fade.bottom_treatment(true).excludes_focus);

        fade.exclude_bottom_focus = false;
        assert!(!fade.bottom_treatment(true).excludes_focus);
        assert!(
            fade.bottom_treatment(true).excludes_semantics,
            "but semantics stay excluded regardless"
        );
    }

    #[test]
    fn a_cross_fade_always_animates_its_size() {
        // Fading two differently-sized children into each other without it
        // makes the surrounding layout jump on the first frame.
        assert!(AnimatedCrossFade::default().animates_size());
    }

    // -- The switcher --------------------------------------------------------

    #[test]
    fn two_texts_with_no_keys_do_not_animate_at_all() {
        // The trap that surprises everyone once: they can update each other,
        // so the text simply changes.
        let outcome = AnimatedSwitcher::decide(
            Some(SwitcherChild::new("Text")),
            Some(SwitcherChild::new("Text")),
        );
        assert_eq!(outcome, SwitchOutcome::UpdatedInPlace);
    }

    #[test]
    fn a_key_is_what_makes_the_switch_happen() {
        // Which is why nearly every example in the wild has one.
        let outcome = AnimatedSwitcher::decide(
            Some(SwitcherChild::keyed("Text", 1)),
            Some(SwitcherChild::keyed("Text", 2)),
        );
        assert_eq!(outcome, SwitchOutcome::Switched);
    }

    #[test]
    fn a_different_widget_type_switches_without_a_key() {
        let outcome = AnimatedSwitcher::decide(
            Some(SwitcherChild::new("Text")),
            Some(SwitcherChild::new("Icon")),
        );
        assert_eq!(outcome, SwitchOutcome::Switched);
    }

    #[test]
    fn appearing_and_disappearing_both_count_as_a_switch() {
        assert_eq!(
            AnimatedSwitcher::decide(None, Some(SwitcherChild::new("Text"))),
            SwitchOutcome::Switched
        );
        assert_eq!(
            AnimatedSwitcher::decide(Some(SwitcherChild::new("Text")), None),
            SwitchOutcome::Switched
        );
        assert_eq!(AnimatedSwitcher::decide(None, None), SwitchOutcome::Nothing);
    }

    #[test]
    fn the_arriving_child_is_painted_on_top_of_the_ones_leaving() {
        assert_eq!(
            AnimatedSwitcher::paint_order(&[1, 2], Some(3)),
            vec![1, 2, 3]
        );
        assert_eq!(AnimatedSwitcher::paint_order(&[1], None), vec![1]);
    }

    #[test]
    fn the_transition_is_keyed_by_the_child_so_it_is_not_reused_across_a_switch() {
        // The outgoing child keeps its own opacity animation while the
        // incoming one starts a fresh one.
        assert_eq!(
            AnimatedSwitcher::transition_key(&SwitcherChild::keyed("Text", 7)),
            Some(7)
        );
        assert_eq!(
            AnimatedSwitcher::transition_key(&SwitcherChild::new("Text")),
            None
        );
    }

    #[test]
    fn the_outgoing_child_runs_the_same_animation_backwards() {
        // Which is what makes reverseDuration and switchOutCurve apply to it.
        assert!(AnimatedSwitcher::outgoing_runs_in_reverse());
    }

    // -- The fading image ----------------------------------------------------

    #[test]
    fn the_placeholder_leaves_faster_than_the_image_arrives() {
        // A symmetric cross-fade would show both at half strength through the
        // middle, which on a photograph over a grey box reads as a smear.
        let image = FadeInImage::new();
        assert_eq!(image.fade_out_micros, 300_000);
        assert_eq!(image.fade_in_micros, 700_000);
        assert!(image.fade_out_is_quicker());
    }

    #[test]
    fn nothing_happens_until_the_real_image_has_arrived() {
        let image = FadeInImage::new();
        assert_eq!(image.phase_at(false, 0), FadeInPhase::Placeholder);
        assert_eq!(
            image.phase_at(false, 10_000_000),
            FadeInPhase::Placeholder,
            "however long it takes"
        );
    }

    #[test]
    fn the_two_fades_run_one_after_the_other_rather_than_together() {
        let image = FadeInImage::new();
        assert_eq!(image.phase_at(true, 0), FadeInPhase::FadingOut);
        assert_eq!(image.phase_at(true, 299_999), FadeInPhase::FadingOut);
        assert_eq!(image.phase_at(true, 300_000), FadeInPhase::FadingIn);
        assert_eq!(image.phase_at(true, 999_999), FadeInPhase::FadingIn);
        assert_eq!(image.phase_at(true, 1_000_000), FadeInPhase::Complete);
    }

    #[test]
    fn the_placeholder_is_never_itself_a_download() {
        // Or the widget would be waiting on two downloads before it could show
        // the reader anything.
        assert!(FadeInImage::placeholder_must_be_local());
    }

    // -- The icons -----------------------------------------------------------

    #[test]
    fn an_icon_takes_its_size_from_the_theme_when_it_was_given_none() {
        // Which is how a whole toolbar's icons change by one line above them.
        let inherits = Icon::new();
        assert_eq!(inherits.resolved_size(Some(18.0)), 18.0);
        assert_eq!(inherits.resolved_size(None), 24.0, "the framework default");

        let mut explicit = Icon::new();
        explicit.size = Some(32.0);
        assert_eq!(explicit.resolved_size(Some(18.0)), 32.0);
    }

    #[test]
    fn an_image_icon_follows_the_same_theme_resolution() {
        // Which is the whole point of the class: it lines up with the font
        // icons beside it.
        let image_icon = ImageIcon::new();
        assert_eq!(image_icon.resolved_size(Some(18.0)), 18.0);
        assert_eq!(image_icon.resolved_size(None), 24.0);
    }

    #[test]
    fn the_variable_font_axes_have_real_ranges_rather_than_free_numbers() {
        let mut icon = Icon::new();
        assert!(icon.axes_are_valid());

        icon.fill = Some(0.5);
        icon.weight = Some(400.0);
        assert!(icon.axes_are_valid());

        icon.fill = Some(1.5);
        assert!(!icon.axes_are_valid());

        icon.fill = Some(0.5);
        icon.weight = Some(0.0);
        assert!(!icon.axes_are_valid());
    }

    #[test]
    fn an_icon_has_no_semantic_label_by_default() {
        // Most sit next to a label that already says what they are, and
        // announcing both would say it twice.
        assert!(!Icon::new().has_semantic_label);
    }
}

#[cfg(test)]
mod icon_theme_tests {
    use super::*;
    use crate::component_themes::{IconTheme, IconThemeData, ResolvedIcon};
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component};

    struct Reader {
        icon: Icon,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedIcon>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.icon.resolved(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(icon: Icon, data: IconThemeData) -> ResolvedIcon {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(IconTheme::new(
            data,
            component(Reader {
                icon,
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// No theme installed at all, which is the case the two defaults differ in.
    fn resolve_bare(icon: Icon) -> ResolvedIcon {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader {
            icon,
            seen: std::rc::Rc::clone(&seen),
        }));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn a_theme_says_twenty_four_and_no_theme_at_all_says_fourteen() {
        // Not an oversight. An icon with nothing around it to belong to is a
        // glyph in a line of type, and fourteen is what a glyph is; twenty-four
        // is the Material icon size, which is a thing a *theme* knows.
        let mut data = IconThemeData::new();
        data.size = Some(ResolvedIcon::THEME_SIZE);
        assert_eq!(resolve(Icon::new(), data).size, 24.0);
        assert_eq!(
            resolve_bare(Icon::new()).size,
            ResolvedIcon::DEFAULT_FONT_SIZE
        );
        assert_eq!(ResolvedIcon::DEFAULT_FONT_SIZE, 14.0);
    }

    #[test]
    fn the_icons_own_size_beats_the_themes() {
        let mut data = IconThemeData::new();
        data.size = Some(24.0);
        let mut icon = Icon::new();
        icon.size = Some(48.0);
        assert_eq!(resolve(icon, data).size, 48.0);
    }

    #[test]
    fn an_icon_does_not_grow_with_the_text_unless_it_is_told_to() {
        // An icon in a sentence should follow the reader's text size; an icon
        // that is a button should not, because the button around it is a fixed
        // target and a growing glyph would burst it.
        let mut data = IconThemeData::new();
        data.size = Some(20.0);
        assert!(!resolve(Icon::new(), data.clone()).apply_text_scaling);
        assert_eq!(resolve(Icon::new(), data.clone()).size, 20.0);

        // Under a real scale, because at the default of 1.0 a scaled size and
        // an unscaled one are the same number and the test would pass either
        // way.
        crate::media_query::with_text_scale(2.0, || {
            let mut icon = Icon::new();
            icon.apply_text_scaling = Some(true);
            let scaled = resolve(icon, data.clone());
            assert!(scaled.apply_text_scaling);
            assert_eq!(
                scaled.size, 40.0,
                "the tentative twenty, through the scaler"
            );

            assert_eq!(
                resolve(Icon::new(), data.clone()).size,
                20.0,
                "and an icon that did not ask is left alone"
            );
        });

        // And the theme can ask for it on everything below it.
        data.apply_text_scaling = Some(true);
        assert!(resolve(Icon::new(), data).apply_text_scaling);
    }

    #[test]
    fn the_opacity_applies_to_whichever_colour_came_out() {
        // Which is why it is not a colour of its own: it dims the icon's own
        // colour and the theme's alike.
        let mut data = IconThemeData::new();
        data.color = Some(Color::argb(0xFF, 1, 2, 3));
        let data = data.with_opacity(0.5);
        assert_eq!(resolve(Icon::new(), data.clone()).color.alpha(), 128);

        let mut icon = Icon::new();
        icon.color = Some(Color::argb(0xFF, 9, 9, 9));
        let mine = resolve(icon, data);
        assert_eq!(mine.color.red(), 9, "my colour");
        assert_eq!(mine.color.alpha(), 128, "and the theme's opacity over it");
    }

    #[test]
    fn an_icon_with_no_colour_anywhere_is_black() {
        assert_eq!(
            resolve_bare(Icon::new()).color,
            Color::argb(0xFF, 0, 0, 0),
            "upstream's IconThemeData.fallback colour"
        );
    }

    #[test]
    fn the_variable_font_axes_fall_back_to_upstreams_fallback_values() {
        let bare = resolve_bare(Icon::new());
        assert_eq!(bare.fill, 0.0);
        assert_eq!(bare.weight, 400.0);
        assert_eq!(bare.grade, 0.0);
        assert_eq!(bare.optical_size, 48.0);
    }

    #[test]
    fn every_axis_prefers_the_icons_own_value_over_the_themes() {
        // Each axis is `icon.x.or(theme.x)`, and with only one side set the
        // direction cannot be seen. Set on both, on every axis at once.
        let mut data = IconThemeData::new();
        data.size = Some(2.0);
        data.fill = Some(0.2);
        data.weight = Some(200.0);
        data.grade = Some(20.0);
        data.optical_size = Some(22.0);
        data.color = Some(Color::argb(0xFF, 2, 2, 2));

        let mut icon = Icon::new();
        icon.size = Some(1.0);
        icon.fill = Some(0.1);
        icon.weight = Some(100.0);
        icon.grade = Some(10.0);
        icon.optical_size = Some(11.0);
        icon.color = Some(Color::argb(0xFF, 1, 1, 1));

        let resolved = resolve(icon, data);
        assert_eq!(resolved.size, 1.0);
        assert_eq!(resolved.fill, 0.1);
        assert_eq!(resolved.weight, 100.0);
        assert_eq!(resolved.grade, 10.0);
        assert_eq!(resolved.optical_size, 11.0);
        assert_eq!(resolved.color, Color::argb(0xFF, 1, 1, 1));
    }

    #[test]
    fn each_axis_is_its_own_three_step_chain() {
        let mut data = IconThemeData::new();
        data.weight = Some(700.0);
        data.grade = Some(200.0);
        let mut icon = Icon::new();
        icon.weight = Some(300.0);
        let resolved = resolve(icon, data);
        assert_eq!(resolved.weight, 300.0, "the icon's");
        assert_eq!(resolved.grade, 200.0, "the theme's");
        assert_eq!(resolved.fill, 0.0, "and the fallback for what neither set");
    }

    #[test]
    fn the_axes_have_ranges_and_a_number_outside_them_is_refused() {
        // Variable-font axes with real ranges, not free numbers.
        let mut fill = Icon::new();
        fill.fill = Some(1.5);
        assert!(!fill.axes_are_valid());

        let mut weight = Icon::new();
        weight.weight = Some(0.0);
        assert!(!weight.axes_are_valid());

        let mut fine = Icon::new();
        fine.fill = Some(1.0);
        fine.weight = Some(1.0);
        assert!(fine.axes_are_valid());
    }
}
