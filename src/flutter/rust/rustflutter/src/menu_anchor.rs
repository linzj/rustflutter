//! Menus anchored to a widget, and the underlined letter in their labels --
//! a port of upstream's `material/menu_anchor.dart`.
//!
//! The piece with the most judgement in it is the smallest:
//! [`MenuAcceleratorLabel::strip_accelerator_markers`], which turns
//! `"&Save As..."` into `"Save As..."` and says the S is the accelerator. It
//! has to answer several questions a first attempt would not think to ask --
//! what `&&` means, what `& ` means, what a trailing `&` means, and what the
//! index refers to once the markers have been taken out.
//!
//! ## What is not here
//!
//! [`MenuAnchor`] and [`SubmenuButton`] put their menus in an `OverlayPortal`
//! and drive them with a route-aware controller; this crate has neither. What
//! is ported is the configuration those widgets carry and the accelerator
//! machinery, which is self-contained.

use crate::render::Offset;

/// Upstream `MenuAcceleratorCallbackBinding`: how a label tells the button
/// above it that its letter was pressed.
///
/// The `has_submenu` flag rides along because a menu **item** and a menu that
/// *opens* another menu do different things when their letter is pressed: the
/// first is invoked and the menu closes, the second opens its submenu and
/// stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MenuAcceleratorCallbackBinding {
    pub has_on_invoke: bool,
    pub has_submenu: bool,
}

impl MenuAcceleratorCallbackBinding {
    pub fn new(has_on_invoke: bool, has_submenu: bool) -> MenuAcceleratorCallbackBinding {
        MenuAcceleratorCallbackBinding {
            has_on_invoke,
            has_submenu,
        }
    }

    /// Upstream's `updateShouldNotify`.
    pub fn update_should_notify(&self, old: &MenuAcceleratorCallbackBinding) -> bool {
        self.has_on_invoke != old.has_on_invoke || self.has_submenu != old.has_submenu
    }
}

/// Upstream `MenuAcceleratorLabel`: a label with one letter marked as its
/// keyboard accelerator.
pub struct MenuAcceleratorLabel {
    /// The label as written, markers and all.
    pub label: String,
}

impl MenuAcceleratorLabel {
    pub fn new(label: impl Into<String>) -> MenuAcceleratorLabel {
        MenuAcceleratorLabel {
            label: label.into(),
        }
    }

    /// Upstream's `displayLabel`: what a reader sees.
    pub fn display_label(&self) -> String {
        Self::strip_accelerator_markers(&self.label).0
    }

    /// Upstream's `hasAccelerator`, whose regular expression is
    /// `&(?!([&\s]|$))` -- an ampersand **not** followed by another ampersand,
    /// by whitespace, or by the end of the string. All three exclusions are
    /// the same idea from different directions: those are the ampersands that
    /// mean a literal ampersand rather than a marker.
    ///
    /// **Derived from the stripping here rather than written twice.** Upstream
    /// has a regular expression and a loop that must agree about the same
    /// rule, and they very nearly do not: the regex matches `&x` anywhere,
    /// while the loop only sets an index for the *first* eligible marker and
    /// skips the character after any marker. For every label the two agree,
    /// because a second marker being ineligible does not stop the first from
    /// being found -- but keeping one implementation removes the question.
    pub fn has_accelerator(&self) -> bool {
        Self::strip_accelerator_markers(&self.label).1.is_some()
    }

    /// Upstream's `stripAcceleratorMarkers`.
    ///
    /// Returns the label to show and the index, **into the stripped string**,
    /// of the accelerator character. The rules, each of which upstream's
    /// implementation earns a comment for:
    ///
    /// * `&&` is a literal ampersand and does **not** mark an accelerator, so
    ///   a label like `"Search && Replace"` shows one ampersand and has none.
    /// * `&` before whitespace marks nothing either -- there is no letter
    ///   there to underline.
    /// * a bare `&` at the very end is **stripped**, not shown. Upstream's
    ///   comment calls it "just treated as a quoted ampersand", but the code
    ///   breaks out of the loop without writing it, so it disappears. Ported as
    ///   written; see the regression line.
    /// * only the **first** eligible marker counts. A second `&Letter` is
    ///   stripped like the first but does not move the index.
    /// * and the index is reduced by the number of quoted ampersands seen
    ///   before it, because it has to index the *stripped* string rather than
    ///   the original.
    pub fn strip_accelerator_markers(label: &str) -> (String, Option<usize>) {
        let characters: Vec<char> = label.chars().collect();
        let mut display = String::new();
        let mut accelerator_index: Option<usize> = None;
        let mut quoted_ampersands = 0usize;
        let mut last_was_ampersand = false;

        for (index, character) in characters.iter().enumerate() {
            if last_was_ampersand {
                last_was_ampersand = false;
                display.push(*character);
                continue;
            }
            if *character != '&' {
                display.push(*character);
                continue;
            }
            if index == characters.len() - 1 {
                // A bare ampersand at the end is dropped.
                break;
            }
            last_was_ampersand = true;
            let next = characters[index + 1];
            if accelerator_index.is_none() && next != '&' && !next.is_whitespace() {
                accelerator_index = Some(index - quoted_ampersands);
            }
            quoted_ampersands += 1;
        }
        (display, accelerator_index)
    }
}

// -- Opening and closing, and the three ways to read the same four states -----

/// Upstream `_MenuAnchorState.isClosing`: the menu is **running its close
/// animation right now**.
///
/// Only `reverse`. A menu that has finished closing is not closing -- it is
/// closed, and that is a different answer to a different question. See
/// [`is_closing_or_closed`] for the other one.
pub fn is_closing(status: crate::animation::AnimationStatus) -> bool {
    status == crate::animation::AnimationStatus::Reverse
}

/// Upstream `_MenuAnchorState.isClosingOrClosed`: `dismissed` or `reverse`.
///
/// This is exactly the complement of
/// [`crate::animation::AnimationStatus::is_forward_or_completed`], written out
/// upstream as its own switch because the menu code reads better asking it
/// this way round.
pub fn is_closing_or_closed(status: crate::animation::AnimationStatus) -> bool {
    !status.is_forward_or_completed()
}

/// What an open request does, from upstream's `_handleMenuOpenRequest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuOpenRequest {
    /// Whether the overlay is put up. Upstream calls `showOverlay()`
    /// **before** looking at the animation at all.
    pub shows_overlay: bool,
    /// Whether the open animation is started. Skipped for a menu already open
    /// or already opening.
    pub starts_animation: bool,
}

/// Upstream's `_handleMenuOpenRequest`.
///
/// # A parent that is *closing* blocks the child; a parent that is *closed*
/// does not
///
/// The guard is `_parent?.isClosing ?? false`, and it is the narrow predicate
/// -- `reverse` alone, not [`is_closing_or_closed`]. That reads backwards for
/// a moment: surely a parent that is entirely shut is worse than one still
/// half on screen? But the comment says what it is for -- "if this menu's
/// parent is closing, submenus should not open. This prevents a submenu
/// calling `MenuController.open()` after a parent menu has started closing."
/// It is a **race**, not a state check. A closing parent is on its way to
/// taking the child down with it, so a child opening now would flash and
/// vanish. A dismissed parent is just a menu, and whatever is opening the
/// child will open it too.
///
/// # The overlay goes up even when the animation does not
///
/// `showOverlay()` runs unconditionally, then the animation is skipped for a
/// menu that is already forward or completed. Folding the two together --
/// returning early before showing the overlay -- would be the natural
/// simplification and would lose the case where the entry was taken down
/// while the animation stayed at its end.
///
/// # A closing menu re-opens rather than counting as open
///
/// `reverse` is not forward-or-completed, so a menu caught mid-close is sent
/// `forward()` from wherever it got to. Asking "is it visible?" instead would
/// have said yes and left it closing.
pub fn menu_open_request(
    parent_status: Option<crate::animation::AnimationStatus>,
    status: crate::animation::AnimationStatus,
) -> MenuOpenRequest {
    if parent_status.is_some_and(is_closing) {
        return MenuOpenRequest {
            shows_overlay: false,
            starts_animation: false,
        };
    }
    MenuOpenRequest {
        shows_overlay: true,
        starts_animation: !status.is_forward_or_completed(),
    }
}

/// Upstream's `_handleMenuCloseRequest`: whether to run the close animation
/// and, when it finishes, take the overlay down.
///
/// The mirror of the open guard, and the mirror matters. A menu **already
/// closing** is left alone: restarting the reverse would jump it back to full
/// size, and `whenComplete(hideOverlay)` would be armed a second time. A menu
/// already **closed** is likewise left alone -- there is no overlay left to
/// hide, and reversing from zero animates nothing.
pub fn menu_close_request(status: crate::animation::AnimationStatus) -> bool {
    status.is_forward_or_completed()
}

/// Upstream `MenuStyle`'s alignment offset and the flags a menu carries --
/// the configuration half of [`MenuAnchor`].
#[derive(Clone)]
pub struct MenuAnchor {
    /// Upstream's `alignmentOffset`.
    pub alignment_offset: Offset,
    /// Upstream's `consumeOutsideTap`.
    ///
    /// Whether a tap that closes the menu is also delivered to whatever was
    /// under it. False by default, and the default is the considered one: a
    /// reader dismissing a menu by tapping a button usually means only to
    /// dismiss it.
    pub consume_outside_tap: bool,
    /// Upstream's deprecated `anchorTapClosesMenu`.
    ///
    /// Kept because upstream kept it. The deprecation notice points at
    /// `consumeOutsideTap`, which answers a wider question -- this one was
    /// only ever about a tap on the anchor itself.
    pub anchor_tap_closes_menu: bool,
    /// Upstream's `crossAxisUnconstrained`, true by default: a submenu is
    /// allowed to be wider than the space beside its parent, because a menu
    /// item wrapped onto two lines is worse than one that overhangs.
    pub cross_axis_unconstrained: bool,
    /// Upstream's `useRootOverlay`.
    pub use_root_overlay: bool,
    /// Upstream's `animated`.
    pub animated: bool,
    /// Identifies this anchor: its node in the menu tree, its tap region, and
    /// the key its opener is filed under.
    pub id: u64,
    /// The tap-region group this anchor and its panel share, so that a press
    /// on the thing that opened the menu is not a press *outside* it.
    pub group_id: u64,
    /// Upstream's `menuChildren`, as the panel they are laid into.
    pub menu: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    /// Upstream's `builder(context, controller, child)`: the widget that marks
    /// the place, built with the controller that opens the menu.
    ///
    /// The controller is handed over rather than looked up because a caller's
    /// button is *outside* the anchor -- upstream's builder has the same
    /// problem and the same answer.
    pub child: Option<
        std::rc::Rc<dyn Fn(crate::raw_menu_anchor::MenuController) -> crate::framework::AnyWidget>,
    >,
    /// Upstream's `onOpen`, called when the menu goes up.
    pub on_open: Option<std::rc::Rc<dyn Fn()>>,
    /// Upstream's `onClose`, called when it comes down **however** it comes
    /// down -- a tap outside and Escape are not the caller's doing, and a
    /// caller who only heard about their own closes would miss both.
    pub on_close: Option<std::rc::Rc<dyn Fn()>>,
}

impl Default for MenuAnchor {
    fn default() -> MenuAnchor {
        MenuAnchor::new()
    }
}

/// The closures are compared the only way closures can be: by whether there is
/// one. Same idiom as [`SubmenuButton`]'s.
impl PartialEq for MenuAnchor {
    fn eq(&self, other: &MenuAnchor) -> bool {
        self.alignment_offset == other.alignment_offset
            && self.consume_outside_tap == other.consume_outside_tap
            && self.anchor_tap_closes_menu == other.anchor_tap_closes_menu
            && self.cross_axis_unconstrained == other.cross_axis_unconstrained
            && self.use_root_overlay == other.use_root_overlay
            && self.animated == other.animated
            && self.id == other.id
            && self.group_id == other.group_id
            && self.menu.is_some() == other.menu.is_some()
            && self.child.is_some() == other.child.is_some()
            && self.on_open.is_some() == other.on_open.is_some()
            && self.on_close.is_some() == other.on_close.is_some()
    }
}

impl std::fmt::Debug for MenuAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuAnchor")
            .field("id", &self.id)
            .field("group_id", &self.group_id)
            .field("alignment_offset", &self.alignment_offset)
            .field("consume_outside_tap", &self.consume_outside_tap)
            .field("anchor_tap_closes_menu", &self.anchor_tap_closes_menu)
            .field("cross_axis_unconstrained", &self.cross_axis_unconstrained)
            .field("use_root_overlay", &self.use_root_overlay)
            .field("animated", &self.animated)
            .field("has_menu", &self.menu.is_some())
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl MenuAnchor {
    pub fn new() -> MenuAnchor {
        MenuAnchor {
            alignment_offset: Offset::ZERO,
            consume_outside_tap: false,
            anchor_tap_closes_menu: false,
            cross_axis_unconstrained: true,
            use_root_overlay: false,
            animated: false,
            id: 0,
            group_id: 0,
            menu: None,
            child: None,
            on_open: None,
            on_close: None,
        }
    }

    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn with_group_id(mut self, group_id: u64) -> Self {
        self.group_id = group_id;
        self
    }

    /// The panel this anchor opens.
    pub fn with_menu(mut self, menu: impl Fn() -> crate::framework::AnyWidget + 'static) -> Self {
        self.menu = Some(std::rc::Rc::new(menu));
        self
    }

    /// Upstream's `builder`. See [`MenuAnchor::child`].
    pub fn with_child(
        mut self,
        child: impl Fn(crate::raw_menu_anchor::MenuController) -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.child = Some(std::rc::Rc::new(child));
        self
    }

    pub fn with_on_open(mut self, on_open: impl Fn() + 'static) -> Self {
        self.on_open = Some(std::rc::Rc::new(on_open));
        self
    }

    pub fn with_on_close(mut self, on_close: impl Fn() + 'static) -> Self {
        self.on_close = Some(std::rc::Rc::new(on_close));
        self
    }

    pub fn with_alignment_offset(mut self, offset: Offset) -> Self {
        self.alignment_offset = offset;
        self
    }

    pub fn with_consume_outside_tap(mut self, consume: bool) -> Self {
        self.consume_outside_tap = consume;
        self
    }

    pub fn with_cross_axis_unconstrained(mut self, unconstrained: bool) -> Self {
        self.cross_axis_unconstrained = unconstrained;
        self
    }

    pub fn with_animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }

    /// This anchor's panel, resolved. An anchored menu is the vertical case,
    /// which is what makes `MenuTheme` the one consulted.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        style: Option<&crate::component_themes::MenuStyle>,
    ) -> crate::component_themes::ResolvedMenuPanel {
        crate::component_themes::ResolvedMenuPanel::of(
            context,
            crate::component_themes::MenuPanelAxis::Vertical,
            style,
        )
    }
}

/// What a [`MenuAnchor`] keeps.
#[derive(Clone)]
pub struct MenuAnchorState {
    id: u64,
    /// Upstream's `_internalMenuController`, attached to this anchor. It is
    /// what a caller's button is handed, and what Escape closes from.
    controller: crate::raw_menu_anchor::MenuController,
    /// Where the anchor is on screen, filled in by its own assemble and read
    /// when the panel is placed -- the moment the question can be answered.
    anchor: crate::theatre::Anchor,
    open: Option<crate::theatre::ModalHandle>,
}

impl Default for MenuAnchorState {
    fn default() -> MenuAnchorState {
        MenuAnchorState {
            id: 0,
            controller: crate::raw_menu_anchor::MenuController::new(),
            anchor: crate::theatre::Anchor::new(),
            open: None,
        }
    }
}

impl crate::framework::StatefulComponent for MenuAnchor {
    type State = MenuAnchorState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    /// Upstream's `initState`: the anchor joins the tree and gets a controller
    /// of its own.
    fn initial_state(&self) -> MenuAnchorState {
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            if tree.node(self.id).is_none() {
                let mut node = crate::raw_menu_anchor::MenuAnchorNode::new(self.id);
                node.consume_outside_taps = self.consume_outside_tap;
                node.use_root_overlay = self.use_root_overlay;
                tree.insert(node);
            }
        });
        let mut controller = crate::raw_menu_anchor::MenuController::new();
        controller.attach(self.id);
        MenuAnchorState {
            id: self.id,
            controller,
            anchor: crate::theatre::Anchor::new(),
            open: None,
        }
    }

    /// Upstream's `dispose`. The opener goes too: a closure left behind holds
    /// the overlay handle of a page that is gone, and a controller somebody
    /// kept would open a menu into it.
    fn dispose(&self, state: &mut MenuAnchorState) {
        if let Some(open) = state.open.take() {
            open.dismiss();
        }
        crate::raw_menu_anchor::forget_menu_opener(state.id);
        state.controller.detach(state.id);
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.dispose(state.id));
    }

    fn build(
        &self,
        state: &MenuAnchorState,
        handle: crate::framework::StateHandle<MenuAnchorState>,
        context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        let overlay = crate::theatre::OverlayHandle::of(context);
        let themes = context.capture_themes();
        let id = self.id;
        let group_id = self.group_id;
        let menu = self.menu.clone();
        let on_open = self.on_open.clone();
        let on_close = self.on_close.clone();
        let anchor = state.anchor.clone();
        // A menu hangs from the anchor's bottom-left corner and runs down.
        // `parent_orientation` is vertical: an anchor standing on its own is
        // not an entry of a bar, so a panel that will not fit flips to the
        // other side of it rather than sliding to the screen's edge.
        let layout = MenuLayout {
            anchor_rect: crate::engine::Rect::ltrb(0.0, 0.0, 0.0, 0.0),
            alignment_offset: self.alignment_offset,
            orientation: MenuAxis::Vertical,
            parent_orientation: MenuAxis::Vertical,
            direction: crate::direction::current_direction(),
            directional_alignment: true,
        };
        let place: crate::theatre::Placement = std::rc::Rc::new(move |rect, child, overlay| {
            MenuLayout {
                anchor_rect: rect,
                ..layout
            }
            .position(
                crate::render::Alignment::BOTTOM_LEFT,
                child,
                crate::engine::Rect::xywh(0.0, 0.0, overlay.width, overlay.height),
            )
        });

        let opener: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
            // Already open, or nothing to open. A second panel would be a
            // second entry in the overlay with nothing holding its handle, so
            // nothing could ever take it down.
            if crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(id)) {
                return;
            }
            let (Some(overlay), Some(menu)) = (overlay.clone(), menu.clone()) else {
                return;
            };
            let themes = themes.clone();
            let opened = crate::raw_menu_anchor::open_menu_surface_at(
                overlay,
                id,
                group_id,
                Some((anchor.clone(), std::rc::Rc::clone(&place))),
                move || themes.wrap(menu()),
            );
            if let Some(opened) = &opened {
                if let Some(on_close) = on_close.clone() {
                    // On the panel's own dismissal, so that a tap outside and
                    // an Escape are heard as well as a caller's own close.
                    opened.on_dismissed(move || on_close());
                }
                if let Some(on_open) = &on_open {
                    on_open();
                }
            }
            handle.set_state(move |state| state.open = opened);
        });
        // Filed under the anchor's id **on every build**: the closure above
        // captured this build's overlay handle and themes, and the one from
        // the last build captured the last page's.
        crate::raw_menu_anchor::note_menu_opener(self.id, std::rc::Rc::clone(&opener));

        let child = match &self.child {
            Some(child) => child(state.controller),
            None => crate::framework::leaf(|| crate::widgets::SizedBox::new(0.0, 0.0)),
        };
        // Recorded from the anchor's own assemble, which is where its render
        // object first exists and is the rectangle the panel is placed
        // against.
        let recording = state.anchor.clone();
        let marked = crate::framework::many(vec![child], move |rendered| {
            let child = rendered.into_iter().next().expect("the child");
            recording.set(child.clone());
            crate::theatre::RenderPortal::new(child)
        });
        // The anchor is in its own menu's tap-region group, so that pressing
        // the thing that opened the menu is not a press outside it -- upstream
        // wraps `buildAnchor` in exactly this.
        //
        // It swallows the tap that dismissed the menu only when asked to and
        // only **while the menu is open** -- upstream's
        // `consumeOutsideTaps: root.isOpen && widget.consumeOutsideTap`. An
        // anchor that swallowed presses while shut would be a hole in the page
        // the shape of a closed menu.
        let region = crate::tap_region::TapRegion::new(self.id)
            .with_group_id(self.group_id)
            .with_consume_outside_taps(
                self.consume_outside_tap
                    && crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(self.id)),
            );
        region.build(context, marked)
    }
}

