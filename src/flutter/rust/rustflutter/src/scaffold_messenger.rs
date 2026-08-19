//! Who shows a snack bar, and where -- a port of the six classes still
//! missing from upstream's `material/scaffold.dart`.
//!
//! A snack bar is not owned by the scaffold it appears in. It is owned by a
//! [`ScaffoldMessenger`] above the whole navigator, which is what lets one
//! survive a page change: the bar a reader is still reading does not vanish
//! because the code that raised it pushed a route.
//!
//! Two rules do most of the work here:
//!
//! * **the bars are a queue, not a stack.** Asking for a second while a first
//!   is up does not interrupt the first; it waits. A reader is never shown the
//!   end of one message and the start of another.
//! * **only the root scaffold of a nested set shows anything.** Scaffolds
//!   nest -- a page inside a tab inside a shell -- and without this a single
//!   `showSnackBar` would put the same bar on screen three times.
//!
//! ## What is not here
//!
//! The scaffold's own layout -- where the app bar, the body, the bottom bar
//! and the floating action button go -- is in [`crate::components::Scaffold`].
//! What this module adds is the messenger, the queues, and the geometry the
//! floating action button's animator reads.

use crate::components::MaterialBannerClosedReason;
use crate::engine::Rect;
use std::collections::VecDeque;

/// Upstream `SnackBarClosedReason` (`snack_bar.dart`): why a bar went away.
///
/// The distinction that matters to a caller is between the reader having
/// *acted* -- `Action`, `Dismiss`, `Swipe` -- and the bar merely having run
/// out of time or been replaced. An undo prompt that closes on `Timeout`
/// should commit; one that closes on `Action` already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnackBarClosedReason {
    /// The reader pressed the bar's action.
    Action,
    /// The reader dismissed it some other way.
    Dismiss,
    /// The reader swiped it away.
    Swipe,
    /// Hidden to make room, or by `hideCurrentSnackBar`.
    Hide,
    /// Removed outright, without the closing animation.
    Remove,
    /// Nobody did anything and its time ran out.
    Timeout,
}

/// Upstream `ScaffoldFeatureController`: the handle a caller gets back.
///
/// Upstream it is generic over the widget and the reason type, and carries a
/// `Future` that completes when the feature closes. Here the reason arrives
/// through [`Self::take_closed_reason`] instead, which is the same fact
/// without a runtime to await on.
#[derive(Debug)]
pub struct ScaffoldFeatureController<R> {
    closed_reason: Option<R>,
    /// Whether [`Self::close`] has been called on this controller.
    closed: bool,
}

impl<R: Copy> Default for ScaffoldFeatureController<R> {
    fn default() -> ScaffoldFeatureController<R> {
        ScaffoldFeatureController::new()
    }
}

impl<R: Copy> ScaffoldFeatureController<R> {
    pub fn new() -> ScaffoldFeatureController<R> {
        ScaffoldFeatureController {
            closed_reason: None,
            closed: false,
        }
    }

    /// Upstream's `close`.
    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Upstream's `_completer.complete(reason)`, which is **guarded**: a
    /// completer completed twice throws, so the reason a feature closed is the
    /// *first* one given. A bar swiped away as its timer expires closed
    /// because it was swiped.
    pub fn complete(&mut self, reason: R) {
        if self.closed_reason.is_none() {
            self.closed_reason = Some(reason);
        }
    }

    /// Upstream's `closed` future, once it has resolved.
    pub fn closed_reason(&self) -> Option<R> {
        self.closed_reason
    }

    pub fn take_closed_reason(&mut self) -> Option<R> {
        self.closed_reason.take()
    }
}

/// Upstream `PersistentBottomSheetController`: the handle for a sheet that is
/// part of the page rather than over it.
///
/// Upstream it adds exactly one field to [`ScaffoldFeatureController`], and it
/// is the interesting one: whether the sheet put an entry on the route's local
/// history. A sheet that did is closed by the system back gesture; one that
/// did not stays until the code that raised it closes it.
#[derive(Debug)]
pub struct PersistentBottomSheetController {
    pub base: ScaffoldFeatureController<()>,
    is_local_history_entry: bool,
}

