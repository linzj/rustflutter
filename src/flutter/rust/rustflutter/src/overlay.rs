//! The overlay -- a port of upstream's `widgets/overlay.dart`.
//!
//! An overlay is a stack that anything in the tree can push onto from
//! anywhere. Routes live in one, so do dialogs, tooltips, drag feedback and
//! selection handles. What makes it more than a `Stack` is that entries are
//! **inserted by code rather than declared by a parent**, so the thing that
//! wants a tooltip does not have to be the thing that lays the screen out.
//!
//! Two ideas carry the file.
//!
//! **Opacity is a build-time optimisation, not a paint one.** An entry that
//! declares itself opaque causes the overlay to *stop building* everything
//! below it -- not to build them and paint over them. That is why
//! `maintainState` exists as a separate opt-out: a route in the background
//! needs its state kept even though nobody can see it, because a future it
//! promised has to be able to complete.
//!
//! **An [`OverlayPortal`] builds its overlay child where it stands and shows
//! it somewhere else.** The child is built in the portal's position in the
//! tree, so it inherits the theme, the directionality and everything else the
//! portal can see; it is only *rendered* in the overlay. Building it in the
//! overlay instead would give a tooltip the overlay's inherited context rather
//! than the button's.
//!
//! ## What is not here
//!
//! The `_Theater` render object, the layout that lets an entry size the
//! overlay, and the element-level plumbing that moves a portal's child into
//! another subtree are absent. What is ported is the entry list and its
//! ordering rules, the onstage/offstage decision, and the portal's z-ordering.

/// Upstream `OverlayEntry`: one thing in the overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayEntry {
    pub id: u64,
    opaque: bool,
    maintain_state: bool,
    /// Upstream's `canSizeOverlay`.
    ///
    /// It matters only when the overlay's own constraints are unbounded, which
    /// is the one case where a stack cannot decide its size by itself. Then
    /// the overlay picks the **topmost non-positioned** entry that offered,
    /// lays that one out, and forces the rest to match. An entry that offers
    /// must therefore cope with unbounded constraints -- it is being asked the
    /// question the overlay could not answer.
    pub can_size_overlay: bool,
    /// Whether this entry is currently in an overlay.
    inserted: bool,
    /// Whether the widget built from it is in the tree. Upstream's `mounted`,
    /// which is **not** the same as inserted: an entry hidden behind an opaque
    /// one is inserted and not mounted.
    mounted: bool,
    disposed: bool,
    notifications: usize,
}

impl OverlayEntry {
    pub fn new(id: u64) -> OverlayEntry {
        OverlayEntry {
            id,
            opaque: false,
            maintain_state: false,
            can_size_overlay: false,
            inserted: false,
            mounted: false,
            disposed: false,
            notifications: 0,
        }
    }

    pub fn with_opaque(mut self, opaque: bool) -> Self {
        self.opaque = opaque;
        self
    }

    /// Upstream's `maintainState`, whose documentation carries an unusually
    /// direct warning: an entry kept alive behind an opaque one that keeps
    /// calling `setState` will "drain the user's battery unnecessarily". It is
    /// the right warning -- nothing on screen changes, so nothing tells the
    /// author their code is running.
    pub fn with_maintain_state(mut self, maintain: bool) -> Self {
        self.maintain_state = maintain;
        self
    }

    pub fn with_can_size_overlay(mut self, can_size: bool) -> Self {
        self.can_size_overlay = can_size;
        self
    }

    pub fn opaque(&self) -> bool {
        self.opaque
    }

    pub fn maintain_state(&self) -> bool {
        self.maintain_state
    }

    pub fn is_inserted(&self) -> bool {
        self.inserted
    }

    /// Upstream's `mounted`, which the entry **notifies about**. A caller
    /// waiting to position something against an entry's widget needs to know
    /// when that widget exists, and inserting the entry is not that moment.
    pub fn mounted(&self) -> bool {
        self.mounted
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn set_mounted(&mut self, mounted: bool) {
        if self.mounted == mounted {
            return;
        }
        self.mounted = mounted;
        self.notifications += 1;
    }

    /// Upstream's `opaque=` setter, which tells the overlay so it can rebuild.
    /// Turning opacity on hides everything below; the overlay cannot notice
    /// that on its own.
    pub fn set_opaque(&mut self, opaque: bool) -> bool {
        debug_assert!(!self.disposed);
        if self.opaque == opaque {
            return false;
        }
        self.opaque = opaque;
        true
    }

    pub fn set_maintain_state(&mut self, maintain: bool) -> bool {
        debug_assert!(!self.disposed);
        if self.maintain_state == maintain {
            return false;
        }
        self.maintain_state = maintain;
        true
    }

    /// Upstream's `dispose`, which **asserts the entry was removed first**.
    /// Disposing an inserted entry would leave the overlay holding something
    /// that has already given up its resources.
    pub fn dispose(&mut self) {
        debug_assert!(!self.disposed, "an OverlayEntry is disposed once");
        debug_assert!(
            !self.inserted,
            "an OverlayEntry must be removed from the Overlay before dispose"
        );
        self.disposed = true;
    }
}

/// Where an entry goes when it is inserted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InsertPosition {
    /// Upstream's default: on top of everything.
    #[default]
    Top,
    /// Just below the named entry.
    Below(u64),
    /// Just above the named entry.
    Above(u64),
}

