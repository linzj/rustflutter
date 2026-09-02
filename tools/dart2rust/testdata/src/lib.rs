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
}
