// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Values that depend on what a control is doing, from upstream
//! `widgets/widget_state.dart`.
//!
//! A button is not one colour. It is one colour while it sits there, another
//! under the pointer, another while held, and another when it is switched
//! off, and the same is true of its border, its text style and its cursor.
//! Upstream's answer is a value that is a function of a set of
//! [`WidgetState`]s -- a `WidgetStateProperty<T>` -- and a theme that carries
//! those instead of plain values. Every Material control reads its paint
//! through one, so this is the floor the material wave stands on.
//!
//! ```ignore
//! let fill = WidgetStateColor::from_map(vec![
//!     (WidgetStatesConstraint::state(WidgetState::Disabled), Color::GREY),
//!     (WidgetStatesConstraint::state(WidgetState::Pressed), Color::BLUE_DARK),
//!     (WidgetStatesConstraint::ANY, Color::BLUE),
//! ]);
//! let colour = fill.resolve(states);
//! ```
//!
//! # The set is a bit set
//!
//! Upstream's states are a `Set<WidgetState>`, built and passed around by
//! every control. [`WidgetStates`] is that set as bits: eight states, one
//! word, `Copy`, and comparable -- which matters because a control compares
//! the set it had against the set it has to decide whether to repaint.
//!
//! # Recorded divergences
//!
//! * Upstream's typed properties (`WidgetStateColor` and friends) *are* the
//!   value type as well as the property -- `WidgetStateColor extends Color`
//!   -- so a resolved-or-not value can be passed anywhere the plain type is
//!   accepted and resolved later by `WidgetStateProperty.resolveAs`. Rust has
//!   no inheritance to do that with, so each is its own type and the caller
//!   resolves at the point of use. [`resolve_as`] is kept for the "plain
//!   value or property" case, taking the two explicitly.
//! * `WidgetStateMapper` with no matching key answers `None` for an optional
//!   value and throws for a non-nullable one. Here the mapper answers
//!   `Option<T>`, and the typed properties fall back to the value they were
//!   given -- the error case upstream reports at runtime is a missing arm the
//!   type system asks about up front.

use std::rc::Rc;

use crate::borders::{BorderSide, ShapeBorder};
use crate::engine::{Color, TextStyle};
use crate::foundation::ValueNotifier;
use crate::services::system::SystemMouseCursor;

/// Upstream `WidgetState`: the states a control can be in, as far as its
/// appearance is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetState {
    /// The pointer is over it.
    Hovered,
    /// It has the keyboard's attention.
    Focused,
    /// It is being held down.
    Pressed,
    /// It is being dragged.
    Dragged,
    /// It is selected -- a checked checkbox, a chosen tab.
    Selected,
    /// Something has been scrolled underneath it (an app bar's shadow).
    ScrolledUnder,
    /// It cannot be interacted with.
    Disabled,
    /// Its input failed validation.
    Error,
}

impl WidgetState {
    /// Every state, in upstream's declaration order.
    pub const ALL: [WidgetState; 8] = [
        WidgetState::Hovered,
        WidgetState::Focused,
        WidgetState::Pressed,
        WidgetState::Dragged,
        WidgetState::Selected,
        WidgetState::ScrolledUnder,
        WidgetState::Disabled,
        WidgetState::Error,
    ];

    const fn bit(self) -> u8 {
        match self {
            WidgetState::Hovered => 1 << 0,
            WidgetState::Focused => 1 << 1,
            WidgetState::Pressed => 1 << 2,
            WidgetState::Dragged => 1 << 3,
            WidgetState::Selected => 1 << 4,
            WidgetState::ScrolledUnder => 1 << 5,
            WidgetState::Disabled => 1 << 6,
            WidgetState::Error => 1 << 7,
        }
    }
}

/// Upstream's `Set<WidgetState>`, as bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WidgetStates(u8);

impl WidgetStates {
    /// No state at all: a control sitting there, enabled and untouched.
    pub const NONE: WidgetStates = WidgetStates(0);

    pub fn of(states: &[WidgetState]) -> WidgetStates {
        let mut set = WidgetStates::NONE;
        for state in states {
            set = set.with(*state);
        }
        set
    }

    pub const fn with(self, state: WidgetState) -> WidgetStates {
        WidgetStates(self.0 | state.bit())
    }

    pub const fn without(self, state: WidgetState) -> WidgetStates {
        WidgetStates(self.0 & !state.bit())
    }