/// Upstream `MenuBar`: a row of menus along the top of a window.
///
/// Upstream's `clipBehavior` defaults to `Clip.none` here where
/// [`MenuAnchor`]'s defaults to `hardEdge`, and the difference is the point: a
/// bar's menus are *meant* to hang below it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MenuBar {
    /// Identifies the bar's own node in the menu tree. Every entry hangs under
    /// it, which is what makes them siblings -- and siblings are what the
    /// hover rule is about.
    pub id: u64,
    pub clip: bool,
    /// The tap-region group the whole bar and its panels share, so that a tap
    /// on one entry is not a tap *outside* the panel another entry opened.
    pub group_id: u64,
    /// The bar's top-level menus, in order across.
    pub entries: Vec<SubmenuButton>,
}

impl MenuBar {
    pub fn new() -> MenuBar {
        MenuBar {
            id: 0,
            clip: false,
            group_id: 0,
            entries: Vec::new(),
        }
    }

    pub fn with_clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn with_group_id(mut self, group_id: u64) -> Self {
        self.group_id = group_id;
        self
    }

    /// Adds one top-level menu.
    pub fn push(mut self, entry: SubmenuButton) -> Self {
        self.entries.push(entry);
        self
    }

    /// One entry, as the bar builds it.
    ///
    /// Three things the bar knows and the entry does not, all of them settled
    /// here so a caller cannot half-assemble a bar: it hangs under the bar, it
    /// shares the bar's tap-region group, and the menu it sits in runs
    /// **across**.
    pub fn entry(&self, at: usize) -> Option<SubmenuButton> {
        self.entries.get(at).map(|entry| {
            entry
                .clone()
                .under(self.id)
                .with_group_id(self.group_id)
                .in_a_bar(true)
        })
    }

    /// This bar's panel, resolved. A bar is the horizontal case, which is what
    /// makes `MenuBarTheme` the one consulted -- see
    /// [`crate::component_themes::ResolvedMenuPanel`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        style: Option<&crate::component_themes::MenuStyle>,
    ) -> crate::component_themes::ResolvedMenuPanel {
        crate::component_themes::ResolvedMenuPanel::of(
            context,
            crate::component_themes::MenuPanelAxis::Horizontal,
            style,
        )
    }
}

/// What the bar keeps: its own id, so that `dispose` knows which node to take
/// out of the tree, and the two maps Escape travels through.
///
/// The maps are **made once**, in `initial_state`, and not rebuilt: an
/// actions scope compares by identity ([`crate::actions::ActionsScope`]), so a
/// fresh dispatcher every build would be a fresh scope every build, and
/// everything that depends on it would rebuild for ever.
#[derive(Clone)]
pub struct MenuBarState {
    id: u64,
    /// Upstream's `_menuController`, attached to the bar. It is what
    /// [`crate::raw_menu_anchor::DismissMenuAction`] closes from, and what
    /// makes the action *disabled* while there is no bar -- an Escape with no
    /// menu open belongs to whatever is above, usually a dialog.
    controller: crate::raw_menu_anchor::MenuController,
    dispatcher: std::rc::Rc<crate::actions::ActionDispatcher>,
    registry: std::rc::Rc<crate::shortcuts::ShortcutRegistry>,
}

/// An empty one, for the framework's sake. Every field is filled in by
/// `initial_state`, which is the only place a bar's state is ever made.
impl Default for MenuBarState {
    fn default() -> MenuBarState {
        MenuBarState {
            id: 0,
            controller: crate::raw_menu_anchor::MenuController::new(),
            dispatcher: std::rc::Rc::new(crate::actions::ActionDispatcher::new()),
            registry: std::rc::Rc::new(crate::shortcuts::ShortcutRegistry::new()),
        }
    }
}

impl MenuBar {
    /// Upstream's `_kMenuTraversalShortcuts`, as far as this crate's intents
    /// reach.
    ///
    /// Escape is here. Tab and shift-Tab are `NextFocusIntent` and
    /// `PreviousFocusIntent`, which exist. The four arrows are
    /// `DirectionalFocusIntent`, which does not exist yet -- there is no
    /// direction in [`crate::actions::Intent`] to carry, so they are left out
    /// rather than mapped to something that means a different thing.
    pub fn traversal_shortcuts() -> crate::shortcuts::ShortcutRegistry {
        crate::shortcuts::ShortcutRegistry::new()
            .with(
                crate::shortcuts::ShortcutActivator::KeySet(
                    crate::shortcuts::LogicalKeySet::single(crate::keyboard::LogicalKey::ESCAPE.0),
                ),
                crate::actions::Intent::Dismiss,
            )
            .with(
                crate::shortcuts::ShortcutActivator::KeySet(
                    crate::shortcuts::LogicalKeySet::single(crate::keyboard::LogicalKey::TAB.0),
                ),
                crate::actions::Intent::NextFocus,
            )
    }
}

impl crate::framework::StatefulComponent for MenuBar {
    type State = MenuBarState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    /// The bar joins the menu tree, once. Upstream's `_MenuBarAnchorState` is
    /// a `_MenuAnchorState` like any other: it is in the tree, it just never
    /// opens.
    fn initial_state(&self) -> MenuBarState {
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            if tree.node(self.id).is_none() {
                tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(self.id));
            }
        });
        let mut controller = crate::raw_menu_anchor::MenuController::new();
        controller.attach(self.id);
        let action = crate::raw_menu_anchor::DismissMenuAction::new(controller);
        MenuBarState {
            id: self.id,
            controller,
            dispatcher: std::rc::Rc::new(crate::actions::ActionDispatcher::new().with_action(
                "Dismiss",
                crate::actions::Action {
                    on_invoke: std::rc::Rc::new(move |_intent| {
                        action.dismiss_the_menus();
                        None
                    }),
                    is_enabled: std::rc::Rc::new(move |_intent| action.is_enabled()),
                    consumes_key: true,
                },
            )),
            registry: std::rc::Rc::new(MenuBar::traversal_shortcuts()),
        }
    }

    fn dispose(&self, state: &mut MenuBarState) {
        state.controller.detach(state.id);
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.dispose(state.id));
    }

    fn build(
        &self,
        state: &MenuBarState,
        _handle: crate::framework::StateHandle<MenuBarState>,
        context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        let panel = self.resolved(context, None);
        let entries: Vec<crate::framework::AnyWidget> = (0..self.entries.len())
            .filter_map(|at| self.entry(at))
            .map(crate::framework::stateful)
            .collect();
        let row = crate::framework::many(entries, move |rendered| {
            // A row, because that is what a bar is -- and it is the same fact
            // the entries were told as `MenuAxis::Horizontal`, which is why
            // their panels slide to the screen's edge instead of flipping to
            // the other side of the button.
            let mut row = crate::render::RenderFlex::row()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Center);
            for child in rendered {
                row = row.push(child);
            }
            let mut bar = crate::widgets::Container::new()
                .with_padding(panel.padding)
                .with_child(row);
            if let Some(background) = panel.background_color {
                bar = bar.with_color(background);
            }
            bar
        });
        // Upstream's `Actions(actions: {DismissIntent: DismissMenuAction})`
        // around `Shortcuts(shortcuts: _kMenuTraversalShortcuts)`, in that
        // order: the shortcut turns the key into an intent and then looks
        // **upwards** for something that serves it, so the actions scope has
        // to be the outer one.
        crate::actions::Actions::scope(
            std::rc::Rc::clone(&state.dispatcher),
            crate::shortcuts::shortcuts(self.id, std::rc::Rc::clone(&state.registry), row),
        )
    }
}

/// Upstream `MenuItemButton`: one line of a menu.
#[derive(Clone)]
pub struct MenuItemButton {
    /// Identifies this line's ink and its focus node.
    pub id: u64,
    /// The words on the line.
    pub label: String,
    /// Upstream's `shortcut`, already spelled for the reader --
    /// `_LocalizedShortcutLabeler` is a table this crate has not ported, so
    /// what arrives is the label rather than the activator.
    pub shortcut: Option<String>,
    /// The menu tree group this line belongs to, the same one its panel and
    /// the button that opened it use.
    ///
    /// Without it a press on the line is a tap **outside** the panel the line
    /// is in, so the panel closes on the way down and the press arrives at a
    /// menu that is already gone.
    pub group_id: u64,
    /// The anchor this line sits in, which is what
    /// [`MenuItemButton::close_on_activate`] closes from.
    ///
    /// Upstream reads it out of the tree -- `_MenuAnchorState._maybeOf(context)`
    /// -- and this crate has no inherited lookup for the menu tree, so the
    /// caller says which anchor. `None` means the line closes nothing, which
    /// is right for a line that is not in a menu at all.
    pub anchor_id: Option<u64>,
    leading: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    trailing: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    on_pressed: Option<std::rc::Rc<dyn Fn()>>,
    on_hover: Option<std::rc::Rc<dyn Fn(bool)>>,
    /// Upstream's `requestFocusOnHover`, **true** by default.
    ///
    /// This port had it false. A pointer moving down a menu carries the focus
    /// with it upstream, so the item under the cursor is the one a keyboard
    /// would act on -- and with the default inverted, moving the mouse and
    /// then pressing Enter acted on whatever the keyboard had left behind.
    pub request_focus_on_hover: bool,
    /// Upstream's `closeOnActivate`, true by default: pressing an item is
    /// normally the end of the interaction.
    pub close_on_activate: bool,
    pub enabled: bool,
}

impl std::fmt::Debug for MenuItemButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuItemButton")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("shortcut", &self.shortcut)
            .field("group_id", &self.group_id)
            .field("anchor_id", &self.anchor_id)
            .field("request_focus_on_hover", &self.request_focus_on_hover)
            .field("close_on_activate", &self.close_on_activate)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// The parts a reader can see. The callbacks are left out for the reason
/// [`crate::search_anchor::SearchBar`]'s are: a closure made afresh each build
/// is never equal to the last one.
impl PartialEq for MenuItemButton {
    fn eq(&self, other: &MenuItemButton) -> bool {
        self.id == other.id
            && self.label == other.label
            && self.shortcut == other.shortcut
            && self.group_id == other.group_id
            && self.anchor_id == other.anchor_id
            && self.leading.is_some() == other.leading.is_some()
            && self.trailing.is_some() == other.trailing.is_some()
            && self.request_focus_on_hover == other.request_focus_on_hover
            && self.close_on_activate == other.close_on_activate
            && self.enabled == other.enabled
    }
}

impl Default for MenuItemButton {
    fn default() -> MenuItemButton {
        MenuItemButton::new()
    }
}

/// What a menu line remembers between builds: which states it is in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuItemButtonState {
    pub states: crate::widget_state::WidgetStates,
}

impl MenuItemButton {
    /// This line's appearance, with `MenuButtonTheme` and the M3 defaults
    /// folded in -- see [`crate::component_themes::ResolvedMenuButton`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        states: crate::widget_state::WidgetStates,
    ) -> crate::component_themes::ResolvedMenuButton {
        crate::component_themes::ResolvedMenuButton::of(context, states)
    }

    pub fn new() -> MenuItemButton {
        MenuItemButton {
            id: 0,
            label: String::new(),
            shortcut: None,
            group_id: 0,
            anchor_id: None,
            leading: None,
            trailing: None,
            on_pressed: None,
            on_hover: None,
            request_focus_on_hover: true,
            close_on_activate: true,
            enabled: true,
        }
    }

    /// A line with an id of its own, which its ink and its focus node share.
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// The menu this line belongs to: its tap-region group and the anchor a
    /// press closes from. See the two fields.
    pub fn in_menu(mut self, anchor_id: u64, group_id: u64) -> Self {
        self.anchor_id = Some(anchor_id);
        self.group_id = group_id;
        self
    }

    pub fn with_leading(
        mut self,
        leading: impl Fn() -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.leading = Some(std::rc::Rc::new(leading));
        self
    }

    pub fn with_trailing(
        mut self,
        trailing: impl Fn() -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.trailing = Some(std::rc::Rc::new(trailing));
        self
    }

    /// Upstream's `onPressed`, which is also what decides `enabled` there.
    /// Kept apart here for the reason [`crate::ink_well::InkResponse`] keeps
    /// them apart.
    pub fn with_on_pressed(mut self, pressed: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(std::rc::Rc::new(pressed));
        self
    }

    /// Upstream's `onHover`, told when the pointer arrives and leaves.
    ///
    /// Upstream hangs it on `MouseRegion.onHover` rather than `onEnter`, and
    /// says why: *"onEnter and TextButton.onHover are called if a button is
    /// hovered after scrolling. This interferes with focus traversal and
    /// scroll position."* A list that scrolled under a still pointer would
    /// otherwise move the focus by itself.
    pub fn with_on_hover(mut self, hover: impl Fn(bool) + 'static) -> Self {
        self.on_hover = Some(std::rc::Rc::new(hover));
        self
    }

    pub fn with_close_on_activate(mut self, close: bool) -> Self {
        self.close_on_activate = close;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The line this item lays out: upstream's `_MenuItemLabel` with
    /// `hasSubmenu: false`.
    ///
    /// `horizontal` is the anchor's orientation, which upstream reads off the
    /// enclosing anchor rather than off the item -- a line does not know
    /// whether it is in a bar until it is in one.
    pub fn label(
        &self,
        leading: bool,
        trailing: bool,
        shortcut: bool,
        horizontal: bool,
    ) -> MenuItemLabel {
        MenuItemLabel::new()
            .with_leading_icon(leading)
            .with_trailing_icon(trailing)
            .with_shortcut(shortcut)
            .in_a_horizontal_bar(horizontal)
    }
}

impl crate::framework::StatefulComponent for MenuItemButton {
    type State = MenuItemButtonState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &MenuItemButtonState,
        handle: crate::framework::StateHandle<MenuItemButtonState>,
        context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        use crate::widget_state::WidgetState;

        let states = if self.enabled {
            state.states
        } else {
            state.states.with(WidgetState::Disabled)
        };
        let resolved = self.resolved(context, states);
        // Upstream hands the whole `overlayColor` property to the `InkWell`.
        // Resolved here one state at a time, because the ink takes a colour
        // per highlight rather than a property -- the same shape
        // [`crate::search_anchor::SearchBar`] uses.
        let pressed = self
            .resolved(context, states.with(WidgetState::Pressed))
            .overlay;
        let hovered = self
            .resolved(context, states.with(WidgetState::Hovered))
            .overlay;
        let focused = self
            .resolved(context, states.with(WidgetState::Focused))
            .overlay;
        let density = crate::theme::ThemeData::of(context).visual_density;

        let label_style = crate::engine::TextStyle {
            color: resolved.foreground,
            ..crate::components::theme_of(context).body()
        };
        // The shortcut is written in the same colour as the label. Upstream
        // wraps it in nothing at all -- it inherits the button's foreground --
        // which is worth saying because a shortcut in a quieter colour is a
        // common design and **not** what Material does here.
        let shortcut_style = label_style.clone();

        let layout = self.label(
            self.leading.is_some(),
            self.trailing.is_some(),
            self.shortcut.is_some(),
            false,
        );
        let gap = MenuItemLabel::spacing(density);
        let leading_gap = layout.leading_gap(density);
        let parts = layout.trailing_parts();

        let leading = self.leading.clone();
        let trailing = self.trailing.clone();
        let label = self.label.clone();
        let shortcut = self.shortcut.clone();
        let minimum = resolved.minimum_size;
        let alignment = resolved.alignment;
        let direction = crate::direction::current_direction();

        let row = move || {
            let mut children: Vec<crate::framework::AnyWidget> = Vec::new();
            if let Some(leading) = &leading {
                children.push(leading());
            }
            let text = label.clone();
            let style = label_style.clone();
            children.push(crate::framework::leaf(move || {
                crate::render::RenderParagraph::new(text.clone()).with_style(style.clone())
            }));
            if let Some(trailing) = &trailing {
                children.push(trailing());
            }
            if let Some(shortcut) = &shortcut {
                let shortcut = shortcut.clone();
                let style = shortcut_style.clone();
                children.push(crate::framework::leaf(move || {
                    crate::render::RenderParagraph::new(shortcut.clone()).with_style(style.clone())
                }));
            }
            let parts = parts.clone();
            crate::framework::many(children, move |rendered| {
                let mut rendered = rendered.into_iter();
                let mut row = crate::render::RenderFlex::row()
                    .with_main_axis_size(crate::render::MainAxisSize::Min)
                    .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Center);
                if leading_gap > 0.0 {
                    row = row.push(rendered.next().expect("the leading widget"));
                    row = row.push(crate::widgets::SizedBox::new(leading_gap, 0.0));
                }
                row = row.push(rendered.next().expect("the label"));
                // One gap before each trailing part, in the order upstream
                // builds them -- see [`MenuItemLabel::trailing_parts`].
                for _ in &parts {
                    row = row.push(crate::widgets::SizedBox::new(gap, 0.0));
                    row = row.push(rendered.next().expect("a trailing part"));
                }
                crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                    min_width: minimum.width,
                    max_width: f32::INFINITY,
                    min_height: minimum.height,
                    max_height: f32::INFINITY,
                })
                // Shrink-wrapped on both axes, then held to the minimum by
                // the box around it. A plain `Align` fills whatever it is
                // offered, so a menu line in a loose column would be as tall
                // as the column -- the alignment is there for the case the
                // *minimum* is bigger than the row, which is a short label in
                // a 64-wide button sitting at the start rather than centred.
                .with_child(
                    crate::render::RenderAlign::new(alignment.resolve(direction), row)
                        .with_factors(Some(1.0), Some(1.0)),
                )
            })
        };

        let on_pressed = self.on_pressed.clone();
        let on_hover = self.on_hover.clone();
        let id = self.id;
        let request_focus_on_hover = self.request_focus_on_hover;
        let closes = self.close_on_activate.then_some(self.anchor_id).flatten();
        let region = crate::tap_region::TapRegion::new(self.id).with_group_id(self.group_id);
        let line = crate::framework::stateful(
            crate::ink_well::InkResponse::new(self.id, row)
                .with_contained(true)
                .with_enabled(self.enabled)
                .with_focus(self.id)
                .with_highlight_color(pressed)
                .with_hover_color(hovered)
                .with_focus_color(focused)
                .with_on_hover({
                    let handle = handle.clone();
                    move |hovering| {
                        // Upstream's `requestFocusOnHover`, **true** by
                        // default: the pointer carries the keyboard with it, so
                        // the line under the cursor is the one Enter acts on.
                        // With it off, moving the mouse and then pressing Enter
                        // acts on whatever the keyboard was left on.
                        if hovering && request_focus_on_hover {
                            crate::focus::focus(id);
                        }
                        if let Some(on_hover) = &on_hover {
                            on_hover(hovering);
                        }
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
                .with_on_focus_change({
                    let handle = handle.clone();
                    move |focused| {
                        handle.set_state(move |state| {
                            state.states.update(WidgetState::Focused, focused);
                        });
                    }
                })
                .with_on_tap(move || {
                    if let Some(on_pressed) = &on_pressed {
                        on_pressed();
                    }
                    // Upstream's `_handleSelect`: `_anchor?._root
                    // ._menuController.close()`. **The root**, not this level
                    // -- choosing an item is the end of the whole
                    // interaction, not of one panel of it, which is why it
                    // reaches for the same place Escape does.
                    //
                    // After the callback, not before: upstream runs
                    // `widget.onPressed?.call()` first, and a handler that
                    // wanted to look at the menu it was chosen from would
                    // otherwise find it gone.
                    if let Some(anchor) = closes {
                        // The panels too, not only the tree: see
                        // [`crate::raw_menu_anchor::close_menu`].
                        crate::raw_menu_anchor::close_menu(anchor);
                    }
                }),
        );
        region.build(context, line)
    }
}

/// Upstream `CheckboxMenuButton`: a menu item with a checkbox in its leading
/// slot.
///
/// Its `value` is tri-state for the same reason a checkbox's is: with
/// `tristate` set, `None` is a real third value rather than a missing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CheckboxMenuButton {
    pub value: Option<bool>,
    pub tristate: bool,
    pub enabled: bool,
}

impl CheckboxMenuButton {
    pub fn new(value: Option<bool>) -> CheckboxMenuButton {
        CheckboxMenuButton {
            value,
            tristate: false,
            enabled: true,
        }
    }

    pub fn with_tristate(mut self, tristate: bool) -> Self {
        self.tristate = tristate;
        self
    }

    /// Upstream's `onChanged` cycle, which a checkbox menu item shares with
    /// [`crate::controls::Checkbox`]: false, true, and -- only when tristate
    /// -- null.
    pub fn next_value(&self) -> Option<bool> {
        match (self.value, self.tristate) {
            (Some(false), _) => Some(true),
            (Some(true), true) => None,
            (Some(true), false) => Some(false),
            (None, _) => Some(false),
        }
    }
}

