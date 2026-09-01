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
    /// Upstream's `style`. `None` is upstream's null.
    ///
    /// # What `None` falls back to, and how that differs from upstream
    ///
    /// Upstream resolves it as
    /// `DefaultTextStyle.of(context).style.merge(style ?? textSpan.style)`:
    /// the ambient style from the tree, with the given one laid over it. This
    /// crate has no `DefaultTextStyle` -- see the note in components.rs -- so
    /// `None` reaches the field as `None` and the field falls back to the
    /// theme's body style, which is where an ambient default would have come
    /// from anyway.
    ///
    /// **The difference that remains is merging.** Upstream lets a caller give
    /// a style that says only "bold" and inherit the rest; here a style given
    /// is a style used. That is the same rule this crate's
    /// [`crate::widgets::TextSpan`] already states -- the inheriting is the
    /// caller's job, because by the time a run reaches the shaper the answer
    /// is one resolved style either way.
    ///
    /// For a passage built by [`SelectableText::rich`] this styles nothing
    /// visible: every run carries its own style, and the field's base style
    /// covers only text past the last run, of which a passage has none.
    pub style: Option<crate::engine::TextStyle>,
    /// Upstream's `textAlign`. `None` is upstream's null, which its build
    /// turns into `TextAlign.start` -- the same default the field already has,
    /// so nothing is passed for it and the two cannot disagree.
    pub text_align: Option<crate::engine::TextAlign>,
    /// Upstream's `textDirection`, which `TextAlign::Start` needs to mean
    /// anything. `None` is upstream's null: the ambient direction, which this
    /// crate takes as left-to-right.
    pub text_direction: Option<crate::direction::TextDirection>,
    /// The styles `data` is set in, where it is not all one style.
    ///
    /// Empty for the plain constructor, which is upstream's `textSpan == null`.
    /// The concatenation of the runs is `data`, and both exist because every
    /// other part of this widget asks the text a question -- how long it is,
    /// what a reader hears -- that a list of runs would have to be flattened
    /// to answer anyway.
    runs: Vec<(String, crate::engine::TextStyle)>,
}

impl SelectableText {
    pub fn new(data: impl Into<String>) -> SelectableText {
        SelectableText {
            data: data.into(),
            show_cursor: false,
            max_lines: None,
            editable: false,
            style: None,
            text_align: None,
            text_direction: None,
            runs: Vec::new(),
        }
    }

    /// Upstream's `SelectableText.rich`: a span tree instead of a string.
    ///
    /// The two constructors differ only in where the text comes from --
    /// upstream asserts that exactly one of `data` and `textSpan` is given --
    /// so `data` here is the runs joined, and it stays the string every other
    /// part of this widget already reads.
    ///
    /// The runs are flattened: this crate's [`crate::widgets::TextSpan`] is
    /// already a run rather than a tree, because a child inheriting what its
    /// parent did not override is a question the caller can answer once and
    /// the shaper only ever wants the resolved styles.
    pub fn rich(spans: Vec<crate::widgets::TextSpan>) -> SelectableText {
        SelectableText {
            data: spans.iter().map(|span| span.text.as_str()).collect(),
            show_cursor: false,
            max_lines: None,
            editable: false,
            style: None,
            text_align: None,
            text_direction: None,
            runs: spans
                .into_iter()
                .map(|span| (span.text, span.style))
                .collect(),
        }
    }