impl PersistentBottomSheetController {
    pub fn new(is_local_history_entry: bool) -> PersistentBottomSheetController {
        PersistentBottomSheetController {
            base: ScaffoldFeatureController::new(),
            is_local_history_entry,
        }
    }

    /// Whether the back gesture closes this sheet.
    pub fn is_local_history_entry(&self) -> bool {
        self.is_local_history_entry
    }

    pub fn close(&mut self) {
        self.base.close();
    }

    pub fn is_closed(&self) -> bool {
        self.base.is_closed()
    }
}

/// Upstream `ScaffoldGeometry`: where the scaffold put the two things a
/// floating action button has to avoid.
///
/// Upstream's `Scaffold.geometryOf` refuses to be read outside the paint
/// phase, and the error message says why: the geometry is computed during the
/// animation and layout phases *before* painting, so a reader asking earlier
/// would get the previous frame's answer without knowing it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScaffoldGeometry {
    /// The top of the bottom navigation bar, if there is one.
    pub bottom_navigation_bar_top: Option<f32>,
    /// Where the floating action button is, if there is one.
    pub floating_action_button_area: Option<Rect>,
}

impl ScaffoldGeometry {
    pub fn new() -> ScaffoldGeometry {
        ScaffoldGeometry::default()
    }

    pub fn with_bottom_navigation_bar_top(mut self, top: f32) -> Self {
        self.bottom_navigation_bar_top = Some(top);
        self
    }

    pub fn with_floating_action_button_area(mut self, area: Rect) -> Self {
        self.floating_action_button_area = Some(area);
        self
    }

    /// Upstream's `copyWith`, whose `??` means a `None` argument keeps what
    /// was there rather than clearing it.
    pub fn copy_with(
        &self,
        bottom_navigation_bar_top: Option<f32>,
        floating_action_button_area: Option<Rect>,
    ) -> ScaffoldGeometry {
        ScaffoldGeometry {
            bottom_navigation_bar_top: bottom_navigation_bar_top.or(self.bottom_navigation_bar_top),
            floating_action_button_area: floating_action_button_area
                .or(self.floating_action_button_area),
        }
    }

    /// Upstream's `_scaleFloatingActionButton`.
    ///
    /// **A fully scaled-away button has no area at all**, rather than an area
    /// of zero size. The difference matters to whatever is reading the
    /// geometry to avoid the button: with `None` it stops avoiding it, and
    /// with a zero-size rect at the button's centre it would go on treating
    /// that point as occupied.
    pub fn scale_floating_action_button(&self, scale_factor: f32) -> ScaffoldGeometry {
        if scale_factor == 1.0 {
            return *self;
        }
        if scale_factor == 0.0 {
            return ScaffoldGeometry {
                bottom_navigation_bar_top: self.bottom_navigation_bar_top,
                floating_action_button_area: None,
            };
        }
        let Some(area) = self.floating_action_button_area else {
            return *self;
        };
        let centre_x = (area.left + area.right) / 2.0;
        let centre_y = (area.top + area.bottom) / 2.0;
        let lerp = |from: f32, to: f32| from + (to - from) * scale_factor;
        self.copy_with(
            None,
            Some(Rect::ltrb(
                lerp(centre_x, area.left),
                lerp(centre_y, area.top),
                lerp(centre_x, area.right),
                lerp(centre_y, area.bottom),
            )),
        )
    }
}

/// One scaffold, as the messenger knows it.
///
/// Upstream this is a `ScaffoldState` and the messenger keeps a
/// `LinkedHashSet` of them -- linked because registration order is the order
/// they were built in, which is what makes "the first one" meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaffoldRegistration {
    pub id: u64,
    /// The scaffold this one is nested inside, if any.
    pub parent: Option<u64>,
}

