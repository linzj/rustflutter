//! A port of the directional half of `widgets/focus_traversal.dart`:
//! `DirectionalFocusTraversalPolicyMixin`, `DirectionalFocusIntent` and
//! `DirectionalFocusAction`.
//!
//! Next/previous traversal has one right answer, because the widgets are in an
//! order. Arrow keys do not: "the widget to the right of this one" is a
//! question about geometry, and the answer has to be picked. The rule upstream
//! settled on is a *band* -- an infinite strip the width (or height) of the
//! focused widget, extended in the direction of travel. Anything the band
//! touches is a candidate, and the nearest of those wins. If the band is empty
//! the search widens to everything ahead, ranked by how far off the band it is.

use crate::engine::Rect;
use std::cmp::Ordering;
use std::collections::HashMap;

/// Upstream `TraversalDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TraversalDirection {
    Up,
    Right,
    Down,
    Left,
}

impl TraversalDirection {
    /// Whether this direction moves along the vertical axis. The policy's
    /// history is kept per axis, not per direction.
    pub fn is_vertical(self) -> bool {
        matches!(self, TraversalDirection::Up | TraversalDirection::Down)
    }

    pub fn same_axis_as(self, other: TraversalDirection) -> bool {
        self.is_vertical() == other.is_vertical()
    }
}

/// Upstream `ScrollPositionAlignmentPolicy`, as much of it as the pop needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollPositionAlignmentPolicy {
    KeepVisibleAtStart,
    KeepVisibleAtEnd,
}

/// A focusable node as the directional policy sees it: a rectangle, and the
/// scrollable it lives in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraversalCandidate {
    pub id: u64,
    pub rect: Rect,
    /// The nearest enclosing `Scrollable` on the axis being traversed, if any.
    pub scrollable: Option<u64>,
    /// Whether the node is still in the tree. A node in a sliver that scrolled
    /// off screen gets unmounted, and upstream detects that by its parent going
    /// null.
    pub attached: bool,
}

impl TraversalCandidate {
    pub fn new(id: u64, rect: Rect) -> TraversalCandidate {
        TraversalCandidate {
            id,
            rect,
            scrollable: None,
            attached: true,
        }
    }

    pub fn in_scrollable(mut self, scrollable: u64) -> Self {
        self.scrollable = Some(scrollable);
        self
    }

    pub fn detached(mut self) -> Self {
        self.attached = false;
        self
    }

    fn center(&self) -> (f32, f32) {
        self.rect.center()
    }
}

/// Upstream `_DirectionalPolicyDataEntry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectionalPolicyDataEntry {
    direction: TraversalDirection,
    node: u64,
}

/// Upstream `_DirectionalPolicyData`: the path taken to the current node.
#[derive(Clone, Debug, Default, PartialEq)]
struct DirectionalPolicyData {
    history: Vec<DirectionalPolicyDataEntry>,
}

/// What a directional move did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectionalMove {
    /// Nothing lay in that direction.
    Nowhere,
    /// A fresh node was picked by the band search.
    Found(u64),
    /// The history was walked back, because the reader reversed direction. The
    /// alignment policy is what keeps the node the reader is returning to
    /// scrolled to the edge they are coming from.
    Retraced {
        node: u64,
        alignment: ScrollPositionAlignmentPolicy,
    },
}

/// Upstream `DirectionalFocusTraversalPolicyMixin`.
///
/// The state it carries is one stack per scope, and it is there for a single
/// reason: **hysteresis**. The band search is not symmetric. Going right from a
/// narrow field can land on a wide button whose band, going back left, covers
/// a different field. Without a memory, right-then-left leaves the reader
/// somewhere they have never been. With one, it puts them back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DirectionalFocusTraversalPolicyMixin {
    policy_data: HashMap<u64, DirectionalPolicyData>,
}

impl DirectionalFocusTraversalPolicyMixin {
    pub fn new() -> DirectionalFocusTraversalPolicyMixin {
        DirectionalFocusTraversalPolicyMixin::default()
    }