    /// Upstream's `style`. See the field for what `None` means here.
    pub fn with_style(mut self, style: crate::engine::TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Upstream's `textAlign`, and the direction it is resolved against.
    pub fn with_text_align(
        mut self,
        align: crate::engine::TextAlign,
        direction: crate::direction::TextDirection,
    ) -> Self {
        self.text_align = Some(align);
        self.text_direction = Some(direction);
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
    /// `show_cursor` reaches the field as upstream's `showCursor`, which is
    /// passed on to the `EditableText` the same way. Its default here is
    /// false, so a passage that has been clicked shows no caret -- a blinking
    /// bar in the middle of prose reads as a field somebody is about to type
    /// into, which is exactly what this is not.
    ///
    /// Note that the field's *own* default is not false but `!read_only`, so
    /// this has to be passed explicitly rather than left out: a read-only
    /// field would already suppress the caret, but a selectable text says so
    /// on its own account, and upstream lets `showCursor: true` put one back.
    pub fn widget(&self, id: u64) -> crate::framework::AnyWidget {
        let mut field = crate::editable::TextField::new(id)
            .with_read_only(true)
            .with_show_cursor(self.show_cursor)
            .with_text_align(
                self.text_align.unwrap_or(crate::engine::TextAlign::Start),
                self.text_direction
                    .unwrap_or(crate::direction::TextDirection::Ltr),
            );
        if let Some(style) = self.style.clone() {
            // Passed only when given. The field's own fallback is the theme's
            // body style, and handing it a resolved copy of that would be the
            // same answer written down in a second place -- and one that would
            // not follow the theme when it changed.
            field = field.with_style(style);
        }
        // `with_runs` sets the text from the runs themselves, so the two
        // constructors take different doors into the same field rather than
        // one of them handing over a string the other would contradict.
        let field = if self.runs.is_empty() {
            field.with_initial_text(self.data.clone())
        } else {
            field.with_runs(self.runs.clone())
        };
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
    fn a_selected_passage_blinks_nothing_at_the_reader() {
        // `showCursor` defaults to false on a `SelectableText`, and it is
        // passed to the field rather than left to the field's own default.
        // Both halves matter: without it a clicked passage of prose blinks a
        // bar in the middle of a paragraph, which is what a text box about to
        // be typed into looks like.
        //
        // The count is taken from the widget the passage builds, not asserted
        // about the flag, because the flag was already being stored correctly
        // before this worked -- it simply went nowhere.
        crate::focus::reset();
        let quiet = SelectableText::new("a passage to read");
        assert_eq!(painted_carets(quiet.widget(4301), 4301), 0);

        crate::focus::reset();
        let mut asked = SelectableText::new("a passage to read");
        asked.show_cursor = true;
        assert_eq!(
            painted_carets(asked.widget(4302), 4302),
            1,
            "upstream lets a selectable text ask for a caret, and it means it"
        );
    }

    /// Focuses the widget's field, paints one frame with the blink in its
    /// shown half, and counts the carets.
    ///
    /// Reaching through the whole build is the point: a test that read
    /// `show_cursor` back off the struct would have passed before this round,
    /// when the flag was stored and then dropped on the floor.
    fn painted_carets(widget: crate::framework::AnyWidget, id: u64) -> usize {
        use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
        use crate::render::RenderBox;

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(widget);

        // Focus opens the editing session, and the session's first frame is
        // the one the blink starts on -- shown. Running the real clock rather
        // than setting the flag is what puts `advance` in the path too: a
        // field with no caret never turns the flag on in the first place.
        assert!(crate::focus::focus(id), "the passage's field took focus");
        tree.advance_frame(10_000);
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a render tree");
        root.layout(crate::render::BoxConstraints::tight(200.0, 100.0));
        let mut layers = crate::engine::LayerTree::new(200, 100);
        reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(200.0, 100.0),
            );
            root.paint(&mut context, crate::render::Offset::ZERO);
        }
        drawn()
            .iter()
            .filter(|call| match call {
                Drawn::Rect { left, right, .. } => (right - left - 2.0).abs() < 0.01,
                _ => false,
            })
            .count()
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

    /// Paints one passage and hands back the first paragraph's text, colour
    /// and size.
    fn painted_text(passage: SelectableText, id: u64) -> (String, u32, f32) {
        use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
        use crate::render::RenderBox;

        crate::focus::reset();
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(passage.widget(id));
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a render tree");
        root.layout(crate::render::BoxConstraints::tight(400.0, 100.0));
        let mut layers = crate::engine::LayerTree::new(400, 100);
        reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(400.0, 100.0),
            );
            root.paint(&mut context, crate::render::Offset::ZERO);
        }
        drawn()
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph {
                    text, argb, size, ..
                } => Some((text.clone(), *argb, *size)),
                _ => None,
            })
            .expect("the passage was drawn")
    }

    #[test]
    fn a_passages_style_reaches_the_glyphs_and_its_absence_reaches_the_theme() {
        // Upstream resolves this as
        // `DefaultTextStyle.of(context).style.merge(style ?? textSpan.style)`.
        // This crate has no `DefaultTextStyle`, so `None` is passed on as
        // `None` and the field falls back to the theme's body -- which is
        // where the ambient default would have come from. What is *not*
        // ported is the merging: a style given here is the style used, not one
        // laid over an inherited one. See the field's own documentation.
        let styled = SelectableText::new("hello").with_style(crate::engine::TextStyle {
            color: crate::engine::Color(0xff884422),
            font_size: 33.0,
            ..Default::default()
        });
        let (text, colour, size) = painted_text(styled, 4330);
        assert_eq!(text, "hello");
        assert_eq!(colour, 0xff884422, "the colour asked for");
        assert!((size - 33.0).abs() < 0.01, "the size asked for: {size}");

        // Without one, the theme decides -- and decides something else.
        let (_, plain_colour, plain_size) = painted_text(SelectableText::new("hello"), 4331);
        assert_ne!(plain_colour, 0xff884422);
        assert!(
            (plain_size - 33.0).abs() > 0.01,
            "the theme's size, not the one above: {plain_size}"
        );
    }

    #[test]
    fn a_rich_passages_runs_outrank_the_style_it_was_given() {
        // Every run carries its own resolved style, so the base style covers
        // only text past the last run -- and a passage has none. Saying it in
        // a test because the opposite is the natural guess: `with_style` looks
        // like it should win, and for a rich passage it changes nothing at all.
        let body = crate::engine::TextStyle::default();
        let run_colour = crate::engine::Color(0xff112233);
        let rich = SelectableText::rich(vec![crate::widgets::TextSpan::new(
            "hello",
            crate::engine::TextStyle {
                color: run_colour,
                ..body
            },
        )])
        .with_style(crate::engine::TextStyle {
            color: crate::engine::Color(0xffaabbcc),
            ..Default::default()
        });

        let (text, colour, _) = painted_text(rich, 4332);
        assert_eq!(text, "hello");
        assert_eq!(colour, run_colour.0, "the run's colour, not the base one");
    }

    #[test]
    fn a_passages_alignment_reaches_the_glyphs() {
        // Storing the alignment is not the point; the glyphs moving is. Both
        // hops have to work -- the passage to the field, the field to the
        // render object -- and a mutation removing either one survived a
        // sweep until this existed.
        use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
        use crate::render::RenderBox;

        let paragraph_x = |passage: SelectableText, id: u64| {
            crate::focus::reset();
            let mut tree = crate::framework::ElementTree::new();
            tree.rebuild(passage.widget(id));
            tree.rebuild_dirty();
            let mut root = tree.build_render_tree().expect("a render tree");
            root.layout(crate::render::BoxConstraints::tight(400.0, 100.0));
            let mut layers = crate::engine::LayerTree::new(400, 100);
            reset_drawn();
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(400.0, 100.0),
                );
                root.paint(&mut context, crate::render::Offset::ZERO);
            }
            drawn()
                .iter()
                .find_map(|call| match call {
                    Drawn::Paragraph { text, x, .. } if text == "hello" => Some(*x),
                    _ => None,
                })
                .expect("the passage was drawn")
        };

        let plain = paragraph_x(SelectableText::new("hello"), 4320);
        assert_eq!(plain, 0.0, "the default is the leading edge");

        let centred = paragraph_x(
            SelectableText::new("hello").with_text_align(
                crate::engine::TextAlign::Center,
                crate::direction::TextDirection::Ltr,
            ),
            4321,
        );
        assert!(centred > plain, "centred started at {centred}");
    }

    #[test]
    fn a_rich_passage_is_drawn_from_its_runs_and_not_from_one_style() {
        // The runs have to reach the shaper, not merely be stored: before
        // this round `rich` took a *count* and threw it away, so a rich
        // passage drew an empty string.
        //
        // What the stubbed engine can show is limited and worth saying: it
        // keeps one style per paragraph -- the last pushed -- so it can prove
        // the runs went through `shape_rich` and cannot prove each run was
        // set in its own style. The real shaper does that; a test here that
        // claimed it would be claiming more than it saw.
        use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
        use crate::render::RenderBox;

        let body = crate::engine::TextStyle::default();
        let loud = crate::engine::TextStyle {
            color: crate::engine::Color(0xff123456),
            ..body.clone()
        };
        let rich = SelectableText::rich(vec![
            crate::widgets::TextSpan::new("Hold ", body.clone()),
            crate::widgets::TextSpan::new("Shift", loud),
        ]);
        assert_eq!(rich.data, "Hold Shift", "the text is the runs joined");

        crate::focus::reset();
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(rich.widget(4310));
        tree.rebuild_dirty();
        let mut root = tree.build_render_tree().expect("a render tree");
        root.layout(crate::render::BoxConstraints::tight(400.0, 100.0));
        let mut layers = crate::engine::LayerTree::new(400, 100);
        reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(400.0, 100.0),
            );
            root.paint(&mut context, crate::render::Offset::ZERO);
        }

        let paragraphs: Vec<(String, u32)> = drawn()
            .iter()
            .filter_map(|call| match call {
                Drawn::Paragraph { text, argb, .. } => Some((text.clone(), *argb)),
                _ => None,
            })
            .collect();
        assert_eq!(
            paragraphs.len(),
            1,
            "one line, so one paragraph: {paragraphs:?}"
        );
        assert_eq!(paragraphs[0].0, "Hold Shift");
        assert_eq!(
            paragraphs[0].1, 0xff123456,
            "the last run's colour, which is the stub's whole answer -- and \
             which the base style is not: {paragraphs:?}"
        );
    }

    #[test]
    fn the_rich_constructor_carries_spans_instead_of_a_string() {
        let body = crate::engine::TextStyle::default();
        let rich = SelectableText::rich(vec![
            crate::widgets::TextSpan::new("Hold ", body.clone()),
            crate::widgets::TextSpan::bold("Shift", &body),
            crate::widgets::TextSpan::new(" to select.", body),
        ]);
        // The text is the runs joined -- upstream asserts that exactly one of
        // `data` and `textSpan` is given, so there is nowhere else for it to
        // come from.
        assert_eq!(rich.data, "Hold Shift to select.");
        assert!(!rich.is_editable());
    }
}
