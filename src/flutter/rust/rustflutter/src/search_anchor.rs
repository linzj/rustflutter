//! Ports of `material/search_anchor.dart` and `material/search.dart`.
//!
//! Searching, twice: the Material 3 `SearchAnchor` family and the older
//! `SearchDelegate`. Both open a search **route**, which is the decision that
//! shapes everything else -- a search is somewhere you go, so the system back
//! button closes it without anybody writing that down.

use crate::scroll_plumbing::ScrollPlatform;

/// Upstream `SearchController`, which extends `TextEditingController`.
///
/// One object holding both the query text and the view's open/closed state,
/// and the reason they belong together shows up in `closeView`: choosing a
/// suggestion **sets the text and then pops**, in that order, so the anchor's
/// bar already reads the chosen text while the view animates away. Two objects
/// would give you a frame of the old query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchController {
    pub text: String,
    anchor: Option<u64>,
    view_open: bool,
}

impl SearchController {
    pub fn new() -> SearchController {
        SearchController::default()
    }

    /// Upstream's `isAttached`. Every other method asserts it: a controller
    /// with no anchor cannot open a view, and asking it to is a mistake rather
    /// than a no-op.
    pub fn is_attached(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn is_open(&self) -> bool {
        self.view_open
    }

    pub fn attach(&mut self, anchor: u64) {
        self.anchor = Some(anchor);
    }

    /// Upstream's `_detach`, guarded on the anchor being the one that attached.
    /// A controller handed to a new anchor must not be detached by the old
    /// one's disposal.
    pub fn detach(&mut self, anchor: u64) {
        if self.anchor == Some(anchor) {
            self.anchor = None;
            self.view_open = false;
        }
    }

    pub fn open_view(&mut self) -> Result<(), &'static str> {
        if !self.is_attached() {
            return Err("a search controller with no anchor has no view to open");
        }
        self.view_open = true;
        Ok(())
    }

    /// Upstream `closeView`. The text is set **before** the pop.
    pub fn close_view(&mut self, selected_text: Option<&str>) -> Result<(), &'static str> {
        if !self.is_attached() {
            return Err("a search controller with no anchor has no view to close");
        }
        if let Some(text) = selected_text {
            self.text = text.to_string();
        }
        self.view_open = false;
        Ok(())
    }
}

/// A [`SearchController`] two widgets can hold at once.
///
/// Upstream's controller *is* a `TextEditingController`, and the bar and the
/// view's header are given the same one -- which is the whole mechanism by
/// which the words typed into the bar are already in the view that grows out
/// of it. There is no shared-controller machinery in this crate to reach for:
/// what there is, and what this follows, is the sink `TextField` already
/// uses -- an `Rc<RefCell<..>>` handed to both ends
/// ([`crate::editable::TextField::with_state_sink`]).
pub type SharedSearchController = std::rc::Rc<std::cell::RefCell<SearchController>>;

/// Why a search view closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewDismissal {
    /// The window changed size and the view was docked to an anchor that may no
    /// longer be where it was.
    WindowResizedWhileDocked,
    /// Nothing: a full-screen view is not attached to anything, so rotating the
    /// device merely lays it out again.
    Kept,
}

/// Upstream `SearchAnchor`.
#[derive(Clone)]
pub struct SearchAnchor {
    /// Identifies the bar, its field and its header, so that the text somebody
    /// typed into the bar is still there in the view that grew out of it.
    pub id: u64,
    /// `None` means "decide from the platform". Upstream shows the view full
    /// screen on mobile only.
    pub is_full_screen: Option<bool>,
    /// Upstream's `viewHintText`, which the bar and the view's header share.
    pub hint_text: Option<String>,
    /// Upstream's `suggestionsBuilder`. Absent means an empty view -- which is
    /// what a search with nothing typed into it shows anyway.
    suggestions: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    /// Upstream's `searchController`. `None` means the anchor keeps one of its
    /// own, which is upstream's `_internalSearchController`.
    controller: Option<SharedSearchController>,
}

impl std::fmt::Debug for SearchAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchAnchor")
            .field("id", &self.id)
            .field("is_full_screen", &self.is_full_screen)
            .field("hint_text", &self.hint_text)
            .finish_non_exhaustive()
    }
}

/// The parts a reader can see. The suggestions builder is left out for the
/// reason [`SearchBar`]'s callbacks are: a closure made afresh each build is
/// never equal to the last one.
impl PartialEq for SearchAnchor {
    fn eq(&self, other: &SearchAnchor) -> bool {
        self.id == other.id
            && self.is_full_screen == other.is_full_screen
            && self.hint_text == other.hint_text
    }
}

/// What an anchor keeps between builds.
///
/// # Only the anchor, and what is not kept
///
/// The [`crate::theatre::ModalHandle`] the tap gets back is **dropped**. That
/// is deliberate: nothing here can use it. The view is dismissed through its
/// own barrier, and a second tap where the bar was lands on that barrier
/// rather than on the bar, so there is no "already open" to guard against.
///
/// Upstream's `SearchController.closeView` *does* close a view from the
/// outside, and porting it needs somebody to hold the handle -- a controller
/// the caller owns, which this crate's `SearchController` is not wired to yet.
/// Keeping the handle here in the meantime would be state that is written
/// every open and read by nothing.
#[derive(Default)]
pub struct SearchAnchorState {
    /// Where the bar is. Filled in from the bar's own assemble, which is the
    /// moment its render object exists, and read when the bar is tapped --
    /// which is the moment the question can be answered.
    anchor: crate::theatre::Anchor,
    /// Upstream's `_internalSearchController`, used when the caller did not
    /// bring one.
    controller: SharedSearchController,
    /// The bar's field, published from its own build, so that the query can be
    /// put back into it when the view closes.
    ///
    /// In the state rather than made per build, which is where this crate puts
    /// a sink -- the same place the [`crate::theatre::Anchor`] above it lives.
    /// **A per-build sink would behave identically today** and a mutation
    /// sweep says so: the tap's closure would hold the sink from its own
    /// build, and the handle in it stays good as long as the element does, so
    /// the query still arrives. It stops being identical only when the bar's
    /// element is replaced rather than rebuilt -- and by then the text it held
    /// is gone too, so there is nothing to put back. Kept in the state because
    /// one sink for one bar is the thing being described.
    field: FieldSink,
}

impl SearchAnchor {
    pub fn new(id: u64) -> SearchAnchor {
        SearchAnchor {
            id,
            is_full_screen: None,
            hint_text: None,
            suggestions: None,
            controller: None,
        }
    }

    /// Upstream's `searchController`, for a caller that wants to read the
    /// query or set it.
    pub fn with_controller(mut self, controller: SharedSearchController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// The controller this anchor works through: the caller's when there is
    /// one, and otherwise the anchor's own -- upstream's
    /// `_internalSearchController`.
    ///
    /// **The internal one comes from the state**, so it is the same object on
    /// every build. One made afresh in `build` would be written to by the bar
    /// and thrown away before the view could read it, and the difference would
    /// only show up as a query that goes missing on opening.
    pub fn controller_for(&self, state: &SearchAnchorState) -> SharedSearchController {
        self.controller
            .clone()
            .unwrap_or_else(|| std::rc::Rc::clone(&state.controller))
    }

    /// The bar this anchor puts on the page, wired to `controller` but not yet
    /// to the tap that opens the view.
    ///
    /// Split out from `build` because the wiring is the interesting part and
    /// `build` needs a live tree to run: a bar built here can be asked whether
    /// typing into it reaches the controller.
    pub fn bar_for(&self, controller: &SharedSearchController) -> SearchBar {
        self.bar_into(controller, None)
    }

    /// [`SearchAnchor::bar_for`], with the bar's field published into `sink`.
    pub fn bar_into(
        &self,
        controller: &SharedSearchController,
        sink: Option<FieldSink>,
    ) -> SearchBar {
        let mut bar = SearchBar::new(self.id);
        if let Some(sink) = sink {
            bar = bar.with_state_sink(sink);
        }
        bar.is_anchor_child = true;
        if let Some(hint) = &self.hint_text {
            bar = bar.with_hint_text(hint.clone());
        }
        // Typing in the bar writes to the controller, so that the view opens
        // holding the same words. Upstream does not need this line: there, the
        // bar's field *is* the controller's field.
        let writing = std::rc::Rc::clone(controller);
        let mut bar = bar.with_on_changed(move |text| {
            writing.borrow_mut().text = text.to_string();
        });
        bar.initial_text = Some(controller.borrow().text.clone());
        bar
    }

    pub fn with_hint_text(mut self, hint: impl Into<String>) -> Self {
        self.hint_text = Some(hint.into());
        self
    }

    /// Upstream's `suggestionsBuilder`.
    pub fn with_suggestions(
        mut self,
        suggestions: impl Fn() -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.suggestions = Some(std::rc::Rc::new(suggestions));
        self
    }

    pub fn with_full_screen(mut self, full: bool) -> Self {
        self.is_full_screen = Some(full);
        self
    }

    pub fn resolve_full_screen(&self, platform: ScrollPlatform) -> bool {
        self.is_full_screen.unwrap_or(matches!(
            platform,
            ScrollPlatform::Android | ScrollPlatform::IOS | ScrollPlatform::Fuchsia
        ))
    }

    /// Upstream's documented asymmetry, and it has a reason worth keeping:
    /// *"The search view route will be popped if the window size is changed and
    /// the search view route is not in full-screen mode. However, if the search
    /// view route is in full-screen mode, changing the window size ... will not
    /// close the search view."*
    ///
    /// **The one that is attached to something has to go when that something
    /// moves; the one that is not, does not.** A docked view is positioned
    /// against an anchor widget whose place on screen just changed, and it has
    /// nowhere to be. A full-screen view was never anchored, so rotating the
    /// device only lays it out again.
    pub fn on_window_resized(&self, platform: ScrollPlatform) -> ViewDismissal {
        if self.resolve_full_screen(platform) {
            ViewDismissal::Kept
        } else {
            ViewDismissal::WindowResizedWhileDocked
        }
    }

    /// The view's appearance. Full screen is an input to the defaults and not
    /// only to the layout -- see
    /// [`crate::component_themes::ResolvedSearchView`].
    pub fn resolved_view(
        &self,
        context: &mut crate::framework::BuildContext,
        platform: ScrollPlatform,
    ) -> crate::component_themes::ResolvedSearchView {
        crate::component_themes::ResolvedSearchView::of(context, self.resolve_full_screen(platform))
    }

    /// Opening the view pushes a route, so the system back gesture closes it
    /// without the anchor arranging anything.
    pub fn view_is_a_route() -> bool {
        true
    }

    /// Where the opened view ends up: upstream's `_SearchViewRoute
    /// .updateTweens`, whose `_rectTween.end` this is.
    ///
    /// **The tween's `begin` is `anchor` itself**, which is the whole idea:
    /// the view grows out of the bar that was tapped rather than appearing
    /// over it, so a reader's eye follows one object instead of losing the bar
    /// and finding a panel.
    ///
    /// # The two sizes come from different places, and that is deliberate
    ///
    /// The width is the **anchor's**, clamped: a view that opens under a bar
    /// should be the width of that bar, because it is the same field
    /// continued. The height is **two thirds of the screen**, clamped, and has
    /// nothing to do with the anchor -- a 56-tall bar cannot say how much room
    /// a list of suggestions wants, and the answer that scales is a fraction
    /// of the window.
    ///
    /// # Off the edge: the corner moves, the size does not
    ///
    /// When there is not enough room to the right (or below) for the view, its
    /// top-left corner is pulled back so that it fits. Upstream's comment
    /// there says *"If the window is smaller than the view, then we resize the
    /// view to fit the window"* -- **its code does not resize**. The `min` is
    /// applied to the corner's position and `endSize` stays
    /// `Size(viewWidth, viewHeight)`, so a view wider than the window starts
    /// at the window's edge and runs off the far side. Ported as written
    /// rather than as described: the comment is the thing that is wrong, and a
    /// port that quietly "fixed" it would lay windows out differently from
    /// upstream for a reason nobody could find later.
    ///
    /// # A full-screen view ignores all of it
    ///
    /// It is the screen. It has no anchor to grow from in any meaningful
    /// sense, which is also why [`SearchAnchor::on_window_resized`] lets it
    /// live through a resize while a docked one has to go.
    pub fn view_rect(
        anchor: crate::engine::Rect,
        screen: crate::render::Size,
        constraints: crate::render::BoxConstraints,
        full_screen: bool,
        direction: crate::direction::TextDirection,
    ) -> crate::engine::Rect {
        if full_screen {
            return crate::engine::Rect::xywh(0.0, 0.0, screen.width, screen.height);
        }
        let width = anchor
            .width()
            .clamp(constraints.min_width, constraints.max_width);
        let height =
            (screen.height * 2.0 / 3.0).clamp(constraints.min_height, constraints.max_height);

        let mut left = match direction {
            crate::direction::TextDirection::Ltr => anchor.left,
            // Anchored by its right edge instead, and never past the left one.
            //
            // Upstream then writes the mirror of the left-to-right correction
            // below -- `if (viewRightToScreenLeft < viewWidth) topLeft =
            // Offset(0.0, topLeft.dy)` -- and **it cannot change the answer**.
            // Its condition is `anchorRect.right < viewWidth`, and in exactly
            // that case the `max` above has already produced zero. It is not
            // ported, because a branch that no input can reach is a branch no
            // test can hold: a sweep that mutates it stays green, and the
            // green would look like missing coverage rather than what it is.
            crate::direction::TextDirection::Rtl => (anchor.right - width).max(0.0),
        };
        let mut top = anchor.top;

        // Left-to-right only, per the above.
        if matches!(direction, crate::direction::TextDirection::Ltr)
            && screen.width - anchor.left < width
        {
            left = screen.width - width.min(screen.width);
        }
        if screen.height - anchor.top < height {
            top = screen.height - height.min(screen.height);
        }
        crate::engine::Rect::xywh(left, top, width, height)
    }
}

impl Default for SearchAnchor {
    fn default() -> Self {
        SearchAnchor::new(0)
    }
}

impl crate::framework::StatefulComponent for SearchAnchor {
    type State = SearchAnchorState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &SearchAnchorState,
        _handle: crate::framework::StateHandle<SearchAnchorState>,
        context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        // Everything the view will need is read **here**, under the anchor's
        // own themes, and carried into the tap. The view is built in the
        // overlay, which is not below them -- upstream takes the same capture
        // at the same place, `InheritedTheme.capture(from: context, ...)`.
        let full_screen =
            self.resolve_full_screen(crate::theme::ThemeData::of(context).platform.into());
        let view = crate::component_themes::ResolvedSearchView::of(context, full_screen);
        let media = crate::media_query::media_query_of(context);
        let overlay = crate::theatre::OverlayHandle::of(context);
        let themes = context.capture_themes();
        let direction = crate::direction::current_direction();

        let anchor = state.anchor.clone();
        let id = self.id;
        let hint_text = self.hint_text.clone();
        let suggestions = self.suggestions.clone();
        let controller = self.controller_for(state);
        let field = std::rc::Rc::clone(&state.field);
        let bar = self.bar_into(&controller, Some(std::rc::Rc::clone(&field)));
        let controller_for_close = std::rc::Rc::clone(&controller);
        let bar = bar.with_on_tap(move || {
            // There is no guard here against opening a second view, and there
            // is nothing to guard: while a view is up its barrier is over the
            // bar, so the bar cannot be tapped at all. A tap where the bar is
            // *closes* the view instead -- which is the behaviour a reader
            // expects and is covered by
            // `tapping_where_the_bar_was_closes_the_view_over_it`.
            //
            // A guard would read as caution and be a branch no input can
            // reach, which is the shape round 436 recorded: a mutation sweep
            // cannot hold it, and the green would look like missing coverage
            // rather than like nothing being there.
            let (Some(overlay), Some(rect)) = (overlay.clone(), anchor.rect()) else {
                return;
            };
            let screen = media.size;
            let placement = SearchViewTransition {
                anchor: rect,
                view: SearchAnchor::view_rect(
                    rect,
                    screen,
                    view.constraints,
                    full_screen,
                    direction,
                ),
                full_screen,
            };
            let content = SearchViewContent {
                placement,
                view: view.clone(),
                constraints: view.constraints,
                screen,
                // Nothing has been typed yet, so upstream's
                // `result.isNotEmpty` is false on the frame the view opens.
                has_results: false,
                media_top: media.padding.top,
                header_id: id,
                hint_text: hint_text.clone(),
                controller: std::rc::Rc::clone(&controller),
            };
            let suggestions = suggestions.clone();
            let themes = themes.clone();
            let shown = open_search_view(overlay, content, move || {
                let built = match &suggestions {
                    Some(suggestions) => suggestions(),
                    None => crate::framework::leaf(|| crate::widgets::SizedBox::new(0.0, 0.0)),
                };
                themes.wrap(built)
            });
            // Whatever the reader ended up with goes back into the bar, and it
            // has to happen **when the view comes down** rather than when
            // somebody closes it: the usual way out is a tap on the barrier,
            // which nothing here is told about otherwise. That is what
            // `ModalHandle::on_dismissed` is for.
            if let Some(shown) = shown {
                let controller = std::rc::Rc::clone(&controller_for_close);
                let field = std::rc::Rc::clone(&field);
                shown.on_dismissed(move || {
                    put_query_back(&field, &controller.borrow().text);
                });
            }
        });

        let anchor = state.anchor.clone();
        crate::framework::many(vec![crate::framework::stateful(bar)], move |rendered| {
            let bar = rendered.into_iter().next().expect("the bar");
            // Recorded from the bar's own assemble: this is where its render
            // object first exists, and the rectangle it will be asked for is
            // the one the view grows out of.
            anchor.set(bar.clone());
            crate::theatre::RenderPortal::new(bar)
        })
    }
}

/// How the view gets from the bar to where [`SearchAnchor::view_rect`] put it:
/// upstream's `_SearchViewRoute.buildPage` and the five constants above it.
///
/// # One animation, six curves off it
///
/// The route runs a single 600ms animation, and everything the view does is a
/// different curve over **that same parent**. That is the part worth keeping
/// straight, because it is what makes the opening read as one movement with
/// things arriving inside it rather than as six animations that happen to
/// start together:
///
/// * the **rectangle** grows on `easeInOutCubicEmphasized`, which is 95% of
///   the way there by the half-way point -- so the view is essentially in
///   place while the second half is still running,
/// * the **view itself** fades in over the first half (`Interval(0, 1/2)`),
/// * the **divider** over the first sixth, the **icons** over the second
///   sixth, and the **list** from 133ms to 233ms.
///
/// The staggering is why the intervals are read off the *raw* animation and
/// not off the eased one. Upstream builds each as `CurvedAnimation(parent:
/// animation, curve: <interval>)`, with `animation` -- not `curvedAnimation`
/// -- as the parent. Feeding them the emphasized value instead would compress
/// all four fades into the first fifth of the time, because that is where the
/// emphasized curve spends its distance, and the staggering would collapse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchViewTransition {
    /// Where the bar is: the tween's `begin`.
    pub anchor: crate::engine::Rect,
    /// Where the view goes, from [`SearchAnchor::view_rect`].
    pub view: crate::engine::Rect,
    pub full_screen: bool,
}

impl SearchViewTransition {
    /// Upstream's `_kOpenViewMilliseconds`, which is also the denominator of
    /// the list's interval -- the 133 and 233 below are milliseconds, written
    /// upstream as fractions of this.
    pub const OPEN_MILLISECONDS: i64 = 600;
    pub const OPEN_MICROS: i64 = SearchViewTransition::OPEN_MILLISECONDS * 1000;
    /// Upstream's `_kAnchorFadeDuration`: how long the *bar* takes to fade out
    /// under the opening view. Not the same 600 -- the bar is gone long before
    /// the view has finished arriving.
    pub const ANCHOR_FADE_MICROS: i64 = 150 * 1000;

    /// Upstream's `curve: Curves.easeInOutCubicEmphasized` on the rect.
    pub const CURVE: crate::animation::Curve =
        crate::animation::Curve::EASE_IN_OUT_CUBIC_EMPHASIZED;

    /// Upstream's `_kViewFadeOnInterval`.
    pub fn view_fade() -> crate::curves2d::Interval {
        crate::curves2d::Interval::new(0.0, 1.0 / 2.0)
    }

    /// Upstream's `_kViewIconsFadeOnInterval`. It **starts after** the divider
    /// has finished: the frame the panel's edge is drawn on is not the frame
    /// its buttons appear on.
    pub fn icons_fade() -> crate::curves2d::Interval {
        crate::curves2d::Interval::new(1.0 / 6.0, 2.0 / 6.0)
    }

    /// Upstream's `_kViewDividerFadeOnInterval`.
    pub fn divider_fade() -> crate::curves2d::Interval {
        crate::curves2d::Interval::new(0.0, 1.0 / 6.0)
    }

    /// Upstream's `_kViewListFadeOnInterval`, written there as
    /// `133 / _kOpenViewMilliseconds` to `233 / _kOpenViewMilliseconds`.
    ///
    /// The odd numbers are the point: this one is specified in milliseconds
    /// rather than in sixths, so it is the only one that would change meaning
    /// if the duration ever did.
    pub fn list_fade() -> crate::curves2d::Interval {
        crate::curves2d::Interval::new(
            133.0 / SearchViewTransition::OPEN_MILLISECONDS as f32,
            233.0 / SearchViewTransition::OPEN_MILLISECONDS as f32,
        )
    }

    /// The emphasized curve at `t`, flipped when the route is closing.
    ///
    /// Upstream passes `reverseCurve: Curves.easeInOutCubicEmphasized.flipped`
    /// for the reason every reversing animation does: a curve that leaves
    /// slowly should also *arrive* slowly when played backwards, and replaying
    /// the forward curve would make the close snap away and drift in.
    pub fn eased(t: f32, direction: crate::animation::AnimationStatus) -> f32 {
        crate::animation::curve_for_direction(
            direction,
            SearchViewTransition::CURVE,
            Some(SearchViewTransition::CURVE.flipped()),
        )
        .transform(t)
    }

    /// One of the four fades at `t`, flipped when the route is closing.
    ///
    /// `Interval` is not a [`crate::animation::Curve`] here, so the flip is
    /// written out: upstream's `FlippedCurve.transform` is `1 - curve(1 - t)`,
    /// evaluated at the animation's current value rather than at a reversed
    /// clock.
    pub fn fade_at(
        fade: crate::curves2d::Interval,
        t: f32,
        direction: crate::animation::AnimationStatus,
    ) -> f32 {
        match direction {
            crate::animation::AnimationStatus::Reverse => 1.0 - fade.transform(1.0 - t),
            _ => fade.transform(t),
        }
    }