    /// Upstream `invalidateScopeData`.
    pub fn invalidate_scope_data(&mut self, scope: u64) {
        self.policy_data.remove(&scope);
    }

    /// Upstream `changedScope`. A node that moved out of a scope is struck from
    /// that scope's history, rather than the whole history being thrown away:
    /// the rest of the path is still a path.
    pub fn changed_scope(&mut self, node: Option<u64>, old_scope: Option<u64>) {
        let (Some(node), Some(old_scope)) = (node, old_scope) else {
            return;
        };
        if let Some(data) = self.policy_data.get_mut(&old_scope) {
            data.history.retain(|entry| entry.node != node);
        }
    }

    pub fn history_len(&self, scope: u64) -> usize {
        self.policy_data
            .get(&scope)
            .map_or(0, |data| data.history.len())
    }

    /// Upstream `_pushPolicyData`.
    pub fn push_policy_data(&mut self, direction: TraversalDirection, scope: u64, node: u64) {
        self.policy_data
            .entry(scope)
            .or_default()
            .history
            .push(DirectionalPolicyDataEntry { direction, node });
    }

    /// Upstream `_popPolicyDataIfNeeded`.
    ///
    /// Two things are worth reading twice. The guard looks at the **first**
    /// entry -- the direction the path was started in -- while the pop takes
    /// the **last**: the whole stack belongs to one leg of travel, so what
    /// matters is whether this move continues that leg, reverses it, or leaves
    /// its axis entirely.
    ///
    /// And a reversal is not the same as a turn. Reversing walks the path back
    /// one step; turning onto the other axis throws the path away, because a
    /// path built going down says nothing about where left is.
    pub fn pop_policy_data_if_needed(
        &mut self,
        direction: TraversalDirection,
        scope: u64,
        current_scrollable: Option<u64>,
        candidates: &[TraversalCandidate],
    ) -> Option<DirectionalMove> {
        let should_clear;
        {
            let Some(data) = self.policy_data.get(&scope) else {
                return None;
            };
            if data.history.is_empty() {
                self.invalidate_scope_data(scope);
                return None;
            }
            if data.history[0].direction == direction {
                // Still going the same way: keep walking forwards.
                return None;
            }

            let last = data.history[data.history.len() - 1].node;
            let attached = candidates
                .iter()
                .find(|candidate| candidate.id == last)
                .is_some_and(|candidate| candidate.attached);
            if !attached {
                // A node that left the tree -- typically a sliver's child that
                // scrolled out and was unmounted -- must not be focused. The
                // acknowledged cost is that hysteresis is then not avoided for
                // exactly the case a long list makes most likely.
                self.invalidate_scope_data(scope);
                return None;
            }

            should_clear = !data.history[0].direction.same_axis_as(direction);
        }

        if should_clear {
            self.invalidate_scope_data(scope);
            return None;
        }

        let data = self.policy_data.get_mut(&scope).unwrap();
        let entry = data.history.pop().unwrap();
        // Upstream leaves the now-empty record in place; the empty check at the
        // top of the next call is what clears it.

        // Leaving the scrollable invalidates the path as well: the remembered
        // rectangles were measured in a viewport the reader has left.
        let node_scrollable = candidates
            .iter()
            .find(|candidate| candidate.id == entry.node)
            .and_then(|candidate| candidate.scrollable);
        if node_scrollable != current_scrollable {
            self.invalidate_scope_data(scope);
            return None;
        }

        let alignment = match direction {
            TraversalDirection::Up | TraversalDirection::Left => {
                ScrollPositionAlignmentPolicy::KeepVisibleAtStart
            }
            TraversalDirection::Right | TraversalDirection::Down => {
                ScrollPositionAlignmentPolicy::KeepVisibleAtEnd
            }
        };
        Some(DirectionalMove::Retraced {
            node: entry.node,
            alignment,
        })
    }