/// Upstream `ScaffoldState`: what a scaffold shows and can be asked to show.
///
/// The layout half lives in [`crate::components::Scaffold`]; this is the half
/// the messenger drives, plus the drawer and bottom-sheet API a caller reaches
/// through `Scaffold.of`.
#[derive(Debug, Default)]
pub struct ScaffoldState {
    pub id: u64,
    pub parent: Option<u64>,
    /// The snack bar this scaffold is currently showing, by the queue position
    /// the messenger last handed it.
    showing_snack_bar: bool,
    showing_material_banner: bool,
    drawer_open: bool,
    end_drawer_open: bool,
}

impl ScaffoldState {
    pub fn new(id: u64) -> ScaffoldState {
        ScaffoldState {
            id,
            parent: None,
            showing_snack_bar: false,
            showing_material_banner: false,
            drawer_open: false,
            end_drawer_open: false,
        }
    }

    pub fn with_parent(mut self, parent: u64) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn registration(&self) -> ScaffoldRegistration {
        ScaffoldRegistration {
            id: self.id,
            parent: self.parent,
        }
    }

    pub fn is_showing_snack_bar(&self) -> bool {
        self.showing_snack_bar
    }

    pub fn is_showing_material_banner(&self) -> bool {
        self.showing_material_banner
    }

    /// Upstream's `_updateSnackBar`.
    pub fn set_showing_snack_bar(&mut self, showing: bool) {
        self.showing_snack_bar = showing;
    }

    /// Upstream's `_updateMaterialBanner`.
    pub fn set_showing_material_banner(&mut self, showing: bool) {
        self.showing_material_banner = showing;
    }

    /// Upstream's `openDrawer`.
    pub fn open_drawer(&mut self) {
        self.drawer_open = true;
    }

    /// Upstream's `openEndDrawer`.
    pub fn open_end_drawer(&mut self) {
        self.end_drawer_open = true;
    }

    pub fn is_drawer_open(&self) -> bool {
        self.drawer_open
    }

    pub fn is_end_drawer_open(&self) -> bool {
        self.end_drawer_open
    }

    pub fn close_drawer(&mut self) {
        self.drawer_open = false;
    }

    pub fn close_end_drawer(&mut self) {
        self.end_drawer_open = false;
    }
}

/// Upstream `ScaffoldMessenger`: the widget that owns the snack bars.
#[derive(Debug, Default)]
pub struct ScaffoldMessenger;

impl ScaffoldMessenger {
    pub fn new() -> ScaffoldMessenger {
        ScaffoldMessenger
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> ScaffoldMessengerState {
        ScaffoldMessengerState::new()
    }
}

/// Upstream `ScaffoldMessengerState`: the two queues and the scaffolds they go
/// to.
#[derive(Debug, Default)]
pub struct ScaffoldMessengerState {
    scaffolds: Vec<ScaffoldRegistration>,
    snack_bars: VecDeque<ScaffoldFeatureController<SnackBarClosedReason>>,
    material_banners: VecDeque<ScaffoldFeatureController<MaterialBannerClosedReason>>,
    /// Upstream's `_accessibleNavigation`, from the ambient `MediaQuery`.
    ///
    /// With a screen reader on, hiding a bar skips its closing animation
    /// entirely: someone listening rather than watching gains nothing from
    /// waiting through it, and the next announcement should not be held up.
    pub accessible_navigation: bool,
}

impl ScaffoldMessengerState {
    pub fn new() -> ScaffoldMessengerState {
        ScaffoldMessengerState::default()
    }

    /// Upstream's `_register`.
    pub fn register(&mut self, scaffold: ScaffoldRegistration) {
        if !self.scaffolds.iter().any(|held| held.id == scaffold.id) {
            self.scaffolds.push(scaffold);
        }
    }

    /// Upstream's `_unregister`, which asserts the scaffold really was
    /// registered -- a scaffold should only ever be removed once.
    pub fn unregister(&mut self, id: u64) {
        let at = self.scaffolds.iter().position(|held| held.id == id);
        debug_assert!(at.is_some(), "a scaffold was unregistered twice");
        if let Some(at) = at {
            self.scaffolds.remove(at);
        }
    }