    /// Where the view is at `t`: upstream's `_rectTween.evaluate`.
    pub fn rect_at(
        &self,
        t: f32,
        direction: crate::animation::AnimationStatus,
    ) -> crate::engine::Rect {
        use crate::animation::Animatable;
        crate::animation::RectTween {
            begin: self.anchor,
            end: self.view,
        }
        .transform(SearchViewTransition::eased(t, direction))
    }

    /// The inset the view keeps clear at the top, which only a full-screen
    /// view has: upstream's `showFullScreenView ? lerpDouble(0.0,
    /// MediaQuery.paddingOf(context).top, curvedAnimation.value) : 0.0`.
    ///
    /// It grows with the view rather than being there from the start, and the
    /// reason is what the inset is *for*: it holds content out from under the
    /// status bar, and a view that is still a small rectangle floating over
    /// the middle of the screen is not under the status bar yet.
    pub fn top_padding_at(
        &self,
        t: f32,
        direction: crate::animation::AnimationStatus,
        media_top: f32,
    ) -> f32 {
        if !self.full_screen {
            return 0.0;
        }
        media_top * SearchViewTransition::eased(t, direction)
    }
}

/// What a caller has said about a [`SearchBar`]'s appearance, overriding the
/// theme.
///
/// # Why these are plain values and upstream's are properties
///
/// Upstream's `SearchBar` takes each of these as a
/// `WidgetStateProperty<T>` -- a function of the states -- so that a bar can
/// be a different colour while pressed. Here they are plain values, and the
/// reason that is faithful rather than a simplification is who sets them:
/// **the view's header passes `WidgetStatePropertyAll` for every one of
/// them**, which is a property that ignores the states. A search bar used as
/// a header is transparent pressed, hovered or idle, because the panel behind
/// it is the thing being seen.
///
/// A caller that genuinely wants a state-dependent colour has the theme, whose
/// fields *are* properties, and which this overrides rather than replaces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchBarOverrides {
    pub background_color: Option<crate::engine::Color>,
    pub overlay: Option<crate::engine::Color>,
    pub elevation: Option<f32>,
    pub text_style: Option<crate::engine::TextStyle>,
    pub hint_style: Option<crate::engine::TextStyle>,
    pub constraints: Option<crate::render::BoxConstraints>,
    pub padding: Option<crate::render::EdgeInsets>,
}

impl SearchBarOverrides {
    /// Lays these over a resolved appearance. Anything not set is left alone,
    /// which is what makes this an override and not a replacement.
    pub fn apply(
        &self,
        mut resolved: crate::component_themes::ResolvedSearchBar,
    ) -> crate::component_themes::ResolvedSearchBar {
        if let Some(color) = self.background_color {
            resolved.background_color = color;
        }
        if let Some(overlay) = self.overlay {
            resolved.overlay = overlay;
        }
        if let Some(elevation) = self.elevation {
            resolved.elevation = elevation;
        }
        if let Some(style) = &self.text_style {
            resolved.text_style = Some(style.clone());
        }
        if let Some(style) = &self.hint_style {
            resolved.hint_style = Some(style.clone());
        }
        if let Some(constraints) = self.constraints {
            resolved.constraints = constraints;
        }
        if let Some(padding) = self.padding {
            resolved.padding = padding;
        }
        resolved
    }
}

/// Puts `text` into a published field, with the caret at the end.
///
/// # Why not just rebuild the bar with a new `initial_text`
///
/// Because `initial_text` is spent once, when the field first appears -- it is
/// upstream's *initial* value and nothing else. The field the view was opened
/// from is still mounted and still holds the text it had, so the only way to
/// change it is to reach the state that exists. That is what the sink is for.
///
/// Nothing happens when the sink is empty or the handle is stale: a bar that
/// has gone away has no text to put anything into, and upstream's controller
/// is equally quiet about it.
pub fn put_query_back(field: &FieldSink, text: &str) -> bool {
    let Some(handle) = field.borrow().clone() else {
        return false;
    };
    let value = crate::services::text_input::TextEditingValue::new(text);
    handle.set_state(move |state| state.value = value)
}

/// A published handle on a bar's text field: the crate's sink idiom, spelled
/// once.
pub type FieldSink = std::rc::Rc<
    std::cell::RefCell<Option<crate::framework::StateHandle<crate::editable::TextFieldState>>>,
>;

/// Upstream `SearchBar`: the field an anchor usually puts in front of its view.
///
/// It is a widget in its own right rather than part of the anchor, because a
/// caller may want the bar without the view -- and because an anchor's builder
/// may return an icon instead, in which case, as upstream notes, *"we don't
/// have to explicitly call `SearchController.openView`"*: an untappable widget
/// gets the tap handling for free.
#[derive(Clone)]
pub struct SearchBar {
    /// Identifies the bar's field and its ink, so that a rebuilt bar keeps the
    /// text somebody was typing.
    pub id: u64,
    pub hint_text: Option<String>,
    /// What the field starts with. See [`SearchBar::with_initial_text`].
    pub initial_text: Option<String>,
    pub has_leading: bool,
    pub has_trailing: bool,
    /// Whether this bar is the thing an anchor taps through.
    pub is_anchor_child: bool,
    /// Upstream's `enabled`. A disabled bar is dimmed and takes no pointers;
    /// it is *not* a `WidgetState::Disabled` on the states controller, because
    /// upstream never puts one there -- see [`SearchBar::DISABLED_OPACITY`].
    pub enabled: bool,
    /// Upstream's `leading` and `trailing`. Closures rather than widgets
    /// because a widget is built per build, and these are rebuilt with the
    /// bar; `has_leading`/`has_trailing` say whether they are there without
    /// running them.
    leading: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    trailing: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    on_changed: Option<std::rc::Rc<dyn Fn(&str)>>,
    on_submitted: Option<std::rc::Rc<dyn Fn(&str)>>,
    on_tap: Option<std::rc::Rc<dyn Fn()>>,
    overrides: SearchBarOverrides,
    /// Where the bar publishes its field's [`crate::framework::StateHandle`],
    /// so that something outside the bar can put text into it after it has
    /// been built. See [`SearchBar::with_state_sink`].
    state_sink: Option<FieldSink>,
}

impl std::fmt::Debug for SearchBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchBar")
            .field("id", &self.id)
            .field("hint_text", &self.hint_text)
            .field("initial_text", &self.initial_text)
            .field("has_leading", &self.has_leading)
            .field("has_trailing", &self.has_trailing)
            .field("is_anchor_child", &self.is_anchor_child)
            .field("enabled", &self.enabled)
            .field("overrides", &self.overrides)
            .finish_non_exhaustive()
    }
}

/// Two bars are the same bar when the parts a reader can see are the same.
/// The callbacks are deliberately left out: a closure built afresh each build
/// is never equal to the last one, so comparing them would make every bar
/// differ from itself and defeat the point of asking.
impl PartialEq for SearchBar {
    fn eq(&self, other: &SearchBar) -> bool {
        self.id == other.id
            && self.hint_text == other.hint_text
            && self.initial_text == other.initial_text
            && self.has_leading == other.has_leading
            && self.has_trailing == other.has_trailing
            && self.is_anchor_child == other.is_anchor_child
            && self.enabled == other.enabled
            && self.overrides == other.overrides
    }
}

impl Default for SearchBar {
    fn default() -> SearchBar {
        SearchBar::new(0)
    }
}

/// What a [`SearchBar`] remembers between builds: which states it is in.
///
/// Upstream keeps this in a `MaterialStatesController` that the `InkWell`
/// writes and the build reads, and the whole reason the bar owns one is that
/// its *surface* has to change with the pointer -- the ink alone would tint
/// only the ink's own rectangle, and the bar wants the hover to read as the
/// whole pill lighting up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchBarState {
    pub states: crate::widget_state::WidgetStates,
}

impl SearchBar {
    /// Upstream's `_kDisableSearchBarOpacity`.
    ///
    /// A disabled bar is drawn at this opacity rather than in a disabled
    /// colour, which is why there is no `WidgetState::Disabled` anywhere in
    /// the bar: the fade is applied over the finished bar, so the background,
    /// the shadow, the hint and any leading icon all dim together and by the
    /// same amount. Resolving a disabled colour per part would have let them
    /// drift.
    pub const DISABLED_OPACITY: f32 = 0.38;

    pub fn new(id: u64) -> SearchBar {
        SearchBar {
            id,
            hint_text: None,
            initial_text: None,
            has_leading: false,
            has_trailing: false,
            is_anchor_child: false,
            enabled: true,
            leading: None,
            trailing: None,
            on_changed: None,
            on_submitted: None,
            on_tap: None,
            overrides: SearchBarOverrides::default(),
            state_sink: None,
        }
    }

    /// Publishes the bar's field, the way
    /// [`crate::editable::TextField::with_state_sink`] does -- and for the same
    /// reason. `initial_text` is spent when the field is first built; a bar
    /// that has to show text chosen somewhere else *afterwards* needs to reach
    /// the field that already exists.
    pub fn with_state_sink(mut self, sink: FieldSink) -> Self {
        self.state_sink = Some(sink);
        self
    }

    /// Upstream's per-instance appearance arguments, all at once. See
    /// [`SearchBarOverrides`] for why they are plain values here.
    pub fn with_overrides(mut self, overrides: SearchBarOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    pub fn with_hint_text(mut self, hint: impl Into<String>) -> Self {
        self.hint_text = Some(hint.into());
        self
    }

    /// The text the bar's field opens with, upstream's `controller`'s value at
    /// the moment the field is built. Used once, when the field first appears
    /// -- see [`crate::editable::TextField::initial_text`].
    pub fn with_initial_text(mut self, text: impl Into<String>) -> Self {
        self.initial_text = Some(text.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's `leading`, typically an icon.
    pub fn with_leading(
        mut self,
        leading: impl Fn() -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.leading = Some(std::rc::Rc::new(leading));
        self.has_leading = true;
        self
    }

    /// Upstream's `trailing`, which is a list there. One is enough here until
    /// something asks for more, and the row takes them the same way.
    pub fn with_trailing(
        mut self,
        trailing: impl Fn() -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.trailing = Some(std::rc::Rc::new(trailing));
        self.has_trailing = true;
        self
    }

    pub fn with_on_changed(mut self, changed: impl Fn(&str) + 'static) -> Self {
        self.on_changed = Some(std::rc::Rc::new(changed));
        self
    }

    pub fn with_on_submitted(mut self, submitted: impl Fn(&str) + 'static) -> Self {
        self.on_submitted = Some(std::rc::Rc::new(submitted));
        self
    }

    /// Runs this bar's `onChanged`, which is what its field does when the
    /// text in it changes.
    ///
    /// Public because the wiring around a bar is a thing worth asking about
    /// from outside it: an anchor hands the bar a handler that writes the
    /// query into a shared controller, and whether that handler is actually
    /// attached is not visible in anything the bar draws. Upstream's
    /// `TextField` calls `widget.onChanged` at this same point.
    pub fn notify_changed(&self, text: &str) {
        if let Some(changed) = &self.on_changed {
            changed(text);
        }
    }

    pub fn with_on_tap(mut self, tap: impl Fn() + 'static) -> Self {
        self.on_tap = Some(std::rc::Rc::new(tap));
        self
    }

    /// This bar's appearance, with the theme and the M3 defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        states: crate::widget_state::WidgetStates,
    ) -> crate::component_themes::ResolvedSearchBar {
        // The overrides go on here rather than at each use, so that the three
        // resolutions a build does -- idle, hovered, pressed -- cannot
        // disagree about them.
        self.overrides
            .apply(crate::component_themes::ResolvedSearchBar::of(
                context, states,
            ))
    }

    /// Whether the anchor has to wire up a tap itself.
    pub fn anchor_supplies_tap_handling(builder_returns_tappable: bool) -> bool {
        !builder_returns_tappable
    }

    /// The bar's surface, as upstream's `Material(elevation:, shadowColor:,
    /// color:, surfaceTintColor:, shape:)` around everything else.
    ///
    /// The shape goes onto the render object rather than being reduced to a
    /// radius here, because the default is a `StadiumBorder` and a stadium's
    /// radius is half the shorter side -- which nobody knows until the bar has
    /// been laid out inside `constraints`, whose `maxHeight` is unbounded.
    fn surface(
        resolved: &crate::component_themes::ResolvedSearchBar,
        child: impl crate::render::RenderBox + 'static,
    ) -> crate::render::RenderDecoratedBox {
        // Upstream's `Material` tints its colour by its elevation when a
        // surface tint is given; the search bar's default tint is transparent,
        // so at the default this is the background unchanged.
        let background = crate::elevation_overlay::ElevationOverlay::apply_surface_tint(
            resolved.background_color,
            Some(resolved.surface_tint_color),
            resolved.elevation,
        );
        // The elevation shadows are shaped by the elevation and coloured by
        // `shadowColor`: each keeps the alpha its layer was defined with -- the
        // umbra is denser than the ambient, and that difference is what makes
        // the shadow read as a shadow -- and takes its hue from the theme.
        let shadows = crate::painting::elevation_shadows(resolved.elevation.max(0.0) as u32)
            .iter()
            .map(|shadow| crate::painting::BoxShadow {
                color: resolved.shadow_color.with_alpha(shadow.color.alpha()),
                ..*shadow
            })
            .collect::<Vec<_>>();
        crate::render::RenderDecoratedBox::new()
            .with_fill(crate::render::Fill::Solid(background))
            .with_shadows(shadows)
            .with_shape(resolved.shape.clone())
            .with_child(child)
    }
}

impl crate::framework::StatefulComponent for SearchBar {
    type State = SearchBarState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &SearchBarState,
        handle: crate::framework::StateHandle<SearchBarState>,
        context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        use crate::widget_state::WidgetState;

        let resolved = self.resolved(context, state.states);
        // Upstream hands the whole `overlayColor` property to the `InkWell`,
        // which resolves it per state as the pointer moves. Resolving it here
        // for each state separately is the same thing said in this crate's
        // shape: the ink takes one colour per state, not a property.
        let hovered = self
            .resolved(context, state.states.with(WidgetState::Hovered))
            .overlay;
        let pressed = self
            .resolved(context, state.states.with(WidgetState::Pressed))
            .overlay;

        // The field is built inside the closure rather than outside it: the
        // ink runs this every time it rebuilds, and a `TextField` holds
        // callbacks, so it is made fresh each time rather than cloned.
        let id = self.id;
        let initial_text = self.initial_text.clone();
        let state_sink = self.state_sink.clone();
        let hint_style = resolved.hint_style.clone().unwrap_or_default();
        let text_style = resolved.text_style.clone();
        let hint_text = self.hint_text.clone();
        let on_changed = self.on_changed.clone();
        let on_submitted = self.on_submitted.clone();

        let leading = self.leading.clone();
        let trailing = self.trailing.clone();
        let padding = resolved.padding;
        let row = move || {
            let mut field = crate::editable::TextField::new(id).with_hint_style(hint_style.clone());
            if let Some(text) = &initial_text {
                field = field.with_initial_text(text.clone());
            }
            if let Some(sink) = &state_sink {
                field = field.with_state_sink(std::rc::Rc::clone(sink));
            }
            if let Some(style) = text_style.clone() {
                field = field.with_style(style);
            }
            if let Some(hint) = &hint_text {
                field = field.with_placeholder(hint.clone());
            }
            if let Some(changed) = &on_changed {
                let changed = changed.clone();
                field = field.with_on_changed(move |text| changed(text));
            }
            if let Some(submitted) = &on_submitted {
                let submitted = submitted.clone();
                field = field.with_on_submitted(move |text| submitted(text));
            }

            let mut children = Vec::new();
            let has_leading = leading.is_some();
            if let Some(leading) = &leading {
                children.push(leading());
            }
            children.push(crate::framework::stateful(field));
            let has_trailing = trailing.is_some();
            if let Some(trailing) = &trailing {
                children.push(trailing());
            }
            crate::framework::many(children, move |rendered| {
                let mut rendered = rendered.into_iter();
                let mut row = crate::widgets::Row::new()
                    .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Center);
                if has_leading {
                    row = row.push(rendered.next().expect("the leading widget"));
                }
                // The padding is applied twice on purpose, and upstream does
                // the same: once around the whole row, and again around the
                // field alone. That is what keeps the text off the leading
                // icon -- without the inner one, an icon and the first letter
                // would touch.
                row = row.push_flex(crate::widgets::FlexChild::expanded(
                    crate::widgets::Padding::new(padding, rendered.next().expect("the field")),
                    1,
                ));
                if has_trailing {
                    row = row.push(rendered.next().expect("the trailing widget"));
                }
                crate::widgets::Padding::new(padding, row)
            })
        };

        let shape = resolved.shape.clone();
        let on_tap = self.on_tap.clone();
        let ink = crate::ink_well::InkResponse::new(self.id, row)
            // Upstream's `customBorder: effectiveShape`: the splash is clipped
            // to the pill rather than to the box around it, so a tap near a
            // rounded end does not flash into the corner outside the bar.
            .with_custom_border(shape)
            .with_enabled(self.enabled)
            .with_hover_color(hovered)
            .with_highlight_color(pressed)
            .with_on_hover({
                let handle = handle.clone();
                move |hovering| {
                    handle.set_state(move |state| {
                        state.states.update(WidgetState::Hovered, hovering);
                    });
                }
            })
            .with_on_highlight_changed({
                let handle = handle.clone();
                move |pressed| {
                    handle.set_state(move |state| {
                        state.states.update(WidgetState::Pressed, pressed);
                    });
                }
            })
            .with_on_tap(move || {
                if let Some(tap) = &on_tap {
                    tap();
                }
            });

        let constraints = resolved.constraints;
        let enabled = self.enabled;
        crate::framework::many(vec![crate::framework::stateful(ink)], move |rendered| {
            let ink = rendered.into_iter().next().expect("the ink");
            // Upstream's `IgnorePointer(ignoring: !widget.enabled)`, which sits
            // between the surface and the ink: the bar is still drawn, and
            // still takes up its space, but nothing under it can be reached.
            // The ink's own `enabled` stops the splash; this stops the field
            // below it from taking the tap that the ink declined.
            let ink: crate::render::BoxedRender = if enabled {
                ink
            } else {
                crate::render::RenderRef::new(crate::render::RenderIgnorePointer::boxed(ink))
            };
            let surface = SearchBar::surface(&resolved, ink);
            crate::render::RenderConstrainedBox::new(constraints).with_child(
                crate::render::RenderOpacity::new(
                    if enabled {
                        1.0
                    } else {
                        SearchBar::DISABLED_OPACITY
                    },
                    surface,
                ),
            )
        })
    }
}

/// Which page a [`SearchDelegate`] is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchBody {
    #[default]
    Suggestions,
    Results,
}

/// Upstream `SearchDelegate`, the older API.
///
/// Where `SearchAnchor` hands you a builder and lets you arrange the view, this
/// one is a small state machine with **two pages**: suggestions while typing,
/// results after submitting. A caller fills in four builders and the delegate
/// decides which is on screen.
///
/// The two-page shape is the whole of it, and the interesting part is that
/// moving between them is explicit -- upstream's own guidance for
/// `buildSuggestions` says that tapping a suggestion should set `query` and
/// then call `showResults`. **Choosing a suggestion is not a search; it is
/// filling in the box and then searching.**
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchDelegate {
    pub query: String,
    body: SearchBody,
    closed_with: Option<String>,
    /// Upstream's `searchFieldLabel`. `None` is not "no label" -- see
    /// [`SearchDelegate::search_field_label`].
    search_field_label: Option<String>,
}

impl SearchDelegate {
    pub fn new() -> SearchDelegate {
        SearchDelegate::default()
    }

    /// Upstream's `SearchDelegate.searchFieldLabel`.
    pub fn with_search_field_label(mut self, label: impl Into<String>) -> Self {
        self.search_field_label = Some(label.into());
        self
    }

    /// The word in the empty search box, from upstream's
    /// `delegate.searchFieldLabel ?? MaterialLocalizations.of(context)
    /// .searchFieldLabel`.
    ///
    /// # One string, two jobs
    ///
    /// Upstream uses it as the field's `hintText` and then assigns
    /// `routeName = searchFieldLabel`, so the same word is the placeholder a
    /// reader sees in the empty box and the name a screen reader announces on
    /// arriving at the page. That is why it is one string rather than a hint
    /// and a label: they are the same answer to "what is this page for", given
    /// to two different senses, and letting them drift apart would let a page
    /// hinted "Search recipes" announce itself as "Search".
    pub fn search_field_label(&self) -> String {
        self.search_field_label.clone().unwrap_or_else(|| {
            crate::material_app::DefaultMaterialLocalizations::SEARCH_FIELD_LABEL.to_string()
        })
    }

    /// Upstream's `routeName`, which it assigns from the label above rather
    /// than taking separately.
    pub fn route_name(&self) -> String {
        self.search_field_label()
    }

    pub fn body(&self) -> SearchBody {
        self.body
    }

    pub fn closed_with(&self) -> Option<&str> {
        self.closed_with.as_deref()
    }

    /// Upstream `showResults`.
    pub fn show_results(&mut self) {
        self.body = SearchBody::Results;
    }

    /// Upstream `showSuggestions`, which is what editing the query again does.
    pub fn show_suggestions(&mut self) {
        self.body = SearchBody::Suggestions;
    }

    /// The path upstream describes for a tapped suggestion: set the query,
    /// **then** show the results. Two steps, because the query is what the
    /// results are computed from and it has to be right first.
    pub fn choose_suggestion(&mut self, suggestion: &str) {
        self.query = suggestion.to_string();
        self.show_results();
    }

    /// Upstream `close`, which pops the search page with a result.
    pub fn close(&mut self, result: Option<&str>) {
        self.closed_with = result.map(str::to_string);
    }

    pub fn is_closed(&self) -> bool {
        self.closed_with.is_some()
    }
}

/// Opens the search view as a real route: upstream's `_SearchViewRoute`,
/// pushed onto the theatre the way this crate pushes every other route-shaped
/// thing.
///
/// # The barrier is there and invisible, and both halves matter
///
/// Upstream's route sets `barrierColor = Colors.transparent` with
/// `barrierDismissible = true` and `barrierLabel = 'Dismiss'`. So the sheet
/// between the view and the page **paints nothing** -- a search view is a
/// panel that has grown out of a bar, and dimming the page behind it would
/// make it read as a dialog -- while still catching the tap that closes it and
/// still announcing itself to a screen reader as something to dismiss.
///
/// # What this does not do yet: the close is instant
///
/// Upstream reverses the same animation to close, and re-runs `updateTweens`
/// first because the bar may have moved while the view was up. Here the modal
/// comes down the moment it is dismissed, which is what every other
/// `show_*` in this crate does today -- see
/// [`crate::dialogs::show_cupertino_modal_popup`], whose sheet also only
/// animates in. Recorded rather than hidden: the closing half needs a place to
/// hang a reversal on `ModalHandle`, and that does not exist.
pub fn show_search_view(
    overlay: std::rc::Rc<crate::theatre::OverlayHandle>,
    placement: SearchViewTransition,
    constraints: crate::render::BoxConstraints,
    content: impl Fn(f32) -> crate::framework::AnyWidget + 'static,
) -> Option<crate::theatre::ModalHandle> {
    let content: std::rc::Rc<dyn Fn(f32) -> crate::framework::AnyWidget> =
        std::rc::Rc::new(content);
    crate::theatre::show_modal(overlay, search_view_barrier(), move || {
        crate::framework::stateful(SearchViewOpening {
            placement,
            constraints,
            content: std::rc::Rc::clone(&content),
        })
    })
}