    /// Upstream `findFirstFocusInDirection`: with nothing focused yet, entry
    /// into a scope starts from the edge the reader is coming from -- moving
    /// down starts at the top, moving up starts at the bottom.
    pub fn find_first_focus_in_direction(
        direction: TraversalDirection,
        candidates: &[TraversalCandidate],
    ) -> Option<u64> {
        let mut sorted: Vec<&TraversalCandidate> = candidates.iter().collect();
        match direction {
            TraversalDirection::Down => {
                sorted.sort_by(|a, b| cmp(a.rect.top, b.rect.top));
            }
            TraversalDirection::Up => {
                sorted.sort_by(|a, b| cmp(b.rect.bottom, a.rect.bottom));
            }
            TraversalDirection::Right => {
                sorted.sort_by(|a, b| cmp(a.rect.left, b.rect.left));
            }
            TraversalDirection::Left => {
                sorted.sort_by(|a, b| cmp(b.rect.right, a.rect.right));
            }
        }
        sorted.first().map(|candidate| candidate.id)
    }

    /// Upstream `_findNodeInDirection` -- the band search.
    pub fn find_node_in_direction(
        direction: TraversalDirection,
        focused: &TraversalCandidate,
        candidates: &[TraversalCandidate],
        forward: bool,
    ) -> Option<u64> {
        let target = focused.rect;
        let (tx, ty) = focused.center();

        // Filter to what lies ahead. Note that the test is on the candidate's
        // *centre* against the focused node's *edge*: a widget that overlaps
        // the focused one is still ahead of it if its middle is past the edge.
        // And `rect != target` drops coincident rectangles by geometry rather
        // than by identity, so two widgets stacked exactly on top of each other
        // cannot reach one another at all.
        let mut eligible: Vec<&TraversalCandidate> = candidates
            .iter()
            .filter(|candidate| {
                if candidate.rect == target {
                    return false;
                }
                let (cx, cy) = candidate.center();
                match direction {
                    TraversalDirection::Down => {
                        if forward {
                            cy >= target.bottom
                        } else {
                            cy <= target.bottom
                        }
                    }
                    TraversalDirection::Up => {
                        if forward {
                            cy <= target.top
                        } else {
                            cy >= target.top
                        }
                    }
                    TraversalDirection::Right => {
                        if forward {
                            cx >= target.right
                        } else {
                            cx <= target.right
                        }
                    }
                    TraversalDirection::Left => {
                        if forward {
                            cx <= target.left
                        } else {
                            cx >= target.left
                        }
                    }
                }
            })
            .collect();
        if eligible.is_empty() {
            return None;
        }

        if direction.is_vertical() {
            eligible.sort_by(|a, b| cmp(a.center().1, b.center().1));
        } else {
            eligible.sort_by(|a, b| cmp(a.center().0, b.center().0));
        }

        // Prefer staying inside the scrollable the reader is in -- but only if
        // that leaves anything. It is a preference, not a fence: focus can walk
        // out of a list, just not while the list still has somewhere to go.
        if let Some(scrollable) = focused.scrollable {
            let inside: Vec<&TraversalCandidate> = eligible
                .iter()
                .copied()
                .filter(|candidate| candidate.scrollable == Some(scrollable))
                .collect();
            if !inside.is_empty() {
                eligible = inside;
            }
        }

        if matches!(direction, TraversalDirection::Up | TraversalDirection::Left) {
            eligible.reverse();
        }

        // The band: an infinite strip the width (or height) of the focused
        // widget. Dart's `Rect.intersect(...).isEmpty` is a strict test, so a
        // widget merely touching the band's edge is out of it.
        let in_band: Vec<&TraversalCandidate> = eligible
            .iter()
            .copied()
            .filter(|candidate| {
                if direction.is_vertical() {
                    candidate.rect.right > target.left && candidate.rect.left < target.right
                } else {
                    candidate.rect.bottom > target.top && candidate.rect.top < target.bottom
                }
            })
            .collect();

        if !in_band.is_empty() {
            let mut sorted = in_band;
            if direction.is_vertical() {
                sorted.sort_by(|a, b| {
                    vertical_compare(ty, a.center().1, b.center().1)
                        .then_with(|| horizontal_compare(tx, a.center().0, b.center().0))
                });
            } else {
                sorted.sort_by(|a, b| {
                    horizontal_compare(tx, a.center().0, b.center().0)
                        .then_with(|| vertical_compare(ty, a.center().1, b.center().1))
                });
            }
            return Some(if forward {
                sorted.first().unwrap().id
            } else {
                sorted.last().unwrap().id
            });
        }

        // Nothing in the band, so widen: rank by how far off the band each one
        // is *across* the direction of travel, and break ties by how far along
        // it. Distance is measured to whichever of the candidate's two edges is
        // nearer, so a wide widget reaching towards the band beats a small one
        // whose centre happens to be closer.
        let mut sorted = eligible;
        if direction.is_vertical() {
            sorted.sort_by(|a, b| {
                horizontal_compare_closest_edge(tx, a.rect, b.rect)
                    .then_with(|| vertical_compare(ty, a.center().1, b.center().1))
            });
        } else {
            sorted.sort_by(|a, b| {
                vertical_compare_closest_edge(ty, a.rect, b.rect)
                    .then_with(|| horizontal_compare(tx, a.center().0, b.center().0))
            });
        }
        Some(if forward {
            sorted.first().unwrap().id
        } else {
            sorted.last().unwrap().id
        })
    }