/// Upstream `RadioMenuButton`: a menu item with a radio in its leading slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadioMenuButton<T> {
    pub value: T,
    pub group_value: Option<T>,
    /// Upstream's `toggleable`, false by default: a radio in a group is not
    /// normally allowed to be turned off by pressing it again, because the
    /// group is meant to have an answer.
    pub toggleable: bool,
    pub enabled: bool,
}

impl<T: PartialEq + Copy> RadioMenuButton<T> {
    pub fn new(value: T) -> RadioMenuButton<T> {
        RadioMenuButton {
            value,
            group_value: None,
            toggleable: false,
            enabled: true,
        }
    }

    pub fn with_group_value(mut self, group_value: T) -> Self {
        self.group_value = Some(group_value);
        self
    }

    pub fn with_toggleable(mut self, toggleable: bool) -> Self {
        self.toggleable = toggleable;
        self
    }

    pub fn is_selected(&self) -> bool {
        self.group_value == Some(self.value)
    }

    /// What pressing this radio sets the group to.
    ///
    /// Pressing the one already selected clears the group **only** when
    /// toggleable; otherwise it stays where it is, which is what keeps a
    /// required choice required.
    pub fn next_group_value(&self) -> Option<T> {
        if self.is_selected() && self.toggleable {
            None
        } else {
            Some(self.value)
        }
    }
}

/// Upstream `SubmenuButton`: a menu item that opens another menu.
#[derive(Clone)]
pub struct SubmenuButton {
    /// Identifies this button's ink, its focus node **and its node in the menu
    /// tree**. One number, because they are one thing: the anchor a submenu
    /// hangs off is the button the reader pressed.
    pub id: u64,
    pub label: String,
    pub alignment_offset: Offset,
    /// Upstream's `submenuIcon` slot being present at all is what makes a
    /// submenu look different from an item.
    pub has_submenu_icon: bool,
    pub enabled: bool,
    /// The menu tree group this button's panels belong to -- what
    /// [`crate::theatre::show_tap_dismissed`] uses to tell a tap on a sibling
    /// panel from a tap outside the whole menu.
    pub group_id: u64,
    /// The anchor this button hangs under in the menu tree -- its **parent**.
    ///
    /// Upstream gets this from the element tree
    /// (`_MenuAnchorState._maybeOf(context)`); this crate has no inherited
    /// lookup for the menu tree, so the parent says. `None` is a button that
    /// is its own root.
    ///
    /// It matters for more than bookkeeping: the hover rule asks about the
    /// **root**, and a button with no parent *is* the root -- so an entry that
    /// never joined its bar would ask about itself and answer wrongly.
    pub parent_anchor: Option<u64>,
    /// Which way the menu this button sits in runs: `Horizontal` for an entry
    /// of a menu **bar**, `Vertical` for a line of a panel.
    ///
    /// It decides two different things, which is why it is one field and not
    /// two: where this button's own panel is placed (see [`MenuLayout`]) and
    /// whether hovering opens it (see
    /// [`SubmenuButton::opens_on_hover`]).
    pub parent_orientation: MenuAxis,
    /// The panel this button opens. `None` opens nothing, which is a button
    /// that looks like a submenu and is not one -- worth being able to build,
    /// and worth being obvious.
    menu: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
    leading: Option<std::rc::Rc<dyn Fn() -> crate::framework::AnyWidget>>,
}

impl std::fmt::Debug for SubmenuButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmenuButton")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("alignment_offset", &self.alignment_offset)
            .field("has_submenu_icon", &self.has_submenu_icon)
            .field("enabled", &self.enabled)
            .field("group_id", &self.group_id)
            .field("parent_anchor", &self.parent_anchor)
            .field("parent_orientation", &self.parent_orientation)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SubmenuButton {
    fn eq(&self, other: &SubmenuButton) -> bool {
        self.id == other.id
            && self.label == other.label
            && self.alignment_offset == other.alignment_offset
            && self.has_submenu_icon == other.has_submenu_icon
            && self.enabled == other.enabled
            && self.group_id == other.group_id
            && self.parent_anchor == other.parent_anchor
            && self.parent_orientation == other.parent_orientation
            && self.menu.is_some() == other.menu.is_some()
            && self.leading.is_some() == other.leading.is_some()
    }
}

impl Default for SubmenuButton {
    fn default() -> SubmenuButton {
        SubmenuButton::new()
    }
}

/// What a submenu button keeps between builds.
///
/// # The node goes into the tree once, not once a frame
///
/// [`crate::raw_menu_anchor::MenuAnchorTree::insert`] asserts that an anchor is
/// added once -- an anchor added twice would be two nodes under one id and
/// closing one of them would leave the other half attached. A `build` runs
/// every frame, so the insert cannot live there; it lives here, in
/// `initial_state`, which is upstream's `initState`. Coming out again is
/// `dispose`, upstream's.
pub struct SubmenuButtonState {
    id: u64,
    open: Option<crate::theatre::ModalHandle>,
    states: crate::widget_state::WidgetStates,
    /// Where the button is. Filled in from its own assemble -- the moment its
    /// render object exists -- and read when the panel is placed, which is the
    /// moment the question can be answered.
    anchor: crate::theatre::Anchor,
}

impl Default for SubmenuButtonState {
    fn default() -> SubmenuButtonState {
        SubmenuButtonState {
            id: 0,
            open: None,
            states: crate::widget_state::WidgetStates::NONE,
            anchor: crate::theatre::Anchor::new(),
        }
    }
}

impl SubmenuButton {
    /// This line's appearance, with `MenuButtonTheme` and the M3 defaults
    /// folded in -- see [`crate::component_themes::ResolvedMenuButton`].
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        states: crate::widget_state::WidgetStates,
    ) -> crate::component_themes::ResolvedMenuButton {
        crate::component_themes::ResolvedMenuButton::of(context, states)
    }

    pub fn new() -> SubmenuButton {
        SubmenuButton {
            id: 0,
            label: String::new(),
            alignment_offset: Offset::ZERO,
            has_submenu_icon: true,
            enabled: true,
            group_id: 0,
            parent_anchor: None,
            parent_orientation: MenuAxis::Vertical,
            menu: None,
            leading: None,
        }
    }

    /// Hangs this button under `anchor` in the menu tree. See
    /// [`SubmenuButton::parent_anchor`].
    pub fn under(mut self, anchor: u64) -> Self {
        self.parent_anchor = Some(anchor);
        self
    }

    /// Marks this button as an entry of a menu **bar**. See
    /// [`SubmenuButton::parent_orientation`].
    pub fn in_a_bar(mut self, in_a_bar: bool) -> Self {
        self.parent_orientation = if in_a_bar {
            MenuAxis::Horizontal
        } else {
            MenuAxis::Vertical
        };
        self
    }

    /// Whether the pointer arriving on this button should open its menu.
    ///
    /// Upstream's rule, quoted from `handlePointerHover`:
    ///
    /// > Don't open the root menu bar menus on hover unless a sibling menu is
    /// > already open. This means that the user has to first click to open a
    /// > menu on the menu bar before hovering allows them to traverse it.
    ///
    /// So a menu bar is **inert to the pointer until somebody clicks it**, and
    /// afterwards the whole bar tracks the pointer. Without the first half, a
    /// pointer crossing the top of a window on its way somewhere else would
    /// drop menus open behind it.
    ///
    /// A button inside a panel has no such condition: its parent menu is by
    /// definition already open, or the button would not be on screen.
    ///
    /// Upstream asks `root._menuController.isOpen`, and the root of a menu bar
    /// is a **group** ([`crate::raw_menu_anchor::RawMenuAnchorGroup`]): a bar
    /// is never itself open, so the question "is the root open" is really "is
    /// any entry of the bar open". Asking the bar's own flag instead would
    /// answer *no* forever, and a real menu bar would never track the pointer
    /// at all.
    pub fn opens_on_hover(&self) -> bool {
        if self.parent_orientation != MenuAxis::Horizontal {
            return true;
        }
        crate::raw_menu_anchor::with_menu_tree(|tree| {
            crate::raw_menu_anchor::RawMenuAnchorGroup::is_open(tree, tree.root_of(self.id))
        })
    }

    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The menu tree group this button's panels belong to. See the field.
    pub fn with_group_id(mut self, group_id: u64) -> Self {
        self.group_id = group_id;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_leading(
        mut self,
        leading: impl Fn() -> crate::framework::AnyWidget + 'static,
    ) -> Self {
        self.leading = Some(std::rc::Rc::new(leading));
        self
    }

    /// Upstream's `menuChildren`, as one panel rather than a list -- the
    /// arranging is the panel's, and this crate has
    /// [`crate::component_themes::ResolvedMenuPanel`] for that.
    pub fn with_menu(mut self, menu: impl Fn() -> crate::framework::AnyWidget + 'static) -> Self {
        self.menu = Some(std::rc::Rc::new(menu));
        self
    }

    /// Whether pressing this button should open anything: upstream's `_open`,
    /// which returns early for a disabled anchor and for one that is already
    /// open.
    ///
    /// A method rather than three lines inside the tap handler, because a
    /// handler built in a `build` cannot be asked anything from outside. The
    /// third condition in particular is invisible from a test that only taps:
    /// what a second press does depends on tap regions, overlay stacking and
    /// hit order, and a test that got any of those wrong would pass while
    /// proving nothing.
    pub fn should_open(&self) -> bool {
        self.enabled
            && self.menu.is_some()
            && !crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(self.id))
    }

    /// The line this button draws: a menu item's, with the arrow.
    ///
    /// Built out of [`MenuItemButton`] so that the two cannot drift. Upstream
    /// has them share `_MenuItemLabel` and `_MenuButtonDefaultsM3` for the
    /// same reason -- a submenu line that looked different from an item line
    /// in the same panel would read as a different kind of thing.
    pub fn line(&self) -> MenuItemButton {
        let mut line = MenuItemButton::new()
            .with_id(self.id)
            .with_label(self.label.clone())
            .with_enabled(self.enabled);
        // The same tap-region group, so that pressing the button is not a tap
        // outside the panel it opened. **No anchor**, though: a line with one
        // closes the menu when it is chosen, and a submenu button is the one
        // line in a menu that does the opposite.
        line.group_id = self.group_id;
        if let Some(leading) = &self.leading {
            let leading = std::rc::Rc::clone(leading);
            line = line.with_leading(move || leading());
        }
        line
    }

    pub fn with_alignment_offset(mut self, offset: Offset) -> Self {
        self.alignment_offset = offset;
        self
    }

    /// The line this submenu lays out: the same `_MenuItemLabel` an item
    /// builds, with `hasSubmenu: true`.
    ///
    /// So the arrow is a **trailing part like any other** and takes the same
    /// gap -- and in a horizontal bar it is suppressed with the shortcut,
    /// which is why a menu bar's top-level entries are bare words even though
    /// every one of them opens a submenu.
    pub fn label(
        &self,
        leading: bool,
        trailing: bool,
        shortcut: bool,
        horizontal: bool,
    ) -> MenuItemLabel {
        let mut label = MenuItemLabel::new()
            .with_leading_icon(leading)
            .with_trailing_icon(trailing)
            .with_shortcut(shortcut)
            .in_a_horizontal_bar(horizontal);
        label.has_submenu = self.has_submenu_icon;
        label
    }

    /// The binding a submenu publishes to its label: it has a submenu, so its
    /// accelerator opens rather than invokes.
    pub fn accelerator_binding(&self) -> MenuAcceleratorCallbackBinding {
        MenuAcceleratorCallbackBinding::new(self.enabled, true)
    }
}

/// How a menu line lays its parts out: upstream's `_MenuItemLabel`.
///
/// Kept apart from the buttons for the reason every other rule in this crate
/// is: it can be asked without building anything, and what it answers is a
/// number a test can hold. Both [`MenuItemButton`] and [`SubmenuButton`] build
/// one -- upstream's `_MenuItemLabel` is shared by them in exactly the same
/// way `_MenuButtonDefaultsM3` is.
///
/// # One spacing, and only where two things meet
///
/// Upstream computes a single `horizontalPadding` and spends it in four
/// places: before the label (**only when there is a leading icon**), before
/// the trailing icon, before the shortcut, and before the submenu arrow. There
/// is none at the outer edges -- the button's own padding does that -- so a
/// line with no leading icon starts its text exactly where a line with one
/// starts its icon, and a column of menu items has one left edge rather than
/// two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuItemLabel {
    pub has_leading_icon: bool,
    pub has_trailing_icon: bool,
    pub has_shortcut: bool,
    pub has_submenu: bool,
    /// Upstream's `showDecoration`, false for a line in a horizontal menu bar:
    /// a bar's items show neither their shortcut nor a submenu arrow, because
    /// a bar is a row of words and either would turn it into a table.
    pub show_decoration: bool,
}

impl MenuItemLabel {
    /// Upstream's `_kLabelItemDefaultSpacing`.
    pub const DEFAULT_SPACING: f32 = 12.0;
    /// Upstream's `_kLabelItemMinSpacing`, the floor a negative density cannot
    /// push through.
    pub const MIN_SPACING: f32 = 4.0;

    pub fn new() -> MenuItemLabel {
        MenuItemLabel {
            has_leading_icon: false,
            has_trailing_icon: false,
            has_shortcut: false,
            has_submenu: false,
            show_decoration: true,
        }
    }

    pub fn with_leading_icon(mut self, has: bool) -> Self {
        self.has_leading_icon = has;
        self
    }

    pub fn with_trailing_icon(mut self, has: bool) -> Self {
        self.has_trailing_icon = has;
        self
    }

    pub fn with_shortcut(mut self, has: bool) -> Self {
        self.has_shortcut = has;
        self
    }

    /// Upstream's `showDecoration`, which
    /// [`MenuItemLabel::in_a_horizontal_bar`] names the case for.
    pub fn with_decoration(mut self, show: bool) -> Self {
        self.show_decoration = show;
        self
    }

    /// A line in a menu bar: upstream passes `showDecoration: _orientation ==
    /// Axis.vertical`, so a horizontal bar suppresses both decorations.
    pub fn in_a_horizontal_bar(mut self, horizontal: bool) -> Self {
        self.show_decoration = !horizontal;
        self
    }

    /// Upstream's `horizontalPadding`:
    /// `math.max(_kLabelItemMinSpacing, _kLabelItemDefaultSpacing + density.horizontal * 2)`.
    ///
    /// **Twice the density**, not once. A denser menu closes the gaps between
    /// a line's parts at twice the rate the density itself moves, which is how
    /// a compact menu stays readable while getting smaller: the vertical
    /// squeeze comes from the button's minimum size and the horizontal one
    /// from here.
    ///
    /// The floor is the half worth stating. At the minimum density of -4 the
    /// arithmetic gives `12 - 8 = 4`, exactly the floor -- so the floor is not
    /// reachable from below by any legal density, and it is there to stop the
    /// gap going negative if either constant ever moves.
    pub fn spacing(density: crate::theme::VisualDensity) -> f32 {
        (MenuItemLabel::DEFAULT_SPACING + density.horizontal * 2.0).max(MenuItemLabel::MIN_SPACING)
    }

    /// The gap before the label, which exists only when something is in front
    /// of it.
    pub fn leading_gap(&self, density: crate::theme::VisualDensity) -> f32 {
        if self.has_leading_icon {
            MenuItemLabel::spacing(density)
        } else {
            0.0
        }
    }

    /// What follows the label, in the order upstream builds it: the trailing
    /// icon, then the shortcut, then the submenu arrow, each preceded by the
    /// same gap.
    ///
    /// The two decorations are **suppressed together** and the trailing icon is
    /// not: a caller who put an icon there asked for it, where the shortcut and
    /// the arrow are the menu's own furniture.
    pub fn trailing_parts(&self) -> Vec<MenuItemPart> {
        let mut parts = Vec::new();
        if self.has_trailing_icon {
            parts.push(MenuItemPart::TrailingIcon);
        }
        if self.show_decoration && self.has_shortcut {
            parts.push(MenuItemPart::Shortcut);
        }
        if self.show_decoration && self.has_submenu {
            parts.push(MenuItemPart::SubmenuIcon);
        }
        parts
    }

    /// How wide the line's gaps come to altogether: one before the label when
    /// there is a leading icon, and one before each trailing part.
    ///
    /// A line's own width is its parts plus this, which is what a menu panel
    /// needs in order to be as wide as its widest line.
    pub fn total_gaps(&self, density: crate::theme::VisualDensity) -> f32 {
        let spacing = MenuItemLabel::spacing(density);
        self.leading_gap(density) + spacing * self.trailing_parts().len() as f32
    }
}

impl Default for MenuItemLabel {
    fn default() -> MenuItemLabel {
        MenuItemLabel::new()
    }
}

/// One of the things that can sit after a menu line's label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItemPart {
    TrailingIcon,
    Shortcut,
    SubmenuIcon,
}

impl crate::framework::StatefulComponent for SubmenuButton {
    type State = SubmenuButtonState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    /// Upstream's `initState`: the anchor joins the tree once. See
    /// [`SubmenuButtonState`].
    fn initial_state(&self) -> SubmenuButtonState {
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            if tree.node(self.id).is_none() {
                tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(self.id));
            }
            // Upstream's `didChangeDependencies`, which re-parents the anchor
            // once it can see the tree above it. Here the parent is told to
            // the button, and this is the first moment its node exists to
            // hang.
            if let Some(parent) = self.parent_anchor {
                let _ = tree.set_parent(self.id, Some(parent));
            }
        });
        SubmenuButtonState {
            id: self.id,
            open: None,
            states: crate::widget_state::WidgetStates::NONE,
            anchor: crate::theatre::Anchor::new(),
        }
    }

    /// Upstream's `dispose`, which takes the anchor out of the tree. A node
    /// left behind is an anchor the tree still believes in: Escape would reach
    /// for a root that is not on screen, and a later button with the same id
    /// would trip the "added once" assert.
    fn dispose(&self, state: &mut SubmenuButtonState) {
        if let Some(open) = state.open.take() {
            open.dismiss();
        }
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.dispose(state.id));
    }

    fn build(
        &self,
        state: &SubmenuButtonState,
        handle: crate::framework::StateHandle<SubmenuButtonState>,
        context: &mut crate::framework::BuildContext,
    ) -> crate::framework::AnyWidget {
        let overlay = crate::theatre::OverlayHandle::of(context);
        let themes = context.capture_themes();
        let id = self.id;
        let group_id = self.group_id;
        let menu = self.menu.clone();
        let asking = self.clone();
        let anchor = state.anchor.clone();
        // The panel hangs from the button's bottom-left corner and runs down.
        // Whether its parent runs across or down is the button's own
        // `parent_orientation`, and it is what turns the "try the other side"
        // flip into a slide -- see [`MenuLayout`].
        let layout = MenuLayout {
            anchor_rect: crate::engine::Rect::ltrb(0.0, 0.0, 0.0, 0.0),
            alignment_offset: self.alignment_offset,
            orientation: MenuAxis::Vertical,
            parent_orientation: self.parent_orientation,
            direction: crate::direction::current_direction(),
            directional_alignment: true,
        };
        let place: crate::theatre::Placement = {
            let anchor_for_place = state.anchor.clone();
            std::rc::Rc::new(move |rect, child, overlay| {
                let _ = &anchor_for_place;
                MenuLayout {
                    anchor_rect: rect,
                    ..layout
                }
                .position(
                    crate::render::Alignment::BOTTOM_LEFT,
                    child,
                    crate::engine::Rect::xywh(0.0, 0.0, overlay.width, overlay.height),
                )
            })
        };

        // The arrow is a trailing part of the line, in the slot
        // `MenuItemLabel` reserved for it -- see
        // [`MenuItemLabel::trailing_parts`]. Drawn as the text upstream draws
        // an icon into, because this crate has no icon font yet: what matters
        // for the layout is that something occupies the slot and takes the
        // gap.
        let mut line = self.line();
        if self.has_submenu_icon {
            line = line.with_trailing(|| {
                crate::framework::leaf(|| crate::render::RenderParagraph::new("\u{25B8}"))
            });
        }

        // **The button is in its own menu's tap-region group.** Upstream's
        // `RawMenuAnchor` wraps its child in a `TapRegion` with the same group
        // id its panels use, and the reason shows up the moment the button is
        // pressed a second time: without it the button is *outside* the panel
        // it opened, so pressing it closes the panel on the way down and the
        // "already open" guard below can never be reached.
        let recording = state.anchor.clone();
        let open: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
            // Disabled, no menu, or already open -- see
            // [`SubmenuButton::should_open`]. A second panel would be a second
            // entry in the overlay with nothing holding its handle, so nothing
            // could ever take it down.
            if !asking.should_open() {
                return;
            }
            let Some(overlay) = overlay.clone() else {
                return;
            };
            let Some(menu) = menu.clone() else {
                return;
            };
            let themes = themes.clone();
            let opened = crate::raw_menu_anchor::open_menu_surface_at(
                overlay,
                id,
                group_id,
                Some((anchor.clone(), std::rc::Rc::clone(&place))),
                move || themes.wrap(menu()),
            );
            handle.set_state(move |state| state.open = opened);
        });
        let hovering = std::rc::Rc::clone(&open);
        let asking_hover = self.clone();
        let pressed = crate::framework::stateful(
            line.with_on_pressed({
                let open = std::rc::Rc::clone(&open);
                move || open()
            })
            .with_on_hover(move |entered| {
                // Upstream's `handlePointerHover`: the pointer opens the
                // menu it arrives on, **except** on a bar nobody has
                // clicked yet -- see [`SubmenuButton::opens_on_hover`].
                if entered && asking_hover.opens_on_hover() {
                    hovering();
                }
            }),
        );
        // Recorded from the button's own assemble, which is where its render
        // object first exists and is the rectangle the panel is placed against.
        crate::framework::many(vec![pressed], move |rendered| {
            let button = rendered.into_iter().next().expect("the button");
            recording.set(button.clone());
            crate::theatre::RenderPortal::new(button)
        })
    }
}