    pub const fn contains(self, state: WidgetState) -> bool {
        self.0 & state.bit() != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `add` or `remove`, as upstream's `WidgetStatesController.update` puts
    /// it. Answers whether the set actually changed.
    pub fn update(&mut self, state: WidgetState, add: bool) -> bool {
        let updated = if add {
            self.with(state)
        } else {
            self.without(state)
        };
        let changed = updated != *self;
        *self = updated;
        changed
    }

    /// The states in it, in upstream's declaration order.
    pub fn iter(self) -> impl Iterator<Item = WidgetState> {
        WidgetState::ALL
            .into_iter()
            .filter(move |state| self.contains(*state))
    }
}

/// Upstream `WidgetStatesConstraint`: a question about a set of states.
///
/// Upstream it is a mixin with `&`, `|` and `~` operators, and the enum
/// itself satisfies it. Here it is the closed set of shapes those operators
/// build, with [`WidgetState`] the leaf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetStatesConstraint {
    /// Upstream `WidgetState.isSatisfiedBy`: the set contains this state.
    State(WidgetState),
    /// Upstream `WidgetState.any`, `_AnyWidgetStates`: satisfied by
    /// everything, which is what a map's last arm wants.
    Any,
    /// Upstream `operator &`, `_WidgetStateAnd`.
    And(Box<WidgetStatesConstraint>, Box<WidgetStatesConstraint>),
    /// Upstream `operator |`, `_WidgetStateOr`.
    Or(Box<WidgetStatesConstraint>, Box<WidgetStatesConstraint>),
    /// Upstream `operator ~`, `_WidgetStateNot`.
    Not(Box<WidgetStatesConstraint>),
}

impl WidgetStatesConstraint {
    /// Upstream `WidgetState.any`.
    pub const ANY: WidgetStatesConstraint = WidgetStatesConstraint::Any;

    pub const fn state(state: WidgetState) -> WidgetStatesConstraint {
        WidgetStatesConstraint::State(state)
    }

    /// Upstream `operator &`.
    pub fn and(self, other: WidgetStatesConstraint) -> WidgetStatesConstraint {
        WidgetStatesConstraint::And(Box::new(self), Box::new(other))
    }

    /// Upstream `operator |`.
    pub fn or(self, other: WidgetStatesConstraint) -> WidgetStatesConstraint {
        WidgetStatesConstraint::Or(Box::new(self), Box::new(other))
    }

    /// Upstream `operator ~`.
    pub fn not(self) -> WidgetStatesConstraint {
        WidgetStatesConstraint::Not(Box::new(self))
    }

    /// Upstream `isSatisfiedBy`.
    pub fn is_satisfied_by(&self, states: WidgetStates) -> bool {
        match self {
            WidgetStatesConstraint::State(state) => states.contains(*state),
            WidgetStatesConstraint::Any => true,
            WidgetStatesConstraint::And(first, second) => {
                first.is_satisfied_by(states) && second.is_satisfied_by(states)
            }
            WidgetStatesConstraint::Or(first, second) => {
                first.is_satisfied_by(states) || second.is_satisfied_by(states)
            }
            WidgetStatesConstraint::Not(inner) => !inner.is_satisfied_by(states),
        }
    }
}

impl From<WidgetState> for WidgetStatesConstraint {
    fn from(state: WidgetState) -> WidgetStatesConstraint {
        WidgetStatesConstraint::State(state)
    }
}

/// Upstream `WidgetPropertyResolver<T>`: the callback form of a property.
pub type WidgetPropertyResolver<T> = Rc<dyn Fn(WidgetStates) -> T>;

/// Upstream `WidgetStateProperty<T>`: a value that depends on the states.
pub trait WidgetStateProperty<T> {
    /// Upstream `resolve`.
    fn resolve(&self, states: WidgetStates) -> T;
}

/// Upstream `WidgetStateProperty.resolveAs`: a value that may or may not be
/// a property, resolved either way.
///
/// Upstream tells the two apart with a type test, because a
/// `WidgetStateColor` *is* a `Color`. Here they are different types, so the
/// caller says which it has -- and that is the whole of the difference.
pub fn resolve_as<T: Clone>(value: &MaybeStateful<T>, states: WidgetStates) -> T {
    match value {
        MaybeStateful::Plain(value) => value.clone(),
        MaybeStateful::Stateful(property) => property.resolve(states),
    }
}

