//! The raw menu anchor -- a port of upstream's `widgets/raw_menu_anchor.dart`.
//!
//! A menu system is a **tree of anchors**, not a list of open menus. Each
//! anchor knows its parent and its children, and almost every decision here
//! is about which direction along that tree a request travels.
//!
//! * A tap outside closes **downwards**: the submenu goes, its parent stays.
//!   A reader who clicked away from a submenu did not ask to lose the menu
//!   bar.
//! * Escape closes from the **root**: `DismissMenuAction` reaches for
//!   `_anchor.root` rather than the anchor it was invoked on, because escape
//!   means "I am done with this menu", not "one level please".
//! * An open-state change travels **upwards** first, so an ancestor that
//!   paints differently while a descendant is open finds out before anyone
//!   rebuilds.
//!
//! The other recurring idea is that closing has two speeds. `closeChildren`
//! shuts a child immediately; `requestChildrenClose` starts its *closing
//! sequence*, which an animated menu needs in order to animate out at all.
//! Upstream keeps both and documents the difference on each.
//!
//! ## What is not here
//!
//! The overlay portal that hosts the menu surface, the focus traversal
//! between items and the tap-region plumbing belong to widgets this crate
//! spells differently -- see [`crate::overlay`] and [`crate::menu_anchor`].
//! What is ported is the anchor tree, the open and close request paths, and
//! the two automatic closes.

use crate::render::{Offset, Size};

/// A rectangle, as this module needs it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchorRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Upstream `RawMenuOverlayInfo`: what a menu's overlay builder is told.
///
/// The anchor rect is measured against **whichever overlay the menu is going
/// into** -- the nearest one, or the root when `useRootOverlay` is set. Two
/// different coordinate spaces for the same anchor, and the flag is what says
/// which one the builder is being handed.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMenuOverlayInfo {
    pub anchor_rect: AnchorRect,
    pub overlay_size: Size,
    /// The `position` given to [`MenuController::open`], to be applied as an
    /// offset from the anchor's **top-left corner**.
    pub position: Option<Offset>,
    /// Upstream's `tapRegionGroupId`: the whole menu system shares one, which
    /// is what makes a tap inside a submenu not count as a tap outside its
    /// parent.
    pub tap_region_group_id: u64,
}

impl RawMenuOverlayInfo {
    pub fn new(
        anchor_rect: AnchorRect,
        overlay_size: Size,
        tap_region_group_id: u64,
    ) -> RawMenuOverlayInfo {
        RawMenuOverlayInfo {
            anchor_rect,
            overlay_size,
            position: None,
            tap_region_group_id,
        }
    }

    pub fn with_position(mut self, position: Offset) -> Self {
        self.position = Some(position);
        self
    }
}

/// How thoroughly a close is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseKind {
    /// Upstream's `closeChildren`: shut now, no sequence.
    Immediate,
    /// Upstream's `requestChildrenClose`: run each child's closing sequence,
    /// which is what lets an animated menu animate out.
    Requested,
}

/// One anchor in the menu tree.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuAnchorNode {
    pub id: u64,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub open: bool,
    /// Upstream's `consumeOutsideTaps`: whether the tap that dismissed the
    /// menu is also swallowed. False by default, so a tap that closes a menu
    /// still reaches whatever it landed on -- which is usually what the reader
    /// meant by tapping there.
    pub consume_outside_taps: bool,
    /// Upstream's `useRootOverlay`, which decides whose coordinate space the
    /// overlay info is in.
    pub use_root_overlay: bool,
    /// How many times this anchor's *closing sequence* was started, as
    /// opposed to it being shut outright. Upstream's `onCloseRequested` runs
    /// here, and a menu that animates out is animating during this count.
    pub close_requests: usize,
    /// How many times a rebuild was asked for.
    pub dirty_marks: usize,
    /// Rebuilds deferred because the request arrived during a build.
    pub deferred_marks: usize,
}

impl MenuAnchorNode {
    pub fn new(id: u64) -> MenuAnchorNode {
        MenuAnchorNode {
            id,
            parent: None,
            children: Vec::new(),
            open: false,
            close_requests: 0,
            consume_outside_taps: false,
            use_root_overlay: false,
            dirty_marks: 0,
            deferred_marks: 0,
        }
    }
}

/// The anchor tree, which is what a [`MenuController`] and the two anchor
/// widgets act on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuAnchorTree {
    nodes: Vec<MenuAnchorNode>,
    /// What was opened and closed, in order.
    log: Vec<(u64, bool)>,
}

impl MenuAnchorTree {
    pub fn new() -> MenuAnchorTree {
        MenuAnchorTree::default()
    }

    pub fn log(&self) -> &[(u64, bool)] {
        &self.log
    }

    pub fn node(&self, id: u64) -> Option<&MenuAnchorNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    fn node_mut(&mut self, id: u64) -> Option<&mut MenuAnchorNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    pub fn is_open(&self, id: u64) -> bool {
        self.node(id).map(|node| node.open).unwrap_or(false)
    }

    pub fn insert(&mut self, node: MenuAnchorNode) {
        debug_assert!(self.node(node.id).is_none(), "an anchor is added once");
        self.nodes.push(node);
    }

