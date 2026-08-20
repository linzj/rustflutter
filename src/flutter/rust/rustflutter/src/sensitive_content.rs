//! A port of `widgets/sensitive_content.dart`.
//!
//! Marks a subtree the operating system should hide from screen recording and
//! screenshots. Android calls it content sensitivity.
//!
//! Note that upstream does **not export this file**: the class is
//! `@visibleForTesting` with a TODO saying "This is not ready for production"
//! and an open issue about content still being revealed during media
//! projection. It is ported here for the same reason it is written there --
//! the arrangement is finished even though the platform side is not.

/// Upstream `ContentSensitivity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentSensitivity {
    /// Always hidden.
    Sensitive,
    /// The platform decides -- Android looks at whether the content appears to
    /// be a password field and the like.
    AutoSensitive,
    /// Never hidden.
    NotSensitive,
}

/// Upstream's `_ContentSensitivitySetting`: how many widgets asked for each
/// level, and what that adds up to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentSensitivityCounts {
    pub sensitive: i32,
    pub auto_sensitive: i32,
    pub not_sensitive: i32,
}

impl ContentSensitivityCounts {
    pub fn add(&mut self, sensitivity: ContentSensitivity) {
        match sensitivity {
            ContentSensitivity::Sensitive => self.sensitive += 1,
            ContentSensitivity::AutoSensitive => self.auto_sensitive += 1,
            ContentSensitivity::NotSensitive => self.not_sensitive += 1,
        }
    }

    pub fn remove(&mut self, sensitivity: ContentSensitivity) {
        match sensitivity {
            ContentSensitivity::Sensitive => self.sensitive -= 1,
            ContentSensitivity::AutoSensitive => self.auto_sensitive -= 1,
            ContentSensitivity::NotSensitive => self.not_sensitive -= 1,
        }
    }

    /// Upstream reports a `FlutterError` rather than asserting when a count
    /// goes negative, because the message can say what to do: "Please file an
    /// issue." A count below zero means the register and unregister calls got
    /// out of step, which is a framework bug and not a caller's mistake.
    pub fn is_consistent(&self) -> bool {
        self.sensitive >= 0 && self.auto_sensitive >= 0 && self.not_sensitive >= 0
    }

    /// Upstream `contentSensitivityBasedOnWidgetCounts`.
    ///
    /// **A strict priority, not a vote.** One sensitive widget anywhere in the
    /// tree makes the whole window sensitive, whatever else is on screen -- and
    /// that is the only rule that can be right here, because a screen recording
    /// is all-or-nothing per window. There is no way to record half of it, so a
    /// single widget saying "not this" has to outrank every widget saying "this
    /// is fine".
    ///
    /// The counts exist because widgets come and go. The **answer** is a
    /// maximum, never a sum.
    pub fn resolve(&self) -> Option<ContentSensitivity> {
        if self.sensitive > 0 {
            return Some(ContentSensitivity::Sensitive);
        }
        if self.auto_sensitive > 0 {
            return Some(ContentSensitivity::AutoSensitive);
        }
        if self.not_sensitive > 0 {
            return Some(ContentSensitivity::NotSensitive);
        }
        // Nobody is asking, which is different from everybody saying no.
        None
    }
}

/// What the host decided to tell the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitivityOutcome {
    /// The platform cannot do this, so nothing was said.
    Unsupported,
    /// The level did not change, so nothing was said. Upstream compares before
    /// calling: a channel message per widget build would be a message per
    /// frame.
    Unchanged,
    /// Set to this level.
    Set(ContentSensitivity),
    /// Every `SensitiveContent` widget has gone, so the platform is put back
    /// to whatever it had **before Flutter touched it** -- restored, not reset
    /// to a default.
    Restored(ContentSensitivity),
}

/// Upstream `SensitiveContentHost`, the tree-wide singleton the widgets
/// register with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SensitiveContentHost {
    counts: ContentSensitivityCounts,
    /// Asked once and remembered. Upstream caches it in
    /// `_contentSensitivityIsSupported`.
    supported: Option<bool>,
    /// What the platform had before the first widget registered.
    fallback: Option<ContentSensitivity>,
    current: Option<ContentSensitivity>,
}

impl SensitiveContentHost {
    pub fn new() -> SensitiveContentHost {
        SensitiveContentHost::default()
    }

    pub fn calculated_content_sensitivity(&self) -> Option<ContentSensitivity> {
        self.counts.resolve()
    }