/// The view's header: upstream's `_ViewContent.build`, whose header **is a
/// `SearchBar`** -- the same widget the anchor puts on the page, not a
/// look-alike built out of a `TextField`.
///
/// That is the whole point of the transition this crate spent two rounds on.
/// The bar grows into the panel and the field at the top of the panel is the
/// field that was tapped, so a reader's caret never moves from one widget to
/// another. Building a second, similar field here would work and would be
/// wrong in the way that only shows up when the two drift.
///
/// # What the header overrides, and why every one of them is a constant
///
/// Transparent background, transparent overlay, no elevation: the *panel* is
/// the surface, and a bar drawing its own pill on top of it would be a raised
/// shape inside a raised shape. The text and hint styles come from the view's
/// own `headerTextStyle`/`headerHintStyle` rather than the bar's, and the
/// padding is the view's `barPadding` -- which, as
/// [`crate::component_themes::ResolvedSearchView`] records, is the same
/// `EdgeInsets.symmetric(horizontal: 8)` the bar uses, because the header *is*
/// the bar continued.
pub fn search_view_header(
    view: &crate::component_themes::ResolvedSearchView,
    full_screen: bool,
    id: u64,
) -> SearchBar {
    SearchBar::new(id).with_overrides(SearchBarOverrides {
        background_color: Some(crate::engine::Color::TRANSPARENT),
        overlay: Some(crate::engine::Color::TRANSPARENT),
        elevation: Some(0.0),
        text_style: view.header_text_style.clone(),
        hint_style: view.header_hint_style.clone(),
        constraints: search_view_header_constraints(view.header_height, full_screen),
        padding: Some(view.bar_padding),
    })
}

/// How tall the header is allowed to be: upstream's `headerConstraints ??
/// (showFullScreenView ? BoxConstraints(minHeight: fullScreenBarHeight) :
/// null)`.
///
/// Three answers, and the difference between the last two is the interesting
/// part:
///
/// * a **stated** header height is *tight* -- `BoxConstraints.tightFor(height:)`
///   -- so a caller who asks for 64 gets 64 and not "at least 64";
/// * with none stated, a **full-screen** view has a floor of 72 and no
///   ceiling, because a header under a status bar has to clear it and may need
///   more room than a docked one;
/// * a **docked** view says nothing at all, and the bar falls back to its own
///   constraints -- 56 tall at the least, which is what a search bar is
///   everywhere else.
pub fn search_view_header_constraints(
    header_height: Option<f32>,
    full_screen: bool,
) -> Option<crate::render::BoxConstraints> {
    match (header_height, full_screen) {
        (Some(height), _) => Some(crate::render::BoxConstraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        }),
        (None, true) => Some(crate::render::BoxConstraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: crate::component_themes::ResolvedSearchView::FULL_SCREEN_BAR_HEIGHT,
            max_height: f32::INFINITY,
        }),
        (None, false) => None,
    }
}

/// What is in the panel below the header, and whether anything is:
/// upstream's `_ViewContent.build` from the header down.
///
/// # The divider and the list are present or absent together
///
/// Upstream guards both with one condition, and it is an **or of four**:
///
/// ```dart
/// if (!effectiveShrinkWrap || minHeight > 0 || showFullScreenView || result.isNotEmpty)
/// ```
///
/// So they are there *unless every one of the four is against them*: the view
/// shrink-wraps, it has no minimum height to fill, it is not full screen, and
/// there is nothing to show. That is not "show the list when there are
/// results" -- three of the four have nothing to do with results.
///
/// The reason is what a divider means. A rule under the header says *"there is
/// more below"*, and in three of these cases there is: a view that has a
/// minimum height, or fills the screen, has room below the header whether or
/// not anything has been typed yet, and drawing the header with nothing under
/// it would leave a rule pointing at blank space. Only the fourth case -- a
/// shrink-wrapping docked view with no floor and no results -- is a panel that
/// really is just a field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchViewBody {
    pub full_screen: bool,
    /// Upstream's `effectiveShrinkWrap`.
    pub shrink_wrap: bool,
    /// Upstream's `minHeight`, which is **already clamped**:
    /// `math.min(effectiveConstraints.minHeight, _viewRect.height)`. The
    /// clamped one is what the condition reads, so it is what this holds.
    pub min_height: f32,
    pub has_results: bool,
}

impl SearchViewBody {
    /// Whether the divider and the list are in the column at all.
    pub fn shows_the_list(&self) -> bool {
        !self.shrink_wrap || self.min_height > 0.0 || self.full_screen || self.has_results
    }

    /// Whether the list must fill the room left over, upstream's
    /// `fit: (effectiveShrinkWrap && !showFullScreenView) ? loose : tight`.
    ///
    /// **A shrink-wrapping view is the only one that lets it be shorter**, and
    /// even then only while docked: a full-screen view has a screen to fill,
    /// and a panel with a fixed height that left a gap under its list would
    /// show the surface through it.
    pub fn list_fills_the_room(&self) -> bool {
        !(self.shrink_wrap && !self.full_screen)
    }
}

/// The column upstream builds inside the panel: the header, then -- when
/// [`SearchViewBody::shows_the_list`] says so -- a divider and the results.
///
/// # The fade named for the icons is over the whole column
///
/// Upstream wraps this entire column in `FadeTransition(opacity:
/// viewIconsFadeCurve)`, and the divider and the list then carry a second
/// fade each on top of it. So `_kViewIconsFadeOnInterval` is not the icons'
/// fade at all -- it is *everything's*, and the name survives from whatever it
/// used to wrap. Ported where it is applied rather than where it is named,
/// because the name is the part that is wrong.
///
/// The result is that the divider and the list are multiplied by two curves:
/// they arrive on their own schedule *within* a column that is itself still
/// fading in.
pub fn search_view_column(
    body: SearchViewBody,
    t: f32,
    direction: crate::animation::AnimationStatus,
    top_padding: f32,
    header: crate::render::BoxedRender,
    divider: crate::render::BoxedRender,
    list: crate::render::BoxedRender,
) -> crate::render::RenderOpacity {
    let fade = |interval| SearchViewTransition::fade_at(interval, t, direction);

    let mut column = crate::render::RenderFlex::column()
        .with_main_axis_size(crate::render::MainAxisSize::Min)
        // `crossAxisAlignment: stretch`: the divider is a full-width rule and
        // the header is a full-width bar, so neither is centred.
        .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch);

    // The top inset goes on the header alone rather than on the column,
    // because it is the status bar's room and only the header is under it.
    column = column.push(crate::render::RenderPadding::new(
        crate::render::EdgeInsets::only(0.0, top_padding, 0.0, 0.0),
        header,
    ));

    if body.shows_the_list() {
        column = column.push(crate::render::RenderOpacity::new(
            fade(SearchViewTransition::divider_fade()),
            divider,
        ));
        let faded_list =
            crate::render::RenderOpacity::new(fade(SearchViewTransition::list_fade()), list);
        column = column.push_flex(if body.list_fills_the_room() {
            crate::render::FlexChild::expanded(faded_list, 1)
        } else {
            crate::render::FlexChild::flexible(faded_list, 1)
        });
    }

    crate::render::RenderOpacity::new(fade(SearchViewTransition::icons_fade()), column)
}

/// The panel itself: the `Padding` / `Material` / `OverflowBox` that
/// `_ViewContent.build` puts between the animated rectangle and the column.
///
/// # The content is laid out at the width the view will *end* at
///
/// That is what the `OverflowBox` is for, and it is the least obvious part of
/// the whole view. Its `maxWidth` is `math.min(viewMaxWidth, screenSize.width)`
/// where `viewMaxWidth` is `_rectTween.end!.width` -- **the finished width, not
/// the animated one** -- with `minWidth: 0` and `fit: deferToChild`, aligned to
/// the top left.
///
/// Without it the column would be measured against the growing rectangle, and
/// every line of text inside it would re-wrap on every frame of the opening:
/// the panel would not appear to grow, it would appear to reflow. With it the
/// content is laid out once at its final width and merely *revealed* as the
/// rectangle catches up -- which is why the box has to let its child overflow,
/// and why the clip below it is what stops the overflow being seen.
///
/// The `min` against the screen is the guard for the case
/// [`SearchAnchor::view_rect`] leaves alone: a view wider than the window
/// keeps its width there, and this stops the *content* from being laid out
/// wider than the screen as well.
///
/// # Padding, and the one case that has none
///
/// A full-screen view gets `EdgeInsets.zero` however the theme is set. It is
/// the screen; an inset would leave a strip of the page showing around
/// something that is supposed to have replaced it.
///
/// # The clip is the shape, not a rectangle
///
/// Upstream's `Material(clipBehavior: Clip.antiAlias, shape: effectiveShape)`
/// clips to the shape's own outline. The docked default is a
/// `RoundedRectangleBorder` of 28 and the full-screen one is the same shape
/// with a radius of zero, so the corners come from the theme rather than from
/// a constant here -- and they are resolved at the laid-out size, because a
/// shape's rounding is not a number until then.
pub fn search_view_panel(
    view: &crate::component_themes::ResolvedSearchView,
    full_screen: bool,
    view_max_width: f32,
    screen_width: f32,
    column: crate::render::BoxedRender,
) -> crate::render::BoxedRender {
    let content = crate::render::RenderOverflowBox::boxed(column)
        .with_alignment(crate::render::Alignment::TOP_LEFT)
        .with_fit(crate::render::OverflowBoxFit::DeferToChild)
        .with_min_width(0.0)
        .with_max_width(view_max_width.min(screen_width));

    let surface = crate::render::RenderDecoratedBox::new()
        .with_fill(crate::render::Fill::Solid(
            crate::elevation_overlay::ElevationOverlay::apply_surface_tint(
                view.background_color,
                Some(view.surface_tint_color),
                view.elevation,
            ),
        ))
        .with_shadows(crate::painting::elevation_shadows(view.elevation.max(0.0) as u32).to_vec())
        .with_shape(view.shape.clone())
        // The clip goes *inside* the surface rather than around it: a clip
        // outside would cut the panel's own shadow off at its edge, and a
        // shadow that stops at the thing casting it is not a shadow.
        .with_child(
            crate::render::RenderClipRRect::new(crate::borders::BorderRadius::ZERO, content)
                .with_shape(view.shape.clone()),
        );

    let padding = if full_screen {
        crate::render::EdgeInsets::ZERO
    } else {
        view.padding.unwrap_or(crate::render::EdgeInsets::ZERO)
    };
    crate::render::RenderRef::new(crate::render::RenderPadding::new(padding, surface))
}

/// Everything inside the growing rectangle: upstream's `_ViewContent`, joined
/// up.
///
/// The four pieces the last four rounds built each answered one question and
/// none of them called another. This is the thing that calls them, and it
/// exists as a struct rather than a nine-argument function because six of
/// those arguments do not change between frames and one does.
///
/// # The body's decision is remade every frame
///
/// [`SearchViewBody::min_height`] is upstream's `minHeight`, which is
/// `math.min(effectiveConstraints.minHeight, _viewRect.height)` -- and
/// `_viewRect` is the **animated** rectangle. So the answer to "is there a
/// divider" is not a property of the view; it is a property of the frame.
///
/// It matters at exactly one moment, and it is the first one: a view whose
/// rectangle is still the bar's has a clamped minimum of the bar's height, and
/// a shrink-wrapping docked view with nothing to show would therefore open as
/// a bare field and grow a divider as the rectangle passes zero. Computing it
/// once, at the top, would get that frame wrong in the other direction.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchViewContent {
    pub placement: SearchViewTransition,
    pub view: crate::component_themes::ResolvedSearchView,
    /// The view's resolved constraints, which the body clamps against the
    /// animated rectangle. Not the rectangle.
    pub constraints: crate::render::BoxConstraints,
    /// The window, for the cap on how wide the content may be laid out.
    pub screen: crate::render::Size,
    /// Upstream's `result.isNotEmpty`.
    pub has_results: bool,
    /// `MediaQuery.paddingOf(context).top`, which only a full-screen view
    /// keeps clear -- see [`SearchViewTransition::top_padding_at`].
    pub media_top: f32,
    /// Identifies the header's field, so that the text somebody is typing
    /// survives the rebuild every frame of the opening causes.
    pub header_id: u64,
    /// Upstream's `viewHintText`.
    pub hint_text: Option<String>,
    /// The query, shared with the bar this view grew out of.
    ///
    /// Upstream hands the same `SearchController` to both, so the view's
    /// header opens holding whatever was typed into the bar -- and typing into
    /// the header writes back to the same place.
    pub controller: SharedSearchController,
}

impl SearchViewContent {
    /// The bar at the top of the panel, as this view styles it.
    ///
    /// # What is not here yet
    ///
    /// Upstream also gives the header a `defaultLeading` -- a `BackButton`
    /// that pops the route -- and a `defaultTrailing` of one close button,
    /// present only while there is text to clear. Neither is built here: a
    /// back button needs the route to pop, and this crate's view is dismissed
    /// through its barrier, so wiring one to nothing would be worse than
    /// leaving the space empty. Recorded rather than quietly omitted.
    pub fn header(&self) -> SearchBar {
        let mut header = search_view_header(&self.view, self.placement.full_screen, self.header_id);
        header.hint_text = self.hint_text.clone();
        // Seeded from the shared controller: the view opens holding what the
        // bar held. Without this the panel grows out of a filled-in bar and
        // arrives empty, which reads as the search having been thrown away.
        header.initial_text = Some(self.controller.borrow().text.clone());
        let controller = std::rc::Rc::clone(&self.controller);
        header.with_on_changed(move |text| {
            controller.borrow_mut().text = text.to_string();
        })
    }

    /// What is in the panel on the frame at `t`. See the type's documentation
    /// for why this is a function of `t` at all.
    pub fn body_at(&self, t: f32, direction: crate::animation::AnimationStatus) -> SearchViewBody {
        let rect = self.placement.rect_at(t, direction);
        SearchViewBody {
            full_screen: self.placement.full_screen,
            shrink_wrap: self.view.shrink_wrap,
            min_height: self.constraints.min_height.min(rect.height()),
            has_results: self.has_results,
        }
    }

    /// The panel and everything in it, on the frame at `t`.
    ///
    /// `header`, `divider` and `list` are widgets rather than render objects
    /// because they are rebuilt with the view -- the header is a live
    /// `SearchBar` with a field in it, and the list is whatever the caller's
    /// suggestions builder returned this frame.
    pub fn build(
        &self,
        t: f32,
        direction: crate::animation::AnimationStatus,
        header: crate::framework::AnyWidget,
        divider: crate::framework::AnyWidget,
        list: crate::framework::AnyWidget,
    ) -> crate::framework::AnyWidget {
        let body = self.body_at(t, direction);
        let view = self.view.clone();
        let full_screen = self.placement.full_screen;
        // The panel's content is laid out at the width the view *ends* at, not
        // the one it is passing through -- see [`search_view_panel`].
        let view_width = self.placement.view.width();
        let screen_width = self.screen.width;
        let top_padding = self.placement.top_padding_at(t, direction, self.media_top);

        crate::framework::many(vec![header, divider, list], move |rendered| {
            let mut rendered = rendered.into_iter();
            let column = search_view_column(
                body,
                t,
                direction,
                top_padding,
                rendered.next().expect("the header"),
                rendered.next().expect("the divider"),
                rendered.next().expect("the list"),
            );
            crate::render::RenderRef::new(search_view_panel(
                &view,
                full_screen,
                view_width,
                screen_width,
                crate::render::RenderRef::new(column),
            ))
        })
    }
}

/// Opens a search view with the panel this crate builds, rather than with
/// whatever the caller hands over: [`show_search_view`] with its `content`
/// filled in.
pub fn open_search_view(
    overlay: std::rc::Rc<crate::theatre::OverlayHandle>,
    content: SearchViewContent,
    list: impl Fn() -> crate::framework::AnyWidget + 'static,
) -> Option<crate::theatre::ModalHandle> {
    let placement = content.placement;
    let constraints = content.constraints;
    show_search_view(overlay, placement, constraints, move |t| {
        // Only the suggestions are the caller's. The header and the rule are
        // the view's own -- upstream builds both inside `_ViewContent`, and a
        // caller who supplied them could give the panel a header that was not
        // the bar it grew out of.
        content.build(
            t,
            crate::animation::AnimationStatus::Forward,
            crate::framework::stateful(content.header()),
            search_view_divider(&content.view),
            list(),
        )
    })
}

/// The rule under the header: upstream's
/// `DividerTheme(data: dividerTheme.copyWith(color: effectiveDividerColor),
/// child: const Divider(height: 1))`.
///
/// A `Divider` of height **one**, not of the theme's usual sixteen. Upstream's
/// default divider reserves space around its line so that a rule between two
/// list items has air on both sides; the one under a search header is a seam
/// between two parts of the same surface, and air around it would read as a
/// gap in the panel.
///
/// The colour is the view's own `dividerColor` laid over the divider theme
/// rather than taken from it, which is why it is set on the widget here: a
/// search view's rule follows the *view's* theme, and an app that has restyled
/// its dividers elsewhere should not restyle this one by accident.
pub fn search_view_divider(
    view: &crate::component_themes::ResolvedSearchView,
) -> crate::framework::AnyWidget {
    crate::framework::component(
        crate::components::Divider::new()
            .with_color(view.divider_color)
            .with_height(1.0),
    )
}

/// The sheet between the view and the page: upstream's `barrierColor`,
/// `barrierDismissible` and `barrierLabel`, together.
///
/// It is its own function because those three are the whole of what makes a
/// search view's barrier different from a dialog's, and they are easier to
/// check where they are stated than where they are used.
pub fn search_view_barrier() -> crate::modal_barrier::ModalBarrier {
    // No colour: `Colors.transparent`. The label is upstream's `barrierLabel`,
    // and `dismissible` is `ModalBarrier`'s own default of true.
    crate::modal_barrier::ModalBarrier::new().with_semantics_label("Dismiss")
}

/// The growing half of [`show_search_view`]: upstream's `buildPage` builder
/// together with `_ViewContentState.build`'s placement.
struct SearchViewOpening {
    placement: SearchViewTransition,
    /// The view's resolved constraints, which are **not** the rectangle it is
    /// drawn in -- see `build` for why both are needed.
    constraints: crate::render::BoxConstraints,
    /// Rebuilt every frame with the fraction of the opening that has passed,
    /// because what is *inside* the panel changes with it too -- the divider
    /// and the results have fades of their own, and the decision about
    /// whether they are there at all reads the animated rectangle. A content
    /// built once could only be revealed, not opened.
    content: std::rc::Rc<dyn Fn(f32) -> crate::framework::AnyWidget>,
}

/// How far into the opening the view is. The transition is a pure function of
/// the elapsed time, so all that is kept is the clock.
#[derive(Default)]
pub struct SearchViewOpeningState {
    elapsed_micros: i64,
    last_frame_micros: Option<i64>,
}

impl crate::framework::StatefulComponent for SearchViewOpening {
    type State = SearchViewOpeningState;

    fn advance(&self, state: &mut SearchViewOpeningState, frame_time_micros: i64) -> bool {
        // Clamped for the reason every on-demand animation in this crate
        // clamps: the previous frame may be a page-load away, and an unclamped
        // step would put the whole opening into one frame.
        const MAX_FRAME_MICROS: i64 = 50_000;
        if let Some(previous) = state.last_frame_micros {
            state.elapsed_micros += (frame_time_micros - previous).clamp(0, MAX_FRAME_MICROS);
        }
        state.last_frame_micros = Some(frame_time_micros);
        state.elapsed_micros < SearchViewTransition::OPEN_MICROS
    }