/// Why an insertion was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayError {
    /// Upstream asserts `above == null || below == null`.
    BothAboveAndBelow,
    /// The anchor named by `above` or `below` is not in this overlay.
    AnchorNotPresent,
    /// The entry is already in an overlay -- this one or another.
    AlreadyInserted,
    /// The same entry was named twice in one call.
    Duplicated,
}

/// Upstream `Overlay`: the widget's configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overlay {
    /// Upstream's `alwaysSizeToContent`.
    pub always_size_to_content: bool,
}

impl Overlay {
    pub fn new() -> Overlay {
        Overlay::default()
    }
}

/// Which entries the overlay builds this frame, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnstageEntry {
    pub id: u64,
    /// Upstream builds offstage entries with `tickerEnabled: false`.
    ///
    /// The reason is worth stating: a route behind an opaque one keeps its
    /// state, but its animations must not keep ticking. An invisible thing
    /// animating is work with no observer, and on a stack of ten routes it is
    /// ten times the work.
    pub ticker_enabled: bool,
}

/// Upstream `OverlayState`: the entry list and what it builds.
#[derive(Debug, Default)]
pub struct OverlayState {
    entries: Vec<OverlayEntry>,
    rebuilds: usize,
    mounted: bool,
}

impl OverlayState {
    pub fn new() -> OverlayState {
        OverlayState {
            entries: Vec::new(),
            rebuilds: 0,
            mounted: true,
        }
    }

    pub fn entries(&self) -> &[OverlayEntry] {
        &self.entries
    }

    pub fn entry_ids(&self) -> Vec<u64> {
        self.entries.iter().map(|entry| entry.id).collect()
    }

    pub fn rebuilds(&self) -> usize {
        self.rebuilds
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted
    }

    pub fn unmount(&mut self) {
        self.mounted = false;
    }

    /// Upstream's `_insertionIndex`.
    ///
    /// Note the asymmetry, which is not arbitrary: `below` returns the
    /// anchor's own index and `above` returns one past it. Both put the new
    /// entry on the side its name says, given that later in the list means
    /// higher in the stack.
    fn insertion_index(&self, position: InsertPosition) -> Result<usize, OverlayError> {
        match position {
            InsertPosition::Top => Ok(self.entries.len()),
            InsertPosition::Below(anchor) => {
                self.index_of(anchor).ok_or(OverlayError::AnchorNotPresent)
            }
            InsertPosition::Above(anchor) => self
                .index_of(anchor)
                .map(|at| at + 1)
                .ok_or(OverlayError::AnchorNotPresent),
        }
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    /// Upstream's `insert`.
    pub fn insert(
        &mut self,
        mut entry: OverlayEntry,
        position: InsertPosition,
    ) -> Result<(), OverlayError> {
        if entry.inserted {
            return Err(OverlayError::AlreadyInserted);
        }
        let at = self.insertion_index(position)?;
        entry.inserted = true;
        self.entries.insert(at, entry);
        self.rebuilds += 1;
        Ok(())
    }

    /// Upstream's `insertAll`, which **returns early on an empty iterable**
    /// rather than rebuilding for nothing.
    pub fn insert_all(
        &mut self,
        entries: Vec<OverlayEntry>,
        position: InsertPosition,
    ) -> Result<(), OverlayError> {
        if entries.is_empty() {
            return Ok(());
        }
        if entries.iter().any(|entry| entry.inserted) {
            return Err(OverlayError::AlreadyInserted);
        }
        let at = self.insertion_index(position)?;
        for (offset, mut entry) in entries.into_iter().enumerate() {
            entry.inserted = true;
            self.entries.insert(at + offset, entry);
        }
        self.rebuilds += 1;
        Ok(())
    }

    /// Upstream's `rearrange`, whose contract has a part that is easy to miss:
    /// **entries already in the overlay that are not named in the new list are
    /// kept**, and reinserted at the given position. It is a reordering of the
    /// named ones, not a replacement of the list.
    ///
    /// It also returns early when the new order equals the old, so a caller
    /// that recomputes an unchanged order does not cost a rebuild.
    pub fn rearrange(
        &mut self,
        new_order: &[u64],
        position: InsertPosition,
    ) -> Result<(), OverlayError> {
        if new_order.is_empty() {
            return Ok(());
        }
        let mut seen = Vec::new();
        for id in new_order {
            if seen.contains(id) {
                return Err(OverlayError::Duplicated);
            }
            seen.push(*id);
        }
        if self.entry_ids() == new_order {
            return Ok(());
        }
        let at = self.insertion_index(position)?;
        let mut leftovers: Vec<OverlayEntry> = Vec::new();
        let mut named: Vec<OverlayEntry> = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            if new_order.contains(&entry.id) {
                named.push(entry);
            } else {
                leftovers.push(entry);
            }
        }
        named.sort_by_key(|entry| {
            new_order
                .iter()
                .position(|id| *id == entry.id)
                .unwrap_or(usize::MAX)
        });
        self.entries = named;
        let at = at.min(self.entries.len());
        for (offset, entry) in leftovers.into_iter().enumerate() {
            self.entries.insert(at + offset, entry);
        }
        self.rebuilds += 1;
        Ok(())
    }