/// Which way a menu's lines run, which is also which way its *parent's* ran:
/// upstream's `Axis` on `_MenuLayout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAxis {
    /// A menu bar: its entries sit side by side.
    Horizontal,
    /// A panel: its lines are stacked.
    Vertical,
}

/// Where a menu panel goes: upstream's `_MenuLayout._positionChild`.
///
/// # The ideal place, and then four ways of not fitting
///
/// The wanted position is a point on the anchor -- `alignment.withinRect(
/// anchorRect)` -- shifted by `alignmentOffset`. Everything after that is the
/// panel not fitting, and upstream's answers are worth stating because two of
/// them are not what a first attempt would write:
///
/// * **A panel too wide for the screen is put at the left edge**, not
///   centred and not shrunk. As much of it as will fit is shown, from the
///   start, because a menu is read from its leading edge.
/// * **Off one side, it tries the other side of the button first** -- a
///   submenu that will not fit to the right of its parent opens to the left of
///   it -- and only slides along the edge if that fails too. Sliding first
///   would leave the panel overlapping the button it came from.
/// * **Except when the parent runs the other way.** A panel hanging off a menu
///   *bar* has no "other side of the button" worth trying: the bar is
///   horizontal and the panel is vertical, so upstream pushes it along instead.
///   That is the `parentOrientation != orientation` arm, and it is the one
///   that reads like a special case and is not.
/// * **`alignmentOffset.dy` is subtracted only when moving up past a
///   horizontal parent.** Everywhere else the flip is exact; here the gap the
///   caller asked for below the bar has to be re-applied above it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuLayout {
    /// The button the panel hangs off, in the overlay's coordinates.
    pub anchor_rect: crate::engine::Rect,
    /// Upstream's `alignmentOffset`, whose sign is flipped across in
    /// right-to-left when the alignment is a directional one.
    pub alignment_offset: Offset,
    /// This menu's own axis.
    pub orientation: MenuAxis,
    /// The axis of the menu this one hangs off. Equal to `orientation` for a
    /// submenu of a panel; different for the first panel under a menu bar.
    pub parent_orientation: MenuAxis,
    pub direction: crate::direction::TextDirection,
    /// Whether `alignment_offset` came from an `AlignmentDirectional`, which
    /// is what decides whether its `dx` flips in right-to-left.
    pub directional_alignment: bool,
}

impl MenuLayout {
    /// Upstream's `_positionChild`, with `menuPosition` null -- the case where
    /// the panel is placed against its anchor rather than at a point the
    /// caller named.
    ///
    /// `alignment` is resolved against the anchor; `allowed` is the rectangle
    /// the panel has to stay inside, which upstream takes from the display
    /// feature sub-screen nearest the anchor's centre and which is the whole
    /// overlay when there are no display features.
    pub fn position(
        &self,
        alignment: crate::render::Alignment,
        child: crate::render::Size,
        allowed: crate::engine::Rect,
    ) -> Offset {
        let anchor = self.anchor_rect;
        let within = Offset::new(
            anchor.left + (alignment.x + 1.0) / 2.0 * anchor.width(),
            anchor.top + (alignment.y + 1.0) / 2.0 * anchor.height(),
        );
        let directional = match (self.directional_alignment, self.direction) {
            (true, crate::direction::TextDirection::Rtl) => {
                Offset::new(-self.alignment_offset.dx, self.alignment_offset.dy)
            }
            _ => self.alignment_offset,
        };
        let mut x = within.dx + directional.dx;
        let mut y = within.dy + directional.dy;
        if self.direction == crate::direction::TextDirection::Rtl {
            x -= child.width;
        }

        let off_left = |x: f32| x < allowed.left;
        let off_right = |x: f32| x + child.width > allowed.right;
        let off_top = |y: f32| y < allowed.top;
        let off_bottom = |y: f32| y + child.height > allowed.bottom;

        if child.width >= allowed.width() {
            // It just does not fit: as much on the screen as possible, from
            // the leading edge.
            x = allowed.left;
        } else if off_left(x) {
            if self.parent_orientation != self.orientation {
                x = allowed.left;
            } else {
                let flipped = anchor.right + self.alignment_offset.dx;
                x = if off_right(flipped) {
                    allowed.left
                } else {
                    flipped
                };
            }
        } else if off_right(x) {
            if self.parent_orientation != self.orientation {
                x = allowed.right - child.width;
            } else {
                let flipped = anchor.left - child.width - self.alignment_offset.dx;
                x = if off_left(flipped) {
                    allowed.right - child.width
                } else {
                    flipped
                };
            }
        }

        if child.height >= allowed.height() {
            y = allowed.top;
        } else if off_top(y) {
            let below = anchor.bottom;
            y = if off_bottom(below) {
                allowed.top
            } else {
                below
            };
        } else if off_bottom(y) {
            let above = anchor.top - child.height;
            if off_top(above) {
                y = allowed.bottom - child.height;
            } else if self.parent_orientation == MenuAxis::Horizontal {
                // The gap asked for below a bar has to be re-applied above it.
                y = above - self.alignment_offset.dy;
            } else {
                y = above;
            }
        }
        Offset::new(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_themes::ResolvedMenuButton;

    // -- Where a menu panel goes ---------------------------------------------

    use crate::engine::Rect;
    use crate::render::Alignment;

    const SCREEN: Rect = Rect {
        left: 0.0,
        top: 0.0,
        right: 800.0,
        bottom: 600.0,
    };

    fn under(anchor: Rect) -> MenuLayout {
        MenuLayout {
            anchor_rect: anchor,
            alignment_offset: Offset::ZERO,
            orientation: MenuAxis::Vertical,
            parent_orientation: MenuAxis::Vertical,
            direction: crate::direction::TextDirection::Ltr,
            directional_alignment: true,
        }
    }

    #[test]
    fn a_panel_starts_where_the_alignment_points_on_the_anchor() {
        // `alignment.withinRect(anchorRect)`: bottom-left of the button is
        // where a menu hanging under it begins.
        let layout = under(Rect::xywh(100.0, 100.0, 60.0, 40.0));
        assert_eq!(
            layout.position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN),
            Offset::new(100.0, 140.0)
        );
        assert_eq!(
            layout.position(Alignment::TOP_RIGHT, Size::new(150.0, 200.0), SCREEN),
            Offset::new(160.0, 100.0),
            "and the other corner is the other corner"
        );
    }

    #[test]
    fn the_offset_moves_it_and_flips_across_in_right_to_left() {
        // Upstream flips `dx` and leaves `dy` alone, and only for a
        // *directional* alignment: an `Alignment` written in absolute terms
        // means the same thing in both directions.
        // Well inside the screen on both sides, so that what is being read
        // here is the offset and not one of the off-screen corrections. A
        // first draft anchored at x = 100 and measured 168 -- which is right,
        // and is the *flip*: a 150-wide panel hanging leftwards from there
        // runs off the edge.
        let mut layout = under(Rect::xywh(400.0, 100.0, 60.0, 40.0));
        layout.alignment_offset = Offset::new(8.0, 4.0);
        assert_eq!(
            layout.position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN),
            Offset::new(408.0, 144.0)
        );

        let mut rtl = layout;
        rtl.direction = crate::direction::TextDirection::Rtl;
        // Right-to-left also hangs the panel from its right edge, so the width
        // comes off as well: 400 - 8 - 150.
        assert_eq!(
            rtl.position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN),
            Offset::new(242.0, 144.0)
        );

