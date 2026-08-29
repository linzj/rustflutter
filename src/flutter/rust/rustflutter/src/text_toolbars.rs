//! Ports of `material/adaptive_text_selection_toolbar.dart`,
//! `material/spell_check_suggestions_toolbar.dart` and
//! `material/spell_check_suggestions_toolbar_layout_delegate.dart`.
//!
//! The little bars that appear next to text you have selected, and where each
//! decides to put itself.

use std::rc::Rc;

use crate::text_selection::TextSelectionToolbarLayoutDelegate;

/// Upstream's `_kToolbarHeight` (`material/text_selection_toolbar.dart`), and
/// half of it is the corner radius -- "eyeballed to match the native text
/// selection menu on a Pixel 6 emulator", says the line it came from.
pub const TOOLBAR_HEIGHT: f32 = 44.0;

/// Upstream's `_kToolbarContentDistance`: how far above the selection the bar
/// sits when it goes above.
pub const TOOLBAR_CONTENT_DISTANCE: f32 = 8.0;

/// Upstream `TextSelectionToolbarTextButton._kMiddlePadding`, "eyeballed to
/// match the native text selection menu on a Pixel 2 running Android 10".
///
/// The ends of the bar are padded wider than the gaps between its buttons,
/// which is what stops the first label sitting hard against the rounded
/// corner. The two are cited separately rather than in one sentence over both
/// so that each is checked against its own upstream name: one doc block naming
/// both leaves a ruler able to say only that 9.5 is *one of* the two numbers,
/// which a swapped pair would also satisfy.
pub const BUTTON_MIDDLE_PADDING: f32 = 9.5;

/// Upstream `TextSelectionToolbarTextButton._kEndPadding`.
pub const BUTTON_END_PADDING: f32 = 14.5;

/// The padding for the button at `index` of `total` -- upstream's
/// `TextSelectionToolbarTextButton.getPadding`, whose whole content is that
/// the outside edges are wider than the inside ones.
pub fn button_padding(index: usize, total: usize) -> (f32, f32) {
    debug_assert!(total > 0 && index < total);
    let first = index == 0;
    let last = index + 1 == total;
    (
        if first {
            BUTTON_END_PADDING
        } else {
            BUTTON_MIDDLE_PADDING
        },
        if last {
            BUTTON_END_PADDING
        } else {
            BUTTON_MIDDLE_PADDING
        },
    )
}

/// One command in a selection toolbar: its label and what pressing it does.
///
/// Upstream's `ContextMenuButtonItem`, minus the `type` -- the type there
/// exists to look a label up in the localizations, and the label is already
/// resolved by the time it reaches this.
pub struct ToolbarButton {
    pub label: String,
    /// The hit id the button's pointer region answers to.
    pub id: u64,
    pub on_pressed: Rc<dyn Fn()>,
}

impl ToolbarButton {
    pub fn new(id: u64, label: impl Into<String>, on_pressed: Rc<dyn Fn()>) -> ToolbarButton {
        ToolbarButton {
            label: label.into(),
            id,
            on_pressed,
        }
    }
}

/// Upstream `TextSelectionToolbar`: the Material bar of text buttons that
/// appears beside a selection.
///
/// What is here is the bar itself -- the rounded card and the row of labels.
/// *Where* it goes is [`TextSelectionToolbarLayoutDelegate`]'s decision, made
/// by [`crate::selection_host::SelectionHost::place_toolbar`], and the
/// overflow menu upstream grows when the buttons do not fit is not ported:
/// the four commands a text field offers have always fitted.
pub fn material_selection_toolbar(
    buttons: Vec<ToolbarButton>,
    surface: crate::engine::Color,
    ink: crate::engine::Color,
    label_style: crate::engine::TextStyle,
) -> crate::framework::AnyWidget {
    use crate::borders::{BorderRadius, Radius};
    use crate::framework::{AnyWidget, leaf, many};
    use crate::render::{CrossAxisAlignment, EdgeInsets, MainAxisSize, RenderFlex};
    use crate::widgets::{Container, Pointer, Text};

    let total = buttons.len();
    let children: Vec<AnyWidget> = buttons
        .into_iter()
        .enumerate()
        .map(|(index, button)| {
            let (start, end) = button_padding(index, total);
            let mut style = label_style.clone();
            style.color = ink;
            leaf(move || {
                Pointer::new(
                    button.id,
                    Container::new()
                        .with_height(TOOLBAR_HEIGHT)
                        .with_padding(EdgeInsets::only(start, 0.0, end, 0.0))
                        .with_child(crate::widgets::Align::new(
                            crate::render::Alignment::CENTER,
                            Text::new(button.label.clone()).with_style(style.clone()),
                        )),
                )
                .with_handlers({
                    let on_pressed = Rc::clone(&button.on_pressed);
                    crate::gestures::PointerHandlers::new().with_tap(move |_| on_pressed())
                })
            })
        })
        .collect();

    many(children, move |rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        for child in rendered {
            row = row.push(child);
        }
        Box::new(
            Container::new()
                .with_color(surface)
                // Upstream's radius is half the bar's height, so the ends are
                // semicircles rather than rounded corners.
                .with_border_radius(BorderRadius::circular(TOOLBAR_HEIGHT / 2.0))
                // Upstream's `elevation: 1.0` on a `MaterialType.card`.
                .with_elevation(1)
                .with_child(row),
        )
    })
}