    pub fn scaffolds(&self) -> &[ScaffoldRegistration] {
        &self.scaffolds
    }

    /// Upstream's `_isRoot`.
    ///
    /// Not "has no parent" but "**has no parent this messenger knows about**".
    /// A scaffold inside another scaffold that belongs to a *different*
    /// messenger is a root as far as this one is concerned, which is what
    /// makes a nested messenger able to own its own bars.
    pub fn is_root(&self, scaffold: &ScaffoldRegistration) -> bool {
        match scaffold.parent {
            None => true,
            Some(parent) => !self.scaffolds.iter().any(|held| held.id == parent),
        }
    }

    /// Which scaffolds a bar should actually appear in -- upstream's
    /// `_updateScaffolds`.
    pub fn presenting_scaffolds(&self) -> Vec<u64> {
        self.scaffolds
            .iter()
            .filter(|scaffold| self.is_root(scaffold))
            .map(|scaffold| scaffold.id)
            .collect()
    }

    pub fn snack_bars(&self) -> usize {
        self.snack_bars.len()
    }

    pub fn material_banners(&self) -> usize {
        self.material_banners.len()
    }

    /// Upstream's `showSnackBar`.
    ///
    /// **The new bar goes to the back of the queue.** Asking for a second
    /// while a first is up does not interrupt it: a reader is never shown the
    /// end of one message and the start of another.
    ///
    /// Upstream asserts there is a descendant scaffold to present to, because
    /// a bar shown to nobody would sit in the queue for ever and block every
    /// later one.
    pub fn show_snack_bar(&mut self) {
        debug_assert!(
            !self.scaffolds.is_empty(),
            "showSnackBar was called with no descendant Scaffolds to present to"
        );
        self.snack_bars.push_back(ScaffoldFeatureController::new());
    }

    /// Upstream's `showMaterialBanner`.
    pub fn show_material_banner(&mut self) {
        debug_assert!(!self.scaffolds.is_empty());
        self.material_banners
            .push_back(ScaffoldFeatureController::new());
    }

    /// Upstream's `hideCurrentSnackBar`: close it the polite way.
    ///
    /// Returns whether the closing animation should be played. With accessible
    /// navigation on it is skipped and the bar is gone at once.
    pub fn hide_current_snack_bar(&mut self, reason: SnackBarClosedReason) -> bool {
        let accessible = self.accessible_navigation;
        let Some(current) = self.snack_bars.front_mut() else {
            return false;
        };
        current.complete(reason);
        if accessible {
            self.snack_bars.pop_front();
            false
        } else {
            true
        }
    }

    /// Upstream's `removeCurrentSnackBar`: gone now, no animation.
    pub fn remove_current_snack_bar(&mut self, reason: SnackBarClosedReason) {
        let Some(mut current) = self.snack_bars.pop_front() else {
            return;
        };
        current.complete(reason);
    }

    /// Upstream's `clearSnackBars`.
    ///
    /// **Keeps the one on screen and drops the rest.** Cutting the current bar
    /// off mid-word would leave the reader with half a message; what a caller
    /// clearing the queue wants is that nothing *further* appears.
    pub fn clear_snack_bars(&mut self) -> bool {
        if self.snack_bars.is_empty() {
            return false;
        }
        while self.snack_bars.len() > 1 {
            self.snack_bars.pop_back();
        }
        self.hide_current_snack_bar(SnackBarClosedReason::Hide)
    }

    /// Upstream's `clearMaterialBanners`.
    pub fn clear_material_banners(&mut self) -> bool {
        if self.material_banners.is_empty() {
            return false;
        }
        while self.material_banners.len() > 1 {
            self.material_banners.pop_back();
        }
        self.hide_current_material_banner(MaterialBannerClosedReason::Hide)
    }