        let mut absolute = rtl;
        absolute.directional_alignment = false;
        assert_eq!(
            absolute.position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN),
            Offset::new(258.0, 144.0),
            "an absolute alignment does not flip the offset"
        );
    }

    #[test]
    fn a_panel_that_will_not_fit_across_starts_at_the_leading_edge() {
        // Not centred and not shrunk: as much as fits, from the start, because
        // a menu is read from its leading edge.
        let layout = under(Rect::xywh(400.0, 100.0, 60.0, 40.0));
        assert_eq!(
            layout
                .position(Alignment::BOTTOM_LEFT, Size::new(900.0, 200.0), SCREEN)
                .dx,
            SCREEN.left
        );
    }

    #[test]
    fn a_submenu_off_the_right_opens_to_the_left_of_its_parent() {
        // The flip upstream tries *before* sliding: a submenu that will not
        // fit to the right of its parent opens on the other side of it.
        // Sliding first would leave the panel over the line it came from.
        let mut layout = under(Rect::xywh(700.0, 100.0, 60.0, 40.0));
        layout.alignment_offset = Offset::new(4.0, 0.0);
        let x = layout
            .position(Alignment::BOTTOM_LEFT, Size::new(200.0, 100.0), SCREEN)
            .dx;
        assert_eq!(x, 700.0 - 200.0 - 4.0, "left of the anchor, by the offset");
    }

    #[test]
    fn a_submenu_off_the_left_opens_to_the_right_of_its_parent() {
        // The mirror of the flip above, and the one a right-to-left menu takes
        // first: a panel hanging leftwards from a button near the left edge
        // opens to the *right* of it rather than sliding to the margin.
        let mut layout = under(Rect::xywh(100.0, 100.0, 60.0, 40.0));
        layout.direction = crate::direction::TextDirection::Rtl;
        layout.alignment_offset = Offset::new(8.0, 4.0);
        let x = layout
            .position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN)
            .dx;
        assert_eq!(
            x,
            160.0 + 8.0,
            "the anchor's right edge plus the offset, not the screen's margin"
        );

        // And when the other side does not fit either, it does slide.
        let narrow = Rect::xywh(0.0, 0.0, 200.0, 600.0);
        let x = layout
            .position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), narrow)
            .dx;
        assert_eq!(x, narrow.left, "nowhere else to go");
    }

    #[test]
    fn a_panel_under_a_menu_bar_is_pushed_along_instead_of_flipped() {
        // `parentOrientation != orientation`. A panel hanging off a *bar* has
        // no other side of the button worth trying -- the bar runs across and
        // the panel runs down -- so it slides to the edge.
        let mut layout = under(Rect::xywh(700.0, 100.0, 60.0, 40.0));
        layout.parent_orientation = MenuAxis::Horizontal;
        let x = layout
            .position(Alignment::BOTTOM_LEFT, Size::new(200.0, 100.0), SCREEN)
            .dx;
        assert_eq!(x, SCREEN.right - 200.0, "slid to the edge, not flipped");
    }

    #[test]
    fn a_panel_that_does_not_fit_below_goes_above() {
        let layout = under(Rect::xywh(100.0, 520.0, 60.0, 40.0));
        let y = layout
            .position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN)
            .dy;
        assert_eq!(y, 520.0 - 200.0, "its bottom on the anchor's top");
    }

    #[test]
    fn moving_up_past_a_bar_re_applies_the_gap_that_was_asked_for_below_it() {
        // The one place the flip is not exact. A caller who asked for four
        // pixels of air under the bar wants four above it too; everywhere else
        // the offset is already in the number being flipped.
        let mut layout = under(Rect::xywh(100.0, 520.0, 60.0, 40.0));
        layout.alignment_offset = Offset::new(0.0, 4.0);
        layout.parent_orientation = MenuAxis::Horizontal;
        let y = layout
            .position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN)
            .dy;
        assert_eq!(y, 520.0 - 200.0 - 4.0);

        let mut under_a_panel = layout;
        under_a_panel.parent_orientation = MenuAxis::Vertical;
        assert_eq!(
            under_a_panel
                .position(Alignment::BOTTOM_LEFT, Size::new(150.0, 200.0), SCREEN)
                .dy,
            520.0 - 200.0,
            "and under a panel it is not re-applied"
        );
    }

    #[test]
    fn a_panel_taller_than_the_screen_starts_at_the_top() {
        let layout = under(Rect::xywh(100.0, 300.0, 60.0, 40.0));
        assert_eq!(
            layout
                .position(Alignment::BOTTOM_LEFT, Size::new(150.0, 700.0), SCREEN)
                .dy,
            SCREEN.top
        );
    }

    #[test]
    fn a_panel_that_would_start_above_the_screen_drops_below_the_anchor() {
        // The mirror of "does not fit below": upstream tries the anchor's
        // bottom before giving up and pinning to the top.
        let layout = under(Rect::xywh(100.0, 10.0, 60.0, 40.0));
        let y = layout
            .position(Alignment::TOP_LEFT, Size::new(150.0, 100.0), SCREEN)
            .dy;
        assert_eq!(y, 10.0, "the anchor's own top already fits");

        let mut above = under(Rect::xywh(100.0, 10.0, 60.0, 40.0));
        above.alignment_offset = Offset::new(0.0, -60.0);
        assert_eq!(
            above
                .position(Alignment::TOP_LEFT, Size::new(150.0, 100.0), SCREEN)
                .dy,
            50.0,
            "pushed off the top, it takes the anchor's bottom instead"
        );
    }

    // -- A submenu button opens its menu -------------------------------------

    use crate::framework::{AnyWidget, BuildContext, Component};

    const SUBMENU: u64 = 8401;
    const MENU_GROUP: u64 = 8402;
    const BAR_ROOT: u64 = 8404;
    const SIBLING: u64 = 8405;
    const PANEL: Color = Color(0xFF00_00AA);

    /// A page with an overlay, holding whatever `body` builds.
    ///
    /// One harness rather than one per test: an overlay only as big as its
    /// page would leave a menu panel nowhere to be but on top of the button,
    /// and a page with nothing hittable under it never hears the tap that was
    /// meant to close a menu. Both facts were learnt once, and every staging
    /// below inherits them.
    ///
    /// `body` is called on **every** build, not handed over once: opening a
    /// menu rebuilds this page, and a first draft that `take()`d its child out
    /// of a cell replaced the button with an empty box the second time round.
    fn staged_page(
        body: impl Fn() -> AnyWidget + 'static,
    ) -> (ElementTree, std::rc::Rc<crate::theatre::OverlayHandle>) {
        let found: std::rc::Rc<
            std::cell::RefCell<Option<std::rc::Rc<crate::theatre::OverlayHandle>>>,
        > = std::rc::Rc::new(std::cell::RefCell::new(None));
        struct Finder(
            std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<crate::theatre::OverlayHandle>>>>,
            Box<dyn Fn() -> AnyWidget>,
        );
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = crate::theatre::OverlayHandle::of(context);
                crate::framework::many(vec![(self.1)()], |rendered| {
                    crate::render::RenderPointerRegion::new(
                        9999,
                        crate::render::RenderAlign::new(
                            crate::render::Alignment::TOP_LEFT,
                            rendered.into_iter().next().expect("the page"),
                        ),
                    )
                    .with_behavior(crate::render::HitTestBehavior::Opaque)
                })
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(crate::tap_region::TapRegionSurface::new(
            8400,
            crate::theatre::overlay(crate::framework::component(Finder(
                std::rc::Rc::clone(&found),
                Box::new(body),
            ))),
        ));
        tree.build_render_tree();
        let handle = found.borrow().clone().expect("a descendant found it");
        (tree, handle)
    }

    fn staged(button: SubmenuButton) -> (ElementTree, std::rc::Rc<crate::theatre::OverlayHandle>) {
        staged_page(move || stateful(button.clone()))
    }

    /// [`staged`], for a whole bar.
    fn staged_bar(bar: MenuBar) -> (ElementTree, std::rc::Rc<crate::theatre::OverlayHandle>) {
        staged_page(move || stateful(bar.clone()))
    }

    /// [`staged`], with the button pushed `down` pixels from the top.
    fn staged_at(
        button: SubmenuButton,
        down: f32,
    ) -> (ElementTree, std::rc::Rc<crate::theatre::OverlayHandle>) {
        staged_inset(button, 0.0, down)
    }

    /// [`staged`], with the button inset `across` and `down` from the corner.
    fn staged_inset(
        button: SubmenuButton,
        across: f32,
        down: f32,
    ) -> (ElementTree, std::rc::Rc<crate::theatre::OverlayHandle>) {
        staged_page(move || {
            crate::framework::many(vec![stateful(button.clone())], move |rendered| {
                crate::render::RenderPadding::new(
                    crate::render::EdgeInsets::only(across, down, 0.0, 0.0),
                    rendered.into_iter().next().expect("the button"),
                )
            })
        })
    }

    fn a_submenu() -> SubmenuButton {
        SubmenuButton::new()
            .with_id(SUBMENU)
            .with_label("File")
            .with_group_id(MENU_GROUP)
            .with_menu(|| {
                // Shrink-wrapped, with no alignment of its own: a panel that
                // filled the overlay and placed itself would draw in the same
                // spot whatever the placement said, and every test about
                // *where* it went would be about the panel's own `Align`
                // instead. A real menu panel is the size of its lines.
                leaf(|| {
                    crate::render::RenderDecoratedBox::new()
                        .with_fill(crate::render::Fill::Solid(PANEL))
                        .with_child(crate::widgets::SizedBox::new(100.0, 100.0))
                })
            })
    }

    #[test]
    fn a_bar_is_inert_to_the_pointer_until_somebody_clicks_it() {
        // Upstream's rule, in its own words: *"Don't open the root menu bar
        // menus on hover unless a sibling menu is already open. This means
        // that the user has to first click to open a menu on the menu bar
        // before hovering allows them to traverse it."*
        //
        // Without the first half, a pointer crossing the top of a window on
        // its way somewhere else would drop menus open behind it.
        crate::raw_menu_anchor::reset_menu_tree();
        a_bar_of_two();
        let entry = a_submenu().in_a_bar(true).under(BAR_ROOT);
        assert!(!entry.opens_on_hover(), "nobody has opened the bar yet");

        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.open(SIBLING));
        assert!(
            entry.opens_on_hover(),
            "and once a sibling is open the whole bar tracks the pointer"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    /// A bar in the tree with two entries under it: [`SUBMENU`] and
    /// [`SIBLING`]. The shape is the point -- a bar is never itself open, so
    /// what makes it live is one of its children.
    fn a_bar_of_two() {
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(BAR_ROOT));
            for entry in [SUBMENU, SIBLING] {
                if tree.node(entry).is_none() {
                    tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(entry));
                }
                tree.set_parent(entry, Some(BAR_ROOT))
                    .expect("an entry of the bar");
            }
        });
    }

    #[test]
    fn a_line_of_a_panel_opens_on_hover_with_no_such_condition() {
        // A button inside a panel has no condition to meet: its parent menu is
        // by definition already open, or the button would not be on screen.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(SUBMENU))
        });
        assert!(a_submenu().opens_on_hover(), "closed, and still opens");
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_bar_entry_asks_about_the_root_and_not_about_itself() {
        // `_MenuAnchorState._maybeOf(context)!._root`. A sibling being open is
        // what makes the bar live; this entry's own menu is precisely the one
        // that is not open yet.
        crate::raw_menu_anchor::reset_menu_tree();
        a_bar_of_two();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.open(SIBLING));
        let entry = a_submenu().in_a_bar(true).under(BAR_ROOT);
        assert!(
            !crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(SUBMENU)),
            "this entry's own menu is shut"
        );
        assert!(
            entry.opens_on_hover(),
            "but the bar it belongs to is open, so the pointer may traverse it"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn hovering_a_bar_entry_that_is_live_opens_it() {
        // End to end, through a real pointer: the rule above, reached by the
        // hover the line reports.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu().in_a_bar(true));
        hover(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(
            overlay.entry_count(),
            0,
            "a bar nobody has clicked stays shut under the pointer"
        );

        a_bar_of_two();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.open(SIBLING));
        hover(&mut tree, Offset::new(200.0, 200.0));
        hover(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(
            overlay.entry_count(),
            0,
            "and an entry whose own menu is already open opens nothing more"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn the_pointer_leaving_a_button_does_not_open_it() {
        // The `entered` half of the guard. A hover callback fires twice -- once
        // arriving, once leaving -- and a rule that read only "may this open"
        // would open the menu as the pointer walked *off* it.
        //
        // Staged so that the leaving is the only chance to open: the pointer
        // arrives while the bar is still shut, the bar opens under it, and then
        // the pointer leaves. Arriving opened nothing, so anything on screen at
        // the end got there by departing.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu().in_a_bar(true));
        let mut router = crate::gestures::GestureRouter::new();
        hover_using(&mut router, &mut tree, Offset::new(30.0, 24.0));
        assert_eq!(overlay.entry_count(), 0, "a shut bar ignores the pointer");

        a_bar_of_two();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.open(SIBLING));
        hover_using(&mut router, &mut tree, Offset::new(300.0, 250.0));
        assert_eq!(
            overlay.entry_count(),
            0,
            "leaving is not arriving, and opens nothing"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_bar_entry_near_the_edge_slides_its_panel_instead_of_flipping_it() {
        // The button's `parent_orientation` reaches the placement: a panel
        // hanging off a *bar* has no other side of the button worth trying, so
        // it slides to the edge rather than opening to the left of it. A line
        // of a panel in the same place does flip.
        let panel_left = |entry: SubmenuButton| {
            crate::raw_menu_anchor::reset_menu_tree();
            let (mut tree, _overlay) = staged_inset(entry, 750.0, 0.0);
            tap_in(&mut tree, Offset::new(770.0, 24.0), 800.0, 600.0);
            let drawn = painted_tree(&mut tree);
            let left = drawn
                .iter()
                .find_map(|call| match call {
                    Drawn::Rect { left, argb, .. } if Color(*argb) == PANEL => Some(*left),
                    _ => None,
                })
                .expect("the panel painted");
            crate::raw_menu_anchor::reset_menu_tree();
            left
        };
        // The paint happens in 800 x 600, so a 100-wide panel hanging from a
        // button at x = 750 runs off the right. A line of a panel flips to the
        // other side of the button; a bar entry slides to the edge.
        assert_eq!(
            panel_left(a_submenu()),
            750.0 - 100.0,
            "a line of a panel flips to the button's left"
        );
        assert_eq!(
            panel_left(a_submenu().in_a_bar(true)),
            800.0 - 100.0,
            "and a bar entry slides to the screen's edge"
        );
    }

    #[test]
    fn hovering_a_line_of_a_panel_opens_its_submenu() {
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu());
        hover(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(overlay.entry_count(), 1, "the pointer opened it");
        assert!(crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(SUBMENU)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    // -- A menu anchor of one's own ----------------------------------------

    const ANCHOR: u64 = 8420;
    const ANCHOR_BUTTON: u64 = 8421;
    const ELSEWHERE: u64 = 8422;
    const ANCHOR_PANEL: Color = Color(0xFF00_00CC);

    /// An anchor whose child is a button that opens it, which is what
    /// upstream's `builder(context, controller, child)` is for.
    fn an_anchor() -> MenuAnchor {
        MenuAnchor::new()
            .with_id(ANCHOR)
            .with_group_id(MENU_GROUP)
            .with_child(|controller| {
                // A group of its **own**, not the menu's: upstream's builder
                // child is any widget at all, and what puts it inside the
                // menu's group is the anchor's own region around it. A child
                // that carried the group itself would hide whether the anchor
                // has one.
                //
                // No anchor id either: a line with one closes the menu when it
                // is chosen, and the button that opens a menu is the one line
                // that must not.
                let mut button = MenuItemButton::new()
                    .with_id(ANCHOR_BUTTON)
                    .with_label("More")
                    .with_on_pressed(move || controller.open_menu());
                button.group_id = MENU_GROUP + 7;
                stateful(button)
            })
            .with_menu(|| {
                leaf(|| {
                    crate::render::RenderDecoratedBox::new()
                        .with_fill(crate::render::Fill::Solid(ANCHOR_PANEL))
                        .with_child(crate::widgets::SizedBox::new(80.0, 60.0))
                })
            })
    }

    /// The anchor on a page, pushed `across` and `down` from the corner.
    fn staged_anchor(
        menu: MenuAnchor,
        across: f32,
        down: f32,
    ) -> (ElementTree, std::rc::Rc<crate::theatre::OverlayHandle>) {
        staged_page(move || {
            crate::framework::many(vec![stateful(menu.clone())], move |rendered| {
                crate::render::RenderPadding::new(
                    crate::render::EdgeInsets::only(across, down, 0.0, 0.0),
                    rendered.into_iter().next().expect("the anchor"),
                )
            })
        })
    }

    #[test]
    fn a_controller_opens_the_menu_it_is_attached_to() {
        // Upstream's `builder(context, controller, child)` hands the caller a
        // controller so that the thing which opens the menu can be any widget
        // at all. Until now a `MenuController` could only reach the *tree*:
        // it could say a menu was open while the screen stayed empty.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::reset_menu_panels();
        let opened = std::rc::Rc::new(std::cell::Cell::new(0));
        let counting = std::rc::Rc::clone(&opened);
        let (mut tree, overlay) = staged_anchor(
            an_anchor().with_on_open(move || counting.set(counting.get() + 1)),
            0.0,
            0.0,
        );
        assert_eq!(overlay.entry_count(), 0, "nothing yet");

        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "the button opened it");
        assert_eq!(opened.get(), 1, "and `onOpen` was told once");
        assert!(crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(ANCHOR)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn the_menu_hangs_off_the_anchor_and_not_off_the_corner() {
        // The panel is placed against the anchor's own rectangle -- which is
        // why the anchor records where it is from its assemble. A panel that
        // ignored it would land at the overlay's origin, on top of the button
        // that opened it.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::reset_menu_panels();
        let (mut tree, _overlay) = staged_anchor(an_anchor(), 120.0, 40.0);
        tap_in(&mut tree, Offset::new(140.0, 52.0), 800.0, 600.0);
        let drawn = painted_tree(&mut tree);
        let panel = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Rect {
                    left, top, argb, ..
                } if Color(*argb) == ANCHOR_PANEL => Some(Offset::new(*left, *top)),
                _ => None,
            })
            .expect("the panel painted");
        assert_eq!(panel.dx, 120.0, "its left edge on the anchor's");
        assert!(
            panel.dy > 40.0,
            "and below the anchor rather than over it: {panel:?}"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn closing_through_the_controller_takes_the_panel_down_and_tells_the_caller() {
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::reset_menu_panels();
        let closed = std::rc::Rc::new(std::cell::Cell::new(0));
        let counting = std::rc::Rc::clone(&closed);
        let (mut tree, overlay) = staged_anchor(
            an_anchor().with_on_close(move || counting.set(counting.get() + 1)),
            0.0,
            0.0,
        );
        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1);

        let controller = crate::raw_menu_anchor::with_menu_tree(|_| {
            let mut controller = crate::raw_menu_anchor::MenuController::new();
            controller.attach(ANCHOR);
            controller
        });
        controller.close_menu();
        assert_eq!(overlay.entry_count(), 0, "the panel came down");
        assert_eq!(closed.get(), 1, "and `onClose` was told");
        assert!(!crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(ANCHOR)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn on_close_hears_a_tap_outside_as_well_as_a_caller() {
        // Upstream hangs `onClose` on the anchor closing, not on the caller
        // calling: a tap outside and an Escape are not the caller's doing, and
        // a caller who only heard about their own closes would miss both.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::reset_menu_panels();
        let closed = std::rc::Rc::new(std::cell::Cell::new(0));
        let counting = std::rc::Rc::clone(&closed);
        let (mut tree, overlay) = staged_anchor(
            an_anchor().with_on_close(move || counting.set(counting.get() + 1)),
            0.0,
            0.0,
        );
        tap(&mut tree, Offset::new(20.0, 12.0));
        tap(&mut tree, Offset::new(300.0, 250.0));
        assert_eq!(overlay.entry_count(), 0, "the tap outside took it down");
        assert_eq!(closed.get(), 1, "and the caller heard about it");
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn pressing_the_anchor_again_is_not_a_press_outside_its_menu() {
        // The anchor is in its own menu's tap-region group. Without that, the
        // second press is a tap *outside* the panel: the panel closes on the
        // way **down** and the button's own tap, which arrives on the way up,
        // opens it again.
        //
        // Counting entries cannot see that -- one panel goes and one arrives,
        // and the total never changes. What sees it is the caller: a menu that
        // was shut and reopened told them twice.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::reset_menu_panels();
        let opens = std::rc::Rc::new(std::cell::Cell::new(0));
        let closes = std::rc::Rc::new(std::cell::Cell::new(0));
        let (counting_open, counting_close) =
            (std::rc::Rc::clone(&opens), std::rc::Rc::clone(&closes));
        let (mut tree, overlay) = staged_anchor(
            an_anchor()
                .with_on_open(move || counting_open.set(counting_open.get() + 1))
                .with_on_close(move || counting_close.set(counting_close.get() + 1)),
            0.0,
            0.0,
        );
        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1);

        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "still one menu");
        assert_eq!(opens.get(), 1, "and it is the same one that was already up");
        assert_eq!(closes.get(), 0, "nothing closed on the way");
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn an_anchor_told_to_claims_the_tap_that_dismissed_it() {
        // Upstream's `consumeOutsideTap`, false by default and the default is
        // the considered one: a reader dismissing a menu by tapping a button
        // usually means only to dismiss it.
        //
        // What "consumed" means here is narrower than upstream, and the
        // difference is `tap_region.rs`'s own recorded divergence: the claim
        // is *reported* -- `TapRegionRegistry::last_dispatch_consumed` -- but
        // it does not yet stop the press, because upstream stops it by putting
        // a dummy member into the gesture arena and this crate's arena has no
        // entry point for a claim from outside it. So this is a test of the
        // claim the anchor makes, which is all of it that exists.
        let claimed = |consume: bool| {
            crate::raw_menu_anchor::reset_menu_tree();
            crate::raw_menu_anchor::reset_menu_panels();
            let found: std::rc::Rc<
                std::cell::RefCell<Option<crate::tap_region::TapRegionRegistry>>,
            > = std::rc::Rc::new(std::cell::RefCell::new(None));
            struct Probe(
                std::rc::Rc<std::cell::RefCell<Option<crate::tap_region::TapRegionRegistry>>>,
                MenuAnchor,
            );
            impl Component for Probe {
                fn build(&self, context: &mut BuildContext) -> AnyWidget {
                    *self.0.borrow_mut() = Some(crate::tap_region::TapRegionRegistry::of(context));
                    stateful(self.1.clone())
                }
            }
            let menu = an_anchor().with_consume_outside_tap(consume);
            let seen = std::rc::Rc::clone(&found);
            let (mut tree, _overlay) = staged_page(move || {
                crate::framework::component(Probe(std::rc::Rc::clone(&seen), menu.clone()))
            });
            tap(&mut tree, Offset::new(20.0, 12.0));
            press_only(&mut tree, Offset::new(300.0, 250.0));
            let registry = found.borrow().clone().expect("a descendant found it");
            crate::raw_menu_anchor::reset_menu_tree();
            registry.last_dispatch_consumed()
        };
        assert!(!claimed(false), "by default nothing is claimed");
        assert!(claimed(true), "and told to, the anchor claims the press");
    }

    #[test]
    fn an_anchor_taken_away_takes_its_menu_and_its_opener_with_it() {
        // A closure left behind holds the overlay handle of a page that is
        // gone, so a controller somebody kept would open a menu into it.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::reset_menu_panels();
        let (mut tree, overlay) = staged_anchor(an_anchor(), 0.0, 0.0);
        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1);

        tree.rebuild(leaf(|| crate::widgets::SizedBox::new(1.0, 1.0)));
        tree.build_render_tree();
        assert_eq!(overlay.entry_count(), 0, "the menu went with it");
        assert!(crate::raw_menu_anchor::with_menu_tree(|tree| tree
            .node(ANCHOR)
            .is_none()));

        crate::raw_menu_anchor::open_menu(ANCHOR);
        assert_eq!(
            overlay.entry_count(),
            0,
            "and nothing knows how to reopen it"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    // -- A menu bar, assembled ---------------------------------------------

    const MENU_BAR: u64 = 8410;
    const FILE_MENU: u64 = 8411;
    const EDIT_MENU: u64 = 8412;
    const FILE_ITEM: u64 = 8413;
    const EDIT_PANEL: Color = Color(0xFF00_AA00);

    /// A bar with two menus on it, each with a panel of its own colour so a
    /// test can say *which* one opened.
    fn a_bar() -> MenuBar {
        MenuBar::new()
            .with_id(MENU_BAR)
            .with_group_id(MENU_GROUP)
            .push(
                SubmenuButton::new()
                    .with_id(FILE_MENU)
                    .with_label("File")
                    // A real line inside, not a coloured box: a panel with
                    // nothing to choose in it cannot show what choosing does.
                    .with_menu(|| {
                        crate::framework::many(
                            vec![stateful(
                                MenuItemButton::new()
                                    .with_id(FILE_ITEM)
                                    .with_label("Open")
                                    .in_menu(FILE_MENU, MENU_GROUP),
                            )],
                            |rendered| {
                                crate::render::RenderDecoratedBox::new()
                                    .with_fill(crate::render::Fill::Solid(PANEL))
                                    .with_child(rendered.into_iter().next().expect("the line"))
                            },
                        )
                    }),
            )
            .push(
                SubmenuButton::new()
                    .with_id(EDIT_MENU)
                    .with_label("Edit")
                    // Told the wrong group on purpose: the bar settles it,
                    // whatever the entry said. See
                    // [`a_bar_settles_the_group_its_entries_are_in`].
                    .with_group_id(9999)
                    .with_menu(|| {
                        leaf(|| {
                            crate::render::RenderDecoratedBox::new()
                                .with_fill(crate::render::Fill::Solid(EDIT_PANEL))
                                .with_child(crate::widgets::SizedBox::new(100.0, 100.0))
                        })
                    }),
            )
    }

    /// Where the label `word` was drawn.
    fn word_at(drawn: &[Drawn], word: &str) -> Offset {
        drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, x, y, .. } if text == word => Some(Offset::new(*x, *y)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{word} was not painted: {drawn:?}"))
    }

    fn a_panel_is_up(drawn: &[Drawn], colour: Color) -> bool {
        drawn
            .iter()
            .any(|call| matches!(call, Drawn::Rect { argb, .. } if Color(*argb) == colour))
    }

    #[test]
    fn a_menu_bar_hangs_its_entries_under_itself() {
        // The whole point of the bar being in the tree at all. Upstream's
        // `_MenuBarAnchorState` is a `_MenuAnchorState` like any other, and
        // every top-level menu re-parents onto it in
        // `didChangeDependencies`. Siblings are what the hover rule is about,
        // and without the parent link there are no siblings -- each entry
        // would be its own root.
        crate::raw_menu_anchor::reset_menu_tree();
        let (_tree, _overlay) = staged_bar(a_bar());
        crate::raw_menu_anchor::with_menu_tree(|tree| {
            assert_eq!(tree.root_of(FILE_MENU), MENU_BAR);
            assert_eq!(tree.root_of(EDIT_MENU), MENU_BAR);
            assert_eq!(
                tree.node(MENU_BAR)
                    .expect("the bar is in the tree")
                    .children,
                vec![FILE_MENU, EDIT_MENU],
                "in the order they run across"
            );
        });
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_menu_bar_is_never_itself_open_and_is_live_when_an_entry_is() {
        // Upstream's `RawMenuAnchorGroup`: a bar has no menu of its own to
        // open, so `isOpen` means *any child is open*. It is why a bar cannot
        // be dismissed the way a menu can -- there is nothing to dismiss.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged_bar(a_bar());
        assert!(!crate::raw_menu_anchor::with_menu_tree(|tree| {
            crate::raw_menu_anchor::RawMenuAnchorGroup::is_open(tree, MENU_BAR)
        }));

        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "File's panel is up");
        crate::raw_menu_anchor::with_menu_tree(|tree| {
            assert!(!tree.is_open(MENU_BAR), "the bar itself never opens");
            assert!(
                crate::raw_menu_anchor::RawMenuAnchorGroup::is_open(tree, MENU_BAR),
                "but it is live, because one of its entries is"
            );
        });
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_click_wakes_the_bar_and_the_pointer_walks_it_from_there() {
        // The rule of the previous round, now with a real bar under it: the
        // pointer alone opens nothing, and after one click it opens everything
        // it crosses. This is the pair of facts upstream describes as having
        // to "first click to open a menu on the menu bar before hovering
        // allows them to traverse it".
        //
        // One router for the whole walk, and a step off the button between the
        // two visits: an ink that was never left is still hovered, so arriving
        // on it again is not an arrival and asks nothing.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged_bar(a_bar());
        let edit = word_at(&painted_tree(&mut tree), "Edit");
        let on_edit = Offset::new(edit.dx + 2.0, edit.dy + 2.0);
        let away = Offset::new(300.0, 250.0);
        let mut router = crate::gestures::GestureRouter::new();
        hover_using(&mut router, &mut tree, on_edit);
        assert_eq!(overlay.entry_count(), 0, "a cold bar ignores the pointer");
        hover_using(&mut router, &mut tree, away);

        tap(&mut tree, Offset::new(20.0, 12.0));
        assert!(
            a_panel_is_up(&painted_tree(&mut tree), PANEL),
            "the click opened File"
        );

        hover_using(&mut router, &mut tree, on_edit);
        assert!(
            a_panel_is_up(&painted_tree(&mut tree), EDIT_PANEL),
            "and now the pointer alone opens Edit"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_bar_settles_the_group_its_entries_are_in() {
        // Every entry of a bar is in the bar's tap-region group, along with
        // every panel those entries open. It is what makes a press on one
        // entry *not* a tap outside the panel another entry has up: without
        // it the open panel is dismissed on the way down, and the press lands
        // in a menu that is already gone.
        crate::raw_menu_anchor::reset_menu_tree();
        let bar = a_bar();
        assert_eq!(
            bar.entry(1).expect("the second entry").group_id,
            MENU_GROUP,
            "whatever the entry itself was told"
        );

        let (mut tree, overlay) = staged_bar(bar);
        let edit = word_at(&painted_tree(&mut tree), "Edit");
        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "File is up");
        tap(&mut tree, Offset::new(edit.dx + 2.0, edit.dy + 2.0));
        assert!(
            a_panel_is_up(&painted_tree(&mut tree), PANEL),
            "and pressing its neighbour did not dismiss it on the way down"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_menu_bar_lays_its_menus_out_across() {
        // A bar is a row. The same fact reaches its entries as
        // `MenuAxis::Horizontal`, which is what makes their panels slide to
        // the screen's edge rather than flip to the other side of the button.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged_bar(a_bar());
        let drawn = painted_tree(&mut tree);
        let first = word_at(&drawn, "File");
        let second = word_at(&drawn, "Edit");
        assert!(
            second.dx > first.dx,
            "the second menu is to the right of the first: {first:?} then {second:?}"
        );
        assert_eq!(second.dy, first.dy, "and on the same line");
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|_| a_bar()
                .entry(1)
                .expect("the second entry")
                .parent_orientation
                == MenuAxis::Horizontal),
            "which is what the entries are told"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_menu_bar_keeps_its_entries_off_the_window_edge() {
        // `_kTopLevelMenuHorizontalMinPadding`, through `MenuBarTheme`. A bar
        // pads across and not down: it is as tall as its entries and no
        // taller.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged_bar(a_bar());
        let bare = word_at(&painted_tree(&mut tree), "File");
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(
            a_bar()
                .entry(0)
                .expect("the same button, on its own")
                .with_menu(|| leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))),
        );
        let alone = word_at(&painted_tree(&mut tree), "File");
        assert_eq!(
            bare.dx - alone.dx,
            crate::component_themes::ResolvedMenuPanel::BAR_PADDING,
            "the bar's padding, and nothing else, moved it across"
        );
        assert_eq!(bare.dy, alone.dy, "and nothing moved it down");
        crate::raw_menu_anchor::reset_menu_tree();
    }

    /// Escape, held and sent the way the binding sends it: the ambient
    /// keyboard has to say the key is down, because an activator compares the
    /// whole held set and not just the event.
    fn escape() -> bool {
        let event = crate::keyboard::KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey::ESCAPE,
            logical: crate::keyboard::LogicalKey::ESCAPE,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        };
        crate::keyboard::reset_keyboard();
        let mut keyboard = crate::keyboard::Keyboard::new();
        let mut down = event.clone();
        keyboard.record(&mut down);
        crate::keyboard::note_keyboard(&keyboard);
        crate::focus::dispatch_key(&event)
    }

    #[test]
    fn escape_takes_the_bar_s_menus_off_the_screen() {
        // Upstream's `_MenuBarAnchorState.actions`, which holds exactly one
        // entry: `DismissIntent: DismissMenuAction(controller)`. Every piece
        // of the path already existed -- the registry knew Escape meant
        // dismiss, the dispatcher knew what to do with the intent, the focus
        // layer walked keys up from the focused node, and the action knew how
        // to close from the root -- and none of them had ever met.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::focus::reset_scopes();
        let (mut tree, overlay) = staged_bar(a_bar());
        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "File is up");

        crate::focus::focus(FILE_MENU);
        assert!(escape(), "the key was taken");
        assert_eq!(overlay.entry_count(), 0, "and the panel came down");
        assert!(!crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(FILE_MENU)
        ));
        crate::focus::unfocus();
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn escape_closes_from_the_root_and_not_one_level() {
        // `controller._anchor!.root.handleCloseRequest()`. Escape means "I am
        // done with this menu", not "one level, please" -- so two menus of the
        // same bar both go, whichever of them the focus was in.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::focus::reset_scopes();
        let (mut tree, overlay) = staged_bar(a_bar());
        let edit = word_at(&painted_tree(&mut tree), "Edit");
        tap(&mut tree, Offset::new(20.0, 12.0));
        tap(&mut tree, Offset::new(edit.dx + 2.0, edit.dy + 2.0));
        assert_eq!(overlay.entry_count(), 2, "both menus are up");

        crate::focus::focus(EDIT_MENU);
        assert!(escape());
        assert_eq!(overlay.entry_count(), 0, "and both came down");
        crate::focus::unfocus();
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn the_bar_names_tab_as_well_as_escape() {
        // The other half of `_kMenuTraversalShortcuts` this crate can spell.
        // Who *serves* `NextFocusIntent` is not the bar: upstream it is the
        // application's own action set, which is why this is a claim about the
        // map and not about what a Tab does on a bare page.
        //
        // The four arrows are `DirectionalFocusIntent`, and this crate's
        // `Intent` has no direction to carry, so they are left out rather than
        // mapped to something that means a different thing.
        let registry = MenuBar::traversal_shortcuts();
        let told = |key: crate::keyboard::LogicalKey| {
            let event = crate::keyboard::KeyEvent {
                change: crate::keyboard::KeyChange::Down,
                physical: crate::keyboard::PhysicalKey(key.0),
                logical: key,
                character: None,
                synthesized: false,
                time_stamp_micros: 0,
            };
            let mut keyboard = crate::keyboard::Keyboard::new();
            let mut down = event.clone();
            keyboard.record(&mut down);
            registry
                .intent_for(&event, &keyboard)
                .map(|intent| intent.action_name().to_string())
        };
        assert_eq!(
            told(crate::keyboard::LogicalKey::ESCAPE),
            Some("Dismiss".to_string())
        );
        assert_eq!(
            told(crate::keyboard::LogicalKey::TAB),
            Some("NextFocus".to_string())
        );
        assert_eq!(told(crate::keyboard::LogicalKey::ENTER), None);
    }

    #[test]
    fn a_menu_opened_twice_leaves_no_stale_panel_behind() {
        // The panel list is keyed by anchor, so a panel that came down without
        // being forgotten is an entry that will be taken down *instead of* the
        // real one next time. The second Escape would then dismiss a handle to
        // nothing and leave the menu on screen.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::focus::reset_scopes();
        let (mut tree, overlay) = staged_bar(a_bar());
        tap(&mut tree, Offset::new(20.0, 12.0));
        tap(&mut tree, Offset::new(300.0, 250.0));
        assert_eq!(overlay.entry_count(), 0, "a tap outside took it down");

        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "and it opens again");
        crate::focus::focus(FILE_MENU);
        assert!(escape());
        assert_eq!(overlay.entry_count(), 0, "Escape reached the live panel");
        crate::focus::unfocus();
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn choosing_a_line_takes_the_panel_off_the_screen() {
        // `close_on_activate`, all the way. The tree closing is half of it:
        // upstream's anchor hides its overlay portal when it closes, and a
        // port that only wrote the tree half leaves the panel on screen,
        // belonging to a menu nothing believes in any more.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged_bar(a_bar());
        tap(&mut tree, Offset::new(20.0, 12.0));
        assert_eq!(overlay.entry_count(), 1, "the menu is up");

        let line = word_at(&painted_tree(&mut tree), "Open");
        tap(&mut tree, Offset::new(line.dx + 2.0, line.dy + 2.0));
        assert_eq!(overlay.entry_count(), 0, "and choosing a line took it down");
        assert!(!crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(FILE_MENU)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_key_the_bar_has_no_shortcut_for_is_left_alone() {
        // The bar's scope names Escape and Tab. Anything else has to travel
        // on: a menu that swallowed every key would take the letters a text
        // field under it was waiting for.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::focus::reset_scopes();
        let (mut tree, overlay) = staged_bar(a_bar());
        tap(&mut tree, Offset::new(20.0, 12.0));
        crate::focus::focus(FILE_MENU);

        let event = crate::keyboard::KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey::ENTER,
            logical: crate::keyboard::LogicalKey::ENTER,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        };
        crate::keyboard::reset_keyboard();
        let mut keyboard = crate::keyboard::Keyboard::new();
        let mut down = event.clone();
        keyboard.record(&mut down);
        crate::keyboard::note_keyboard(&keyboard);
        crate::focus::dispatch_key(&event);
        assert_eq!(overlay.entry_count(), 1, "the menu is still up");
        crate::focus::unfocus();
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_menu_bar_taken_away_leaves_the_tree() {
        // A node left behind is an anchor the tree still believes in: a later
        // bar with the same id would trip the "added once" assert, and Escape
        // would reach for a root that is not on screen.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged_bar(a_bar());
        assert!(crate::raw_menu_anchor::with_menu_tree(|tree| tree
            .node(MENU_BAR)
            .is_some()));

        tree.rebuild(leaf(|| crate::widgets::SizedBox::new(1.0, 1.0)));
        tree.build_render_tree();
        crate::raw_menu_anchor::with_menu_tree(|tree| {
            assert!(tree.node(MENU_BAR).is_none(), "the bar went");
            assert!(tree.node(FILE_MENU).is_none(), "and so did its menus");
        });
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_submenu_button_joins_the_tree_once_and_leaves_when_it_goes() {
        // `MenuAnchorTree::insert` asserts an anchor is added once, and a
        // `build` runs every frame -- so the insert has to be in
        // `initial_state`. A second build must not add a second node.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|tree| tree.node(SUBMENU).is_some()),
            "the anchor is in the tree"
        );
        tree.rebuild_dirty();
        tree.rebuild_dirty();
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|tree| tree.node(SUBMENU).is_some()),
            "and still exactly one after two more builds"
        );

        // Taken away, and the tree forgets it: a node left behind is an anchor
        // the tree still believes in.
        tree.rebuild(leaf(|| crate::widgets::SizedBox::new(1.0, 1.0)));
        tree.build_render_tree();
        assert!(crate::raw_menu_anchor::with_menu_tree(|tree| tree
            .node(SUBMENU)
            .is_none()));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn pressing_a_submenu_button_opens_its_panel() {
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(SUBMENU)),
            "the tree says the anchor is open"
        );
        let drawn = painted_tree(&mut tree);
        assert!(
            drawn
                .iter()
                .any(|call| matches!(call, Drawn::Rect { argb, .. } if Color(*argb) == PANEL)),
            "and the panel is on screen"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_button_says_when_a_press_should_open_nothing() {
        // The three early returns upstream's `_open` makes, asked where they
        // can be asked. Reaching them through a tap depends on tap regions,
        // overlay stacking and hit order; a test that got any of those wrong
        // would pass while proving nothing about the rule.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(SUBMENU))
        });
        assert!(a_submenu().should_open(), "closed, enabled, with a menu");
        assert!(
            !a_submenu().with_enabled(false).should_open(),
            "a disabled button opens nothing"
        );
        assert!(
            !SubmenuButton::new()
                .with_id(SUBMENU)
                .with_group_id(MENU_GROUP)
                .should_open(),
            "and neither does one with no menu"
        );

        crate::raw_menu_anchor::with_menu_tree_mut(|tree| tree.open(SUBMENU));
        assert!(
            !a_submenu().should_open(),
            "an anchor that is already open is not opened again"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn the_panel_lands_under_the_button_and_not_on_it() {
        // What rounds 455 to 457 could not test. An unplaced panel goes to the
        // overlay's origin, which is on top of the button, and then every
        // press meant for the button lands on the panel instead: the three
        // facts below were all invisible for that reason.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        let mut path = crate::render::HitTestResult::new();
        root.hit_test(Offset::new(30.0, 24.0), &mut path);
        let targets: Vec<u64> = path.path.iter().map(|entry| entry.target).collect();
        assert!(
            targets.contains(&SUBMENU),
            "the button is still reachable where it was: {targets:?}"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn pressing_the_button_again_does_not_open_a_second_panel() {
        // Upstream's `_open` returns early for an anchor that is already open.
        // A second panel would be a second overlay entry with nothing holding
        // its handle, so nothing could ever take it down.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(overlay.entry_count(), 1, "one panel to begin with");

        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(overlay.entry_count(), 1, "and still one");
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(SUBMENU)),
            "the same one -- the button is inside its own menu's tap-region \
             group, so pressing it again does not close the panel on the way \
             down"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_second_press_does_not_open_a_second_panel() {
        // Upstream's `_open` returns early for an anchor that is open. Here a
        // second panel would be a second overlay entry with nothing holding
        // its handle, so nothing could ever take it down.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        let first = _overlay.entry_count();
        assert_eq!(first, 1, "one panel to begin with");
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(_overlay.entry_count(), first, "still one panel");
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(SUBMENU)),
            "and it is the one that was already there -- the button is inside              its own menu's tap-region group, so pressing it again does not              close the panel on the way down"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn the_panel_is_drawn_at_the_buttons_bottom_left_corner() {
        // Where the panel actually *is*, read off the canvas rather than off
        // the hit path. Round 459 wired the placement and could not tell
        // whether it was doing anything: every test it had passed with the
        // placement removed, because a hit path only says what was reachable,
        // not where anything was drawn.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        let drawn = painted_tree(&mut tree);
        let panel = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Rect {
                    left, top, argb, ..
                } if Color(*argb) == PANEL => Some((*left, *top)),
                _ => None,
            })
            .expect("the panel painted");
        // The button is at the top left and is 48 tall -- a menu button's
        // minimum height -- so its bottom-left corner is (0, 48).
        assert_eq!(
            panel,
            (0.0, ResolvedMenuButton::MINIMUM_SIZE.height),
            "under the button, not at the overlay's origin"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_button_lower_down_the_page_carries_its_panel_with_it() {
        // The placement is read from the button's own rectangle, not from a
        // constant: a button that is not at the origin opens its panel under
        // *itself*.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged_at(a_submenu(), 60.0);
        tap(&mut tree, Offset::new(30.0, 84.0));
        let drawn = painted_tree(&mut tree);
        let panel = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Rect {
                    left, top, argb, ..
                } if Color(*argb) == PANEL => Some((*left, *top)),
                _ => None,
            })
            .expect("the panel painted");
        assert_eq!(
            panel,
            (0.0, 60.0 + ResolvedMenuButton::MINIMUM_SIZE.height),
            "sixty lower down, so the panel is sixty lower down"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_tap_on_a_panel_of_the_same_menu_does_not_close_it() {
        // The group id the button hands its panels is the menu's, not the
        // button's own id. Give each panel a group of its own and moving from
        // one panel of a menu to another would close the one behind.
        //
        // The assertion is the **overlay's entry count**, not the anchor's
        // `is_open`: an outside tap closes an anchor's *children* and leaves
        // the anchor itself alone, so `is_open` cannot fail here and a test
        // written on it would pass whatever happened to the panel.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        let sibling = overlay
            .insert(|| {
                crate::framework::component(SameGroup {
                    id: 8403,
                    group_id: MENU_GROUP,
                })
            })
            .expect("inserted");
        tree.rebuild_dirty();
        let before = overlay.entry_count();
        assert_eq!(before, 2, "the panel and the sibling");

        // Inside the sibling, which fills the bottom right of the 400 x 300
        // these tests lay out in.
        tap(&mut tree, Offset::new(380.0, 280.0));
        assert_eq!(
            overlay.entry_count(),
            before,
            "a tap on a panel of the same menu is not outside it"
        );
        overlay.remove(sibling);
        crate::raw_menu_anchor::reset_menu_tree();
    }

    /// A second tap region of the same group, somewhere else on screen: what a
    /// submenu's panel is to the menu it grew from.
    struct SameGroup {
        id: u64,
        group_id: u64,
    }

    impl Component for SameGroup {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            crate::tap_region::TapRegion::new(self.id)
                .with_group_id(self.group_id)
                .build(
                    context,
                    leaf(|| {
                        crate::render::RenderAlign::new(
                            crate::render::Alignment::BOTTOM_RIGHT,
                            crate::render::RenderDecoratedBox::new()
                                .with_fill(crate::render::Fill::Solid(Color(0xFF00_FF00)))
                                .with_child(crate::widgets::SizedBox::new(200.0, 200.0)),
                        )
                    }),
                )
        }
    }

    #[test]
    fn a_button_with_no_menu_opens_nothing() {
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(
            SubmenuButton::new()
                .with_id(SUBMENU)
                .with_label("File")
                .with_group_id(MENU_GROUP),
        );
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert!(
            !crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(SUBMENU)),
            "nothing to open"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_disabled_submenu_button_opens_nothing() {
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu().with_enabled(false));
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert!(!crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(SUBMENU)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_submenu_button_taken_away_takes_its_panel_with_it() {
        // `dispose` dismisses what it opened. A panel left in the overlay
        // belongs to a button that no longer exists: nothing holds its handle,
        // so nothing can ever take it down.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(overlay.entry_count(), 1, "the panel is up");

        tree.rebuild(leaf(|| crate::widgets::SizedBox::new(1.0, 1.0)));
        tree.build_render_tree();
        assert_eq!(overlay.entry_count(), 0, "and it went with the button");
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_submenu_line_is_written_the_way_a_menu_item_is() {
        // Built out of `MenuItemButton` so the two cannot drift: the label is
        // the button's, and a disabled one fades exactly as an item's does.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        let enabled = painted_tree(&mut tree);
        let colour_of = |drawn: &[Drawn]| {
            drawn
                .iter()
                .find_map(|call| match call {
                    Drawn::Paragraph { text, argb, .. } if text == "File" => Some(Color(*argb)),
                    _ => None,
                })
                .expect("the label painted")
        };
        let lit = colour_of(&enabled);

        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu().with_enabled(false));
        let faded = colour_of(&painted_tree(&mut tree));
        assert!(
            faded.alpha() < lit.alpha(),
            "a disabled submenu line is faded: {faded:?} against {lit:?}"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_submenu_line_carries_the_arrow_and_an_item_line_does_not() {
        // The one difference between the two lines, in the slot
        // `MenuItemLabel` reserved for it -- and it takes the same gap as any
        // other trailing part.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(a_submenu());
        let drawn = painted_tree(&mut tree);
        let arrow = drawn
            .iter()
            .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "\u{25B8}"));
        assert!(arrow, "the submenu's arrow: {drawn:?}");

        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, _overlay) = staged(SubmenuButton {
            has_submenu_icon: false,
            ..a_submenu()
        });
        let drawn = painted_tree(&mut tree);
        assert!(
            !drawn
                .iter()
                .any(|call| matches!(call, Drawn::Paragraph { text, .. } if text == "\u{25B8}")),
            "and a button with no arrow slot has no arrow"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    /// Everything a staged tree paints, at the screen's size.
    fn painted_tree(tree: &mut ElementTree) -> Vec<Drawn> {
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(800, 600);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(800.0, 600.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    // -- The line as a widget -----------------------------------------------

    use crate::engine::Color;
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{ElementTree, leaf, stateful};
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};

    const ITEM: u64 = 8301;
    const BAR: u64 = 8302;
    const NESTED: u64 = 8303;
    const MARK: Color = Color(0xFF00_FF00);
    const OTHER: Color = Color(0xFFFF_00FF);

    fn an_item() -> MenuItemButton {
        MenuItemButton::new().with_id(ITEM).with_label("Paste")
    }

    fn mark(colour: Color) -> crate::framework::AnyWidget {
        leaf(move || {
            crate::render::RenderDecoratedBox::new()
                .with_fill(crate::render::Fill::Solid(colour))
                .with_child(crate::widgets::SizedBox::new(20.0, 20.0))
        })
    }

    /// Lays a line out in 400x100 and answers everything drawn.
    fn painted(item: MenuItemButton) -> Vec<Drawn> {
        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(item));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        tree.advance_frame(0);
        tree.rebuild_dirty();
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(400, 100);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(400.0, 100.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn laid_out(item: MenuItemButton) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(item));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 200.0));
        crate::render::flush_layout();
        root.size()
    }

    fn text_at(drawn: &[Drawn], wanted: &str) -> Option<f32> {
        drawn.iter().find_map(|call| match call {
            Drawn::Paragraph { text, x, .. } if text == wanted => Some(*x),
            _ => None,
        })
    }

    fn colour_at(drawn: &[Drawn], wanted: Color) -> Option<f32> {
        drawn.iter().find_map(|call| match call {
            Drawn::Rect { left, argb, .. } if Color(*argb) == wanted => Some(*left),
            _ => None,
        })
    }

    #[test]
    fn choosing_a_line_closes_the_whole_menu_and_not_one_panel_of_it() {
        // Upstream's `_handleSelect`: `_anchor?._root._menuController.close()`.
        // **The root.** Choosing an item is the end of the interaction, not of
        // one panel of it, which is why it reaches the same place Escape does.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(BAR));
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(NESTED));
            tree.set_parent(NESTED, Some(BAR))
                .expect("a child of the bar");
            tree.open(BAR);
            tree.open(NESTED);
        });

        let chosen = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = std::rc::Rc::clone(&chosen);
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(
            an_item()
                .in_menu(NESTED, MENU_GROUP)
                .with_on_pressed(move || count.set(count.get() + 1)),
        ));
        tree.build_render_tree();
        tap(&mut tree, Offset::new(30.0, 24.0));

        assert_eq!(chosen.get(), 1, "the callback ran");
        assert!(
            !crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(BAR)),
            "and the root went, not just the panel the line was in"
        );
        assert!(!crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(NESTED)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_line_told_not_to_close_leaves_the_menu_up() {
        // Upstream's `closeOnActivate: false`, for an item that is a toggle
        // rather than a choice -- the reader is expected to press another.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(BAR));
            tree.open(BAR);
        });

        let mut tree = ElementTree::new();
        tree.rebuild(stateful(
            an_item()
                .in_menu(BAR, MENU_GROUP)
                .with_close_on_activate(false)
                .with_on_pressed(|| {}),
        ));
        tree.build_render_tree();
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert!(crate::raw_menu_anchor::with_menu_tree(
            |tree| tree.is_open(BAR)
        ));
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_line_that_is_in_no_menu_closes_nothing() {
        // `anchor_id` is `None` for a line built on its own -- a menu item in
        // a gallery page, say. Closing "the root" of nothing would be reaching
        // into whatever menu happened to be open elsewhere.
        crate::raw_menu_anchor::reset_menu_tree();
        crate::raw_menu_anchor::with_menu_tree_mut(|tree| {
            tree.insert(crate::raw_menu_anchor::MenuAnchorNode::new(BAR));
            tree.open(BAR);
        });

        let mut tree = ElementTree::new();
        tree.rebuild(stateful(an_item().with_on_pressed(|| {})));
        tree.build_render_tree();
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert!(
            crate::raw_menu_anchor::with_menu_tree(|tree| tree.is_open(BAR)),
            "somebody else's menu is not this line's to close"
        );
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn pressing_a_line_does_not_close_the_panel_it_is_in() {
        // The tap region. Without it a press on a line is a tap *outside* the
        // panel the line sits in, so the panel comes down on the way and the
        // press arrives at a menu that is already gone.
        crate::raw_menu_anchor::reset_menu_tree();
        let (mut tree, overlay) = staged(a_submenu());
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(overlay.entry_count(), 1, "the panel is up");

        // A line of that panel, in the same group, somewhere the panel is not.
        let chosen = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = std::rc::Rc::clone(&chosen);
        let line = overlay
            .insert(move || {
                let count = std::rc::Rc::clone(&count);
                crate::framework::many(
                    vec![stateful(
                        MenuItemButton::new()
                            .with_id(8405)
                            .with_label("Paste")
                            .in_menu(SUBMENU, MENU_GROUP)
                            .with_close_on_activate(false)
                            .with_on_pressed(move || count.set(count.get() + 1)),
                    )],
                    |rendered| {
                        crate::render::RenderAlign::new(
                            crate::render::Alignment::BOTTOM_RIGHT,
                            rendered.into_iter().next().expect("the line"),
                        )
                    },
                )
            })
            .expect("inserted");
        tree.rebuild_dirty();

        tap(&mut tree, Offset::new(380.0, 280.0));
        assert_eq!(chosen.get(), 1, "the line was pressed");
        assert_eq!(
            overlay.entry_count(),
            2,
            "and the panel it belongs to is still up"
        );
        overlay.remove(line);
        crate::raw_menu_anchor::reset_menu_tree();
    }

    #[test]
    fn a_line_is_at_least_the_minimum_a_menu_button_asks_for() {
        // `_MenuButtonDefaultsM3.minimumSize`, 64 x 48. The height is what
        // makes a menu tappable; the width is what stops a one-letter item
        // from being a sliver.
        let size = laid_out(an_item());
        assert_eq!(size.height, ResolvedMenuButton::MINIMUM_SIZE.height);
        assert!(
            size.width >= ResolvedMenuButton::MINIMUM_SIZE.width,
            "{size:?}"
        );
    }

    #[test]
    fn the_label_is_written_in_the_foreground_the_theme_resolved() {
        // All four arms of `foregroundColor` answer `onSurface`, so the label
        // does not move as the pointer crosses it -- the overlay is the whole
        // of the feedback. What this pins is that the *label* is that colour
        // at all, which nothing read before.
        let scheme = crate::theme::ThemeData::default().color_scheme;
        let drawn = painted(an_item());
        let label = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, argb, .. } if text == "Paste" => Some(Color(*argb)),
                _ => None,
            })
            .expect("the label painted");
        assert_eq!(label, scheme.on_surface);
    }

    #[test]
    fn a_leading_icon_pushes_the_label_along_by_one_gap() {
        // The gap is between two things: with no leading icon the label starts
        // at the line's own edge, and with one it starts a gap past the icon.
        let bare = painted(an_item());
        let with_icon = painted(an_item().with_leading(|| mark(MARK)));
        let bare_x = text_at(&bare, "Paste").expect("the label");
        let icon_left = colour_at(&with_icon, MARK).expect("the icon");
        let moved_x = text_at(&with_icon, "Paste").expect("the label");
        assert_eq!(icon_left, bare_x, "the icon starts where the text used to");
        assert!(
            (moved_x - (icon_left + 20.0 + MenuItemLabel::DEFAULT_SPACING)).abs() < 0.5,
            "the icon, then a gap, then the text: {moved_x} against {icon_left}"
        );
    }

    #[test]
    fn a_shortcut_is_written_after_the_label_and_after_a_gap() {
        let drawn = painted(an_item().with_shortcut("Ctrl+V"));
        let label = text_at(&drawn, "Paste").expect("the label");
        let shortcut = text_at(&drawn, "Ctrl+V").expect("the shortcut");
        assert!(shortcut > label, "after it: {shortcut} against {label}");
    }

    #[test]
    fn the_trailing_icon_comes_before_the_shortcut() {
        // `_MenuItemLabel` builds the trailing icon, then the shortcut. The
        // order is the widget's to keep, and nothing about the two says it.
        let drawn = painted(
            an_item()
                .with_trailing(|| mark(OTHER))
                .with_shortcut("Ctrl+V"),
        );
        let icon = colour_at(&drawn, OTHER).expect("the trailing icon");
        let shortcut = text_at(&drawn, "Ctrl+V").expect("the shortcut");
        assert!(icon < shortcut, "{icon} then {shortcut}");
    }

    /// A press and a release at `at`, through the real router.
    fn tap(tree: &mut ElementTree, at: Offset) {
        tap_in(tree, at, 400.0, 300.0);
    }

    /// [`tap`], in a window of the given size. The size matters: a press
    /// outside the laid-out root reaches nothing, and a test aimed there
    /// measures a tap that missed.
    fn tap_in(tree: &mut ElementTree, at: Offset, width: f32, height: f32) {
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(width, height));
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

    /// A press at `at` with **no release**.
    ///
    /// The release matters for one reader: the tap-region surface records
    /// whether the last dispatch was claimed, and the release is a dispatch
    /// too -- one that claims nothing and overwrites the answer. A test asking
    /// who claimed the press has to ask between the two.
    fn press_only(tree: &mut ElementTree, at: Offset) {
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        let mut router = crate::gestures::GestureRouter::new();
        router.dispatch(
            &root,
            &crate::gestures::PointerEvent {
                view_id: 0,
                device: 0,
                pointer_id: 1,
                change: crate::gestures::PointerChange::Down,
                kind: crate::gestures::PointerKind::Touch,
                signal_kind: crate::gestures::SignalKind::None,
                buttons: 1,
                time_stamp_micros: 0,
                position: at,
                delta: Offset::ZERO,
                scroll_delta: Offset::ZERO,
                pressure: 1.0,
                local_position: at,
            },
        );
        tree.rebuild_dirty();
    }

    /// A mouse moving onto `at`, through a router the caller keeps.
    ///
    /// The router is the caller's because an exit is a memory: a fresh one has
    /// never seen the pointer anywhere, so it reports arrivals and never
    /// departures, and a test that wants to watch something leave gets nothing
    /// at all.
    fn hover_using(
        router: &mut crate::gestures::GestureRouter,
        tree: &mut ElementTree,
        at: Offset,
    ) {
        {
            let root = tree.build_render_tree().expect("a root");
            crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
            crate::render::flush_layout();
            router.dispatch(
                &root,
                &crate::gestures::PointerEvent {
                    view_id: 0,
                    device: 0,
                    pointer_id: 1,
                    change: crate::gestures::PointerChange::Hover,
                    kind: crate::gestures::PointerKind::Mouse,
                    signal_kind: crate::gestures::SignalKind::None,
                    buttons: 0,
                    time_stamp_micros: 0,
                    position: at,
                    delta: Offset::ZERO,
                    scroll_delta: Offset::ZERO,
                    pressure: 0.0,
                    local_position: at,
                },
            );
            tree.rebuild_dirty();
        }
    }

    /// A mouse moving onto `at`.
    fn hover(tree: &mut ElementTree, at: Offset) {
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        let mut router = crate::gestures::GestureRouter::new();
        router.dispatch(
            &root,
            &crate::gestures::PointerEvent {
                view_id: 0,
                device: 0,
                pointer_id: 1,
                change: crate::gestures::PointerChange::Hover,
                kind: crate::gestures::PointerKind::Mouse,
                signal_kind: crate::gestures::SignalKind::None,
                buttons: 0,
                time_stamp_micros: 0,
                position: at,
                delta: Offset::ZERO,
                scroll_delta: Offset::ZERO,
                pressure: 0.0,
                local_position: at,
            },
        );
        tree.rebuild_dirty();
    }

    #[test]
    fn each_trailing_part_is_exactly_one_gap_along() {
        // Not merely "after": the gap is `_MenuItemLabel`'s single spacing,
        // spent once at each place two parts meet.
        let drawn = painted(
            an_item()
                .with_trailing(|| mark(OTHER))
                .with_shortcut("Ctrl+V"),
        );
        let label = text_at(&drawn, "Paste").expect("the label");
        let label_end = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::Paragraph { text, x, size, .. } if text == "Paste" => Some(*x + *size * 0.0),
                _ => None,
            })
            .expect("the label");
        let _ = label_end;
        let icon = colour_at(&drawn, OTHER).expect("the trailing icon");
        let shortcut = text_at(&drawn, "Ctrl+V").expect("the shortcut");
        assert!(
            (shortcut - (icon + 20.0 + MenuItemLabel::DEFAULT_SPACING)).abs() < 0.5,
            "the icon is 20 wide, then a gap, then the shortcut: \
             icon at {icon}, shortcut at {shortcut}"
        );
        assert!(icon > label, "and both are after the label");
    }

    #[test]
    fn a_disabled_line_is_written_in_a_faded_foreground() {
        // `_MenuButtonDefaultsM3.foregroundColor` has four arms that all
        // answer `onSurface` and one that does not: disabled fades it. So the
        // states the line resolves with have to carry `Disabled` at all --
        // resolving as though it were enabled would look identical everywhere
        // except here.
        let enabled = painted(an_item());
        let disabled = painted(an_item().with_enabled(false));
        let colour_of = |drawn: &[Drawn]| {
            drawn
                .iter()
                .find_map(|call| match call {
                    Drawn::Paragraph { text, argb, .. } if text == "Paste" => Some(Color(*argb)),
                    _ => None,
                })
                .expect("the label painted")
        };
        assert_ne!(
            colour_of(&disabled),
            colour_of(&enabled),
            "a disabled line does not look like an enabled one"
        );
        assert!(
            colour_of(&disabled).alpha() < colour_of(&enabled).alpha(),
            "and the difference is that it is faded"
        );
    }

    #[test]
    fn a_disabled_line_does_not_run_its_callback() {
        let pressed = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = std::rc::Rc::clone(&pressed);
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(
            an_item()
                .with_enabled(false)
                .with_on_pressed(move || count.set(count.get() + 1)),
        ));
        tree.build_render_tree();
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(pressed.get(), 0);

        // And the same tap on the same line, enabled, does run it -- or the
        // assertion above is about a tap that missed.
        let pressed = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = std::rc::Rc::clone(&pressed);
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(
            an_item().with_on_pressed(move || count.set(count.get() + 1)),
        ));
        tree.build_render_tree();
        tap(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(pressed.get(), 1);
    }

    #[test]
    fn the_pointer_moving_onto_a_line_gives_it_the_keyboard() {
        // Upstream's `requestFocusOnHover`, true by default: the line under
        // the cursor is the one Enter acts on. Without it, moving the mouse
        // down a menu and pressing Enter acts on whatever the keyboard was
        // left on.
        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(an_item()));
        tree.build_render_tree();
        hover(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(crate::focus::focused(), Some(ITEM));

        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(MenuItemButton {
            request_focus_on_hover: false,
            ..an_item()
        }));
        tree.build_render_tree();
        hover(&mut tree, Offset::new(30.0, 24.0));
        assert_eq!(
            crate::focus::focused(),
            None,
            "and a line told not to does not"
        );
    }

    #[test]
    fn a_disabled_line_takes_no_taps_and_lights_up_for_nobody() {
        // Two halves. The callback is the obvious one; the highlight is the
        // one worth pinning, because a disabled response still *makes* its
        // highlights -- at alpha zero, so that becoming enabled again is a
        // colour change rather than a highlight appearing from nowhere.
        let pressed = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = std::rc::Rc::clone(&pressed);
        let item = an_item()
            .with_enabled(false)
            .with_on_pressed(move || count.set(count.get() + 1));

        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(item));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        tree.advance_frame(0);
        tree.rebuild_dirty();

        crate::focus::focus(ITEM);
        tree.rebuild_dirty();
        tree.advance_frame(200_000);
        tree.rebuild_dirty();

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 300.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(400, 100);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(400.0, 100.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let lit = crate::engine_test_stubs::drawn().into_iter().any(|call| {
            matches!(call, Drawn::Rect { argb, .. } | Drawn::RRect { argb, .. }
                if Color(argb).alpha() > 0 && Color(argb) != Color::TRANSPARENT
                    && Color(argb).red() == 0 && Color(argb).green() == 0
                    && Color(argb).blue() == 0)
        });
        assert!(!lit, "a disabled line does not light up under the keyboard");
        assert_eq!(pressed.get(), 0);
        crate::focus::unfocus();
    }

    #[test]
    fn the_pointer_carries_the_keyboard_with_it() {
        // Upstream's `requestFocusOnHover`, true by default. Without it,
        // moving the mouse down a menu and pressing Enter acts on whatever the
        // keyboard was left on rather than on the line under the cursor.
        assert!(MenuItemButton::new().request_focus_on_hover);
    }

    // -- The line's geometry ------------------------------------------------

    use crate::theme::VisualDensity;

    fn density(horizontal: f32) -> VisualDensity {
        VisualDensity {
            horizontal,
            vertical: 0.0,
        }
    }

    #[test]
    fn the_gap_moves_at_twice_the_density() {
        // `_kLabelItemDefaultSpacing + density.horizontal * 2`. Twice, not
        // once: the horizontal squeeze of a compact menu comes from here while
        // the vertical one comes from the button's minimum size, and a menu
        // that tightened at the same rate in both directions would run out of
        // room across long before it did down.
        assert_eq!(MenuItemLabel::spacing(density(0.0)), 12.0);
        assert_eq!(MenuItemLabel::spacing(density(1.0)), 14.0);
        assert_eq!(MenuItemLabel::spacing(density(-1.0)), 10.0);
    }

    #[test]
    fn the_gap_has_a_floor_that_the_densest_menu_lands_exactly_on() {
        // At the minimum density of -4 the arithmetic gives 12 - 8 = 4, which
        // is `_kLabelItemMinSpacing` to the pixel. So the floor is not
        // reachable from below by any legal density -- it is there to stop the
        // gap going negative if either constant moves, and the two numbers
        // being in that relationship is the fact worth pinning.
        assert_eq!(
            MenuItemLabel::spacing(density(VisualDensity::MINIMUM)),
            MenuItemLabel::MIN_SPACING
        );
        assert_eq!(
            MenuItemLabel::spacing(density(VisualDensity::MINIMUM - 1.0)),
            MenuItemLabel::MIN_SPACING,
            "and past it the floor holds"
        );
    }

    #[test]
    fn the_label_is_only_padded_when_something_is_in_front_of_it() {
        // The gap is between two things, not an inset. A line with no leading
        // icon starts its text exactly where a line with one starts its icon,
        // so a column of items has one left edge rather than two.
        let plain = MenuItemLabel::new();
        assert_eq!(plain.leading_gap(density(0.0)), 0.0);
        assert_eq!(
            plain.with_leading_icon(true).leading_gap(density(0.0)),
            MenuItemLabel::DEFAULT_SPACING
        );
    }

    #[test]
    fn what_follows_the_label_comes_in_upstreams_order() {
        let full = MenuItemLabel::new()
            .with_trailing_icon(true)
            .with_shortcut(true);
        let mut full = full;
        full.has_submenu = true;
        assert_eq!(
            full.trailing_parts(),
            vec![
                MenuItemPart::TrailingIcon,
                MenuItemPart::Shortcut,
                MenuItemPart::SubmenuIcon
            ]
        );
    }

    #[test]
    fn a_bar_hides_the_shortcut_and_the_arrow_and_keeps_the_icon() {
        // `showDecoration: _orientation == Axis.vertical`. The two decorations
        // go together because both are the menu's own furniture; a trailing
        // icon is the caller's and stays.
        let mut line = MenuItemLabel::new()
            .with_trailing_icon(true)
            .with_shortcut(true);
        line.has_submenu = true;

        assert_eq!(line.in_a_horizontal_bar(false).trailing_parts().len(), 3);
        assert_eq!(
            line.in_a_horizontal_bar(true).trailing_parts(),
            vec![MenuItemPart::TrailingIcon],
            "the icon stays and the furniture goes"
        );
    }

    #[test]
    fn the_gaps_add_up_to_one_per_join() {
        // What a panel needs in order to be as wide as its widest line: the
        // parts plus a gap at each place two of them meet.
        let bare = MenuItemLabel::new();
        assert_eq!(bare.total_gaps(density(0.0)), 0.0, "a label on its own");

        let mut busy = MenuItemLabel::new()
            .with_leading_icon(true)
            .with_trailing_icon(true)
            .with_shortcut(true);
        busy.has_submenu = true;
        assert_eq!(
            busy.total_gaps(density(0.0)),
            4.0 * MenuItemLabel::DEFAULT_SPACING,
            "one before the label and one before each of the three after it"
        );
        assert_eq!(
            busy.in_a_horizontal_bar(true).total_gaps(density(0.0)),
            2.0 * MenuItemLabel::DEFAULT_SPACING,
            "and in a bar, one before the label and one before the icon"
        );
    }

    #[test]
    fn an_item_never_has_a_submenu_and_a_submenu_button_does() {
        // The one difference between the two lines upstream builds from the
        // same `_MenuItemLabel`.
        assert!(
            !MenuItemButton::new()
                .label(false, false, false, false)
                .has_submenu
        );
        assert!(
            SubmenuButton::new()
                .label(false, false, false, false)
                .has_submenu
        );
        assert!(
            !SubmenuButton {
                has_submenu_icon: false,
                ..SubmenuButton::new()
            }
            .label(false, false, false, false)
            .has_submenu,
            "a submenu with no arrow slot has no arrow"
        );
    }

    #[test]
    fn a_submenus_arrow_takes_the_same_gap_as_anything_else_after_the_label() {
        // The arrow is a trailing part like the others, not a special case
        // with a spacing of its own.
        let submenu = SubmenuButton::new().label(false, false, false, false);
        assert_eq!(submenu.trailing_parts(), vec![MenuItemPart::SubmenuIcon]);
        assert_eq!(
            submenu.total_gaps(density(0.0)),
            MenuItemLabel::DEFAULT_SPACING
        );
        assert_eq!(
            SubmenuButton::new()
                .label(false, false, false, true)
                .total_gaps(density(0.0)),
            0.0,
            "and in a bar it is not there at all -- which is why a menu bar's \
             top-level entries are bare words though every one opens a submenu"
        );
    }

    fn stripped(label: &str) -> (String, Option<usize>) {
        MenuAcceleratorLabel::strip_accelerator_markers(label)
    }

    // -- Opening and closing, tick 319 -------------------------------------

    use crate::animation::AnimationStatus::{Completed, Dismissed, Forward, Reverse};

    const EVERY_STATE: [crate::animation::AnimationStatus; 4] =
        [Dismissed, Forward, Reverse, Completed];

    #[test]
    fn a_menu_that_has_finished_closing_is_not_closing() {
        // Three predicates over the same four states, and no two of them are
        // the same set.
        assert!(is_closing(Reverse));
        assert!(!is_closing(Dismissed), "closed, which is not closing");
        assert!(!is_closing(Forward) && !is_closing(Completed));

        assert!(is_closing_or_closed(Dismissed) && is_closing_or_closed(Reverse));
        assert!(!is_closing_or_closed(Forward) && !is_closing_or_closed(Completed));

        // The two differ on exactly one state, which is the one the parent
        // guard turns on.
        let differ: Vec<_> = EVERY_STATE
            .iter()
            .filter(|status| is_closing(**status) != is_closing_or_closed(**status))
            .collect();
        assert_eq!(differ, vec![&Dismissed]);
    }

    #[test]
    fn a_closing_parent_blocks_a_submenu_and_a_closed_one_does_not() {
        // It is a race, not a state check: a closing parent is on its way to
        // taking the child down with it. A dismissed parent is just a menu.
        let blocked = menu_open_request(Some(Reverse), Dismissed);
        assert!(!blocked.shows_overlay && !blocked.starts_animation);

        let allowed = menu_open_request(Some(Dismissed), Dismissed);
        assert!(
            allowed.shows_overlay && allowed.starts_animation,
            "a shut parent is not an obstacle"
        );

        for parent in [Forward, Completed] {
            assert!(menu_open_request(Some(parent), Dismissed).shows_overlay);
        }
        assert!(
            menu_open_request(None, Dismissed).shows_overlay,
            "no parent"
        );
    }

    #[test]
    fn the_overlay_goes_up_even_when_the_animation_is_skipped() {
        // showOverlay() runs before the animation is looked at. Folding the
        // two into one early return is the natural simplification and loses
        // the case where the entry was taken down while the animation stayed
        // at its end.
        let already = menu_open_request(None, Completed);
        assert!(already.shows_overlay, "still shown");
        assert!(!already.starts_animation, "but not animated again");
    }

    #[test]
    fn a_menu_caught_mid_close_re_opens_from_where_it_got_to() {
        // `reverse` is not forward-or-completed. Asking "is it visible?"
        // would have said yes and left it closing.
        let reopened = menu_open_request(None, Reverse);
        assert!(reopened.starts_animation);
        assert!(
            !menu_open_request(None, Forward).starts_animation,
            "and one already opening is left to finish"
        );
    }

    #[test]
    fn closing_something_already_closing_does_nothing_at_all() {
        // Restarting the reverse would jump it back to full size, and the
        // completion callback that hides the overlay would be armed twice.
        assert!(!menu_close_request(Reverse));
        assert!(!menu_close_request(Dismissed), "nothing left to hide");
        assert!(menu_close_request(Forward), "caught mid-open, so turn back");
        assert!(menu_close_request(Completed));
    }

    #[test]
    fn the_open_and_close_guards_are_exact_mirrors() {
        // Every state either starts an open or starts a close, never both and
        // never neither -- which is what makes the pair total.
        for status in EVERY_STATE {
            assert_ne!(
                menu_open_request(None, status).starts_animation,
                menu_close_request(status),
                "{status:?}"
            );
        }
    }

    #[test]
    fn the_marker_is_taken_out_and_the_letter_after_it_is_the_accelerator() {
        assert_eq!(stripped("&Save"), ("Save".to_string(), Some(0)));
        assert_eq!(stripped("Save &As..."), ("Save As...".to_string(), Some(5)));
        assert_eq!(stripped("Save"), ("Save".to_string(), None));
    }

    #[test]
    fn a_doubled_ampersand_is_a_literal_one_and_marks_nothing() {
        // Which is what a label like "Search && Replace" needs: one ampersand
        // on screen and no underlined letter.
        assert_eq!(
            stripped("Search && Replace"),
            ("Search & Replace".to_string(), None)
        );
        assert!(!MenuAcceleratorLabel::new("Search && Replace").has_accelerator());
    }

    #[test]
    fn an_ampersand_before_a_space_marks_nothing_either() {
        // There is no letter there to underline.
        let (display, index) = stripped("Save & Quit");
        assert_eq!(display, "Save  Quit", "the marker still comes out");
        assert_eq!(index, None);
    }

    #[test]
    fn a_bare_ampersand_at_the_very_end_disappears() {
        // Upstream's comment calls it "just treated as a quoted ampersand",
        // but the code breaks out of the loop without writing it, so it is
        // dropped rather than shown. Ported as written, and pinned here so the
        // disagreement between the comment and the code is not mistaken for a
        // porting slip.
        assert_eq!(stripped("Save&"), ("Save".to_string(), None));
        assert_eq!(stripped("&"), (String::new(), None));
    }

    #[test]
    fn only_the_first_eligible_marker_counts() {
        // A second &Letter is stripped like the first but does not move the
        // index -- a label has one accelerator or none.
        let (display, index) = stripped("&Save &As");
        assert_eq!(display, "Save As");
        assert_eq!(index, Some(0), "the S, not the A");
    }

    #[test]
    fn the_index_is_into_the_stripped_string_and_not_the_original() {
        // Every quoted ampersand before the marker shifts it, and the index is
        // reduced to match. Getting this wrong underlines the wrong letter,
        // and only in labels that also contain a literal ampersand.
        let (display, index) = stripped("A && B &Cut");
        assert_eq!(display, "A & B Cut");
        let index = index.expect("there is one");
        assert_eq!(
            display.chars().nth(index),
            Some('C'),
            "index {index} into {display:?}"
        );
    }

    #[test]
    fn the_index_still_lands_on_the_right_letter_with_several_quoted_ampersands() {
        let (display, index) = stripped("&& && &X");
        assert_eq!(display, "& & X");
        let index = index.expect("there is one");
        assert_eq!(display.chars().nth(index), Some('X'));
    }

    #[test]
    fn a_label_has_an_accelerator_exactly_when_a_marker_survives_the_rules() {
        // Upstream asks this with a regular expression while stripping with a
        // loop; this port derives it from the loop, so what is worth pinning
        // is the *answer* for each shape of label rather than an agreement
        // between two implementations.
        for (label, expected) in [
            ("&Save", true),
            ("Save &As", true),
            ("Save", false),
            ("Search && Replace", false),
            ("Save & Quit", false),
            ("Save&", false),
            ("&", false),
            ("&& &X", true),
        ] {
            assert_eq!(
                MenuAcceleratorLabel::new(label).has_accelerator(),
                expected,
                "{label:?}"
            );
        }
    }

    #[test]
    fn the_display_label_is_what_a_reader_sees() {
        let label = MenuAcceleratorLabel::new("&Open Recent");
        assert_eq!(label.display_label(), "Open Recent");
        assert_eq!(label.label, "&Open Recent", "and the original is kept");
        assert!(label.has_accelerator());
    }

    #[test]
    fn a_marker_on_a_multi_byte_letter_still_indexes_by_character() {
        // Upstream uses `characters` so as not to split a surrogate pair. The
        // same care in Rust means indexing by char rather than by byte.
        let (display, index) = stripped("Ré&sumé");
        assert_eq!(display, "Résumé");
        let index = index.expect("there is one");
        assert_eq!(display.chars().nth(index), Some('s'));
    }

    #[test]
    fn a_submenu_and_an_item_do_different_things_with_their_letter() {
        // Which is why the binding carries hasSubmenu: the first is invoked
        // and the menu closes, the second opens its submenu and stays.
        let item = MenuAcceleratorCallbackBinding::new(true, false);
        let submenu = SubmenuButton::new().accelerator_binding();
        assert!(!item.has_submenu);
        assert!(submenu.has_submenu);
        assert!(submenu.has_on_invoke);
        assert!(item.update_should_notify(&submenu));
    }

    #[test]
    fn the_binding_notifies_only_on_a_real_change() {
        let binding = MenuAcceleratorCallbackBinding::new(true, false);
        assert!(!binding.update_should_notify(&MenuAcceleratorCallbackBinding::new(true, false)));
        assert!(binding.update_should_notify(&MenuAcceleratorCallbackBinding::new(false, false)));
        assert!(binding.update_should_notify(&MenuAcceleratorCallbackBinding::new(true, true)));
    }

    #[test]
    fn a_bars_menus_are_meant_to_hang_below_it() {
        // Upstream's MenuBar defaults to no clipping where MenuAnchor defaults
        // to hardEdge, and the difference is the whole point.
        assert!(!MenuBar::new().clip);
        assert!(MenuBar::new().with_clip(true).clip);
    }

    #[test]
    fn a_menu_item_closes_the_menu_when_pressed_unless_told_not_to() {
        // Pressing an item is normally the end of the interaction.
        assert!(MenuItemButton::new().close_on_activate);
        assert!(
            !MenuItemButton::new()
                .with_close_on_activate(false)
                .close_on_activate
        );
        assert!(MenuItemButton::new().enabled);
    }

    #[test]
    fn a_submenu_is_allowed_to_be_wider_than_the_space_beside_its_parent() {
        // A menu item wrapped onto two lines is worse than one that overhangs.
        assert!(MenuAnchor::new().cross_axis_unconstrained);
        assert!(
            !MenuAnchor::new()
                .with_cross_axis_unconstrained(false)
                .cross_axis_unconstrained
        );

        // And a tap that dismisses a menu does not by default also press what
        // was under it.
        assert!(!MenuAnchor::new().consume_outside_tap);
        assert!(
            MenuAnchor::new()
                .with_consume_outside_tap(true)
                .consume_outside_tap
        );
        assert_eq!(MenuAnchor::new().alignment_offset, Offset::ZERO);
    }

    #[test]
    fn a_checkbox_menu_item_cycles_the_way_a_checkbox_does() {
        let plain = CheckboxMenuButton::new(Some(false));
        assert_eq!(plain.next_value(), Some(true));
        assert_eq!(
            CheckboxMenuButton::new(Some(true)).next_value(),
            Some(false),
            "two states without tristate"
        );

        let tri = CheckboxMenuButton::new(Some(true)).with_tristate(true);
        assert_eq!(tri.next_value(), None, "and a real third state with it");
        assert_eq!(
            CheckboxMenuButton::new(None)
                .with_tristate(true)
                .next_value(),
            Some(false)
        );
    }

    #[test]
    fn a_radio_in_a_group_cannot_normally_be_turned_off_by_pressing_it_again() {
        // The group is meant to have an answer, which is what keeps a required
        // choice required.
        let selected = RadioMenuButton::new(2).with_group_value(2);
        assert!(selected.is_selected());
        assert_eq!(selected.next_group_value(), Some(2), "it stays");

        let toggleable = RadioMenuButton::new(2)
            .with_group_value(2)
            .with_toggleable(true);
        assert_eq!(toggleable.next_group_value(), None, "unless told otherwise");

        // Pressing one that is not selected always selects it.
        let other = RadioMenuButton::new(3).with_group_value(2);
        assert!(!other.is_selected());
        assert_eq!(other.next_group_value(), Some(3));
        assert_eq!(
            RadioMenuButton::new(3)
                .with_group_value(2)
                .with_toggleable(true)
                .next_group_value(),
            Some(3),
            "toggleable or not"
        );
    }
}

#[cfg(test)]
mod menu_theme_tests {
    use super::*;
    use crate::component_themes::{
        ButtonStyle, MenuBarTheme, MenuBarThemeData, MenuButtonTheme, MenuButtonThemeData,
        MenuPanelAxis, MenuStyle, MenuTheme, MenuThemeData, ResolvedMenuButton, ResolvedMenuPanel,
    };
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, leaf};
    use crate::render::{AlignmentDirectional, AlignmentGeometry, EdgeInsets, Size};
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

    fn panel(axis: MenuPanelAxis) -> ResolvedMenuPanel {
        read_under(
            |child| child,
            move |context| ResolvedMenuPanel::of(context, axis, None),
        )
    }

    // -- The axis picks the theme ----------------------------------------------

    #[test]
    fn a_bar_theme_moves_the_horizontal_panel_and_not_the_vertical_one() {
        // Upstream switches on the orientation, so it never consults the theme
        // it is not using. Both wrapped at once, disagreeing, so the switch is
        // what decides and not which one happens to be present.
        let bar_colour = Color(0xFF110000);
        let menu_colour = Color(0xFF001100);
        let mut bar = MenuStyle::new();
        bar.background_color = Some(StateProperty::all(Some(bar_colour)));
        let mut menu = MenuStyle::new();
        menu.background_color = Some(StateProperty::all(Some(menu_colour)));

        let wrap = move |child: AnyWidget| {
            MenuBarTheme::new(
                MenuBarThemeData {
                    style: Some(bar.clone()),
                },
                MenuTheme::new(
                    MenuThemeData {
                        style: Some(menu.clone()),
                    },
                    child,
                ),
            )
        };
        assert_eq!(
            read_under(wrap.clone(), |context| ResolvedMenuPanel::of(
                context,
                MenuPanelAxis::Horizontal,
                None
            ))
            .background_color,
            Some(bar_colour)
        );
        assert_eq!(
            read_under(wrap, |context| ResolvedMenuPanel::of(
                context,
                MenuPanelAxis::Vertical,
                None
            ))
            .background_color,
            Some(menu_colour)
        );
    }

    #[test]
    fn the_two_defaults_differ_in_exactly_two_fields() {
        // The claim the type's docs make, checked field by field rather than
        // asserted in prose.
        let bar = panel(MenuPanelAxis::Horizontal);
        let menu = panel(MenuPanelAxis::Vertical);

        assert_eq!(bar.background_color, menu.background_color);
        assert_eq!(bar.shadow_color, menu.shadow_color);
        assert_eq!(bar.surface_tint_color, menu.surface_tint_color);
        assert_eq!(bar.elevation, menu.elevation);
        assert_eq!(bar.shape, menu.shape);
        assert_eq!(bar.visual_density, menu.visual_density);
        assert_eq!(bar.minimum_size, menu.minimum_size);
        assert_eq!(bar.fixed_size, menu.fixed_size);
        assert_eq!(bar.maximum_size, menu.maximum_size);
        assert_eq!(bar.side, menu.side);

        assert_ne!(bar.alignment, menu.alignment);
        assert_ne!(bar.padding, menu.padding);
    }

    #[test]
    fn and_both_differences_are_the_axis() {
        // A row is padded at the ends of a row; a column at the ends of a
        // column. A bar's submenu drops below it; a menu's flies out beside it.
        let bar = panel(MenuPanelAxis::Horizontal);
        let menu = panel(MenuPanelAxis::Vertical);

        assert_eq!(bar.padding, EdgeInsets::symmetric(4.0, 0.0));
        assert_eq!(bar.padding.top, 0.0, "a bar is not padded across its run");
        assert_eq!(menu.padding, EdgeInsets::symmetric(0.0, 8.0));
        assert_eq!(menu.padding.left, 0.0, "nor is a menu");

        assert_eq!(
            bar.alignment,
            AlignmentGeometry::Directional(AlignmentDirectional::BOTTOM_START)
        );
        assert_eq!(
            menu.alignment,
            AlignmentGeometry::Directional(AlignmentDirectional::TOP_END)
        );
    }

    #[test]
    fn a_panel_is_asked_as_though_nothing_were_happening() {
        // Upstream resolves with `<WidgetState>{}` unconditionally. A panel is
        // a surface: it is not hovered, its items are.
        let resting = Color(0xFF010101);
        let hovered = Color(0xFF020202);
        let mut style = MenuStyle::new();
        style.background_color = Some(StateProperty::resolve_with(move |states| {
            Some(if states.contains(WidgetState::Hovered) {
                hovered
            } else {
                resting
            })
        }));
        let resolved = read_under(
            move |child| {
                MenuTheme::new(
                    MenuThemeData {
                        style: Some(style.clone()),
                    },
                    child,
                )
            },
            |context| ResolvedMenuPanel::of(context, MenuPanelAxis::Vertical, None),
        );
        assert_eq!(resolved.background_color, Some(resting));
        assert_ne!(resolved.background_color, Some(hovered));
    }

    #[test]
    fn the_zero_after_the_elevation_chain_cannot_be_reached() {
        // `resolve(...elevation) ?? 0` is a fourth step the chain never falls
        // out of: the defaults supply 3, and a style whose elevation resolves
        // to null falls through to that rather than past it.
        let mut style = MenuStyle::new();
        style.elevation = Some(StateProperty::all(None));
        for axis in [MenuPanelAxis::Horizontal, MenuPanelAxis::Vertical] {
            let resolved = read_under(
                {
                    let style = style.clone();
                    move |child| MenuTheme::new(MenuThemeData { style: Some(style) }, child)
                },
                move |context| ResolvedMenuPanel::of(context, axis, None),
            );
            assert_eq!(resolved.elevation, ResolvedMenuPanel::ELEVATION);
            assert_ne!(resolved.elevation, ResolvedMenuPanel::UNREACHABLE_ELEVATION);
        }
    }

    #[test]
    fn the_widget_is_the_first_step_and_the_theme_the_second() {
        let mine = Color(0xFF123456);
        let theirs = Color(0xFF654321);
        let mut widget = MenuStyle::new();
        widget.background_color = Some(StateProperty::all(Some(mine)));
        let mut themed = MenuStyle::new();
        themed.background_color = Some(StateProperty::all(Some(theirs)));

        let wrap = move |child: AnyWidget| {
            MenuTheme::new(
                MenuThemeData {
                    style: Some(themed.clone()),
                },
                child,
            )
        };
        assert_eq!(
            read_under(wrap.clone(), move |context| {
                ResolvedMenuPanel::of(context, MenuPanelAxis::Vertical, Some(&widget))
            })
            .background_color,
            Some(mine)
        );
        assert_eq!(
            read_under(wrap, |context| ResolvedMenuPanel::of(
                context,
                MenuPanelAxis::Vertical,
                None
            ))
            .background_color,
            Some(theirs)
        );
    }

    // -- One line of a menu ----------------------------------------------------

    fn line(states: WidgetStates) -> ResolvedMenuButton {
        read_under(
            |child| child,
            move |context| ResolvedMenuButton::of(context, states),
        )
    }

    fn states(list: &[WidgetState]) -> WidgetStates {
        WidgetStates::of(list)
    }

    #[test]
    fn neither_the_label_nor_the_icon_reacts_to_anything_but_being_disabled() {
        // Four arms upstream, all returning the same colour. A menu line that
        // recoloured its text would flicker as the pointer crossed it.
        let resting = line(WidgetStates::NONE);
        for interaction in [
            states(&[WidgetState::Pressed]),
            states(&[WidgetState::Hovered]),
            states(&[WidgetState::Focused]),
            states(&[WidgetState::Hovered, WidgetState::Focused]),
        ] {
            let touched = line(interaction);
            assert_eq!(touched.foreground, resting.foreground);
            assert_eq!(touched.icon_color, resting.icon_color);
        }

        let off = line(states(&[WidgetState::Disabled]));
        assert_ne!(off.foreground, resting.foreground);
        assert_ne!(off.icon_color, resting.icon_color);
    }

    #[test]
    fn the_overlay_is_the_whole_of_the_feedback() {
        // And it does move -- otherwise the test above would only prove that
        // nothing anywhere reacts.
        let resting = line(WidgetStates::NONE);
        assert_eq!(resting.overlay, Color::TRANSPARENT);

        let scheme = ThemeData::fallback().color_scheme;
        let pressed = line(states(&[WidgetState::Pressed]));
        let hovered = line(states(&[WidgetState::Hovered]));
        let focused = line(states(&[WidgetState::Focused]));

        assert_eq!(
            pressed.overlay,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.1)
        );
        assert_eq!(
            hovered.overlay,
            crate::elevation_overlay::with_opacity(scheme.on_surface, 0.08)
        );
        assert_ne!(
            hovered.overlay, pressed.overlay,
            "hovering is the lighter one"
        );
        assert_eq!(
            focused.overlay, pressed.overlay,
            "pressed and focused agree; only hovering is weaker"
        );
    }

    #[test]
    fn pressing_beats_hovering_when_both_are_true() {
        // The order of the arms, which is only visible where the values differ
        // -- and a pointer that presses is always also hovering.
        let both = line(states(&[WidgetState::Pressed, WidgetState::Hovered]));
        assert_eq!(both.overlay, line(states(&[WidgetState::Pressed])).overlay);
        assert_ne!(both.overlay, line(states(&[WidgetState::Hovered])).overlay);
    }

    #[test]
    fn hovering_beats_being_focused_when_both_are_true() {
        // The other order in the ladder. Pressed and focused agree, so this is
        // the only pair below the top that a swap could show.
        let both = line(states(&[WidgetState::Hovered, WidgetState::Focused]));
        assert_eq!(both.overlay, line(states(&[WidgetState::Hovered])).overlay);
        assert_ne!(both.overlay, line(states(&[WidgetState::Focused])).overlay);
    }

    #[test]
    fn the_label_is_stronger_than_the_icon() {
        let scheme = ThemeData::fallback().color_scheme;
        let resting = line(WidgetStates::NONE);
        assert_eq!(resting.foreground, scheme.on_surface);
        assert_eq!(resting.icon_color, scheme.on_surface_variant());
        assert_ne!(resting.foreground, resting.icon_color);
    }

    #[test]
    fn a_line_paints_no_background_of_its_own() {
        // It sits on the panel's; painting one would draw the panel twice.
        let resting = line(WidgetStates::NONE);
        assert_eq!(resting.background, Color::TRANSPARENT);
        assert_eq!(resting.elevation, 0.0);
        assert_eq!(resting.minimum_size, Size::new(64.0, 48.0));
        assert_eq!(resting.icon_size, 24.0);
    }

    #[test]
    fn both_kinds_of_line_read_the_one_theme() {
        // `MenuItemButton` and `SubmenuButton` share `MenuButtonTheme` and
        // `_MenuButtonDefaultsM3` -- two widgets, one theme, the mirror of the
        // panel's one widget and two themes.
        let mine = Color(0xFF00FFFF);
        let mut style = ButtonStyle::new();
        style.foreground_color = Some(StateProperty::all(Some(mine)));
        let data = MenuButtonThemeData { style: Some(style) };

        let item = read_under(
            {
                let data = data.clone();
                move |child| MenuButtonTheme::new(data, child)
            },
            |context| MenuItemButton::new().resolved(context, WidgetStates::NONE),
        );
        let submenu = read_under(
            move |child| MenuButtonTheme::new(data, child),
            |context| SubmenuButton::new().resolved(context, WidgetStates::NONE),
        );
        assert_eq!(item.foreground, mine);
        assert_eq!(submenu.foreground, mine);
    }
}
