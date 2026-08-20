//! A tree in a sliver -- a port of upstream's `widgets/sliver_tree.dart`.
//!
//! A sliver lays out a **flat list** of rows, and a tree is not one. So the
//! whole of this file is one idea: keep the tree as the caller wrote it, and
//! maintain beside it a flattened list of the rows currently on screen. That
//! list is upstream's `_activeNodes`, and "active" means *reachable through
//! expanded parents* -- which is not the same as visible, and upstream is
//! careful to say so on every method that could be confused about it.
//!
//! The decision worth arriving at is what happens **while a node is
//! collapsing**. Its children have to stay in the flat list until the
//! animation ends -- there would otherwise be nothing left to animate away.
//! So the unpack rule is not "is this node expanded" but "is it expanded *or
//! currently animating in either direction*".
//!
//! ## What is not here
//!
//! The sliver's layout, the per-row extent builder and the animation
//! controllers belong to this crate's own render tree. What is ported is the
//! node, the flattening, the toggle rules, and the controller's contract.

use std::collections::HashMap;

/// Upstream `TreeSliverIndentationType`.
///
/// A closed set with a `custom` escape hatch, which is upstream's way of
/// making the two answers people actually want -- the standard indent, and
/// none at all -- the ones that read as names rather than as numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeSliverIndentationType {
    value: f32,
}

impl Default for TreeSliverIndentationType {
    fn default() -> TreeSliverIndentationType {
        TreeSliverIndentationType::STANDARD
    }
}

impl TreeSliverIndentationType {
    /// Ten pixels per level.
    pub const STANDARD: TreeSliverIndentationType = TreeSliverIndentationType { value: 10.0 };

    /// No offset at all. Upstream's note says why anyone would want it: the
    /// indentation may be built into the row builder instead, where it can be
    /// a guide line or a disclosure triangle rather than empty space.
    pub const NONE: TreeSliverIndentationType = TreeSliverIndentationType { value: 0.0 };

    /// Upstream asserts the value is not negative -- a negative indent would
    /// walk deeper rows back out past the shallow ones.
    pub fn custom(value: f32) -> Option<TreeSliverIndentationType> {
        if value < 0.0 {
            return None;
        }
        Some(TreeSliverIndentationType { value })
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// How far in a row at `depth` sits.
    pub fn offset_for_depth(&self, depth: usize) -> f32 {
        self.value * depth as f32
    }
}

/// Upstream `TreeSliverNode`.
///
/// Nodes are identified here by a `u64` and held in an arena, because
/// upstream's `_parent` and `_depth` are back-pointers the tree writes into
/// its own nodes -- a shape Rust does not spell with owned children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSliverNode {
    pub id: u64,
    /// Upstream's `content`, the thing the row is about.
    pub content: String,
    pub children: Vec<u64>,
    expanded: bool,
    /// Upstream's `depth`, null until the tree has been unpacked once.
    depth: Option<usize>,
    parent: Option<u64>,
}

impl TreeSliverNode {
    /// Upstream's constructor, whose initialiser carries an invariant worth
    /// keeping: `_expanded = (children?.isNotEmpty ?? false) && expanded`.
    ///
    /// **A node with no children cannot be expanded**, however it was asked.
    /// The alternative would be a leaf that reports itself expanded and has
    /// nothing to show for it, and every caller that trusted the flag would
    /// have to check the children as well.
    pub fn new(
        id: u64,
        content: impl Into<String>,
        children: Vec<u64>,
        expanded: bool,
    ) -> TreeSliverNode {
        let has_children = !children.is_empty();
        TreeSliverNode {
            id,
            content: content.into(),
            children,
            expanded: has_children && expanded,
            depth: None,
            parent: None,
        }
    }