    /// The whole move: retrace if the reader reversed, otherwise search and
    /// remember where they came from.
    pub fn in_direction(
        &mut self,
        direction: TraversalDirection,
        scope: u64,
        focused: &TraversalCandidate,
        candidates: &[TraversalCandidate],
    ) -> DirectionalMove {
        if let Some(retraced) =
            self.pop_policy_data_if_needed(direction, scope, focused.scrollable, candidates)
        {
            return retraced;
        }
        match DirectionalFocusTraversalPolicyMixin::find_node_in_direction(
            direction, focused, candidates, true,
        ) {
            Some(found) => {
                self.push_policy_data(direction, scope, focused.id);
                DirectionalMove::Found(found)
            }
            None => DirectionalMove::Nowhere,
        }
    }
}

fn cmp(a: f32, b: f32) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

fn vertical_compare(target: f32, a: f32, b: f32) -> Ordering {
    cmp((a - target).abs(), (b - target).abs())
}

fn horizontal_compare(target: f32, a: f32, b: f32) -> Ordering {
    cmp((a - target).abs(), (b - target).abs())
}

fn closest_edge(target: f32, near: f32, far: f32) -> f32 {
    if (near - target).abs() < (far - target).abs() {
        near
    } else {
        far
    }
}

fn vertical_compare_closest_edge(target: f32, a: Rect, b: Rect) -> Ordering {
    let a_coord = closest_edge(target, a.top, a.bottom);
    let b_coord = closest_edge(target, b.top, b.bottom);
    cmp((a_coord - target).abs(), (b_coord - target).abs())
}

fn horizontal_compare_closest_edge(target: f32, a: Rect, b: Rect) -> Ordering {
    let a_coord = closest_edge(target, a.left, a.right);
    let b_coord = closest_edge(target, b.left, b.right);
    cmp((a_coord - target).abs(), (b_coord - target).abs())
}

/// Upstream `DirectionalFocusIntent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalFocusIntent {
    pub direction: TraversalDirection,
    /// Defaults to `true`. Inside a text field an arrow key moves the caret,
    /// and a reader pressing it is asking to move within the text, not to leave
    /// it -- so the intent carries permission to be ignored there.
    pub ignore_text_fields: bool,
}

impl DirectionalFocusIntent {
    pub fn new(direction: TraversalDirection) -> DirectionalFocusIntent {
        DirectionalFocusIntent {
            direction,
            ignore_text_fields: true,
        }
    }

    pub fn with_ignore_text_fields(mut self, ignore: bool) -> Self {
        self.ignore_text_fields = ignore;
        self
    }
}

/// Upstream `DirectionalFocusAction`.
///
/// The decision is split across the two: the *intent* says whether it may be
/// ignored in a text field, and the *action* knows whether it is in one. Either
/// half alone would be wrong -- a key binding cannot know where it will be
/// invoked, and an action cannot know what the binding meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalFocusAction {
    is_for_text_field: bool,
}