/// The platforms these toolbars distinguish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarPlatform {
    Android,
    Fuchsia,
    IOS,
    Linux,
    MacOS,
    Windows,
}

/// Which button widget `getAdaptiveButtons` produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarButtonStyle {
    /// `CupertinoTextSelectionToolbarButton`.
    Cupertino,
    /// `TextSelectionToolbarTextButton`, the Material one.
    Material,
    /// `DesktopTextSelectionToolbarButton`.
    Desktop,
    /// `CupertinoDesktopTextSelectionToolbarButton`.
    CupertinoDesktop,
}

/// Which toolbar `build` wraps them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarChrome {
    CupertinoTextSelectionToolbar,
    TextSelectionToolbar,
    DesktopTextSelectionToolbar,
    CupertinoDesktopTextSelectionToolbar,
    /// Upstream's `SizedBox.shrink()`.
    Empty,
}

/// Upstream `AdaptiveTextSelectionToolbar`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveTextSelectionToolbar {
    /// `None` means the caller passed no `children` list at all.
    pub child_count: Option<usize>,
    /// `None` means no `buttonItems` list at all.
    pub button_item_count: Option<usize>,
    pub has_secondary_anchor: bool,
}

impl AdaptiveTextSelectionToolbar {
    pub fn new(button_item_count: usize) -> AdaptiveTextSelectionToolbar {
        AdaptiveTextSelectionToolbar {
            child_count: None,
            button_item_count: Some(button_item_count),
            has_secondary_anchor: false,
        }
    }

    /// Upstream `getAdaptiveButtons`' platform switch.
    ///
    /// Note where Fuchsia lands: `case TargetPlatform.fuchsia: case
    /// TargetPlatform.android:` -- **with Android**, taking the Material text
    /// buttons.
    pub fn button_style(platform: ToolbarPlatform) -> ToolbarButtonStyle {
        match platform {
            ToolbarPlatform::IOS => ToolbarButtonStyle::Cupertino,
            ToolbarPlatform::Fuchsia | ToolbarPlatform::Android => ToolbarButtonStyle::Material,
            ToolbarPlatform::Linux | ToolbarPlatform::Windows => ToolbarButtonStyle::Desktop,
            ToolbarPlatform::MacOS => ToolbarButtonStyle::CupertinoDesktop,
        }
    }