    /// Upstream's `didChangeDependencies` re-parenting, which **removes from
    /// the old parent before adding to the new**. An anchor moved in the tree
    /// would otherwise be a child of two menus, and closing one of them would
    /// leave it half attached.
    pub fn set_parent(&mut self, child: u64, parent: Option<u64>) -> Result<(), &'static str> {
        if parent == Some(child) {
            return Err("a MenuController should only be attached to one anchor at a time");
        }
        let old_parent = self.node(child).and_then(|node| node.parent);
        if old_parent == parent {
            return Ok(());
        }
        if let Some(old) = old_parent {
            if let Some(node) = self.node_mut(old) {
                node.children.retain(|held| *held != child);
            }
        }
        if let Some(node) = self.node_mut(child) {
            node.parent = parent;
        }
        if let Some(new) = parent {
            if let Some(node) = self.node_mut(new) {
                debug_assert!(!node.children.contains(&child));
                node.children.push(child);
            }
        }
        Ok(())
    }

    /// Upstream's `root`: walk up until there is no parent.
    pub fn root_of(&self, id: u64) -> u64 {
        let mut at = id;
        while let Some(parent) = self.node(at).and_then(|node| node.parent) {
            at = parent;
        }
        at
    }

    pub fn is_root(&self, id: u64) -> bool {
        self.node(id)
            .map(|node| node.parent.is_none())
            .unwrap_or(false)
    }

    /// Upstream's `handleOpenRequest` reaching `open`.
    pub fn open(&mut self, id: u64) {
        if let Some(node) = self.node_mut(id) {
            if node.open {
                return;
            }
            node.open = true;
        }
        self.log.push((id, true));
        self.child_changed_open_state(id, false);
    }

    /// Upstream's `close`.
    pub fn close(&mut self, id: u64) {
        // Children go first: a submenu outliving its parent would be a menu
        // floating over nothing.
        self.close_children(id, CloseKind::Immediate);
        let was_open = self.node(id).map(|node| node.open).unwrap_or(false);
        if let Some(node) = self.node_mut(id) {
            node.open = false;
        }
        if was_open {
            self.log.push((id, false));
            self.child_changed_open_state(id, false);
        }
    }

    /// Upstream's `closeChildren` and `requestChildrenClose`, which differ in
    /// **how** each child is shut rather than in which children are shut.
    ///
    /// `Immediate` is `closeChildren`: it calls `close` on each child, which
    /// shuts it now. `Requested` is `requestChildrenClose`: it calls
    /// `handleCloseRequest`, which starts the child's *closing sequence* --
    /// and that sequence is where an animated menu animates out. Upstream
    /// documents the pair on each method with a cross-reference to the other,
    /// which is how you can tell the difference is load-bearing.
    ///
    /// The `inDispose` path uses the immediate one, and has to: a menu being
    /// unmounted has no frames left to animate in.
    ///
    /// Both iterate a **copy** of the child list, because closing a child
    /// removes it from that list.
    pub fn close_children(&mut self, id: u64, kind: CloseKind) {
        let children = self
            .node(id)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        for child in children {
            match kind {
                CloseKind::Immediate => self.close(child),
                CloseKind::Requested => self.handle_close_request(child),
            }
        }
    }

    /// Upstream's `handleCloseRequest`, the entry point of the closing
    /// sequence. The default implementation closes straight away; a menu with
    /// an `onCloseRequested` of its own may delay it, or decline.
    pub fn handle_close_request(&mut self, id: u64) {
        if let Some(node) = self.node_mut(id) {
            node.close_requests += 1;
        }
        self.close(id);
    }

    /// A menu whose `onCloseRequested` is still waiting: the request was
    /// recorded but the close has not happened.
    pub fn handle_close_request_deferred(&mut self, id: u64) {
        if let Some(node) = self.node_mut(id) {
            node.close_requests += 1;
        }
    }

    /// Upstream's `handleOutsideTap`, which closes this anchor's **children**
    /// and leaves the anchor itself open.
    ///
    /// A reader who clicked away from a submenu did not ask to lose the menu
    /// bar it hangs off. Only a tap outside the whole system reaches the root,
    /// and the shared tap-region group id is what makes a tap inside a submenu
    /// not count as outside its parent.
    pub fn handle_outside_tap(&mut self, id: u64) -> bool {
        if !self.is_open(id) {
            return false;
        }
        self.close_children(id, CloseKind::Requested);
        self.node(id)
            .map(|node| node.consume_outside_taps)
            .unwrap_or(false)
    }

    /// Upstream's `DismissMenuAction.invoke`, which reaches for **the root**
    /// rather than the anchor it was invoked on. Escape means "I am done with
    /// this menu", not "one level, please".
    pub fn dismiss(&mut self, id: u64) {
        let root = self.root_of(id);
        self.close(root);
    }

    /// Upstream's `_handleScroll`, and the comment on it is the decision:
    ///
    /// > If an ancestor scrolls, and we're a root anchor, then close the
    /// > menus. Don't just close it on *any* scroll, since we want to be able
    /// > to scroll menus themselves if they're too big for the view.
    ///
    /// So only the **root** listens. A menu long enough to scroll would
    /// otherwise close itself the moment the reader scrolled it.
    pub fn handle_ancestor_scroll(&mut self, id: u64) -> bool {
        if !self.is_root(id) || !self.is_open(id) {
            return false;
        }
        self.close(id);
        true
    }

    /// Upstream's view-size check in `didChangeDependencies`: a menu is
    /// positioned against a viewport that just changed, so its position is
    /// stale. Closing is the honest answer -- there is no way to know where
    /// the anchor moved to until the next layout.
    ///
    /// Only checked when this anchor is the root **and** already open, and
    /// only when the size genuinely differs; the first observation records the
    /// size without closing anything.
    pub fn handle_view_size_change(
        &mut self,
        id: u64,
        old_size: Option<Size>,
        new_size: Size,
    ) -> bool {
        let changed = old_size.is_some_and(|old| old != new_size);
        if !self.is_root(id) || !changed || !self.is_open(id) {
            return false;
        }
        self.close(id);
        true
    }

    /// Upstream's `_childChangedOpenState`, which travels **up first** and
    /// marks dirty second.
    ///
    /// `during_build` is upstream's `SchedulerPhase.persistentCallbacks`
    /// check: a state change arriving mid-build cannot mark anything dirty, so
    /// it is deferred to a post-frame callback. Marking during a build is the
    /// error this avoids, and deferring costs one frame of a menu drawn in its
    /// old state.
    pub fn child_changed_open_state(&mut self, id: u64, during_build: bool) {
        let mut at = Some(id);
        while let Some(current) = at {
            if let Some(node) = self.node_mut(current) {
                if during_build {
                    node.deferred_marks += 1;
                } else {
                    node.dirty_marks += 1;
                }
                at = node.parent;
            } else {
                break;
            }
        }
    }

    /// Upstream's `dispose`, which closes, detaches from the parent, clears
    /// the children and detaches the controller -- in that order.
    pub fn dispose(&mut self, id: u64) {
        if self.is_open(id) {
            self.close(id);
        }
        let parent = self.node(id).and_then(|node| node.parent);
        if let Some(parent) = parent {
            if let Some(node) = self.node_mut(parent) {
                node.children.retain(|held| *held != id);
            }
        }
        if let Some(node) = self.node_mut(id) {
            node.parent = None;
            node.children.clear();
        }
        self.nodes.retain(|node| node.id != id);
    }
}

