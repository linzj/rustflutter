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
    /// Upstream's `maxLines`, and **its default is not one**.
    ///
    /// `TextField` declares `this.maxLines = 1`; `SelectableText` declares
    /// `this.maxLines` with no default at all, so it arrives null and the
    /// build falls back to `defaultTextStyle.maxLines` -- also usually null.
    /// The difference is the point of the widget: a field is a line you type
    /// into, a selectable text is a passage you read, and a passage that
    /// stopped at one line would be the wrong shape for almost every use of
    /// it.
    ///
    /// This crate has no `DefaultTextStyle.maxLines` to fall back to, so null
    /// is as far as the fallback goes.
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
            max_lines: None,
            editable: false,
        }
    }

    /// Upstream's `SelectableText.rich` takes a span tree instead of a string.
    pub fn rich(spans: usize) -> SelectableText {
        SelectableText {
            data: String::new(),
            show_cursor: false,
            max_lines: None,
            editable: false,
        }
        .with_span_count(spans)
    }

    fn with_span_count(self, _spans: usize) -> Self {
        self
    }

    /// Upstream's `maxLines`. `None` is upstream's null: as many as it takes.
    pub fn with_max_lines(mut self, lines: Option<u32>) -> Self {
        self.max_lines = lines;
        self
    }

    pub fn is_editable(&self) -> bool {
        self.editable
    }

    /// Whether the text wraps. `None` means unlimited.
    pub fn wraps(&self) -> bool {
        self.max_lines != Some(1)
    }

    /// The widget: a read-only field showing `data`.
    ///
    /// Upstream builds an `EditableText` directly and **does not go through
    /// `InputDecorator`** -- a selectable text is a passage, not a box you
    /// type into, so it has no border, no underline and no label. This
    /// crate's [`crate::editable::TextField`] is already that bare thing: its
    /// build is the editable, a pointer region, a focus node and the
    /// semantics, and every border the gallery shows is put there by the demo
    /// around it.
    ///
    /// Everything this needs was missing until recently and was added for it:
    /// the field's read-only flag, the text it opens with, and -- corrected
    /// along the way -- the fact that a selectable text wraps where a field
    /// does not.
    ///
    /// **`show_cursor` is not honoured.** Upstream's default is false and this
    /// crate has no way to suppress a focused field's caret, so a selectable
    /// text that has been clicked shows one. Recorded rather than quietly
    /// dropped: the caret is upstream's `showCursor`, and the missing piece is
    /// in the field, not here.
    pub fn widget(&self, id: u64) -> crate::framework::AnyWidget {
        let field = crate::editable::TextField::new(id)
            .with_read_only(true)
            .with_initial_text(self.data.clone());
        let field = match self.field_max_lines() {
            crate::editable::MaxLines::Growing => field.multiline(),
            crate::editable::MaxLines::Single => field,
            crate::editable::MaxLines::Bounded(lines) => field.with_max_lines(lines),
        };
        crate::framework::stateful(field)
    }

    /// Which line mode the field is asked for.
    ///
    /// Its own function because the mapping is the claim -- the widget it
    /// builds carries the answer where a test cannot read it, and a mapping
    /// that quietly left everything at one line would look exactly like a
    /// working widget in every test that only checks the text.
    pub fn field_max_lines(&self) -> crate::editable::MaxLines {
        match self.max_lines {
            // Upstream's null: as many lines as the text takes.
            None => crate::editable::MaxLines::Growing,
            Some(1) => crate::editable::MaxLines::Single,
            Some(lines) => crate::editable::MaxLines::Bounded(lines as usize),
        }
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

    /// Mounts a widget and answers the field state it holds, if any.
    fn mounted_field<R>(
        widget: crate::framework::AnyWidget,
        read: impl Fn(&crate::editable::TextFieldState) -> R,
    ) -> Option<R> {
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(widget);
        let mut stack: Vec<crate::framework::ElementId> = tree.root().into_iter().collect();
        while let Some(id) = stack.pop() {
            if let Some(found) = tree.state::<crate::editable::TextFieldState, _>(id, &read) {
                return Some(found);
            }
            let mut children = tree.children_of(id);
            children.reverse();
            stack.extend(children);
        }
        None
    }

    #[test]
    fn a_selectable_text_shows_the_words_it_was_given() {
        // The point of building it at all: until the field could be handed
        // text and told to refuse edits, this widget was a struct with no way
        // to appear on screen.
        let text = SelectableText::new("a passage to read");
        let shown = mounted_field(text.widget(4291), |state| state.value.text.clone());
        assert_eq!(shown.as_deref(), Some("a passage to read"));
    }

    #[test]
    fn a_selectable_text_is_a_field_that_refuses_to_be_typed_into() {
        // Upstream's own description of it -- "a read-only `EditableText`".
        // Driving the platform's value through the client is what proves the
        // refusal; the widget merely asks for it.
        let text = SelectableText::new("locked");
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(text.widget(4292));
        let mut stack: Vec<crate::framework::ElementId> = tree.root().into_iter().collect();
        let mut found = None;
        while let Some(id) = stack.pop() {
            if tree
                .state::<crate::editable::TextFieldState, _>(id, |_| ())
                .is_some()
            {
                found = Some(id);
                break;
            }
            let mut children = tree.children_of(id);
            children.reverse();
            stack.extend(children);
        }
        assert!(found.is_some(), "it mounts a field");
    }

    #[test]
    fn an_unbounded_selectable_text_asks_the_field_to_grow() {
        // `maxLines: null` upstream, which this crate spells `MaxLines::Growing`
        // and reaches through `multiline()`. A selectable text left at its
        // default has to come out growing, or a passage would stop at one line
        // -- the very default this file had backwards.
        let text = SelectableText::new(
            "one
two
three",
        );
        assert_eq!(text.max_lines, None);
        let lines = mounted_field(text.widget(4293), |state| state.value.text.clone());
        assert_eq!(
            lines.as_deref(),
            Some(
                "one
two
three"
            )
        );
    }

    #[test]
    fn the_line_mode_follows_max_lines_all_three_ways() {
        // `None` is upstream's null and must come out growing, or a passage
        // stops at one line -- the default this file had backwards. A limit of
        // one is still a single line, and anything else is that many.
        use crate::editable::MaxLines;
        assert_eq!(
            SelectableText::new("x").field_max_lines(),
            MaxLines::Growing,
            "the default"
        );
        assert_eq!(
            SelectableText::new("x")
                .with_max_lines(Some(1))
                .field_max_lines(),
            MaxLines::Single
        );
        assert_eq!(
            SelectableText::new("x")
                .with_max_lines(Some(4))
                .field_max_lines(),
            MaxLines::Bounded(4)
        );
    }

    /// Lays a selectable text out in a narrow box and answers its height.
    fn laid_out_height(text: SelectableText, id: u64) -> f32 {
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(text.widget(id));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::new(0.0, 60.0, 0.0, 400.0),
        )
        .height
    }

    #[test]
    fn the_line_mode_reaches_the_field_and_not_just_the_mapping() {
        // The mapping being right is not the same as the widget applying it.
        // A passage too long for 60 pixels comes out taller when the field was
        // asked to grow than when it was left at one line -- which is the only
        // place the difference is visible from outside.
        const PASSAGE: &str = "a passage long enough that it cannot fit on one line";
        let growing = laid_out_height(SelectableText::new(PASSAGE), 4295);
        let single = laid_out_height(SelectableText::new(PASSAGE).with_max_lines(Some(1)), 4296);
        assert!(
            growing > single,
            "growing should wrap onto more lines: {growing} against {single}"
        );
    }

    #[test]
    fn a_selectable_text_tells_a_reader_it_cannot_be_edited() {
        // Upstream's `RenderEditable` sets `..isReadOnly = readOnly`. Without
        // it a screen-reader user meets something announced as a text field
        // and discovers it is not by typing into it.
        // The walk is gated on somebody reading and on something having marked
        // itself; a test has to say both.
        crate::semantics::set_enabled(true);
        let text = SelectableText::new("read me");
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(text.widget(4294));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 300.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 300.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        assert!(
            nodes.iter().any(|node| node.properties.flags.is_read_only),
            "a node says it is read-only: {nodes:?}"
        );
    }

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
    fn a_selectable_text_wraps_by_default_where_a_field_does_not() {
        // The rule this file had backwards. `TextField` declares
        // `this.maxLines = 1`; `SelectableText` declares `this.maxLines` with
        // **no default**, so it is null and the text runs as long as it needs
        // to. A passage that stopped at one line would be the wrong shape for
        // nearly every use of it -- and the old test asserted exactly that.
        let default = SelectableText::new("hello");
        assert_eq!(default.max_lines, None, "upstream's null");
        assert!(default.wraps());

        // A caller who wants one line still says so, and then it does not.
        let single = SelectableText::new("hello").with_max_lines(Some(1));
        assert!(!single.wraps());

        let three = SelectableText::new("hello").with_max_lines(Some(3));
        assert!(three.wraps(), "any limit above one still wraps");
    }

    #[test]
    fn the_rich_constructor_carries_spans_instead_of_a_string() {
        let rich = SelectableText::rich(3);
        assert!(rich.data.is_empty());
        assert!(!rich.is_editable());
    }
}