    /// Upstream `build`'s platform switch, and **Fuchsia has moved.**
    ///
    /// Here it is `case fuchsia: case linux: case windows:` -- grouped with the
    /// desktops, while Android takes `TextSelectionToolbar` on its own. So a
    /// Fuchsia context menu gets **Android's buttons inside a desktop
    /// toolbar**, and the two switches in this one class cut the six platforms
    /// in different places.
    ///
    /// The previous tick found that happening between two files -- an icon set
    /// splitting Apple from the rest while a scroll behaviour split desktop from
    /// touch -- with the moral that each question draws its own line. This is
    /// the same thing between two methods of one class, which is a harder place
    /// to be sure it was meant: buttons and chrome are genuinely separate
    /// questions, and Fuchsia plausibly wants Material styling in a desktop
    /// frame, but nothing here says so.
    ///
    /// Ported as it behaves, with the disagreement pinned rather than smoothed.
    pub fn chrome(&self, platform: ToolbarPlatform) -> ToolbarChrome {
        if self.is_empty() {
            return ToolbarChrome::Empty;
        }
        match platform {
            ToolbarPlatform::IOS => ToolbarChrome::CupertinoTextSelectionToolbar,
            ToolbarPlatform::Android => ToolbarChrome::TextSelectionToolbar,
            ToolbarPlatform::Fuchsia | ToolbarPlatform::Linux | ToolbarPlatform::Windows => {
                ToolbarChrome::DesktopTextSelectionToolbar
            }
            ToolbarPlatform::MacOS => ToolbarChrome::CupertinoDesktopTextSelectionToolbar,
        }
    }

    /// Upstream's opening line: `if ((children ?? buttonItems)?.isEmpty ?? true)`.
    ///
    /// Read the fallbacks in order: `children` if there is one, otherwise
    /// `buttonItems`; if neither exists the `?.` yields null and the `?? true`
    /// calls that empty. **Nothing to show is a small empty box, not an
    /// error** -- a context menu with no applicable actions is an ordinary
    /// thing, and it simply does not appear.
    pub fn is_empty(&self) -> bool {
        match self.child_count.or(self.button_item_count) {
            Some(count) => count == 0,
            None => true,
        }
    }

    /// Upstream passes `anchorBelow: anchors.secondaryAnchor ?? anchors.primaryAnchor`
    /// to the two toolbars that take both, so a single-anchor selection is
    /// described as one whose above and below are the same point.
    pub fn anchor_below_falls_back_to_primary(&self) -> bool {
        !self.has_secondary_anchor
    }
}

/// Upstream `SpellCheckSuggestionsToolbarLayoutDelegate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpellCheckSuggestionsToolbarLayoutDelegate {
    /// In local coordinates, per the field's own doc.
    pub anchor: (f32, f32),
}

impl SpellCheckSuggestionsToolbarLayoutDelegate {
    pub fn new(anchor: (f32, f32)) -> SpellCheckSuggestionsToolbarLayoutDelegate {
        SpellCheckSuggestionsToolbarLayoutDelegate { anchor }
    }

    /// Upstream `getPositionForChild`.
    ///
    /// The horizontal half is shared with the general delegate --
    /// [`TextSelectionToolbarLayoutDelegate::center_on`] -- and the vertical
    /// half is where this one differs from every other toolbar in the
    /// framework:
    ///
    /// ```dart
    /// anchor.dy + childSize.height > size.height ? size.height - childSize.height : anchor.dy
    /// ```
    ///
    /// **It slides, it does not flip.** The general text selection toolbar has
    /// an anchor above and an anchor below and jumps between them when it runs
    /// out of room. This one only ever goes below the misspelled word, because
    /// above would cover the very word you are looking at, so when it does not
    /// fit it moves up by exactly as much as it must and no further.
    ///
    /// And there is no lower bound on the result. A toolbar taller than the
    /// space available gives a negative y and hangs off the top -- upstream
    /// clamps nothing here, on the reasoning that the alternative (covering the
    /// word) is worse than overflowing.
    pub fn position_for_child(&self, size: (f32, f32), child_size: (f32, f32)) -> (f32, f32) {
        let x = TextSelectionToolbarLayoutDelegate::center_on(self.anchor.0, child_size.0, size.0);
        let y = if self.anchor.1 + child_size.1 > size.1 {
            size.1 - child_size.1
        } else {
            self.anchor.1
        };
        (x, y)
    }

    /// Upstream `getConstraintsForChild` returns `constraints.loosen()`: the
    /// toolbar may be any size up to the space available, with no minimum
    /// forced on it.
    pub fn loosens_constraints() -> bool {
        true
    }

    pub fn should_relayout(&self, old: &SpellCheckSuggestionsToolbarLayoutDelegate) -> bool {
        self.anchor != old.anchor
    }
}

/// Upstream `SpellCheckSuggestionsToolbar`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpellCheckSuggestionsToolbar {
    pub anchor: (f32, f32),
    pub suggestion_count: usize,
}

