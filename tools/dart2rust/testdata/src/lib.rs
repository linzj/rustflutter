//! Stubs for the types the translated code calls into, plus the tests.
//!
//! `unused_mut` is an error here, deliberately. Whether the compiler marks
//! only the reassigned locals `mut` is otherwise invisible: marking every
//! local `mut` compiles and passes every test, and the mutation sweep found
//! exactly that. Denying the warning turns a claim about precision into
//! something the build checks.
#![deny(unused_mut)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    x: f32,
    y: f32,
}
impl Offset {
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { x: dx, y: dy }
    }
    // Getters, as upstream has them. They were fields here while the compiler
    // could not tell a Dart getter from a Dart field; it can now.
    pub const fn dx(&self) -> f32 {
        self.x
    }
    pub const fn dy(&self) -> f32 {
        self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    w: f32,
    h: f32,
}
impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self {
            w: width,
            h: height,
        }
    }
    pub const fn width(&self) -> f32 {
        self.w
    }
    pub const fn height(&self) -> f32 {
        self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    // In dart:ui these four really are fields; `width` and `height` are
    // getters. The stub draws the line where upstream draws it -- checked
    // against dart:ui rather than guessed from how the compiler happened to
    // read them.
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}
impl Rect {
    pub const fn from_l_t_w_h(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }
    pub const fn from_l_t_r_b(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
    pub const fn width(&self) -> f32 {
        self.right - self.left
    }
    pub const fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// `Object` is Dart's root type. Nothing is translated into it yet; it exists
/// so that a signature mentioning it -- `operator ==(Object other)` -- compiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Object;
impl Object {
    /// `Object.hash` is variadic in Dart and cannot be in Rust. The stub takes
    /// the arity the translated code happens to use; making this general is a
    /// real question for whoever translates `dart:core`, not for a stub.
    #[allow(clippy::too_many_arguments)]
    pub fn hash(
        a: f32,
        b: f32,
        c: f32,
        _d: SentinelValue,
        _e: SentinelValue,
        _f: SentinelValue,
        _g: SentinelValue,
        _h: SentinelValue,
        _i: SentinelValue,
        _j: SentinelValue,
        _k: SentinelValue,
        _l: SentinelValue,
        _m: SentinelValue,
        _n: SentinelValue,
        _o: SentinelValue,
        _p: SentinelValue,
        _q: SentinelValue,
        _r: SentinelValue,
        _s: SentinelValue,
        _t: SentinelValue,
    ) -> i64 {
        (a as i64) ^ (b as i64) ^ (c as i64)
    }
}

/// Upstream's `_SentinelValue`, used as a "not given" marker in copyWith.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SentinelValue;
impl SentinelValue {
    pub const fn new(_id: i64) -> Self {
        Self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

mod alignment;
pub use alignment::{Alignment, AlignmentDirectional, AlignmentGeometry};

mod named_args;
pub use named_args::NamedArgs;

mod asserts;
pub use asserts::Asserts;

/// Stubs for the geometry `EdgeInsets` calls into. Shapes only -- what is being
/// tested is the translation, not whether Radius is right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radius {
    pub x: f32,
    pub y: f32,
}
impl Radius {
    pub const ZERO: Radius = Radius { x: 0.0, y: 0.0 };
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn clamp(&self, minimum: Radius, maximum: Option<Radius>) -> Radius {
        let hi = maximum.unwrap_or(Radius {
            x: f32::MAX,
            y: f32::MAX,
        });
        Radius {
            x: self.x.max(minimum.x).min(hi.x),
            y: self.y.max(minimum.y).min(hi.y),
        }
    }
}
impl std::ops::Add for Radius {
    type Output = Radius;
    fn add(self, other: Radius) -> Radius {
        Radius {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}
impl std::ops::Sub for Radius {
    type Output = Radius;
    fn sub(self, other: Radius) -> Radius {
        Radius {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    tl: Radius,
    tr: Radius,
    br: Radius,
    bl: Radius,
}
impl RRect {
    // dart:ui's RRect stores `tlRadiusX`/`tlRadiusY` and exposes `tlRadius` as
    // a getter, so these are methods.
    pub const fn tl_radius(&self) -> Radius {
        self.tl
    }
    pub const fn tr_radius(&self) -> Radius {
        self.tr
    }
    pub const fn br_radius(&self) -> Radius {
        self.br
    }
    pub const fn bl_radius(&self) -> Radius {
        self.bl
    }
}
impl RRect {
    #[allow(clippy::too_many_arguments)]
    pub const fn from_l_t_r_b_and_corners(
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        tl_radius: Radius,
        tr_radius: Radius,
        br_radius: Radius,
        bl_radius: Radius,
    ) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
            tl: tl_radius,
            tr: tr_radius,
            br: br_radius,
            bl: bl_radius,
        }
    }
}

/// `EdgeInsets.fromViewPadding` takes one of these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPadding {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

mod edge_insets;
pub use edge_insets::EdgeInsets;

mod supercalls;
pub use supercalls::{Doubled, Shape, Untouched};

mod assignment;
pub use assignment::Assignment;

mod nullcheck;
pub use nullcheck::NullCheck;

mod mutation;
pub use mutation::Counter;

mod setters;
pub use setters::Temperature;

mod enums;
pub use enums::{Axis, Layout, MainAxisAlignment};

mod toplevel;
pub use toplevel::{K_DERIVED, K_MAX_ITEMS, K_SPACING, K_VERBOSE};

mod nulltest;
pub use nulltest::Maybe;

mod ifnull;
pub use ifnull::IfNull;

mod nullaware;
pub use nullaware::{Branch, Leaf};

mod superctor;
pub use superctor::{Padded, Rectangle, Square};

mod closures;
pub use closures::Closures;

mod cascade;
pub use cascade::{Paint, Painter, Tinted};

/// dart:core's RangeError, as far as the fixture needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeError {
    pub message: String,
}
impl RangeError {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

mod failure;
pub use failure::Bounds;

mod building;
pub use building::{Shade, Slot};

mod pieces;
pub use pieces::Label;

mod branching;
pub use branching::{Corner, Placement};

mod control;
pub use control::Sieve;

mod loops;
pub use loops::{Ladder, Rung};

mod constinstance;
pub use constinstance::{Inset, Spacing, Span};

mod trycatch;
pub use trycatch::{Guarded, Tally};
// The base trait, renamed on import:  has a  of its own.
pub use superctor::Shape as GeometricShape;

#[cfg(test)]
mod tests {
    use super::*;

    // Every expected value below is what Dart produces for the same call, read
    // off upstream's own definitions -- not off the generated Rust. A test that
    // asserts the output equals itself would pass on any translation.

    #[test]
    fn constants_carry_their_coordinates() {
        assert_eq!(Alignment::CENTER, Alignment::new(0.0, 0.0));
        assert_eq!(Alignment::TOP_LEFT, Alignment::new(-1.0, -1.0));
        assert_eq!(Alignment::BOTTOM_RIGHT, Alignment::new(1.0, 1.0));
    }

    #[test]
    fn along_size_maps_the_corners_of_the_box() {
        let box_ = Size::new(100.0, 100.0);
        assert_eq!(Alignment::TOP_LEFT.along_size(box_), Offset::new(0.0, 0.0));
        assert_eq!(Alignment::CENTER.along_size(box_), Offset::new(50.0, 50.0));
        assert_eq!(
            Alignment::BOTTOM_RIGHT.along_size(box_),
            Offset::new(100.0, 100.0)
        );
    }

    #[test]
    fn the_operators_became_real_traits() {
        assert_eq!(
            Alignment::new(1.0, 1.0) + Alignment::new(2.0, 3.0),
            Alignment::new(3.0, 4.0)
        );
        assert_eq!(
            Alignment::new(5.0, 5.0) - Alignment::new(2.0, 3.0),
            Alignment::new(3.0, 2.0)
        );
        assert_eq!(-Alignment::new(1.0, 2.0), Alignment::new(-1.0, -2.0));
        assert_eq!(Alignment::new(1.0, 2.0) * 3.0, Alignment::new(3.0, 6.0));
        assert_eq!(Alignment::new(6.0, 8.0) / 2.0, Alignment::new(3.0, 4.0));
    }

    #[test]
    fn truncating_division_truncates_like_dart() {
        // Dart's `~/` truncates toward zero, and upstream then calls
        // `.toDouble()`. 5 ~/ 2 == 2, not 2.5 and not 3.
        assert_eq!(
            Alignment::new(5.0, 7.0).int_div(2.0),
            Alignment::new(2.0, 3.0)
        );
        assert_eq!(
            Alignment::new(-5.0, 5.0).int_div(2.0),
            Alignment::new(-2.0, 2.0)
        );
    }

    #[test]
    fn remainder_matches_dart() {
        assert_eq!(Alignment::new(5.0, 7.0) % 2.0, Alignment::new(1.0, 1.0));
    }

    #[test]
    fn inscribe_centres_a_smaller_box() {
        let outer = Rect::from_l_t_w_h(0.0, 0.0, 100.0, 100.0);
        let inner = Size::new(20.0, 20.0);
        let centred = Alignment::CENTER.inscribe(inner, outer);
        assert_eq!((centred.left, centred.top), (40.0, 40.0));
        let corner = Alignment::TOP_LEFT.inscribe(inner, outer);
        assert_eq!((corner.left, corner.top), (0.0, 0.0));
    }

    // -- named arguments ------------------------------------------------------
    //
    // Rust has no named arguments, so the compiler flattens a named call to a
    // positional one. Flattening in call-site order rather than the callee's
    // declaration order works on most calls by luck; the fixture's calls are
    // written so that it does not, and the weights are powers of ten so a
    // wrong permutation cannot reach the right total.

    #[test]
    fn named_arguments_go_to_the_parameters_they_named() {
        let p = NamedArgs::new(1.0, 10.0, 100.0);
        // first=1, second=10, third=100 -> 1*1 + 10*10 + 100*100
        assert_eq!(p.out_of_order(), 10101.0);
        // Call-site order would have been first=100, second=1, third=10,
        // giving 1110.0 -- a number this assert would not accept.
    }

    #[test]
    fn an_omitted_argument_takes_its_declared_default() {
        let p = NamedArgs::new(1.0, 10.0, 100.0);
        // first=1, second=2 (the default), third=1 -> 1 + 20 + 100
        assert_eq!(p.with_omission(), 121.0);
    }

    #[test]
    fn omitting_every_argument_still_passes_every_default() {
        let p = NamedArgs::new(1.0, 10.0, 100.0);
        // 1*1 + 10*2 + 100*4. Emitting `weigh()` here was a real bug: the
        // no-named-arguments shortcut skipped the defaults entirely.
        assert_eq!(p.all_defaults(), 421.0);
    }

    // -- asserts --------------------------------------------------------------
    //
    // Each of these has a partner that trips the check. Without one, "the
    // assert was translated" and "the assert was silently dropped" are the same
    // observation: every test that stays inside the condition passes either way.

    #[test]
    fn a_satisfied_assert_does_not_fire() {
        assert_eq!(Asserts::new(8.0).halved(), 4.0);
        assert_eq!(Asserts::new(3.0).squared(), 9.0);
        assert_eq!(Asserts::new(4.0).doubled(), 8.0);
    }

    #[test]
    #[should_panic(expected = "value must not be negative")]
    fn the_constructors_assert_still_fires() {
        Asserts::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "halving zero is not useful")]
    fn a_body_assert_still_fires_with_its_message() {
        Asserts::new(0.0).halved();
    }

    #[test]
    #[should_panic]
    fn an_assert_whose_message_was_dropped_still_fires() {
        // The message was an interpolation and is not translated; the condition
        // is the contract and it is.
        Asserts::new(5000.0).doubled();
    }

    // -- named constructors ---------------------------------------------------
    //
    // Real upstream EdgeInsets, not a fixture. Every expected value is read off
    // upstream's own definitions in painting/edge_insets.dart.

    #[test]
    fn a_named_constructor_is_an_associated_function() {
        let e = EdgeInsets::from_l_t_r_b(1.0, 2.0, 3.0, 4.0);
        assert_eq!((e.left, e.top, e.right, e.bottom), (1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn all_sets_every_side() {
        let e = EdgeInsets::all(8.0);
        assert_eq!((e.left, e.top, e.right, e.bottom), (8.0, 8.0, 8.0, 8.0));
    }

    #[test]
    fn symmetric_splits_the_two_axes() {
        // `vertical` is top and bottom; `horizontal` is left and right.
        let e = EdgeInsets::symmetric(2.0, 5.0);
        assert_eq!((e.left, e.right), (5.0, 5.0));
        assert_eq!((e.top, e.bottom), (2.0, 2.0));
    }

    #[test]
    fn only_leaves_the_others_at_zero() {
        let e = EdgeInsets::only(40.0, 0.0, 0.0, 0.0);
        assert_eq!((e.left, e.top, e.right, e.bottom), (40.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn zero_is_a_const_built_from_a_named_constructor() {
        // `static const EdgeInsets zero = EdgeInsets.only();` -- a const whose
        // value comes from a named constructor with every argument defaulted.
        // It compiles as an associated const, so the const-ness survived too.
        const Z: EdgeInsets = EdgeInsets::ZERO;
        assert_eq!((Z.left, Z.top, Z.right, Z.bottom), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn copy_with_keeps_what_it_was_not_given() {
        // `left ?? this.left` -- the expression that was emitting `*self.left`.
        let e = EdgeInsets::all(4.0).copy_with(None, Some(9.0), None, None);
        assert_eq!((e.left, e.top, e.right, e.bottom), (4.0, 9.0, 4.0, 4.0));
    }

    #[test]
    fn deflate_rect_pulls_every_edge_inward() {
        let r = Rect::from_l_t_r_b(0.0, 0.0, 100.0, 100.0);
        let d = EdgeInsets::all(10.0).deflate_rect(r);
        assert_eq!((d.left, d.top, d.right, d.bottom), (10.0, 10.0, 90.0, 90.0));
    }

    // -- the class hierarchy --------------------------------------------------
    //
    // `AlignmentGeometry` is abstract upstream, so it becomes a trait, and its
    // two concrete subclasses implement it. These go through `dyn` on purpose:
    // calling `Alignment`'s own inherent method would not exercise the impl at
    // all, and the impl is what this round built.

    #[test]
    fn a_subclass_can_be_used_through_the_base_trait() {
        let a: Box<dyn AlignmentGeometry> = Box::new(Alignment::new(1.0, 2.0));
        let scaled = a.op_mul(3.0);
        // Back down to the concrete type to read the numbers. `resolve` is on
        // the trait and returns Alignment, so it is the way through.
        let got = scaled.resolve(None);
        assert_eq!(got, Alignment::new(3.0, 6.0));
    }

    #[test]
    fn the_trait_dispatches_to_the_right_subclass() {
        // Two different implementors behind the same trait object type.
        let items: Vec<Box<dyn AlignmentGeometry>> = vec![
            Box::new(Alignment::new(1.0, 1.0)),
            Box::new(AlignmentDirectional::new(1.0, 1.0)),
        ];
        // Negation is `impl Neg` on each struct; the trait method boxes it.
        // Alignment resolves to itself; AlignmentDirectional's `resolve` did
        // not translate, so only the first is asked for its numbers.
        let negated = items[0].op_neg();
        assert_eq!(negated.resolve(None), Alignment::new(-1.0, -1.0));
    }

    #[test]
    fn a_covariant_return_is_boxed_at_the_trait_boundary() {
        // Upstream's `Alignment operator -()` overrides one returning
        // `AlignmentGeometry` -- legal in Dart, impossible in Rust. The inherent
        // operator keeps the precise type for Rust callers...
        let precise: Alignment = -Alignment::new(2.0, 3.0);
        assert_eq!(precise, Alignment::new(-2.0, -3.0));
        // ...and the trait method boxes the same value for callers who only
        // know the base.
        let base: &dyn AlignmentGeometry = &Alignment::new(2.0, 3.0);
        assert_eq!(base.op_neg().resolve(None), Alignment::new(-2.0, -3.0));
    }

    #[test]
    fn an_assert_bearing_constructor_is_still_const() {
        // `TextAlignVertical` asserts in its constructor and has `static const`
        // fields built from it. An earlier round dropped `const` from such
        // constructors on a wrong assumption; these fields are the proof it was
        // wrong, because they cannot exist without it.
        assert_eq!(alignment::TextAlignVertical::TOP.y, -1.0);
        assert_eq!(alignment::TextAlignVertical::BOTTOM.y, 1.0);
    }

    // -- super ----------------------------------------------------------------
    //
    // Every call here goes through `dyn Shape`. Calling the inherent method
    // instead would pass whether or not the trait impl carries the override,
    // and whether the override is there is exactly what is being asked.

    #[test]
    fn super_reaches_the_base_body_not_the_override() {
        let d: Box<dyn Shape> = Box::new(Doubled::new());
        // Base says 100*scale; Doubled adds one. If `super.area` came back to
        // the override this would recurse instead of returning 201.
        assert_eq!(d.area(2.0), 201.0);
    }

    #[test]
    fn a_class_that_overrides_nothing_gets_the_default() {
        let u: Box<dyn Shape> = Box::new(Untouched::new());
        assert_eq!(u.area(2.0), 200.0);
        assert_eq!(u.perimeter(), 3.0);
    }

    #[test]
    fn overriding_a_concrete_base_method_is_visible_through_the_trait() {
        // The bug this catches: only abstract members were being put in the
        // impl, so `Doubled`'s override of the concrete `area` was missing and
        // dyn dispatch found the trait default. 200.0, not 201.0.
        let shapes: Vec<Box<dyn Shape>> =
            vec![Box::new(Doubled::new()), Box::new(Untouched::new())];
        let areas: Vec<f32> = shapes.iter().map(|s| s.area(1.0)).collect();
        assert_eq!(areas, vec![101.0, 100.0]);
    }

    // -- assigning a local ----------------------------------------------------
    //
    // Each case is paired with a local that is never reassigned, because
    // marking every local `mut` would compile too and a test that only touched
    // the reassigned ones would pass on that.

    #[test]
    fn a_reassigned_local_accumulates() {
        // total = 0; total += step*2; total += step  ->  3*step
        assert_eq!(Assignment::new(5.0).accumulate(), 15.0);
    }

    #[test]
    fn compound_assignment_means_what_dart_means() {
        // 10 + 5 = 15; 15 - 1 = 14; 14 * 2 = 28. The order matters, and a
        // wrong expansion of `x *= 2` into `x = 2 * x` would still give 28 --
        // so the subtraction sits between them, where order is visible.
        assert_eq!(Assignment::new(5.0).compound(), 28.0);
    }

    #[test]
    fn a_local_assigned_in_one_branch_only() {
        assert_eq!(Assignment::new(1.0).branch(true), 100.0);
        assert_eq!(Assignment::new(1.0).branch(false), 1.0);
    }

    #[test]
    fn a_reassigned_parameter_gets_mut_in_the_signature() {
        // Rust parameters are immutable unless declared `mut`; Dart's are not.
        assert_eq!(Assignment::new(0.0).shadow(41.0), 42.0);
    }

    // -- postfix `!` ----------------------------------------------------------
    //
    // Each case has a partner that is actually null, because "the check is
    // there" and "the check was replaced by a default" look the same on values
    // that are never null.

    #[test]
    fn a_null_check_passes_the_value_through() {
        let n = NullCheck::new(Some(3.0), Some(4.0));
        assert_eq!(n.doubled(), 6.0);
        assert_eq!(n.summed(), 7.0);
        assert_eq!(n.via_call(), 7.0);
    }

    #[test]
    #[should_panic]
    fn a_null_check_on_null_crashes_rather_than_defaulting() {
        // Dart's `!` throws here; `unwrap_or_default()` would have returned 0.0
        // and this test would pass with the wrong answer instead of panicking.
        NullCheck::new(None, Some(1.0)).doubled();
    }

    #[test]
    #[should_panic]
    fn the_second_null_check_is_checked_too() {
        // Only the first operand is non-null, so an implementation that
        // unwrapped just the first would return 3.0 rather than crash.
        NullCheck::new(Some(3.0), None).summed();
    }

    #[test]
    fn a_fallback_and_a_check_are_not_the_same_thing() {
        // `other ?? fallback` supplies a default; `maybe!` insists on a value.
        let n = NullCheck::new(Some(5.0), None);
        assert_eq!(n.with_fallback(10.0), 15.0);
        let m = NullCheck::new(Some(5.0), Some(1.0));
        assert_eq!(m.with_fallback(10.0), 6.0);
    }

    // -- &mut self ------------------------------------------------------------
    //
    // The non-mutating cases are checked on an immutable binding on purpose: if
    // the compiler made every method `&mut self`, those lines would not build.
    // Without them, "only the mutating methods take &mut self" is invisible --
    // marking everything mut compiles and passes.

    #[test]
    fn a_method_that_writes_a_field_takes_mut_self() {
        let mut c = Counter::new(10.0, 5.0);
        c.bump();
        assert_eq!(c.value, 15.0);
        c.scale(2.0);
        assert_eq!(c.value, 30.0);
    }

    #[test]
    fn mut_spreads_one_hop() {
        let mut c = Counter::new(10.0, 5.0);
        c.middle();
        assert_eq!(c.value, 15.0);
    }

    #[test]
    fn mut_spreads_two_hops() {
        // `outer` calls `middle` calls `bump`. One pass over the call graph
        // would have found `middle` and left `outer` as `&self`, which would
        // not compile -- so this is really checked by the build as well.
        let mut c = Counter::new(10.0, 5.0);
        c.outer();
        assert_eq!(c.value, 15.0);
    }

    #[test]
    fn a_reading_method_stays_immutable() {
        // Not `let mut`. If `doubled` or `quiet` took `&mut self` this would
        // fail to compile, which is how their receiver is checked at all.
        let c = Counter::new(10.0, 5.0);
        assert_eq!(c.doubled(), 20.0);
        assert_eq!(c.quiet(), 21.0);
    }

    // -- setters --------------------------------------------------------------
    //
    // `get x` and `set x` are one name in Dart and two in Rust. The reading
    // cases below use an immutable binding, so if the getter had also been
    // marked `&mut self` -- which keying the mutability analysis on the Dart
    // name would do -- they would not compile.

    #[test]
    fn a_setter_is_a_call_not_a_write() {
        let mut t = Temperature::new(20.0);
        t.set_celsius(25.0);
        assert_eq!(t.celsius(), 25.0);
    }

    #[test]
    fn a_setter_with_logic_is_not_an_assignment() {
        // `set fahrenheit` converts; translating it as a field write would have
        // stored 212.0 where 100.0 belongs.
        let mut t = Temperature::new(0.0);
        t.set_fahrenheit(212.0);
        assert_eq!(t.celsius(), 100.0);
    }

    #[test]
    fn assigning_through_your_own_setter_spreads_mut() {
        let mut t = Temperature::new(20.0);
        t.warm_by(5.0);
        assert_eq!(t.celsius(), 25.0);
    }

    #[test]
    fn a_compound_assignment_reads_back_through_the_getter() {
        // 0C is 32F; +18F is 50F, which is 10C. There is no `fahrenheit` field
        // to read, so the current value can only come from the getter.
        let mut t = Temperature::new(0.0);
        t.heat_up();
        assert_eq!(t.celsius(), 10.0);
    }

    #[test]
    fn getters_stay_immutable() {
        // Not `let mut`.
        let t = Temperature::new(100.0);
        assert_eq!(t.fahrenheit(), 212.0);
        assert_eq!(t.difference(), 112.0);
    }

    // -- enums ----------------------------------------------------------------
    //
    // A plain Dart enum is a Rust enum and nothing else. The variants are
    // renamed and nothing more is, so `spaceBetween` becomes `SpaceBetween` --
    // a multi-word value is in the fixture because a single-word one would pass
    // whether or not the renaming did anything.

    #[test]
    fn a_plain_enum_is_a_rust_enum() {
        let l = Layout::new(Axis::Horizontal, MainAxisAlignment::Center);
        assert!(l.is_horizontal());
        assert!(l.is_centred());
        assert!(!l.is_spaced());
    }

    #[test]
    fn a_multi_word_variant_keeps_its_identity() {
        let l = Layout::new(Axis::Vertical, MainAxisAlignment::SpaceBetween);
        assert!(!l.is_horizontal());
        assert!(!l.is_centred());
        assert!(l.is_spaced());
    }

    #[test]
    fn enum_values_are_copied_not_moved() {
        // `Copy` is not decoration: without it, reading `self.axis` twice in a
        // translated body would move out of `&self`.
        let a = Axis::Vertical;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn an_enhanced_enum_is_refused_rather_than_flattened() {
        // `Season` carries a method, so it is a Rust enum *plus* an impl. This
        // reads the generated file because the claim is about what the compiler
        // emitted, and there is no `Season` type to make an assertion against --
        // which is the point. Without this the refusal was untested, and a
        // mutation deleting it survived the whole suite.
        let emitted = include_str!("enums.rs");
        assert!(
            emitted.contains("NOT TRANSLATED: `Season` is an enhanced enum"),
            "expected Season to be refused, got:
{emitted}"
        );
        assert!(
            !emitted.contains("enum Season {"),
            "Season was emitted as a plain enum, dropping its method"
        );
    }

    // -- top-level constants --------------------------------------------------
    //
    // Dart has module-level names and so does Rust, so these need no owner on
    // either side. Analyzer models a top-level `const` as a *synthetic* getter,
    // which is how a stored constant is told from a computed `get foo => ...`
    // -- the same distinction that separates a field from a real getter.

    #[test]
    fn top_level_constants_keep_their_values() {
        assert_eq!(K_SPACING, 8.0);
        assert_eq!(K_MAX_ITEMS, 10);
        assert!(!K_VERBOSE);
    }

    #[test]
    fn a_constant_may_be_built_from_another() {
        // `final kDerived = kSpacing * 2.0` is still a module constant.
        assert_eq!(K_DERIVED, 16.0);
    }

    #[test]
    fn a_method_reads_the_right_constant() {
        // Two different constants are read, so one standing in for the other
        // would change an answer rather than pass unnoticed.
        let l = toplevel::Layout::new(4);
        assert_eq!(l.total_spacing(), 24.0);
        assert!(!l.is_full());
        assert!(toplevel::Layout::new(10).is_full());
    }

    #[test]
    fn a_computed_getter_is_not_a_constant() {
        // `get computed => ...` is a function, not a stored value, so the
        // method reading it is refused rather than emitted against a constant
        // that was never written.
        let emitted = include_str!("toplevel.rs");
        assert!(
            emitted.contains("NOT TRANSLATED: Layout: unsupported top-level getter"),
            "expected usesComputed to be refused, got:
{emitted}"
        );
        assert!(!emitted.contains("const COMPUTED"));
    }

    // -- `x == null` ----------------------------------------------------------
    //
    // Rust asks this differently: a nullable value is an Option and the test is
    // `is_none()`. Every case is paired with its opposite, since a test that
    // only passes non-null values cannot tell `is_none` from `is_some`.

    #[test]
    fn a_null_test_answers_both_ways() {
        assert!(Maybe::new(None, None).is_missing());
        assert!(!Maybe::new(Some(1.0), None).is_missing());
    }

    #[test]
    fn not_equal_null_is_the_opposite() {
        assert!(Maybe::new(Some(1.0), None).is_present());
        assert!(!Maybe::new(None, None).is_present());
    }

    #[test]
    fn null_on_the_left_reads_the_same() {
        assert!(Maybe::new(Some(1.0), None).missing_on_the_left());
        assert!(!Maybe::new(Some(1.0), Some(2.0)).missing_on_the_left());
    }

    #[test]
    fn both_operands_are_tested() {
        assert!(Maybe::new(None, None).both_missing());
        // Only the second is null: an implementation that looked at the first
        // alone would say true here.
        assert!(!Maybe::new(Some(1.0), None).both_missing());
        assert!(!Maybe::new(None, Some(1.0)).both_missing());
    }

    #[test]
    fn a_null_test_guarding_a_null_assertion() {
        // The shape upstream uses most: test, then unwrap.
        assert_eq!(Maybe::new(None, None).resolve(7.0), 7.0);
        assert_eq!(Maybe::new(Some(3.0), None).resolve(7.0), 3.0);
    }

    // -- `a ?? b` -------------------------------------------------------------
    //
    // Dart's `??` is short-circuit. Rust's `unwrap_or(b)` is not, so it is only
    // right when `b` has no effects; `boom()` makes the difference observable
    // by panicking if it is ever called. A fixture whose default was an
    // ordinary number could not tell the two forms apart.

    #[test]
    fn a_literal_default_is_used_when_the_value_is_missing() {
        assert_eq!(IfNull::new(None).with_literal(), 1.0);
        assert_eq!(IfNull::new(Some(5.0)).with_literal(), 5.0);
    }

    #[test]
    fn the_right_side_is_not_evaluated_when_the_left_is_present() {
        // With `unwrap_or(self.boom())` this panics. That is the whole test:
        // 77% of upstream's `??` have a call, a constructor or a throw here.
        assert_eq!(IfNull::new(Some(5.0)).with_call(), 5.0);
    }

    #[test]
    #[should_panic(expected = "the right side of ?? was evaluated")]
    fn the_right_side_is_evaluated_when_the_left_is_missing() {
        // The partner: short-circuiting must not mean "never".
        IfNull::new(None).with_call();
    }

    #[test]
    fn nested_if_nulls_chain() {
        assert_eq!(IfNull::new(Some(1.0)).nested(Some(2.0)), 1.0);
        assert_eq!(IfNull::new(None).nested(Some(2.0)), 2.0);
        assert_eq!(IfNull::new(None).nested(None), 2.0);
    }

    // -- `a?.b` ---------------------------------------------------------------
    //
    // Rust says this with `a.map(|it| ...)`, and the risk is the `??` family
    // again: the body must not run when the receiver is null. `boom()` asserts,
    // so "skipped" and "ran anyway" are different observations rather than the
    // same one.

    #[test]
    fn a_null_aware_read_gives_none_for_a_missing_receiver() {
        assert_eq!(Branch::new(None).leaf_size(), None);
        assert_eq!(Branch::new(Some(Leaf::new(4.0))).leaf_size(), Some(4.0));
    }

    #[test]
    fn a_null_aware_call_runs_only_when_there_is_a_receiver() {
        assert_eq!(Branch::new(None).leaf_doubled(), None);
        assert_eq!(Branch::new(Some(Leaf::new(4.0))).leaf_doubled(), Some(8.0));
    }

    #[test]
    fn the_body_does_not_run_for_a_null_receiver() {
        // With an eager translation this panics.
        assert_eq!(Branch::new(None).leaf_boom(), None);
    }

    #[test]
    #[should_panic(expected = "the body of ?. was evaluated")]
    fn the_body_does_run_when_there_is_a_receiver() {
        // The partner: skipping must not mean "never".
        Branch::new(Some(Leaf::new(1.0))).leaf_boom();
    }

    #[test]
    fn null_aware_beside_if_null() {
        // Two lowerings that look alike in Kernel -- one has null in the then,
        // the other has the temporary in the else -- standing next to each
        // other so a confusion between them shows.
        assert_eq!(Branch::new(None).size_or(9.0), 9.0);
        assert_eq!(Branch::new(Some(Leaf::new(4.0))).size_or(9.0), 4.0);
    }

    // -- `: super(...)` -------------------------------------------------------
    //
    // Rust has no constructor inheritance, so the base's fields live in the
    // subclass's struct and the base's constructor is inlined with its
    // parameters replaced. `Square` passes a computed argument up, so a wrong
    // pairing changes a number rather than shuffling equal ones.

    #[test]
    fn a_subclass_carries_its_bases_fields() {
        let r = Rectangle::new(3.0, 4.0);
        assert_eq!((r.width, r.height), (3.0, 4.0));
        assert_eq!(GeometricShape::area(&r), 12.0);
    }

    #[test]
    fn super_arguments_reach_the_bases_fields() {
        // One argument feeds two base fields, so dropping the substitution
        // would leave one of them unset and fail to compile -- and pairing them
        // wrongly would still be caught by `side` differing from the two.
        let s = Square::new(5.0);
        assert_eq!((s.width, s.height, s.side), (5.0, 5.0, 5.0));
        assert_eq!(GeometricShape::area(&s), 25.0);
    }

    #[test]
    fn a_two_level_chain_flattens() {
        // Padded -> Square -> Shape. Upstream's chains go six deep.
        let p = Padded::new(2.0, 1.0);
        assert_eq!((p.width, p.height, p.side, p.padding), (2.0, 2.0, 2.0, 1.0));
        assert_eq!(GeometricShape::area(&p), 4.0);
        assert_eq!(p.padded_area(), 9.0);
    }

    // -- closures -------------------------------------------------------------
    //
    // Only the ones that capture nothing or read outer locals. `byFactor`
    // reaches `this` and is refused, which matters because Dart lets an
    // instance member be named without writing `this` -- a text search for the
    // word let exactly the wrong ones through.

    #[test]
    fn a_closure_capturing_nothing() {
        // applyTwice adds one twice: 3 -> 5.
        assert_eq!(Closures::new(2.0).doubled(3.0), 5.0);
    }

    #[test]
    fn a_closure_reading_an_outer_local() {
        // v * 3, twice: 2 -> 18.
        assert_eq!(Closures::new(0.0).scaled_by(3.0, 2.0), 18.0);
    }

    #[test]
    fn two_captured_locals_keep_their_places() {
        // v*2 + 10, twice: 1 -> 12 -> 34. Swapping a and b gives 1 -> 12 -> ...
        // a different number, which is why they are not equal in the fixture.
        assert_eq!(Closures::new(0.0).blend(2.0, 10.0, 1.0), 34.0);
    }

    #[test]
    fn a_two_parameter_closure_keeps_its_order() {
        // 10 - 3 = 7. Reversed it is -7, so the two parameters are not
        // interchangeable -- which a one-parameter closure cannot show.
        assert_eq!(Closures::new(0.0).subtracted(), 7.0);
    }

    #[test]
    fn a_closure_reaching_this_is_refused() {
        // `factor` is an instance field named without `this`, so the guard has
        // to resolve the identifier rather than search the text.
        let emitted = include_str!("closures.rs");
        assert!(
            emitted.contains("NOT TRANSLATED") && emitted.contains("capturing `this`"),
            "expected byFactor to be refused, got:
{emitted}"
        );
        assert!(!emitted.contains("fn by_factor"));
    }

    // -- cascades -------------------------------------------------------------
    //
    // `Paint()..width = 2..alpha = 3` is, in Kernel, "bind, do the steps,
    // produce the binding" -- a Rust block expression exactly. The steps set
    // different fields to different values, so a dropped step and a duplicated
    // one give different answers.

    #[test]
    fn a_one_step_cascade_returns_the_receiver() {
        let p = Painter::new().thin();
        assert_eq!(p.width, 1.0);
        // The untouched fields keep their declaration-site values.
        assert_eq!((p.alpha, p.blur), (0.0, 0.0));
    }

    #[test]
    fn every_step_of_a_cascade_runs() {
        let p = Painter::new().styled();
        assert_eq!((p.width, p.alpha, p.blur), (2.0, 3.0, 4.0));
    }

    #[test]
    fn a_cascade_may_call_as_well_as_write() {
        // width = 5, then widen(2) makes it 7. A step that was dropped or run
        // out of order gives 5 or 2.
        let p = Painter::new().widened();
        assert_eq!(p.width, 7.0);
    }

    #[test]
    fn the_constructor_outranks_the_declaration() {
        // Dart applies a declaration value only where the constructor says
        // nothing. Without a field set both ways, the order is untested.
        let t = Tinted::new(0.25);
        assert_eq!(t.opacity, 0.25);
        assert_eq!(t.tint, 0.5);
    }

    #[test]
    fn a_field_initialised_at_its_declaration() {
        // `double width = 0.0;` with a constructor that says nothing about it.
        let p = Paint::new();
        assert_eq!((p.width, p.alpha, p.blur), (0.0, 0.0, 0.0));
    }

    // -- throw -> Result ------------------------------------------------------
    //
    // Failure travels in the return value. Which functions return one is
    // computed, not written: nothing in 's Dart says it can fail. Each
    // case is paired with a success, since a test that only fails cannot tell
    // an  from a panic.

    #[test]
    fn a_throwing_method_returns_err() {
        let b = Bounds::new(10.0);
        assert_eq!(b.checked(5.0), Ok(5.0));
        assert!(b.checked(50.0).is_err());
    }

    #[test]
    fn failure_spreads_one_hop() {
        let b = Bounds::new(10.0);
        assert_eq!(b.doubled(4.0), Ok(8.0));
        assert!(b.doubled(50.0).is_err());
    }

    #[test]
    fn failure_spreads_two_hops() {
        // One pass over the call graph would find  and leave
        //  returning a bare f32 -- which would not compile, the
        // same way it did not for  in round twelve.
        let b = Bounds::new(10.0);
        assert_eq!(b.quadrupled(2.0), Ok(8.0));
        assert!(b.quadrupled(50.0).is_err());
    }

    #[test]
    fn a_method_that_cannot_fail_is_left_alone() {
        // Not : giving every method a Result it does not need would
        // compile too, and this is the line that says it was not done.
        let b = Bounds::new(10.0);
        assert_eq!(b.halved(5.0), 2.5);
    }

    // -- try/catch ------------------------------------------------------------
    //
    // A catch stops the failure, and stopping it is what these check. The
    // signatures carry the claim: `recovered` returns a plain f32 and
    // `uncaught` returns a Result, so a catch that failed to stop anything
    // would not compile rather than quietly returning the wrong thing.

    #[test]
    fn a_catch_stops_the_failure() {
        let g = Guarded::new(10.0);
        assert_eq!(g.recovered(5.0), 5.0);
        // The throw happens and is caught, so this is the handler's value.
        assert_eq!(g.recovered(50.0), -1.0);
    }

    #[test]
    fn an_ignored_stack_trace_costs_nothing() {
        // The clause binds a stack trace it never reads. A Result carries no
        // stack, and ignoring one is free; reading one is refused instead.
        let g = Guarded::new(10.0);
        assert_eq!(g.recovered_with_unused_trace(5.0), 5.0);
        assert_eq!(g.recovered_with_unused_trace(50.0), -2.0);
    }

    #[test]
    fn without_a_catch_the_failure_keeps_travelling() {
        // The pair to the two above: not catching has to give a different
        // signature, or "the catch stopped it" is not being tested at all.
        let g = Guarded::new(10.0);
        assert_eq!(g.uncaught(5.0), Ok(6.0));
        assert!(g.uncaught(50.0).is_err());
    }

    // -- a return inside a try body -------------------------------------------
    //
    // The try body is a closure, so a plain `return` in it would return from
    // the closure and let the method carry on -- compiling, and wrong. These
    // three assertions are what says it did not: a method that carried on
    // would return the value after the try (0.0, or -1.0 from the handler),
    // and every number below is different from those on purpose.

    #[test]
    fn a_return_inside_a_try_returns_from_the_method() {
        let g = Guarded::new(10.0);
        assert_eq!(g.returns_from_inside_try(6.0), 6.0);
    }

    #[test]
    fn a_return_inside_a_try_still_lets_the_catch_catch() {
        // The throw happens inside the same closure the return travels through.
        let g = Guarded::new(10.0);
        assert_eq!(g.returns_from_inside_try(50.0), -3.0);
    }

    #[test]
    fn a_try_body_that_returns_on_only_one_path() {
        // Three outcomes from one body: returned early, fell off the end, and
        // threw. Without the `Ok(None)` case the first two cannot be told
        // apart, which is the whole reason this method is here beside the one
        // above.
        let g = Guarded::new(10.0);
        assert_eq!(g.returns_on_one_path(-2.0), -4.0); // returned early
        assert_eq!(g.returns_on_one_path(7.0), 7.0); // fell off the end
        assert_eq!(g.returns_on_one_path(50.0), -5.0); // threw, was caught
    }

    // -- try/finally ----------------------------------------------------------

    #[test]
    fn the_finalizer_runs_on_every_way_out() {
        // Rust's usual answer is a `Drop` guard. It is the wrong one: a guard's
        // `drop` can neither `?` nor `return`, and the dispatch below does
        // both. The finalizer instead runs between collecting the body's exit
        // and acting on it, which is one place rather than three.
        let mut t = Tally::new(10.0);
        assert_eq!(t.counted(6.0), Ok(6.0)); // fell through to a return
        assert_eq!(t.runs, 1);
        assert_eq!(t.counted(-1.0), Ok(-6.0)); // returned early
        assert_eq!(t.runs, 2);
        assert!(t.counted(50.0).is_err()); // threw, and nothing caught it
        assert_eq!(t.runs, 3);
    }

    // -- const instances the constructor cannot rebuild ------------------------
    //
    // These come from the Kernel front end, which is the only one that meets an
    // evaluated constant. Each class is one of the ways matching the
    // constructor's parameter names against the field names fails, and each
    // holds two constants with different values so that writing one where the
    // other belongs cannot pass.

    #[test]
    fn a_class_with_only_a_named_constructor() {
        assert_eq!(Spacing::TIGHT.amount, 3.0);
        assert_eq!(Spacing::WIDE.amount, 17.0);
        assert_eq!(Spacing::WIDE.twice(), 34.0);
    }

    #[test]
    fn fields_the_base_holds_under_other_names() {
        // `Inset(h, v)` names nothing: the values live on `InsetBase` as `_h`
        // and `_v`. Upstream is `Offset(dx, dy)` storing `_dx`/`_dy`.
        assert_eq!(Inset::SMALL.span(), 12.0);
        assert_eq!(Inset::LARGE.span(), 52.0);
    }

    #[test]
    fn a_field_the_initialiser_works_out() {
        // `end` is `start + length`, and `length` is not a field at all -- so
        // the constant carries a value no parameter names. 2 + 11 and 40 + 60,
        // which no pairing of the two constants' numbers can produce by luck.
        assert_eq!(Span::FIRST.end, 13);
        assert_eq!(Span::FIRST.width(), 11);
        assert_eq!(Span::SECOND.end, 100);
        assert_eq!(Span::SECOND.width(), 60);
    }

    // -- for, while, identical ------------------------------------------------

    #[test]
    fn a_for_loop_and_a_while_loop_agree() {
        // 0+1+2+3+4. Both methods do the same work by different routes, and a
        // `for` that dropped its update or ran its body once too often would
        // make them differ rather than both being wrong the same way.
        let l = Ladder::new(5);
        assert_eq!(l.climbed(), 10.0);
        assert_eq!(l.climbed_the_long_way(), 10.0);
    }

    #[test]
    fn a_for_loop_that_never_runs() {
        // The condition is checked first, not after the first pass.
        let l = Ladder::new(0);
        assert_eq!(l.climbed(), 0.0);
    }

    #[test]
    fn two_declarations_and_an_early_return() {
        // Stops at 4.0 because of the `return`, not at 10.0 where `j` would
        // have stopped it -- so the loop really is running the body, and the
        // return really is leaving the method.
        assert_eq!(Ladder::new(99).paired(), 4.0);
    }

    #[test]
    fn identical_is_not_equality() {
        // Two ladders with the same steps are equal and not identical. If this
        // had been translated as `==` both assertions would read the same way
        // and the second would fail.
        let a = Ladder::new(3);
        let b = Ladder::new(3);
        assert_eq!(a, b);
        assert!(a.is_the(&a));
        assert!(!a.is_the(&b));
    }

    // -- throw as an expression, break, continue -------------------------------

    #[test]
    fn a_throw_where_a_value_was_wanted() {
        // The `??` right side has to leave the *method*. Written as the closure
        // `unwrap_or_else` wants, the `return Err(..)` would have left only the
        // closure -- the same mistake a try body made with `?`.
        let s = Sieve::new(10);
        assert_eq!(s.at_least_one(Some(7)), Ok(7));
        assert!(s.at_least_one(None).is_err());
    }

    #[test]
    fn break_leaves_the_loop() {
        // -2 is what the body sets on every pass that does not break, so a
        // `break` that did not break would give -2 and a `break` that left one
        // level too many would skip the assignment before it.
        let s = Sieve::new(10);
        assert_eq!(s.first_over(3), 4);
        assert_eq!(s.first_over(99), -2); // never over the bound
        assert_eq!(Sieve::new(0).first_over(3), -1); // loop never ran
    }

    #[test]
    fn break_and_continue_in_the_same_loop() {
        // 5 is the first odd number over 4. A `break` that could not cross the
        // body's label would not compile; a `continue` that skipped the update
        // would not stop.
        let s = Sieve::new(10);
        assert_eq!(s.first_odd_over(4), 5);
        assert_eq!(s.first_odd_over(6), 7);
        assert_eq!(s.first_odd_over(99), -1); // never over the bound
    }

    #[test]
    fn continue_skips_the_rest_of_the_body() {
        // 1+3+5+7+9. A `continue` translated as `break` would give 1.
        assert_eq!(Sieve::new(10).odds_below(), 25);
    }

    // -- switch, and the library calls beside it -------------------------------

    #[test]
    fn a_switch_with_no_default_covers_every_case() {
        // Rust checks the exhaustiveness Dart only assumes. The two arms that
        // give 0.0 and the two that give the width are separate arms, so a
        // case wired to the wrong one would show.
        let p = Placement::new(7.0, 13.0);
        assert_eq!(p.offset_x(Corner::TopLeft), 0.0);
        assert_eq!(p.offset_x(Corner::TopRight), 7.0);
        assert_eq!(p.offset_x(Corner::BottomLeft), 0.0);
        assert_eq!(p.offset_x(Corner::BottomRight), 7.0);
    }

    #[test]
    fn two_values_on_one_arm_and_a_default() {
        // The `break` at the end of each case means "leave the switch", which
        // a match arm does by ending -- dropped, not translated. If it had been
        // kept the file would not compile; if the arm had fallen through,
        // TopLeft would give 11.0 instead of 3.0.
        let p = Placement::new(7.0, 13.0);
        assert_eq!(p.depth(Corner::TopLeft), 3.0);
        assert_eq!(p.depth(Corner::TopRight), 3.0);
        assert_eq!(p.depth(Corner::BottomLeft), 11.0);
        assert_eq!(p.depth(Corner::BottomRight), 29.0); // the default
    }

    #[test]
    fn clamp_keeps_its_bounds() {
        let p = Placement::new(7.0, 13.0);
        assert_eq!(p.bounded(3.0), 7.0);
        assert_eq!(p.bounded(9.0), 9.0);
        assert_eq!(p.bounded(99.0), 13.0);
    }

    // -- interpolation, function values, local functions -----------------------

    #[test]
    fn interpolation_becomes_format() {
        let l = Label::new("gauge".to_string(), 3);
        assert_eq!(l.describe(), "gauge has 3");
        // A literal brace has to survive `format!` reading braces.
        assert_eq!(l.braced(), "{gauge}");
    }

    #[test]
    fn a_static_method_used_as_a_value() {
        // Read as a static *field* this came out as `Label::TWICE`, naming a
        // constant nobody declared.
        let l = Label::new("gauge".to_string(), 3);
        assert_eq!(l.doubled(5.0), 10.0);
    }

    #[test]
    fn a_local_function_is_a_closure_in_a_local() {
        // Two steps, so a version that called it once would give 6.0.
        let l = Label::new("gauge".to_string(), 3);
        assert_eq!(l.stepped(5.0), 7.0);
    }

    #[test]
    fn an_assignment_used_for_its_value() {
        // 7.0 doubled is 14.0, plus the 7.0 the assignment left in `total`.
        // Rust's assignment produces `()`, so a translation that forgot to
        // keep the value would not compile; one that read the field back
        // afterwards would give the same answer here and a different one if
        // anything else had written it.
        let l = Label::new("gauge".to_string(), 3);
        assert_eq!(l.running(4.0), 21.0);
    }

    // -- constructor bodies, factories, writes through a field -----------------

    #[test]
    fn a_constructor_body_runs_against_the_value_it_built() {
        // 2.5 * 2 = 5.0. A dropped body would give 1.0 -- the declaration's
        // value -- for every argument, and would compile, which is why it was
        // refused before there was anywhere to put it.
        assert_eq!(Shade::new(2.5).opacity, 5.0);
        assert_eq!(Shade::new(0.5).opacity, 1.0); // not the default by luck
    }

    #[test]
    fn a_factory_is_an_associated_function() {
        // 0.05 doubled by the constructor body it calls.
        assert_eq!(Shade::faint().opacity, 0.1);
    }

    #[test]
    fn a_write_through_a_field_of_this() {
        // `self.tint.opacity`, not `self.opacity`: the receiver has to survive
        // the lowering, and losing it named a field of the wrong object.
        let mut s = Slot::new(Shade::new(1.0));
        assert_eq!(s.tint.opacity, 2.0);
        s.fade(0.25);
        assert_eq!(s.tint.opacity, 0.25);
    }
}
