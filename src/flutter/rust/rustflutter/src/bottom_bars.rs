//! Ports of `material/bottom_navigation_bar.dart`, `material/navigation_bar.dart`
//! and `material/bottom_app_bar.dart`.
//!
//! The bar along the bottom, three ways: the Material 2 navigation bar, the
//! Material 3 one that replaced it, and the plain bar that holds actions rather
//! than destinations. Kept in one module because the interesting thing is the
//! contrast between them.

/// Upstream `BottomNavigationBarType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomNavigationBarType {
    /// Every item the same width, every label always shown.
    Fixed,
    /// Items move and labels fade in when tapped -- only the selected one is
    /// labelled.
    Shifting,
}

/// Upstream `BottomNavigationBarLandscapeLayout`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BottomNavigationBarLandscapeLayout {
    /// Spread evenly across the whole width.
    #[default]
    Spread,
    /// Keep the width they would have had in portrait and centre the row.
    ///
    /// Five items stretched across a landscape phone end up absurdly far
    /// apart, and a thumb that reached the first one cannot reach the last.
    Centered,
    /// Each item's label beside its icon rather than under it, which is what
    /// makes a short bar work in landscape.
    Linear,
}

/// Upstream `BottomNavigationBar`.
#[derive(Clone, Debug, PartialEq)]
pub struct BottomNavigationBar {
    pub item_count: usize,
    /// Whether every item was given a label. Upstream asserts it.
    pub all_items_labelled: bool,
    pub current_index: usize,
    /// `None` means "work it out from the count".
    pub bar_type: Option<BottomNavigationBarType>,
    /// Upstream's `selectedItemColor` and `fixedColor`, which are the same
    /// slot under two names -- see [`BottomNavigationBar::check`].
    ///
    /// These were `has_selected_item_color` and `has_fixed_color`, two
    /// booleans: enough for the "not both" assertion and nothing else, so a
    /// caller naming a colour had nowhere to put it and the resolver had
    /// nothing to read.
    pub selected_item_color: Option<crate::engine::Color>,
    pub fixed_color: Option<crate::engine::Color>,
    pub unselected_item_color: Option<crate::engine::Color>,
    /// `None` defers to the theme, then to `true`.
    pub show_selected_labels: Option<bool>,
    /// `None` defers to the theme, then to a default computed from the
    /// *resolved* type -- see
    /// [`crate::component_themes::ResolvedBottomNavigationBar`].
    pub show_unselected_labels: Option<bool>,
}

impl BottomNavigationBar {
    pub fn new(item_count: usize, current_index: usize) -> BottomNavigationBar {
        BottomNavigationBar {
            item_count,
            all_items_labelled: true,
            current_index,
            bar_type: None,
            selected_item_color: None,
            fixed_color: None,
            unselected_item_color: None,
            show_selected_labels: None,
            show_unselected_labels: None,
        }
    }

    /// This bar's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedBottomNavigationBar {
        crate::component_themes::ResolvedBottomNavigationBar::of(context, self)
    }

    /// Upstream's constructor asserts.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.item_count < 2 {
            // A bar with one destination is not navigation.
            return Err("items.length must be at least two");
        }
        if !self.all_items_labelled {
            // Even in shifting mode, where labels are hidden. **Hiding a label
            // is not removing it** -- it is still what a screen reader reads
            // out, and an item with none would be an unnamed button.
            return Err("Every item must have a non-null label");
        }
        if self.current_index >= self.item_count {
            return Err("currentIndex must be a valid index into items");
        }
        if self.selected_item_color.is_some() && self.fixed_color.is_some() {
            // The same slot under two names, one of them older.
            return Err("Either selectedItemColor or fixedColor can be specified, but not both");
        }
        Ok(())
    }

    /// Upstream `_effectiveType`, whose default is the interesting part:
    /// **fixed for three items or fewer, shifting for four or more.**
    ///
    /// The layout changes because the room ran out, not because anybody chose.
    /// With four labels across a phone there is not width for all of them, so
    /// only the selected one is written and the items slide to make space for
    /// it.
    pub fn effective_type(
        &self,
        theme_type: Option<BottomNavigationBarType>,
    ) -> BottomNavigationBarType {
        self.bar_type.or(theme_type).unwrap_or({
            if self.item_count <= 3 {
                BottomNavigationBarType::Fixed
            } else {
                BottomNavigationBarType::Shifting
            }
        })
    }

    /// Whether an item's label is drawn.
    pub fn shows_label(&self, index: usize, effective_type: BottomNavigationBarType) -> bool {
        match effective_type {
            BottomNavigationBarType::Fixed => true,
            BottomNavigationBarType::Shifting => index == self.current_index,
        }
    }
}

/// Upstream `NavigationBar`, the Material 3 replacement.
///
/// The same two asserts and **no type at all**: it does not shift, so the
/// count-based default disappears. Every destination keeps its label whatever
/// the count, which is the design deciding one way rather than adapting.
#[derive(Clone, Debug, PartialEq)]
pub struct NavigationBar {
    /// Upstream's `labelBehavior`, which decides whether
    /// [`NavigationBar::shows_label`] answers about the bar or about the
    /// destination.
    pub label_behavior: crate::component_themes::NavigationDestinationLabelBehavior,
    pub destination_count: usize,
    pub selected_index: usize,
    /// `None` is 500ms.
    ///
    /// **Not from the theme.** Upstream reads
    /// `animationDuration ?? const Duration(milliseconds: 500)` and
    /// `NavigationBarThemeData` has no duration field, so there is nothing in
    /// between to consult. This used to claim the theme supplied it and gave a
    /// default for a step that exists on neither side.
    pub animation_duration_ms: Option<u32>,
}

impl NavigationBar {
    /// This bar's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedNavigationBar {
        crate::component_themes::ResolvedNavigationBar::of(context, self)
    }

    pub const DEFAULT_ANIMATION_MS: u32 = 500;

    pub fn new(destination_count: usize, selected_index: usize) -> NavigationBar {
        NavigationBar {
            destination_count,
            selected_index,
            animation_duration_ms: None,
            // Upstream's default is alwaysShow.
            label_behavior: crate::component_themes::NavigationDestinationLabelBehavior::AlwaysShow,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.destination_count < 2 {
            return Err("destinations.length must be at least two");
        }
        if self.selected_index >= self.destination_count {
            return Err("selectedIndex must be a valid index into destinations");
        }
        Ok(())
    }

    /// Whether a destination's label is drawn.
    ///
    /// This answered `true` and ignored the index, on the grounds that "there
    /// is no shifting mode to hide it in". That reasoning belongs to Material
    /// 2's `BottomNavigationBarType.shifting`; **this is the Material 3 bar**,
    /// and it has `labelBehavior`, whose three values include one that hides
    /// every label and one that hides all but the selected one.
    ///
    /// So the answer was wrong under two of three behaviours, and the ignored
    /// argument was the tell: `onlyShowSelected` is exactly the case where
    /// which destination is being asked about decides the answer.
    pub fn shows_label(&self, index: usize) -> bool {
        match self.label_behavior {
            crate::component_themes::NavigationDestinationLabelBehavior::AlwaysShow => true,
            crate::component_themes::NavigationDestinationLabelBehavior::AlwaysHide => false,
            crate::component_themes::NavigationDestinationLabelBehavior::OnlyShowSelected => {
                index == self.selected_index
            }
        }
    }
}