    /// Upstream's `OverlayEntry.remove` seen from this side.
    ///
    /// `tree_is_locked` is upstream's `SchedulerPhase.persistentCallbacks`
    /// check: a removal during a build cannot mark the overlay dirty, so it is
    /// deferred to a post-frame callback. Upstream's own documentation spells
    /// out the consequence -- remove after the overlay has rebuilt this frame
    /// and the screen does not change until the next one, "many milliseconds
    /// later".
    pub fn remove(&mut self, id: u64, tree_is_locked: bool) -> Option<OverlayEntry> {
        let at = self.index_of(id)?;
        let mut entry = self.entries.remove(at);
        entry.inserted = false;
        entry.set_mounted(false);
        if !self.mounted {
            return Some(entry);
        }
        if !tree_is_locked {
            self.rebuilds += 1;
        }
        Some(entry)
    }

    /// Upstream's `build`.
    ///
    /// It walks the entries **from the top down**, keeping them onstage until
    /// it meets an opaque one, and everything after that is offstage. The list
    /// is built backwards and reversed at the end, which is the natural way to
    /// answer "what is still visible" -- you can only know by starting at the
    /// thing nearest the reader.
    pub fn onstage(&self) -> Vec<OnstageEntry> {
        let mut built = Vec::new();
        let mut onstage = true;
        for entry in self.entries.iter().rev() {
            if onstage {
                built.push(OnstageEntry {
                    id: entry.id,
                    ticker_enabled: true,
                });
                if entry.opaque {
                    onstage = false;
                }
            } else if entry.maintain_state {
                built.push(OnstageEntry {
                    id: entry.id,
                    ticker_enabled: false,
                });
            }
        }
        built.reverse();
        built
    }

    /// Mounts whatever [`OverlayState::onstage`] says should be built, so the
    /// entries' `mounted` flags and notifications follow.
    pub fn flush_build(&mut self) {
        let building: Vec<u64> = self.onstage().into_iter().map(|entry| entry.id).collect();
        for entry in self.entries.iter_mut() {
            entry.set_mounted(building.contains(&entry.id));
        }
    }

    /// Upstream's `debugIsVisible`, which upstream implements **only in debug**
    /// and has always return false otherwise, explicitly so that nobody builds
    /// behaviour on it. It is O(n) and it is a question the overlay does not
    /// otherwise need to answer.
    pub fn debug_is_visible(&self, id: u64) -> bool {
        for entry in self.entries.iter().rev() {
            if entry.id == id {
                return true;
            }
            if entry.opaque {
                return false;
            }
        }
        false
    }
}

/// Upstream `OverlayChildLocation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayChildLocation {
    /// The nearest enclosing overlay.
    #[default]
    NearestOverlay,
    /// The root overlay -- in a multi-view application, the first one below
    /// the `View`. A menu that should escape a nested overlay wants this.
    RootOverlay,
}

/// Upstream `OverlayPortalController`.
///
/// The z-order is a **monotonically increasing counter**, not a stack, and
/// that is what makes "the last portal to call `show` is on top" true without
/// anybody maintaining an order. Upstream starts it at `-2^63` (`-2^53` on the
/// web, where that is the largest exactly-representable integer) so that any
/// index at all beats an unset one, and so that a real application never
/// exhausts it.
#[derive(Debug, Default)]
pub struct OverlayPortalController {
    pub debug_label: Option<String>,
    /// The index held here while the controller is not attached. Once it is,
    /// the portal's own index is the source of truth and this stays `None`.
    z_order_index: Option<i64>,
    attached: bool,
    attached_z_order_index: Option<i64>,
}

/// The shared counter behind [`OverlayPortalController::now`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPortalClock {
    now: i64,
}

impl Default for OverlayPortalClock {
    fn default() -> OverlayPortalClock {
        OverlayPortalClock::new()
    }
}

impl OverlayPortalClock {
    /// Upstream's `_wallTime` starting point on a native platform.
    pub const START: i64 = i64::MIN;
    /// Upstream's starting point on the web, `-2^53`: the most negative
    /// integer a double can hold exactly. Past that, incrementing would stop
    /// changing the value and two portals would tie.
    pub const WEB_START: i64 = -9_007_199_254_740_992;

