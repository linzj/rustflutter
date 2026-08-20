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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchBar {
    pub hint_text: Option<String>,
    pub has_leading: bool,
    pub has_trailing: bool,
    /// Whether this bar is the thing an anchor taps through.
    pub is_anchor_child: bool,
}

impl SearchBar {
    pub fn new() -> SearchBar {
        SearchBar::default()
    }

    /// Whether the anchor has to wire up a tap itself.
    pub fn anchor_supplies_tap_handling(builder_returns_tappable: bool) -> bool {
        !builder_returns_tappable
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
}

impl SearchDelegate {
    pub fn new() -> SearchDelegate {
        SearchDelegate::default()
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
