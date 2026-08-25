//! The focus tree -- a port of `FocusNode` and `FocusScopeNode` from
//! upstream's `widgets/focus_manager.dart`.
//!
//! Focus is a **tree**, and exactly one node in it holds the primary focus.
//! Every other node that is an ancestor of that one "has focus" too, which is
//! the distinction the file turns on: `hasFocus` means *somewhere in the
//! chain*, `hasPrimaryFocus` means *at the end of it*.
//!
//! Scopes are the other half. A [`FocusScopeNode`] remembers **a stack** of
//! the children that have held focus, not just the last one, so that removing
//! the focused child returns focus to the one before it rather than to
//! nowhere. Almost every awkward-looking line in `unfocus` is maintaining that
//! stack against the ways it can go stale.
//!
//! ## What is not here
//!
//! `FocusManager`, the key-event dispatch, the highlight mode and
//! `FocusAttachment`'s tie to a `BuildContext` are elsewhere -- see
//! [`crate::focus`]. What is ported is the tree, the focusability rules, and
//! the request and unfocus paths.

use std::collections::HashMap;

/// Upstream `UnfocusDisposition`: where focus goes when a node gives it up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnfocusDisposition {
    /// **The default.** Focus the enclosing scope itself, and **clear its
    /// history** on the way.
    ///
    /// Clearing is what makes a following `nextFocus` pick what the traversal
    /// policy thinks should be first, rather than resuming where the reader
    /// was. That is right for "I am done with this field" and wrong for
    /// "temporarily take focus away", which is what the other disposition is
    /// for.
    #[default]
    Scope,
    /// Walk up to the nearest focusable scope, then walk **back down** through
    /// each scope's focused child until reaching a leaf. Focus that.
    PreviouslyFocusedChild,
}

/// One node of the focus tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusNode {
    pub id: u64,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    /// Whether this node is a [`FocusScopeNode`].
    pub is_scope: bool,
    /// Upstream's `_canRequestFocus`, before ancestors are consulted.
    can_request_focus: bool,
    /// Upstream's `_descendantsAreFocusable`, which does **not** affect this
    /// node -- only everything under it.
    ///
    /// The stored field, not the answer. On a scope the getter is an override
    /// -- see [`FocusTree::descendants_are_focusable`].
    descendants_are_focusable: bool,
    /// Upstream's `skipTraversal`: still focusable, just skipped by the
    /// traversal policy. A node reachable by tap but not by tab.
    pub skip_traversal: bool,
    descendants_are_traversable: bool,
    /// Upstream's `_hasKeyboardToken`.
    has_keyboard_token: bool,
    /// Upstream's `_requestFocusWhenReparented`.
    request_focus_when_reparented: bool,
    /// A scope's stack of children that have held focus, most recent last.
    focused_children: Vec<u64>,
}

impl FocusNode {
    pub fn new(id: u64) -> FocusNode {
        FocusNode {
            id,
            parent: None,
            children: Vec::new(),
            is_scope: false,
            can_request_focus: true,
            descendants_are_focusable: true,
            skip_traversal: false,
            descendants_are_traversable: true,
            has_keyboard_token: false,
            request_focus_when_reparented: false,
            focused_children: Vec::new(),
        }
    }

    pub fn scope(id: u64) -> FocusNode {
        FocusNode {
            is_scope: true,
            ..FocusNode::new(id)
        }
    }

    pub fn with_can_request_focus(mut self, can: bool) -> Self {
        self.can_request_focus = can;
        self
    }

    pub fn with_descendants_are_focusable(mut self, focusable: bool) -> Self {
        self.descendants_are_focusable = focusable;
        self
    }

    /// Upstream's `_hasKeyboardToken`, and the mechanism it serves is worth
    /// stating: it distinguishes **a field focused by default from one focused
    /// by an explicit user action**.
    ///
    /// A node gets a token when it requests focus. The widget managing the
    /// text input shows the keyboard only if it can *consume* one. So a form
    /// that autofocuses its first field does not throw the keyboard up over
    /// half the screen before the reader has asked to type.
    pub fn has_keyboard_token(&self) -> bool {
        self.has_keyboard_token
    }

    /// Upstream's `consumeKeyboardToken`, which returns whether there was one
    /// and takes it.
    pub fn consume_keyboard_token(&mut self) -> bool {
        if !self.has_keyboard_token {
            return false;
        }
        self.has_keyboard_token = false;
        true
    }

    pub fn will_request_focus_when_reparented(&self) -> bool {
        self.request_focus_when_reparented
    }

    /// A scope's focused child: the **top of the stack**, not the only entry.
    pub fn focused_child(&self) -> Option<u64> {
        self.focused_children.last().copied()
    }

    pub fn focused_children(&self) -> &[u64] {
        &self.focused_children
    }
}