    pub fn new() -> OverlayPortalClock {
        OverlayPortalClock {
            now: OverlayPortalClock::START,
        }
    }

    pub fn web() -> OverlayPortalClock {
        OverlayPortalClock {
            now: OverlayPortalClock::WEB_START,
        }
    }

    /// Upstream's `_now()`: increments first, then returns. Every call gives a
    /// value strictly greater than the last.
    pub fn tick(&mut self) -> i64 {
        self.now += 1;
        self.now
    }
}

impl OverlayPortalController {
    pub fn new(debug_label: Option<&str>) -> OverlayPortalController {
        OverlayPortalController {
            debug_label: debug_label.map(str::to_string),
            z_order_index: None,
            attached: false,
            attached_z_order_index: None,
        }
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Upstream's `isShowing`, which reads the attached state when there is
    /// one and the pending index otherwise -- so a controller told to show
    /// before its portal exists already answers yes.
    pub fn is_showing(&self) -> bool {
        if self.attached {
            self.attached_z_order_index.is_some()
        } else {
            self.z_order_index.is_some()
        }
    }

    pub fn z_order_index(&self) -> Option<i64> {
        if self.attached {
            self.attached_z_order_index
        } else {
            self.z_order_index
        }
    }

    /// Upstream's `show`. **Calling it while already showing brings the child
    /// to the top** rather than doing nothing -- the index is taken again, and
    /// a fresh index is by construction the highest.
    pub fn show(&mut self, clock: &mut OverlayPortalClock) {
        let now = clock.tick();
        if self.attached {
            self.attached_z_order_index = Some(now);
        } else {
            self.z_order_index = Some(now);
        }
    }

    pub fn hide(&mut self) {
        if self.attached {
            self.attached_z_order_index = None;
        } else {
            debug_assert!(self.z_order_index.is_some(), "already hidden");
            self.z_order_index = None;
        }
    }

    pub fn toggle(&mut self, clock: &mut OverlayPortalClock) {
        if self.is_showing() {
            self.hide();
        } else {
            self.show(clock);
        }
    }

    /// The portal appearing and taking over. The index the controller was
    /// holding moves across, so a `show()` before the portal was built is not
    /// lost.
    pub fn attach(&mut self) {
        self.attached = true;
        self.attached_z_order_index = self.z_order_index.take();
    }

    pub fn detach(&mut self) {
        self.attached = false;
        self.z_order_index = self.attached_z_order_index.take();
    }
}

/// Upstream `OverlayPortal`: a widget whose overlay child is built here and
/// shown there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayPortal {
    pub overlay_location: OverlayChildLocation,
    /// Whether the portal was given a `child` of its own. Upstream's is
    /// optional -- a portal that exists only to place something in the overlay
    /// occupies no space itself.
    pub has_child: bool,
    /// Whether the overlay child is built through
    /// `OverlayPortal.overlayChildLayoutBuilder`, which is called **during
    /// layout** and is handed the child's position within the overlay. That is
    /// how a tooltip can both follow its target and shrink near an edge.
    pub uses_layout_builder: bool,
}

impl OverlayPortal {
    pub fn new() -> OverlayPortal {
        OverlayPortal::default()
    }

    /// Upstream's deprecated `OverlayPortal.targetsRootOverlay`.
    pub fn targets_root_overlay() -> OverlayPortal {
        OverlayPortal {
            overlay_location: OverlayChildLocation::RootOverlay,
            ..OverlayPortal::default()
        }
    }

    pub fn with_layout_builder(mut self) -> Self {
        self.uses_layout_builder = true;
        self
    }

    /// Whether the overlay child should be built this frame.
    pub fn builds_overlay_child(&self, controller: &OverlayPortalController) -> bool {
        controller.is_showing()
    }

    /// Sorts portals into the order their children stack in.
    pub fn stacking_order(controllers: &[(u64, Option<i64>)]) -> Vec<u64> {
        let mut showing: Vec<(u64, i64)> = controllers
            .iter()
            .filter_map(|(id, index)| index.map(|index| (*id, index)))
            .collect();
        showing.sort_by_key(|(_, index)| *index);
        showing.into_iter().map(|(id, _)| id).collect()
    }
}

/// Upstream `ContextMenuController`: shows a context menu, one at a time.
///
/// The one-at-a-time rule is enforced by **static** state -- upstream keeps a
/// single `_shownInstance` and a single `_menuOverlayEntry` for the whole
/// application -- and its comment says why plainly: "only one context menu can
/// be displayed at one time". Two would be two answers to one right-click.
///
/// It goes into the **root** overlay rather than the nearest one, so a menu
/// raised from inside a dialog is not clipped by the dialog.
#[derive(Debug, Default)]
pub struct ContextMenuController {
    pub id: u64,
    removals: usize,
}