/// A slot that upstream would type as `T` and fill with either a plain value
/// or a `WidgetStateProperty<T>` -- a theme field, most often.
pub enum MaybeStateful<T> {
    Plain(T),
    Stateful(Rc<dyn WidgetStateProperty<T>>),
}

impl<T: Clone> MaybeStateful<T> {
    pub fn resolve(&self, states: WidgetStates) -> T {
        resolve_as(self, states)
    }
}

/// Upstream `_WidgetStatePropertyWith`, which is what
/// `WidgetStateProperty.resolveWith` builds: the callback as a property.
pub struct WidgetStatePropertyWith<T> {
    resolver: WidgetPropertyResolver<T>,
}

impl<T> WidgetStatePropertyWith<T> {
    /// Upstream `WidgetStateProperty.resolveWith`.
    pub fn new(resolver: WidgetPropertyResolver<T>) -> WidgetStatePropertyWith<T> {
        WidgetStatePropertyWith { resolver }
    }
}

impl<T> WidgetStateProperty<T> for WidgetStatePropertyWith<T> {
    fn resolve(&self, states: WidgetStates) -> T {
        (self.resolver)(states)
    }
}

/// A [`WidgetStateProperty`] held in a theme.
///
/// Upstream a theme field is `WidgetStateProperty<T>?` and two themes compare
/// with `==`, which for a `WidgetStatePropertyAll` is value equality and for
/// a `resolveWith` callback is identity. Every property here is behind an
/// `Rc`, so this compares by identity throughout: the same property object is
/// the same property, and a theme rebuilt with a freshly built resolver
/// counts as changed -- which is the safe direction, since a resolver may
/// close over anything.
pub struct StateProperty<T>(Rc<dyn WidgetStateProperty<T>>);

impl<T> StateProperty<T> {
    pub fn new(property: Rc<dyn WidgetStateProperty<T>>) -> StateProperty<T> {
        StateProperty(property)
    }

    pub fn resolve(&self, states: WidgetStates) -> T {
        self.0.resolve(states)
    }
}

impl<T: 'static> StateProperty<T> {
    /// Upstream `WidgetStateProperty.resolveWith`.
    pub fn resolve_with(resolver: impl Fn(WidgetStates) -> T + 'static) -> StateProperty<T> {
        StateProperty(Rc::new(WidgetStatePropertyWith::new(Rc::new(resolver))))
    }
}

impl<T: Clone + 'static> StateProperty<T> {
    /// Upstream `WidgetStatePropertyAll`.
    pub fn all(value: T) -> StateProperty<T> {
        StateProperty(Rc::new(WidgetStatePropertyAll(value)))
    }
}

impl<T> Clone for StateProperty<T> {
    fn clone(&self) -> StateProperty<T> {
        StateProperty(Rc::clone(&self.0))
    }
}

impl<T> PartialEq for StateProperty<T> {
    fn eq(&self, other: &StateProperty<T>) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> std::fmt::Debug for StateProperty<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The property is a callback; there is nothing to print but that it
        // is there.
        formatter.write_str("StateProperty(..)")
    }
}

/// Upstream `WidgetStateProperty.lerp`, as a theme field: both ends resolved
/// against the same states and then blended.
pub fn lerp_state_property<T: Clone + 'static>(
    a: Option<&StateProperty<T>>,
    b: Option<&StateProperty<T>>,
    t: f32,
    lerp: impl Fn(Option<T>, Option<T>, f32) -> T + 'static,
) -> Option<StateProperty<T>> {
    if a.is_none() && b.is_none() {
        return None;
    }
    let (a, b) = (a.cloned(), b.cloned());
    Some(StateProperty::resolve_with(move |states| {
        let first = a.as_ref().map(|property| property.resolve(states));
        let second = b.as_ref().map(|property| property.resolve(states));
        lerp(first, second, t)
    }))
}

/// Upstream `MaterialTapTargetSize`: whether a control pads itself out to the
/// minimum touch target or takes only the room it draws in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MaterialTapTargetSize {
    /// Padded to the 48-by-48 minimum -- upstream's default on a touch
    /// platform, and what an accessible touch target needs.
    #[default]
    Padded,
    /// No padding: the control is as big as it draws.
    ShrinkWrap,
}

impl MaterialTapTargetSize {
    /// Upstream's `kMinInteractiveDimension`.
    pub const MIN_INTERACTIVE_DIMENSION: f32 = 48.0;