    fn build(
        &self,
        state: &SearchViewOpeningState,
        _handle: crate::framework::StateHandle<SearchViewOpeningState>,
        _context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        use crate::animation::AnimationStatus;

        let t = (state.elapsed_micros as f32 / SearchViewTransition::OPEN_MICROS as f32)
            .clamp(0.0, 1.0);
        let placement = self.placement;
        let constraints = self.constraints;
        crate::framework::many(vec![(self.content)(t)], move |rendered| {
            let child = rendered.into_iter().next().expect("the view's content");
            let rect = placement.rect_at(t, AnimationStatus::Forward);
            let opacity = SearchViewTransition::fade_at(
                SearchViewTransition::view_fade(),
                t,
                AnimationStatus::Forward,
            );
            // Upstream's `_ViewContentState.build`: the *maxima* are the
            // animated rectangle and the *minima* are the resolved constraints
            // **clamped down to it** -- `math.min(effectiveConstraints
            // .minWidth, _viewRect.width)`.
            //
            // That `min` is the whole reason both are passed in. For most of
            // the opening the rectangle is smaller than the view's own minimum
            // (360 x 240), and a minimum left unclamped would be larger than
            // the maximum beside it: the view would jump straight to full size
            // on the first frame and the growth would never be seen.
            let sized = crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                min_width: constraints.min_width.min(rect.width()),
                max_width: rect.width(),
                min_height: constraints.min_height.min(rect.height()),
                max_height: rect.height(),
            })
            .with_child(child);
            crate::render::RenderOpacity::new(
                opacity,
                crate::render::RenderAlign::new(
                    crate::render::Alignment::TOP_LEFT,
                    // `Transform.translate(offset: _viewRect.topLeft)`, which
                    // moves the panel without laying it out anywhere else.
                    crate::render::RenderTransform::new(
                        [1.0, 0.0, 0.0, 1.0, rect.left, rect.top],
                        sized,
                    ),
                ),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attached() -> SearchController {
        let mut controller = SearchController::new();
        controller.attach(1);
        controller
    }

    // -- The controller ---------------------------------------------------------

    #[test]
    fn choosing_a_suggestion_sets_the_text_before_it_pops() {
        // Two objects would give you a frame of the old query while the view
        // animates away.
        let mut controller = attached();
        controller.text = "wid".to_string();
        controller.open_view().unwrap();

        controller.close_view(Some("widgets")).unwrap();
        assert_eq!(controller.text, "widgets");
        assert!(!controller.is_open());
    }

    #[test]
    fn closing_without_a_selection_leaves_the_query_alone() {
        let mut controller = attached();
        controller.text = "wid".to_string();
        controller.open_view().unwrap();
        controller.close_view(None).unwrap();
        assert_eq!(controller.text, "wid");
    }

    #[test]
    fn a_controller_with_no_anchor_has_no_view_and_says_so() {
        let mut orphan = SearchController::new();
        assert!(!orphan.is_attached());
        assert!(orphan.open_view().is_err());
        assert!(orphan.close_view(None).is_err());
    }

    #[test]
    fn a_controller_handed_to_a_new_anchor_is_not_detached_by_the_old_one() {
        let mut controller = attached();
        controller.attach(2);
        controller.detach(1);
        assert!(
            controller.is_attached(),
            "the first anchor's disposal does not take the second one's controller"
        );

        controller.detach(2);
        assert!(!controller.is_attached());
    }

    // -- The anchor ---------------------------------------------------------------

    #[test]
    fn the_view_is_full_screen_on_a_phone_and_docked_on_a_desktop() {
        let anchor = SearchAnchor::new(1);
        assert!(anchor.resolve_full_screen(ScrollPlatform::Android));
        assert!(anchor.resolve_full_screen(ScrollPlatform::IOS));
        assert!(!anchor.resolve_full_screen(ScrollPlatform::MacOS));
        assert!(!anchor.resolve_full_screen(ScrollPlatform::Windows));
    }

    #[test]
    fn saying_so_outright_overrules_the_platform() {
        assert!(
            SearchAnchor::new(1)
                .with_full_screen(true)
                .resolve_full_screen(ScrollPlatform::Windows)
        );
        assert!(
            !SearchAnchor::new(1)
                .with_full_screen(false)
                .resolve_full_screen(ScrollPlatform::Android)
        );
    }

    #[test]
    fn the_view_attached_to_something_goes_when_that_something_moves() {
        // A docked view is positioned against an anchor whose place on screen
        // just changed and has nowhere to be. A full-screen one was never
        // anchored, so rotating only lays it out again.
        let anchor = SearchAnchor::new(1);
        assert_eq!(
            anchor.on_window_resized(ScrollPlatform::Windows),
            ViewDismissal::WindowResizedWhileDocked
        );
        assert_eq!(
            anchor.on_window_resized(ScrollPlatform::Android),
            ViewDismissal::Kept
        );
    }

    #[test]
    fn opening_the_view_is_going_somewhere() {
        // Which is why the system back gesture closes it without the anchor
        // arranging anything.
        assert!(SearchAnchor::view_is_a_route());
    }

    #[test]
    fn an_untappable_builder_gets_its_tap_handling_from_the_anchor() {
        // Upstream: if builder returns an Icon, we do not have to explicitly
        // call openView.
        assert!(SearchBar::anchor_supplies_tap_handling(false));
        assert!(!SearchBar::anchor_supplies_tap_handling(true));
    }

    // -- The older delegate ---------------------------------------------------------

    #[test]
    fn choosing_a_suggestion_is_filling_in_the_box_and_then_searching() {
        // Upstream's own guidance: set query, then call showResults. Two steps,
        // because the results are computed from the query and it has to be
        // right first.
        let mut delegate = SearchDelegate::new();
        assert_eq!(delegate.body(), SearchBody::Suggestions);

        delegate.query = "wid".to_string();
        assert_eq!(
            delegate.body(),
            SearchBody::Suggestions,
            "typing alone does not search"
        );

        delegate.choose_suggestion("widgets");
        assert_eq!(delegate.query, "widgets");
        assert_eq!(delegate.body(), SearchBody::Results);
    }

    #[test]
    fn editing_the_query_again_goes_back_to_the_suggestions() {
        let mut delegate = SearchDelegate::new();
        delegate.choose_suggestion("widgets");
        delegate.show_suggestions();
        assert_eq!(delegate.body(), SearchBody::Suggestions);
        assert_eq!(delegate.query, "widgets", "and the query is still there");
    }

    #[test]
    fn closing_carries_a_result_back_or_nothing() {
        let mut chosen = SearchDelegate::new();
        chosen.close(Some("widgets"));
        assert!(chosen.is_closed());
        assert_eq!(chosen.closed_with(), Some("widgets"));

        let mut abandoned = SearchDelegate::new();
        abandoned.close(None);
        assert!(!abandoned.is_closed(), "nothing chosen is not a result");
    }
}

#[cfg(test)]
mod search_theme_tests {
    use super::*;
    use crate::component_themes::{
        ResolvedMenuButton, ResolvedSearchBar, ResolvedSearchView, SearchBarTheme,
        SearchBarThemeData, SearchViewTheme, SearchViewThemeData,
    };
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::EdgeInsets;
    use crate::theme::ThemeData;
    use crate::widget_state::{StateProperty, WidgetState, WidgetStates};
    use crate::widgets::SizedBox;

    struct Reader<T> {
        read: std::rc::Rc<dyn Fn(&mut BuildContext) -> T>,
        seen: std::rc::Rc<std::cell::RefCell<Option<T>>>,
    }

    impl<T: 'static> Component for Reader<T> {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some((self.read)(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn read_under<T: 'static>(
        wrap: impl FnOnce(AnyWidget) -> AnyWidget,
        read: impl Fn(&mut BuildContext) -> T + 'static,
    ) -> T {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(wrap(component(Reader {
            read: std::rc::Rc::new(read),
            seen: std::rc::Rc::clone(&seen),
        })));
        seen.borrow_mut().take().expect("built once")
    }

    fn bar(states: WidgetStates) -> ResolvedSearchBar {
        read_under(
            |child| child,
            move |context| ResolvedSearchBar::of(context, states),
        )
    }

    fn view(is_full_screen: bool) -> ResolvedSearchView {
        read_under(
            |child| child,
            move |context| ResolvedSearchView::of(context, is_full_screen),
        )
    }

    fn states(list: &[WidgetState]) -> WidgetStates {
        WidgetStates::of(list)
    }

    // -- A bar is a control, a view is a surface -------------------------------

    #[test]
    fn a_search_bar_does_not_react_to_being_focused_and_a_menu_line_does() {
        // Upstream writes `focused => Colors.transparent` on the bar,
        // identical to the fall-through, where `_MenuButtonDefaultsM3` gives
        // focused the same weight as pressed. A focused bar has a caret in it
        // saying so; a focused menu line has only the highlight.
        let resting = bar(WidgetStates::NONE);
        let focused = bar(states(&[WidgetState::Focused]));
        assert_eq!(focused.overlay, resting.overlay);
        assert_eq!(focused.overlay, Color::TRANSPARENT);

        let scheme = ThemeData::fallback().color_scheme;
        assert_ne!(
            ResolvedMenuButton::overlay_for(states(&[WidgetState::Focused]), &scheme),
            ResolvedMenuButton::overlay_for(WidgetStates::NONE, &scheme),
            "the menu line's focused arm is not its fall-through"
        );
    }

    #[test]
    fn but_it_does_react_to_being_pressed_and_hovered() {
        // Or the test above would only show that nothing reaches the overlay.
        let scheme = ThemeData::fallback().color_scheme;
        let pressed = bar(states(&[WidgetState::Pressed]));
        let hovered = bar(states(&[WidgetState::Hovered]));
        assert_eq!(
            pressed.overlay,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.1)
        );
        assert_eq!(
            hovered.overlay,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.08)
        );
        assert_ne!(pressed.overlay, hovered.overlay);
    }

    #[test]
    fn pressing_beats_hovering_on_the_bar_too() {
        // The order of the two arms that differ. A pointer that presses is
        // always also hovering, so the ladder's order is what decides.
        let both = bar(states(&[WidgetState::Pressed, WidgetState::Hovered]));
        assert_eq!(both.overlay, bar(states(&[WidgetState::Pressed])).overlay);
        assert_ne!(both.overlay, bar(states(&[WidgetState::Hovered])).overlay);
    }

    #[test]
    fn a_bars_theme_answers_by_state_and_a_views_cannot() {
        // The distinction upstream draws in the *types*: every
        // `SearchBarThemeData` field is a state property and every
        // `SearchViewThemeData` field is a plain nullable.
        let resting = Color(0xFF010101);
        let pressed = Color(0xFF020202);
        let mut data = SearchBarThemeData::new();
        data.background_color = Some(StateProperty::resolve_with(move |states| {
            Some(if states.contains(WidgetState::Pressed) {
                pressed
            } else {
                resting
            })
        }));
        let wrap = move |child: AnyWidget| SearchBarTheme::new(data.clone(), child);
        assert_eq!(
            read_under(wrap.clone(), |context| ResolvedSearchBar::of(
                context,
                WidgetStates::NONE
            ))
            .background_color,
            resting
        );
        assert_eq!(
            read_under(wrap, |context| ResolvedSearchBar::of(
                context,
                WidgetStates::of(&[WidgetState::Pressed])
            ))
            .background_color,
            pressed
        );

        // The view has no states to be given, so the two calls cannot differ.
        assert_eq!(view(false).background_color, view(false).background_color);
    }

    // -- The view is the bar, grown --------------------------------------------

    #[test]
    fn the_views_bar_padding_is_the_bars_padding() {
        // A header that padded its field differently from the bar that opened
        // it would read as a second, unrelated field appearing in its place.
        assert_eq!(view(false).bar_padding, bar(WidgetStates::NONE).padding);
        assert_eq!(view(false).bar_padding, EdgeInsets::symmetric(8.0, 0.0));
    }

    #[test]
    fn and_shares_its_surface_its_elevation_and_its_two_text_colours() {
        let plain_bar = bar(WidgetStates::NONE);
        let docked = view(false);
        assert_eq!(docked.background_color, plain_bar.background_color);
        assert_eq!(docked.elevation, plain_bar.elevation);
        assert_eq!(docked.surface_tint_color, plain_bar.surface_tint_color);
        assert_eq!(
            docked.header_text_style.map(|style| style.color),
            plain_bar.text_style.map(|style| style.color)
        );
        assert_eq!(
            docked.header_hint_style.map(|style| style.color),
            plain_bar.hint_style.map(|style| style.color)
        );
    }

    #[test]
    fn the_bar_is_capped_at_a_readable_width_and_the_view_is_not() {
        let plain_bar = bar(WidgetStates::NONE);
        assert_eq!(plain_bar.constraints.max_width, 800.0);
        assert_eq!(view(false).constraints.max_width, f32::INFINITY);

        // The same statement in the other direction: a line against a place.
        assert_eq!(plain_bar.constraints.min_height, 56.0);
        assert_eq!(view(false).constraints.min_height, 240.0);
        assert_eq!(
            plain_bar.constraints.min_width,
            view(false).constraints.min_width,
            "they start at the same width and only the ceiling differs"
        );
    }

    #[test]
    fn going_full_screen_takes_the_corners_off() {
        // Rounding a full-screen view would draw a card floating on a
        // background that is not there.
        let docked = view(false);
        let full = view(true);
        assert_ne!(docked.shape, full.shape);
        match (&docked.shape, &full.shape) {
            (
                crate::borders::ShapeBorder::Rounded(docked),
                crate::borders::ShapeBorder::Rounded(full),
            ) => {
                let direction = crate::direction::current_direction();
                assert_eq!(docked.resolved_radius(direction).top_left.x, 28.0);
                assert_eq!(full.resolved_radius(direction).top_left.x, 0.0);
            }
            other => panic!("expected two rounded rectangles, got {other:?}"),
        }
    }

    #[test]
    fn full_screen_changes_nothing_else_about_the_view() {
        // It is the shape and only the shape -- everything else a full-screen
        // view is, a docked one is too.
        let docked = view(false);
        let full = view(true);
        assert_eq!(docked.background_color, full.background_color);
        assert_eq!(docked.elevation, full.elevation);
        assert_eq!(docked.divider_color, full.divider_color);
        assert_eq!(docked.bar_padding, full.bar_padding);
        assert_eq!(docked.constraints.min_height, full.constraints.min_height);
        assert_eq!(docked.shrink_wrap, full.shrink_wrap);
    }

    #[test]
    fn a_theme_that_names_a_shape_keeps_it_full_screen_or_not() {
        // The full-screen branch is the *default*, so a caller who chose a
        // shape has already answered the question it was asking.
        let mine = crate::borders::ShapeBorder::Stadium(Default::default());
        let mut data = SearchViewThemeData::new();
        data.shape = Some(mine.clone());
        for full in [false, true] {
            let resolved = read_under(
                {
                    let data = data.clone();
                    move |child| SearchViewTheme::new(data, child)
                },
                move |context| ResolvedSearchView::of(context, full),
            );
            assert_eq!(resolved.shape, mine);
        }
    }

    #[test]
    fn the_view_leaves_its_header_height_and_its_padding_unanswered() {
        // Upstream has no default for either: the header is as tall as what is
        // in it, and the padding is the caller's business.
        let docked = view(false);
        assert_eq!(docked.header_height, None);
        assert_eq!(docked.padding, None);
        assert_eq!(docked.side, None);
        assert!(!docked.shrink_wrap);
        assert_eq!(
            docked.divider_color,
            ThemeData::fallback().color_scheme.outline()
        );
    }

    #[test]
    fn a_bar_is_told_from_its_background_by_colour_and_not_by_an_outline() {
        let plain = bar(WidgetStates::NONE);
        assert_eq!(plain.side, None, "no default side");
        assert_ne!(
            plain.background_color,
            ThemeData::fallback().color_scheme.surface,
            "which only works because the surface it sits on is a different one"
        );
        assert!(matches!(
            plain.shape,
            crate::borders::ShapeBorder::Stadium(_)
        ));
    }
}

#[cfg(test)]
mod search_field_label_tests {
    use super::SearchDelegate;

    #[test]
    fn an_empty_search_box_says_search() {
        assert_eq!(SearchDelegate::new().search_field_label(), "Search");
    }

    #[test]
    fn and_a_delegate_that_said_what_it_searches_says_that() {
        assert_eq!(
            SearchDelegate::new()
                .with_search_field_label("Search recipes")
                .search_field_label(),
            "Search recipes"
        );
    }

    #[test]
    fn the_hint_and_the_route_name_are_the_same_word() {
        // Upstream assigns `routeName = searchFieldLabel`, so the placeholder
        // a reader sees and the announcement a reader hears are one answer to
        // one question. Letting them drift would let a page hinted "Search
        // recipes" announce itself as "Search".
        let named = SearchDelegate::new().with_search_field_label("Search recipes");
        assert_eq!(named.route_name(), named.search_field_label());

        let plain = SearchDelegate::new();
        assert_eq!(plain.route_name(), plain.search_field_label());
        assert_eq!(plain.route_name(), "Search");
    }

    #[test]
    fn the_label_survives_the_two_page_state_machine() {
        // The delegate moves between suggestions and results; the word in the
        // box is not one of the things that changes.
        let mut delegate = SearchDelegate::new().with_search_field_label("Search recipes");
        delegate.show_results();
        assert_eq!(delegate.search_field_label(), "Search recipes");
        delegate.choose_suggestion("carbonara");
        assert_eq!(delegate.search_field_label(), "Search recipes");
        assert_eq!(
            delegate.query, "carbonara",
            "the query moved, the label did not"
        );
    }
}

#[cfg(test)]
mod search_bar_widget_tests {
    use super::*;
    use crate::borders::{ShapeBorder, StadiumBorder};
    use crate::component_themes::ResolvedSearchBar;
    use crate::engine::Color;
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{AnyWidget, ElementTree, leaf, stateful};
    use crate::render::{BoxConstraints, EdgeInsets, Offset, RenderBox, Size};
    use crate::widgets::SizedBox;

    /// A bar's appearance built by hand, so that a test says what it is
    /// looking at instead of depending on whatever the theme happens to be.
    fn resolved() -> ResolvedSearchBar {
        ResolvedSearchBar {
            background_color: Color::argb(255, 0, 0, 255),
            elevation: ResolvedSearchBar::ELEVATION,
            shadow_color: Color::argb(255, 255, 0, 0),
            surface_tint_color: Color::TRANSPARENT,
            overlay: Color::TRANSPARENT,
            side: None,
            shape: ShapeBorder::Stadium(StadiumBorder::default()),
            padding: EdgeInsets::symmetric(ResolvedSearchBar::PADDING, 0.0),
            text_style: None,
            hint_style: None,
            constraints: BoxConstraints {
                min_width: ResolvedSearchBar::MIN_WIDTH,
                max_width: ResolvedSearchBar::MAX_WIDTH,
                min_height: ResolvedSearchBar::MIN_HEIGHT,
                max_height: f32::INFINITY,
            },
            text_capitalization: crate::component_themes::TextCapitalization::None,
        }
    }

    /// What a bar painted, laid out the way a frame lays it out.
    fn painted(widget: AnyWidget, width: f32, height: f32) -> Vec<Drawn> {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(width, height));
        crate::render::flush_layout();

        let mut layers = crate::engine::LayerTree::new(width as i32, height as i32);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(width, height));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn laid_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(width, height));
        crate::render::flush_layout();
        root.size()
    }

    #[test]
    fn the_surface_is_rounded_by_the_height_it_ended_up_at() {
        // The whole reason the shape reaches the render object instead of
        // being turned into a radius at build time. A `StadiumBorder` is half
        // the shorter side, the bar's `maxHeight` is unbounded, and so the
        // radius is not knowable until the bar has been measured.
        let resolved = resolved();
        let at = |height: f32| {
            let mut surface = SearchBar::surface(&resolved, SizedBox::new(200.0, height));
            RenderBox::layout(&mut surface, BoxConstraints::tight(200.0, height));
            surface
                .rounding(crate::engine::Rect::ltrb(0.0, 0.0, 200.0, height))
                .map(|radius| radius.top_left.x)
        };
        assert_eq!(at(56.0), Some(28.0), "a bar at its minimum height");
        assert_eq!(
            at(120.0),
            Some(60.0),
            "the same bar, taller, is rounded by more"
        );
    }

    #[test]
    fn the_shadow_is_the_themes_colour_at_each_layers_own_alpha() {
        // Upstream's `Material(shadowColor:)`. The alphas differ between the
        // three layers on purpose -- the umbra is denser than the ambient, and
        // flattening them to one would turn a shadow into a grey halo -- so
        // the colour is taken and the alpha is left alone.
        let surface = SearchBar::surface(&resolved(), SizedBox::new(200.0, 56.0));
        let expected = crate::painting::elevation_shadows(ResolvedSearchBar::ELEVATION as u32);
        let shadows = surface.shadows();
        assert_eq!(shadows.len(), expected.len(), "one per layer");
        assert!(
            shadows.iter().all(|shadow| shadow.color.red() == 255
                && shadow.color.green() == 0
                && shadow.color.blue() == 0),
            "every layer takes the theme's hue: {shadows:?}"
        );
        assert_eq!(
            shadows
                .iter()
                .map(|shadow| shadow.color.alpha())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|shadow| shadow.color.alpha())
                .collect::<Vec<_>>(),
            "and keeps its own alpha"
        );
    }

    #[test]
    fn a_bar_is_no_narrower_and_no_wider_than_it_is_allowed() {
        // `ResolvedSearchBar.constraints`: 360 to 800 wide, 56 tall at the
        // least. A bar offered more than it may take is capped, and one
        // offered less than its minimum still asks for the minimum.
        let roomy = laid_out(stateful(SearchBar::new(1)), 1000.0, 400.0);
        assert_eq!(roomy.width, ResolvedSearchBar::MAX_WIDTH, "{roomy:?}");
        assert!(roomy.height >= ResolvedSearchBar::MIN_HEIGHT, "{roomy:?}");

        // And a bar in a space narrower than its own minimum takes the space
        // rather than the minimum. Upstream's `ConstrainedBox` is
        // `_additionalConstraints.enforce(constraints)` in that order, so the
        // incoming constraints always win -- a widget that overflowed its
        // parent because its own defaults said 360 would be worse than one
        // that is merely small.
        let cramped = laid_out(stateful(SearchBar::new(1)), 200.0, 400.0);
        assert_eq!(cramped.width, 200.0, "{cramped:?}");
    }

    #[test]
    fn a_disabled_bar_is_dimmed_rather_than_recoloured() {
        // Upstream fades the finished bar rather than resolving a disabled
        // colour per part, which is why there is no `WidgetState::Disabled`
        // anywhere in it. An opacity layer is the observable difference.
        let alphas = |bar: SearchBar| {
            painted(stateful(bar), 400.0, 200.0)
                .into_iter()
                .filter_map(|call| match call {
                    Drawn::OpacityLayer { alpha } => Some(alpha),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            alphas(SearchBar::new(1)),
            Vec::<u8>::new(),
            "an enabled bar is not faded at all"
        );
        assert_eq!(
            alphas(SearchBar::new(1).with_enabled(false)),
            vec![97],
            "0.38 of 255"
        );
    }

    #[test]
    fn a_disabled_bar_cannot_be_reached_through() {
        // The fade is only what it looks like; `IgnorePointer` is what stops
        // the field underneath from taking the tap. Without it a bar could be
        // drawn at 38% and still be typed into.
        let hits = |bar: SearchBar| {
            let mut tree = ElementTree::new();
            tree.rebuild(stateful(bar));
            let root = tree.build_render_tree().expect("a root");
            crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 200.0));
            crate::render::flush_layout();
            let mut result = crate::render::HitTestResult::new();
            root.hit_test(Offset::new(100.0, 20.0), &mut result);
            result.path.len()
        };
        let enabled = hits(SearchBar::new(1));
        assert!(enabled > 0, "an enabled bar is hittable");
        assert_eq!(hits(SearchBar::new(1).with_enabled(false)), 0);
    }

    #[test]
    fn the_hint_is_drawn_in_the_hint_colour_and_not_the_texts() {
        // The two are different by default -- `onSurfaceVariant` for the hint
        // against `onSurface` for the text -- and the field would otherwise
        // have muted its own style, which lands somewhere near but not on it.
        let theme = crate::theme::ThemeData::default();
        let hint_colour = theme.color_scheme.on_surface_variant();
        let text_colour = theme.color_scheme.on_surface;
        assert_ne!(hint_colour, text_colour, "there would be nothing to see");

        let drawn = painted(
            stateful(SearchBar::new(1).with_hint_text("Search")),
            400.0,
            200.0,
        );
        let hint = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, argb, .. } if text == "Search" => Some(*argb),
                _ => None,
            })
            .expect("the hint reached the canvas");
        assert_eq!(Color(hint), hint_colour);
    }

    #[test]
    fn the_leading_widget_comes_before_the_field_and_the_trailing_after() {
        // The row's order, and the reason the bar carries the two separately
        // rather than as one list of children.
        let marker = |colour: Color| {
            move || -> AnyWidget {
                leaf(move || {
                    crate::render::RenderDecoratedBox::new()
                        .with_fill(crate::render::Fill::Solid(colour))
                        .with_child(SizedBox::new(20.0, 20.0))
                })
            }
        };
        let lead = Color::argb(255, 0, 255, 0);
        let trail = Color::argb(255, 255, 0, 255);
        let drawn = painted(
            stateful(
                SearchBar::new(1)
                    .with_hint_text("Search")
                    .with_leading(marker(lead))
                    .with_trailing(marker(trail)),
            ),
            400.0,
            200.0,
        );
        let x_of = |wanted: Color| {
            drawn
                .iter()
                .find_map(|call| match call {
                    Drawn::Rect { left, argb, .. } if Color(*argb) == wanted => Some(*left),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted:?} was not painted"))
        };
        let hint_x = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, x, .. } if text == "Search" => Some(*x),
                _ => None,
            })
            .expect("the hint");
        assert!(x_of(lead) < hint_x, "the leading widget is before the text");
        assert!(x_of(trail) > hint_x, "and the trailing one after it");
    }

    #[test]
    fn the_padding_is_applied_once_around_the_row_and_once_around_the_field() {
        // Upstream applies `effectivePadding` twice, and the inner one is what
        // keeps the first letter off the leading icon. Dropping it puts the
        // text against the icon; the leading widget's own edge does not move.
        let marker = || -> AnyWidget {
            leaf(|| {
                crate::render::RenderDecoratedBox::new()
                    .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 255, 0)))
                    .with_child(SizedBox::new(20.0, 20.0))
            })
        };
        let drawn = painted(
            stateful(
                SearchBar::new(1)
                    .with_hint_text("Search")
                    .with_leading(marker),
            ),
            400.0,
            200.0,
        );
        let icon_right = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Rect { right, argb, .. } if Color(*argb) == Color::argb(255, 0, 255, 0) => {
                    Some(*right)
                }
                _ => None,
            })
            .expect("the leading widget");
        let hint_x = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, x, .. } if text == "Search" => Some(*x),
                _ => None,
            })
            .expect("the hint");
        assert!(
            hint_x - icon_right >= ResolvedSearchBar::PADDING,
            "the text is a padding away from the icon, not against it: \
             icon ends at {icon_right}, text starts at {hint_x}"
        );
    }
}