    pub fn leaf(id: u64, content: impl Into<String>) -> TreeSliverNode {
        TreeSliverNode::new(id, content, Vec::new(), false)
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn depth(&self) -> Option<usize> {
        self.depth
    }

    pub fn parent(&self) -> Option<u64> {
        self.parent
    }

    /// Upstream's `toString`, which distinguishes the three kinds of node a
    /// reader of a debug dump cares about.
    pub fn describe(&self) -> String {
        let depth = match self.depth {
            Some(0) => "root".to_string(),
            Some(depth) => depth.to_string(),
            None => "null".to_string(),
        };
        let kind = if self.children.is_empty() {
            "leaf".to_string()
        } else {
            format!("parent, expanded: {}", self.expanded)
        };
        format!("TreeSliverNode: {}, depth: {depth}, {kind}", self.content)
    }
}

/// Upstream `TreeSliverStateMixin`: what a [`TreeSliverController`] can ask
/// of whatever implements the tree.
///
/// It is a mixin upstream rather than a private interface **so that other
/// widgets can implement it** and be driven by the same controller. A tree
/// that is not a `TreeSliver` still gets the controller's vocabulary.
pub trait TreeSliverStateMixin {
    fn is_expanded(&self, node: u64) -> bool;

    /// Upstream's `isActive`, with an unusually careful doc: a node is active
    /// when its parent chain is expanded, and **this does not reflect whether
    /// it is visible in the viewport**. A row scrolled a mile off screen is
    /// active; a row inside a collapsed parent is not.
    fn is_active(&self, node: u64) -> bool;

    fn toggle_node(&mut self, node: u64);
    fn collapse_all(&mut self);
    fn expand_all(&mut self);

    /// Upstream's `getNodeFor`, which searches the **whole** tree rather than
    /// the active list -- so a caller can find and expand something currently
    /// hidden.
    fn get_node_for(&self, content: &str) -> Option<u64>;

    /// Upstream's `getActiveIndexFor`, `None` for an inactive node. The row
    /// index of something that has no row is not a number.
    fn get_active_index_for(&self, node: u64) -> Option<usize>;
}

/// Which way a node is animating, if it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleDirection {
    Expanding,
    Collapsing,
}

/// Upstream `TreeSliver`, reduced to the tree it keeps and the flat list it
/// derives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeSliver {
    nodes: HashMap<u64, TreeSliverNode>,
    /// The caller's tree, at the root.
    roots: Vec<u64>,
    /// Upstream's `_activeNodes`, in row order.
    active: Vec<u64>,
    /// Upstream's `_currentAnimationForParent`.
    animating: HashMap<u64, ToggleDirection>,
    pub indentation: TreeSliverIndentationType,
    /// Whether the toggle animation is switched off, which upstream checks
    /// alongside a zero duration.
    pub animation_disabled: bool,
    toggles: Vec<u64>,
}

impl TreeSliver {
    /// Upstream's `defaultAnimationDuration`.
    pub const DEFAULT_ANIMATION_MICROS: i64 = 150_000;

    pub fn new(nodes: Vec<TreeSliverNode>, roots: Vec<u64>) -> TreeSliver {
        let mut tree = TreeSliver {
            nodes: nodes.into_iter().map(|node| (node.id, node)).collect(),
            roots,
            active: Vec::new(),
            animating: HashMap::new(),
            indentation: TreeSliverIndentationType::STANDARD,
            animation_disabled: true,
            toggles: Vec::new(),
        };
        tree.unpack_active_nodes();
        tree
    }

    pub fn node(&self, id: u64) -> Option<&TreeSliverNode> {
        self.nodes.get(&id)
    }

    pub fn active_nodes(&self) -> &[u64] {
        &self.active
    }

    /// Which nodes were toggled, in order, for a caller standing in for
    /// upstream's `onNodeToggle`.
    pub fn toggles(&self) -> &[u64] {
        &self.toggles
    }

    /// Upstream's `_shouldUnpackNode`, and the middle branch is the one that
    /// matters.
    ///
    /// A node with no children has nothing to unpack. A node **currently
    /// animating -- either way** keeps its children active: a collapsing
    /// node's children have to stay in the list until the animation ends, or
    /// there would be nothing left to animate away. Only when nothing is
    /// animating does the answer come from `isExpanded`.
    pub fn should_unpack_node(&self, id: u64) -> bool {
        let Some(node) = self.nodes.get(&id) else {
            return false;
        };
        if node.children.is_empty() {
            return false;
        }
        if self.animating.contains_key(&id) {
            return true;
        }
        node.expanded
    }

    /// Upstream's `_unpackActiveNodes`: a depth-first walk that writes each
    /// node's depth and parent as it goes, and appends it to the flat list.
    ///
    /// The depth and parent are **derived here rather than stored by the
    /// caller**, which is why a node moved to a different place in the tree
    /// needs no fixing up: the next unpack tells it where it now is.
    pub fn unpack_active_nodes(&mut self) {
        self.active.clear();
        let roots = self.roots.clone();
        self.unpack(&roots, 0, None);
    }