    /// The smallest box a control at this size may occupy.
    pub fn minimum_size(self, drawn: crate::render::Size) -> crate::render::Size {
        match self {
            MaterialTapTargetSize::Padded => crate::render::Size::new(
                drawn
                    .width
                    .max(MaterialTapTargetSize::MIN_INTERACTIVE_DIMENSION),
                drawn
                    .height
                    .max(MaterialTapTargetSize::MIN_INTERACTIVE_DIMENSION),
            ),
            MaterialTapTargetSize::ShrinkWrap => drawn,
        }
    }
}

/// Upstream `WidgetStatePropertyAll<T>`: the same value whatever the states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetStatePropertyAll<T>(pub T);

impl<T: Clone> WidgetStateProperty<T> for WidgetStatePropertyAll<T> {
    fn resolve(&self, _states: WidgetStates) -> T {
        self.0.clone()
    }
}

/// Upstream `WidgetStateMap<T>`: the arms of a [`WidgetStateMapper`], in the
/// order they are tried.
///
/// Upstream's is a `Map`, whose iteration order is insertion order; a `Vec`
/// of pairs is that order made explicit, and first-match-wins is the whole
/// of the semantics.
pub type WidgetStateMap<T> = Vec<(WidgetStatesConstraint, T)>;

/// Upstream `WidgetStateMapper<T>`: the first arm whose constraint the states
/// satisfy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetStateMapper<T> {
    map: WidgetStateMap<T>,
}

impl<T: Clone> WidgetStateMapper<T> {
    /// Upstream `WidgetStateProperty.fromMap` / `WidgetStateMapper(map)`.
    pub fn new(map: WidgetStateMap<T>) -> WidgetStateMapper<T> {
        WidgetStateMapper { map }
    }

    /// Upstream `resolve`, for the nullable case: no arm matched, no value.
    ///
    /// The non-nullable case upstream throws in is the caller's to rule out,
    /// by ending the map with [`WidgetStatesConstraint::ANY`]; the typed
    /// properties below do exactly that.
    pub fn resolve_optional(&self, states: WidgetStates) -> Option<T> {
        self.map
            .iter()
            .find(|(constraint, _)| constraint.is_satisfied_by(states))
            .map(|(_, value)| value.clone())
    }

    pub fn arms(&self) -> &WidgetStateMap<T> {
        &self.map
    }
}

impl<T: Clone> WidgetStateProperty<Option<T>> for WidgetStateMapper<T> {
    fn resolve(&self, states: WidgetStates) -> Option<T> {
        self.resolve_optional(states)
    }
}

/// Upstream `WidgetStateProperty.lerp`: two properties interpolated, resolved
/// against the same states and then blended.
///
/// Upstream's `_LerpProperties` keeps the two properties and the `t`, and
/// resolves both on every `resolve`; this is that, as a closure over them.
pub fn lerp_properties<T: Clone + 'static>(
    a: Option<Rc<dyn WidgetStateProperty<T>>>,
    b: Option<Rc<dyn WidgetStateProperty<T>>>,
    t: f32,
    lerp: Rc<dyn Fn(Option<T>, Option<T>, f32) -> Option<T>>,
) -> Option<WidgetStatePropertyWith<Option<T>>> {
    if a.is_none() && b.is_none() {
        // Upstream avoids building a `_LerpProperties` for this case.
        return None;
    }
    Some(WidgetStatePropertyWith::new(Rc::new(move |states| {
        let resolved_a = a.as_ref().map(|property| property.resolve(states));
        let resolved_b = b.as_ref().map(|property| property.resolve(states));
        lerp(resolved_a, resolved_b, t)
    })))
}

// -- The typed properties -----------------------------------------------------
//
// Upstream each of these is the value type *and* a property, so a theme can
// hold one where it declares a plain `Color`. That is inheritance, which
// there is not; each is its own type here, and the fallback arm every one of
// them carries is what upstream's non-nullable `resolve` would have thrown
// without.

/// Upstream `WidgetStateColor`.
pub struct WidgetStateColor {
    resolver: WidgetPropertyResolver<Color>,
}

impl WidgetStateColor {
    /// Upstream `WidgetStateColor.resolveWith`.
    pub fn resolve_with(resolver: WidgetPropertyResolver<Color>) -> WidgetStateColor {
        WidgetStateColor { resolver }
    }

    /// Upstream `WidgetStateColor.fromMap`, with the fallback the map's
    /// missing `any` arm would otherwise have thrown for.
    pub fn from_map(map: WidgetStateMap<Color>, fallback: Color) -> WidgetStateColor {
        let mapper = WidgetStateMapper::new(map);
        WidgetStateColor::resolve_with(Rc::new(move |states| {
            mapper.resolve_optional(states).unwrap_or(fallback)
        }))
    }