    pub fn counts(&self) -> ContentSensitivityCounts {
        self.counts
    }

    pub fn fallback(&self) -> Option<ContentSensitivity> {
        self.fallback
    }

    /// Upstream's support check, which is asked once and cached.
    ///
    /// A `PlatformException` while asking is recorded as **unsupported**, with
    /// the error reported. Failing to find out whether the platform can do
    /// something is not a reason to assume it can: the cost of guessing wrong
    /// that way is content the reader believed was hidden appearing in a
    /// recording.
    pub fn resolve_support(&mut self, answer: Result<bool, ()>) -> bool {
        if let Some(known) = self.supported {
            return known;
        }
        let supported = answer.unwrap_or(false);
        self.supported = Some(supported);
        supported
    }

    /// Upstream `register`.
    ///
    /// The fallback is captured **when the first widget registers**, not at
    /// startup -- so it is whatever the embedding or the developer had set, and
    /// on Android API 35 that is auto-sensitive unless somebody said otherwise.
    pub fn register(
        &mut self,
        desired: ContentSensitivity,
        platform_current: ContentSensitivity,
    ) -> SensitivityOutcome {
        if self.supported != Some(true) {
            return SensitivityOutcome::Unsupported;
        }
        if self.fallback.is_none() {
            self.fallback = Some(platform_current);
        }
        let before = self.counts.resolve().or(self.fallback);
        self.counts.add(desired);
        let after = self.counts.resolve();
        self.apply(before, after)
    }

    /// Upstream `unregister`.
    pub fn unregister(&mut self, desired: ContentSensitivity) -> SensitivityOutcome {
        if self.supported != Some(true) {
            return SensitivityOutcome::Unsupported;
        }
        let before = self.counts.resolve();
        self.counts.remove(desired);
        let after = self.counts.resolve();
        match after {
            Some(_) => self.apply(before, after),
            None => {
                // The last one has gone. Put the platform back where it was.
                let fallback = self.fallback.unwrap_or(ContentSensitivity::NotSensitive);
                self.current = Some(fallback);
                SensitivityOutcome::Restored(fallback)
            }
        }
    }

    fn apply(
        &mut self,
        before: Option<ContentSensitivity>,
        after: Option<ContentSensitivity>,
    ) -> SensitivityOutcome {
        match after {
            Some(level) if Some(level) != before => {
                self.current = Some(level);
                SensitivityOutcome::Set(level)
            }
            _ => SensitivityOutcome::Unchanged,
        }
    }

    pub fn current(&self) -> Option<ContentSensitivity> {
        self.current
    }
}

/// Upstream `SensitiveContent`.
///
/// A marker. It builds its child and nothing else; the whole of it is the
/// registration it performs on the way in and the way out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensitiveContent {
    pub sensitivity: ContentSensitivity,
    pub child: u64,
}

impl SensitiveContent {
    pub fn new(sensitivity: ContentSensitivity, child: u64) -> SensitiveContent {
        SensitiveContent { sensitivity, child }
    }