/// Upstream `FocusScopeNode`: a focus node that also keeps a focused-child
/// stack.
///
/// A typed handle onto a node in the tree rather than a separate object,
/// because a scope **is** a [`FocusNode`] -- upstream subclasses it. What the
/// handle buys is that the scope-only operations say so: `focused_child`,
/// `set_first_focus` and `autofocus` are meaningless on an ordinary node, and
/// asking for a handle is where that gets checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusScopeNode {
    id: u64,
}

impl FocusScopeNode {
    /// `None` if the node is not a scope.
    pub fn of(tree: &FocusTree, id: u64) -> Option<FocusScopeNode> {
        tree.node(id)
            .filter(|node| node.is_scope)
            .map(|_| FocusScopeNode { id })
    }

    pub fn id(self) -> u64 {
        self.id
    }

    /// Upstream's `focusedChild`: the top of the stack.
    pub fn focused_child(self, tree: &FocusTree) -> Option<u64> {
        tree.node(self.id).and_then(|node| node.focused_child())
    }

    /// Upstream's `isFirstFocus`: whether this scope is the focused child of
    /// the scope above it.
    pub fn is_first_focus(self, tree: &FocusTree) -> bool {
        tree.enclosing_scope(self.id)
            .and_then(|parent| tree.node(parent))
            .and_then(|parent| parent.focused_child())
            == Some(self.id)
    }

    /// Upstream's `traversalChildren`.
    pub fn traversal_children(self, tree: &FocusTree) -> Vec<u64> {
        tree.traversal_children(self.id)
    }

    /// Upstream's `setFirstFocus`.
    pub fn set_first_focus(self, tree: &mut FocusTree, child: u64) {
        debug_assert!(
            child != self.id,
            "unexpected self-reference in setFirstFocus"
        );
        tree.set_first_focus(self.id, child);
    }

    /// Upstream's `autofocus`.
    pub fn autofocus(self, tree: &mut FocusTree, node: u64) {
        tree.autofocus(self.id, node);
    }

    /// Upstream's `FocusScopeNode._doRequestFocus`.
    pub fn request_focus(self, tree: &mut FocusTree, find_first_focus: bool) {
        tree.request_scope_focus(self.id, find_first_focus);
    }
}

/// The focus tree, and the primary focus within it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FocusTree {
    nodes: HashMap<u64, FocusNode>,
    root: Option<u64>,
    primary_focus: Option<u64>,
    /// Upstream's `_markedForFocus`: the node that will be focused when the
    /// change is applied. Focus moves **between** frames, not during one.
    marked_for_focus: Option<u64>,
}

impl FocusTree {
    /// Builds a tree with `root` as the root scope.
    pub fn new(root: FocusNode) -> FocusTree {
        let id = root.id;
        let mut nodes = HashMap::new();
        nodes.insert(id, root);
        FocusTree {
            nodes,
            root: Some(id),
            primary_focus: Some(id),
            marked_for_focus: None,
        }
    }