    /// Upstream `WidgetStateColor.transparent`.
    pub fn transparent() -> WidgetStateColor {
        WidgetStateColor::resolve_with(Rc::new(|_| Color::TRANSPARENT))
    }
}

impl WidgetStateProperty<Color> for WidgetStateColor {
    fn resolve(&self, states: WidgetStates) -> Color {
        (self.resolver)(states)
    }
}

/// Upstream `WidgetStateMouseCursor`.
pub struct WidgetStateMouseCursor {
    resolver: WidgetPropertyResolver<SystemMouseCursor>,
}

impl WidgetStateMouseCursor {
    pub fn resolve_with(
        resolver: WidgetPropertyResolver<SystemMouseCursor>,
    ) -> WidgetStateMouseCursor {
        WidgetStateMouseCursor { resolver }
    }

    pub fn from_map(
        map: WidgetStateMap<SystemMouseCursor>,
        fallback: SystemMouseCursor,
    ) -> WidgetStateMouseCursor {
        let mapper = WidgetStateMapper::new(map);
        WidgetStateMouseCursor::resolve_with(Rc::new(move |states| {
            mapper.resolve_optional(states).unwrap_or(fallback)
        }))
    }

    /// Upstream `WidgetStateMouseCursor.clickable`: the hand, and the
    /// forbidden sign when it cannot be clicked.
    pub fn clickable() -> WidgetStateMouseCursor {
        WidgetStateMouseCursor::resolve_with(Rc::new(|states| {
            if states.contains(WidgetState::Disabled) {
                SystemMouseCursor::Basic
            } else {
                SystemMouseCursor::Click
            }
        }))
    }

    /// Upstream `WidgetStateMouseCursor.textable`.
    pub fn textable() -> WidgetStateMouseCursor {
        WidgetStateMouseCursor::resolve_with(Rc::new(|states| {
            if states.contains(WidgetState::Disabled) {
                SystemMouseCursor::Basic
            } else {
                SystemMouseCursor::Text
            }
        }))
    }
}

impl WidgetStateProperty<SystemMouseCursor> for WidgetStateMouseCursor {
    fn resolve(&self, states: WidgetStates) -> SystemMouseCursor {
        (self.resolver)(states)
    }
}

/// Upstream `WidgetStateBorderSide`.
pub struct WidgetStateBorderSide {
    resolver: WidgetPropertyResolver<Option<BorderSide>>,
}

impl WidgetStateBorderSide {
    pub fn resolve_with(
        resolver: WidgetPropertyResolver<Option<BorderSide>>,
    ) -> WidgetStateBorderSide {
        WidgetStateBorderSide { resolver }
    }

    /// Upstream `WidgetStateBorderSide.fromMap`, which is nullable and so
    /// needs no fallback: no matching arm means no side.
    pub fn from_map(map: WidgetStateMap<BorderSide>) -> WidgetStateBorderSide {
        let mapper = WidgetStateMapper::new(map);
        WidgetStateBorderSide::resolve_with(Rc::new(move |states| mapper.resolve_optional(states)))
    }

    /// Upstream `WidgetStateBorderSide.lerp`, whose `_LerpSides` gives a
    /// missing side the other's colour at zero alpha and zero width, so that
    /// a border appearing fades in rather than snapping.
    pub fn lerp(
        a: Option<Rc<WidgetStateBorderSide>>,
        b: Option<Rc<WidgetStateBorderSide>>,
        t: f32,
    ) -> Option<WidgetStateBorderSide> {
        if a.is_none() && b.is_none() {
            return None;
        }
        Some(WidgetStateBorderSide::resolve_with(Rc::new(
            move |states| {
                let resolved_a = a.as_ref().and_then(|side| side.resolve(states));
                let resolved_b = b.as_ref().and_then(|side| side.resolve(states));
                match (resolved_a, resolved_b) {
                    (None, None) => None,
                    (None, Some(b)) => Some(BorderSide::lerp(vanishing(&b), b, t)),
                    (Some(a), None) => Some(BorderSide::lerp(a, vanishing(&a), t)),
                    (Some(a), Some(b)) => Some(BorderSide::lerp(a, b, t)),
                }
            },
        )))
    }
}