/// The application-wide slot the controllers share.
#[derive(Debug, Default)]
pub struct ContextMenuSlot {
    shown: Option<u64>,
    entry: Option<u64>,
    builds: usize,
    next_entry: u64,
}

impl ContextMenuSlot {
    pub fn new() -> ContextMenuSlot {
        ContextMenuSlot {
            shown: None,
            entry: None,
            builds: 0,
            next_entry: 1,
        }
    }

    pub fn shown(&self) -> Option<u64> {
        self.shown
    }

    pub fn entry(&self) -> Option<u64> {
        self.entry
    }

    /// How many times the overlay entry was asked to rebuild.
    pub fn builds(&self) -> usize {
        self.builds
    }
}

impl ContextMenuController {
    pub fn new(id: u64) -> ContextMenuController {
        ContextMenuController { id, removals: 0 }
    }

    /// How many times upstream's `onRemove` would have fired.
    pub fn removals(&self) -> usize {
        self.removals
    }

    pub fn is_shown(&self, slot: &ContextMenuSlot) -> bool {
        slot.shown == Some(self.id)
    }

    /// Upstream's `show`, and the early return is the careful part: showing a
    /// menu **that is already shown** swaps the builder and rebuilds the
    /// existing entry rather than tearing it down and putting a new one up.
    ///
    /// Rebuilding in place is what lets a menu update -- a paste button
    /// becoming available when the clipboard answers -- without the menu
    /// blinking out and back.
    pub fn show(&self, slot: &mut ContextMenuSlot, others: &mut [&mut ContextMenuController]) {
        if self.is_shown(slot) {
            slot.builds += 1;
            return;
        }
        Self::remove_any(slot, others);
        slot.entry = Some(slot.next_entry);
        slot.next_entry += 1;
        slot.shown = Some(self.id);
    }

    /// Upstream's static `removeAny`, which takes down whichever menu is up --
    /// including somebody else's.
    pub fn remove_any(slot: &mut ContextMenuSlot, others: &mut [&mut ContextMenuController]) {
        slot.entry = None;
        if let Some(shown) = slot.shown.take() {
            for controller in others.iter_mut() {
                if controller.id == shown {
                    controller.removals += 1;
                }
            }
        }
    }

    /// Upstream's instance `remove`, which does **nothing if another menu is
    /// currently shown**.
    ///
    /// The difference from `removeAny` is the whole reason both exist: a
    /// widget tearing down should take its own menu with it, and must not take
    /// down the one that replaced it.
    pub fn remove(&mut self, slot: &mut ContextMenuSlot) {
        if !self.is_shown(slot) {
            return;
        }
        slot.entry = None;
        slot.shown = None;
        self.removals += 1;
    }

