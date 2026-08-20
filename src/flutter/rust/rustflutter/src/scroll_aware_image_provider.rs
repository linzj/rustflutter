//! A port of `widgets/scroll_aware_image_provider.dart`.
//!
//! An image provider that declines to start work while the list it is in is
//! flying past. `Image` wraps every provider it is given in one of these, so
//! the saving is automatic rather than something a caller has to ask for.
//!
//! The whole class is four checks in a particular order, and **the order is the
//! design**.

/// Which of upstream's four steps `resolveStreamForKey` took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// Steps 1 and 2: the stream was already completed, or the cache already
    /// holds this key. The wrapped provider is told anyway.
    AlreadyAvailable,
    /// Step 3: the context left the tree. Nothing happens, and the stream is
    /// **never completed** -- listeners are not notified, because nobody is
    /// waiting any more.
    ContextGone,
    /// Step 4: scrolling too fast. Try again at the end of the next frame,
    /// from the top.
    Deferred,
    /// Step 5: go.
    Resolved,
}

/// What `resolveStreamForKey` is looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveConditions {
    /// Somebody set a completer on the stream.
    pub stream_completed: bool,
    /// The image cache already holds this key -- precached, or resolved by
    /// another provider for the same image.
    pub in_cache: bool,
    /// Whether the `DisposableBuildContext` still has a context.
    pub context_alive: bool,
    /// Upstream `Scrollable.recommendDeferredLoadingForContext`.
    pub scrolling_fast: bool,
}

/// Upstream `ScrollAwareImageProvider`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollAwareImageProvider {
    /// How many times the resolution has been deferred to a later frame.
    deferrals: u32,
}

impl ScrollAwareImageProvider {
    pub fn new() -> ScrollAwareImageProvider {
        ScrollAwareImageProvider::default()
    }

    pub fn deferrals(&self) -> u32 {
        self.deferrals
    }

    /// Upstream `resolveStreamForKey`.
    ///
    /// The first check runs **before** the scrolling check and **before** the
    /// disposed check, and upstream explains both. Telling the wrapped provider
    /// about an image already in the cache updates the cache's LRU information:
    /// *"Even though we never showed the image, it was still touched more
    /// recently."* And doing it before the scrolling check means that **if the
    /// bytes are already there, they are rendered however fast the list is
    /// moving** -- there is no texture memory left to allocate, so there is
    /// nothing to save by waiting.
    ///
    /// Which is the point of the whole class: the deferral is about **work**,
    /// not about display.
    pub fn resolve_stream_for_key(&mut self, conditions: ResolveConditions) -> ResolveOutcome {
        if conditions.stream_completed || conditions.in_cache {
            return ResolveOutcome::AlreadyAvailable;
        }
        if !conditions.context_alive {
            return ResolveOutcome::ContextGone;
        }
        if conditions.scrolling_fast {
            self.deferrals += 1;
            return ResolveOutcome::Deferred;
        }
        ResolveOutcome::Resolved
    }

    /// Whether a deferred attempt starts again from the first check rather than
    /// carrying on from where it stopped.
    ///
    /// It does, and that matters: by the next frame the image may have arrived
    /// in the cache from somewhere else, or the context may have left the tree.
    /// Resuming at step four would miss both.
    pub fn retry_restarts_from_the_top() -> bool {
        true
    }

    /// Drives a deferred resolution forward one frame.
    pub fn next_frame(&mut self, conditions: ResolveConditions) -> ResolveOutcome {
        self.resolve_stream_for_key(conditions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditions() -> ResolveConditions {
        ResolveConditions {
            stream_completed: false,
            in_cache: false,
            context_alive: true,
            scrolling_fast: false,
        }
    }

    #[test]
    fn bytes_that_are_already_there_are_rendered_however_fast_the_list_moves() {
        // The cache check comes before the scrolling check on purpose: there is
        // no texture memory left to allocate, so there is nothing to save by
        // waiting. The deferral is about work, not about display.
        let mut provider = ScrollAwareImageProvider::new();
        let flying_past = ResolveConditions {
            in_cache: true,
            scrolling_fast: true,
            ..conditions()
        };
        assert_eq!(
            provider.resolve_stream_for_key(flying_past),
            ResolveOutcome::AlreadyAvailable
        );
        assert_eq!(provider.deferrals(), 0, "nothing was put off");
    }

    #[test]
    fn a_cached_image_is_reported_even_when_the_context_has_gone() {
        // Which updates the cache LRU: even though it was never shown, it was
        // touched more recently.
        let mut provider = ScrollAwareImageProvider::new();
        let gone_but_cached = ResolveConditions {
            in_cache: true,
            context_alive: false,
            ..conditions()
        };
        assert_eq!(
            provider.resolve_stream_for_key(gone_but_cached),
            ResolveOutcome::AlreadyAvailable
        );
    }

    #[test]
    fn a_stream_somebody_else_completed_is_left_alone() {
        let mut provider = ScrollAwareImageProvider::new();
        let done = ResolveConditions {
            stream_completed: true,
            ..conditions()
        };
        assert_eq!(
            provider.resolve_stream_for_key(done),
            ResolveOutcome::AlreadyAvailable
        );
    }

    #[test]
    fn a_context_that_left_the_tree_ends_the_cycle_without_completing_anything() {
        // Listeners are never notified, because nobody is waiting any more.
        let mut provider = ScrollAwareImageProvider::new();
        let gone = ResolveConditions {
            context_alive: false,
            ..conditions()
        };
        assert_eq!(
            provider.resolve_stream_for_key(gone),
            ResolveOutcome::ContextGone
        );
        assert_eq!(provider.deferrals(), 0);
    }

    #[test]
    fn a_list_flying_past_puts_the_work_off_rather_than_giving_up_on_it() {
        let mut provider = ScrollAwareImageProvider::new();
        let fast = ResolveConditions {
            scrolling_fast: true,
            ..conditions()
        };
        assert_eq!(
            provider.resolve_stream_for_key(fast),
            ResolveOutcome::Deferred
        );
        assert_eq!(provider.deferrals(), 1);

        // Still moving next frame.
        assert_eq!(provider.next_frame(fast), ResolveOutcome::Deferred);
        assert_eq!(provider.deferrals(), 2);

        // And when it settles, the work happens.
        assert_eq!(provider.next_frame(conditions()), ResolveOutcome::Resolved);
        assert_eq!(provider.deferrals(), 2, "no further deferral");
    }

    #[test]
    fn a_retry_starts_again_from_the_first_check() {
        // By the next frame the image may have arrived from somewhere else, or
        // the context may have left the tree. Resuming at the scrolling check
        // would miss both.
        assert!(ScrollAwareImageProvider::retry_restarts_from_the_top());

        let mut arrived = ScrollAwareImageProvider::new();
        let fast = ResolveConditions {
            scrolling_fast: true,
            ..conditions()
        };
        arrived.resolve_stream_for_key(fast);
        assert_eq!(
            arrived.next_frame(ResolveConditions {
                in_cache: true,
                ..fast
            }),
            ResolveOutcome::AlreadyAvailable
        );

        let mut disposed = ScrollAwareImageProvider::new();
        disposed.resolve_stream_for_key(fast);
        assert_eq!(
            disposed.next_frame(ResolveConditions {
                context_alive: false,
                ..fast
            }),
            ResolveOutcome::ContextGone
        );
    }

    #[test]
    fn an_ordinary_image_in_a_still_list_is_loaded_straight_away() {
        let mut provider = ScrollAwareImageProvider::new();
        assert_eq!(
            provider.resolve_stream_for_key(conditions()),
            ResolveOutcome::Resolved
        );
    }
}