#[cfg(test)]
mod search_view_rect_tests {
    use super::*;
    use crate::component_themes::ResolvedSearchView;
    use crate::direction::TextDirection;
    use crate::engine::Rect;
    use crate::render::{BoxConstraints, Size};

    /// The defaults `ResolvedSearchView::of` hands out with no theme: at least
    /// 360 wide and 240 tall, with no maximum of either.
    fn defaults() -> BoxConstraints {
        BoxConstraints {
            min_width: ResolvedSearchView::MIN_WIDTH,
            max_width: f32::INFINITY,
            min_height: ResolvedSearchView::MIN_HEIGHT,
            max_height: f32::INFINITY,
        }
    }

    fn docked(anchor: Rect, screen: Size) -> Rect {
        SearchAnchor::view_rect(anchor, screen, defaults(), false, TextDirection::Ltr)
    }

    #[test]
    fn the_view_is_as_wide_as_the_bar_and_two_thirds_of_the_screen_tall() {
        // The two measurements come from different places on purpose: the
        // width continues the field that was tapped, and the height is a
        // fraction of the window because a 56-tall bar cannot say how much
        // room a list of results wants.
        let rect = docked(
            Rect::xywh(20.0, 40.0, 500.0, 56.0),
            Size::new(1000.0, 900.0),
        );
        assert_eq!(rect.width(), 500.0, "the bar's width");
        assert_eq!(rect.height(), 600.0, "two thirds of 900, not of the bar");
        assert_eq!(
            (rect.left, rect.top),
            (20.0, 40.0),
            "it opens where the bar is"
        );
    }

    #[test]
    fn a_narrow_bar_still_opens_a_view_wide_enough_to_read() {
        // `clamp` against the resolved constraints, which is what stops a
        // 100-wide bar from opening a 100-wide list of results.
        let rect = docked(Rect::xywh(0.0, 0.0, 100.0, 56.0), Size::new(1000.0, 900.0));
        assert_eq!(rect.width(), ResolvedSearchView::MIN_WIDTH);
    }

    #[test]
    fn a_short_screen_does_not_give_a_view_shorter_than_its_minimum() {
        // Two thirds of 300 is 200, below the 240 minimum. The clamp wins, and
        // the view is then taller than two thirds of the window.
        let rect = docked(Rect::xywh(0.0, 0.0, 400.0, 56.0), Size::new(1000.0, 300.0));
        assert_eq!(rect.height(), ResolvedSearchView::MIN_HEIGHT);
    }

    #[test]
    fn a_bar_near_the_right_edge_pulls_the_view_back_onto_the_screen() {
        // 800 - 700 = 100 of room for a 400-wide view, so the corner moves to
        // 800 - 400 rather than the view hanging off the side.
        let rect = docked(
            Rect::xywh(700.0, 10.0, 400.0, 56.0),
            Size::new(800.0, 900.0),
        );
        assert_eq!(rect.left, 400.0);
        assert_eq!(rect.right, 800.0, "it ends exactly at the edge");
    }

    #[test]
    fn a_bar_near_the_bottom_pulls_the_view_up() {
        // The same rule downwards, and it is a separate `if` upstream: a bar
        // in the bottom-right corner is moved on both axes.
        let rect = docked(
            Rect::xywh(10.0, 800.0, 400.0, 56.0),
            Size::new(1000.0, 900.0),
        );
        assert_eq!(rect.top, 300.0, "900 - 600");
        assert_eq!(rect.left, 10.0, "and across, nothing moved");
    }

    #[test]
    fn a_view_wider_than_the_window_starts_at_the_edge_and_keeps_its_width() {
        // Upstream's comment says it resizes the view to fit the window. Its
        // code does not: the `min` lands on the corner and `endSize` is left
        // alone. Ported as written -- see `view_rect`'s documentation.
        let rect = docked(Rect::xywh(50.0, 0.0, 900.0, 56.0), Size::new(600.0, 900.0));
        assert_eq!(rect.left, 0.0, "pulled back to the window's edge");
        assert_eq!(rect.width(), 900.0, "and still wider than the window");
    }

    #[test]
    fn a_right_to_left_view_hangs_from_the_bars_right_edge() {
        // Not a mirror of the whole rectangle: the anchoring edge changes, so
        // a view wider than its bar grows leftwards from the bar's right side.
        let anchor = Rect::xywh(500.0, 20.0, 200.0, 56.0);
        let rect = SearchAnchor::view_rect(
            anchor,
            Size::new(1000.0, 900.0),
            defaults(),
            false,
            TextDirection::Rtl,
        );
        assert_eq!(
            rect.width(),
            ResolvedSearchView::MIN_WIDTH,
            "200 clamped to 360"
        );
        assert_eq!(rect.right, 700.0, "its right edge is the bar's right edge");
        assert_eq!(rect.left, 340.0, "700 - 360");
    }

    #[test]
    fn a_right_to_left_bar_near_the_right_edge_is_not_pulled_back() {
        // The left-to-right correction asks how much room there is to the
        // *right* of the bar, and in this direction that is the wrong
        // question: the view hangs leftwards from the bar's right edge, so a
        // bar near the right edge already fits. Applying the correction anyway
        // would shove the view left by 100 for no reason.
        let rect = SearchAnchor::view_rect(
            Rect::xywh(800.0, 20.0, 100.0, 56.0),
            Size::new(1000.0, 900.0),
            defaults(),
            false,
            TextDirection::Rtl,
        );
        assert_eq!(rect.right, 900.0, "still hanging from the bar's right edge");
        assert_eq!(rect.left, 540.0, "900 - 360, not 1000 - 360");
    }

    #[test]
    fn a_right_to_left_view_with_no_room_to_its_left_starts_at_zero() {
        // The mirror of the left-to-right edge rule, and the reason it is
        // written as `0.0` rather than `screen.width - width`: in this
        // direction the edge that runs out is the near one.
        let rect = SearchAnchor::view_rect(
            Rect::xywh(0.0, 20.0, 200.0, 56.0),
            Size::new(1000.0, 900.0),
            defaults(),
            false,
            TextDirection::Rtl,
        );
        assert_eq!(rect.left, 0.0);
    }

    #[test]
    fn a_full_screen_view_is_the_screen_and_asks_the_anchor_nothing() {
        // Every rule above is skipped. A bar in the corner of a small window
        // would otherwise produce something quite different.
        let rect = SearchAnchor::view_rect(
            Rect::xywh(700.0, 800.0, 100.0, 56.0),
            Size::new(400.0, 850.0),
            defaults(),
            true,
            TextDirection::Ltr,
        );
        assert_eq!(rect, Rect::xywh(0.0, 0.0, 400.0, 850.0));
    }
}

#[cfg(test)]
mod search_view_transition_tests {
    use super::*;
    use crate::animation::AnimationStatus;
    use crate::engine::Rect;

    fn opening() -> SearchViewTransition {
        SearchViewTransition {
            anchor: Rect::xywh(20.0, 40.0, 400.0, 56.0),
            view: Rect::xywh(20.0, 40.0, 400.0, 600.0),
            full_screen: false,
        }
    }

    #[test]
    fn the_view_starts_as_the_bar_and_ends_where_it_was_put() {
        // The tween's two ends. The view grows out of the bar that was tapped
        // rather than appearing over it.
        let opening = opening();
        assert_eq!(
            opening.rect_at(0.0, AnimationStatus::Forward),
            opening.anchor
        );
        assert_eq!(opening.rect_at(1.0, AnimationStatus::Forward), opening.view);
    }

    #[test]
    fn the_rectangle_is_nearly_arrived_by_the_half_way_point() {
        // The emphasized curve's own shape, and the reason it is the one on
        // the rect: the panel is essentially in place while the second half of
        // the 600ms is still running, so the things fading in inside it are
        // fading into a panel that has stopped moving.
        let opening = opening();
        let half = opening.rect_at(0.5, AnimationStatus::Forward);
        let arrived = opening.view.height() - opening.anchor.height();
        let travelled = half.height() - opening.anchor.height();
        assert!(
            travelled / arrived > 0.9,
            "{}% of the way at the half",
            travelled / arrived * 100.0
        );
    }

    #[test]
    fn the_four_fades_take_their_turns() {
        // The staggering upstream spells out in four constants. At a sixth of
        // the way in the divider is done, the icons are only starting, and the
        // list has not begun -- 133/600 is 0.22, past a sixth.
        let sixth = 1.0 / 6.0;
        let at = |fade| SearchViewTransition::fade_at(fade, sixth, AnimationStatus::Forward);
        assert_eq!(
            at(SearchViewTransition::divider_fade()),
            1.0,
            "the divider is in"
        );
        assert_eq!(
            at(SearchViewTransition::icons_fade()),
            0.0,
            "the icons start here"
        );
        assert_eq!(
            at(SearchViewTransition::list_fade()),
            0.0,
            "the list has not begun"
        );
        assert!(
            at(SearchViewTransition::view_fade()) > 0.3
                && at(SearchViewTransition::view_fade()) < 0.4,
            "and the view itself is a third faded in"
        );
    }

    #[test]
    fn the_list_fades_on_milliseconds_and_not_on_sixths() {
        // The only one of the four written in milliseconds upstream, and the
        // one that would change meaning if the 600 ever did. A sixth to two
        // sixths is 100ms to 200ms, and the list actually runs 133 to 233 --
        // so at 200ms in, sixths would have it fully arrived and it is in fact
        // two thirds of the way.
        let at = |ms: f32| {
            SearchViewTransition::fade_at(
                SearchViewTransition::list_fade(),
                ms / SearchViewTransition::OPEN_MILLISECONDS as f32,
                AnimationStatus::Forward,
            )
        };
        assert_eq!(at(133.0), 0.0, "it starts at 133ms, not at 100");
        assert_eq!(at(233.0), 1.0, "and is done at 233ms, not at 200");
        let two_hundred = at(200.0);
        assert!(
            (two_hundred - 0.67).abs() < 0.01,
            "at 200ms it is two thirds in, not finished: {two_hundred}"
        );
    }

    #[test]
    fn the_view_is_fully_faded_in_by_the_half_and_the_list_well_before_it() {
        let half = |fade| SearchViewTransition::fade_at(fade, 0.5, AnimationStatus::Forward);
        assert_eq!(half(SearchViewTransition::view_fade()), 1.0);
        assert_eq!(
            half(SearchViewTransition::list_fade()),
            1.0,
            "233ms of 600 is well inside the half"
        );
    }

    #[test]
    fn the_fades_are_read_off_the_plain_animation_and_not_the_eased_one() {
        // The mistake this guards against: feeding the intervals the
        // emphasized value. That curve is 95% done by the half, so every fade
        // would finish inside the first fifth and the staggering would
        // collapse into one instant.
        let sixth = 1.0 / 6.0;
        let eased = SearchViewTransition::eased(sixth, AnimationStatus::Forward);
        assert!(
            eased > 0.35,
            "the emphasized curve is already well along: {eased}"
        );
        assert_eq!(
            SearchViewTransition::fade_at(
                SearchViewTransition::list_fade(),
                sixth,
                AnimationStatus::Forward
            ),
            0.0,
            "the list still has not started, so the interval saw {sixth} and not {eased}"
        );
    }

    #[test]
    fn closing_runs_the_curves_flipped_rather_than_backwards() {
        // Upstream's `reverseCurve: ....flipped`. A curve that leaves slowly
        // should arrive slowly played backwards; replaying the forward curve
        // would make the close snap away and then drift in.
        let opening = opening();
        let forward = opening.rect_at(0.5, AnimationStatus::Forward).height();
        let closing = opening.rect_at(0.5, AnimationStatus::Reverse).height();
        assert!(
            closing < forward,
            "half way through a close the view is still large: {closing} vs {forward}"
        );

        let fade = SearchViewTransition::view_fade();
        assert_eq!(
            SearchViewTransition::fade_at(fade, 1.0, AnimationStatus::Reverse),
            1.0,
            "a close begins fully visible"
        );
        assert_eq!(
            SearchViewTransition::fade_at(fade, 0.5, AnimationStatus::Reverse),
            0.0,
            "and is gone by the half, which is the mirror of the first half in"
        );
    }

    #[test]
    fn only_a_full_screen_view_grows_a_top_inset() {
        // A docked panel never reaches the status bar, so it has nothing to
        // hold its content out from.
        let docked = opening();
        assert_eq!(
            docked.top_padding_at(1.0, AnimationStatus::Forward, 44.0),
            0.0
        );

        let full = SearchViewTransition {
            full_screen: true,
            ..docked
        };
        assert_eq!(
            full.top_padding_at(1.0, AnimationStatus::Forward, 44.0),
            44.0
        );
        assert_eq!(
            full.top_padding_at(0.0, AnimationStatus::Forward, 44.0),
            0.0,
            "a view that is still a small rectangle is not under the status bar"
        );
    }

    #[test]
    fn the_bar_fades_out_in_a_quarter_of_the_time_the_view_takes_to_open() {
        // Two durations, and they are deliberately different: the bar is gone
        // long before the view has finished arriving.
        assert_eq!(SearchViewTransition::OPEN_MICROS, 600_000);
        assert_eq!(SearchViewTransition::ANCHOR_FADE_MICROS, 150_000);
        assert!(SearchViewTransition::ANCHOR_FADE_MICROS < SearchViewTransition::OPEN_MICROS / 2);
    }
}