    /// Upstream's `hideCurrentMaterialBanner`.
    pub fn hide_current_material_banner(&mut self, reason: MaterialBannerClosedReason) -> bool {
        let accessible = self.accessible_navigation;
        let Some(current) = self.material_banners.front_mut() else {
            return false;
        };
        current.complete(reason);
        if accessible {
            self.material_banners.pop_front();
            false
        } else {
            true
        }
    }

    /// Upstream's `removeCurrentMaterialBanner`.
    pub fn remove_current_material_banner(&mut self, reason: MaterialBannerClosedReason) {
        let Some(mut current) = self.material_banners.pop_front() else {
            return;
        };
        current.complete(reason);
    }

    /// Upstream's `_handleSnackBarStatusChanged` for `dismissed`: the closing
    /// animation finished, so this bar leaves and **the next one starts at
    /// once**.
    ///
    /// Returns whether there is another bar to show.
    pub fn snack_bar_dismissed(&mut self) -> bool {
        debug_assert!(!self.snack_bars.is_empty());
        self.snack_bars.pop_front();
        !self.snack_bars.is_empty()
    }

    /// The same for banners.
    pub fn material_banner_dismissed(&mut self) -> bool {
        debug_assert!(!self.material_banners.is_empty());
        self.material_banners.pop_front();
        !self.material_banners.is_empty()
    }