/// The side upstream's `_LerpSides` blends a missing one from: no width, and
/// the present side's colour at zero alpha.
fn vanishing(side: &BorderSide) -> BorderSide {
    BorderSide {
        width: 0.0,
        color: side.color.with_alpha(0),
        ..*side
    }
}

impl WidgetStateProperty<Option<BorderSide>> for WidgetStateBorderSide {
    fn resolve(&self, states: WidgetStates) -> Option<BorderSide> {
        (self.resolver)(states)
    }
}

/// Upstream `WidgetStateOutlinedBorder`, whose value is a shape.
pub struct WidgetStateOutlinedBorder {
    resolver: WidgetPropertyResolver<Option<ShapeBorder>>,
}

impl WidgetStateOutlinedBorder {
    pub fn resolve_with(
        resolver: WidgetPropertyResolver<Option<ShapeBorder>>,
    ) -> WidgetStateOutlinedBorder {
        WidgetStateOutlinedBorder { resolver }
    }

    pub fn from_map(map: WidgetStateMap<ShapeBorder>) -> WidgetStateOutlinedBorder {
        let mapper = WidgetStateMapper::new(map);
        WidgetStateOutlinedBorder::resolve_with(Rc::new(move |states| {
            mapper.resolve_optional(states)
        }))
    }
}

impl WidgetStateProperty<Option<ShapeBorder>> for WidgetStateOutlinedBorder {
    fn resolve(&self, states: WidgetStates) -> Option<ShapeBorder> {
        (self.resolver)(states)
    }
}

/// Upstream `WidgetStateTextStyle`.
pub struct WidgetStateTextStyle {
    resolver: WidgetPropertyResolver<TextStyle>,
}

impl WidgetStateTextStyle {
    pub fn resolve_with(resolver: WidgetPropertyResolver<TextStyle>) -> WidgetStateTextStyle {
        WidgetStateTextStyle { resolver }
    }

    pub fn from_map(map: WidgetStateMap<TextStyle>, fallback: TextStyle) -> WidgetStateTextStyle {
        let mapper = WidgetStateMapper::new(map);
        WidgetStateTextStyle::resolve_with(Rc::new(move |states| {
            mapper
                .resolve_optional(states)
                .unwrap_or_else(|| fallback.clone())
        }))
    }
}

impl WidgetStateProperty<TextStyle> for WidgetStateTextStyle {
    fn resolve(&self, states: WidgetStates) -> TextStyle {
        (self.resolver)(states)
    }
}

/// Upstream `WidgetStatesController`: the set of states, held where several
/// widgets can watch it.
///
/// Upstream it extends `ValueNotifier<Set<WidgetState>>`; here it holds one,
/// since Rust has no inheritance and the notifier's whole surface is two
/// methods.
pub struct WidgetStatesController {
    notifier: ValueNotifier<WidgetStates>,
}

impl WidgetStatesController {
    pub fn new(states: WidgetStates) -> WidgetStatesController {
        WidgetStatesController {
            notifier: ValueNotifier::new(states),
        }
    }

    pub fn value(&self) -> WidgetStates {
        self.notifier.value()
    }

    /// Upstream `update`: adds or removes, and tells the listeners only if
    /// the set actually changed.
    pub fn update(&self, state: WidgetState, add: bool) {
        let mut states = self.notifier.value();
        if states.update(state, add) {
            self.notifier.set_value(states);
        }
    }

    /// The whole set at once, which upstream reaches through `value =`.
    pub fn set_value(&self, states: WidgetStates) {
        self.notifier.set_value(states);
    }

    pub fn notifier(&self) -> &ValueNotifier<WidgetStates> {
        &self.notifier
    }
}