#[cfg(test)]
mod search_view_route_tests {
    use super::*;
    use crate::animation::AnimationStatus;
    use crate::component_themes::ResolvedSearchView;
    use crate::engine::{Color, Rect};
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};
    use crate::theatre::{ModalHandle, OverlayHandle, overlay};
    use std::cell::RefCell;
    use std::rc::Rc;

    const SCREEN: Size = Size {
        width: 800.0,
        height: 900.0,
    };

    fn constraints() -> BoxConstraints {
        BoxConstraints {
            min_width: ResolvedSearchView::MIN_WIDTH,
            max_width: f32::INFINITY,
            min_height: ResolvedSearchView::MIN_HEIGHT,
            max_height: f32::INFINITY,
        }
    }

    fn placement() -> SearchViewTransition {
        let anchor = Rect::xywh(20.0, 40.0, 400.0, 56.0);
        SearchViewTransition {
            anchor,
            view: SearchAnchor::view_rect(
                anchor,
                SCREEN,
                constraints(),
                false,
                crate::direction::TextDirection::Ltr,
            ),
            full_screen: false,
        }
    }

    /// A tree with an overlay in it, and the handle a descendant found.
    fn staged() -> (ElementTree, Rc<OverlayHandle>) {
        let found: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));
        struct Finder(Rc<RefCell<Option<Rc<OverlayHandle>>>>);
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = OverlayHandle::of(context);
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(overlay(component(Finder(Rc::clone(&found)))));
        tree.build_render_tree();
        let handle = found.borrow().clone().expect("a descendant found it");
        (tree, handle)
    }

    /// A panel that takes the **smallest** size it is offered rather than the
    /// largest, so that a test can see the minimum the view was constrained
    /// with. The bare panel below cannot: a decorated box with no child takes
    /// `constraints.biggest()`, and a minimum it never reads is a minimum no
    /// assertion about its rectangle can catch.
    fn minimum_sized_panel() -> AnyWidget {
        leaf(|| {
            crate::render::RenderDecoratedBox::new()
                .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 0, 255)))
                .with_child(crate::render::RenderConstrainedBox::new(BoxConstraints {
                    min_width: 0.0,
                    max_width: f32::INFINITY,
                    min_height: 0.0,
                    max_height: f32::INFINITY,
                }))
        })
    }

    /// The panel the view's content paints, so a test can watch it grow.
    fn panel() -> AnyWidget {
        leaf(|| {
            crate::render::RenderDecoratedBox::new()
                .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 0, 255)))
        })
    }

    /// Opens the view, runs the clock to `micros`, and answers the rectangle
    /// the panel reached the canvas as, plus any opacity layer over it.
    fn opened_at(micros: i64) -> (Option<(f32, f32, f32, f32)>, Vec<u8>, Option<(f32, f32)>) {
        opened_with(placement(), panel, micros)
    }

    fn opened_with(
        placement: SearchViewTransition,
        content: fn() -> AnyWidget,
        micros: i64,
    ) -> (Option<(f32, f32, f32, f32)>, Vec<u8>, Option<(f32, f32)>) {
        let (mut tree, handle) = staged();
        let shown = show_search_view(Rc::clone(&handle), placement, constraints(), move |_| {
            content()
        })
        .expect("shown");
        // Stepped rather than jumped. `advance` clamps each frame to 50ms --
        // every on-demand animation in this crate does, so that a page-load
        // between frames cannot put a whole transition into one of them -- so
        // a test that set the clock to 600ms in one call would see 50.
        // Built before the clock starts: an element that is not mounted yet
        // takes no `advance`, and its first one would then be the frame that
        // establishes its zero -- leaving the whole opening one frame behind.
        tree.rebuild_dirty();
        tree.advance_frame(0);
        tree.rebuild_dirty();
        let mut now = 0;
        while now < micros {
            now = (now + 16_000).min(micros);
            tree.advance_frame(now);
            tree.rebuild_dirty();
        }

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(
            &root,
            BoxConstraints::tight(SCREEN.width, SCREEN.height),
        );
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(800, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(&mut layers, SCREEN);
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let drawn = crate::engine_test_stubs::drawn();
        shown.dismiss();
        let panel = drawn.iter().find_map(|call| match call {
            Drawn::Rect {
                left,
                top,
                right,
                bottom,
                argb,
                ..
            } if Color(*argb) == Color::argb(255, 0, 0, 255) => {
                Some((*left, *top, *right, *bottom))
            }
            _ => None,
        });
        let alphas = drawn
            .iter()
            .filter_map(|call| match call {
                Drawn::OpacityLayer { alpha } => Some(*alpha),
                _ => None,
            })
            .collect();
        // The panel's position is not in its own rectangle: a `Transform`
        // paints through a layer, so the rectangle reaching the canvas is the
        // one before the translation and the offset is on the layer.
        let moved = drawn.iter().find_map(|call| match call {
            Drawn::TransformLayer { e, f, .. } => Some((*e, *f)),
            _ => None,
        });
        (panel, alphas, moved)
    }

    #[test]
    fn the_view_starts_the_size_of_the_bar_and_ends_the_size_of_the_view() {
        // The route is the thing this round added: the geometry and the curve
        // were already there, and nothing put them on screen.
        // Nothing at all on the frame the route is pushed: the view's fade
        // starts at zero, and a fully transparent subtree paints nothing.
        assert_eq!(opened_at(0).0, None, "the first frame is empty");

        // One frame in, it is the bar: at the bar's corner, at the bar's size.
        let (start, _, moved) = opened_at(16_000);
        let start = start.expect("the panel painted");
        let moved = moved.expect("the panel was moved into place");
        assert!(
            (moved.0 - 20.0).abs() < 0.01 && (moved.1 - 40.0).abs() < 0.01,
            "it opens over the bar: {moved:?}"
        );
        assert!(
            start.2 - start.0 == 400.0 && start.3 - start.1 - 56.0 < 5.0,
            "and at the bar's size: {start:?}"
        );

        let (end, _, _) = opened_at(SearchViewTransition::OPEN_MICROS * 2);
        let end = end.expect("the panel painted");
        let view = placement().view;
        assert!(
            (end.2 - end.0 - view.width()).abs() < 1.0
                && (end.3 - end.1 - view.height()).abs() < 1.0,
            "it ends at the view's rectangle: {end:?} against {view:?}"
        );
    }

    #[test]
    fn the_minimum_is_clamped_down_to_the_animated_rectangle() {
        // The `math.min(effectiveConstraints.minWidth, _viewRect.width)` that
        // `_ViewContentState.build` applies. The view's own minimum is 360 x
        // 240 and the bar is 400 x 56 -- so on the first frame an unclamped
        // minimum height would be four times the maximum beside it, and the
        // panel would appear full-size instead of growing.
        let (start, _, _) = opened_with(placement(), minimum_sized_panel, 16_000);
        let start = start.expect("the panel painted");
        assert!(
            start.3 - start.1 < ResolvedSearchView::MIN_HEIGHT,
            "the first frame is the bar's height, not the view's minimum: {start:?}"
        );
        assert!(
            (start.2 - start.0 - ResolvedSearchView::MIN_WIDTH).abs() < 1.0,
            "across, the bar is wider than the minimum, so the minimum stands: {start:?}"
        );

        // And the same clamp across, which needs a bar **narrower** than the
        // view's minimum to show at all: with a 400-wide bar the width is over
        // the 360 minimum from the first frame, so clamping it changes
        // nothing. A 200-wide bar opens a 360-wide view, and every frame
        // before the last is narrower than the minimum it is being given.
        let narrow = Rect::xywh(20.0, 40.0, 200.0, 56.0);
        let placement = SearchViewTransition {
            anchor: narrow,
            view: SearchAnchor::view_rect(
                narrow,
                SCREEN,
                constraints(),
                false,
                crate::direction::TextDirection::Ltr,
            ),
            full_screen: false,
        };
        let (start, _, _) = opened_with(placement, minimum_sized_panel, 16_000);
        let start = start.expect("the panel painted");
        assert!(
            start.2 - start.0 < ResolvedSearchView::MIN_WIDTH,
            "it starts at the bar's width, not the view's minimum: {start:?}"
        );
    }

    #[test]
    fn a_view_that_was_pulled_back_onto_the_screen_opens_towards_where_it_goes() {
        // The panel is moved to the tween's rectangle each frame, not parked
        // at the bar. It only shows up when the two corners differ, which is
        // exactly the case `view_rect` moves: a bar near the bottom of the
        // screen opens a view that has been pulled upwards.
        let anchor = Rect::xywh(20.0, 800.0, 400.0, 56.0);
        let placement = SearchViewTransition {
            anchor,
            view: SearchAnchor::view_rect(
                anchor,
                SCREEN,
                constraints(),
                false,
                crate::direction::TextDirection::Ltr,
            ),
            full_screen: false,
        };
        assert_ne!(
            placement.view.top, anchor.top,
            "the case would not test anything otherwise"
        );

        let (_, _, moved) = opened_with(placement, panel, SearchViewTransition::OPEN_MICROS * 2);
        let moved = moved.expect("the panel was moved into place");
        assert!(
            (moved.1 - placement.view.top).abs() < 1.0,
            "it arrives where the view goes, not where the bar was: {moved:?}"
        );
    }

    #[test]
    fn a_frame_that_arrives_late_moves_the_opening_on_by_one_frame_and_no_more() {
        // Every on-demand animation in this crate clamps its step, and the
        // reason is what a long frame means: the app was busy, not that time
        // should be skipped. Without the clamp a single 600ms hitch would put
        // the whole opening into one frame and there would be no animation at
        // all -- the view would simply appear.
        let (mut tree, handle) = staged();
        let shown = show_search_view(Rc::clone(&handle), placement(), constraints(), |_| panel())
            .expect("shown");
        tree.rebuild_dirty();
        tree.advance_frame(0);
        tree.rebuild_dirty();
        // One frame, a whole opening long.
        tree.advance_frame(SearchViewTransition::OPEN_MICROS);
        assert!(
            tree.advance_frame(SearchViewTransition::OPEN_MICROS),
            "50ms of the 600 have passed, so it is still opening"
        );
        shown.dismiss();
    }

    #[test]
    fn the_barrier_is_transparent_dismissible_and_named() {
        // Upstream's three: `barrierColor: Colors.transparent`,
        // `barrierDismissible: true`, `barrierLabel: 'Dismiss'`. The first is
        // what keeps a search view from reading as a dialog, and the second is
        // the only way out that does not need a keyboard.
        let barrier = search_view_barrier();
        assert!(!barrier.paints(), "a dimmed page would read as a dialog");
        assert!(barrier.dismissible);
        assert_eq!(barrier.semantics_label.as_deref(), Some("Dismiss"));
    }

    #[test]
    fn the_view_fades_in_over_the_first_half_and_is_not_faded_after_it() {
        // The route's `FadeTransition`, driven by `_kViewFadeOnInterval`. A
        // fully opaque child pushes no layer at all, which is how the two
        // states are told apart.
        let quarter = SearchViewTransition::OPEN_MICROS / 4;
        let (_, part_way, _) = opened_at(quarter);
        assert_eq!(
            part_way,
            vec![
                (SearchViewTransition::fade_at(
                    SearchViewTransition::view_fade(),
                    0.25,
                    AnimationStatus::Forward
                ) * 255.0)
                    .round() as u8
            ],
            "half faded at a quarter of the way, since the fade takes half the time"
        );

        let (_, arrived, _) = opened_at(SearchViewTransition::OPEN_MICROS);
        assert_eq!(arrived, Vec::<u8>::new(), "and not faded once it is in");
    }

    #[test]
    fn the_barrier_is_invisible_and_still_closes_the_view() {
        // Upstream's `barrierColor: Colors.transparent` with
        // `barrierDismissible: true`. Dimming the page would make a panel that
        // grew out of a bar read as a dialog; not catching the tap would leave
        // no way out but the keyboard.
        let (mut tree, handle) = staged();
        let shown: ModalHandle =
            show_search_view(Rc::clone(&handle), placement(), constraints(), |_| panel())
                .expect("shown");
        tree.rebuild_dirty();
        assert!(shown.is_showing());

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(
            &root,
            BoxConstraints::tight(SCREEN.width, SCREEN.height),
        );
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(800, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(&mut layers, SCREEN);
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let painted_full_screen = crate::engine_test_stubs::drawn().into_iter().any(|call| {
            matches!(
                call,
                Drawn::Rect { left, top, right, bottom, .. }
                    if left == 0.0 && top == 0.0 && right == SCREEN.width && bottom == SCREEN.height
            )
        });
        assert!(!painted_full_screen, "the barrier paints nothing");

        shown.dismiss();
        tree.rebuild_dirty();
        assert!(!shown.is_showing());
    }

    #[test]
    fn the_opening_asks_for_frames_until_it_is_over_and_then_stops() {
        // An animation that keeps asking is a device that never sleeps.
        let (mut tree, handle) = staged();
        let shown = show_search_view(Rc::clone(&handle), placement(), constraints(), |_| panel())
            .expect("shown");
        tree.rebuild_dirty();
        tree.advance_frame(0);
        assert!(
            tree.advance_frame(SearchViewTransition::OPEN_MICROS / 4),
            "still opening"
        );
        for step in 1..40 {
            tree.advance_frame(step * SearchViewTransition::OPEN_MICROS / 20);
        }
        assert!(
            !tree.advance_frame(SearchViewTransition::OPEN_MICROS * 4),
            "and done"
        );
        shown.dismiss();
    }
}

#[cfg(test)]
mod search_view_header_tests {
    use super::*;
    use crate::component_themes::ResolvedSearchView;
    use crate::engine::{Color, TextStyle};
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{
        AnyWidget, BuildContext, Component, ElementTree, component, leaf, stateful,
    };
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};
    use crate::widget_state::WidgetStates;

    /// The view's appearance as `ResolvedSearchView::of` gives it with no
    /// theme, read through a real build so the defaults are the real ones.
    fn view(full_screen: bool) -> ResolvedSearchView {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        struct Reader(
            std::rc::Rc<std::cell::RefCell<Option<ResolvedSearchView>>>,
            bool,
        );
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some(ResolvedSearchView::of(context, self.1));
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader(std::rc::Rc::clone(&seen), full_screen)));
        let read = seen.borrow_mut().take().expect("built once");
        read
    }

    /// What a bar resolves to once its overrides are on, read through a build.
    fn resolved_of(bar: SearchBar) -> crate::component_themes::ResolvedSearchBar {
        resolved_in(bar, WidgetStates::NONE)
    }

    fn resolved_in(
        bar: SearchBar,
        states: WidgetStates,
    ) -> crate::component_themes::ResolvedSearchBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        struct Reader(
            std::rc::Rc<std::cell::RefCell<Option<crate::component_themes::ResolvedSearchBar>>>,
            SearchBar,
            WidgetStates,
        );
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some(self.1.resolved(context, self.2));
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader(std::rc::Rc::clone(&seen), bar, states)));
        let read = seen.borrow_mut().take().expect("built once");
        read
    }

    #[test]
    fn the_header_draws_no_surface_of_its_own() {
        // The panel is the surface. A bar that kept its own background, its own
        // elevation and its own hover overlay would be a raised pill sitting
        // inside a raised panel, and every one of the three would show.
        let plain = resolved_of(SearchBar::new(1));
        assert_ne!(
            plain.background_color,
            Color::TRANSPARENT,
            "a bar on the page has a surface, or there is nothing to override"
        );
        assert!(plain.elevation > 0.0);

        let header = resolved_of(search_view_header(&view(false), false, 1));
        assert_eq!(header.background_color, Color::TRANSPARENT);
        assert_eq!(header.elevation, 0.0);

        // The overlay has to be asked for **hovered**. Idle it is transparent
        // anyway -- that is the bar's own fall-through -- so a check at rest
        // would pass whether the override were there or not.
        let hovered =
            crate::widget_state::WidgetStates::of(&[crate::widget_state::WidgetState::Hovered]);
        assert_ne!(
            resolved_in(SearchBar::new(1), hovered).overlay,
            Color::TRANSPARENT,
            "a bar on the page lights up under the pointer"
        );
        assert_eq!(
            resolved_in(search_view_header(&view(false), false, 1), hovered).overlay,
            Color::TRANSPARENT,
            "and a header does not"
        );
    }

    #[test]
    fn the_header_takes_the_views_text_styles_and_not_the_bars() {
        // `headerTextStyle` and `headerHintStyle`, which a view theme can set
        // separately from the bar's -- so the same bar reads one way on the
        // page and another inside the panel.
        let view = view(false);
        let header = resolved_of(search_view_header(&view, false, 1));
        assert_eq!(header.text_style, view.header_text_style);
        assert_eq!(header.hint_style, view.header_hint_style);
        assert!(header.text_style.is_some(), "or the check is vacuous");
    }

    #[test]
    fn the_header_is_padded_the_way_the_view_pads_its_bar() {
        // Read off a doctored view rather than the default one, because at the
        // default the two are **the same numbers**: the view's `barPadding`
        // and the bar's `padding` are both `symmetric(horizontal: 8)`, which
        // `ResolvedSearchView` records as deliberate -- the header is the bar
        // continued. A test against the default could not tell "the view's
        // padding was used" from "the bar's own was left alone".
        let mut view = view(false);
        assert_eq!(
            view.bar_padding,
            crate::render::EdgeInsets::symmetric(
                crate::component_themes::ResolvedSearchBar::PADDING,
                0.0
            ),
            "the two agree by default, which is why this test doctors one"
        );
        view.bar_padding = crate::render::EdgeInsets::all(3.0);
        let header = resolved_of(search_view_header(&view, false, 1));
        assert_eq!(header.padding, crate::render::EdgeInsets::all(3.0));
    }

    #[test]
    fn each_of_the_headers_overrides_comes_from_the_view_it_was_built_from() {
        // The same doctoring for the rest of them at once: at the defaults
        // several of these agree with the bar's own values, and a wiring that
        // dropped one would go unseen.
        let mut view = view(false);
        view.header_text_style = Some(TextStyle {
            color: Color::argb(255, 1, 2, 3),
            ..TextStyle::default()
        });
        view.header_hint_style = Some(TextStyle {
            color: Color::argb(255, 4, 5, 6),
            ..TextStyle::default()
        });
        let header = resolved_of(search_view_header(&view, false, 1));
        assert_eq!(header.text_style, view.header_text_style);
        assert_eq!(header.hint_style, view.header_hint_style);
        assert_ne!(header.text_style, header.hint_style, "two, not one twice");
    }

    #[test]
    fn a_stated_header_height_is_tight_and_the_full_screen_floor_is_not() {
        // The difference upstream draws between `tightFor(height:)` and
        // `BoxConstraints(minHeight:)`. A caller who asks for 64 gets 64; a
        // full-screen header with nothing asked gets "at least 72", because it
        // has a status bar to clear and may need more.
        let stated = search_view_header_constraints(Some(64.0), false).expect("stated");
        assert_eq!((stated.min_height, stated.max_height), (64.0, 64.0));

        let full = search_view_header_constraints(None, true).expect("a floor");
        assert_eq!(full.min_height, ResolvedSearchView::FULL_SCREEN_BAR_HEIGHT);
        assert_eq!(full.max_height, f32::INFINITY, "a floor, not a height");

        assert_eq!(
            search_view_header_constraints(None, false),
            None,
            "a docked header says nothing and the bar's own 56 stands"
        );
    }

    #[test]
    fn a_stated_height_beats_the_full_screen_floor() {
        // The `??` order upstream: `headerConstraints ?? (showFullScreenView ?
        // ... : null)`. A caller who states a height means it on a full-screen
        // view too.
        let stated = search_view_header_constraints(Some(40.0), true).expect("stated");
        assert_eq!((stated.min_height, stated.max_height), (40.0, 40.0));
    }

    #[test]
    fn a_full_screen_header_is_taller_than_a_docked_one() {
        // The two constraints, seen through a laid-out bar rather than read off
        // the struct: the docked header is the bar's own 56 and the full-screen
        // one is the view's 72.
        let laid_out = |full_screen: bool| {
            let bar = search_view_header(&view(full_screen), full_screen, 1);
            let mut tree = ElementTree::new();
            tree.rebuild(stateful(bar));
            let root = tree.build_render_tree().expect("a root");
            crate::render::schedule_root_layout(&root, BoxConstraints::loose(500.0, 400.0));
            crate::render::flush_layout();
            root.size()
        };
        assert_eq!(laid_out(false).height, 56.0, "the bar's own minimum");
        assert_eq!(
            laid_out(true).height,
            ResolvedSearchView::FULL_SCREEN_BAR_HEIGHT
        );
    }

    #[test]
    fn overriding_one_thing_leaves_the_rest_of_the_theme_alone() {
        // It is an override, not a replacement. The mistake it guards against
        // is a header that sets three colours and silently loses the shape,
        // the constraints and the capitalisation with them.
        let plain = resolved_of(SearchBar::new(1));
        let tinted = resolved_of(SearchBar::new(1).with_overrides(SearchBarOverrides {
            background_color: Some(Color::argb(255, 255, 0, 0)),
            ..SearchBarOverrides::default()
        }));
        assert_eq!(tinted.background_color, Color::argb(255, 255, 0, 0));
        assert_eq!(tinted.shape, plain.shape);
        assert_eq!(tinted.elevation, plain.elevation);
        assert_eq!(tinted.padding, plain.padding);
    }

    #[test]
    fn a_header_paints_its_hint_in_the_views_hint_colour() {
        // End to end: the override reaches the canvas rather than only the
        // struct. The view's hint style and the bar's are both derived from
        // `bodyLarge`, so this is checked by overriding it outright.
        let view = view(false);
        let mut header = search_view_header(&view, false, 1);
        let marked = TextStyle {
            color: Color::argb(255, 0, 200, 0),
            ..view.header_hint_style.clone().unwrap_or_default()
        };
        header = header.with_overrides(SearchBarOverrides {
            hint_style: Some(marked.clone()),
            ..SearchBarOverrides::default()
        });
        header.hint_text = Some("Search".to_string());

        let mut tree = ElementTree::new();
        tree.rebuild(stateful(header));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(500.0, 400.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(500, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(500.0, 400.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let hint = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, argb, .. } if text == "Search" => Some(argb),
                _ => None,
            })
            .expect("the hint reached the canvas");
        assert_eq!(Color(hint), marked.color);
    }
}

#[cfg(test)]
mod search_view_body_tests {
    use super::*;
    use crate::animation::AnimationStatus;
    use crate::engine::Color;
    use crate::engine_test_stubs::Drawn;
    use crate::render::{BoxConstraints, Offset, RenderBox, RenderRef, Size};

    fn body() -> SearchViewBody {
        SearchViewBody {
            full_screen: false,
            shrink_wrap: false,
            min_height: 240.0,
            has_results: false,
        }
    }

    /// A coloured block that fills whatever it is given, so a test can find it
    /// on the canvas and see how tall it was made.
    fn block(color: Color) -> crate::render::BoxedRender {
        RenderRef::new(
            crate::render::RenderDecoratedBox::new().with_fill(crate::render::Fill::Solid(color)),
        )
    }

    const HEADER: Color = Color(0xFF00_00FF);
    const DIVIDER: Color = Color(0xFF00_FF00);
    const LIST: Color = Color(0xFFFF_0000);

    /// Lays the column out in a 400x600 panel and answers each block's
    /// rectangle, plus the opacity layers in the order they were pushed.
    fn painted(
        body: SearchViewBody,
        t: f32,
        top_padding: f32,
    ) -> (Vec<(Color, f32, f32, f32)>, Vec<u8>) {
        let column = search_view_column(
            body,
            t,
            AnimationStatus::Forward,
            top_padding,
            RenderRef::new(
                crate::render::RenderDecoratedBox::new()
                    .with_fill(crate::render::Fill::Solid(HEADER))
                    .with_child(crate::widgets::SizedBox::new(400.0, 56.0)),
            ),
            // Deliberately narrower than the column: the divider is a rule
            // across the whole panel, and it is the cross-axis *stretch* that
            // makes it one. A 400-wide probe would be stretched and centred
            // alike, and could not tell the two apart.
            RenderRef::new(
                crate::render::RenderDecoratedBox::new()
                    .with_fill(crate::render::Fill::Solid(DIVIDER))
                    .with_child(crate::widgets::SizedBox::new(100.0, 1.0)),
            ),
            block(LIST),
        );
        let mut column: crate::render::BoxedRender = RenderRef::new(column);
        RenderBox::layout(&mut column, BoxConstraints::tight(400.0, 600.0));
        let mut layers = crate::engine::LayerTree::new(400, 600);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(400.0, 600.0));
            RenderBox::paint(&column, &mut context, Offset::ZERO);
        }
        let drawn = crate::engine_test_stubs::drawn();
        let blocks = drawn
            .iter()
            .filter_map(|call| match call {
                Drawn::Rect {
                    left,
                    right,
                    top,
                    bottom,
                    argb,
                    ..
                } => Some((Color(*argb), *top, *bottom, *right - *left)),
                _ => None,
            })
            .collect();
        let alphas = drawn
            .iter()
            .filter_map(|call| match call {
                Drawn::OpacityLayer { alpha } => Some(*alpha),
                _ => None,
            })
            .collect();
        (blocks, alphas)
    }

    #[test]
    fn a_view_with_room_below_the_header_shows_a_divider_and_a_list() {
        let (blocks, _) = painted(body(), 1.0, 0.0);
        let colours: Vec<Color> = blocks.iter().map(|block| block.0).collect();
        assert!(colours.contains(&HEADER) && colours.contains(&DIVIDER));
        assert!(colours.contains(&LIST));

        // And the rule runs the whole width of the panel, though the widget
        // handed in asked for a hundred: `crossAxisAlignment: stretch`.
        let divider = blocks
            .iter()
            .find(|block| block.0 == DIVIDER)
            .expect("the rule painted");
        assert_eq!(divider.3, 400.0, "a rule that stops short is a dash");
    }

    #[test]
    fn a_shrink_wrapping_docked_view_with_nothing_to_show_is_only_a_field() {
        // The one case of the four where the guard closes: a rule under the
        // header would be pointing at blank space.
        let bare = SearchViewBody {
            shrink_wrap: true,
            min_height: 0.0,
            full_screen: false,
            has_results: false,
        };
        assert!(!bare.shows_the_list());
        let (blocks, _) = painted(bare, 1.0, 0.0);
        let colours: Vec<Color> = blocks.iter().map(|block| block.0).collect();
        assert!(colours.contains(&HEADER));
        assert!(!colours.contains(&DIVIDER), "no rule pointing at nothing");
        assert!(!colours.contains(&LIST));
    }

    #[test]
    fn any_one_of_the_four_is_enough_to_open_the_panel() {
        // The guard is an or, and three of the four have nothing to do with
        // whether anything has been typed.
        let bare = SearchViewBody {
            shrink_wrap: true,
            min_height: 0.0,
            full_screen: false,
            has_results: false,
        };
        assert!(!bare.shows_the_list(), "the closed case");
        assert!(
            SearchViewBody {
                shrink_wrap: false,
                ..bare
            }
            .shows_the_list(),
            "a view that does not shrink-wrap has room below the header"
        );
        assert!(
            SearchViewBody {
                min_height: 1.0,
                ..bare
            }
            .shows_the_list(),
            "so does one with a floor to fill"
        );
        assert!(
            SearchViewBody {
                full_screen: true,
                ..bare
            }
            .shows_the_list(),
            "and one that is the screen"
        );
        assert!(
            SearchViewBody {
                has_results: true,
                ..bare
            }
            .shows_the_list(),
            "and one that has something to say"
        );
    }

    #[test]
    fn only_a_docked_shrink_wrapping_view_lets_its_list_be_short() {
        // `fit: (shrinkWrap && !fullScreen) ? loose : tight`. A full-screen
        // view has a screen to fill even when it shrink-wraps.
        assert!(body().list_fills_the_room(), "the ordinary view");
        assert!(
            !SearchViewBody {
                shrink_wrap: true,
                ..body()
            }
            .list_fills_the_room()
        );
        assert!(
            SearchViewBody {
                shrink_wrap: true,
                full_screen: true,
                ..body()
            }
            .list_fills_the_room(),
            "a screen is a screen"
        );
    }

    #[test]
    fn a_list_that_must_fill_the_room_takes_all_of_what_is_left() {
        // Seen through the layout rather than off the flag: 600 tall, 56 of
        // header and 1 of divider, so a tight list is 543 and a loose one --
        // a block that would take everything offered -- is the same here, so
        // the difference is asked for with a list that wants nothing.
        let (blocks, _) = painted(body(), 1.0, 0.0);
        let list = blocks
            .iter()
            .find(|block| block.0 == LIST)
            .expect("the list painted");
        assert!(
            (list.2 - list.1 - 543.0).abs() < 1.0,
            "600 - 56 - 1: {list:?}"
        );
    }

    #[test]
    fn the_top_inset_pads_the_header_and_moves_everything_below_it() {
        let (flush, _) = painted(body(), 1.0, 0.0);
        let (inset, _) = painted(body(), 1.0, 44.0);
        let top_of = |blocks: &Vec<(Color, f32, f32, f32)>, wanted: Color| {
            blocks
                .iter()
                .find(|block| block.0 == wanted)
                .expect("painted")
                .1
        };
        assert_eq!(top_of(&flush, HEADER), 0.0);
        assert_eq!(top_of(&inset, HEADER), 44.0, "the header moved down");
        assert_eq!(
            top_of(&inset, DIVIDER) - top_of(&flush, DIVIDER),
            44.0,
            "and so did the rule under it"
        );
    }

    #[test]
    fn the_column_carries_three_fades_that_are_not_the_same_fade() {
        // The one named for the icons is over the whole column, and the other
        // two are inside it -- so the divider and the list are multiplied by
        // two curves each. At a fifth of the way in the three are all
        // different, which is the whole of the staggering.
        // No moment has all three part-way: the divider's sixth is over
        // before the column's own fade even starts, which is the staggering
        // stated as a fact about the constants. At 0.3 the divider is done --
        // and a finished fade pushes **no layer at all** -- while the column
        // and the list are each part-way, and by different amounts.
        let t = 0.3;
        let (_, alphas) = painted(body(), t, 0.0);
        let expect = |interval| {
            (SearchViewTransition::fade_at(interval, t, AnimationStatus::Forward) * 255.0).round()
                as u8
        };
        assert_eq!(
            SearchViewTransition::fade_at(
                SearchViewTransition::divider_fade(),
                t,
                AnimationStatus::Forward
            ),
            1.0,
            "the rule is already in"
        );
        assert_eq!(
            alphas,
            vec![
                expect(SearchViewTransition::icons_fade()),
                expect(SearchViewTransition::list_fade()),
            ],
            "outermost first: the column, then the results inside it"
        );
        assert_ne!(alphas[0], alphas[1], "two curves, not one twice");
    }

    #[test]
    fn nothing_is_faded_once_the_opening_is_over() {
        // A fully opaque subtree pushes no layer at all, so an empty list here
        // is the whole assertion: three curves, all arrived.
        let (_, alphas) = painted(body(), 1.0, 0.0);
        assert_eq!(alphas, Vec::<u8>::new());
    }
}