    pub fn node(&self, id: u64) -> Option<&FocusNode> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: u64) -> Option<&mut FocusNode> {
        self.nodes.get_mut(&id)
    }

    pub fn primary_focus(&self) -> Option<u64> {
        self.primary_focus
    }

    pub fn marked_for_focus(&self) -> Option<u64> {
        self.marked_for_focus
    }

    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// Adds `child` under `parent`.
    ///
    /// Upstream's `_reparent` is also where a deferred focus request is
    /// honoured: a node that asked for focus before it was in the tree gets it
    /// **the next time it is reparented**, once. That is what lets a widget
    /// call `requestFocus` in `initState`.
    pub fn attach(&mut self, mut child: FocusNode, parent: u64) {
        let deferred = child.request_focus_when_reparented;
        child.parent = Some(parent);
        child.request_focus_when_reparented = false;
        let id = child.id;
        self.nodes.insert(id, child);
        if let Some(parent) = self.nodes.get_mut(&parent) {
            if !parent.children.contains(&id) {
                parent.children.push(id);
            }
        }
        if deferred {
            self.request_focus(id);
        }
    }

    /// Upstream's `ancestors`, nearest first.
    pub fn ancestors(&self, id: u64) -> Vec<u64> {
        let mut found = Vec::new();
        let mut at = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(current) = at {
            found.push(current);
            at = self.nodes.get(&current).and_then(|node| node.parent);
        }
        found
    }

    /// Upstream's `enclosingScope`: the nearest scope **above** this node.
    pub fn enclosing_scope(&self, id: u64) -> Option<u64> {
        self.ancestors(id)
            .into_iter()
            .find(|ancestor| self.nodes.get(ancestor).is_some_and(|node| node.is_scope))
    }

    /// Upstream's `nearestScope`, which **includes this node** if it is one.
    /// The two differ by exactly that, and mixing them up is how a scope ends
    /// up looking for itself.
    pub fn nearest_scope(&self, id: u64) -> Option<u64> {
        if self.nodes.get(&id).is_some_and(|node| node.is_scope) {
            return Some(id);
        }
        self.enclosing_scope(id)
    }

    /// Upstream's `canRequestFocus`:
    ///
    /// ```dart
    /// _canRequestFocus && ancestors.every(_allowDescendantsToBeFocused)
    /// ```
    ///
    /// A node is focusable only if **it** says so and **every** ancestor
    /// permits its descendants to be. One `descendantsAreFocusable: false`
    /// anywhere above turns off an entire subtree, which is how a disabled
    /// panel disables everything inside it without touching any of them.
    pub fn can_request_focus(&self, id: u64) -> bool {
        let Some(node) = self.nodes.get(&id) else {
            return false;
        };
        node.can_request_focus
            && self
                .ancestors(id)
                .into_iter()
                .all(|ancestor| self.descendants_are_focusable(ancestor))
    }

    /// Upstream's `descendantsAreFocusable` **getter**, which a scope
    /// overrides:
    ///
    /// ```dart
    /// // FocusScopeNode
    /// bool get descendantsAreFocusable => _canRequestFocus && super.descendantsAreFocusable;
    /// ```
    ///
    /// So the two kinds of node treat their own `canRequestFocus: false`
    /// completely differently, and the difference is the point of a scope:
    ///
    /// * an ordinary node that cannot take focus is **one** node the keyboard
    ///   skips. Its children are untouched, which is what makes a `Focus`
    ///   wrapper around a widget a thing you can turn off without disabling
    ///   the widget.
    /// * a **scope** that cannot take focus shuts out everything inside it.
    ///   That is how `FocusScope(canRequestFocus: false)` disables a whole
    ///   page under a modal without touching a single field on it.
    ///
    /// This port read the stored field for every ancestor alike, so an
    /// unfocusable scope was a door that said closed and stood open: every
    /// field on the page underneath a modal was still focusable, by tap and
    /// by keyboard.
    ///
    /// Upstream also forbids the other spelling. `FocusScopeNode`'s
    /// constructor passes `super(descendantsAreFocusable: true)` and offers no
    /// parameter for it, so a scope's stored field is always true and the
    /// override is the *only* way a scope shuts its subtree out. That is why
    /// this reads `can_request_focus` and not the pair.
    pub fn descendants_are_focusable(&self, id: u64) -> bool {
        let Some(node) = self.nodes.get(&id) else {
            return false;
        };
        if node.is_scope {
            return node.can_request_focus && node.descendants_are_focusable;
        }
        node.descendants_are_focusable
    }

    /// Upstream's `hasPrimaryFocus`: at the **end** of the chain.
    pub fn has_primary_focus(&self, id: u64) -> bool {
        self.primary_focus == Some(id)
    }

    /// Upstream's `hasFocus`: **anywhere** in the chain.
    ///
    /// The distinction is the file's central one. A text field has primary
    /// focus; the form around it and the page around that both have focus, and
    /// both want to know.
    pub fn has_focus(&self, id: u64) -> bool {
        if self.has_primary_focus(id) {
            return true;
        }
        self.primary_focus
            .is_some_and(|primary| self.ancestors(primary).contains(&id))
    }

    /// Upstream's `traversalChildren` for a scope: **empty** when the scope
    /// cannot request focus.
    ///
    /// Not "the children minus the unfocusable ones" -- nothing at all. An
    /// unfocusable scope is a closed door, and tabbing should walk past it
    /// rather than into it.
    pub fn traversal_children(&self, id: u64) -> Vec<u64> {
        let Some(node) = self.nodes.get(&id) else {
            return Vec::new();
        };
        // The stored field and not [`FocusTree::descendants_are_focusable`],
        // deliberately: for a scope the two differ only when the scope cannot
        // request focus, and the clause before this one has already returned
        // by then. A mutation swapping this for the getter stayed green on
        // every tree, so the getter here would be a line that reads like a
        // rule and decides nothing.
        if !self.can_request_focus(id) || !node.descendants_are_focusable {
            return Vec::new();
        }
        node.children
            .iter()
            .copied()
            .filter(|child| {
                self.nodes
                    .get(child)
                    .is_some_and(|child| !child.skip_traversal)
                    && self.can_request_focus(*child)
                    && node.descendants_are_traversable
            })
            .collect()
    }

    /// Upstream's `_setAsFocusedChildForScope`: mark this node as the focused
    /// child of its scope, that scope as the focused child of the one above,
    /// and so on to the root.
    ///
    /// It does **not** change the primary focus. It changes what *would* be
    /// focused if the enclosing scope received focus -- and it keeps the
    /// history, so that removing the focused child returns focus to the one
    /// before it.
    pub fn set_as_focused_child_for_scope(&mut self, id: u64) {
        let mut current = id;
        let scopes: Vec<u64> = self
            .ancestors(id)
            .into_iter()
            .filter(|ancestor| self.nodes.get(ancestor).is_some_and(|node| node.is_scope))
            .collect();
        for scope in scopes {
            debug_assert!(current != scope, "a scope cannot be its own focused child");
            if let Some(scope_node) = self.nodes.get_mut(&scope) {
                scope_node.focused_children.retain(|held| *held != current);
                scope_node.focused_children.push(current);
            }
            current = scope;
        }
    }

    /// Upstream's `_doRequestFocus` for an ordinary node.
    pub fn request_focus(&mut self, id: u64) {
        if !self.can_request_focus(id) {
            return;
        }
        // Not in the tree yet: defer until the next reparent, so a widget can
        // ask for focus before it has been mounted.
        let parented = self
            .nodes
            .get(&id)
            .is_some_and(|node| node.parent.is_some());
        let is_root = self.root == Some(id);
        if !parented && !is_root {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.request_focus_when_reparented = true;
            }
            return;
        }
        self.set_as_focused_child_for_scope(id);
        if self.has_primary_focus(id)
            && (self.marked_for_focus.is_none() || self.marked_for_focus == Some(id))
        {
            return;
        }
        if let Some(node) = self.nodes.get_mut(&id) {
            node.has_keyboard_token = true;
        }
        self.marked_for_focus = Some(id);
    }

    /// Upstream's `FocusScopeNode._doRequestFocus`.
    ///
    /// It starts by **pruning the stack** of children that can no longer take
    /// focus, which is the only place that cleanup happens: a child that
    /// became unfocusable or left the tree is still on the list until somebody
    /// asks the scope to focus.
    ///
    /// `find_first_focus` false is "focus the scope itself"; true is "focus
    /// whatever the scope would focus", which recurses down.
    pub fn request_scope_focus(&mut self, id: u64, find_first_focus: bool) {
        loop {
            let last = self
                .nodes
                .get(&id)
                .and_then(|node| node.focused_children.last().copied());
            let Some(last) = last else { break };
            let still_good = self.can_request_focus(last)
                && self
                    .nodes
                    .get(&last)
                    .is_some_and(|node| node.parent.is_some());
            if still_good {
                break;
            }
            if let Some(node) = self.nodes.get_mut(&id) {
                node.focused_children.pop();
            }
        }

        let focused_child = self.nodes.get(&id).and_then(|node| node.focused_child());
        if !find_first_focus || focused_child.is_none() {
            if self.can_request_focus(id) {
                self.set_as_focused_child_for_scope(id);
                self.marked_for_focus = Some(id);
            }
            return;
        }
        let child = focused_child.expect("checked above");
        if self.nodes.get(&child).is_some_and(|node| node.is_scope) {
            self.request_scope_focus(child, true);
        } else {
            self.request_focus(child);
        }
    }

    /// Upstream's `unfocus`.
    pub fn unfocus(&mut self, id: u64, disposition: UnfocusDisposition) {
        // Upstream's guard: nothing to do unless this node has focus or is
        // already on its way to having it. Unfocusing a node that never had
        // focus must not move focus somewhere else.
        if !self.has_focus(id) && self.marked_for_focus != Some(id) {
            return;
        }
        let Some(mut scope) = self.enclosing_scope(id) else {
            // The root, or a node not yet in the tree -- neither of which does
            // anything when unfocused.
            return;
        };

        match disposition {
            UnfocusDisposition::Scope => {
                // Upstream's comment: clearing here prevents re-focusing the
                // node just unfocused if "next" is hit immediately, and
                // prevents choosing the next-to-last focused child when
                // unfocus is called more than once.
                if self.can_request_focus(scope) {
                    if let Some(node) = self.nodes.get_mut(&scope) {
                        node.focused_children.clear();
                    }
                }
                while !self.can_request_focus(scope) {
                    scope = match self.enclosing_scope(scope) {
                        Some(next) => next,
                        None => match self.root {
                            Some(root) => root,
                            None => return,
                        },
                    };
                }
                self.request_scope_focus(scope, false);
            }
            UnfocusDisposition::PreviouslyFocusedChild => {
                if self.can_request_focus(scope) {
                    if let Some(node) = self.nodes.get_mut(&scope) {
                        node.focused_children.retain(|held| *held != id);
                    }
                }
                while !self.can_request_focus(scope) {
                    // Each unfocusable scope is also removed from *its*
                    // parent's history on the way up, or focus would come back
                    // to a scope that cannot take it.
                    if let Some(parent_scope) = self.enclosing_scope(scope) {
                        if let Some(node) = self.nodes.get_mut(&parent_scope) {
                            node.focused_children.retain(|held| *held != scope);
                        }
                    }
                    scope = match self.enclosing_scope(scope) {
                        Some(next) => next,
                        None => match self.root {
                            Some(root) => root,
                            None => return,
                        },
                    };
                }
                self.request_scope_focus(scope, true);
            }
        }
    }

    /// Upstream's `canRequestFocus` setter, which **unfocuses first and sets
    /// the flag before doing so**.
    ///
    /// The order is upstream's own comment: the flag has to be false before
    /// `unfocus` runs, because unfocus consults it while culling unfocusable
    /// previously-focused children. Setting it afterwards would leave this
    /// node looking focusable during exactly the walk meant to skip it.
    ///
    /// # The `changed` guard decides nothing here yet
    ///
    /// Upstream's setter wraps its whole body in `if (value != _canRequestFocus)`,
    /// and what that guard is really protecting is the last line --
    /// `_manager?._markPropertiesChanged(this)`, a notification. The
    /// unfocusing below is already conditional on `had_focus && !value`, so
    /// setting a value to what it already is does nothing either way.
    ///
    /// This port has no `_markPropertiesChanged` yet, so the guard has nothing
    /// left to protect and a mutation deleting it stays green. It is kept as
    /// upstream's shape and as the place the notification will go, and that is
    /// written down rather than left to be rediscovered.
    pub fn set_can_request_focus(&mut self, id: u64, value: bool) {
        let changed = self
            .nodes
            .get(&id)
            .is_some_and(|node| node.can_request_focus != value);
        if !changed {
            return;
        }
        let had_focus = self.has_focus(id);
        if let Some(node) = self.nodes.get_mut(&id) {
            node.can_request_focus = value;
        }
        if had_focus && !value {
            self.unfocus(id, UnfocusDisposition::PreviouslyFocusedChild);
        }
    }

    /// Upstream's `descendantsAreFocusable` setter, with the same ordering for
    /// the same reason.
    ///
    /// Its doc adds one thing worth keeping: when set back to true the
    /// descendants are **not** refocused, though they can accept focus again.
    /// A panel that disables and re-enables itself does not steal focus back.
    ///
    /// Its `changed` guard is the same story as the one above:
    /// [`FocusTree::set_can_request_focus`] says why it decides nothing yet.
    pub fn set_descendants_are_focusable(&mut self, id: u64, value: bool) {
        let changed = self
            .nodes
            .get(&id)
            .is_some_and(|node| node.descendants_are_focusable != value);
        if !changed {
            return;
        }
        let had_focus = self.has_focus(id);
        if let Some(node) = self.nodes.get_mut(&id) {
            node.descendants_are_focusable = value;
        }
        if !value && had_focus {
            self.unfocus(id, UnfocusDisposition::PreviouslyFocusedChild);
        }
    }

    /// Upstream's `FocusScopeNode.setFirstFocus`.
    ///
    /// The branch is the interesting part: if this scope **has** focus the
    /// child is focused now; if it does not, the child is merely recorded as
    /// what would be focused when the scope gets focus. Setting the first
    /// focus of a scope nobody is in should not pull focus into it.
    pub fn set_first_focus(&mut self, scope: u64, child: u64) {
        if self.has_focus(scope) {
            self.request_scope_focus(child, true);
        } else {
            self.set_as_focused_child_for_scope(child);
        }
    }

    /// Upstream's `autofocus`: focus this node **only if the scope has no
    /// focused child yet**.
    ///
    /// The whole point is not to fight. Two widgets both autofocusing should
    /// give the first one focus, and neither should take it from a reader who
    /// has already chosen.
    pub fn autofocus(&mut self, scope: u64, node: u64) {
        if self
            .nodes
            .get(&scope)
            .and_then(|scope| scope.focused_child())
            .is_some()
        {
            return;
        }
        self.request_focus(node);
    }

    /// Applies the pending focus change. Upstream does this once per frame.
    pub fn apply_focus_change(&mut self) {
        if let Some(next) = self.marked_for_focus.take() {
            self.primary_focus = Some(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_scope_can_be_asked_for_a_scope_handle() {
        // Which is what the handle buys: focused_child and set_first_focus are
        // meaningless on an ordinary node.
        let tree = tree();
        assert!(FocusScopeNode::of(&tree, 2).is_some());
        assert!(FocusScopeNode::of(&tree, 4).is_none(), "an ordinary node");
        assert!(FocusScopeNode::of(&tree, 99).is_none());
    }

    #[test]
    fn a_scope_knows_whether_it_is_the_first_focus_of_the_one_above() {
        let mut tree = tree();
        let two = FocusScopeNode::of(&tree, 2).unwrap();
        assert!(!two.is_first_focus(&tree));

        focus(&mut tree, 4);
        assert!(two.is_first_focus(&tree), "scope 1 would focus scope 2");

        focus(&mut tree, 6);
        assert!(!two.is_first_focus(&tree), "now it would focus scope 3");
    }

    #[test]
    fn the_scope_handle_reaches_the_same_operations() {
        let mut tree = tree();
        let two = FocusScopeNode::of(&tree, 2).unwrap();

        two.autofocus(&mut tree, 4);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4));
        assert_eq!(two.focused_child(&tree), Some(4));
        assert_eq!(two.traversal_children(&tree), vec![4, 5]);

        two.request_focus(&mut tree, false);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(2));
    }

    /// root(1) ─┬─ scope(2) ─┬─ field(4)
    ///          │            └─ field(5)
    ///          └─ scope(3) ─── field(6)
    fn tree() -> FocusTree {
        let mut tree = FocusTree::new(FocusNode::scope(1));
        tree.attach(FocusNode::scope(2), 1);
        tree.attach(FocusNode::scope(3), 1);
        tree.attach(FocusNode::new(4), 2);
        tree.attach(FocusNode::new(5), 2);
        tree.attach(FocusNode::new(6), 3);
        tree
    }

    fn focus(tree: &mut FocusTree, id: u64) {
        tree.request_focus(id);
        tree.apply_focus_change();
    }

    // -- Has focus versus has primary focus --------------------------------

    #[test]
    fn everything_in_the_chain_has_focus_but_only_the_end_has_primary() {
        // A text field has primary focus; the form around it and the page
        // around that both have focus, and both want to know.
        let mut tree = tree();
        focus(&mut tree, 4);

        assert!(tree.has_primary_focus(4));
        assert!(tree.has_focus(4));
        assert!(tree.has_focus(2), "the scope is in the chain");
        assert!(tree.has_focus(1), "and so is the root");

        assert!(!tree.has_primary_focus(2));
        assert!(!tree.has_focus(5), "a sibling is not");
        assert!(!tree.has_focus(3));
    }

    #[test]
    fn focus_moves_between_frames_rather_than_during_one() {
        let mut tree = tree();
        tree.request_focus(4);
        assert_eq!(tree.marked_for_focus(), Some(4));
        assert!(!tree.has_primary_focus(4), "not yet");

        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4));
        assert_eq!(tree.marked_for_focus(), None);
    }

    // -- Focusability -------------------------------------------------------

    #[test]
    fn one_unfocusable_ancestor_turns_off_a_whole_subtree() {
        // Which is how a disabled panel disables everything inside it without
        // touching any of them.
        let mut tree = tree();
        assert!(tree.can_request_focus(4));

        tree.set_descendants_are_focusable(2, false);
        assert!(!tree.can_request_focus(4));
        assert!(!tree.can_request_focus(5));
        assert!(
            tree.can_request_focus(2),
            "but the panel itself is still focusable"
        );
        assert!(tree.can_request_focus(6), "and the other subtree is fine");
    }

    // -- The one thing a scope does that an ordinary node does not -----------

    #[test]
    fn a_scope_that_cannot_take_focus_shuts_out_everything_inside_it() {
        // `FocusScopeNode.descendantsAreFocusable` is an override:
        // `_canRequestFocus && super.descendantsAreFocusable`. That is how
        // `FocusScope(canRequestFocus: false)` disables a whole page under a
        // modal without touching a single field on it. This port read the
        // stored field for every ancestor alike, so the door said closed and
        // stood open.
        let mut tree = tree();
        assert!(tree.can_request_focus(4));
        assert!(tree.can_request_focus(5));

        tree.set_can_request_focus(2, false);
        assert!(!tree.can_request_focus(4), "the page underneath is out");
        assert!(!tree.can_request_focus(5));
        assert!(tree.can_request_focus(6), "and the other scope is untouched");
    }

    #[test]
    fn but_an_ordinary_node_that_cannot_take_focus_is_only_itself() {
        // The half that says the rule above belongs to scopes rather than to
        // unfocusable ancestors in general. Without this the fix could be
        // "any unfocusable ancestor shuts out its subtree", which is a
        // different rule that happens to agree on the case above -- and it
        // would break every `Focus` wrapper somebody turned off around a
        // widget they still wanted usable.
        let mut tree = tree();
        tree.attach(FocusNode::new(7), 4);
        assert!(tree.can_request_focus(7));

        tree.set_can_request_focus(4, false);
        assert!(!tree.can_request_focus(4), "itself, yes");
        assert!(tree.can_request_focus(7), "its child, no");
    }

    #[test]
    fn and_a_shut_scope_hands_back_no_traversal_children_either() {
        // Not "the children minus the unfocusable ones" -- nothing at all.
        let mut tree = tree();
        assert_eq!(tree.traversal_children(2), vec![4, 5]);
        tree.set_can_request_focus(2, false);
        assert!(tree.traversal_children(2).is_empty());
        assert_eq!(
            tree.traversal_children(3),
            vec![6],
            "and the scope beside it still walks"
        );
    }

    #[test]
    fn a_scopes_stored_flag_is_not_the_thing_that_shuts_it() {
        // Upstream's `FocusScopeNode` constructor passes
        // `super(descendantsAreFocusable: true)` and offers no parameter for
        // it, so a scope's stored field is always true and the override is the
        // only way it shuts. Reading the pair rather than `canRequestFocus`
        // alone would agree with upstream on every tree a scope can actually
        // be in, and disagree here -- which is why this asks about a tree
        // upstream cannot build.
        let mut shut = FocusTree::new(FocusNode::scope(1));
        shut.attach(
            FocusNode::scope(2).with_descendants_are_focusable(false),
            1,
        );
        shut.attach(FocusNode::new(3), 2);
        assert!(
            !shut.descendants_are_focusable(2),
            "the stored flag still counts when it is somehow false"
        );
        assert!(!shut.can_request_focus(3));

        // And a scope that can take focus, with the flag at its only upstream
        // value, lets its children through.
        let ordinary = tree();
        assert!(ordinary.descendants_are_focusable(2));
        assert!(ordinary.can_request_focus(4));
    }

    #[test]
    fn a_node_that_cannot_take_focus_does_not_get_it() {
        let mut tree = tree();
        tree.set_can_request_focus(4, false);
        tree.request_focus(4);
        assert_eq!(tree.marked_for_focus(), None);
    }

    #[test]
    fn turning_focusability_off_under_a_focused_node_moves_focus_away() {
        let mut tree = tree();
        focus(&mut tree, 4);
        tree.set_descendants_are_focusable(2, false);
        tree.apply_focus_change();
        assert!(!tree.has_primary_focus(4));
    }

    #[test]
    fn turning_it_back_on_does_not_take_focus_back() {
        // A panel that disables and re-enables itself does not steal focus.
        let mut tree = tree();
        focus(&mut tree, 4);
        tree.set_descendants_are_focusable(2, false);
        tree.apply_focus_change();
        let after = tree.primary_focus();

        tree.set_descendants_are_focusable(2, true);
        tree.apply_focus_change();
        assert_eq!(tree.primary_focus(), after, "left where it was");
        assert!(tree.can_request_focus(4), "though it could take it again");
    }

    #[test]
    fn a_skipped_node_is_still_focusable_just_not_by_tabbing() {
        // Reachable by tap, not by tab.
        let mut tree = tree();
        tree.node_mut(4).unwrap().skip_traversal = true;
        assert!(tree.can_request_focus(4));
        assert_eq!(tree.traversal_children(2), vec![5]);
    }

    #[test]
    fn an_unfocusable_scope_offers_no_traversal_children_at_all() {
        // Not "its children minus the unfocusable ones" -- nothing. A closed
        // door is walked past rather than into.
        let mut tree = tree();
        tree.set_can_request_focus(2, false);
        assert!(tree.traversal_children(2).is_empty());
    }

    // -- Scopes and their history ------------------------------------------

    #[test]
    fn a_scope_remembers_a_stack_rather_than_only_the_last_child() {
        // So that removing the focused child returns focus to the one before
        // it rather than to nowhere.
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 5);
        assert_eq!(tree.node(2).unwrap().focused_children(), &[4, 5]);
        assert_eq!(tree.node(2).unwrap().focused_child(), Some(5));
    }

    #[test]
    fn focusing_a_child_again_moves_it_to_the_top_rather_than_duplicating_it() {
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 5);
        focus(&mut tree, 4);
        assert_eq!(tree.node(2).unwrap().focused_children(), &[5, 4]);
    }

    #[test]
    fn the_chain_of_focused_children_runs_all_the_way_to_the_root() {
        // Focusing a leaf makes its scope the focused child of the scope above
        // it, and so on.
        let mut tree = tree();
        focus(&mut tree, 4);
        assert_eq!(tree.node(2).unwrap().focused_child(), Some(4));
        assert_eq!(tree.node(1).unwrap().focused_child(), Some(2));
    }

    #[test]
    fn asking_a_scope_to_focus_walks_down_to_its_leaf() {
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 6);

        tree.request_scope_focus(2, true);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4), "back to where scope 2 was");
    }

    #[test]
    fn asking_a_scope_to_focus_itself_stops_there() {
        // find_first_focus false is "focus the scope", not "focus what the
        // scope would focus".
        let mut tree = tree();
        focus(&mut tree, 4);

        tree.request_scope_focus(2, false);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(2));
    }

    #[test]
    fn a_stale_focused_child_is_pruned_when_the_scope_is_next_asked() {
        // The only place that cleanup happens: a child that became unfocusable
        // stays on the list until somebody asks the scope to focus.
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 5);
        tree.node_mut(5).unwrap().can_request_focus = false;

        tree.request_scope_focus(2, true);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4), "fell back to the one before");
        assert_eq!(tree.node(2).unwrap().focused_children(), &[4]);
    }

    // -- Unfocusing ---------------------------------------------------------

    #[test]
    fn unfocusing_to_the_scope_clears_its_history() {
        // So a following "next" picks what the traversal policy thinks should
        // be first, rather than resuming where the reader was.
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 5);

        tree.unfocus(5, UnfocusDisposition::Scope);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(2), "the scope itself");
        assert_eq!(
            tree.node(2).unwrap().focused_children(),
            &[] as &[u64],
            "and the history is gone"
        );
    }

    #[test]
    fn unfocusing_to_the_previous_child_resumes_where_the_reader_was() {
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 5);

        tree.unfocus(5, UnfocusDisposition::PreviouslyFocusedChild);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4));
    }

    #[test]
    fn unfocusing_twice_does_not_walk_back_through_the_history() {
        // Upstream's comment on clearing: it prevents choosing the
        // next-to-last focused child when unfocus is called more than once.
        let mut tree = tree();
        focus(&mut tree, 4);
        focus(&mut tree, 5);

        tree.unfocus(5, UnfocusDisposition::Scope);
        tree.apply_focus_change();
        tree.unfocus(2, UnfocusDisposition::Scope);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(1), "up to the root, not back to 4");
    }

    #[test]
    fn unfocusing_something_that_never_had_focus_moves_nothing() {
        let mut tree = tree();
        focus(&mut tree, 4);
        tree.unfocus(6, UnfocusDisposition::Scope);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4), "untouched");
    }

    #[test]
    fn unfocusing_a_node_that_is_only_marked_for_focus_still_counts() {
        // The guard is `!hasFocus && marked != this`, so a node on its way to
        // focus can still give it up.
        let mut tree = tree();
        tree.request_focus(4);
        assert!(!tree.has_focus(4));

        tree.unfocus(4, UnfocusDisposition::Scope);
        assert_eq!(tree.marked_for_focus(), Some(2), "redirected to the scope");
    }

    #[test]
    fn unfocusing_the_root_does_nothing_because_there_is_nowhere_above_it() {
        let mut tree = tree();
        focus(&mut tree, 1);
        tree.unfocus(1, UnfocusDisposition::Scope);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(1));
    }

    #[test]
    fn an_unfocusable_scope_is_walked_past_on_the_way_up() {
        let mut tree = tree();
        focus(&mut tree, 4);
        tree.node_mut(2).unwrap().can_request_focus = false;

        tree.unfocus(4, UnfocusDisposition::Scope);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(1), "up to the root");
    }

    // -- Requesting focus ---------------------------------------------------

    #[test]
    fn a_node_not_yet_in_the_tree_gets_focus_when_it_is_attached() {
        // Which is what lets a widget call requestFocus in initState.
        let mut tree = tree();
        let mut pending = FocusNode::new(7);
        pending.request_focus_when_reparented = true;
        assert!(pending.will_request_focus_when_reparented());

        tree.attach(pending, 2);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(7));
        assert!(
            !tree.node(7).unwrap().will_request_focus_when_reparented(),
            "and the deferral was spent, not kept"
        );
    }

    #[test]
    fn requesting_focus_for_something_that_already_has_it_is_a_no_op() {
        let mut tree = tree();
        focus(&mut tree, 4);
        tree.request_focus(4);
        assert_eq!(tree.marked_for_focus(), None);
    }

    #[test]
    fn autofocus_does_not_fight_a_scope_that_already_chose() {
        // Two widgets both autofocusing should give the first one focus, and
        // neither should take it from a reader who has already chosen.
        let mut tree = tree();
        tree.autofocus(2, 4);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4));

        tree.autofocus(2, 5);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(4), "the second one stood down");
    }

    #[test]
    fn setting_a_first_focus_on_an_unfocused_scope_only_records_it() {
        // It should not pull focus into a scope nobody is in.
        let mut tree = tree();
        focus(&mut tree, 6);

        tree.set_first_focus(2, 5);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(6), "focus did not move");
        assert_eq!(
            tree.node(2).unwrap().focused_child(),
            Some(5),
            "but the scope now knows what it would focus"
        );
    }

    #[test]
    fn setting_a_first_focus_on_a_focused_scope_focuses_it_now() {
        let mut tree = tree();
        focus(&mut tree, 4);

        tree.set_first_focus(2, 5);
        tree.apply_focus_change();
        assert!(tree.has_primary_focus(5));
    }

    // -- The keyboard token -------------------------------------------------

    #[test]
    fn the_keyboard_token_separates_being_focused_from_being_chosen() {
        // A form that autofocuses its first field does not throw the keyboard
        // up over half the screen before the reader has asked to type.
        let mut tree = tree();
        assert!(!tree.node(4).unwrap().has_keyboard_token());

        tree.request_focus(4);
        assert!(tree.node(4).unwrap().has_keyboard_token());

        assert!(tree.node_mut(4).unwrap().consume_keyboard_token());
        assert!(
            !tree.node_mut(4).unwrap().consume_keyboard_token(),
            "and it is spent"
        );
    }

    // -- Scope lookups ------------------------------------------------------

    #[test]
    fn the_nearest_scope_includes_this_node_and_the_enclosing_one_does_not() {
        // Mixing them up is how a scope ends up looking for itself.
        let tree = tree();
        assert_eq!(tree.nearest_scope(2), Some(2));
        assert_eq!(tree.enclosing_scope(2), Some(1));

        assert_eq!(tree.nearest_scope(4), Some(2));
        assert_eq!(tree.enclosing_scope(4), Some(2));
    }

    #[test]
    fn ancestors_come_back_nearest_first() {
        let tree = tree();
        assert_eq!(tree.ancestors(4), vec![2, 1]);
        assert_eq!(tree.ancestors(1), Vec::<u64>::new());
    }
}