    fn unpack(&mut self, ids: &[u64], depth: usize, parent: Option<u64>) {
        for id in ids {
            if let Some(node) = self.nodes.get_mut(id) {
                node.depth = Some(depth);
                node.parent = parent;
            }
            self.active.push(*id);
            if self.should_unpack_node(*id) {
                let children = self
                    .nodes
                    .get(id)
                    .map(|node| node.children.clone())
                    .unwrap_or_default();
                self.unpack(&children, depth + 1, Some(*id));
            }
        }
    }

    /// How far in a row sits, from its depth and the indentation.
    pub fn indent_for(&self, id: u64) -> f32 {
        let depth = self.nodes.get(&id).and_then(|node| node.depth).unwrap_or(0);
        self.indentation.offset_for_depth(depth)
    }

    /// Upstream's `_updateActiveAnimations`, which recomputes the animating
    /// **index range** on every build.
    ///
    /// It has to: the row indexes of an animating node's children move
    /// whenever anything above them expands or collapses, so the range cannot
    /// be recorded once when the animation starts. What is stable is the node;
    /// its children's indexes are not.
    pub fn animation_range(&self, id: u64) -> Option<(usize, usize)> {
        let at = self.active.iter().position(|held| *held == id)?;
        let node = self.nodes.get(&id)?;
        if node.children.is_empty() {
            return None;
        }
        let leading = at + 1;
        Some((leading, leading + node.children.len() - 1))
    }

    /// The start of a toggle: upstream flips `_expanded`, tells
    /// `onNodeToggle`, and then either animates or unpacks at once.
    ///
    /// The immediate path is not an optimisation -- upstream's comment says a
    /// zero duration would otherwise **freeze the application**, because the
    /// tree would be updated while the node's children were no longer active.
    /// Zero is not a very short animation; it is no animation, and has to be
    /// treated as one.
    pub fn toggle_node(&mut self, id: u64) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        if node.children.is_empty() {
            // Upstream: "No state to change."
            return;
        }
        let now_expanded = !node.expanded;
        if let Some(node) = self.nodes.get_mut(&id) {
            node.expanded = now_expanded;
        }
        self.toggles.push(id);

        if self.animation_disabled {
            self.animating.remove(&id);
            self.unpack_active_nodes();
            return;
        }
        self.animating.insert(
            id,
            if now_expanded {
                ToggleDirection::Expanding
            } else {
                ToggleDirection::Collapsing
            },
        );
        // An expanding node's children must appear at once so there is
        // something to animate in.
        self.unpack_active_nodes();
    }

    /// The animation finishing. Upstream unpacks again **only when the node
    /// collapsed** -- an expansion already put the children in the list on the
    /// way in, and re-unpacking would be work with no change.
    pub fn finish_toggle_animation(&mut self, id: u64) {
        let direction = self.animating.remove(&id);
        if direction == Some(ToggleDirection::Collapsing) {
            self.unpack_active_nodes();
        }
    }

    pub fn is_animating(&self, id: u64) -> bool {
        self.animating.contains_key(&id)
    }

    /// Upstream's `_expandAll`/`_collapseAll` walk, and the split in it is the
    /// point.
    ///
    /// A node that is **hidden** has its flag set directly; a node that is
    /// **active** goes on a list and is toggled afterwards. Toggling a hidden
    /// node would start an animation nobody can see, and on a large tree that
    /// is one controller per node for a change that is instantaneous to the
    /// reader.
    ///
    /// The list is built **post-order**, so the deepest node lands in it
    /// first -- and then it is walked **in reverse**, which means the toggles
    /// actually run **shallowest first**.
    ///
    /// That works because of the rule in [`TreeSliver::should_unpack_node`]: a
    /// collapsing node keeps its descendants active until its animation ends,
    /// so collapsing the shallow node does not take the deeper ones out of the
    /// active list before their own turn comes. Upstream asserts in
    /// `toggleNode` that the node is active, and this is what keeps that true.
    fn walk_all(&mut self, expand: bool) {
        let mut deferred: Vec<u64> = Vec::new();
        let roots = self.roots.clone();
        self.walk_all_from(&roots, expand, &mut deferred);
        for id in deferred.into_iter().rev() {
            self.toggle_node(id);
        }
    }

    fn walk_all_from(&mut self, ids: &[u64], expand: bool, deferred: &mut Vec<u64>) {
        for id in ids {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if node.children.is_empty() {
                continue;
            }
            let children = node.children.clone();
            self.walk_all_from(&children, expand, deferred);
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if node.expanded == expand {
                continue;
            }
            if self.active.contains(id) {
                deferred.push(*id);
            } else if let Some(node) = self.nodes.get_mut(id) {
                node.expanded = expand;
            }
        }
    }
}