    /// The reason the bar at the front of the queue closed, if it has.
    pub fn current_snack_bar_reason(&self) -> Option<SnackBarClosedReason> {
        self.snack_bars
            .front()
            .and_then(|controller| controller.closed_reason())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_one_scaffold() -> ScaffoldMessengerState {
        let mut messenger = ScaffoldMessenger::new().create_state();
        messenger.register(ScaffoldState::new(1).registration());
        messenger
    }

    #[test]
    fn the_bars_are_a_queue_and_not_a_stack() {
        // Asking for a second while a first is up does not interrupt it. A
        // reader is never shown the end of one message and the start of
        // another.
        let mut messenger = with_one_scaffold();
        messenger.show_snack_bar();
        messenger.show_snack_bar();
        messenger.show_snack_bar();
        assert_eq!(messenger.snack_bars(), 3);

        // The first one finishes, and the next takes its place.
        assert!(messenger.snack_bar_dismissed());
        assert_eq!(messenger.snack_bars(), 2);
        assert!(messenger.snack_bar_dismissed());
        assert!(!messenger.snack_bar_dismissed(), "and then there are none");
        assert_eq!(messenger.snack_bars(), 0);
    }

    #[test]
    fn clearing_keeps_the_one_on_screen_and_drops_the_rest() {
        // Cutting the current bar off mid-word would leave the reader with
        // half a message; what a caller clearing the queue wants is that
        // nothing *further* appears.
        let mut messenger = with_one_scaffold();
        messenger.show_snack_bar();
        messenger.show_snack_bar();
        messenger.show_snack_bar();

        assert!(messenger.clear_snack_bars(), "the current one animates out");
        assert_eq!(messenger.snack_bars(), 1, "only the one being read");
        assert_eq!(
            messenger.current_snack_bar_reason(),
            Some(SnackBarClosedReason::Hide)
        );

        // With nothing showing there is nothing to clear.
        let mut empty = with_one_scaffold();
        assert!(!empty.clear_snack_bars());
    }

    #[test]
    fn a_screen_reader_does_not_have_to_wait_through_the_closing_animation() {
        // Someone listening rather than watching gains nothing from it, and
        // the next announcement should not be held up.
        let mut watching = with_one_scaffold();
        watching.show_snack_bar();
        assert!(
            watching.hide_current_snack_bar(SnackBarClosedReason::Hide),
            "play the animation"
        );
        assert_eq!(watching.snack_bars(), 1, "still on screen, going away");

        let mut listening = with_one_scaffold();
        listening.accessible_navigation = true;
        listening.show_snack_bar();
        assert!(
            !listening.hide_current_snack_bar(SnackBarClosedReason::Hide),
            "no animation to play"
        );
        assert_eq!(listening.snack_bars(), 0, "gone at once");
    }

    #[test]
    fn removing_is_immediate_where_hiding_is_polite() {
        let mut messenger = with_one_scaffold();
        messenger.show_snack_bar();
        messenger.show_snack_bar();
        messenger.remove_current_snack_bar(SnackBarClosedReason::Remove);
        assert_eq!(messenger.snack_bars(), 1, "gone without animating");

        // And removing when there is nothing is not an error.
        let mut empty = with_one_scaffold();
        empty.remove_current_snack_bar(SnackBarClosedReason::Remove);
        assert_eq!(empty.snack_bars(), 0);
        assert!(!empty.hide_current_snack_bar(SnackBarClosedReason::Hide));
    }

    #[test]
    fn the_reason_a_bar_closed_is_the_first_one_given() {
        // Upstream guards the completer, because completing twice throws. A
        // bar swiped away as its timer expires closed because it was swiped,
        // and an undo prompt reading the reason must not be told otherwise.
        let mut controller: ScaffoldFeatureController<SnackBarClosedReason> =
            ScaffoldFeatureController::new();
        controller.complete(SnackBarClosedReason::Swipe);
        controller.complete(SnackBarClosedReason::Timeout);
        assert_eq!(
            controller.closed_reason(),
            Some(SnackBarClosedReason::Swipe)
        );
        assert!(!controller.is_closed(), "closing is the other thing");
        controller.close();
        assert!(controller.is_closed());
    }

    #[test]
    fn only_the_root_scaffold_of_a_nested_set_shows_anything() {
        // Scaffolds nest -- a page inside a tab inside a shell -- and without
        // this one showSnackBar would put the same bar on screen three times.
        let mut messenger = ScaffoldMessenger::new().create_state();
        messenger.register(ScaffoldState::new(1).registration());
        messenger.register(ScaffoldState::new(2).with_parent(1).registration());
        messenger.register(ScaffoldState::new(3).with_parent(2).registration());
        assert_eq!(messenger.presenting_scaffolds(), vec![1]);
    }

    #[test]
    fn a_scaffold_whose_parent_belongs_to_another_messenger_is_a_root_here() {
        // "No parent this messenger knows about", not "no parent" -- which is
        // what lets a nested messenger own its own bars.
        let mut inner = ScaffoldMessenger::new().create_state();
        inner.register(ScaffoldState::new(7).with_parent(1).registration());
        assert_eq!(
            inner.presenting_scaffolds(),
            vec![7],
            "scaffold 1 is somebody else's"
        );
    }

    #[test]
    fn two_sibling_scaffolds_both_show_the_bar() {
        let mut messenger = ScaffoldMessenger::new().create_state();
        messenger.register(ScaffoldState::new(1).registration());
        messenger.register(ScaffoldState::new(2).registration());
        assert_eq!(messenger.presenting_scaffolds(), vec![1, 2]);
    }

    #[test]
    fn a_scaffold_that_goes_away_stops_being_presented_to() {
        let mut messenger = ScaffoldMessenger::new().create_state();
        messenger.register(ScaffoldState::new(1).registration());
        messenger.register(ScaffoldState::new(2).with_parent(1).registration());
        assert_eq!(messenger.presenting_scaffolds(), vec![1]);

        // With the outer one gone, the inner one becomes the root.
        messenger.unregister(1);
        assert_eq!(messenger.presenting_scaffolds(), vec![2]);
        assert_eq!(messenger.scaffolds().len(), 1);
    }

    #[test]
    fn banners_have_their_own_queue_beside_the_bars() {
        // A banner and a snack bar can be up at the same time; they are
        // different parts of the screen and different queues.
        let mut messenger = with_one_scaffold();
        messenger.show_snack_bar();
        messenger.show_material_banner();
        messenger.show_material_banner();
        assert_eq!(messenger.snack_bars(), 1);
        assert_eq!(messenger.material_banners(), 2);

        assert!(messenger.clear_material_banners());
        assert_eq!(messenger.material_banners(), 1);
        assert_eq!(messenger.snack_bars(), 1, "the bar is untouched");

        messenger.remove_current_material_banner(MaterialBannerClosedReason::Remove);
        assert_eq!(messenger.material_banners(), 0);
        assert_eq!(messenger.snack_bars(), 1, "and still untouched");
    }

    #[test]
    fn a_button_scaled_away_has_no_area_rather_than_an_area_of_nothing() {
        // The difference matters to whatever is reading the geometry to avoid
        // the button: with no area it stops avoiding it, and with a zero-size
        // rect at its centre it would go on treating that point as occupied.
        let geometry = ScaffoldGeometry::new()
            .with_bottom_navigation_bar_top(700.0)
            .with_floating_action_button_area(Rect::ltrb(100.0, 600.0, 156.0, 656.0));

        let gone = geometry.scale_floating_action_button(0.0);
        assert_eq!(gone.floating_action_button_area, None);
        assert_eq!(
            gone.bottom_navigation_bar_top,
            Some(700.0),
            "and the bar is still where it was"
        );

        let whole = geometry.scale_floating_action_button(1.0);
        assert_eq!(whole, geometry, "untouched at full size");
    }

    #[test]
    fn a_half_scaled_button_shrinks_towards_its_own_centre() {
        // Not towards the origin, which is what makes the shrink read as the
        // button receding rather than sliding away.
        let geometry = ScaffoldGeometry::new()
            .with_floating_action_button_area(Rect::ltrb(100.0, 600.0, 156.0, 656.0));
        let half = geometry
            .scale_floating_action_button(0.5)
            .floating_action_button_area
            .expect("still there");
        assert_eq!(half.width(), 28.0);
        assert_eq!(half.height(), 28.0);
        assert_eq!((half.left + half.right) / 2.0, 128.0, "same centre");
        assert_eq!((half.top + half.bottom) / 2.0, 628.0);
    }

    #[test]
    fn copying_the_geometry_keeps_what_it_is_not_given() {
        let geometry = ScaffoldGeometry::new()
            .with_bottom_navigation_bar_top(700.0)
            .with_floating_action_button_area(Rect::ltrb(0.0, 0.0, 10.0, 10.0));
        let same = geometry.copy_with(None, None);
        assert_eq!(same, geometry);
        let moved = geometry.copy_with(Some(650.0), None);
        assert_eq!(moved.bottom_navigation_bar_top, Some(650.0));
        assert_eq!(
            moved.floating_action_button_area,
            geometry.floating_action_button_area
        );
    }

    #[test]
    fn a_persistent_sheet_remembers_whether_the_back_gesture_closes_it() {
        // Upstream's one added field, and the interesting one: a sheet that
        // put an entry on the route's local history is closed by the system
        // back gesture; one that did not stays until the code that raised it
        // closes it.
        let mut with_history = PersistentBottomSheetController::new(true);
        assert!(with_history.is_local_history_entry());
        assert!(!with_history.is_closed());
        with_history.close();
        assert!(with_history.is_closed());

        let without = PersistentBottomSheetController::new(false);
        assert!(!without.is_local_history_entry());
    }

    #[test]
    fn a_scaffold_tracks_its_own_drawers_and_what_it_has_been_told_to_show() {
        let mut scaffold = ScaffoldState::new(1);
        assert!(!scaffold.is_drawer_open() && !scaffold.is_end_drawer_open());
        scaffold.open_drawer();
        assert!(scaffold.is_drawer_open());
        assert!(!scaffold.is_end_drawer_open(), "the two are separate");
        scaffold.open_end_drawer();
        assert!(scaffold.is_end_drawer_open());
        scaffold.close_drawer();
        assert!(!scaffold.is_drawer_open());
        assert!(scaffold.is_end_drawer_open(), "still separate");

        assert!(!scaffold.is_showing_snack_bar());
        scaffold.set_showing_snack_bar(true);
        assert!(scaffold.is_showing_snack_bar());
        assert!(!scaffold.is_showing_material_banner());
        scaffold.set_showing_material_banner(true);
        assert!(scaffold.is_showing_material_banner());
    }
}
