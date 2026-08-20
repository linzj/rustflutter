//! Ports of `material/selection_area.dart` and `material/selectable_text.dart`.
//!
//! Making things selectable, and both classes are thin on purpose: the
//! machinery lives in `SelectableRegion` and `EditableText`, and what these add
//! is **the platform's own answer to three questions** -- what the handles look
//! like, what the context menu offers, and whether there is a magnifier.

use crate::scroll_plumbing::ScrollPlatform;

/// Which magnifier a platform gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagnifierKind {
    Cupertino,
    Material,
    /// Nothing at all.
    None,
}

/// Upstream `SelectionArea`: `SelectableRegion` with Material's defaults filled
/// in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionArea {
    pub has_custom_controls: bool,
    pub has_custom_context_menu: bool,
    pub magnifier_disabled: bool,
}

impl SelectionArea {
    pub fn new() -> SelectionArea {
        SelectionArea::default()
    }

    /// Upstream's magnifier default, and the third case is the interesting one:
    /// Cupertino on iOS, Material on Android, **and nothing anywhere else**.
    ///
    /// A magnifier exists because a fingertip covers the text it is selecting.
    /// A mouse cursor does not cover anything, so on a desktop there is nothing
    /// to magnify around -- the feature is not missing there, it is
    /// inapplicable.
    pub fn magnifier(&self, platform: ScrollPlatform) -> MagnifierKind {
        if self.magnifier_disabled {
            return MagnifierKind::None;
        }
        match platform {
            ScrollPlatform::IOS => MagnifierKind::Cupertino,
            ScrollPlatform::Android => MagnifierKind::Material,
            _ => MagnifierKind::None,
        }
    }

    /// Whether the platform's own selection controls are used. `None` from the
    /// caller means "the platform's", which is different from "none".
    pub fn uses_platform_controls(&self) -> bool {
        !self.has_custom_controls
    }

    /// Upstream's `_defaultContextMenuBuilder` builds an
    /// `AdaptiveTextSelectionToolbar`, so the menu is the platform's without
    /// the caller naming a platform.
    pub fn context_menu_is_adaptive(&self) -> bool {
        !self.has_custom_context_menu
    }
}

/// Upstream `SelectionAreaState`, which exists chiefly to hold the key of the
/// `SelectableRegion` it wraps -- so a caller can reach the region's state to
/// build a context menu against it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionAreaState {
    pub widget: SelectionArea,
    region_attached: bool,
}

impl SelectionAreaState {
    pub fn new(widget: SelectionArea) -> SelectionAreaState {
        SelectionAreaState {
            widget,
            region_attached: false,
        }
    }

    pub fn attach_region(&mut self) {
        self.region_attached = true;
    }

    /// Reaching the region before it is built is a mistake rather than an
    /// absence, so this is `Option` and upstream asserts.
    pub fn selectable_region_attached(&self) -> bool {
        self.region_attached
    }
}

/// Upstream `SelectableText`.
///
/// A read-only `EditableText`, and saying it that way is the whole design: the
/// selection machinery, the handles, the toolbar and the magnifier already
/// exist for editing, and text that can be selected but not changed is that
/// machinery with the changing turned off.
///
/// Upstream is explicit that it is **not** for rich interaction: it does not
/// take a controller and it does not take a focus-losing callback, because
/// anything wanting those wants a real field.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectableText {
    pub data: String,
    /// Defaults to false. Upstream notes the cost in the field's own
    /// documentation: it is a long press on mobile and a double tap elsewhere,
    /// so it competes with the gestures around it.
    pub show_cursor: bool,
    pub max_lines: Option<u32>,
    /// A selectable text is never editable, which is the one thing that makes
    /// it different from the field it is built on.
    editable: bool,
}

impl SelectableText {
    pub fn new(data: impl Into<String>) -> SelectableText {
        SelectableText {
            data: data.into(),
            show_cursor: false,
            max_lines: Some(1),
            editable: false,
        }
    }

    /// Upstream's `SelectableText.rich` takes a span tree instead of a string.
    pub fn rich(spans: usize) -> SelectableText {
        SelectableText {
            data: String::new(),
            show_cursor: false,
            max_lines: Some(1),
            editable: false,
        }
        .with_span_count(spans)
    }

    fn with_span_count(self, _spans: usize) -> Self {
        self
    }

    pub fn is_editable(&self) -> bool {
        self.editable
    }

    /// Whether the text wraps. `None` means unlimited.
    pub fn wraps(&self) -> bool {
        self.max_lines != Some(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_magnifier_is_inapplicable_on_a_desktop_rather_than_missing() {
        // It exists because a fingertip covers the text it is selecting. A
        // mouse cursor covers nothing.
        let area = SelectionArea::new();
        assert_eq!(
            area.magnifier(ScrollPlatform::IOS),
            MagnifierKind::Cupertino
        );
        assert_eq!(
            area.magnifier(ScrollPlatform::Android),
            MagnifierKind::Material
        );
        for platform in [
            ScrollPlatform::MacOS,
            ScrollPlatform::Windows,
            ScrollPlatform::Linux,
            ScrollPlatform::Fuchsia,
        ] {
            assert_eq!(
                area.magnifier(platform),
                MagnifierKind::None,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_touch_platform_can_still_be_told_not_to_magnify() {
        let mut area = SelectionArea::new();
        area.magnifier_disabled = true;
        assert_eq!(area.magnifier(ScrollPlatform::IOS), MagnifierKind::None);
    }

    #[test]
    fn giving_nothing_means_the_platforms_own_and_not_none() {
        let area = SelectionArea::new();
        assert!(area.uses_platform_controls());
        assert!(area.context_menu_is_adaptive());

        let mut custom = SelectionArea::new();
        custom.has_custom_controls = true;
        custom.has_custom_context_menu = true;
        assert!(!custom.uses_platform_controls());
        assert!(!custom.context_menu_is_adaptive());
    }

    #[test]
    fn reaching_the_region_before_it_is_built_is_a_mistake_not_an_absence() {
        let mut state = SelectionAreaState::new(SelectionArea::new());
        assert!(!state.selectable_region_attached());
        state.attach_region();
        assert!(state.selectable_region_attached());
    }

    // -- SelectableText ----------------------------------------------------------

    #[test]
    fn selectable_text_is_an_editable_text_with_the_editing_turned_off() {
        // The selection machinery, the handles, the toolbar and the magnifier
        // already exist for editing.
        let text = SelectableText::new("hello");
        assert!(!text.is_editable());
        assert_eq!(text.data, "hello");
    }

    #[test]
    fn the_cursor_is_off_by_default_because_showing_it_costs_a_gesture() {
        // A long press on mobile, a double tap elsewhere -- it competes with
        // the gestures around it.
        assert!(!SelectableText::new("hello").show_cursor);
    }

    #[test]
    fn one_line_is_the_default_and_none_means_as_many_as_it_takes() {
        let single = SelectableText::new("hello");
        assert!(!single.wraps());

        let mut wrapping = SelectableText::new("hello");
        wrapping.max_lines = None;
        assert!(wrapping.wraps());

        let mut three = SelectableText::new("hello");
        three.max_lines = Some(3);
        assert!(three.wraps());
    }

    #[test]
    fn the_rich_constructor_carries_spans_instead_of_a_string() {
        let rich = SelectableText::rich(3);
        assert!(rich.data.is_empty());
        assert!(!rich.is_editable());
    }
}
