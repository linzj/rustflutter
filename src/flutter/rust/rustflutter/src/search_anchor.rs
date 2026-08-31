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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchAnchor {
    /// `None` means "decide from the platform". Upstream shows the view full
    /// screen on mobile only.
    pub is_full_screen: Option<bool>,
}

impl SearchAnchor {
    pub fn new() -> SearchAnchor {
        SearchAnchor {
            is_full_screen: None,
        }
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
}

impl Default for SearchAnchor {
    fn default() -> Self {
        SearchAnchor::new()
    }
}

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
}

impl std::fmt::Debug for SearchBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchBar")
            .field("id", &self.id)
            .field("hint_text", &self.hint_text)
            .field("has_leading", &self.has_leading)
            .field("has_trailing", &self.has_trailing)
            .field("is_anchor_child", &self.is_anchor_child)
            .field("enabled", &self.enabled)
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
            && self.has_leading == other.has_leading
            && self.has_trailing == other.has_trailing
            && self.is_anchor_child == other.is_anchor_child
            && self.enabled == other.enabled
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
            has_leading: false,
            has_trailing: false,
            is_anchor_child: false,
            enabled: true,
            leading: None,
            trailing: None,
            on_changed: None,
            on_submitted: None,
            on_tap: None,
        }
    }

    pub fn with_hint_text(mut self, hint: impl Into<String>) -> Self {
        self.hint_text = Some(hint.into());
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
        crate::component_themes::ResolvedSearchBar::of(context, states)
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
        let anchor = SearchAnchor::new();
        assert!(anchor.resolve_full_screen(ScrollPlatform::Android));
        assert!(anchor.resolve_full_screen(ScrollPlatform::IOS));
        assert!(!anchor.resolve_full_screen(ScrollPlatform::MacOS));
        assert!(!anchor.resolve_full_screen(ScrollPlatform::Windows));
    }

    #[test]
    fn saying_so_outright_overrules_the_platform() {
        assert!(
            SearchAnchor::new()
                .with_full_screen(true)
                .resolve_full_screen(ScrollPlatform::Windows)
        );
        assert!(
            !SearchAnchor::new()
                .with_full_screen(false)
                .resolve_full_screen(ScrollPlatform::Android)
        );
    }

    #[test]
    fn the_view_attached_to_something_goes_when_that_something_moves() {
        // A docked view is positioned against an anchor whose place on screen
        // just changed and has nowhere to be. A full-screen one was never
        // anchored, so rotating only lays it out again.
        let anchor = SearchAnchor::new();
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