impl Default for WidgetStatesController {
    fn default() -> WidgetStatesController {
        WidgetStatesController::new(WidgetStates::NONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const HOVERED: WidgetStates = WidgetStates::NONE.with(WidgetState::Hovered);
    const DISABLED: WidgetStates = WidgetStates::NONE.with(WidgetState::Disabled);

    #[test]
    fn a_state_set_is_a_set() {
        let mut states = WidgetStates::of(&[WidgetState::Hovered, WidgetState::Pressed]);
        assert!(states.contains(WidgetState::Hovered));
        assert!(!states.contains(WidgetState::Disabled));
        assert!(!states.update(WidgetState::Hovered, true), "already there");
        assert!(states.update(WidgetState::Hovered, false));
        assert!(!states.contains(WidgetState::Hovered));
        assert_eq!(
            states.iter().collect::<Vec<_>>(),
            vec![WidgetState::Pressed]
        );
        assert!(WidgetStates::NONE.is_empty());
    }

    #[test]
    fn the_constraint_operators_read_the_set() {
        let hovered = WidgetStatesConstraint::state(WidgetState::Hovered);
        let disabled = WidgetStatesConstraint::state(WidgetState::Disabled);

        assert!(hovered.is_satisfied_by(HOVERED));
        assert!(!hovered.is_satisfied_by(DISABLED));
        assert!(WidgetStatesConstraint::ANY.is_satisfied_by(WidgetStates::NONE));

        // `hovered & ~disabled`, which is what a hover highlight really means.
        let live_hover = hovered.clone().and(disabled.clone().not());
        assert!(live_hover.is_satisfied_by(HOVERED));
        assert!(!live_hover.is_satisfied_by(HOVERED.with(WidgetState::Disabled)));

        let either = hovered.or(disabled);
        assert!(either.is_satisfied_by(DISABLED));
        assert!(!either.is_satisfied_by(WidgetStates::of(&[WidgetState::Focused])));
    }

    #[test]
    fn a_mapper_answers_with_the_first_arm_that_matches() {
        let mapper = WidgetStateMapper::new(vec![
            (WidgetStatesConstraint::state(WidgetState::Disabled), 1),
            (WidgetStatesConstraint::state(WidgetState::Hovered), 2),
            (WidgetStatesConstraint::ANY, 3),
        ]);
        // Both arms match; the first one wins, which is why the disabled arm
        // is written above the hover arm in every theme upstream ships.
        assert_eq!(
            mapper.resolve_optional(HOVERED.with(WidgetState::Disabled)),
            Some(1)
        );
        assert_eq!(mapper.resolve_optional(HOVERED), Some(2));
        assert_eq!(mapper.resolve_optional(WidgetStates::NONE), Some(3));

        // Without an `any` arm, an unmatched set has no answer -- the case
        // upstream throws for when the type is not nullable.
        let partial = WidgetStateMapper::new(vec![(
            WidgetStatesConstraint::state(WidgetState::Disabled),
            1,
        )]);
        assert_eq!(partial.resolve_optional(HOVERED), None);
    }

    #[test]
    fn a_property_that_is_all_one_value_ignores_the_states() {
        let property = WidgetStatePropertyAll(7);
        assert_eq!(property.resolve(HOVERED), 7);
        assert_eq!(property.resolve(WidgetStates::NONE), 7);
    }

    #[test]
    fn resolve_with_is_the_callback_form() {
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        let property = WidgetStatePropertyWith::new(Rc::new(move |states: WidgetStates| {
            counter.set(counter.get() + 1);
            states.contains(WidgetState::Pressed)
        }));
        assert!(property.resolve(WidgetStates::of(&[WidgetState::Pressed])));
        assert!(!property.resolve(HOVERED));
        assert_eq!(calls.get(), 2, "resolved on every ask, as upstream's is");
    }

    #[test]
    fn a_stateful_slot_resolves_and_a_plain_one_does_not() {
        let plain: MaybeStateful<i32> = MaybeStateful::Plain(4);
        assert_eq!(plain.resolve(HOVERED), 4);

        let stateful: MaybeStateful<i32> = MaybeStateful::Stateful(Rc::new(
            WidgetStatePropertyWith::new(Rc::new(|states: WidgetStates| {
                if states.contains(WidgetState::Hovered) {
                    9
                } else {
                    4
                }
            })),
        ));
        assert_eq!(stateful.resolve(HOVERED), 9);
        assert_eq!(stateful.resolve(WidgetStates::NONE), 4);
    }

    #[test]
    fn a_state_colour_falls_back_where_upstream_would_have_thrown() {
        let colour = WidgetStateColor::from_map(
            vec![(
                WidgetStatesConstraint::state(WidgetState::Disabled),
                Color::argb(255, 128, 128, 128),
            )],
            Color::argb(255, 0, 0, 255),
        );
        assert_eq!(colour.resolve(DISABLED), Color::argb(255, 128, 128, 128));
        assert_eq!(colour.resolve(HOVERED), Color::argb(255, 0, 0, 255));
        assert_eq!(
            WidgetStateColor::transparent().resolve(HOVERED),
            Color::TRANSPARENT
        );
    }

    #[test]
    fn a_clickable_cursor_gives_up_when_the_control_is_off() {
        let cursor = WidgetStateMouseCursor::clickable();
        assert_eq!(cursor.resolve(HOVERED), SystemMouseCursor::Click);
        assert_eq!(cursor.resolve(DISABLED), SystemMouseCursor::Basic);
        assert_eq!(
            WidgetStateMouseCursor::textable().resolve(HOVERED),
            SystemMouseCursor::Text
        );
    }

    #[test]
    fn a_side_that_appears_fades_in_rather_than_snapping() {
        let solid = Rc::new(WidgetStateBorderSide::from_map(vec![(
            WidgetStatesConstraint::ANY,
            BorderSide {
                color: Color::argb(255, 255, 0, 0),
                width: 4.0,
                ..BorderSide::NONE
            },
        )]));
        // Upstream's `_LerpSides`: the missing end is the present side at
        // zero width and zero alpha, so halfway is half the width and half
        // the alpha rather than nothing at all.
        let halfway = WidgetStateBorderSide::lerp(None, Some(Rc::clone(&solid)), 0.5)
            .expect("one end is enough")
            .resolve(WidgetStates::NONE)
            .expect("a side on the present end");
        assert_eq!(halfway.width, 2.0);
        assert_eq!(halfway.color.alpha(), 128);

        assert!(WidgetStateBorderSide::lerp(None, None, 0.5).is_none());
    }

    #[test]
    fn a_states_controller_tells_its_listeners_only_when_the_set_moved() {
        use crate::foundation::Listenable;

        let controller = WidgetStatesController::default();
        let heard = Rc::new(Cell::new(0));
        let counter = Rc::clone(&heard);
        controller
            .notifier()
            .add_listener(Rc::new(move || counter.set(counter.get() + 1)));

        controller.update(WidgetState::Hovered, true);
        assert_eq!(heard.get(), 1);
        assert!(controller.value().contains(WidgetState::Hovered));

        // Already hovered: nothing changed, nobody told.
        controller.update(WidgetState::Hovered, true);
        assert_eq!(heard.get(), 1);

        controller.update(WidgetState::Hovered, false);
        assert_eq!(heard.get(), 2);
        assert!(controller.value().is_empty());
    }
}

#[cfg(test)]
mod state_bit_tests {
    use super::{WidgetState, WidgetStates, WidgetStatesConstraint};

    #[test]
    fn no_two_states_share_a_bit() {
        // A variant sweep found three arms of `WidgetState::bit` could answer
        // as the arm above them with nothing noticing -- Selected taking
        // Dragged's bit, ScrolledUnder taking Selected's, Disabled taking
        // ScrolledUnder's. Two states on one bit means a set cannot tell them
        // apart: asking for one would answer yes because the other is there.
        //
        // Asserted as a property over every pair rather than as eight separate
        // values, so a ninth state added later is covered without anybody
        // remembering to extend this.
        for held in WidgetState::ALL {
            let only = WidgetStates::NONE.with(held);
            assert!(only.contains(held), "{held:?}");
            for other in WidgetState::ALL {
                if other != held {
                    assert!(
                        !only.contains(other),
                        "a set holding only {held:?} answers yes to {other:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn and_eight_states_fit_in_eight_bits_without_overlapping() {
        // The other half of the same rule: every state can be held at once,
        // and holding all of them is not the same as holding any subset.
        let all = WidgetState::ALL
            .into_iter()
            .fold(WidgetStates::NONE, |states, state| states.with(state));
        for state in WidgetState::ALL {
            assert!(all.contains(state), "{state:?}");
            // And removing one removes exactly one.
            let missing = all.without(state);
            assert!(!missing.contains(state));
            for other in WidgetState::ALL {
                if other != state {
                    assert!(missing.contains(other), "{state:?} took {other:?}");
                }
            }
        }
        assert!(!all.is_empty());
        assert!(WidgetStates::NONE.is_empty());
    }

    #[test]
    fn a_constraint_reads_the_same_set_the_same_way() {
        // Through the public route, since that is what a theme resolution
        // actually calls.
        for held in WidgetState::ALL {
            let only = WidgetStates::of(&[held]);
            assert!(WidgetStatesConstraint::state(held).is_satisfied_by(only));
            for other in WidgetState::ALL {
                if other != held {
                    assert!(
                        !WidgetStatesConstraint::state(other).is_satisfied_by(only),
                        "{held:?} satisfied a constraint on {other:?}"
                    );
                }
            }
        }
    }
}