/// Upstream `BottomAppBar`.
///
/// Not navigation: a bar of **actions**, and the reason it is a separate class
/// is the notch. A floating action button docked over this bar has a hole cut
/// for it, and the hole is the bar's job because only the bar knows its own
/// outline.
/// Not `Debug`/`PartialEq` any more: it holds a child now, and a widget is
/// neither printable nor comparable. The metrics it used to be compared on
/// live on [`crate::component_themes::ResolvedBottomAppBar`], which still is.
#[derive(Clone)]
pub struct BottomAppBar {
    /// The outline the notch is cut from, or `None` to defer.
    ///
    /// `None` does **not** mean "no notch": under Material 3 the chain
    /// continues to `_BottomAppBarDefaultsM3.shape`, which is an
    /// `AutomaticNotchedShape`, so a bar nobody configured still carries one.
    /// Whether a hole is actually cut needs a floating action button as well --
    /// see [`crate::component_themes::ResolvedBottomAppBar::cuts_a_notch`].
    ///
    /// This used to be a `bool` defaulting to false, with a `cuts_a_notch`
    /// that answered from it alone. That was wrong twice over: it said a
    /// default Material 3 bar never notches, where upstream's always has a
    /// shape, and it never looked for the button, where upstream cuts nothing
    /// without one.
    pub shape: Option<crate::borders::NotchedShape>,
    /// The gap left between the button and the edge of the hole, so the two do
    /// not touch.
    pub notch_margin: f32,
    child: std::cell::RefCell<Option<crate::framework::AnyWidget>>,
    /// See [`BottomAppBar::docked_at`].
    docked: Option<crate::engine::Rect>,
}

/// The bar's surface: a filled shape that is a plain rectangle until a docked
/// button gives it something to cut around.
///
/// A render object rather than a `Container` with a clip, because **the path
/// depends on the bar's own size**, which is not known until layout. Building
/// it from a size reported by the previous frame is the arrangement
/// [`crate::ink_well`] is stuck with for its splashes, and it costs a frame of
/// the wrong picture; a bar that is laid out and then painted has the size in
/// hand at the moment it needs it.
impl crate::framework::Component for BottomAppBar {
    fn build(&self, context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let bar = self.resolved(context);
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        let color = bar.color;
        let padding = bar.padding;
        let height = bar.height;
        // The shape says a notch is *possible*; a docked button is what makes
        // one happen. `cuts_a_notch` is the same two conditions, and asking it
        // rather than repeating them keeps the answer in one place.
        // The scaffold's channel, if this bar is in one. Looked up at build
        // time -- which is when a context is available -- and *read* at paint
        // time, which is when the answer exists.
        let geometry = context.inherited::<crate::components::ScaffoldGeometry>();
        let docked = self.docked.is_some()
            || geometry
                .as_ref()
                .map(|geometry| geometry.floating_action_button_area().is_some())
                .unwrap_or(false);
        // The shape says a notch is *possible*; a docked button is what makes
        // one happen. `cuts_a_notch` is the same two conditions, and asking it
        // rather than repeating them keeps the answer in one place.
        //
        // **Kept when the bar is in a scaffold at all**, even if no button has
        // been placed yet: the first layout has not run when this builds, so a
        // shape dropped here could never come back and the notch would depend
        // on which happened first.
        let shape = bar
            .shape
            .clone()
            .filter(|_| bar.cuts_a_notch(docked || geometry.is_some()));
        // Grown by the margin here rather than in the painter, because it is a
        // property of *this bar* -- upstream's `_BottomAppBarClipper` inflates
        // the button's rectangle by `notchMargin` for the same reason: the gap
        // belongs to the bar that leaves it, not to the button.
        let guest = self.notch_rect();
        let margin = self.notch_margin;

        crate::framework::single(child, move |inner| {
            let mut content = crate::widgets::Container::new()
                .with_padding(padding)
                .with_child(inner);
            if let Some(height) = height {
                content = content.with_height(height);
            }
            NotchedSurface {
                color,
                shape: shape.clone(),
                guest,
                geometry: geometry.as_deref().cloned(),
                notch_margin: margin,
                child: crate::render::RenderRef::new(content),
                size: crate::render::Size::ZERO,
            }
        })
    }
}

struct NotchedSurface {
    color: crate::engine::Color,
    shape: Option<crate::borders::NotchedShape>,
    /// Where the docked button is, in this bar's coordinates, already grown by
    /// the notch margin. `None` is a bar with nothing docked over it, which is
    /// upstream's condition too: `_BottomAppBarClipper` cuts nothing when the
    /// scaffold's geometry reports no button.
    guest: Option<crate::engine::Rect>,
    /// The scaffold's channel, read at **paint** time -- see
    /// [`crate::components::ScaffoldGeometry`]. `None` when the bar is not in
    /// a scaffold, which is when `guest` is the only source.
    geometry: Option<crate::components::ScaffoldGeometry>,
    /// How much of the button's rectangle to leave clear, kept for the same
    /// late reading: the scaffold publishes the button's own bounds, and the
    /// margin is this bar's to add.
    notch_margin: f32,
    child: crate::render::BoxedRender,
    size: crate::render::Size,
}

impl NotchedSurface {
    /// Where the hole goes on the canvas: the docked rectangle, which is in
    /// the bar's own coordinates, moved to where the bar is being painted.
    ///
    /// A named step for the same reason [`BottomAppBar::notch_rect`] is one --
    /// **the canvas cannot show it**. The notch dips inward, so the drawn
    /// path's bounding box is the bar's rectangle wherever the hole ends up;
    /// a painter that forgot the offset would cut at the top of the window and
    /// every test watching the paint calls would still see `(0, 0, 400, 80)`.
    fn guest_at(&self, offset: crate::render::Offset) -> Option<crate::engine::Rect> {
        // **The scaffold's answer first.** A bar inside a scaffold learns
        // where the button actually landed; `guest` is what a caller said by
        // hand, which is the only source for a bar standing on its own.
        // Upstream's clipper is the same order of preference -- it uses the
        // geometry when the scaffold published one and falls back otherwise.
        if let Some(published) = self
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.floating_action_button_area())
        {
            // Already in the same coordinates this paint is working in, so the
            // margin is all that is left to add -- the gap belongs to the bar
            // that leaves it, not to the button.
            let margin = self.notch_margin;
            return Some(crate::engine::Rect::ltrb(
                published.left - margin,
                published.top - margin,
                published.right + margin,
                published.bottom + margin,
            ));
        }
        self.guest.map(|guest| {
            crate::engine::Rect::ltrb(
                guest.left + offset.dx,
                guest.top + offset.dy,
                guest.right + offset.dx,
                guest.bottom + offset.dy,
            )
        })
    }
}

impl crate::render::RenderBox for NotchedSurface {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> crate::render::Size {
        self.size = self.child.layout_child(constraints, true);
        self.size
    }

    fn size(&self) -> crate::render::Size {
        self.size
    }

    fn visit_children(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        visit(&self.child, crate::render::Offset::ZERO);
    }

    fn visit_children_for_semantics(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        visit(&self.child, crate::render::Offset::ZERO);
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.child.hit_test(position, result)
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        let host = crate::engine::Rect::ltrb(
            offset.dx,
            offset.dy,
            offset.dx + self.size.width,
            offset.dy + self.size.height,
        );
        let paint = crate::engine::Paint::new(self.color);
        // **A notch needs both**: a shape to cut it with and a button to cut it
        // around. Deciding here rather than at build time is what lets the
        // scaffold's answer arrive late -- when the bar was built, the first
        // layout had not run and there was no button to hear about yet.
        //
        // Without a button the outline is a plain rectangle, and it is drawn
        // as one: `outer_path` would trace the same shape, but a rectangle
        // drawn as a path is indistinguishable from a notched one to anything
        // watching the canvas, and this is the difference the tests are for.
        let guest = self.guest_at(offset);
        match self.shape.as_ref().filter(|_| guest.is_some()) {
            // The notch is cut only when there is a button to cut around.
            // `shape` alone means one is *possible* -- a default Material 3
            // bar always carries a shape -- which is the distinction
            // `ResolvedBottomAppBar::cuts_a_notch` exists to draw.
            Some(shape) => {
                context
                    .canvas()
                    .draw_path(&shape.outer_path(host, guest), &paint);
            }
            None => context.canvas().draw_rect(host, &paint),
        }
        self.child.paint(context, offset);
    }
}