    /// Upstream's `markNeedsBuild`, which **asserts the menu is shown**.
    /// Rebuilding a menu that is not up is a caller error rather than a no-op.
    pub fn mark_needs_build(&self, slot: &mut ContextMenuSlot) {
        debug_assert!(self.is_shown(slot), "the context menu is not shown");
        if self.is_shown(slot) {
            slot.builds += 1;
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn only_one_context_menu_can_be_up_at_a_time() {
        // Two would be two answers to one right-click.
        let mut slot = ContextMenuSlot::new();
        let first = ContextMenuController::new(1);
        let mut first_owned = ContextMenuController::new(1);
        let second = ContextMenuController::new(2);

        first.show(&mut slot, &mut []);
        assert!(first.is_shown(&slot));

        second.show(&mut slot, &mut [&mut first_owned]);
        assert!(second.is_shown(&slot));
        assert!(!first.is_shown(&slot));
        assert_eq!(
            first_owned.removals(),
            1,
            "and the one it replaced was told"
        );
    }

    #[test]
    fn showing_a_menu_that_is_already_up_rebuilds_it_in_place() {
        // Which is what lets a paste button appear when the clipboard answers,
        // without the menu blinking out and back.
        let mut slot = ContextMenuSlot::new();
        let controller = ContextMenuController::new(1);
        controller.show(&mut slot, &mut []);
        let entry = slot.entry();

        controller.show(&mut slot, &mut []);
        assert_eq!(slot.entry(), entry, "the same overlay entry");
        assert_eq!(slot.builds(), 1, "rebuilt rather than replaced");
    }

    #[test]
    fn removing_your_own_menu_does_not_take_down_the_one_that_replaced_it() {
        // Which is the whole reason remove and removeAny both exist: a widget
        // tearing down should take its own menu, and only its own.
        let mut slot = ContextMenuSlot::new();
        let mut first = ContextMenuController::new(1);
        let second = ContextMenuController::new(2);

        first.show(&mut slot, &mut []);
        second.show(&mut slot, &mut []);

        first.remove(&mut slot);
        assert!(second.is_shown(&slot), "untouched");
        assert_eq!(first.removals(), 0, "and its onRemove did not fire");
    }

    #[test]
    fn remove_any_takes_down_whichever_menu_is_up() {
        let mut slot = ContextMenuSlot::new();
        let mut owned = ContextMenuController::new(1);
        let controller = ContextMenuController::new(1);
        controller.show(&mut slot, &mut []);

        ContextMenuController::remove_any(&mut slot, &mut [&mut owned]);
        assert_eq!(slot.shown(), None);
        assert_eq!(slot.entry(), None);
        assert_eq!(owned.removals(), 1);
    }

    #[test]
    fn removing_when_nothing_is_up_is_harmless() {
        let mut slot = ContextMenuSlot::new();
        let mut controller = ContextMenuController::new(1);
        controller.remove(&mut slot);
        ContextMenuController::remove_any(&mut slot, &mut []);
        assert_eq!(slot.shown(), None);
    }

    #[test]
    #[should_panic(expected = "the context menu is not shown")]
    fn rebuilding_a_menu_that_is_not_up_is_a_caller_error() {
        let mut slot = ContextMenuSlot::new();
        ContextMenuController::new(1).mark_needs_build(&mut slot);
    }

    use super::*;

    fn overlay_with(entries: Vec<OverlayEntry>) -> OverlayState {
        let mut overlay = OverlayState::new();
        overlay
            .insert_all(entries, InsertPosition::Top)
            .expect("fresh entries");
        overlay
    }

    // -- Insertion order ---------------------------------------------------

    #[test]
    fn an_entry_with_nothing_said_about_it_goes_on_top() {
        let mut overlay = OverlayState::new();
        overlay
            .insert(OverlayEntry::new(1), InsertPosition::Top)
            .unwrap();
        overlay
            .insert(OverlayEntry::new(2), InsertPosition::Top)
            .unwrap();
        assert_eq!(overlay.entry_ids(), vec![1, 2], "later means higher");
    }

    #[test]
    fn below_takes_the_anchors_place_and_above_takes_the_one_after_it() {
        // Both put the new entry on the side its name says, given that later
        // in the list means higher in the stack.
        let mut overlay = overlay_with(vec![OverlayEntry::new(1), OverlayEntry::new(2)]);
        overlay
            .insert(OverlayEntry::new(3), InsertPosition::Below(2))
            .unwrap();
        assert_eq!(overlay.entry_ids(), vec![1, 3, 2]);

        overlay
            .insert(OverlayEntry::new(4), InsertPosition::Above(1))
            .unwrap();
        assert_eq!(overlay.entry_ids(), vec![1, 4, 3, 2]);
    }

    #[test]
    fn an_anchor_that_is_not_in_the_overlay_is_refused() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1)]);
        assert_eq!(
            overlay.insert(OverlayEntry::new(2), InsertPosition::Above(99)),
            Err(OverlayError::AnchorNotPresent)
        );
        assert_eq!(overlay.entry_ids(), vec![1], "and nothing was inserted");
    }

    #[test]
    fn an_entry_already_in_an_overlay_cannot_be_inserted_again() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1)]);
        let already = overlay.entries()[0].clone();
        assert_eq!(
            overlay.insert(already, InsertPosition::Top),
            Err(OverlayError::AlreadyInserted)
        );
    }

    #[test]
    fn inserting_nothing_does_not_cost_a_rebuild() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1)]);
        let before = overlay.rebuilds();
        overlay.insert_all(Vec::new(), InsertPosition::Top).unwrap();
        assert_eq!(overlay.rebuilds(), before);
    }

    #[test]
    fn inserting_several_keeps_the_order_they_were_given_in() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1), OverlayEntry::new(9)]);
        overlay
            .insert_all(
                vec![OverlayEntry::new(2), OverlayEntry::new(3)],
                InsertPosition::Below(9),
            )
            .unwrap();
        assert_eq!(overlay.entry_ids(), vec![1, 2, 3, 9]);
    }

    // -- Rearranging -------------------------------------------------------

    #[test]
    fn rearranging_keeps_entries_it_was_not_told_about() {
        // It is a reordering of the named ones, not a replacement of the list.
        let mut overlay = overlay_with(vec![
            OverlayEntry::new(1),
            OverlayEntry::new(2),
            OverlayEntry::new(3),
        ]);
        overlay.rearrange(&[3, 1], InsertPosition::Top).unwrap();
        assert_eq!(
            overlay.entry_ids(),
            vec![3, 1, 2],
            "2 survived and went to the given position"
        );
    }

    #[test]
    fn rearranging_to_the_order_that_is_already_there_costs_nothing() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1), OverlayEntry::new(2)]);
        let before = overlay.rebuilds();
        overlay.rearrange(&[1, 2], InsertPosition::Top).unwrap();
        assert_eq!(overlay.rebuilds(), before);
    }

    #[test]
    fn naming_the_same_entry_twice_is_refused() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1), OverlayEntry::new(2)]);
        assert_eq!(
            overlay.rearrange(&[1, 1], InsertPosition::Top),
            Err(OverlayError::Duplicated)
        );
    }

    #[test]
    fn rearranging_nothing_is_not_an_error() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1)]);
        assert!(overlay.rearrange(&[], InsertPosition::Top).is_ok());
        assert_eq!(overlay.entry_ids(), vec![1]);
    }

    // -- Opacity, and what it actually saves -------------------------------

    #[test]
    fn an_opaque_entry_stops_the_overlay_building_what_is_under_it() {
        // Not painting over it -- not building it at all. That is where the
        // saving is.
        let overlay = overlay_with(vec![
            OverlayEntry::new(1),
            OverlayEntry::new(2).with_opaque(true),
            OverlayEntry::new(3),
        ]);
        assert_eq!(
            overlay.onstage().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 3],
            "1 is behind the opaque entry and is not built"
        );
    }

    #[test]
    fn maintain_state_buys_a_place_in_the_tree_but_not_a_running_ticker() {
        // A route behind an opaque one keeps its state, but an invisible thing
        // animating is work with no observer.
        let overlay = overlay_with(vec![
            OverlayEntry::new(1).with_maintain_state(true),
            OverlayEntry::new(2).with_opaque(true),
        ]);
        let built = overlay.onstage();
        assert_eq!(built.len(), 2);
        assert_eq!(
            built[0],
            OnstageEntry {
                id: 1,
                ticker_enabled: false
            }
        );
        assert_eq!(
            built[1],
            OnstageEntry {
                id: 2,
                ticker_enabled: true
            }
        );
    }

    #[test]
    fn the_opaque_entry_itself_is_still_built() {
        // It occludes what is under it, not itself.
        let overlay = overlay_with(vec![OverlayEntry::new(1).with_opaque(true)]);
        assert_eq!(overlay.onstage().len(), 1);
    }

    #[test]
    fn only_the_topmost_opaque_entry_matters() {
        let overlay = overlay_with(vec![
            OverlayEntry::new(1).with_opaque(true),
            OverlayEntry::new(2),
            OverlayEntry::new(3).with_opaque(true),
            OverlayEntry::new(4),
        ]);
        assert_eq!(
            overlay.onstage().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[test]
    fn inserted_and_mounted_are_different_questions() {
        // An entry hidden behind an opaque one is inserted and not mounted,
        // and a caller positioning something against its widget needs to know
        // which.
        let mut overlay = overlay_with(vec![
            OverlayEntry::new(1),
            OverlayEntry::new(2).with_opaque(true),
        ]);
        overlay.flush_build();

        let hidden = &overlay.entries()[0];
        assert!(hidden.is_inserted());
        assert!(!hidden.mounted());
        assert!(overlay.entries()[1].mounted());
    }

    #[test]
    fn an_entry_notifies_when_its_widget_appears_and_when_it_goes() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1)]);
        overlay.flush_build();
        assert_eq!(overlay.entries()[0].notifications(), 1);

        overlay.flush_build();
        assert_eq!(
            overlay.entries()[0].notifications(),
            1,
            "nothing changed, nobody told"
        );

        let removed = overlay.remove(1, false).unwrap();
        assert_eq!(removed.notifications(), 2);
        assert!(!removed.mounted() && !removed.is_inserted());
    }

    #[test]
    fn visibility_is_the_same_question_the_build_answers() {
        let overlay = overlay_with(vec![
            OverlayEntry::new(1),
            OverlayEntry::new(2).with_opaque(true),
            OverlayEntry::new(3),
        ]);
        assert!(!overlay.debug_is_visible(1));
        assert!(overlay.debug_is_visible(2), "the opaque entry itself");
        assert!(overlay.debug_is_visible(3));
        assert!(!overlay.debug_is_visible(99));
    }

    // -- Removal -----------------------------------------------------------

    #[test]
    fn a_removal_during_a_build_waits_for_the_frame_to_end() {
        // Upstream's own note: remove after the overlay has rebuilt this frame
        // and the screen does not change until the next one, many milliseconds
        // later.
        let mut overlay = overlay_with(vec![OverlayEntry::new(1), OverlayEntry::new(2)]);
        let before = overlay.rebuilds();

        overlay.remove(1, true);
        assert_eq!(overlay.rebuilds(), before, "deferred");
        assert_eq!(overlay.entry_ids(), vec![2], "though it is already gone");

        overlay.remove(2, false);
        assert_eq!(overlay.rebuilds(), before + 1);
    }

    #[test]
    fn removing_from_an_unmounted_overlay_asks_for_no_rebuild() {
        let mut overlay = overlay_with(vec![OverlayEntry::new(1)]);
        overlay.unmount();
        let before = overlay.rebuilds();
        overlay.remove(1, false);
        assert_eq!(overlay.rebuilds(), before);
    }

    #[test]
    #[should_panic(expected = "must be removed from the Overlay before dispose")]
    fn disposing_an_inserted_entry_leaves_the_overlay_holding_a_dead_thing() {
        let overlay = overlay_with(vec![OverlayEntry::new(1)]);
        let mut still_in = overlay.entries()[0].clone();
        still_in.dispose();
    }

    #[test]
    fn changing_opacity_is_something_the_overlay_has_to_be_told() {
        // Turning it on hides everything below, and the overlay cannot notice
        // that on its own.
        let mut entry = OverlayEntry::new(1);
        assert!(entry.set_opaque(true));
        assert!(!entry.set_opaque(true), "same value, nothing to say");
        assert!(entry.set_maintain_state(true));
        assert!(!entry.set_maintain_state(true));
    }

    // -- The portal --------------------------------------------------------

    #[test]
    fn the_last_portal_to_show_is_the_one_on_top() {
        // The z-order is a counter rather than a stack, which is what makes
        // that true without anyone maintaining an order.
        let mut clock = OverlayPortalClock::new();
        let mut first = OverlayPortalController::new(Some("first"));
        let mut second = OverlayPortalController::new(Some("second"));
        first.attach();
        second.attach();

        first.show(&mut clock);
        second.show(&mut clock);
        assert_eq!(
            OverlayPortal::stacking_order(&[
                (1, first.z_order_index()),
                (2, second.z_order_index())
            ]),
            vec![1, 2]
        );

        first.show(&mut clock);
        assert_eq!(
            OverlayPortal::stacking_order(&[
                (1, first.z_order_index()),
                (2, second.z_order_index())
            ]),
            vec![2, 1],
            "showing again brings it to the top rather than doing nothing"
        );
    }

    #[test]
    fn a_hidden_portal_is_not_in_the_stack_at_all() {
        let mut clock = OverlayPortalClock::new();
        let mut controller = OverlayPortalController::new(None);
        controller.attach();
        controller.show(&mut clock);
        controller.hide();
        assert!(!controller.is_showing());
        assert_eq!(
            OverlayPortal::stacking_order(&[(1, controller.z_order_index())]),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn a_show_before_the_portal_exists_is_not_lost() {
        // The controller holds the index until it has somewhere to put it.
        let mut clock = OverlayPortalClock::new();
        let mut controller = OverlayPortalController::new(None);
        controller.show(&mut clock);
        assert!(
            controller.is_showing(),
            "it already answers yes, detached or not"
        );

        let index = controller.z_order_index();
        controller.attach();
        assert!(controller.is_showing());
        assert_eq!(controller.z_order_index(), index, "and it moved across");
    }

    #[test]
    fn the_index_goes_back_to_the_controller_when_the_portal_leaves() {
        let mut clock = OverlayPortalClock::new();
        let mut controller = OverlayPortalController::new(None);
        controller.attach();
        controller.show(&mut clock);
        let index = controller.z_order_index();

        controller.detach();
        assert!(!controller.is_attached());
        assert_eq!(controller.z_order_index(), index);
        assert!(controller.is_showing());
    }

    #[test]
    fn toggling_alternates() {
        let mut clock = OverlayPortalClock::new();
        let mut controller = OverlayPortalController::new(None);
        controller.attach();
        assert!(!controller.is_showing());

        controller.toggle(&mut clock);
        assert!(controller.is_showing());
        controller.toggle(&mut clock);
        assert!(!controller.is_showing());
    }

    #[test]
    fn every_index_is_strictly_greater_than_the_last() {
        // Two portals must never tie, or their order would be undefined.
        let mut clock = OverlayPortalClock::new();
        let mut previous = i64::MIN;
        for _ in 0..1000 {
            let now = clock.tick();
            assert!(now > previous);
            previous = now;
        }
    }

    #[test]
    fn the_web_counter_starts_where_a_double_can_still_count_exactly() {
        // Past 2^53 an increment stops changing the value, and two portals
        // would tie.
        assert_eq!(OverlayPortalClock::WEB_START, -9_007_199_254_740_992);
        assert_eq!(
            OverlayPortalClock::WEB_START as f64 as i64,
            OverlayPortalClock::WEB_START,
            "and it survives the round trip through a double"
        );

        let mut clock = OverlayPortalClock::web();
        assert_eq!(clock.tick(), OverlayPortalClock::WEB_START + 1);
    }

    #[test]
    fn a_portal_builds_its_overlay_child_only_while_showing() {
        let mut clock = OverlayPortalClock::new();
        let portal = OverlayPortal::new();
        let mut controller = OverlayPortalController::new(None);
        controller.attach();
        assert!(!portal.builds_overlay_child(&controller));

        controller.show(&mut clock);
        assert!(portal.builds_overlay_child(&controller));
    }

    #[test]
    fn a_portal_can_choose_the_root_overlay_over_the_nearest_one() {
        // A menu that should escape a nested overlay wants the root.
        assert_eq!(
            OverlayPortal::new().overlay_location,
            OverlayChildLocation::NearestOverlay
        );
        assert_eq!(
            OverlayPortal::targets_root_overlay().overlay_location,
            OverlayChildLocation::RootOverlay
        );
    }
}