impl TreeSliverStateMixin for TreeSliver {
    fn is_expanded(&self, node: u64) -> bool {
        self.nodes
            .get(&node)
            .map(|node| node.expanded)
            .unwrap_or(false)
    }

    fn is_active(&self, node: u64) -> bool {
        self.active.contains(&node)
    }

    fn toggle_node(&mut self, node: u64) {
        TreeSliver::toggle_node(self, node);
    }

    fn collapse_all(&mut self) {
        self.walk_all(false);
    }

    fn expand_all(&mut self) {
        self.walk_all(true);
    }

    fn get_node_for(&self, content: &str) -> Option<u64> {
        // Upstream walks the tree depth-first from the roots, so the answer is
        // the first match in tree order rather than in map order.
        fn search(tree: &TreeSliver, ids: &[u64], content: &str) -> Option<u64> {
            for id in ids {
                let node = tree.nodes.get(id)?;
                if node.content == content {
                    return Some(*id);
                }
                if let Some(found) = search(tree, &node.children, content) {
                    return Some(found);
                }
            }
            None
        }
        search(self, &self.roots, content)
    }

    fn get_active_index_for(&self, node: u64) -> Option<usize> {
        self.active.iter().position(|held| *held == node)
    }
}

/// Upstream `TreeSliverController`.
///
/// Every method asserts it is attached, and upstream's class doc carries the
/// warning that matters more: expanding or collapsing **rebuilds the tree**,
/// so these may not be called from a build method. A controller that quietly
/// tolerated it would produce a tree that disagreed with the frame being
/// built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeSliverController {
    attached: bool,
}

impl TreeSliverController {
    pub fn new() -> TreeSliverController {
        TreeSliverController::default()
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Upstream asserts a controller is associated with **one** tree: "A
    /// TreeSliverController can only be associated with one TreeSliver."
    /// Sharing one would leave `expandNode` ambiguous about which tree it
    /// meant.
    pub fn attach(&mut self) -> Result<(), &'static str> {
        if self.attached {
            return Err("a TreeSliverController can only be associated with one TreeSliver");
        }
        self.attached = true;
        Ok(())
    }

    pub fn detach(&mut self) {
        self.attached = false;
    }

    pub fn is_expanded(&self, tree: &TreeSliver, node: u64) -> bool {
        debug_assert!(self.attached, "controller is not attached");
        TreeSliverStateMixin::is_expanded(tree, node)
    }

    pub fn is_active(&self, tree: &TreeSliver, node: u64) -> bool {
        debug_assert!(self.attached, "controller is not attached");
        tree.is_active(node)
    }

    pub fn get_node_for(&self, tree: &TreeSliver, content: &str) -> Option<u64> {
        debug_assert!(self.attached, "controller is not attached");
        tree.get_node_for(content)
    }

    pub fn toggle_node(&self, tree: &mut TreeSliver, node: u64) {
        debug_assert!(self.attached, "controller is not attached");
        TreeSliver::toggle_node(tree, node);
    }

    /// Upstream's `expandNode`: **no effect if already expanded**, rather than
    /// a toggle. A caller that wanted a toggle has `toggleNode`, and a caller
    /// that said "expand" meant the end state, not the transition.
    pub fn expand_node(&self, tree: &mut TreeSliver, node: u64) {
        debug_assert!(self.attached, "controller is not attached");
        if TreeSliverStateMixin::is_expanded(tree, node) {
            return;
        }
        TreeSliver::toggle_node(tree, node);
    }