    /// Upstream's `build`, which is its child.
    pub fn build(&self) -> u64 {
        self.child
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ContentSensitivity::{AutoSensitive, NotSensitive, Sensitive};

    fn host() -> SensitiveContentHost {
        let mut host = SensitiveContentHost::new();
        host.resolve_support(Ok(true));
        host
    }

    #[test]
    fn one_sensitive_widget_anywhere_makes_the_whole_window_sensitive() {
        // The only rule that can be right: a screen recording is
        // all-or-nothing per window, so a single "not this" has to outrank
        // every "this is fine".
        let mut counts = ContentSensitivityCounts::default();
        for _ in 0..20 {
            counts.add(NotSensitive);
        }
        counts.add(AutoSensitive);
        counts.add(Sensitive);
        assert_eq!(
            counts.resolve(),
            Some(Sensitive),
            "outvoted twenty-one to one"
        );
    }

    #[test]
    fn the_answer_is_a_maximum_and_never_a_sum() {
        let mut counts = ContentSensitivityCounts::default();
        counts.add(AutoSensitive);
        counts.add(NotSensitive);
        assert_eq!(counts.resolve(), Some(AutoSensitive));

        counts.remove(AutoSensitive);
        assert_eq!(
            counts.resolve(),
            Some(NotSensitive),
            "and it falls back down"
        );
    }

    #[test]
    fn nobody_asking_is_different_from_everybody_saying_no() {
        let empty = ContentSensitivityCounts::default();
        assert_eq!(empty.resolve(), None);

        let mut declining = ContentSensitivityCounts::default();
        declining.add(NotSensitive);
        assert_eq!(declining.resolve(), Some(NotSensitive));
    }

    #[test]
    fn a_count_below_zero_means_the_framework_lost_track() {
        // Upstream reports an error saying "Please file an issue", because it
        // is a framework bug rather than a caller's mistake.
        let mut counts = ContentSensitivityCounts::default();
        assert!(counts.is_consistent());
        counts.remove(Sensitive);
        assert!(!counts.is_consistent());
    }

    #[test]
    fn failing_to_find_out_whether_the_platform_can_do_this_means_it_cannot() {
        // The cost of guessing the other way is content the reader believed
        // was hidden turning up in a recording.
        let mut unknown = SensitiveContentHost::new();
        assert!(!unknown.resolve_support(Err(())));

        let mut supported = SensitiveContentHost::new();
        assert!(supported.resolve_support(Ok(true)));
    }

    #[test]
    fn the_answer_is_asked_for_once_and_remembered() {
        let mut host = SensitiveContentHost::new();
        assert!(host.resolve_support(Ok(true)));
        assert!(
            host.resolve_support(Ok(false)),
            "the cached answer stands; it is not asked again"
        );
    }

    #[test]
    fn nothing_is_said_to_a_platform_that_cannot_hear_it() {
        let mut unsupported = SensitiveContentHost::new();
        unsupported.resolve_support(Ok(false));
        assert_eq!(
            unsupported.register(Sensitive, NotSensitive),
            SensitivityOutcome::Unsupported
        );
        assert_eq!(
            unsupported.unregister(Sensitive),
            SensitivityOutcome::Unsupported
        );
    }

    #[test]
    fn the_first_registration_sets_the_level_and_the_second_says_nothing() {
        // A channel message per widget build would be a message per frame.
        let mut host = host();
        assert_eq!(
            host.register(Sensitive, NotSensitive),
            SensitivityOutcome::Set(Sensitive)
        );
        assert_eq!(
            host.register(Sensitive, NotSensitive),
            SensitivityOutcome::Unchanged
        );
        assert_eq!(
            host.register(NotSensitive, NotSensitive),
            SensitivityOutcome::Unchanged,
            "and a lesser one changes nothing either"
        );
    }

    #[test]
    fn the_platform_is_put_back_where_it_was_rather_than_reset_to_a_default() {
        // The fallback is whatever the embedding or the developer had set,
        // captured when the first widget registered -- on Android API 35 that
        // is auto-sensitive unless somebody said otherwise.
        let mut host = host();
        host.register(Sensitive, AutoSensitive);
        assert_eq!(host.fallback(), Some(AutoSensitive));

        assert_eq!(
            host.unregister(Sensitive),
            SensitivityOutcome::Restored(AutoSensitive)
        );
        assert_eq!(host.calculated_content_sensitivity(), None);
    }

    #[test]
    fn the_fallback_is_captured_once_and_not_re_read() {
        let mut host = host();
        host.register(Sensitive, AutoSensitive);
        host.register(NotSensitive, NotSensitive);
        assert_eq!(
            host.fallback(),
            Some(AutoSensitive),
            "the second registration did not overwrite it"
        );
    }

    #[test]
    fn removing_one_of_several_leaves_the_level_where_the_rest_put_it() {
        let mut host = host();
        host.register(Sensitive, NotSensitive);
        host.register(AutoSensitive, NotSensitive);

        assert_eq!(
            host.unregister(AutoSensitive),
            SensitivityOutcome::Unchanged
        );
        assert_eq!(host.calculated_content_sensitivity(), Some(Sensitive));

        assert_eq!(
            host.unregister(Sensitive),
            SensitivityOutcome::Restored(NotSensitive)
        );
    }

    #[test]
    fn removing_the_strictest_drops_the_window_to_the_next_one_down() {
        let mut host = host();
        host.register(AutoSensitive, NotSensitive);
        host.register(Sensitive, NotSensitive);
        assert_eq!(host.calculated_content_sensitivity(), Some(Sensitive));

        assert_eq!(
            host.unregister(Sensitive),
            SensitivityOutcome::Set(AutoSensitive)
        );
    }

    #[test]
    fn the_widget_itself_is_only_a_marker() {
        let widget = SensitiveContent::new(Sensitive, 7);
        assert_eq!(widget.build(), 7);
        assert_eq!(widget.sensitivity, Sensitive);
    }
}