impl BottomAppBar {
    pub const DEFAULT_NOTCH_MARGIN: f32 = 4.0;
    /// Upstream's Material 3 default padding, as `(horizontal, vertical)`.
    pub const M3_PADDING: (f32, f32) = (16.0, 12.0);

    pub fn new() -> BottomAppBar {
        BottomAppBar {
            shape: None,
            notch_margin: BottomAppBar::DEFAULT_NOTCH_MARGIN,
            child: std::cell::RefCell::new(None),
            docked: None,
        }
    }

    /// What the bar holds -- upstream's `child`, normally a row of actions.
    pub fn with_child(self, child: crate::framework::AnyWidget) -> Self {
        *self.child.borrow_mut() = Some(child);
        self
    }

    /// Where the docked button sits, in the bar's own coordinates.
    ///
    /// Upstream's bar does not work this out either: it **reads** the button's
    /// rectangle off `Scaffold.geometryOf(context)`, a listenable the scaffold
    /// writes after it has laid the button out, and hands it to
    /// `_BottomAppBarClipper`. That channel is not ported -- this crate's
    /// [`crate::fab_location`] computes where a button goes but nothing
    /// publishes the result -- so the caller says. Everything downstream of
    /// knowing the rectangle is the same either way, which is why this is the
    /// seam: when the geometry channel lands it fills this in instead of
    /// replacing anything.
    ///
    /// `None` is a bar with nothing docked over it, and it cuts no notch --
    /// which is upstream's condition, not merely a default: a hole with no
    /// button behind it is a hole in the bar.
    pub fn docked_at(mut self, guest: crate::engine::Rect) -> Self {
        self.docked = Some(guest);
        self
    }

    /// The rectangle the notch is actually cut around: the docked button grown
    /// by [`BottomAppBar::notch_margin`] on every side.
    ///
    /// Upstream's `_BottomAppBarClipper.getClip` does the same inflation
    /// before handing the rectangle to the shape, and the gap is the point --
    /// a notch traced exactly on the button leaves the two touching, which
    /// reads as the button having grown a collar rather than sitting in a
    /// hole.
    ///
    /// Named rather than inlined because it is the one part of the geometry a
    /// picture cannot show: the drawn path's *bounding box* is the bar either
    /// way, since the notch dips inward, so a margin that stopped being
    /// applied would repaint identically to any test watching the canvas.
    pub fn notch_rect(&self) -> Option<crate::engine::Rect> {
        self.docked.map(|guest| {
            crate::engine::Rect::ltrb(
                guest.left - self.notch_margin,
                guest.top - self.notch_margin,
                guest.right + self.notch_margin,
                guest.bottom + self.notch_margin,
            )
        })
    }

    /// Upstream's usual shape, `CircularNotchedRectangle`.
    pub fn with_notch(mut self) -> Self {
        self.shape = Some(crate::borders::NotchedShape::Circular { inverted: false });
        self
    }

    /// This bar's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedBottomAppBar {
        crate::component_themes::ResolvedBottomAppBar::of(context, self)
    }

    /// Upstream's inline padding default, which reads
    /// [`crate::theme::ThemeData::use_material3`] rather than anything on the
    /// bar.
    pub fn default_padding(use_material3: bool) -> (f32, f32) {
        if use_material3 {
            BottomAppBar::M3_PADDING
        } else {
            (0.0, 0.0)
        }
    }
}

impl Default for BottomAppBar {
    fn default() -> Self {
        BottomAppBar::new()
    }
}

#[cfg(test)]
mod tests {

    /// What the compositor was told to draw for this bar.
    fn bar_calls(bar: BottomAppBar) -> Vec<crate::engine_test_stubs::Drawn> {
        bar_calls_at(bar, 0.0)
    }