impl DirectionalFocusAction {
    pub fn new() -> DirectionalFocusAction {
        DirectionalFocusAction {
            is_for_text_field: false,
        }
    }

    /// Upstream `DirectionalFocusAction.forTextField`.
    pub fn for_text_field() -> DirectionalFocusAction {
        DirectionalFocusAction {
            is_for_text_field: true,
        }
    }

    /// Upstream `invoke`, which returns nothing and calls
    /// `primaryFocus!.focusInDirection`. Here it returns the direction to move
    /// in, or `None` when the key belongs to the text field.
    pub fn invoke(&self, intent: DirectionalFocusIntent) -> Option<TraversalDirection> {
        if !intent.ignore_text_fields || !self.is_for_text_field {
            Some(intent.direction)
        } else {
            None
        }
    }
}

impl Default for DirectionalFocusAction {
    fn default() -> Self {
        DirectionalFocusAction::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use TraversalDirection::{Down, Left, Right, Up};

    const SCOPE: u64 = 1;

    fn at(id: u64, left: f32, top: f32, right: f32, bottom: f32) -> TraversalCandidate {
        TraversalCandidate::new(id, Rect::ltrb(left, top, right, bottom))
    }

    fn find(
        direction: TraversalDirection,
        focused: &TraversalCandidate,
        candidates: &[TraversalCandidate],
    ) -> Option<u64> {
        DirectionalFocusTraversalPolicyMixin::find_node_in_direction(
            direction, focused, candidates, true,
        )
    }

    // -- The band -------------------------------------------------------------

    #[test]
    fn something_far_away_in_the_band_beats_something_near_beside_it() {
        // Which is the whole rule: "below" means below *this widget*, not
        // merely further down the screen than it.
        let focused = at(1, 0.0, 0.0, 50.0, 20.0);
        let far_below = at(2, 0.0, 200.0, 50.0, 220.0);
        let near_aside = at(3, 100.0, 25.0, 150.0, 45.0);
        assert_eq!(find(Down, &focused, &[far_below, near_aside]), Some(2));
    }

    #[test]
    fn a_widget_touching_the_bands_edge_is_outside_it() {
        // The intersection test is strict, so sharing an edge is not
        // overlapping.
        let focused = at(1, 0.0, 0.0, 50.0, 20.0);
        let touching = at(2, 50.0, 40.0, 100.0, 60.0);
        let in_band_far = at(3, 0.0, 400.0, 50.0, 420.0);
        assert_eq!(find(Down, &focused, &[touching, in_band_far]), Some(3));

        // A single pixel of overlap changes the answer.
        let overlapping = at(2, 49.0, 40.0, 100.0, 60.0);
        assert_eq!(find(Down, &focused, &[overlapping, in_band_far]), Some(2));
    }

    #[test]
    fn out_of_band_targets_are_ranked_by_their_nearest_edge_not_their_middle() {
        // So a wide widget reaching towards the band beats a narrow one whose
        // centre happens to sit closer to it.
        let focused = at(1, 0.0, 0.0, 50.0, 20.0);
        let wide = at(2, 60.0, 100.0, 300.0, 120.0);
        let narrow = at(3, 100.0, 40.0, 140.0, 60.0);
        assert_eq!(
            find(Down, &focused, &[wide, narrow]),
            Some(2),
            "wide's near edge is 35 away; narrow's is 75"
        );
    }

    #[test]
    fn a_widget_overlapping_the_focused_one_counts_as_ahead_only_past_its_middle() {
        // The filter puts the candidate's *centre* against the focused
        // widget's *edge*, so overlap alone neither qualifies nor disqualifies.
        let focused = at(1, 0.0, 0.0, 50.0, 100.0);
        let centre_past_the_bottom = at(2, 0.0, 60.0, 50.0, 140.0);
        let centre_still_above = at(3, 0.0, 50.0, 50.0, 120.0);
        assert_eq!(find(Down, &focused, &[centre_past_the_bottom]), Some(2));
        assert_eq!(find(Down, &focused, &[centre_still_above]), None);
    }

    #[test]
    fn two_widgets_on_the_very_same_rectangle_cannot_reach_each_other() {
        // The exclusion is by geometry rather than identity, which is a real
        // difference for a stack.
        let focused = at(1, 0.0, 0.0, 50.0, 20.0);
        let stacked = at(2, 0.0, 0.0, 50.0, 20.0);
        assert_eq!(find(Down, &focused, &[stacked]), None);
        assert_eq!(find(Up, &focused, &[stacked]), None);
    }

    #[test]
    fn nothing_ahead_is_answered_with_nothing() {
        let focused = at(1, 0.0, 200.0, 50.0, 220.0);
        let above = at(2, 0.0, 0.0, 50.0, 20.0);
        assert_eq!(find(Down, &focused, &[above]), None);
        assert_eq!(find(Up, &focused, &[above]), Some(2));
    }

    #[test]
    fn the_horizontal_band_is_the_height_of_the_focused_widget() {
        let focused = at(1, 0.0, 0.0, 20.0, 50.0);
        let far_right_in_row = at(2, 400.0, 0.0, 420.0, 50.0);
        let near_but_below = at(3, 30.0, 100.0, 50.0, 150.0);
        assert_eq!(
            find(Right, &focused, &[far_right_in_row, near_but_below]),
            Some(2)
        );
    }

    // -- Entering a scope ------------------------------------------------------

    #[test]
    fn entering_a_scope_starts_at_the_edge_you_came_in_through() {
        let nodes = [
            at(1, 0.0, 0.0, 20.0, 20.0),
            at(2, 100.0, 100.0, 120.0, 120.0),
        ];
        let first = |direction| {
            DirectionalFocusTraversalPolicyMixin::find_first_focus_in_direction(direction, &nodes)
        };
        assert_eq!(first(Down), Some(1), "moving down starts at the top");
        assert_eq!(first(Up), Some(2), "moving up starts at the bottom");
        assert_eq!(first(Right), Some(1), "moving right starts at the left");
        assert_eq!(first(Left), Some(2), "moving left starts at the right");
    }

    #[test]
    fn an_empty_scope_has_no_first_focus() {
        assert_eq!(
            DirectionalFocusTraversalPolicyMixin::find_first_focus_in_direction(Down, &[]),
            None
        );
    }

    // -- Scrollables -----------------------------------------------------------

    #[test]
    fn focus_prefers_the_list_it_is_already_in() {
        let focused = at(1, 0.0, 0.0, 50.0, 20.0).in_scrollable(7);
        let next_in_list = at(2, 0.0, 300.0, 50.0, 320.0).in_scrollable(7);
        let nearer_outside = at(3, 0.0, 30.0, 50.0, 50.0);
        assert_eq!(
            find(Down, &focused, &[next_in_list, nearer_outside]),
            Some(2)
        );
    }

    #[test]
    fn but_it_leaves_the_list_once_the_list_has_nowhere_left_to_go() {
        // It is a preference, not a fence -- otherwise the last row of a list
        // would be a dead end.
        let focused = at(1, 0.0, 0.0, 50.0, 20.0).in_scrollable(7);
        let below_the_list = at(3, 0.0, 30.0, 50.0, 50.0);
        assert_eq!(find(Down, &focused, &[below_the_list]), Some(3));
    }

    // -- Hysteresis ------------------------------------------------------------

    // A small field, a wide button under it, and a second field whose middle
    // sits closer to the button's middle than the first field's does. Going
    // down from `a` lands on `b`; going up from `b` lands on `c`.
    fn asymmetric() -> (TraversalCandidate, TraversalCandidate, TraversalCandidate) {
        (
            at(1, 0.0, 0.0, 50.0, 20.0),
            at(2, 0.0, 40.0, 200.0, 60.0),
            at(3, 120.0, 0.0, 160.0, 20.0),
        )
    }

    #[test]
    fn the_search_alone_does_not_bring_you_back_where_you_came_from() {
        // This is the asymmetry the history exists to paper over; without it
        // the rest of these tests would prove nothing.
        let (a, b, c) = asymmetric();
        assert_eq!(find(Down, &a, &[b, c]), Some(2));
        assert_eq!(find(Up, &b, &[a, c]), Some(3), "not back to a");
    }

    #[test]
    fn reversing_direction_retraces_the_path_instead_of_searching_again() {
        let (a, b, c) = asymmetric();
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        let nodes = [a, b, c];

        assert_eq!(
            policy.in_direction(Down, SCOPE, &a, &nodes),
            DirectionalMove::Found(2)
        );
        assert_eq!(
            policy.in_direction(Up, SCOPE, &b, &nodes),
            DirectionalMove::Retraced {
                node: 1,
                alignment: ScrollPositionAlignmentPolicy::KeepVisibleAtStart,
            }
        );
    }

    #[test]
    fn the_alignment_keeps_the_node_against_the_edge_you_are_coming_from() {
        let (a, b, _) = asymmetric();
        let nodes = [a, b];
        for (outbound, inbound, alignment) in [
            (Down, Up, ScrollPositionAlignmentPolicy::KeepVisibleAtStart),
            (Up, Down, ScrollPositionAlignmentPolicy::KeepVisibleAtEnd),
            (
                Right,
                Left,
                ScrollPositionAlignmentPolicy::KeepVisibleAtStart,
            ),
            (Left, Right, ScrollPositionAlignmentPolicy::KeepVisibleAtEnd),
        ] {
            let mut policy = DirectionalFocusTraversalPolicyMixin::new();
            policy.push_policy_data(outbound, SCOPE, 1);
            assert_eq!(
                policy.pop_policy_data_if_needed(inbound, SCOPE, None, &nodes),
                Some(DirectionalMove::Retraced { node: 1, alignment }),
                "{outbound:?} then {inbound:?}"
            );
        }
    }

    #[test]
    fn continuing_the_same_way_keeps_walking_forwards() {
        let (a, b, c) = asymmetric();
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        let nodes = [a, b, c];
        policy.in_direction(Down, SCOPE, &a, &nodes);
        assert_eq!(
            policy.pop_policy_data_if_needed(Down, SCOPE, None, &nodes),
            None
        );
        assert_eq!(policy.history_len(SCOPE), 1, "the path was left alone");
    }

    #[test]
    fn turning_onto_the_other_axis_throws_the_path_away() {
        // A path built going down says nothing about where left is.
        let (a, b, c) = asymmetric();
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        let nodes = [a, b, c];
        policy.in_direction(Down, SCOPE, &a, &nodes);
        assert_eq!(policy.history_len(SCOPE), 1);

        assert_eq!(
            policy.pop_policy_data_if_needed(Left, SCOPE, None, &nodes),
            None
        );
        assert_eq!(policy.history_len(SCOPE), 0, "and cleared, not popped");
    }

    #[test]
    fn the_whole_path_is_walked_back_one_step_at_a_time() {
        let a = at(1, 0.0, 0.0, 50.0, 20.0);
        let b = at(2, 0.0, 40.0, 50.0, 60.0);
        let c = at(3, 0.0, 80.0, 50.0, 100.0);
        let nodes = [a, b, c];
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        policy.in_direction(Down, SCOPE, &a, &nodes);
        policy.in_direction(Down, SCOPE, &b, &nodes);
        assert_eq!(policy.history_len(SCOPE), 2);

        assert_eq!(
            policy.in_direction(Up, SCOPE, &c, &nodes),
            DirectionalMove::Retraced {
                node: 2,
                alignment: ScrollPositionAlignmentPolicy::KeepVisibleAtStart,
            }
        );
        assert_eq!(
            policy.in_direction(Up, SCOPE, &b, &nodes),
            DirectionalMove::Retraced {
                node: 1,
                alignment: ScrollPositionAlignmentPolicy::KeepVisibleAtStart,
            }
        );
    }

    #[test]
    fn a_node_that_scrolled_out_of_the_tree_is_not_focused_again() {
        // A sliver child that went off screen was unmounted. The acknowledged
        // cost is that this is exactly where a long list would want hysteresis.
        let (a, b, c) = asymmetric();
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        policy.in_direction(Down, SCOPE, &a, &[a, b, c]);

        let gone = [a.detached(), b, c];
        assert_eq!(
            policy.pop_policy_data_if_needed(Up, SCOPE, None, &gone),
            None
        );
        assert_eq!(policy.history_len(SCOPE), 0);
    }

    #[test]
    fn leaving_the_scrollable_invalidates_the_remembered_path() {
        // The remembered rectangle was measured in a viewport the reader has
        // left.
        let a = at(1, 0.0, 0.0, 50.0, 20.0).in_scrollable(7);
        let b = at(2, 0.0, 40.0, 200.0, 60.0);
        let nodes = [a, b];
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        policy.push_policy_data(Down, SCOPE, 1);

        assert_eq!(
            policy.pop_policy_data_if_needed(Up, SCOPE, None, &nodes),
            None,
            "a is in scrollable 7 and the focus is not"
        );
        assert_eq!(policy.history_len(SCOPE), 0);
    }

    #[test]
    fn a_node_that_moved_out_of_the_scope_is_struck_from_its_path() {
        // The rest of the path is still a path.
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        policy.push_policy_data(Down, SCOPE, 1);
        policy.push_policy_data(Down, SCOPE, 2);

        policy.changed_scope(Some(1), Some(SCOPE));
        assert_eq!(policy.history_len(SCOPE), 1);

        policy.changed_scope(Some(2), None);
        assert_eq!(
            policy.history_len(SCOPE),
            1,
            "no old scope, nothing to strike"
        );
    }

    #[test]
    fn a_move_that_finds_nowhere_to_go_records_nothing() {
        let a = at(1, 0.0, 200.0, 50.0, 220.0);
        let above = at(2, 0.0, 0.0, 50.0, 20.0);
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        assert_eq!(
            policy.in_direction(Down, SCOPE, &a, &[above]),
            DirectionalMove::Nowhere
        );
        assert_eq!(policy.history_len(SCOPE), 0);
    }

    #[test]
    fn each_scope_keeps_its_own_path() {
        let (a, b, c) = asymmetric();
        let nodes = [a, b, c];
        let mut policy = DirectionalFocusTraversalPolicyMixin::new();
        policy.in_direction(Down, SCOPE, &a, &nodes);
        assert_eq!(policy.history_len(SCOPE), 1);
        assert_eq!(policy.history_len(2), 0);
        assert_eq!(
            policy.in_direction(Up, 2, &b, &nodes),
            DirectionalMove::Found(3),
            "the other scope searches rather than retracing"
        );
    }

    // -- The intent and the action ---------------------------------------------

    #[test]
    fn an_arrow_key_in_a_text_field_moves_the_caret_rather_than_the_focus() {
        let intent = DirectionalFocusIntent::new(Right);
        assert!(intent.ignore_text_fields, "which is the default");
        assert_eq!(
            DirectionalFocusAction::for_text_field().invoke(intent),
            None
        );
        assert_eq!(
            DirectionalFocusAction::new().invoke(intent),
            Some(Right),
            "the same intent outside one moves the focus"
        );
    }

    #[test]
    fn an_intent_can_insist_on_leaving_the_text_field() {
        // Which is why the decision is split: the binding says whether it may
        // be ignored, and the action knows where it landed.
        let insistent = DirectionalFocusIntent::new(Down).with_ignore_text_fields(false);
        assert_eq!(
            DirectionalFocusAction::for_text_field().invoke(insistent),
            Some(Down)
        );
        assert_eq!(DirectionalFocusAction::new().invoke(insistent), Some(Down));
    }

    #[test]
    fn the_axis_is_what_the_history_is_kept_by() {
        assert!(Up.is_vertical() && Down.is_vertical());
        assert!(!Left.is_vertical() && !Right.is_vertical());
        assert!(Up.same_axis_as(Down));
        assert!(Left.same_axis_as(Right));
        assert!(!Up.same_axis_as(Right));
    }
}