impl SpellCheckSuggestionsToolbar {
    /// Upstream's `_kMaxSuggestions`: at most three spelling suggestions are
    /// offered, however many the engine returned.
    pub const MAX_SUGGESTIONS: usize = 3;

    /// Upstream's `_kDefaultToolbarHeight`, and the row height it counts in.
    ///
    /// **193 is 4 x 48 + 1.** The height is computed as
    /// `_kDefaultToolbarHeight - (48.0 * (4 - buttonItems.length))`, so the bar
    /// is described as a full four-row toolbar with the missing rows taken off
    /// rather than as the rows it has -- and the stray pixel rides along at
    /// every size. Three items come to 145, which is 3 x 48 + 1.
    ///
    /// Another of the numbers nobody has explained. Kept as upstream has it,
    /// since a bar one pixel shorter would be a visible change nobody asked
    /// for.
    pub const DEFAULT_TOOLBAR_HEIGHT: f32 = 193.0;
    pub const ROW_HEIGHT: f32 = 48.0;
    /// Upstream's `assert(buttonItems.length <= _kMaxSuggestions + 1)`.
    ///
    /// Written as `3 + 1` rather than `4`, which says out loud that **the fourth
    /// item is not a suggestion** -- it is the delete button that sits under
    /// them.
    pub const MAX_BUTTON_ITEMS: usize = SpellCheckSuggestionsToolbar::MAX_SUGGESTIONS + 1;

    pub fn new(anchor: (f32, f32), suggestion_count: usize) -> SpellCheckSuggestionsToolbar {
        SpellCheckSuggestionsToolbar {
            anchor,
            suggestion_count,
        }
    }

    /// How many suggestion buttons are built.
    pub fn shown_suggestions(&self) -> usize {
        self.suggestion_count
            .min(SpellCheckSuggestionsToolbar::MAX_SUGGESTIONS)
    }

    /// Upstream's height arithmetic, counting down from the four-row constant.
    pub fn height(&self, button_item_count: usize) -> f32 {
        SpellCheckSuggestionsToolbar::DEFAULT_TOOLBAR_HEIGHT
            - SpellCheckSuggestionsToolbar::ROW_HEIGHT
                * (SpellCheckSuggestionsToolbar::MAX_BUTTON_ITEMS as f32 - button_item_count as f32)
    }

    /// Upstream's constructor assert.
    pub fn accepts_button_items(count: usize) -> bool {
        count <= SpellCheckSuggestionsToolbar::MAX_BUTTON_ITEMS
    }