    /// The same, with the bar pushed `down` pixels from the top, so a painter
    /// that ignored its offset can be told from one that does not.
    fn bar_calls_at(bar: BottomAppBar, down: f32) -> Vec<crate::engine_test_stubs::Drawn> {
        let body = if down > 0.0 {
            crate::framework::leaf(move || {
                crate::render::RenderFlex::column()
                    .with_main_axis_size(crate::render::MainAxisSize::Min)
                    .push(crate::widgets::Container::new().with_size(1.0, down))
            })
        } else {
            crate::framework::leaf(|| crate::widgets::Empty)
        };
        let _ = &body;
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            if down > 0.0 {
                crate::framework::single(crate::framework::component(bar), move |inner| {
                    crate::render::RenderPadding::new(
                        crate::render::EdgeInsets::only(0.0, down, 0.0, 0.0),
                        inner,
                    )
                })
            } else {
                crate::framework::component(bar)
            },
        ));
        let mut root = tree.build_render_tree().expect("a root");
        // **Tight** across, which is what a scaffold hands a bottom bar: it
        // spans the window. Laid out loose the bar shrink-wraps to its padding
        // -- 32 pixels wide with an empty child -- and a button docked at 180
        // is nowhere near it, so nothing is cut and two different margins
        // produce the same picture. That is how the first version of the
        // margin test came out green-looking and meant nothing.
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::new(400.0, 400.0, 0.0, 200.0),
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
    }

    #[test]
    fn a_bottom_app_bar_reaches_the_screen_at_all() {
        // Found by `tools/shells.py`: both halves of the notch were ported and
        // tested -- `NotchedShape::outer_path` draws it, `cuts_a_notch` says
        // when -- and **nothing painted a bar**, so neither could be seen.
        let calls = bar_calls(BottomAppBar::new());
        assert!(
            !calls.is_empty(),
            "the bar painted nothing at all: {calls:?}"
        );
    }

    #[test]
    fn a_bar_with_nothing_docked_over_it_is_a_plain_rectangle() {
        // The condition is upstream's, not a default: `_BottomAppBarClipper`
        // cuts nothing when the scaffold's geometry reports no button, because
        // a hole with no button behind it is a hole in the bar. A Material 3
        // bar carries a shape regardless, so the shape alone must not decide.
        let calls = bar_calls(BottomAppBar::new().with_notch());
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Rect { .. })),
            "a bar with no docked button was not drawn as a rectangle: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Path { .. })),
            "it cut a notch around nothing: {calls:?}"
        );
    }

    fn only_path(calls: &[crate::engine_test_stubs::Drawn]) -> (f32, f32, f32, f32) {
        calls
            .iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Path {
                    left,
                    top,
                    right,
                    bottom,
                    ..
                } => Some((*left, *top, *right, *bottom)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no notched outline was drawn: {calls:?}"))
    }

    fn docked_bar(margin: f32) -> BottomAppBar {
        let mut bar = BottomAppBar::new()
            .with_notch()
            .docked_at(crate::engine::Rect::ltrb(180.0, -28.0, 236.0, 28.0));
        bar.notch_margin = margin;
        bar
    }

    #[test]
    fn a_docked_button_gets_a_hole_cut_for_it() {
        only_path(&bar_calls(docked_bar(BottomAppBar::DEFAULT_NOTCH_MARGIN)));
    }

    #[test]
    fn the_notch_leaves_the_margin_the_bar_asked_for() {
        // Upstream inflates the button's rectangle by `notchMargin` before
        // cutting, so the hole is wider than the button and the two never
        // touch. Without it the notch traces the button exactly.
        //
        // Asserted on `notch_rect` rather than on the canvas, and that is not
        // a shortcut: the notch dips *into* the bar, so the drawn path's
        // bounding box is the bar's own rectangle whatever margin was left.
        // A test watching the paint calls sees `(0, 0, 400, 80)` for every
        // margin -- which is exactly what the first version of this test did
        // before the sweep asked what it was measuring.
        let button = crate::engine::Rect::ltrb(180.0, -28.0, 236.0, 28.0);
        assert_eq!(
            docked_bar(0.0).notch_rect(),
            Some(button),
            "no margin should leave the button's own rectangle"
        );
        assert_eq!(
            docked_bar(20.0).notch_rect(),
            Some(crate::engine::Rect::ltrb(160.0, -48.0, 256.0, 48.0)),
            "the margin is left on every side"
        );
        assert_eq!(
            BottomAppBar::new().notch_rect(),
            None,
            "a bar with nothing docked has no hole to describe"
        );
    }

    #[test]
    fn the_scaffolds_rectangle_is_grown_by_this_bars_margin() {
        // The scaffold publishes the button's **own** bounds -- it does not
        // know what gap this bar wants around it, and two bars in one
        // application may want different ones. So the margin is added on this
        // side, on the published rectangle as much as on a hand-written one.
        //
        // Asserted on `guest_at` for the reason round 394 found: the notch
        // dips inward, so the drawn path's bounding box is the bar's outline
        // whatever margin was left, and the canvas cannot tell the two apart.
        let geometry = crate::components::ScaffoldGeometry::default();
        let surface = NotchedSurface {
            color: crate::engine::Color(0xff000000),
            shape: None,
            guest: None,
            geometry: Some(geometry.clone()),
            notch_margin: 4.0,
            child: crate::render::RenderRef::new(crate::widgets::Empty),
            size: crate::render::Size::ZERO,
        };
        // Nothing placed yet: nothing to cut around.
        assert_eq!(surface.guest_at(crate::render::Offset::ZERO), None);

        geometry.publish_for_tests(Some(crate::engine::Rect::ltrb(180.0, 700.0, 236.0, 756.0)));
        assert_eq!(
            surface.guest_at(crate::render::Offset::ZERO),
            Some(crate::engine::Rect::ltrb(176.0, 696.0, 240.0, 760.0)),
            "the published rectangle reached the shape without this bar's gap"
        );
    }

    #[test]
    fn the_hole_moves_with_the_bar() {
        // The painter is told where it is, and the docked rectangle is in the
        // bar's own coordinates, so the two have to be added -- otherwise a
        // bar anywhere but the top of the window cuts its hole somewhere else
        // entirely.
        //
        // Asserted on the step rather than the canvas, for the same reason the
        // margin is: the hole is inside the bar's outline, so moving it does
        // not move the drawn path's bounding box, and the paint calls are the
        // same either way. What the canvas *can* still show is that the bar
        // itself moved, so that much is checked here too.
        let surface = NotchedSurface {
            color: crate::engine::Color(0xff000000),
            shape: None,
            guest: Some(crate::engine::Rect::ltrb(180.0, -28.0, 236.0, 28.0)),
            geometry: None,
            notch_margin: 0.0,
            child: crate::render::RenderRef::new(crate::widgets::Empty),
            size: crate::render::Size::ZERO,
        };
        // **Both axes**, because one of them alone leaves half the line
        // untested: the first version moved the bar only downwards, and a
        // painter that dropped the horizontal offset went unnoticed.
        assert_eq!(
            surface.guest_at(crate::render::Offset::new(0.0, 40.0)),
            Some(crate::engine::Rect::ltrb(180.0, 12.0, 236.0, 68.0))
        );
        assert_eq!(
            surface.guest_at(crate::render::Offset::new(10.0, 40.0)),
            Some(crate::engine::Rect::ltrb(190.0, 12.0, 246.0, 68.0))
        );
        assert_eq!(
            NotchedSurface {
                guest: None,
                ..surface
            }
            .guest_at(crate::render::Offset::new(0.0, 40.0)),
            None
        );

        let at_origin = only_path(&bar_calls_at(
            docked_bar(BottomAppBar::DEFAULT_NOTCH_MARGIN),
            0.0,
        ));
        let pushed_down = only_path(&bar_calls_at(
            docked_bar(BottomAppBar::DEFAULT_NOTCH_MARGIN),
            40.0,
        ));
        assert_ne!(at_origin.1, pushed_down.1, "the bar stayed behind");
    }

    #[test]
    fn the_bar_paints_what_it_holds() {
        // A surface that drew itself and stopped would be a coloured strip
        // where a row of actions should be.
        const MARK: crate::engine::Color = crate::engine::Color(0xff00ff00);
        let calls = bar_calls(BottomAppBar::new().with_child(crate::framework::leaf(|| {
            crate::widgets::Container::new()
                .with_size(24.0, 24.0)
                .with_color(MARK)
        })));
        assert!(
            calls.iter().any(|call| matches!(
                call,
                crate::engine_test_stubs::Drawn::Rect { argb, .. } if *argb == MARK.0
            )),
            "the child was not painted: {calls:?}"
        );
    }

    #[test]
    fn the_bar_is_as_tall_as_the_theme_says() {
        // Material 3 gives the bar a height of its own; Material 2 leaves it
        // as tall as its child. Ignoring the resolved height silently returns
        // every bar to the Material 2 behaviour.
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(BottomAppBar::new()),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        let size = crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 200.0),
        );
        let expected = crate::component_themes::ResolvedBottomAppBar::M3_HEIGHT;
        assert_eq!(size.height, expected);
    }

    use super::*;

    // -- The count decides the layout ------------------------------------------

    #[test]
    fn the_layout_changes_because_the_room_ran_out() {
        // Fixed for three or fewer, shifting for four or more. Nobody chose;
        // there is simply not width for four labels across a phone.
        for count in 2..=3 {
            assert_eq!(
                BottomNavigationBar::new(count, 0).effective_type(None),
                BottomNavigationBarType::Fixed,
                "{count} items"
            );
        }
        for count in 4..=6 {
            assert_eq!(
                BottomNavigationBar::new(count, 0).effective_type(None),
                BottomNavigationBarType::Shifting,
                "{count} items"
            );
        }
    }

    #[test]
    fn saying_so_outright_or_theming_it_overrules_the_count() {
        let five = BottomNavigationBar {
            bar_type: Some(BottomNavigationBarType::Fixed),
            ..BottomNavigationBar::new(5, 0)
        };
        assert_eq!(five.effective_type(None), BottomNavigationBarType::Fixed);

        let themed = BottomNavigationBar::new(2, 0);
        assert_eq!(
            themed.effective_type(Some(BottomNavigationBarType::Shifting)),
            BottomNavigationBarType::Shifting
        );
        assert_eq!(
            five.effective_type(Some(BottomNavigationBarType::Shifting)),
            BottomNavigationBarType::Fixed,
            "and the widget beats the theme"
        );
    }

    #[test]
    fn shifting_writes_only_the_selected_label_and_fixed_writes_them_all() {
        let bar = BottomNavigationBar::new(5, 2);
        for index in 0..5 {
            assert!(bar.shows_label(index, BottomNavigationBarType::Fixed));
        }
        assert!(bar.shows_label(2, BottomNavigationBarType::Shifting));
        assert!(!bar.shows_label(0, BottomNavigationBarType::Shifting));
    }

    // -- What the constructor refuses --------------------------------------------

    #[test]
    fn a_bar_with_one_destination_is_not_navigation() {
        assert!(BottomNavigationBar::new(1, 0).validate().is_err());
        assert_eq!(BottomNavigationBar::new(2, 0).validate(), Ok(()));
        assert!(NavigationBar::new(1, 0).validate().is_err());
    }

    #[test]
    fn hiding_a_label_is_not_removing_it() {
        // Even in shifting mode, where it is not drawn, it is still what a
        // screen reader reads out. An item with none would be an unnamed
        // button.
        let mut bar = BottomNavigationBar::new(5, 0);
        assert_eq!(bar.validate(), Ok(()));
        bar.all_items_labelled = false;
        assert!(bar.validate().is_err());
    }

    #[test]
    fn the_selected_index_has_to_point_at_something() {
        assert!(BottomNavigationBar::new(3, 3).validate().is_err());
        assert_eq!(BottomNavigationBar::new(3, 2).validate(), Ok(()));
        assert!(NavigationBar::new(3, 3).validate().is_err());
    }

    #[test]
    fn two_names_for_one_colour_cannot_both_be_given() {
        let mut bar = BottomNavigationBar::new(3, 0);
        bar.selected_item_color = Some(crate::engine::Color::argb(255, 0, 0, 10));
        assert_eq!(bar.validate(), Ok(()));
        bar.fixed_color = Some(crate::engine::Color::argb(255, 0, 0, 20));
        assert!(bar.validate().is_err());
    }

    // -- What Material 3 dropped ---------------------------------------------------

    #[test]
    fn the_material_three_bar_has_no_shifting_mode_to_hide_a_label_in() {
        // The design decided one way rather than adapting to the count.
        let crowded = NavigationBar::new(6, 0);
        assert_eq!(crowded.validate(), Ok(()));
        for index in 0..6 {
            assert!(crowded.shows_label(index));
        }
    }

    #[test]
    fn its_animation_falls_back_to_a_half_second_with_no_theme_in_between() {
        assert_eq!(NavigationBar::new(3, 0).animation_duration_ms, None);
        assert_eq!(NavigationBar::DEFAULT_ANIMATION_MS, 500);
    }

    // -- The bar that holds actions ---------------------------------------------------

    #[test]
    fn asking_for_a_notch_names_upstreams_usual_shape() {
        // What the widget can say on its own. Whether a hole is cut is a
        // question for the resolution and the Scaffold -- this test used to be
        // called "a bar with no floating button over it has nothing to cut
        // around" while checking a flag that had never heard of a button.
        assert_eq!(BottomAppBar::new().shape, None);
        assert_eq!(
            BottomAppBar::new().with_notch().shape,
            Some(crate::borders::NotchedShape::Circular { inverted: false })
        );
    }

    #[test]
    fn the_notch_margin_keeps_the_button_off_the_edge_of_its_own_hole() {
        assert_eq!(BottomAppBar::new().notch_margin, 4.0);
        assert!(BottomAppBar::new().notch_margin > 0.0);
    }

    #[test]
    fn material_three_pads_the_child_and_material_two_does_not() {
        assert_eq!(BottomAppBar::default_padding(true), (16.0, 12.0));
        assert_eq!(BottomAppBar::default_padding(false), (0.0, 0.0));
    }

    // -- Landscape --------------------------------------------------------------------

    #[test]
    fn spreading_five_items_across_a_landscape_phone_puts_them_out_of_reach() {
        // Which is what the centred layout is for.
        assert_eq!(
            BottomNavigationBarLandscapeLayout::default(),
            BottomNavigationBarLandscapeLayout::Spread
        );
        assert_ne!(
            BottomNavigationBarLandscapeLayout::Centered,
            BottomNavigationBarLandscapeLayout::Spread
        );
        assert_ne!(
            BottomNavigationBarLandscapeLayout::Linear,
            BottomNavigationBarLandscapeLayout::Centered
        );
    }
}