/// Upstream `MenuController`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuController {
    /// Upstream's `_anchor`, set when the controller is attached.
    anchor: Option<u64>,
}

impl MenuController {
    pub fn new() -> MenuController {
        MenuController::default()
    }

    pub fn anchor(&self) -> Option<u64> {
        self.anchor
    }

    /// Upstream's `_attach`.
    pub fn attach(&mut self, anchor: u64) {
        self.anchor = Some(anchor);
    }

    /// Upstream's `_detach`, which detaches **only if it is that anchor**. An
    /// anchor being disposed after the controller has moved on must not tear
    /// the controller off its new one.
    pub fn detach(&mut self, anchor: u64) {
        if self.anchor == Some(anchor) {
            self.anchor = None;
        }
    }

    pub fn is_open(&self, tree: &MenuAnchorTree) -> bool {
        self.anchor.map(|id| tree.is_open(id)).unwrap_or(false)
    }

    /// Upstream's `open`, which **asserts it is attached**. Opening a menu
    /// nobody built is a programming error rather than a no-op.
    pub fn open(&self, tree: &mut MenuAnchorTree) {
        let anchor = self.anchor.expect("MenuController is not attached");
        tree.open(anchor);
    }

    /// Upstream's `close`, which -- unlike `open` -- does **not** assert.
    /// Closing a menu that is already gone is exactly what a dispose path
    /// does, and it should be allowed to say so harmlessly.
    pub fn close(&self, tree: &mut MenuAnchorTree) {
        if let Some(anchor) = self.anchor {
            tree.close(anchor);
        }
    }

    /// Upstream's `closeChildren`, which shuts the submenus and leaves this
    /// menu open.
    pub fn close_children(&self, tree: &mut MenuAnchorTree) {
        let anchor = self.anchor.expect("MenuController is not attached");
        tree.close_children(anchor, CloseKind::Requested);
    }
}

/// Whether a lookup of the controller establishes a dependency.
///
/// Upstream ships both, and the pair is the interesting part.
/// `MenuController.maybeOf` deliberately does **not** depend, so a menu item
/// that holds a controller in order to call `close()` does not rebuild every
/// time any menu opens. `maybeIsOpenOf` does depend, because its answer is
/// exactly the thing that changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerLookup {
    /// `maybeOf`: read it, do not watch it.
    WithoutDependency,
    /// `maybeIsOpenOf`: watch it.
    WithDependency,
}

impl ControllerLookup {
    pub fn establishes_dependency(self) -> bool {
        self == ControllerLookup::WithDependency
    }
}

/// Upstream `RawMenuAnchor`: an anchor with a menu overlay of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawMenuAnchor {
    pub consume_outside_taps: bool,
    pub use_root_overlay: bool,
    /// Whether a `childFocusNode` was supplied. Upstream uses it to send focus
    /// back to the thing that opened the menu when the menu closes -- without
    /// it, closing a menu with the keyboard leaves focus nowhere.
    pub has_child_focus_node: bool,
}

impl RawMenuAnchor {
    pub fn new() -> RawMenuAnchor {
        RawMenuAnchor::default()
    }

    /// Upstream's `_defaultOnOpenRequested`, which calls `showOverlay`
    /// **synchronously** -- so `onOpen` fires in the same turn, and it fires
    /// **whether or not the overlay was already showing**.
    ///
    /// The extension point is what the shape is for: a custom
    /// `onOpenRequested` may delay the call, or never make it, and then
    /// `onOpen` never fires either. That is how a menu waits for an animation
    /// or refuses to open at all.
    pub fn default_on_open_requested(show_overlay: impl FnOnce()) {
        show_overlay();
    }

    /// Upstream's note that calling `showOverlay` after disposal is a no-op
    /// and does not trigger `onOpen`. A delayed opener whose menu went away
    /// while it waited must not announce an opening that cannot happen.
    pub fn show_overlay(&self, disposed: bool) -> bool {
        !disposed
    }
}

/// Upstream `RawMenuAnchorGroup`: an anchor with children and no menu of its
/// own.
///
/// A menu bar is one of these. It never opens -- `isOpen` is true when **any
/// child** is open -- which is why a menu bar can host submenus without
/// itself being a menu that could be dismissed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawMenuAnchorGroup;

impl RawMenuAnchorGroup {
    /// Upstream's `isOpen` for a group.
    pub fn is_open(tree: &MenuAnchorTree, id: u64) -> bool {
        tree.node(id)
            .map(|node| node.children.iter().any(|child| tree.is_open(*child)))
            .unwrap_or(false)
    }
}

/// Upstream `DismissMenuAction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DismissMenuAction {
    pub controller: MenuController,
}

impl DismissMenuAction {
    pub fn new(controller: MenuController) -> DismissMenuAction {
        DismissMenuAction { controller }
    }

    /// Upstream's `isEnabled`: only while the controller is attached. An
    /// escape press with no menu open should reach whatever else wanted it --
    /// a dialog, usually.
    pub fn is_enabled(&self) -> bool {
        self.controller.anchor().is_some()
    }