#[cfg(test)]
mod search_view_panel_tests {
    use super::*;
    use crate::borders::{
        BorderRadius, BorderRadiusGeometry, BorderSide, RoundedRectangleBorder, ShapeBorder,
    };
    use crate::component_themes::ResolvedSearchView;
    use crate::engine::Color;
    use crate::engine_test_stubs::Drawn;
    use crate::render::{BoxConstraints, EdgeInsets, Offset, RenderBox, RenderRef, Size};

    const CONTENT: Color = Color(0xFFFF_00FF);

    fn a_view() -> ResolvedSearchView {
        ResolvedSearchView {
            background_color: Color::argb(255, 0, 0, 255),
            elevation: ResolvedSearchView::ELEVATION,
            surface_tint_color: Color::TRANSPARENT,
            side: None,
            shape: ShapeBorder::Rounded(RoundedRectangleBorder::new(
                BorderSide::NONE,
                BorderRadiusGeometry::circular(ResolvedSearchView::RADIUS),
            )),
            header_height: None,
            header_text_style: None,
            header_hint_style: None,
            constraints: BoxConstraints {
                min_width: ResolvedSearchView::MIN_WIDTH,
                max_width: f32::INFINITY,
                min_height: ResolvedSearchView::MIN_HEIGHT,
                max_height: f32::INFINITY,
            },
            padding: None,
            bar_padding: EdgeInsets::symmetric(8.0, 0.0),
            shrink_wrap: false,
            divider_color: Color::argb(255, 128, 128, 128),
        }
    }

    /// A block that takes everything it is offered, so a test can read back
    /// the constraints the panel handed its content.
    fn content() -> crate::render::BoxedRender {
        RenderRef::new(
            crate::render::RenderDecoratedBox::new().with_fill(crate::render::Fill::Solid(CONTENT)),
        )
    }

    /// Lays a panel out in `at` and answers everything drawn.
    fn painted(
        view: &ResolvedSearchView,
        full_screen: bool,
        view_max_width: f32,
        screen_width: f32,
        at: Size,
    ) -> Vec<Drawn> {
        let mut panel =
            search_view_panel(view, full_screen, view_max_width, screen_width, content());
        RenderBox::layout(&mut panel, BoxConstraints::tight(at.width, at.height));
        let mut layers = crate::engine::LayerTree::new(1000, 1000);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(1000.0, 1000.0));
            RenderBox::paint(&panel, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn content_width(drawn: &[Drawn]) -> f32 {
        drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Rect {
                    left, right, argb, ..
                } if Color(*argb) == CONTENT => Some(*right - *left),
                _ => None,
            })
            .expect("the content painted")
    }

    #[test]
    fn the_content_is_laid_out_at_the_width_the_view_will_end_at() {
        // The whole reason for the `OverflowBox`. Halfway through the opening
        // the panel is 200 wide and the content is already 500 -- so the text
        // inside it was wrapped once, at the final width, and is merely being
        // revealed. Measuring it against the growing rectangle would re-wrap
        // every line on every frame.
        let drawn = painted(&a_view(), false, 500.0, 1000.0, Size::new(200.0, 300.0));
        assert_eq!(content_width(&drawn), 500.0);
    }

    #[test]
    fn the_content_is_never_laid_out_wider_than_the_screen() {
        // `math.min(viewMaxWidth, screenSize.width)`. `view_rect` deliberately
        // lets a view be wider than the window -- it moves the corner and
        // leaves the size -- and this is what stops the *content* from
        // following it out there.
        let drawn = painted(&a_view(), false, 900.0, 600.0, Size::new(200.0, 300.0));
        assert_eq!(content_width(&drawn), 600.0);
    }

    #[test]
    fn the_panel_is_clipped_to_its_shape_at_the_size_it_was_laid_out_at() {
        // `Material(clipBehavior: Clip.antiAlias, shape:)`. Without it the
        // content, which is wider than the panel by design, would be seen
        // hanging out of the rounded corners.
        let drawn = painted(&a_view(), false, 500.0, 1000.0, Size::new(200.0, 300.0));
        let clipped = drawn.iter().any(|call| {
            matches!(
                call,
                Drawn::ClipPathLayer { .. } | Drawn::ClipRRectLayer { .. }
            )
        });
        assert!(clipped, "the panel clips: {drawn:?}");
    }

    #[test]
    fn a_shape_decides_the_clip_and_a_stadium_needs_the_size_to_do_it() {
        // The clip follows the shape rather than a constant, and it is
        // resolved after layout for the same reason the surface's rounding is:
        // a stadium is half the shorter side.
        // Asked directly rather than through a `RenderRef`: a boxed handle
        // does not downcast back to the object inside it, which round 434
        // recorded after a test passed for that reason and not for the right
        // one.
        let mut stadium = crate::render::RenderClipRRect::new(
            BorderRadius::ZERO,
            crate::widgets::SizedBox::new(200.0, 56.0),
        )
        .with_shape(ShapeBorder::Stadium(
            crate::borders::StadiumBorder::default(),
        ));
        RenderBox::layout(&mut stadium, BoxConstraints::tight(200.0, 56.0));
        assert_eq!(stadium.rounding(), BorderRadius::circular(28.0));

        let mut taller = crate::render::RenderClipRRect::new(
            BorderRadius::ZERO,
            crate::widgets::SizedBox::new(200.0, 120.0),
        )
        .with_shape(ShapeBorder::Stadium(
            crate::borders::StadiumBorder::default(),
        ));
        RenderBox::layout(&mut taller, BoxConstraints::tight(200.0, 120.0));
        assert_eq!(taller.rounding(), BorderRadius::circular(60.0));
    }

    #[test]
    fn a_shape_that_is_not_a_rounded_rectangle_leaves_the_given_radius_alone() {
        // `corner_radius` answers nothing for a circle, and the fallback is
        // the radius the caller set rather than square corners.
        let mut clip = crate::render::RenderClipRRect::new(
            BorderRadius::circular(9.0),
            crate::widgets::SizedBox::new(100.0, 100.0),
        )
        .with_shape(ShapeBorder::Circle(crate::borders::CircleBorder::default()));
        RenderBox::layout(&mut clip, BoxConstraints::tight(100.0, 100.0));
        assert_eq!(clip.rounding(), BorderRadius::circular(9.0));
    }

    #[test]
    fn a_docked_panel_takes_the_themes_padding_and_a_full_screen_one_takes_none() {
        // A full-screen view is the screen: an inset would leave a strip of
        // the page showing around the thing that replaced it.
        let mut view = a_view();
        view.padding = Some(EdgeInsets::all(12.0));

        // The **last** path, not the first: a decorated box paints its
        // shadows before its fill, and a shadow is offset and spread, so the
        // first path is at neither the panel's corner nor anywhere useful.
        let surface_of = |drawn: Vec<Drawn>| {
            drawn
                .iter()
                .filter_map(|call| match call {
                    Drawn::Path { left, top, .. } => Some((*left, *top)),
                    _ => None,
                })
                .next_back()
                .expect("the surface painted")
        };
        assert_eq!(
            surface_of(painted(
                &view,
                false,
                500.0,
                1000.0,
                Size::new(400.0, 300.0)
            )),
            (12.0, 12.0),
            "inset by the theme's padding"
        );
        assert_eq!(
            surface_of(painted(&view, true, 500.0, 1000.0, Size::new(400.0, 300.0))),
            (0.0, 0.0),
            "a screen has no margin"
        );
    }

    /// A block of a fixed size, for the cases where the panel's own choices
    /// only show when the content does *not* want everything on offer.
    fn small_content() -> crate::render::BoxedRender {
        RenderRef::new(
            crate::render::RenderDecoratedBox::new()
                .with_fill(crate::render::Fill::Solid(CONTENT))
                .with_child(crate::widgets::SizedBox::new(50.0, 150.0)),
        )
    }

    /// Every render object of one type in a laid-out tree, innermost last.
    fn walk(root: &crate::render::RenderRef, visit: &mut dyn FnMut(&crate::render::RenderRef)) {
        let children: Vec<crate::render::RenderRef> = root.with(|object| {
            let mut found = Vec::new();
            object.visit_children(&mut |child, _| {
                if let Some(handle) = child.as_any().downcast_ref::<crate::render::RenderRef>() {
                    found.push(handle.clone());
                }
            });
            found
        });
        for child in children {
            visit(&child);
            walk(&child, visit);
        }
    }

    #[test]
    fn content_that_wants_to_be_narrow_is_allowed_to_be() {
        // `minWidth: 0`. The overflow box replaces the panel's constraints
        // outright, so without this a list of short suggestions would be
        // stretched to the view's whole width and the panel would have no way
        // to say otherwise.
        let mut panel = search_view_panel(&a_view(), false, 500.0, 1000.0, small_content());
        RenderBox::layout(&mut panel, BoxConstraints::tight(200.0, 300.0));
        let mut layers = crate::engine::LayerTree::new(1000, 1000);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(1000.0, 1000.0));
            RenderBox::paint(&panel, &mut context, Offset::ZERO);
        }
        assert_eq!(content_width(&crate::engine_test_stubs::drawn()), 50.0);
    }

    #[test]
    fn the_panel_takes_its_height_from_its_content_when_it_is_given_a_choice() {
        // `fit: deferToChild`, which upstream notes "cannot be sizedByParent".
        // It only shows where the panel has room to spare: the rectangle it is
        // laid out in is tight across and loose down once the view has
        // arrived, and a panel that ignored its child would be 600 tall around
        // 150 of suggestions.
        let mut panel = search_view_panel(&a_view(), false, 500.0, 1000.0, small_content());
        let height = RenderBox::layout(
            &mut panel,
            crate::render::BoxConstraints::new(400.0, 400.0, 0.0, 600.0),
        )
        .height;
        assert_eq!(height, 150.0, "the content's height, not the room's");
    }

    #[test]
    fn the_panels_clip_takes_the_views_shape() {
        // Found in the tree rather than asserted on the drawing, because the
        // stub records a clip path's bounds and not its rounding -- so "it
        // clips" and "it clips to 28" look the same on the canvas.
        let mut panel = search_view_panel(&a_view(), false, 500.0, 1000.0, content());
        RenderBox::layout(&mut panel, BoxConstraints::tight(400.0, 300.0));
        let mut rounding = None;
        walk(&panel, &mut |handle| {
            handle.with(|object| {
                if let Some(clip) = object
                    .as_any()
                    .downcast_ref::<crate::render::RenderClipRRect>()
                {
                    rounding = Some(clip.rounding());
                }
            });
        });
        assert_eq!(
            rounding,
            Some(BorderRadius::circular(ResolvedSearchView::RADIUS)),
            "the theme's 28, not a square corner"
        );
    }

    #[test]
    fn the_panel_casts_the_shadow_its_elevation_asks_for() {
        // `Material(elevation:)`. Six, shared with the bar.
        let drawn = painted(&a_view(), false, 500.0, 1000.0, Size::new(400.0, 300.0));
        let shadows = crate::painting::elevation_shadows(ResolvedSearchView::ELEVATION as u32);
        assert!(!shadows.is_empty(), "or there is nothing to look for");
        let paths = drawn
            .iter()
            .filter(|call| matches!(call, Drawn::Path { .. }))
            .count();
        assert!(
            paths > shadows.len(),
            "one path per shadow and one for the surface: {paths}"
        );
    }
}

#[cfg(test)]
mod search_view_content_tests {
    use super::*;
    use crate::animation::AnimationStatus;
    use crate::borders::{BorderRadiusGeometry, BorderSide, RoundedRectangleBorder, ShapeBorder};
    use crate::component_themes::ResolvedSearchView;
    use crate::engine::{Color, Rect};
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{AnyWidget, ElementTree, leaf};
    use crate::render::{BoxConstraints, EdgeInsets, Offset, RenderBox, Size};
    use crate::theatre::{OverlayHandle, overlay};
    use std::cell::RefCell;
    use std::rc::Rc;

    const SCREEN: Size = Size {
        width: 800.0,
        height: 900.0,
    };
    const HEADER: Color = Color(0xFF00_00FF);
    const DIVIDER: Color = Color(0xFF00_FF00);
    const LIST: Color = Color(0xFFFF_0000);

    fn constraints() -> BoxConstraints {
        BoxConstraints {
            min_width: ResolvedSearchView::MIN_WIDTH,
            max_width: f32::INFINITY,
            min_height: ResolvedSearchView::MIN_HEIGHT,
            max_height: f32::INFINITY,
        }
    }

    fn a_view() -> ResolvedSearchView {
        ResolvedSearchView {
            background_color: Color::argb(255, 0, 0, 128),
            elevation: ResolvedSearchView::ELEVATION,
            surface_tint_color: Color::TRANSPARENT,
            side: None,
            shape: ShapeBorder::Rounded(RoundedRectangleBorder::new(
                BorderSide::NONE,
                BorderRadiusGeometry::circular(ResolvedSearchView::RADIUS),
            )),
            header_height: None,
            header_text_style: None,
            header_hint_style: None,
            constraints: constraints(),
            padding: None,
            bar_padding: EdgeInsets::symmetric(8.0, 0.0),
            shrink_wrap: false,
            divider_color: Color::argb(255, 128, 128, 128),
        }
    }

    pub(super) fn content() -> SearchViewContent {
        let anchor = Rect::xywh(20.0, 40.0, 400.0, 56.0);
        SearchViewContent {
            placement: SearchViewTransition {
                anchor,
                view: SearchAnchor::view_rect(
                    anchor,
                    SCREEN,
                    constraints(),
                    false,
                    crate::direction::TextDirection::Ltr,
                ),
                full_screen: false,
            },
            view: a_view(),
            constraints: constraints(),
            screen: SCREEN,
            has_results: false,
            media_top: 44.0,
            header_id: 1,
            hint_text: Some("Search".to_string()),
            controller: SharedSearchController::default(),
        }
    }

    fn block(color: Color, width: f32, height: f32) -> AnyWidget {
        leaf(move || {
            crate::render::RenderDecoratedBox::new()
                .with_fill(crate::render::Fill::Solid(color))
                .with_child(crate::widgets::SizedBox::new(width, height))
        })
    }

    /// Everything the content paints at `t`, laid out in the whole screen.
    fn painted(content: &SearchViewContent, t: f32) -> Vec<Drawn> {
        let mut tree = ElementTree::new();
        tree.rebuild(content.build(
            t,
            AnimationStatus::Forward,
            block(HEADER, 400.0, 56.0),
            block(DIVIDER, 400.0, 1.0),
            block(LIST, 400.0, 200.0),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(
            &root,
            BoxConstraints::tight(SCREEN.width, SCREEN.height),
        );
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(800, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(&mut layers, SCREEN);
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn colours(drawn: &[Drawn]) -> Vec<Color> {
        drawn
            .iter()
            .filter_map(|call| match call {
                Drawn::Rect { argb, .. } => Some(Color(*argb)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_content_is_a_panel_with_a_header_a_rule_and_a_list_in_it() {
        // The join itself: four functions that did not call one another until
        // now, and one call that reaches all four.
        let drawn = painted(&content(), 1.0);
        let colours = colours(&drawn);
        assert!(colours.contains(&HEADER), "the header");
        assert!(colours.contains(&DIVIDER), "the rule");
        assert!(colours.contains(&LIST), "the results");
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::ClipPathLayer { .. })),
            "and the panel that clips them"
        );
    }

    #[test]
    fn the_bodys_decision_is_remade_every_frame() {
        // `minHeight` is `math.min(effectiveConstraints.minHeight,
        // _viewRect.height)` and `_viewRect` is the animated rectangle, so
        // this is a property of the frame and not of the view. At t = 0 the
        // rectangle is the bar's 56; at t = 1 it is the view's 600, which is
        // over the 240 minimum and clamps to it.
        let content = content();
        assert_eq!(
            content.body_at(0.0, AnimationStatus::Forward).min_height,
            56.0,
            "the bar's height, clamped down from 240"
        );
        assert_eq!(
            content.body_at(1.0, AnimationStatus::Forward).min_height,
            ResolvedSearchView::MIN_HEIGHT,
            "and the view's minimum once there is room for it"
        );
    }

    #[test]
    fn a_shrink_wrapping_docked_view_with_nothing_to_show_never_grows_a_rule() {
        // The one case where the body's guard closes, and the reason the
        // clamped minimum has to be the animated one: a view whose minimum is
        // zero has a clamped minimum of zero on every frame, so the answer is
        // the same all the way through rather than flipping partway.
        let mut content = content();
        content.view.shrink_wrap = true;
        content.constraints.min_height = 0.0;
        for t in [0.0, 0.25, 0.5, 1.0] {
            assert!(
                !content
                    .body_at(t, AnimationStatus::Forward)
                    .shows_the_list(),
                "at {t}"
            );
        }
        let colours = colours(&painted(&content, 1.0));
        assert!(colours.contains(&HEADER));
        assert!(!colours.contains(&DIVIDER));
    }

    #[test]
    fn the_three_go_into_the_column_in_the_order_they_were_given() {
        // Nothing above says which is which: three blocks all present is the
        // same observation whichever order they were handed over in, and a
        // header under the results would look like a bug in the column rather
        // than in the wiring here.
        let drawn = painted(&content(), 1.0);
        let top_of = |wanted: Color| {
            drawn
                .iter()
                .find_map(|call| match call {
                    Drawn::Rect { top, argb, .. } if Color(*argb) == wanted => Some(*top),
                    _ => None,
                })
                .expect("painted")
        };
        assert!(top_of(HEADER) < top_of(DIVIDER), "the field is at the top");
        assert!(
            top_of(DIVIDER) < top_of(LIST),
            "then the rule, then the results"
        );
    }

    #[test]
    fn results_alone_are_enough_to_open_a_panel_that_would_otherwise_be_bare() {
        // The fourth of the body's four conditions, reached through the
        // content rather than by hand: only this one has anything to do with
        // whether something has been typed.
        let mut bare = content();
        bare.view.shrink_wrap = true;
        bare.constraints.min_height = 0.0;
        assert!(!bare.body_at(1.0, AnimationStatus::Forward).shows_the_list());

        let mut with_results = bare.clone();
        with_results.has_results = true;
        assert!(
            with_results
                .body_at(1.0, AnimationStatus::Forward)
                .shows_the_list()
        );
        assert!(colours(&painted(&with_results, 1.0)).contains(&DIVIDER));
    }

    #[test]
    fn a_full_screen_view_opens_its_panel_even_when_it_shrink_wraps() {
        // The third condition. A screen has room below the header whether or
        // not the view was asked to wrap around its content.
        let mut bare = content();
        bare.view.shrink_wrap = true;
        bare.constraints.min_height = 0.0;
        assert!(!bare.body_at(1.0, AnimationStatus::Forward).shows_the_list());

        let mut full = bare.clone();
        full.placement.full_screen = true;
        assert!(full.body_at(1.0, AnimationStatus::Forward).shows_the_list());
        assert!(colours(&painted(&full, 1.0)).contains(&DIVIDER));
    }

    #[test]
    fn a_narrow_bar_still_lays_its_panel_out_at_the_views_width() {
        // The panel's content is measured at `_rectTween.end!.width`, and the
        // case that shows it is a bar narrower than the view's minimum: a
        // 200-wide bar opens a 360-wide view, so measuring against the bar
        // would wrap every line 160 pixels too soon and then re-wrap them as
        // the panel grew.
        let anchor = Rect::xywh(20.0, 40.0, 200.0, 56.0);
        let mut narrow = content();
        narrow.placement.anchor = anchor;
        narrow.placement.view = SearchAnchor::view_rect(
            anchor,
            SCREEN,
            constraints(),
            false,
            crate::direction::TextDirection::Ltr,
        );
        assert_eq!(narrow.placement.view.width(), ResolvedSearchView::MIN_WIDTH);

        // Measured from a header that takes everything it is offered.
        let mut tree = ElementTree::new();
        // At the end of the opening, but laid out in the *bar's* rectangle:
        // the two are independent, and it is the second that the overflow box
        // is answering. (At t = 0 nothing paints at all -- the column's own
        // fade has not started -- which is a fact about the fades, not about
        // the width.)
        tree.rebuild(narrow.build(
            1.0,
            AnimationStatus::Forward,
            leaf(|| {
                crate::render::RenderDecoratedBox::new()
                    .with_fill(crate::render::Fill::Solid(HEADER))
            }),
            block(DIVIDER, 400.0, 1.0),
            block(LIST, 400.0, 200.0),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(
            &root,
            BoxConstraints::tight(anchor.width(), anchor.height()),
        );
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(800, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(&mut layers, SCREEN);
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let width = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                Drawn::Rect {
                    left, right, argb, ..
                } if Color(argb) == HEADER => Some(right - left),
                _ => None,
            })
            .expect("the header painted");
        assert_eq!(
            width,
            ResolvedSearchView::MIN_WIDTH,
            "laid out at the view's width though the panel is the bar's"
        );
    }

    #[test]
    fn only_a_full_screen_view_pushes_its_header_down_for_the_status_bar() {
        let top_of = |content: &SearchViewContent, wanted: Color| {
            painted(content, 1.0)
                .into_iter()
                .find_map(|call| match call {
                    Drawn::Rect { top, argb, .. } if Color(argb) == wanted => Some(top),
                    _ => None,
                })
                .expect("painted")
        };
        let docked = content();
        let mut full = content();
        full.placement.full_screen = true;
        full.placement.view = Rect::xywh(0.0, 0.0, SCREEN.width, SCREEN.height);

        // Local coordinates: `build` is everything *inside* the animated
        // rectangle, and the move to the rectangle's corner is the route's
        // transform, a layer above this one.
        assert_eq!(top_of(&docked, HEADER), 0.0, "a docked header sits flush");
        assert_eq!(
            top_of(&full, HEADER),
            full.media_top,
            "a full-screen header clears the status bar"
        );
    }

    #[test]
    fn a_view_opened_through_the_route_paints_its_own_panel() {
        // `open_search_view` is `show_search_view` with the content filled in,
        // and the point of it is that a caller no longer supplies one.
        let found: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));
        struct Finder(Rc<RefCell<Option<Rc<OverlayHandle>>>>);
        impl crate::framework::Component for Finder {
            fn build(&self, context: &mut crate::framework::BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = OverlayHandle::of(context);
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Finder(Rc::clone(
            &found,
        )))));
        tree.build_render_tree();
        let handle = found.borrow().clone().expect("a descendant found it");

        let shown = open_search_view(Rc::clone(&handle), content(), || block(LIST, 400.0, 200.0))
            .expect("shown");

        // Stepped, because `advance` clamps each frame to 50ms.
        tree.rebuild_dirty();
        tree.advance_frame(0);
        tree.rebuild_dirty();
        let mut now = 0;
        while now < SearchViewTransition::OPEN_MICROS {
            now = (now + 16_000).min(SearchViewTransition::OPEN_MICROS);
            tree.advance_frame(now);
            tree.rebuild_dirty();
        }

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(
            &root,
            BoxConstraints::tight(SCREEN.width, SCREEN.height),
        );
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(800, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(&mut layers, SCREEN);
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let drawn = crate::engine_test_stubs::drawn();
        shown.dismiss();

        // The list is the caller's; the other two are the view's own, so they
        // are looked for as what they *are* rather than as marker blocks.
        assert!(colours(&drawn).contains(&LIST), "the suggestions");
        assert!(
            drawn.iter().any(|call| matches!(
                call,
                Drawn::Paragraph { text, .. } if text == "Search"
            )),
            "the header's hint, which means the header is the view's own bar"
        );
        let divider_colour = a_view().divider_color;
        assert!(
            drawn.iter().any(|call| match call {
                Drawn::Rect {
                    argb, top, bottom, ..
                } => Color(*argb) == divider_colour && *bottom - *top <= 1.0,
                _ => false,
            }),
            "and a one-pixel rule in the view's divider colour: {drawn:?}"
        );
    }

    #[test]
    fn the_views_rule_is_one_pixel_tall_and_the_views_colour() {
        // Upstream's `Divider(height: 1)` under a `DividerTheme` whose colour
        // is the view's. The default divider reserves sixteen -- air on both
        // sides of a rule between two list items -- and here that air would
        // read as a gap in the panel's surface.
        let mut tree = ElementTree::new();
        tree.rebuild(search_view_divider(&a_view()));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 400.0));
        crate::render::flush_layout();
        assert_eq!(root.size().height, 1.0, "not the theme's sixteen");

        let mut layers = crate::engine::LayerTree::new(400, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(400.0, 400.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let painted = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                Drawn::Rect { argb, .. } => Some(Color(argb)),
                _ => None,
            })
            .expect("the rule painted");
        assert_eq!(painted, a_view().divider_color);
    }

    #[test]
    fn a_rule_follows_the_view_it_was_built_from_and_not_a_constant() {
        let mut recoloured = a_view();
        recoloured.divider_color = Color::argb(255, 7, 8, 9);
        let mut tree = ElementTree::new();
        tree.rebuild(search_view_divider(&recoloured));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 400.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(400, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(400.0, 400.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let painted = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                Drawn::Rect { argb, .. } => Some(Color(argb)),
                _ => None,
            })
            .expect("the rule painted");
        assert_eq!(painted, Color::argb(255, 7, 8, 9));
    }

    #[test]
    fn the_header_the_content_builds_is_the_views_own_bar() {
        // The same overrides `search_view_header` applies, plus the view's
        // hint text -- so a caller cannot hand the panel a header that is not
        // the bar it grew out of.
        let content = content();
        let header = content.header();
        assert_eq!(header.id, content.header_id, "the field keeps its identity");
        assert_eq!(header.hint_text, content.hint_text);

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::stateful(header));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 400.0));
        crate::render::flush_layout();
        assert_eq!(
            root.size().height,
            56.0,
            "a docked header is the bar's own height"
        );
    }

    #[test]
    fn a_full_screen_views_header_is_the_taller_one() {
        // The header follows the view it belongs to rather than a flag passed
        // beside it: `full_screen` lives on the placement.
        let mut full = content();
        full.placement.full_screen = true;
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::stateful(full.header()));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 400.0));
        crate::render::flush_layout();
        assert_eq!(
            root.size().height,
            ResolvedSearchView::FULL_SCREEN_BAR_HEIGHT
        );
    }
}

