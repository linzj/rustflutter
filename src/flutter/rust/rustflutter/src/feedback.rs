//! Upstream's `widgets/feedback.dart`.
//!
//! # What this class is
//!
//! Both halves of it were already here -- [`SystemSound`], [`HapticFeedback`],
//! [`SemanticsEvent::Tap`] and [`SemanticsEvent::LongPress`] -- and the
//! coverage ledger said so, handing `Feedback` off to the vibration and sound
//! functions in `services::system`. That reading had it backwards. The sounds
//! and the buzzes are the *materials*; `Feedback` is the only thing that says
//! which of them a gesture is worth on which platform, and that decision lived
//! nowhere.
//!
//! An application that reached past it and vibrated on every long press would
//! buzz a desktop that has no vibrator and stay silent on the iPhone that
//! wanted a click as well.
//!
//! # The two things it does, in order
//!
//! Every entry point sends a **semantics event first and unconditionally**,
//! and only then asks the platform question. That order matters more than it
//! looks: on Linux, macOS and Windows the platform answer is *nothing at all*,
//! so a port that folded the semantics event into the first arm of the switch
//! would leave every desktop screen reader with no announcement of a tap it
//! could otherwise not see. The quiet platforms are exactly the ones that
//! cannot afford to lose it.

use crate::editable_text::TargetPlatform;
use crate::semantics_event::{SemanticsEvent, SemanticsService};
use crate::services::system::{HapticFeedback, HapticFeedbackType, SystemSound, SystemSoundType};

/// Upstream `Feedback`: the platform's own answer to a gesture.
pub struct Feedback;

impl Feedback {
    /// Upstream `Feedback.forTap`.
    ///
    /// Android and Fuchsia play the platform click. Everywhere else, including
    /// iOS, this is a no-op -- iOS does not answer a tap, which is that
    /// platform's convention rather than an omission.
    pub fn for_tap(node_id: i32, platform: TargetPlatform) {
        SemanticsService::send_for_node(node_id, SemanticsEvent::Tap);
        match platform {
            TargetPlatform::Android | TargetPlatform::Fuchsia => {
                SystemSound::play(SystemSoundType::Click);
            }
            TargetPlatform::IOS
            | TargetPlatform::Linux
            | TargetPlatform::MacOS
            | TargetPlatform::Windows => {}
        }
    }

    /// Upstream `Feedback.forLongPress`.
    ///
    /// Where the two platforms part company. Android and Fuchsia buzz --
    /// [`HapticFeedbackType::Standard`], the argument-less `vibrate`, **not**
    /// a heavy impact. iOS does *two* things for the one gesture: the click
    /// sound and a heavy impact together, which upstream's comment records as
    /// observed behaviour on a physical iPhone 15 Pro rather than as a rule
    /// read out of a document.
    ///
    /// So iOS is not the quiet platform. It is silent for a tap and the
    /// loudest of them for a long press, and the two arms are easy to write
    /// the wrong way round because each is plausible on its own.
    pub fn for_long_press(node_id: i32, platform: TargetPlatform) {
        SemanticsService::send_for_node(node_id, SemanticsEvent::LongPress);
        match platform {
            TargetPlatform::Android | TargetPlatform::Fuchsia => {
                HapticFeedback::vibrate(HapticFeedbackType::Standard);
            }
            TargetPlatform::IOS => {
                SystemSound::play(SystemSoundType::Click);
                HapticFeedback::vibrate(HapticFeedbackType::Heavy);
            }
            TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Windows => {}
        }
    }

    /// Upstream `Feedback.wrapForTap`, which is [`Feedback::for_tap`] and then
    /// the callback.
    ///
    /// The order is the point of the wrapper: the feedback goes out **before**
    /// the handler runs, so a handler that takes a while to decide anything
    /// has already told the reader their tap landed. Upstream returns null for
    /// a null callback rather than a closure that only makes noise, and that
    /// is the same distinction as a disabled button being silent.
    pub fn wrap_for_tap<F: FnMut()>(
        callback: Option<F>,
        node_id: i32,
        platform: TargetPlatform,
    ) -> Option<impl FnMut()> {
        let mut callback = callback?;
        Some(move || {
            Feedback::for_tap(node_id, platform);
            callback();
        })
    }