    /// Upstream's `invoke`, which closes from the root.
    pub fn invoke(&self, tree: &mut MenuAnchorTree) {
        if let Some(anchor) = self.controller.anchor() {
            tree.dismiss(anchor);
        }
    }

    /// [`DismissMenuAction::invoke`] with the screen included: the tree closes
    /// and the panels come down. This is the one a widget wants; the other is
    /// the tree half on its own, which is all a test of the tree can see.
    pub fn dismiss_the_menus(&self) {
        if let Some(anchor) = self.controller.anchor() {
            close_menu(anchor);
        }
    }
}

// -- The tree every anchor shares --------------------------------------------

thread_local! {
    /// One menu tree per UI thread.
    ///
    /// Every rule in [`MenuAnchorTree`] was written to take `&mut self`, and
    /// nothing owned one -- so a widget that wanted to ask "did this tap close
    /// my submenu" had no tree to ask. This is where the tree lives, in the
    /// shape this crate already uses for state that belongs to the view rather
    /// than to any one widget: [`crate::focus`]'s manager and
    /// [`crate::theatre`]'s modal stack are both thread-locals for the same
    /// reason.
    ///
    /// It is a tree and not a stack, which is the difference from `MODALS`: a
    /// menu bar with two open submenus is three nodes and one root, and Escape
    /// has to reach the root while a tap outside reaches only the children.
    static TREE: std::cell::RefCell<MenuAnchorTree> =
        std::cell::RefCell::new(MenuAnchorTree::new());
}

/// Reads the ambient tree.
pub fn with_menu_tree<R>(read: impl FnOnce(&MenuAnchorTree) -> R) -> R {
    TREE.with(|tree| read(&tree.borrow()))
}

/// Changes the ambient tree.
pub fn with_menu_tree_mut<R>(change: impl FnOnce(&mut MenuAnchorTree) -> R) -> R {
    TREE.with(|tree| change(&mut tree.borrow_mut()))
}

/// Empties it. Tests only: the tree outlives one test otherwise, and an anchor
/// left open by one would be open at the start of the next.
#[cfg(test)]
pub fn reset_menu_tree() {
    TREE.with(|tree| *tree.borrow_mut() = MenuAnchorTree::new());
}

/// Puts an anchor's menu on screen: upstream's `RawMenuAnchor` opening its
/// `OverlayPortal`, with the tap-region surface
/// [`crate::theatre::show_tap_dismissed`] provides.
///
/// # What the tap outside does, and what it does not
///
/// The surface's dismissal takes the *panel* down. What happens to the **menu
/// tree** is a second answer, and the two are not the same thing said twice:
/// one is about a panel in an overlay, the other about which anchors are still
/// open. Wiring only the first would leave the tree believing a menu was up
/// that nobody could see.
///
/// The tree answer here is **this anchor closes** -- upstream's panel wraps
/// itself in `TapRegion(onTapOutside: () => anchor._menuController.close())`.
/// It is not [`MenuAnchorTree::handle_outside_tap`], which closes the
/// children and leaves the anchor open: that is the rule for the region
/// around the **button**, where a reader who clicked away from a submenu did
/// not ask to lose the menu bar it hangs off. Two regions, two rules, and
/// giving the panel the button's rule leaves the anchor believing it is still
/// open -- so pressing the button again does nothing at all, because
/// [`crate::menu_anchor::SubmenuButton::should_open`] asks the tree.
pub fn open_menu_surface(
    overlay: std::rc::Rc<crate::theatre::OverlayHandle>,
    anchor: u64,
    group_id: u64,
    content: impl Fn() -> crate::framework::AnyWidget + 'static,
) -> Option<crate::theatre::ModalHandle> {
    open_menu_surface_at(overlay, anchor, group_id, None, content)
}

/// [`open_menu_surface`], with the panel placed against the button that opened
/// it.
///
/// `placed` is the button's [`crate::theatre::Anchor`] together with the
/// placement to use -- normally
/// [`crate::menu_anchor::MenuLayout::position`] wrapped up. Without it the
/// panel lands at the overlay's origin, which is on top of the button in every
/// arrangement where the button is near the top left, and the panel then eats
/// the presses meant for it.
pub fn open_menu_surface_at(
    overlay: std::rc::Rc<crate::theatre::OverlayHandle>,
    anchor: u64,
    group_id: u64,
    placed: Option<(crate::theatre::Anchor, crate::theatre::Placement)>,
    content: impl Fn() -> crate::framework::AnyWidget + 'static,
) -> Option<crate::theatre::ModalHandle> {
    with_menu_tree_mut(|tree| tree.open(anchor));
    // **A fresh region id, not the anchor's.** The anchor is itself a tap
    // region -- upstream's `RawMenuAnchor` wraps its child in one so that
    // pressing the button does not count as a tap outside the menu it opened
    // -- and handing the panel the same number puts two regions in the
    // registry under one id. The registry keys on it, so "was 8401 hit" then
    // means "was either of them hit", and the two cannot be told apart.
    let region_id = crate::theatre::next_surface_id();
    let shown =
        crate::theatre::show_tap_dismissed_at(overlay, region_id, group_id, placed, content)?;
    note_menu_panel(anchor, &shown);
    shown.on_dismissed(move || {
        forget_menu_panel(anchor);
        with_panels_following(|tree| tree.close(anchor));
    });
    Some(shown)
}