#[cfg(test)]
mod search_anchor_widget_tests {
    use super::*;
    use crate::engine::Color;
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{AnyWidget, ElementTree, leaf, stateful};
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};
    use crate::theatre::overlay;

    const SCREEN: Size = Size {
        width: 800.0,
        height: 900.0,
    };
    const SUGGESTION: Color = Color(0xFFFF_0000);

    /// An anchor mounted under an overlay **and a `MediaQuery`**, laid out at
    /// the screen's size.
    ///
    /// The media query is not decoration. The anchor reads the window's size
    /// from it to work out where the view goes, and its top inset to know what
    /// a full-screen view has to clear -- so without one the whole opening
    /// happens against a zero-sized screen: the view is clamped up to its
    /// minimum, and everything inside it is laid out `min(view_width, 0)`
    /// wide. A first draft of these tests had no media query and watched a
    /// 360x240 panel with nothing visible in it.
    fn staged(anchor: SearchAnchor) -> ElementTree {
        let mut tree = ElementTree::new();
        let data = crate::media_query::MediaQueryData {
            size: SCREEN,
            ..crate::media_query::MediaQueryData::default()
        };
        tree.rebuild(crate::media_query::MediaQuery::new(
            data,
            overlay(stateful(anchor)),
        ));
        tree.build_render_tree();
        tree
    }

    fn laid_out(tree: &mut ElementTree) -> crate::render::RenderRef {
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(
            &root,
            BoxConstraints::tight(SCREEN.width, SCREEN.height),
        );
        crate::render::flush_layout();
        root
    }

    fn painted(tree: &mut ElementTree) -> Vec<Drawn> {
        let root = laid_out(tree);
        let mut layers = crate::engine::LayerTree::new(800, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(&mut layers, SCREEN);
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    /// A press and a release at `at`, through the real router.
    fn tap(tree: &mut ElementTree, at: Offset) {
        let root = laid_out(tree);
        let event = |change| crate::gestures::PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: crate::gestures::PointerKind::Touch,
            signal_kind: crate::gestures::SignalKind::None,
            buttons: 1,
            time_stamp_micros: 0,
            position: at,
            delta: Offset::ZERO,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: at,
        };
        let mut router = crate::gestures::GestureRouter::new();
        router.dispatch(&root, &event(crate::gestures::PointerChange::Down));
        router.dispatch(&root, &event(crate::gestures::PointerChange::Up));
        tree.rebuild_dirty();
    }

    fn an_anchor() -> SearchAnchor {
        SearchAnchor::new(1)
            .with_full_screen(false)
            .with_hint_text("Search")
            .with_suggestions(|| {
                leaf(|| {
                    crate::render::RenderDecoratedBox::new()
                        .with_fill(crate::render::Fill::Solid(SUGGESTION))
                })
            })
    }

    #[test]
    fn an_anchor_is_a_bar_until_it_is_tapped() {
        // The whole chain now has a caller, and this is the first frame of it:
        // a search bar on the page, and nothing over it.
        let mut tree = staged(an_anchor());
        let drawn = painted(&mut tree);
        assert!(
            drawn.iter().any(|call| matches!(
                call,
                Drawn::Paragraph { text, .. } if text == "Search"
            )),
            "the bar's hint"
        );
        assert!(
            !drawn
                .iter()
                .any(|call| matches!(call, Drawn::Rect { argb, .. } if Color(*argb) == SUGGESTION)),
            "and no view"
        );
        assert_eq!(crate::theatre::modal_count(), 0);
    }

    #[test]
    fn tapping_the_bar_opens_the_view_it_grows_into() {
        let mut tree = staged(an_anchor());
        // The bar is 56 tall at the top of the screen; the middle of it.
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(crate::theatre::modal_count(), 1, "a route went up");

        // Run the opening out, stepping because `advance` clamps each frame.
        tree.advance_frame(0);
        tree.rebuild_dirty();
        let mut now = 0;
        while now < SearchViewTransition::OPEN_MICROS {
            now = (now + 16_000).min(SearchViewTransition::OPEN_MICROS);
            tree.advance_frame(now);
            tree.rebuild_dirty();
        }

        let drawn = painted(&mut tree);
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::Rect { argb, .. } if Color(*argb) == SUGGESTION)),
            "the caller's suggestions are on screen"
        );
        crate::theatre::dismiss_topmost_modal();
    }

    #[test]
    fn tapping_where_the_bar_was_closes_the_view_over_it() {
        // The view's barrier covers the bar, so the second tap never reaches
        // the anchor: it dismisses instead. That is why the tap handler has no
        // "already open" guard -- there is no input that would reach one.
        let mut tree = staged(an_anchor());
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(crate::theatre::modal_count(), 1);
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(
            crate::theatre::modal_count(),
            0,
            "the barrier took it, and one view did not become two"
        );
    }

    #[test]
    fn the_view_opens_at_the_bar_and_not_at_the_corner_of_the_screen() {
        // The anchor's whole job on the tap: the bar's rectangle, taken from
        // the render object the bar's own assemble recorded. Without it the
        // view would grow out of (0, 0) whatever the bar's place on the page.
        let mut tree = ElementTree::new();
        // The bar is pushed down and across, so a view that ignored the anchor
        // would be visibly in the wrong place.
        tree.rebuild(overlay(crate::framework::many(
            vec![stateful(an_anchor())],
            |rendered| {
                crate::render::RenderPadding::new(
                    crate::render::EdgeInsets::only(30.0, 120.0, 0.0, 0.0),
                    rendered.into_iter().next().expect("the anchor"),
                )
            },
        )));
        tree.build_render_tree();
        tap(&mut tree, Offset::new(230.0, 148.0));
        assert_eq!(crate::theatre::modal_count(), 1, "the tap landed");

        tree.advance_frame(0);
        tree.rebuild_dirty();
        tree.advance_frame(16_000);
        tree.rebuild_dirty();

        let drawn = painted(&mut tree);
        let moved = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::TransformLayer { e, f, .. } => Some((*e, *f)),
                _ => None,
            })
            .expect("the panel was moved into place");
        assert!(
            (moved.0 - 30.0).abs() < 1.0 && (moved.1 - 120.0).abs() < 1.0,
            "the view starts where the bar is: {moved:?}"
        );
        crate::theatre::dismiss_topmost_modal();
    }

    /// Opens the view and runs the whole 600ms, then answers where the panel
    /// ended up and how big it is.
    fn opened_fully(anchor: SearchAnchor, at: Offset) -> ((f32, f32), (f32, f32)) {
        let mut tree = staged(anchor);
        tap(&mut tree, at);
        assert_eq!(crate::theatre::modal_count(), 1, "the tap landed");
        tree.advance_frame(0);
        tree.rebuild_dirty();
        let mut now = 0;
        while now < SearchViewTransition::OPEN_MICROS {
            now = (now + 16_000).min(SearchViewTransition::OPEN_MICROS);
            tree.advance_frame(now);
            tree.rebuild_dirty();
        }
        let drawn = painted(&mut tree);
        crate::theatre::dismiss_topmost_modal();
        let moved = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::TransformLayer { e, f, .. } => Some((*e, *f)),
                _ => None,
            })
            .expect("the panel was moved into place");
        // Measured from the **suggestions**, which fill what is left of the
        // panel below the header and the rule. The panel's own surface is a
        // path among several -- the shadows are paths too, and so is the
        // page's own bar behind the view -- and picking one of those out by
        // position would be guessing.
        let list = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Rect {
                    left,
                    right,
                    bottom,
                    argb,
                    ..
                } if Color(*argb) == SUGGESTION => Some((*right - *left, *bottom)),
                _ => None,
            })
            .expect("the suggestions painted");
        (moved, list)
    }

    #[test]
    fn a_docked_view_settles_over_the_bar_and_two_thirds_down_the_screen() {
        // Where `view_rect` put it, reached all the way through the widget:
        // the bar's corner, the bar's width, two thirds of the screen's
        // height. The frame this is read on matters -- early in the opening
        // the panel is still the bar's rectangle whatever its destination, so
        // a test that looked at the first frame could not tell a docked view
        // from a full-screen one.
        let (moved, list) = opened_fully(an_anchor(), Offset::new(200.0, 28.0));
        assert_eq!(moved, (0.0, 0.0), "the bar is at the screen's corner here");
        assert!(
            (list.0 - SCREEN.width).abs() < 1.0,
            "as wide as the bar, which fills the page: {list:?}"
        );
        assert!(
            (list.1 - SCREEN.height * 2.0 / 3.0).abs() < 1.0,
            "and reaching two thirds down the screen, not all of it: {list:?}"
        );
    }

    #[test]
    fn a_full_screen_view_settles_over_the_whole_screen() {
        let (moved, list) = opened_fully(
            SearchAnchor::new(1)
                .with_full_screen(true)
                .with_hint_text("Search")
                .with_suggestions(|| {
                    leaf(|| {
                        crate::render::RenderDecoratedBox::new()
                            .with_fill(crate::render::Fill::Solid(SUGGESTION))
                    })
                }),
            Offset::new(200.0, 28.0),
        );
        assert_eq!(moved, (0.0, 0.0));
        assert!(
            (list.0 - SCREEN.width).abs() < 1.0 && (list.1 - SCREEN.height).abs() < 1.0,
            "all the way to the bottom of the screen: {list:?}"
        );
    }

    #[test]
    fn the_platform_decides_full_screen_when_nobody_says() {
        // `resolve_full_screen`'s rule, reached through the conversion the
        // widget needs: a theme's `TargetPlatform` and the scroll tier's
        // `ScrollPlatform` are the same six names.
        use crate::editable_text::TargetPlatform;
        let anchor = SearchAnchor::new(1);
        assert!(anchor.resolve_full_screen(TargetPlatform::Android.into()));
        assert!(anchor.resolve_full_screen(TargetPlatform::IOS.into()));
        assert!(!anchor.resolve_full_screen(TargetPlatform::MacOS.into()));
        assert!(!anchor.resolve_full_screen(TargetPlatform::Windows.into()));
        assert!(
            !SearchAnchor::new(1)
                .with_full_screen(false)
                .resolve_full_screen(TargetPlatform::Android.into()),
            "saying so outright overrules the platform"
        );
    }

    #[test]
    fn the_view_opens_holding_what_the_bar_held() {
        // The whole reason the two share a controller. The panel grows out of
        // a filled-in bar; arriving empty would read as the search having been
        // thrown away between one frame and the next.
        let controller = SharedSearchController::default();
        controller.borrow_mut().text = "carbonara".to_string();
        let mut tree = staged(
            SearchAnchor::new(1)
                .with_full_screen(false)
                .with_hint_text("Search")
                .with_controller(std::rc::Rc::clone(&controller)),
        );
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(crate::theatre::modal_count(), 1);

        tree.advance_frame(0);
        tree.rebuild_dirty();
        let mut now = 0;
        while now < SearchViewTransition::OPEN_MICROS {
            now = (now + 16_000).min(SearchViewTransition::OPEN_MICROS);
            tree.advance_frame(now);
            tree.rebuild_dirty();
        }
        let drawn = painted(&mut tree);
        crate::theatre::dismiss_topmost_modal();

        let typed = drawn
            .iter()
            .filter(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "carbonara"))
            .count();
        assert!(typed > 0, "the query is in the view: {drawn:?}");
        assert!(
            !drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "Search")),
            "and the hint is not shown over it"
        );
    }

    #[test]
    fn an_anchor_with_no_controller_keeps_one_of_its_own() {
        // Upstream's `_internalSearchController`. An anchor nobody handed a
        // controller still has to have somewhere to put the query, or the view
        // could never open holding anything.
        let mut tree = staged(an_anchor());
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(
            crate::theatre::modal_count(),
            1,
            "it opened, so it found a controller to build the content with"
        );
        crate::theatre::dismiss_topmost_modal();
    }

    #[test]
    fn an_empty_controller_leaves_the_hint_showing() {
        // The other half of the seeding: seeding with "" must not put an empty
        // string where the hint goes.
        let mut tree = staged(an_anchor());
        tap(&mut tree, Offset::new(200.0, 28.0));
        tree.advance_frame(0);
        tree.rebuild_dirty();
        let mut now = 0;
        while now < SearchViewTransition::OPEN_MICROS {
            now = (now + 16_000).min(SearchViewTransition::OPEN_MICROS);
            tree.advance_frame(now);
            tree.rebuild_dirty();
        }
        let drawn = painted(&mut tree);
        crate::theatre::dismiss_topmost_modal();
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "Search")),
            "the header's hint is still there"
        );
    }

    #[test]
    fn what_the_view_was_left_holding_goes_back_into_the_bar() {
        // The half round 445 left open. The reader types in the view, taps the
        // barrier to close it, and the bar shows what they searched for --
        // upstream gets this for free, because there the bar's field and the
        // view's header are one field with one controller.
        let controller = SharedSearchController::default();
        let mut tree = staged(
            SearchAnchor::new(1)
                .with_full_screen(false)
                .with_hint_text("Search")
                .with_controller(std::rc::Rc::clone(&controller)),
        );
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(crate::theatre::modal_count(), 1);

        // What the reader typed into the view.
        controller.borrow_mut().text = "carbonara".to_string();
        assert!(crate::theatre::dismiss_topmost_modal(), "the barrier's tap");
        tree.rebuild_dirty();

        let drawn = painted(&mut tree);
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "carbonara")),
            "the bar shows the query: {drawn:?}"
        );
        assert!(
            !drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "Search")),
            "and the hint has given way to it"
        );

        // And the caret is after the query, not before it: the next keystroke
        // continues the search rather than typing in front of it. Upstream's
        // controller collapses to `text.length` for the same reason.
        let mut stack: Vec<crate::framework::ElementId> = tree.root().into_iter().collect();
        let mut caret = None;
        while let Some(id) = stack.pop() {
            if let Some(found) = tree.state::<crate::editable::TextFieldState, _>(id, |state| {
                (
                    state.value.text.clone(),
                    state.value.selection_base,
                    state.value.selection_extent,
                )
            }) {
                caret = Some(found);
                break;
            }
            let mut children = tree.children_of(id);
            children.reverse();
            stack.extend(children);
        }
        assert_eq!(caret, Some(("carbonara".to_string(), 9, 9)));
    }

    #[test]
    fn a_page_that_rebuilds_while_the_view_is_open_still_gets_its_query_back() {
        // The sink lives in the anchor's **state**, not in its build. One made
        // per build would leave the tap's closure holding the sink from the
        // build the tap happened on, while the bar published into a newer one
        // -- and the handle in the old sink is stale, so the query would
        // quietly fail to come back. A page rebuilding while a search view is
        // open is ordinary.
        let controller = SharedSearchController::default();
        let anchor = || {
            SearchAnchor::new(1)
                .with_full_screen(false)
                .with_hint_text("Search")
                .with_controller(std::rc::Rc::clone(&controller))
        };
        let mut tree = staged(anchor());
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert_eq!(crate::theatre::modal_count(), 1);

        // The whole page again, from the top.
        let data = crate::media_query::MediaQueryData {
            size: SCREEN,
            ..crate::media_query::MediaQueryData::default()
        };
        tree.rebuild(crate::media_query::MediaQuery::new(
            data,
            overlay(stateful(anchor())),
        ));
        tree.rebuild_dirty();

        controller.borrow_mut().text = "carbonara".to_string();
        assert!(crate::theatre::dismiss_topmost_modal());
        tree.rebuild_dirty();

        let drawn = painted(&mut tree);
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "carbonara")),
            "the query still made it back: {drawn:?}"
        );
    }

    #[test]
    fn a_view_closed_with_nothing_typed_leaves_the_bar_as_it_was() {
        let mut tree = staged(an_anchor());
        tap(&mut tree, Offset::new(200.0, 28.0));
        assert!(crate::theatre::dismiss_topmost_modal());
        tree.rebuild_dirty();

        let drawn = painted(&mut tree);
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "Search")),
            "the hint is back, not an empty box"
        );
    }

    #[test]
    fn putting_a_query_into_a_bar_that_is_not_there_does_nothing() {
        // The sink is empty before the bar's first build, and stale after the
        // bar has gone. Neither is an error: upstream's controller is equally
        // quiet about being set while nothing is listening.
        let empty: FieldSink = std::rc::Rc::new(std::cell::RefCell::new(None));
        assert!(!put_query_back(&empty, "carbonara"));
    }

    #[test]
    fn a_query_put_back_leaves_the_caret_at_its_end() {
        // Counted in the units the platform counts in. A caret measured in
        // `chars` lands mid-surrogate on anything outside the basic plane, and
        // a caret that is not on a character boundary is not drawn at all.
        let value = crate::services::text_input::TextEditingValue::new("a\u{1F600}");
        assert_eq!(value.selection_base, 3, "one unit plus a surrogate pair");
        assert_eq!(value.selection_extent, 3);
        assert!(
            value.caret_bytes().is_some(),
            "and it is on a character boundary"
        );
    }

    #[test]
    fn typing_in_the_bar_reaches_the_controller() {
        // The wiring the bar draws nothing about: an anchor hands its bar a
        // handler that writes the query into the shared controller, and
        // whether it is attached is invisible on screen until the view opens.
        let controller = SharedSearchController::default();
        let anchor = SearchAnchor::new(1).with_controller(std::rc::Rc::clone(&controller));
        anchor.bar_for(&controller).notify_changed("carbonara");
        assert_eq!(controller.borrow().text, "carbonara");
    }

    #[test]
    fn typing_in_the_header_reaches_the_same_controller() {
        // The other end of the same thread. Without it the view would open
        // holding the bar's words and then keep whatever was typed into it to
        // itself -- and closing would put the old query back.
        let content = super::search_view_content_tests::content();
        content.header().notify_changed("pasta");
        assert_eq!(content.controller.borrow().text, "pasta");
    }

    #[test]
    fn an_anchor_without_a_controller_keeps_the_same_one_between_builds() {
        // Upstream's `_internalSearchController` is created once in
        // `initState`. One made in `build` instead would be written to by the
        // bar and thrown away before the view could read it, and the only
        // symptom would be a query that goes missing on opening.
        let state = SearchAnchorState::default();
        let anchor = SearchAnchor::new(1);
        let first = anchor.controller_for(&state);
        let second = anchor.controller_for(&state);
        assert!(
            std::rc::Rc::ptr_eq(&first, &second),
            "the same controller, not two"
        );

        let mine = SharedSearchController::default();
        let with_mine = SearchAnchor::new(1).with_controller(std::rc::Rc::clone(&mine));
        assert!(
            std::rc::Rc::ptr_eq(&with_mine.controller_for(&state), &mine),
            "and a caller's own beats the internal one"
        );
    }
}