    /// The delegate this toolbar positions itself with.
    pub fn layout_delegate(&self) -> SpellCheckSuggestionsToolbarLayoutDelegate {
        SpellCheckSuggestionsToolbarLayoutDelegate::new(self.anchor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ToolbarPlatform; 6] = [
        ToolbarPlatform::Android,
        ToolbarPlatform::Fuchsia,
        ToolbarPlatform::IOS,
        ToolbarPlatform::Linux,
        ToolbarPlatform::MacOS,
        ToolbarPlatform::Windows,
    ];

    // -- Fuchsia is on two different sides of one class ----------------------------

    #[test]
    fn fuchsia_takes_androids_buttons_and_the_desktops_toolbar() {
        let toolbar = AdaptiveTextSelectionToolbar::new(2);
        assert_eq!(
            AdaptiveTextSelectionToolbar::button_style(ToolbarPlatform::Fuchsia),
            AdaptiveTextSelectionToolbar::button_style(ToolbarPlatform::Android),
            "grouped with Android in getAdaptiveButtons"
        );
        assert_eq!(
            toolbar.chrome(ToolbarPlatform::Fuchsia),
            toolbar.chrome(ToolbarPlatform::Linux),
            "and with the desktops in build"
        );
        assert_ne!(
            toolbar.chrome(ToolbarPlatform::Fuchsia),
            toolbar.chrome(ToolbarPlatform::Android),
            "so the two switches disagree about it"
        );
    }

    #[test]
    fn every_other_platform_is_consistent_between_the_two_switches() {
        // Which is what makes Fuchsia worth pointing at rather than shrugging
        // about: it is the only one that moves.
        let toolbar = AdaptiveTextSelectionToolbar::new(2);
        let paired = |platform| {
            (
                AdaptiveTextSelectionToolbar::button_style(platform),
                toolbar.chrome(platform),
            )
        };
        assert_eq!(
            paired(ToolbarPlatform::IOS),
            (
                ToolbarButtonStyle::Cupertino,
                ToolbarChrome::CupertinoTextSelectionToolbar
            )
        );
        assert_eq!(
            paired(ToolbarPlatform::MacOS),
            (
                ToolbarButtonStyle::CupertinoDesktop,
                ToolbarChrome::CupertinoDesktopTextSelectionToolbar
            )
        );
        assert_eq!(
            paired(ToolbarPlatform::Windows),
            (
                ToolbarButtonStyle::Desktop,
                ToolbarChrome::DesktopTextSelectionToolbar
            )
        );
        assert_eq!(
            paired(ToolbarPlatform::Android),
            (
                ToolbarButtonStyle::Material,
                ToolbarChrome::TextSelectionToolbar
            )
        );
    }

    #[test]
    fn every_platform_gets_both_a_button_style_and_a_chrome() {
        let toolbar = AdaptiveTextSelectionToolbar::new(1);
        for platform in ALL {
            assert_ne!(
                toolbar.chrome(platform),
                ToolbarChrome::Empty,
                "{platform:?}"
            );
        }
    }

    // -- Nothing to show is a small box ---------------------------------------------

    #[test]
    fn a_context_menu_with_no_applicable_actions_simply_does_not_appear() {
        let empty = AdaptiveTextSelectionToolbar::new(0);
        assert!(empty.is_empty());
        assert_eq!(empty.chrome(ToolbarPlatform::Android), ToolbarChrome::Empty);
    }

    #[test]
    fn neither_list_at_all_counts_as_empty_rather_than_as_an_error() {
        let nothing = AdaptiveTextSelectionToolbar {
            child_count: None,
            button_item_count: None,
            has_secondary_anchor: false,
        };
        assert!(nothing.is_empty());
    }

    #[test]
    fn children_are_consulted_before_button_items() {
        let mut toolbar = AdaptiveTextSelectionToolbar::new(3);
        toolbar.child_count = Some(0);
        assert!(
            toolbar.is_empty(),
            "an empty children list wins over a full buttonItems one"
        );

        toolbar.child_count = Some(2);
        assert!(!toolbar.is_empty());
    }

    // -- It slides, it does not flip --------------------------------------------------

    #[test]
    fn the_spelling_toolbar_sits_below_the_word_when_there_is_room() {
        let delegate = SpellCheckSuggestionsToolbarLayoutDelegate::new((100.0, 200.0));
        let (_, y) = delegate.position_for_child((300.0, 600.0), (150.0, 120.0));
        assert_eq!(y, 200.0, "the anchor, untouched");
    }

    #[test]
    fn and_moves_up_by_exactly_as_much_as_it_must_rather_than_jumping_above() {
        // The general toolbar flips to an anchor above; this one would then be
        // covering the misspelled word.
        let delegate = SpellCheckSuggestionsToolbarLayoutDelegate::new((100.0, 520.0));
        let (_, y) = delegate.position_for_child((300.0, 600.0), (150.0, 120.0));
        assert_eq!(y, 480.0);
        assert_eq!(y + 120.0, 600.0, "flush with the bottom and no higher");
        assert!(y < 520.0, "moved up");
        assert!(y > 520.0 - 120.0, "but nowhere near above the anchor");
    }

    #[test]
    fn a_toolbar_taller_than_the_space_hangs_off_the_top_unclamped() {
        // Upstream bounds nothing here: overflowing beats covering the word.
        let delegate = SpellCheckSuggestionsToolbarLayoutDelegate::new((100.0, 50.0));
        let (_, y) = delegate.position_for_child((300.0, 100.0), (150.0, 260.0));
        assert_eq!(y, -160.0);
    }

    #[test]
    fn the_horizontal_half_is_the_general_delegates() {
        let delegate = SpellCheckSuggestionsToolbarLayoutDelegate::new((150.0, 10.0));
        let (x, _) = delegate.position_for_child((300.0, 600.0), (100.0, 40.0));
        assert_eq!(
            x,
            TextSelectionToolbarLayoutDelegate::center_on(150.0, 100.0, 300.0)
        );
    }

    #[test]
    fn only_a_moved_anchor_forces_a_relayout() {
        let delegate = SpellCheckSuggestionsToolbarLayoutDelegate::new((10.0, 20.0));
        assert!(
            !delegate.should_relayout(&SpellCheckSuggestionsToolbarLayoutDelegate::new((
                10.0, 20.0
            )))
        );
        assert!(
            delegate.should_relayout(&SpellCheckSuggestionsToolbarLayoutDelegate::new((
                10.0, 21.0
            )))
        );
    }

    // -- Three at most ----------------------------------------------------------------

    #[test]
    fn at_most_three_spellings_are_offered_however_many_came_back() {
        assert_eq!(
            SpellCheckSuggestionsToolbar::new((0.0, 0.0), 9).shown_suggestions(),
            3
        );
        assert_eq!(
            SpellCheckSuggestionsToolbar::new((0.0, 0.0), 2).shown_suggestions(),
            2
        );
        assert_eq!(
            SpellCheckSuggestionsToolbar::new((0.0, 0.0), 0).shown_suggestions(),
            0
        );
    }

    #[test]
    fn the_fourth_item_is_not_a_suggestion() {
        // The assert is written 3 + 1, and the extra one is the delete button.
        assert!(SpellCheckSuggestionsToolbar::accepts_button_items(4));
        assert!(!SpellCheckSuggestionsToolbar::accepts_button_items(5));
        assert_eq!(
            SpellCheckSuggestionsToolbar::MAX_BUTTON_ITEMS,
            SpellCheckSuggestionsToolbar::MAX_SUGGESTIONS + 1
        );
    }

    #[test]
    fn the_bar_is_one_pixel_taller_than_its_rows_at_every_size() {
        let toolbar = SpellCheckSuggestionsToolbar::new((0.0, 0.0), 3);
        for count in 1..=4 {
            let height = toolbar.height(count);
            assert_eq!(
                height,
                SpellCheckSuggestionsToolbar::ROW_HEIGHT * count as f32 + 1.0,
                "{count} rows"
            );
        }
        assert_eq!(toolbar.height(4), 193.0);
        assert_eq!(toolbar.height(3), 145.0);
    }

    #[test]
    fn the_height_is_a_full_bar_with_the_missing_rows_taken_off() {
        // Which is the same answer as counting up, and not the same framing:
        // the constant it counts down from is a four-row bar.
        let toolbar = SpellCheckSuggestionsToolbar::new((0.0, 0.0), 1);
        assert_eq!(
            toolbar.height(SpellCheckSuggestionsToolbar::MAX_BUTTON_ITEMS),
            SpellCheckSuggestionsToolbar::DEFAULT_TOOLBAR_HEIGHT
        );
    }

    #[test]
    fn the_toolbar_hands_its_own_anchor_to_its_delegate() {
        let toolbar = SpellCheckSuggestionsToolbar::new((42.0, 84.0), 2);
        assert_eq!(toolbar.layout_delegate().anchor, (42.0, 84.0));
    }

    #[test]
    fn the_bars_ends_are_padded_wider_than_the_gaps_between_its_buttons() {
        // Upstream's `getPadding`, whose whole content this is: the outside
        // edges get `_kEndPadding` and every inside edge `_kMiddlePadding`,
        // which is what stops the first label sitting hard against the
        // rounded corner.
        assert_eq!(
            button_padding(0, 3),
            (BUTTON_END_PADDING, BUTTON_MIDDLE_PADDING)
        );
        assert_eq!(
            button_padding(1, 3),
            (BUTTON_MIDDLE_PADDING, BUTTON_MIDDLE_PADDING)
        );
        assert_eq!(
            button_padding(2, 3),
            (BUTTON_MIDDLE_PADDING, BUTTON_END_PADDING)
        );
    }

    #[test]
    fn a_lone_button_is_padded_at_both_ends() {
        // Upstream's `_TextSelectionToolbarItemPosition.only`, which is a
        // fourth case rather than "first and last happening to coincide" --
        // and it is reached by the single-button bar a long press on empty
        // text puts up, with nothing on it but Paste.
        assert_eq!(
            button_padding(0, 1),
            (BUTTON_END_PADDING, BUTTON_END_PADDING)
        );
    }
}