thread_local! {
    /// The panel each open anchor has on screen.
    ///
    /// # Why this had to exist
    ///
    /// The two halves of a menu -- the tree that knows which anchors are open
    /// and the overlay that holds the panels -- were joined in **one
    /// direction only**. A tap outside told the tree
    /// ([`MenuAnchorTree::handle_outside_tap`]), and a button that opened a
    /// panel kept its own handle. But nothing went the other way: closing a
    /// node in the tree left its panel exactly where it was, on screen,
    /// belonging to a menu the tree no longer believed in.
    ///
    /// Nothing noticed, because until now every close came *from* the panel.
    /// Escape does not: it starts at the root and closes downwards, and the
    /// panels it closes are ones nobody else is holding.
    static PANELS: std::cell::RefCell<Vec<(u64, crate::theatre::ModalHandle)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Records the panel `anchor` has on screen.
pub fn note_menu_panel(anchor: u64, shown: &crate::theatre::ModalHandle) {
    PANELS.with(|panels| panels.borrow_mut().push((anchor, shown.clone())));
}

/// Forgets `anchor`'s panel, whatever took it down.
pub fn forget_menu_panel(anchor: u64) {
    PANELS.with(|panels| panels.borrow_mut().retain(|(id, _)| *id != anchor));
}

/// Takes `anchor`'s panel off the screen, if it has one.
///
/// It does **not** remove the entry: dismissing runs the panel's listeners and
/// one of those is [`forget_menu_panel`], so the removal has exactly one
/// place. Removing here as well would be a second copy of the same sequence,
/// and the copy that mattered could then be deleted with every test still
/// green -- every other way a panel comes down (a tap outside, a button being
/// disposed) reaches the listener and never reaches this function.
///
/// The lookup borrows the list and lets go of it before dismissing, because
/// the listener it is about to run borrows the same list.
fn take_panel_down(anchor: u64) {
    let held = PANELS.with(|panels| {
        panels
            .borrow()
            .iter()
            .find(|(id, _)| *id == anchor)
            .map(|(_, held)| held.clone())
    });
    if let Some(held) = held {
        held.dismiss();
    }
}

/// Makes a change to the tree and **takes down the panel of everything that
/// closed**.
///
/// Which anchors closed is read from the tree's own log rather than guessed
/// from the shape of the tree afterwards: by the time the change returns, the
/// children have already been unhooked, so walking the tree would find nothing
/// to take down.
///
/// The panels are dismissed **outside** the tree borrow. A dismissal runs
/// listeners, and one of those listeners changes the tree -- from inside
/// `with_menu_tree_mut` that is a second borrow of the same cell, which is a
/// panic and not a bug you find by reading.
pub fn with_panels_following<R>(change: impl FnOnce(&mut MenuAnchorTree) -> R) -> R {
    let (answer, closed) = with_menu_tree_mut(|tree| {
        let before = tree.log().len();
        let answer = change(tree);
        let closed = tree.log()[before..]
            .iter()
            .filter(|(_, open)| !*open)
            .map(|(id, _)| *id)
            .collect::<Vec<u64>>();
        (answer, closed)
    });
    for anchor in closed {
        take_panel_down(anchor);
    }
    answer
}

/// Upstream's `DismissMenuAction.invoke` reaching all the way: the tree closes
/// from the root and every panel that closed comes off the screen.
pub fn close_menu(anchor: u64) {
    with_panels_following(|tree| tree.dismiss(anchor));
}

/// Empties the panel list, for a test that wants a clean screen.
#[cfg(test)]
pub fn reset_menu_panels() {
    PANELS.with(|panels| panels.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The tree every anchor shares ----------------------------------------

    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::{BoxConstraints, Offset};
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc;

    const BAR: u64 = 9601;
    const SUB: u64 = 9602;
    const GROUP: u64 = 9603;

    fn staged() -> (ElementTree, Rc<crate::theatre::OverlayHandle>) {
        let found: Rc<StdRefCell<Option<Rc<crate::theatre::OverlayHandle>>>> =
            Rc::new(StdRefCell::new(None));
        struct Finder(Rc<StdRefCell<Option<Rc<crate::theatre::OverlayHandle>>>>);
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = crate::theatre::OverlayHandle::of(context);
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(crate::tap_region::TapRegionSurface::new(
            9600,
            crate::theatre::overlay(component(Finder(Rc::clone(&found)))),
        ));
        tree.build_render_tree();
        let handle = found.borrow().clone().expect("a descendant found it");
        (tree, handle)
    }

    fn panel() -> AnyWidget {
        leaf(|| {
            crate::render::RenderAlign::new(
                crate::render::Alignment::TOP_LEFT,
                crate::render::RenderDecoratedBox::new()
                    .with_fill(crate::render::Fill::Solid(crate::engine::Color(
                        0xFF00_00FF,
                    )))
                    .with_child(crate::widgets::SizedBox::new(100.0, 100.0)),
            )
        })
    }

    fn tap(tree: &mut ElementTree, at: Offset) {
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
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

    #[test]
    fn the_tree_is_there_to_be_asked() {
        // Every rule on `MenuAnchorTree` takes `&mut self`, and until now
        // nothing owned one -- so a widget that wanted to ask "did that tap
        // close my submenu" had no tree to ask.
        reset_menu_tree();
        with_menu_tree_mut(|tree| tree.insert(MenuAnchorNode::new(BAR)));
        assert!(!with_menu_tree(|tree| tree.is_open(BAR)));
        with_menu_tree_mut(|tree| tree.open(BAR));
        assert!(with_menu_tree(|tree| tree.is_open(BAR)));
        reset_menu_tree();
        assert!(
            with_menu_tree(|tree| tree.node(BAR).is_none()),
            "and emptying it empties it"
        );
    }

    #[test]
    fn opening_a_menu_opens_its_anchor_in_the_tree() {
        reset_menu_tree();
        with_menu_tree_mut(|tree| tree.insert(MenuAnchorNode::new(BAR)));
        let (mut tree, overlay) = staged();
        let shown = open_menu_surface(overlay, BAR, GROUP, panel).expect("shown");
        tree.rebuild_dirty();
        assert!(with_menu_tree(|tree| tree.is_open(BAR)));
        assert!(shown.is_showing());
        shown.dismiss();
        reset_menu_tree();
    }

    #[test]
    fn a_tap_outside_a_panel_closes_the_anchor_it_belongs_to() {
        // Two answers, not one. The panel goes -- that is the surface's own
        // dismissal -- and in the *tree* the anchor closes, because upstream's
        // panel wraps itself in
        // `TapRegion(onTapOutside: () => anchor._menuController.close())`.
        //
        // It used to close only the children here, which is the rule for the
        // region around the **button** and not for the panel. The anchor was
        // then left believing it was still open, and pressing the button again
        // did nothing at all -- `should_open` asks the tree.
        reset_menu_tree();
        with_menu_tree_mut(|tree| {
            tree.insert(MenuAnchorNode::new(BAR));
            tree.insert(MenuAnchorNode::new(SUB));
            tree.set_parent(SUB, Some(BAR)).expect("a child of the bar");
            tree.open(SUB);
        });

        let (mut tree, overlay) = staged();
        let shown = open_menu_surface(overlay, BAR, GROUP, panel).expect("shown");
        tree.rebuild_dirty();
        assert!(
            with_menu_tree(|tree| tree.is_open(SUB)),
            "the submenu is up"
        );

        tap(&mut tree, Offset::new(400.0, 400.0));
        assert!(!shown.is_showing(), "the panel went");
        assert!(
            !with_menu_tree(|tree| tree.is_open(SUB)),
            "and the submenu went with it"
        );
        assert!(
            !with_menu_tree(|tree| tree.is_open(BAR)),
            "and so did the anchor whose panel it was"
        );
        reset_menu_tree();
    }

    /// A second tap region of the same group, somewhere else on screen: what
    /// a submenu's panel is to the menu it grew from.
    struct GroupMember {
        id: u64,
        group_id: u64,
    }

    impl Component for GroupMember {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            crate::tap_region::TapRegion::new(self.id)
                .with_group_id(self.group_id)
                .build(
                    context,
                    leaf(|| {
                        crate::render::RenderAlign::new(
                            crate::render::Alignment::BOTTOM_RIGHT,
                            crate::render::RenderDecoratedBox::new()
                                .with_fill(crate::render::Fill::Solid(crate::engine::Color(
                                    0xFF00_FF00,
                                )))
                                .with_child(crate::widgets::SizedBox::new(200.0, 200.0)),
                        )
                    }),
                )
        }
    }

    #[test]
    fn a_tap_on_a_sibling_panel_of_the_same_menu_is_not_outside() {
        // The group id the anchor passes down is what ties the panels of one
        // menu together. Give the surface a group of its own and moving from a
        // menu bar to the submenu it just opened would close the submenu on
        // the way.
        reset_menu_tree();
        with_menu_tree_mut(|tree| {
            tree.insert(MenuAnchorNode::new(BAR));
            tree.insert(MenuAnchorNode::new(SUB));
            tree.set_parent(SUB, Some(BAR)).expect("a child of the bar");
            tree.open(SUB);
        });

        let (mut tree, overlay) = staged();
        let shown = open_menu_surface(Rc::clone(&overlay), BAR, GROUP, panel).expect("shown");
        let sibling = overlay
            .insert(|| {
                component(GroupMember {
                    id: 9604,
                    group_id: GROUP,
                })
            })
            .expect("inserted");
        tree.rebuild_dirty();

        tap(&mut tree, Offset::new(700.0, 500.0));
        assert!(shown.is_showing(), "the sibling is in the same menu");
        assert!(with_menu_tree(|tree| tree.is_open(SUB)));
        overlay.remove(sibling);
        shown.dismiss();
        reset_menu_tree();
    }

    #[test]
    fn a_tap_inside_leaves_the_tree_alone() {
        reset_menu_tree();
        with_menu_tree_mut(|tree| {
            tree.insert(MenuAnchorNode::new(BAR));
            tree.insert(MenuAnchorNode::new(SUB));
            tree.set_parent(SUB, Some(BAR)).expect("a child of the bar");
            tree.open(SUB);
        });

        let (mut tree, overlay) = staged();
        let shown = open_menu_surface(overlay, BAR, GROUP, panel).expect("shown");
        tree.rebuild_dirty();

        tap(&mut tree, Offset::new(50.0, 50.0));
        assert!(shown.is_showing(), "the tap landed on the panel");
        assert!(
            with_menu_tree(|tree| tree.is_open(SUB)),
            "and nothing in the tree moved"
        );
        shown.dismiss();
        reset_menu_tree();
    }

    /// A menu bar (1) with two submenus (2, 3), and a sub-submenu (4) under 2.
    fn menu_tree() -> MenuAnchorTree {
        let mut tree = MenuAnchorTree::new();
        for id in 1..=4 {
            tree.insert(MenuAnchorNode::new(id));
        }
        tree.set_parent(2, Some(1)).unwrap();
        tree.set_parent(3, Some(1)).unwrap();
        tree.set_parent(4, Some(2)).unwrap();
        tree
    }

    fn open_all(tree: &mut MenuAnchorTree) {
        for id in [1, 2, 4] {
            tree.open(id);
        }
    }

    // -- The tree ----------------------------------------------------------

    #[test]
    fn the_root_is_whatever_has_no_parent_above_it() {
        let tree = menu_tree();
        assert_eq!(tree.root_of(4), 1);
        assert_eq!(tree.root_of(1), 1);
        assert!(tree.is_root(1));
        assert!(!tree.is_root(2));
    }

    #[test]
    fn moving_an_anchor_takes_it_off_the_old_parent_first() {
        // Otherwise it is a child of two menus, and closing one leaves it half
        // attached.
        let mut tree = menu_tree();
        assert_eq!(tree.node(2).unwrap().children, vec![4]);

        tree.set_parent(4, Some(3)).unwrap();
        assert!(tree.node(2).unwrap().children.is_empty());
        assert_eq!(tree.node(3).unwrap().children, vec![4]);
    }

    #[test]
    fn an_anchor_cannot_be_its_own_parent() {
        let mut tree = menu_tree();
        assert!(tree.set_parent(2, Some(2)).is_err());
    }

    #[test]
    fn re_parenting_to_where_it_already_is_changes_nothing() {
        let mut tree = menu_tree();
        tree.set_parent(4, Some(2)).unwrap();
        assert_eq!(tree.node(2).unwrap().children, vec![4], "not duplicated");
    }

    // -- Which way a close travels -----------------------------------------

    #[test]
    fn a_tap_outside_a_submenu_leaves_the_menu_bar_alone() {
        // A reader who clicked away from a submenu did not ask to lose the bar
        // it hangs off.
        let mut tree = menu_tree();
        open_all(&mut tree);

        tree.handle_outside_tap(2);
        assert!(!tree.is_open(4), "the sub-submenu went");
        assert!(tree.is_open(2), "but the submenu stayed");
        assert!(tree.is_open(1));
    }

    #[test]
    fn escape_closes_from_the_root_however_deep_it_was_pressed() {
        // "I am done with this menu", not "one level, please".
        let mut tree = menu_tree();
        open_all(&mut tree);

        tree.dismiss(4);
        assert!(!tree.is_open(1) && !tree.is_open(2) && !tree.is_open(4));
    }

    #[test]
    fn closing_a_menu_takes_its_submenus_with_it() {
        // A submenu outliving its parent would be a menu floating over
        // nothing.
        let mut tree = menu_tree();
        open_all(&mut tree);

        tree.close(2);
        assert!(!tree.is_open(4));
        assert!(tree.is_open(1), "and the bar is untouched");
    }

    #[test]
    fn the_children_go_before_the_parent_does() {
        let mut tree = menu_tree();
        open_all(&mut tree);
        let before = tree.log().len();

        tree.close(1);
        let closes: Vec<u64> = tree.log()[before..]
            .iter()
            .filter(|(_, open)| !*open)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(closes, vec![4, 2, 1], "innermost first");
    }

    #[test]
    fn shutting_a_child_and_asking_it_to_close_are_different_paths() {
        // A menu that animates out is animating during the request, and the
        // dispose path uses the immediate one because it has no frames left.
        let mut tree = menu_tree();
        open_all(&mut tree);
        tree.close_children(1, CloseKind::Immediate);
        assert_eq!(tree.node(2).unwrap().close_requests, 0);

        open_all(&mut tree);
        tree.close_children(1, CloseKind::Requested);
        assert_eq!(
            tree.node(2).unwrap().close_requests,
            1,
            "the closing sequence ran"
        );
    }

    #[test]
    fn a_menu_whose_close_is_still_animating_is_not_yet_closed() {
        let mut tree = menu_tree();
        open_all(&mut tree);
        tree.handle_close_request_deferred(2);
        assert_eq!(tree.node(2).unwrap().close_requests, 1);
        assert!(tree.is_open(2), "the sequence started and has not finished");
    }

    #[test]
    fn a_tap_outside_a_closed_menu_does_nothing_at_all() {
        let mut tree = menu_tree();
        assert!(!tree.handle_outside_tap(2));
    }

    #[test]
    fn the_tap_that_closed_a_menu_still_reaches_what_it_landed_on() {
        // Which is usually what the reader meant by tapping there.
        let mut tree = menu_tree();
        open_all(&mut tree);
        assert!(!tree.handle_outside_tap(2), "not consumed, by default");

        let mut greedy = MenuAnchorTree::new();
        greedy.insert(MenuAnchorNode::new(1));
        let mut swallowing = MenuAnchorNode::new(2);
        swallowing.consume_outside_taps = true;
        greedy.insert(swallowing);
        greedy.set_parent(2, Some(1)).unwrap();
        greedy.open(1);
        greedy.open(2);
        assert!(greedy.handle_outside_tap(2));
    }

    // -- The two automatic closes ------------------------------------------

    #[test]
    fn only_the_root_closes_when_an_ancestor_scrolls() {
        // Upstream: don't close on *any* scroll, or a menu too big for the
        // view would close itself the moment the reader scrolled it.
        let mut tree = menu_tree();
        open_all(&mut tree);

        assert!(!tree.handle_ancestor_scroll(2), "a submenu ignores it");
        assert!(tree.is_open(2));

        assert!(tree.handle_ancestor_scroll(1));
        assert!(!tree.is_open(1) && !tree.is_open(2));
    }

    #[test]
    fn a_closed_root_has_nothing_to_close_on_a_scroll() {
        let mut tree = menu_tree();
        assert!(!tree.handle_ancestor_scroll(1));
    }

    #[test]
    fn a_view_that_changed_size_leaves_the_menu_positioned_against_nothing() {
        // There is no way to know where the anchor moved to until the next
        // layout, so closing is the honest answer.
        let mut tree = menu_tree();
        open_all(&mut tree);
        let old = Size {
            width: 400.0,
            height: 800.0,
        };
        let new = Size {
            width: 800.0,
            height: 400.0,
        };
        assert!(tree.handle_view_size_change(1, Some(old), new));
        assert!(!tree.is_open(1));
    }

    #[test]
    fn the_first_time_a_size_is_seen_it_is_only_recorded() {
        // Or every menu would close on the frame it opened.
        let mut tree = menu_tree();
        open_all(&mut tree);
        let size = Size {
            width: 400.0,
            height: 800.0,
        };
        assert!(!tree.handle_view_size_change(1, None, size));
        assert!(tree.is_open(1));

        assert!(
            !tree.handle_view_size_change(1, Some(size), size),
            "and the same size again is not a change"
        );
    }

    #[test]
    fn a_submenu_does_not_close_itself_when_the_view_resizes() {
        // The root will close, and take it along.
        let mut tree = menu_tree();
        open_all(&mut tree);
        let old = Size {
            width: 400.0,
            height: 800.0,
        };
        let new = Size {
            width: 800.0,
            height: 400.0,
        };
        assert!(!tree.handle_view_size_change(2, Some(old), new));
    }

    // -- Rebuild propagation ------------------------------------------------

    #[test]
    fn an_open_state_change_travels_up_to_the_root() {
        // An ancestor that paints differently while a descendant is open finds
        // out before anyone rebuilds.
        let mut tree = menu_tree();
        tree.open(4);
        assert_eq!(tree.node(4).unwrap().dirty_marks, 1);
        assert_eq!(tree.node(2).unwrap().dirty_marks, 1);
        assert_eq!(tree.node(1).unwrap().dirty_marks, 1);
        assert_eq!(tree.node(3).unwrap().dirty_marks, 0, "not a sibling");
    }

    #[test]
    fn a_change_arriving_mid_build_is_deferred_rather_than_marking_dirty() {
        // Marking during a build is the error this avoids; deferring costs one
        // frame of a menu drawn in its old state.
        let mut tree = menu_tree();
        tree.child_changed_open_state(4, true);
        assert_eq!(tree.node(4).unwrap().dirty_marks, 0);
        assert_eq!(tree.node(4).unwrap().deferred_marks, 1);
        assert_eq!(tree.node(1).unwrap().deferred_marks, 1, "all the way up");
    }

    #[test]
    fn opening_something_already_open_says_nothing() {
        let mut tree = menu_tree();
        tree.open(1);
        let marks = tree.node(1).unwrap().dirty_marks;
        tree.open(1);
        assert_eq!(tree.node(1).unwrap().dirty_marks, marks);
        assert_eq!(tree.log().len(), 1);
    }

    #[test]
    fn closing_something_already_closed_says_nothing() {
        let mut tree = menu_tree();
        tree.close(1);
        assert!(tree.log().is_empty());
    }

    // -- The controller ----------------------------------------------------

    #[test]
    fn a_controller_detaches_only_from_the_anchor_it_is_on() {
        // An anchor disposed after the controller moved on must not tear it
        // off its new one.
        let mut controller = MenuController::new();
        controller.attach(1);
        controller.attach(2);

        controller.detach(1);
        assert_eq!(controller.anchor(), Some(2), "the old anchor's dispose");

        controller.detach(2);
        assert_eq!(controller.anchor(), None);
    }

    #[test]
    fn closing_an_unattached_controller_is_harmless_where_opening_is_not() {
        // Closing a menu that is already gone is what a dispose path does, and
        // it should be allowed to say so.
        let mut tree = menu_tree();
        let controller = MenuController::new();
        controller.close(&mut tree);
        assert!(tree.log().is_empty());
    }

    #[test]
    #[should_panic(expected = "MenuController is not attached")]
    fn opening_a_menu_nobody_built_is_a_programming_error() {
        let mut tree = menu_tree();
        MenuController::new().open(&mut tree);
    }

    #[test]
    fn close_children_leaves_the_menu_itself_open() {
        let mut tree = menu_tree();
        open_all(&mut tree);
        let mut controller = MenuController::new();
        controller.attach(2);

        controller.close_children(&mut tree);
        assert!(!tree.is_open(4));
        assert!(tree.is_open(2));
    }

    #[test]
    fn reading_the_controller_and_watching_it_are_different_questions() {
        // A menu item holding a controller to call close() should not rebuild
        // every time any menu opens.
        assert!(!ControllerLookup::WithoutDependency.establishes_dependency());
        assert!(ControllerLookup::WithDependency.establishes_dependency());
    }

    // -- The group and the dismiss action ----------------------------------

    #[test]
    fn a_menu_bar_is_open_when_any_of_its_children_is() {
        // Which is how it can host submenus without itself being a menu that
        // could be dismissed.
        let mut tree = menu_tree();
        assert!(!RawMenuAnchorGroup::is_open(&tree, 1));

        tree.open(3);
        assert!(RawMenuAnchorGroup::is_open(&tree, 1));

        tree.close(3);
        assert!(!RawMenuAnchorGroup::is_open(&tree, 1));
    }

    #[test]
    fn escape_with_no_menu_open_reaches_whatever_else_wanted_it() {
        // A dialog, usually.
        let unattached = DismissMenuAction::new(MenuController::new());
        assert!(!unattached.is_enabled());

        let mut controller = MenuController::new();
        controller.attach(4);
        assert!(DismissMenuAction::new(controller).is_enabled());
    }

    #[test]
    fn the_dismiss_action_closes_the_whole_system() {
        let mut tree = menu_tree();
        open_all(&mut tree);
        let mut controller = MenuController::new();
        controller.attach(4);

        DismissMenuAction::new(controller).invoke(&mut tree);
        assert!(!tree.is_open(1));
    }

    // -- The anchor widget --------------------------------------------------

    #[test]
    fn the_default_open_request_shows_the_overlay_at_once() {
        let mut shown = false;
        RawMenuAnchor::default_on_open_requested(|| shown = true);
        assert!(shown);
    }

    #[test]
    fn a_delayed_opener_whose_menu_went_away_announces_nothing() {
        // Calling showOverlay after disposal is a no-op and must not trigger
        // onOpen.
        let anchor = RawMenuAnchor::new();
        assert!(anchor.show_overlay(false));
        assert!(!anchor.show_overlay(true));
    }

    #[test]
    fn disposing_an_anchor_closes_it_and_unhooks_it_from_its_parent() {
        let mut tree = menu_tree();
        open_all(&mut tree);

        tree.dispose(2);
        assert!(tree.node(2).is_none());
        assert_eq!(
            tree.node(1).unwrap().children,
            vec![3],
            "and the parent forgot it"
        );
        assert!(!tree.is_open(4), "its children went with it");
    }

    #[test]
    fn overlay_info_carries_the_position_the_caller_asked_to_open_at() {
        let info = RawMenuOverlayInfo::new(
            AnchorRect {
                left: 10.0,
                top: 20.0,
                right: 110.0,
                bottom: 44.0,
            },
            Size {
                width: 400.0,
                height: 800.0,
            },
            7,
        );
        assert_eq!(info.position, None);

        let placed = info.clone().with_position(Offset { dx: 4.0, dy: 8.0 });
        assert_eq!(placed.position, Some(Offset { dx: 4.0, dy: 8.0 }));
        assert_ne!(placed, info);
    }
}
