//! Stubs for the types the translated `Alignment` calls into, so the generated
//! file can be compiled on its own. Only the shape matters here: the question
//! this answers is whether dart2rust emits Rust, not whether Offset is right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    pub dx: f32,
    pub dy: f32,
}
impl Offset {
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub const fn from_l_t_w_h(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

mod alignment;
pub use alignment::Alignment;

mod named_args;
pub use named_args::NamedArgs;

mod asserts;
pub use asserts::Asserts;

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
        let box_ = Size {
            width: 100.0,
            height: 100.0,
        };
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
        let inner = Size {
            width: 20.0,
            height: 20.0,
        };
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
}