    pub fn collapse_node(&self, tree: &mut TreeSliver, node: u64) {
        debug_assert!(self.attached, "controller is not attached");
        if !TreeSliverStateMixin::is_expanded(tree, node) {
            return;
        }
        TreeSliver::toggle_node(tree, node);
    }

    pub fn expand_all(&self, tree: &mut TreeSliver) {
        debug_assert!(self.attached, "controller is not attached");
        tree.expand_all();
    }

    pub fn collapse_all(&self, tree: &mut TreeSliver) {
        debug_assert!(self.attached, "controller is not attached");
        tree.collapse_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree:  1 ─┬─ 2 ─┬─ 4
    ///             │     └─ 5
    ///             └─ 3
    ///          6 (root, leaf)
    fn tree() -> TreeSliver {
        TreeSliver::new(
            vec![
                TreeSliverNode::new(1, "one", vec![2, 3], false),
                TreeSliverNode::new(2, "two", vec![4, 5], false),
                TreeSliverNode::leaf(3, "three"),
                TreeSliverNode::leaf(4, "four"),
                TreeSliverNode::leaf(5, "five"),
                TreeSliverNode::leaf(6, "six"),
            ],
            vec![1, 6],
        )
    }

    // -- The node ----------------------------------------------------------

    #[test]
    fn a_leaf_cannot_be_expanded_however_it_was_asked() {
        // Or every caller that trusted the flag would have to check the
        // children as well.
        let leaf = TreeSliverNode::new(1, "one", Vec::new(), true);
        assert!(!leaf.is_expanded());

        let parent = TreeSliverNode::new(1, "one", vec![2], true);
        assert!(parent.is_expanded());
    }

    #[test]
    fn a_node_describes_itself_by_which_of_the_three_kinds_it_is() {
        let mut tree = tree();
        tree.unpack_active_nodes();
        assert_eq!(
            tree.node(1).unwrap().describe(),
            "TreeSliverNode: one, depth: root, parent, expanded: false"
        );
        assert_eq!(
            tree.node(6).unwrap().describe(),
            "TreeSliverNode: six, depth: root, leaf"
        );
    }

    // -- Flattening --------------------------------------------------------

    #[test]
    fn a_collapsed_tree_is_only_its_roots() {
        let tree = tree();
        assert_eq!(tree.active_nodes(), &[1, 6]);
    }

    #[test]
    fn expanding_puts_the_children_in_row_order_beneath_their_parent() {
        let mut tree = tree();
        tree.toggle_node(1);
        assert_eq!(tree.active_nodes(), &[1, 2, 3, 6]);

        tree.toggle_node(2);
        assert_eq!(
            tree.active_nodes(),
            &[1, 2, 4, 5, 3, 6],
            "depth first, so 2's children come before 3"
        );
    }

    #[test]
    fn depth_and_parent_are_derived_by_the_walk_rather_than_stored() {
        // Which is why a node moved elsewhere in the tree needs no fixing up.
        let mut tree = tree();
        tree.toggle_node(1);
        tree.toggle_node(2);

        assert_eq!(tree.node(1).unwrap().depth(), Some(0));
        assert_eq!(tree.node(2).unwrap().depth(), Some(1));
        assert_eq!(tree.node(4).unwrap().depth(), Some(2));
        assert_eq!(tree.node(4).unwrap().parent(), Some(2));
        assert_eq!(tree.node(1).unwrap().parent(), None);
    }

    #[test]
    fn collapsing_a_parent_takes_every_descendant_out_of_the_list() {
        let mut tree = tree();
        tree.toggle_node(1);
        tree.toggle_node(2);
        assert_eq!(tree.active_nodes().len(), 6);

        tree.toggle_node(1);
        assert_eq!(tree.active_nodes(), &[1, 6]);
        assert!(
            tree.is_expanded(2),
            "though 2 remembers it was expanded, for when 1 opens again"
        );

        tree.toggle_node(1);
        assert_eq!(tree.active_nodes(), &[1, 2, 4, 5, 3, 6], "and it did");
    }

    #[test]
    fn a_collapsing_nodes_children_stay_in_the_list_while_it_animates() {
        // There would otherwise be nothing left to animate away.
        let mut tree = tree();
        tree.animation_disabled = false;
        tree.toggle_node(1);
        assert_eq!(tree.active_nodes(), &[1, 2, 3, 6]);

        tree.toggle_node(1);
        assert!(tree.is_animating(1));
        assert!(!tree.is_expanded(1), "the flag flipped at once");
        assert_eq!(
            tree.active_nodes(),
            &[1, 2, 3, 6],
            "but the children are still there to animate out"
        );

        tree.finish_toggle_animation(1);
        assert_eq!(tree.active_nodes(), &[1, 6]);
    }

    #[test]
    fn an_expanding_nodes_children_appear_at_once_so_there_is_something_to_animate_in() {
        let mut tree = tree();
        tree.animation_disabled = false;
        tree.toggle_node(1);
        assert!(tree.is_animating(1));
        assert_eq!(tree.active_nodes(), &[1, 2, 3, 6]);

        tree.finish_toggle_animation(1);
        assert_eq!(
            tree.active_nodes(),
            &[1, 2, 3, 6],
            "and finishing changes nothing, which is why upstream skips the unpack"
        );
    }

    #[test]
    fn a_zero_duration_is_no_animation_rather_than_a_very_short_one() {
        // Upstream's comment: treating it as an animation freezes the app,
        // because the tree is updated while the children are no longer active.
        let mut tree = tree();
        tree.animation_disabled = true;
        tree.toggle_node(1);
        assert!(!tree.is_animating(1));
        assert_eq!(tree.active_nodes(), &[1, 2, 3, 6]);

        tree.toggle_node(1);
        assert_eq!(tree.active_nodes(), &[1, 6], "gone immediately");
    }

    #[test]
    fn toggling_a_leaf_does_nothing_at_all() {
        let mut tree = tree();
        tree.toggle_node(6);
        assert!(tree.toggles().is_empty());
        assert_eq!(tree.active_nodes(), &[1, 6]);
    }

    // -- The animating range -----------------------------------------------

    #[test]
    fn the_animating_row_range_is_recomputed_rather_than_recorded() {
        // The indexes move whenever anything above them expands, so a range
        // captured when the animation started would be stale.
        let mut tree = tree();
        tree.animation_disabled = false;
        tree.toggle_node(1);
        assert_eq!(tree.animation_range(1), Some((1, 2)), "rows 1..2");

        // Something above changes the indexes.
        tree.toggle_node(2);
        assert_eq!(
            tree.animation_range(1),
            Some((1, 2)),
            "1's children are still at 1 and 2"
        );
        assert_eq!(
            tree.animation_range(2),
            Some((2, 3)),
            "and 2's are below its own row"
        );
    }

    #[test]
    fn a_leaf_has_no_animating_range() {
        let tree = tree();
        assert_eq!(tree.animation_range(6), None);
    }

    // -- Expand and collapse all -------------------------------------------

    #[test]
    fn expanding_everything_reaches_nodes_nobody_can_see() {
        let mut tree = tree();
        tree.expand_all();
        assert_eq!(tree.active_nodes(), &[1, 2, 4, 5, 3, 6]);
        assert!(tree.is_expanded(1) && tree.is_expanded(2));
    }

    #[test]
    fn a_hidden_node_has_its_flag_set_rather_than_being_toggled() {
        // Toggling a hidden node would start an animation nobody can see, and
        // on a large tree that is one controller per node.
        let mut tree = tree();
        tree.expand_all();
        assert_eq!(
            tree.toggles(),
            &[1],
            "only the one active node was toggled; 2 was hidden and set directly"
        );
    }

    #[test]
    fn collapsing_everything_leaves_only_the_roots() {
        let mut tree = tree();
        tree.expand_all();
        tree.collapse_all();
        assert_eq!(tree.active_nodes(), &[1, 6]);
        assert!(!tree.is_expanded(1) && !tree.is_expanded(2));
    }

    #[test]
    fn the_deferred_toggles_are_collected_deepest_first_and_run_shallowest_first() {
        // The walk is post-order, so 2 lands in the list before 1 -- and then
        // the list is reversed, so 1 is toggled first. It works because a
        // collapsing node keeps its descendants active until its animation
        // ends, which is what upstream's toggleNode assertion relies on.
        let mut tree = tree();
        tree.animation_disabled = false;
        tree.expand_all();
        let before = tree.toggles().len();

        tree.collapse_all();
        let order = &tree.toggles()[before..];
        assert_eq!(order, &[1, 2], "shallowest first, after the reverse");
        assert!(
            tree.is_animating(1) && tree.is_animating(2),
            "and both are on their way out together"
        );
    }

    #[test]
    fn expanding_an_already_expanded_tree_changes_nothing() {
        let mut tree = tree();
        tree.expand_all();
        let toggles = tree.toggles().len();
        tree.expand_all();
        assert_eq!(tree.toggles().len(), toggles);
    }

    // -- Indentation --------------------------------------------------------

    #[test]
    fn each_level_is_offset_by_the_indentation() {
        let mut tree = tree();
        tree.expand_all();
        assert_eq!(tree.indent_for(1), 0.0);
        assert_eq!(tree.indent_for(2), 10.0);
        assert_eq!(tree.indent_for(4), 20.0);
    }

    #[test]
    fn no_indentation_is_a_real_choice_rather_than_a_degenerate_one() {
        // The indent may be built into the row builder instead, as a guide
        // line or a disclosure triangle rather than empty space.
        let mut tree = tree();
        tree.indentation = TreeSliverIndentationType::NONE;
        tree.expand_all();
        assert_eq!(tree.indent_for(4), 0.0);
    }

    #[test]
    fn a_negative_indent_would_walk_deeper_rows_back_out() {
        assert!(TreeSliverIndentationType::custom(-1.0).is_none());
        assert_eq!(
            TreeSliverIndentationType::custom(24.0).unwrap().value(),
            24.0
        );
        assert_eq!(
            TreeSliverIndentationType::default(),
            TreeSliverIndentationType::STANDARD
        );
    }

    // -- The mixin's contract -----------------------------------------------

    #[test]
    fn active_is_not_the_same_question_as_visible() {
        // A row inside a collapsed parent is inactive; a row scrolled a mile
        // off screen is active.
        let mut tree = tree();
        assert!(!tree.is_active(2), "inside a collapsed parent");

        tree.toggle_node(1);
        assert!(tree.is_active(2));
    }

    #[test]
    fn a_node_with_no_row_has_no_row_index() {
        let mut tree = tree();
        assert_eq!(tree.get_active_index_for(2), None);

        tree.toggle_node(1);
        assert_eq!(tree.get_active_index_for(2), Some(1));
        assert_eq!(tree.get_active_index_for(6), Some(3));
    }

    #[test]
    fn finding_a_node_searches_the_whole_tree_and_not_just_the_visible_rows() {
        // So a caller can find something hidden and then expand to it.
        let tree = tree();
        assert_eq!(tree.get_node_for("five"), Some(5));
        assert!(!tree.is_active(5));
        assert_eq!(tree.get_node_for("nothing"), None);
    }

    // -- The controller -----------------------------------------------------

    #[test]
    fn expand_node_is_an_end_state_and_not_a_toggle() {
        // A caller that wanted a toggle has toggleNode.
        let mut tree = tree();
        let mut controller = TreeSliverController::new();
        controller.attach().unwrap();

        controller.expand_node(&mut tree, 1);
        assert!(controller.is_expanded(&tree, 1));

        controller.expand_node(&mut tree, 1);
        assert!(controller.is_expanded(&tree, 1), "still expanded");
        assert_eq!(tree.toggles().len(), 1, "and it did not toggle twice");
    }

    #[test]
    fn collapse_node_is_the_same_the_other_way() {
        let mut tree = tree();
        let mut controller = TreeSliverController::new();
        controller.attach().unwrap();

        controller.collapse_node(&mut tree, 1);
        assert_eq!(tree.toggles().len(), 0, "already collapsed");

        controller.expand_node(&mut tree, 1);
        controller.collapse_node(&mut tree, 1);
        assert!(!controller.is_expanded(&tree, 1));
    }

    #[test]
    fn one_controller_cannot_drive_two_trees() {
        // expandNode would be ambiguous about which tree it meant.
        let mut controller = TreeSliverController::new();
        assert!(controller.attach().is_ok());
        assert!(controller.attach().is_err());

        controller.detach();
        assert!(controller.attach().is_ok());
    }

    #[test]
    #[should_panic(expected = "controller is not attached")]
    fn an_unattached_controller_has_no_tree_to_answer_about() {
        let tree = tree();
        TreeSliverController::new().is_expanded(&tree, 1);
    }
}