#[cfg(test)]
mod bottom_bar_theme_tests {
    use super::*;
    use crate::component_themes::{
        BottomNavigationBarTheme, BottomNavigationBarThemeData, ResolvedBottomNavigationBar,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component};

    struct Reader {
        bar: BottomNavigationBar,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedBottomNavigationBar>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.bar.resolved(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(
        bar: BottomNavigationBar,
        data: BottomNavigationBarThemeData,
    ) -> ResolvedBottomNavigationBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        // No `Theme` above it: `BottomNavigationBarTheme::of` falls back to
        // `ThemeData::of`, which has its own fallback. Wrapping one here would
        // suggest it took part in the answer.
        tree.rebuild(BottomNavigationBarTheme::new(
            data,
            component(Reader {
                bar,
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// The same, under a named `ThemeData` -- which the two item colours now
    /// end at, so the last step can be checked against a theme that is not
    /// the fallback.
    fn resolve_under(
        bar: BottomNavigationBar,
        data: BottomNavigationBarThemeData,
        theme: crate::theme::ThemeData,
    ) -> ResolvedBottomNavigationBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            BottomNavigationBarTheme::new(
                data,
                component(Reader {
                    bar,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn the_unselected_label_default_is_computed_from_the_type_and_the_selected_one_is_not() {
        // The asymmetry is the design: the selected label tells the reader
        // where they are and is never hidden; the unselected ones are hidden
        // exactly when there is no room, which is what shifting means.
        let three = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(three.bar_type, BottomNavigationBarType::Fixed);
        assert!(three.show_selected_labels);
        assert!(three.show_unselected_labels, "fixed: there is room");

        let four = resolve(
            BottomNavigationBar::new(4, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(four.bar_type, BottomNavigationBarType::Shifting);
        assert!(four.show_selected_labels, "still never hidden");
        assert!(!four.show_unselected_labels, "shifting: there is not");
    }

    #[test]
    fn a_theme_that_asks_for_shifting_changes_what_the_labels_do_without_touching_them() {
        // The default is computed from the *resolved* type, so a theme that
        // only set the type has moved the labels too.
        let mut data = BottomNavigationBarThemeData::new();
        data.bar_type = Some(BottomNavigationBarType::Shifting);
        let bar = resolve(BottomNavigationBar::new(3, 0), data);
        assert_eq!(bar.bar_type, BottomNavigationBarType::Shifting);
        assert!(
            !bar.show_unselected_labels,
            "three items, and still no unselected labels"
        );
    }

    #[test]
    fn the_widgets_own_type_beats_the_themes() {
        let mut data = BottomNavigationBarThemeData::new();
        data.bar_type = Some(BottomNavigationBarType::Shifting);
        let mut bar = BottomNavigationBar::new(4, 0);
        bar.bar_type = Some(BottomNavigationBarType::Fixed);
        let resolved = resolve(bar, data);
        assert_eq!(resolved.bar_type, BottomNavigationBarType::Fixed);
        assert!(
            resolved.show_unselected_labels,
            "and the labels follow the type that won"
        );
    }

    #[test]
    fn saying_so_outright_beats_the_computed_default() {
        let mut bar = BottomNavigationBar::new(4, 0);
        bar.show_unselected_labels = Some(true);
        assert!(
            resolve(bar, BottomNavigationBarThemeData::new()).show_unselected_labels,
            "shifting would have hidden them"
        );

        let mut bar = BottomNavigationBar::new(3, 0);
        bar.show_selected_labels = Some(false);
        assert!(!resolve(bar, BottomNavigationBarThemeData::new()).show_selected_labels);
    }

    #[test]
    fn the_theme_sits_between_the_widget_and_the_computed_default() {
        let mut data = BottomNavigationBarThemeData::new();
        data.show_unselected_labels = Some(true);
        // Four items would compute false; the theme says otherwise.
        assert!(resolve(BottomNavigationBar::new(4, 0), data.clone()).show_unselected_labels);

        let mut bar = BottomNavigationBar::new(4, 0);
        bar.show_unselected_labels = Some(false);
        assert!(
            !resolve(bar, data).show_unselected_labels,
            "and the widget over it"
        );
    }

    #[test]
    fn the_widget_beats_the_theme_on_the_selected_label_too() {
        // Both sides set and *disagreeing*: the theme's own tests set one side
        // at a time, which shows that something comes through and not which
        // side it came from. `tools/order_sweep.py` found this one.
        let mut data = BottomNavigationBarThemeData::new();
        data.show_selected_labels = Some(false);
        assert!(
            !resolve(BottomNavigationBar::new(3, 0), data.clone()).show_selected_labels,
            "the theme's, with the widget silent"
        );

        let mut bar = BottomNavigationBar::new(3, 0);
        bar.show_selected_labels = Some(true);
        assert!(
            resolve(bar, data).show_selected_labels,
            "and the widget over it"
        );
    }

    #[test]
    fn nothing_is_invented_for_the_background_upstream_leaves_null() {
        // This used to say the same of the two item colours, on the grounds
        // that "the widget falls back to the primary and to the caption
        // colour". There is no such widget step in this port: the resolver
        // *is* what upstream does in `build`, so leaving them null left the
        // fallback nowhere at all -- which is how
        // `ThemeData::unselected_widget_color` came to reach nothing. The
        // background is different: upstream really does leave it null for a
        // fixed bar with no colour named, and `Material` supplies its own.
        let resolved = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(resolved.background_color, None);
    }

    #[test]
    fn the_defaults_are_upstreams() {
        let resolved = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(resolved.elevation, 8.0);
        assert!(resolved.enable_feedback);
        assert_eq!(
            resolved.landscape_layout,
            BottomNavigationBarLandscapeLayout::Spread
        );
    }

    // -- Where a bottom bar's item colours come from, tick 231 --------------
    //
    // `tools/unread_theme_fields.py` found `ThemeData::unselected_widget_color`
    // reaching nothing. It reached nothing because
    // `ResolvedBottomNavigationBar` copied the theme's two item colours
    // across as bare `Option<Color>` and stopped -- and the widget carried
    // `has_selected_item_color` and `has_fixed_color` as booleans, enough for
    // upstream's "not both" assertion and nothing else, so a caller naming a
    // colour had nowhere to put it.
    //
    // Every level below uses a number no other level uses.

    fn ink(blue: u8) -> crate::engine::Color {
        crate::engine::Color::argb(255, 0, 0, blue)
    }

    #[test]
    fn a_fixed_bars_item_colours_prefer_the_bar_then_the_theme() {
        let mut bar = BottomNavigationBar::new(3, 0);
        bar.selected_item_color = Some(ink(10));
        bar.unselected_item_color = Some(ink(20));
        let themed = BottomNavigationBarThemeData {
            selected_item_color: Some(ink(30)),
            unselected_item_color: Some(ink(40)),
            ..BottomNavigationBarThemeData::new()
        };
        let resolved = resolve(bar, themed.clone());
        assert_eq!(resolved.selected_item_color, ink(10));
        assert_eq!(resolved.unselected_item_color, ink(20));

        let resolved = resolve(BottomNavigationBar::new(3, 0), themed);
        assert_eq!(resolved.selected_item_color, ink(30));
        assert_eq!(resolved.unselected_item_color, ink(40));
    }

    #[test]
    fn the_older_fixed_color_is_the_step_after_the_theme() {
        // Upstream's chain is `widget.selectedItemColor ?? theme ??
        // widget.fixedColor ?? themeColor` -- `fixedColor` comes *after* the
        // theme, not with the widget's other colour, which is the part a
        // reading would most easily get backwards.
        let mut bar = BottomNavigationBar::new(3, 0);
        bar.fixed_color = Some(ink(50));
        let themed = BottomNavigationBarThemeData {
            selected_item_color: Some(ink(60)),
            ..BottomNavigationBarThemeData::new()
        };
        assert_eq!(resolve(bar, themed).selected_item_color, ink(60));

        let mut bar = BottomNavigationBar::new(3, 0);
        bar.fixed_color = Some(ink(50));
        assert_eq!(
            resolve(bar, BottomNavigationBarThemeData::new()).selected_item_color,
            ink(50)
        );
    }

    #[test]
    fn a_fixed_bar_ends_at_the_theme_datas_own_colours() {
        // The two fallbacks that reached nothing before: the unselected end
        // takes `ThemeData::unselected_widget_color` and the selected end
        // takes `themeColor`.
        let resolved = resolve(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
        );
        let theme = crate::theme::ThemeData::light();
        assert_eq!(
            resolved.unselected_item_color,
            theme.unselected_widget_color
        );
        assert_eq!(resolved.selected_item_color, theme.color_scheme.primary);
    }

    #[test]
    fn a_shifting_bar_ends_at_the_surface_for_both_ends() {
        // Its items sit on a coloured background of their own, so the
        // contrast comes from the background and both ends are the surface.
        // Four items is upstream's threshold for shifting.
        let resolved = resolve(
            BottomNavigationBar::new(4, 0),
            BottomNavigationBarThemeData::new(),
        );
        assert_eq!(resolved.bar_type, BottomNavigationBarType::Shifting);
        let surface = crate::theme::ThemeData::light().color_scheme.surface;
        assert_eq!(resolved.selected_item_color, surface);
        assert_eq!(resolved.unselected_item_color, surface);
    }

    #[test]
    fn a_shifting_bar_ignores_the_older_fixed_color() {
        // `fixedColor` is in the fixed arm of upstream's switch only, and the
        // name says why.
        let mut bar = BottomNavigationBar::new(4, 0);
        bar.fixed_color = Some(ink(70));
        let resolved = resolve(bar, BottomNavigationBarThemeData::new());
        assert_eq!(
            resolved.selected_item_color,
            crate::theme::ThemeData::light().color_scheme.surface
        );
    }

    #[test]
    fn the_selected_end_of_a_fixed_bar_swaps_role_with_the_brightness() {
        // Upstream's `themeColor`: primary under a light theme, secondary
        // under a dark one. A dark theme's primary is a pale tint meant for
        // large areas; a small selected icon needs the accent.
        let dark = crate::theme::ThemeData::dark();
        let resolved = resolve_under(
            BottomNavigationBar::new(3, 0),
            BottomNavigationBarThemeData::new(),
            dark.clone(),
        );
        assert_eq!(resolved.selected_item_color, dark.color_scheme.secondary);
        assert_ne!(
            dark.color_scheme.secondary, dark.color_scheme.primary,
            "the two roles differ, so the assertion above says something"
        );
    }
}

#[cfg(test)]
mod navigation_bar_theme_tests {
    use super::*;
    use crate::component_themes::{
        NavigationBarTheme, NavigationBarThemeData, NavigationDestinationLabelBehavior,
        ResolvedNavigationBar,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component};

    struct Reader {
        bar: NavigationBar,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedNavigationBar>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.bar.resolved(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(bar: NavigationBar, data: NavigationBarThemeData) -> ResolvedNavigationBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        // No `Theme` above it: `NavigationBarTheme::of` falls back to
        // `ThemeData::of`, which has its own fallback. Wrapping one here would
        // suggest it took part in the answer.
        tree.rebuild(NavigationBarTheme::new(
            data,
            component(Reader {
                bar,
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn the_duration_is_the_one_field_the_theme_has_no_say_in() {
        // Upstream reads `animationDuration ?? 500ms` and the theme has no
        // duration field at all -- two steps where every other field has
        // three. This port's doc used to claim the theme supplied it.
        let plain = resolve(NavigationBar::new(3, 0), NavigationBarThemeData::new());
        assert_eq!(plain.animation_duration_ms, 500);

        let mut bar = NavigationBar::new(3, 0);
        bar.animation_duration_ms = Some(120);
        assert_eq!(
            resolve(bar, NavigationBarThemeData::new()).animation_duration_ms,
            120,
            "and the widget's own is the only thing that moves it"
        );
    }

    #[test]
    fn every_other_field_does_go_through_the_theme() {
        // The contrast that makes the duration worth remarking on.
        let mut data = NavigationBarThemeData::new();
        data.height = Some(64.0);
        data.elevation = Some(9.0);
        let resolved = resolve(NavigationBar::new(3, 0), data);
        assert_eq!(resolved.height, 64.0);
        assert_eq!(resolved.elevation, 9.0);
    }

    #[test]
    fn the_label_behaviour_is_a_constant_where_the_older_bars_was_computed() {
        // The M3 bar does not shift, so there is no count at which the labels
        // stop fitting -- `BottomNavigationBar` had to work its default out
        // from the item count and this one does not.
        for count in [2, 3, 4, 7] {
            assert_eq!(
                resolve(NavigationBar::new(count, 0), NavigationBarThemeData::new()).label_behavior,
                crate::component_themes::NavigationDestinationLabelBehavior::AlwaysShow
            );
        }
    }

    #[test]
    fn a_theme_can_still_ask_for_the_labels_to_come_and_go() {
        let mut data = NavigationBarThemeData::new();
        data.label_behavior =
            Some(crate::component_themes::NavigationDestinationLabelBehavior::OnlyShowSelected);
        assert_eq!(
            resolve(NavigationBar::new(3, 0), data).label_behavior,
            crate::component_themes::NavigationDestinationLabelBehavior::OnlyShowSelected
        );
    }

    #[test]
    fn the_default_height_is_the_indicators_height() {
        // A bar height chosen independently would leave the indicator floating
        // in it or clipped by it.
        assert_eq!(
            resolve(NavigationBar::new(3, 0), NavigationBarThemeData::new()).height,
            ResolvedNavigationBar::HEIGHT
        );
        assert_eq!(ResolvedNavigationBar::HEIGHT, 32.0);
    }

    #[test]
    fn nothing_is_invented_for_the_colours_upstream_leaves_null() {
        let resolved = resolve(NavigationBar::new(3, 0), NavigationBarThemeData::new());
        assert_eq!(resolved.background_color, None);
        assert_eq!(resolved.indicator_color, None);
        assert_eq!(resolved.shadow_color, None);
        assert_eq!(resolved.label_padding, crate::render::EdgeInsets::ZERO);
    }
}

#[cfg(test)]
mod bottom_app_bar_theme_tests {
    use super::*;
    use crate::EdgeInsetsGeometry;
    use crate::borders::NotchedShape;
    use crate::component_themes::{BottomAppBarTheme, BottomAppBarThemeData, ResolvedBottomAppBar};
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::EdgeInsets;
    use crate::theme::ThemeData;
    use crate::widgets::SizedBox;

    struct Reader {
        bar: BottomAppBar,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedBottomAppBar>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.bar.resolved(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn resolve(bar: BottomAppBar, data: BottomAppBarThemeData) -> ResolvedBottomAppBar {
        resolve_under(ThemeData::fallback(), bar, data)
    }

    fn resolve_under(
        theme: ThemeData,
        bar: BottomAppBar,
        data: BottomAppBarThemeData,
    ) -> ResolvedBottomAppBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::provide(
            theme,
            BottomAppBarTheme::new(
                data,
                component(Reader {
                    bar,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// A theme in the Material 2 mode, which is where the branch lives now.
    fn m2_theme() -> ThemeData {
        ThemeData {
            use_material3: false,
            ..ThemeData::fallback()
        }
    }

    #[test]
    fn the_elevation_is_an_input_to_the_colour_and_not_only_to_the_shadow() {
        // `effectiveColor` is `applySurfaceTint(color, tint, elevation)`. The
        // resolved colour is never what gets painted.
        let tint = Color(0xFF00FF00);
        let mut data = BottomAppBarThemeData::new();
        data.surface_tint_color = Some(tint);
        data.color = Some(Color(0xFF000000));

        let mut low = data.clone();
        low.elevation = Some(0.0);
        let mut high = data.clone();
        high.elevation = Some(24.0);

        let scheme = ThemeData::fallback().color_scheme;
        let painted = |data: BottomAppBarThemeData| {
            let resolved = resolve(BottomAppBar::new(), data);
            resolved.effective_color(false, scheme.surface, scheme.on_surface)
        };
        assert_ne!(
            painted(low.clone()),
            painted(high.clone()),
            "same colour, same tint, different elevation -- different paint"
        );
        assert_eq!(
            resolve(BottomAppBar::new(), low).color,
            resolve(BottomAppBar::new(), high).color,
            "and the resolved colour itself did not move, which is the point"
        );
    }

    #[test]
    fn neither_default_tints_anything_and_they_fail_to_for_opposite_reasons() {
        let scheme = ThemeData::fallback().color_scheme;

        // Material 3 consults the tint and defaults it to transparent.
        let m3 = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        assert_eq!(m3.surface_tint_color, Color::TRANSPARENT);
        assert_eq!(
            m3.effective_color(false, scheme.surface, scheme.on_surface),
            m3.color,
            "a transparent tint is short-circuited"
        );

        // Material 2 resolves a real scheme colour and takes the branch that
        // never looks at it.
        let two = resolve_under(
            m2_theme(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert_eq!(two.surface_tint_color, scheme.surface_tint());
        assert_ne!(two.surface_tint_color, Color::TRANSPARENT);
        let mut tinted = BottomAppBarThemeData::new();
        tinted.surface_tint_color = Some(Color(0xFFFF0000));
        assert_eq!(
            resolve_under(m2_theme(), BottomAppBar::new(), tinted).effective_color(
                false,
                scheme.surface,
                scheme.on_surface
            ),
            two.effective_color(false, scheme.surface, scheme.on_surface),
            "a different tint entirely, and Material 2 paints the same"
        );
    }

    #[test]
    fn material_two_leaves_the_height_to_the_child_and_material_three_pins_it() {
        assert_eq!(
            resolve(BottomAppBar::new(), BottomAppBarThemeData::new()).height,
            Some(80.0)
        );
        assert_eq!(
            resolve_under(
                m2_theme(),
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .height,
            None,
            "`SizedBox(height: null)` is as tall as what is in it"
        );
    }

    #[test]
    fn a_material_three_bar_carries_a_notch_nobody_asked_for() {
        // The finding that corrected this port: the widget's shape defaults to
        // null, and the chain does not stop there.
        let plain = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        assert!(matches!(plain.shape, Some(NotchedShape::Automatic { .. })));
        assert_eq!(
            resolve_under(
                m2_theme(),
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .shape,
            None,
            "and a Material 2 bar does not"
        );
    }

    #[test]
    fn carrying_a_shape_is_not_cutting_a_hole() {
        // Upstream's `notchedShape != null && hasFab`. With no floating action
        // button the clipper is a plain rounded rectangle, whatever shape
        // resolved.
        let plain = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        assert!(plain.shape.is_some());
        assert!(!plain.cuts_a_notch(false));
        assert!(plain.cuts_a_notch(true));

        // And a button with nothing to cut into is equally not a notch.
        let two = resolve_under(
            m2_theme(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert!(!two.cuts_a_notch(true));
    }

    #[test]
    fn the_widget_is_the_first_step_for_the_shape_and_the_theme_the_second() {
        let mine = NotchedShape::Circular { inverted: true };
        let mut data = BottomAppBarThemeData::new();
        data.shape = Some(NotchedShape::Circular { inverted: false });
        assert_eq!(
            resolve(
                BottomAppBar {
                    shape: Some(mine.clone()),
                    ..BottomAppBar::new()
                },
                data.clone()
            )
            .shape,
            Some(mine)
        );
        assert_eq!(
            resolve(BottomAppBar::new(), data).shape,
            Some(NotchedShape::Circular { inverted: false })
        );
    }

    #[test]
    fn the_two_elevations_and_the_two_colours_are_not_the_same_numbers() {
        let scheme = ThemeData::fallback().color_scheme;
        let three = resolve(BottomAppBar::new(), BottomAppBarThemeData::new());
        let two = resolve_under(
            m2_theme(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert_eq!(three.elevation, 3.0);
        assert_eq!(two.elevation, 8.0);
        assert_eq!(three.color, scheme.surface_container());
        assert_eq!(
            two.color,
            ResolvedBottomAppBar::M2_LIGHT,
            "Material 2 is plain white in the light, from before the scheme"
        );
        assert_eq!(three.shadow_color, Color::TRANSPARENT);
        assert_eq!(two.shadow_color, Color(0xFF000000));
    }

    #[test]
    fn material_twos_colour_is_the_only_thing_here_that_reads_the_brightness() {
        // A mutation deleting this branch survived: nothing built under a dark
        // theme, so the arm was unreachable and the test suite could not tell
        // it from an empty one.
        assert_eq!(
            resolve_under(
                ThemeData {
                    use_material3: false,
                    ..ThemeData::dark()
                },
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .color,
            ResolvedBottomAppBar::M2_DARK,
            "`Colors.grey[800]`"
        );
        assert_eq!(
            resolve_under(
                ThemeData {
                    use_material3: false,
                    ..ThemeData::light()
                },
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .color,
            ResolvedBottomAppBar::M2_LIGHT
        );

        // Material 3 takes its colour from the scheme, which the brightness
        // has already moved -- it does not look at the brightness itself.
        let dark = resolve_under(
            ThemeData::dark(),
            BottomAppBar::new(),
            BottomAppBarThemeData::new(),
        );
        assert_eq!(
            dark.color,
            ThemeData::dark().color_scheme.surface_container()
        );
        assert_ne!(dark.color, ResolvedBottomAppBar::M2_DARK);
    }

    #[test]
    fn the_padding_default_lives_at_the_use_site_and_still_has_a_theme_step() {
        assert_eq!(
            resolve(BottomAppBar::new(), BottomAppBarThemeData::new()).padding,
            EdgeInsets::symmetric(16.0, 12.0)
        );
        assert_eq!(
            resolve_under(
                m2_theme(),
                BottomAppBar::new(),
                BottomAppBarThemeData::new()
            )
            .padding,
            EdgeInsets::ZERO
        );
        let mut data = BottomAppBarThemeData::new();
        data.padding = Some(EdgeInsetsGeometry::Absolute(EdgeInsets::all(7.0)));
        assert_eq!(
            resolve(BottomAppBar::new(), data).padding,
            EdgeInsets::all(7.0),
            "a theme still gets its say even though the default is written elsewhere"
        );
    }

    #[test]
    fn material_twos_overlay_only_fires_in_the_dark_and_only_on_the_surface() {
        // The two transforms are not two spellings of one idea.
        let scheme = ThemeData::fallback().color_scheme;
        let mut data = BottomAppBarThemeData::new();
        data.color = Some(scheme.surface);
        let two = resolve_under(m2_theme(), BottomAppBar::new(), data.clone());
        assert_eq!(
            two.effective_color(false, scheme.surface, scheme.on_surface),
            two.color,
            "in the light it does nothing at all"
        );
        // The dark case has to be resolved under a *dark* theme, not asked of
        // the light one with `is_dark: true`. Upstream's `applyOverlay` reads
        // `theme.applyElevationOverlayColor && theme.brightness == dark`, and
        // both come off the same theme -- the flag defaults to the
        // brightness. A light theme claiming darkness is a state upstream
        // cannot reach, and this used to pass only because the flag was
        // hard-coded `true` here rather than read from the theme.
        let dark_two = resolve_under(
            ThemeData {
                use_material3: false,
                ..ThemeData::dark()
            },
            BottomAppBar::new(),
            data.clone(),
        );
        assert_ne!(
            dark_two.effective_color(true, scheme.surface, scheme.on_surface),
            dark_two.color,
            "and in the dark it lightens the surface by its elevation"
        );

        let mut mine = BottomAppBarThemeData::new();
        mine.color = Some(Color(0xFF123456));
        let hand_coloured = resolve_under(m2_theme(), BottomAppBar::new(), mine);
        assert_eq!(
            hand_coloured.effective_color(true, scheme.surface, scheme.on_surface),
            hand_coloured.color,
            "a colour someone chose is left alone even in the dark"
        );
    }

    #[test]
    fn a_theme_can_turn_the_elevation_overlay_off() {
        // `tools/unread_theme_fields.py` found
        // `ThemeData::apply_elevation_overlay_color` reaching nothing: the
        // resolver passed a literal `true` where upstream passes the theme's
        // flag, so a Material 2 application that wanted its dark surfaces
        // flat was ignored.
        //
        // Upstream's `applyOverlay` checks three things, and this is the
        // second: elevation above zero, the flag, and a dark brightness.
        let scheme = ThemeData::fallback().color_scheme;
        let mut data = BottomAppBarThemeData::new();
        data.color = Some(scheme.surface);

        let on = resolve_under(
            ThemeData {
                use_material3: false,
                ..ThemeData::dark()
            },
            BottomAppBar::new(),
            data.clone(),
        );
        assert_ne!(
            on.effective_color(true, scheme.surface, scheme.on_surface),
            on.color,
            "a dark Material 2 theme applies the overlay by default"
        );

        let off = resolve_under(
            ThemeData {
                use_material3: false,
                apply_elevation_overlay_color: false,
                ..ThemeData::dark()
            },
            BottomAppBar::new(),
            data,
        );
        assert_eq!(
            off.effective_color(true, scheme.surface, scheme.on_surface),
            off.color,
            "and the same theme with the flag off leaves the surface alone"
        );
    }
}

#[cfg(test)]
mod label_behavior_tests {
    use super::NavigationBar;
    use crate::component_themes::NavigationDestinationLabelBehavior;

    fn bar(behavior: NavigationDestinationLabelBehavior) -> NavigationBar {
        let mut bar = NavigationBar::new(4, 2);
        bar.label_behavior = behavior;
        bar
    }

    #[test]
    fn only_show_selected_is_the_one_that_reads_the_index() {
        // The ignored argument was the tell. Under the other two behaviours
        // every destination answers alike; under this one the answer is about
        // the destination rather than about the bar.
        let selective = bar(NavigationDestinationLabelBehavior::OnlyShowSelected);
        assert!(selective.shows_label(2), "the selected one");
        for other in [0, 1, 3] {
            assert!(!selective.shows_label(other), "{other}");
        }
    }

    #[test]
    fn and_the_other_two_answer_the_same_for_every_destination() {
        for (behavior, expected) in [
            (NavigationDestinationLabelBehavior::AlwaysShow, true),
            (NavigationDestinationLabelBehavior::AlwaysHide, false),
        ] {
            let bar = bar(behavior);
            for index in 0..4 {
                assert_eq!(bar.shows_label(index), expected, "{behavior:?} {index}");
            }
        }
    }

    #[test]
    fn hiding_every_label_is_a_thing_this_bar_can_do() {
        // shows_label answered true unconditionally, on reasoning borrowed
        // from Material 2's shifting mode. This is the Material 3 bar, and two
        // of its three behaviours hide labels.
        assert!(!bar(NavigationDestinationLabelBehavior::AlwaysHide).shows_label(2));
        assert!(
            !bar(NavigationDestinationLabelBehavior::OnlyShowSelected).shows_label(0),
            "and this one hides all but one"
        );
    }

    #[test]
    fn moving_the_selection_moves_which_label_shows() {
        // Through the field rather than by constructing two bars, so the
        // answer is shown to follow the selection rather than the fixture.
        let mut selective = bar(NavigationDestinationLabelBehavior::OnlyShowSelected);
        assert!(selective.shows_label(2));
        assert!(!selective.shows_label(3));
        selective.selected_index = 3;
        assert!(!selective.shows_label(2));
        assert!(selective.shows_label(3));
    }

    #[test]
    fn a_bar_shows_every_label_unless_told_otherwise() {
        let plain = NavigationBar::new(4, 2);
        assert_eq!(
            plain.label_behavior,
            NavigationDestinationLabelBehavior::AlwaysShow
        );
        for index in 0..4 {
            assert!(plain.shows_label(index), "{index}");
        }
    }
}