    /// Upstream `Feedback.wrapForLongPress`.
    pub fn wrap_for_long_press<F: FnMut()>(
        callback: Option<F>,
        node_id: i32,
        platform: TargetPlatform,
    ) -> Option<impl FnMut()> {
        let mut callback = callback?;
        Some(move || {
            Feedback::for_long_press(node_id, platform);
            callback();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::codec::{
        JsonMethodCodec, MessageCodec, MethodCodec, StandardMessageCodec, Value,
    };
    use crate::services::tests_support::{Recorder, install};

    /// The channels, in the order the framework wrote to them.
    ///
    /// Two different codecs run on the two channels this touches --
    /// `flutter/accessibility` is standard-encoded and `flutter/platform`
    /// carries JSON method calls -- so nothing here decodes without first
    /// asking which channel it came from.
    fn channels(recorder: &Recorder) -> Vec<String> {
        recorder
            .sent()
            .into_iter()
            .map(|(channel, _, _)| channel)
            .collect()
    }

    /// Every method call on `flutter/platform`, decoded.
    fn calls(recorder: &Recorder) -> Vec<(String, Value)> {
        recorder
            .sent()
            .into_iter()
            .filter(|(channel, _, _)| channel == "flutter/platform")
            .map(|(_, bytes, _)| {
                let call = JsonMethodCodec.decode_method_call(&bytes).unwrap();
                (call.method, call.arguments)
            })
            .collect()
    }

    fn methods(recorder: &Recorder) -> Vec<String> {
        calls(recorder).into_iter().map(|(name, _)| name).collect()
    }

    /// Every semantics event's `type`, in order.
    fn semantics(recorder: &Recorder) -> Vec<String> {
        recorder
            .sent()
            .into_iter()
            .filter(|(channel, _, _)| channel == "flutter/accessibility")
            .filter_map(|(_, bytes, _)| {
                let value = StandardMessageCodec.decode(&bytes).ok()?;
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn a_tap_is_a_sound_on_android_and_nothing_on_the_rest() {
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            let recorder = install();
            Feedback::for_tap(7, platform);
            assert_eq!(methods(&recorder), ["SystemSound.play"], "{platform:?}");
        }
        for platform in [
            TargetPlatform::IOS,
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            let recorder = install();
            Feedback::for_tap(7, platform);
            assert!(methods(&recorder).is_empty(), "{platform:?}");
        }
    }

    #[test]
    fn a_long_press_buzzes_on_android_and_does_two_things_on_ios() {
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            let recorder = install();
            Feedback::for_long_press(7, platform);
            assert_eq!(
                methods(&recorder),
                ["HapticFeedback.vibrate"],
                "{platform:?}"
            );
        }

        // iOS is silent for a tap and the loudest of them here: one gesture,
        // two answers.
        let recorder = install();
        Feedback::for_long_press(7, TargetPlatform::IOS);
        assert_eq!(
            methods(&recorder),
            ["SystemSound.play", "HapticFeedback.vibrate"]
        );

        for platform in [
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            let recorder = install();
            Feedback::for_long_press(7, platform);
            assert!(methods(&recorder).is_empty(), "{platform:?}");
        }
    }

    #[test]
    fn androids_long_press_is_the_plain_buzz_and_not_a_heavy_impact() {
        // The two arms are easy to write the wrong way round: a heavy impact
        // for the heavier gesture reads right and is wrong. Android sends the
        // argument-less `vibrate`; the heavy impact belongs to iOS.
        let recorder = install();
        Feedback::for_long_press(7, TargetPlatform::Android);
        assert_eq!(calls(&recorder).pop().unwrap().1, Value::Null);

        let recorder = install();
        Feedback::for_long_press(7, TargetPlatform::IOS);
        assert_eq!(
            calls(&recorder).pop().unwrap().1,
            Value::from("HapticFeedbackType.heavyImpact")
        );
    }

    #[test]
    fn the_semantics_event_goes_out_on_the_platforms_that_do_nothing_else() {
        // The quiet platforms are exactly the ones that cannot afford to lose
        // it: with no sound and no buzz, the announcement is all a reader who
        // cannot see the screen gets. Folding the send into the first arm of
        // the switch would be invisible on Android and total on Windows.
        for platform in TargetPlatform::ALL {
            let recorder = install();
            Feedback::for_tap(7, platform);
            assert_eq!(semantics(&recorder), ["tap"], "{platform:?}");

            let recorder = install();
            Feedback::for_long_press(7, platform);
            assert_eq!(semantics(&recorder), ["longPress"], "{platform:?}");
        }
    }

    #[test]
    fn and_it_goes_out_first_so_a_slow_handler_has_already_answered() {
        // Upstream sends it before the switch. On Android, where both things
        // happen, that ordering is observable and this is where it is pinned.
        let recorder = install();
        Feedback::for_tap(7, TargetPlatform::Android);
        assert_eq!(
            channels(&recorder),
            ["flutter/accessibility", "flutter/platform"]
        );
    }

    #[test]
    fn the_wrapper_feeds_back_before_it_calls_and_not_after() {
        // Asked from inside the callback, which is the only place the question
        // has two possible answers: by then the feedback has either gone out
        // or it has not. A test that looked afterwards would see both orders
        // as the same.
        let recorder = install();
        let seen = std::cell::RefCell::new(Vec::new());
        {
            let watcher = recorder.clone();
            let mut wrapped = Feedback::wrap_for_tap(
                Some(|| {
                    *seen.borrow_mut() = watcher
                        .sent()
                        .into_iter()
                        .map(|(channel, _, _)| channel)
                        .collect();
                }),
                7,
                TargetPlatform::Android,
            )
            .unwrap();
            wrapped();
        }
        assert_eq!(
            seen.into_inner(),
            ["flutter/accessibility", "flutter/platform"],
            "both had already gone out when the callback ran"
        );
    }

    #[test]
    fn no_callback_is_no_wrapper_rather_than_a_wrapper_that_only_makes_noise() {
        // Upstream returns null. A disabled button should be silent, and a
        // wrapper that fed back and then called nothing would make it noisy.
        assert!(Feedback::wrap_for_tap(None::<fn()>, 7, TargetPlatform::Android).is_none());
        assert!(Feedback::wrap_for_long_press(None::<fn()>, 7, TargetPlatform::Android).is_none());

        // Written with the other half beside it, because "there is no wrapper"
        // is satisfied by never making one. A callback that exists has to come
        // back out, or the absence above says nothing.
        let recorder = install();
        let mut ran = false;
        Feedback::wrap_for_tap(Some(|| ran = true), 7, TargetPlatform::Android)
            .expect("a callback that exists gets a wrapper")();
        assert!(ran);
        assert_eq!(methods(&recorder), ["SystemSound.play"]);
    }
}
