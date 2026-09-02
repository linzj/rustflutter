//! Finding things above you in the tree -- a port of upstream's
//! `inherited_model.dart`, `inherited_notifier.dart`, `inherited_theme.dart`,
//! `lookup_boundary.dart`, and the notification `notification_listener.dart`
//! was missing.
//!
//! A plain `InheritedWidget` offers one bargain: depend on me, and I will
//! rebuild you when I change. The four widgets here each change one term of
//! it.
//!
//! * [`InheritedModel`] narrows *what* counts as a change, so a widget that
//!   asked about one field is not rebuilt when a different one moves.
//! * [`InheritedNotifier`] widens *when* it happens, so a widget is rebuilt
//!   when the value the widget holds notifies -- not only when the widget
//!   itself is replaced.
//! * [`InheritedTheme`] takes a copy of the answer, so a subtree rendered
//!   somewhere else entirely still sees the themes it was built under.
//! * [`LookupBoundary`] stops the search, so a widget cannot reach past a
//!   frame its author drew around it.

use std::collections::HashSet;
use std::hash::Hash;

/// Upstream `InheritedModel`: an inherited widget whose dependents may name
/// which **aspect** of it they care about.
///
/// The pair of methods is the design. `updateShouldNotify` answers "did
/// anything change at all" and is asked once; `updateShouldNotifyDependent` is
/// asked **per dependent**, with the set of aspects that dependent named, and
/// answers "did anything *it* asked about change". A model with ten fields and
/// a hundred dependents therefore rebuilds only the ones whose field moved.
pub trait InheritedModel<Aspect: Eq + Hash + Clone> {
    /// Upstream's `updateShouldNotify`.
    fn update_should_notify(&self, old: &Self) -> bool
    where
        Self: Sized;

    /// Upstream's `updateShouldNotifyDependent`. It is only reached when
    /// `updateShouldNotify` already said yes.
    fn update_should_notify_dependent(&self, old: &Self, dependencies: &HashSet<Aspect>) -> bool
    where
        Self: Sized;

    /// Upstream's `isSupportedAspect`, **true by default**.
    ///
    /// Overriding it is how a model **shadows only part of** an ancestor of
    /// its own type: a model that answers false for an aspect lets the search
    /// carry on upwards for that one aspect while still answering for the
    /// others. A theme that overrides the colours but not the typography is
    /// exactly this.
    fn is_supported_aspect(&self, _aspect: &Aspect) -> bool {
        true
    }
}

/// One model in the chain, for the lookup walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLink<Aspect> {
    pub element: u64,
    /// Which aspects this model answers for. `None` means all of them.
    pub supported: Option<Vec<Aspect>>,
}

impl<Aspect: PartialEq> ModelLink<Aspect> {
    pub fn answering_everything(element: u64) -> ModelLink<Aspect> {
        ModelLink {
            element,
            supported: None,
        }
    }

    pub fn answering(element: u64, supported: Vec<Aspect>) -> ModelLink<Aspect> {
        ModelLink {
            element,
            supported: Some(supported),
        }
    }

    pub fn supports(&self, aspect: &Aspect) -> bool {
        match &self.supported {
            None => true,
            Some(supported) => supported.contains(aspect),
        }
    }
}

/// Upstream's `_findModels`: every model of the type from `context` upwards,
/// **up to and including** the first one that supports the aspect.
///
/// Including the ones that do not support it is the part worth stating. A
/// dependency is created on all of them, because any of them could *start*
/// supporting the aspect on a later build -- and a dependent that had only
/// registered against the far one would never hear about it.
pub fn find_models<Aspect: PartialEq>(chain: &[ModelLink<Aspect>], aspect: &Aspect) -> Vec<u64> {
    let mut found = Vec::new();
    for link in chain {
        found.push(link.element);
        if link.supports(aspect) {
            break;
        }
    }
    found
}

/// Upstream's `InheritedModel.inheritFrom`, as the answer it produces.
///
/// With **no aspect** it degenerates to a plain `dependOnInheritedWidgetOfExactType`
/// -- upstream says so outright -- because a dependent that named no aspect is
/// asking to be rebuilt for any change.
///
/// With an aspect, the value returned comes from the **last** model in the
/// chain: the nearest one that actually answers for it.
pub fn inherit_from<Aspect: PartialEq>(
    chain: &[ModelLink<Aspect>],
    aspect: Option<&Aspect>,
) -> Option<u64> {
    match aspect {
        None => chain.first().map(|link| link.element),
        Some(aspect) => find_models(chain, aspect).last().copied(),
    }
}

/// Upstream `InheritedModelElement`: keeps each dependent's aspect set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InheritedModelElement<Aspect: Eq + Hash + Clone> {
    dependencies: Vec<(u64, HashSet<Aspect>)>,
}

impl<Aspect: Eq + Hash + Clone> InheritedModelElement<Aspect> {
    pub fn new() -> InheritedModelElement<Aspect> {
        InheritedModelElement {
            dependencies: Vec::new(),
        }
    }

    pub fn dependencies_of(&self, dependent: u64) -> Option<&HashSet<Aspect>> {
        self.dependencies
            .iter()
            .find(|(held, _)| *held == dependent)
            .map(|(_, aspects)| aspects)
    }

    /// Upstream's `updateDependencies`, whose first line is the interesting
    /// one:
    ///
    /// ```dart
    /// if (dependencies != null && dependencies.isEmpty) return;
    /// ```
    ///
    /// **An empty set means "everything", and once a dependent has asked for
    /// everything, naming an aspect cannot narrow it back down.** A widget
    /// that called `inheritFrom` with no aspect and later with one is still
    /// asking to hear about every change; the second call must not quietly
    /// take that away.
    pub fn update_dependencies(&mut self, dependent: u64, aspect: Option<Aspect>) {
        if let Some(existing) = self.dependencies_of(dependent) {
            if existing.is_empty() {
                return;
            }
        }
        match aspect {
            None => self.set_dependencies(dependent, HashSet::new()),
            Some(aspect) => {
                let mut aspects = self.dependencies_of(dependent).cloned().unwrap_or_default();
                aspects.insert(aspect);
                self.set_dependencies(dependent, aspects);
            }
        }
    }

    fn set_dependencies(&mut self, dependent: u64, aspects: HashSet<Aspect>) {
        match self
            .dependencies
            .iter_mut()
            .find(|(held, _)| *held == dependent)
        {
            Some((_, existing)) => *existing = aspects,
            None => self.dependencies.push((dependent, aspects)),
        }
    }

    /// Upstream's `notifyDependent`. A dependent with **no** recorded set at
    /// all is not notified; one with an **empty** set always is.
    pub fn notify_dependent(
        &self,
        dependent: u64,
        should_notify: impl Fn(&HashSet<Aspect>) -> bool,
    ) -> bool {
        let Some(dependencies) = self.dependencies_of(dependent) else {
            return false;
        };
        dependencies.is_empty() || should_notify(dependencies)
    }
}

/// Upstream `InheritedNotifier`: an inherited widget whose value is a
/// `Listenable`.
///
/// It changes when the *widget* is replaced **and** when the value it holds
/// says so, which is why an `AnimationController` handed down this way rebuilds
/// its dependents every tick without the widget above them rebuilding at all.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InheritedNotifier {
    /// Upstream's `notifier`, nullable -- and its doc explains the
    /// consequence: "While the notifier is null, no notifications are sent,
    /// since the null object cannot itself send notifications."
    pub notifier: Option<u64>,
    /// Upstream's `_dirty`.
    dirty: bool,
    listening_to: Option<u64>,
    notifications: usize,
}

impl InheritedNotifier {
    pub fn new(notifier: Option<u64>) -> InheritedNotifier {
        InheritedNotifier {
            notifier,
            dirty: false,
            listening_to: notifier,
            notifications: 0,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn listening_to(&self) -> Option<u64> {
        self.listening_to
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's `updateShouldNotify`, which compares the **notifier
    /// identity** rather than anything about its value. A different
    /// `Listenable` is a different source; the same one having changed is what
    /// the listener below is for.
    pub fn update_should_notify(&self, old_notifier: Option<u64>) -> bool {
        old_notifier != self.notifier
    }

    /// Upstream's `update`, which moves the listener **only when the notifier
    /// changed**. Removing and re-adding the same listener every rebuild would
    /// be work per frame for no change.
    pub fn update(&mut self, new_notifier: Option<u64>) {
        if self.listening_to != new_notifier {
            self.listening_to = new_notifier;
        }
        self.notifier = new_notifier;
    }

    /// Upstream's `_handleUpdate`: the value said something, so mark dirty and
    /// schedule a build.
    pub fn handle_update(&mut self) {
        if self.notifier.is_none() {
            return;
        }
        self.dirty = true;
    }

    /// Upstream's `build`, which notifies **during the build** rather than at
    /// the moment the notification arrived.
    ///
    /// That is what turns a burst of notifications inside one frame into one
    /// rebuild of the dependents: the flag is set as often as the value likes,
    /// and cleared once when the frame gets around to it.
    pub fn build(&mut self) -> bool {
        if !self.dirty {
            return false;
        }
        self.notifications += 1;
        self.dirty = false;
        true
    }

    /// Upstream's `unmount`, which drops the listener. A notifier outliving
    /// the widget would otherwise keep calling into an element that is gone.
    pub fn unmount(&mut self) {
        self.listening_to = None;
    }
}

/// Upstream `InheritedTheme`: an inherited widget that knows how to re-apply
/// itself somewhere else.
pub trait InheritedTheme {
    /// Which type of theme this is. Upstream uses `runtimeType`; the capture
    /// below needs it to spot shadowing.
    fn theme_type(&self) -> &'static str;

    /// Upstream's `wrap`: rebuild this theme around `child`.
    fn wrap(&self, child: u64) -> u64;
}

/// One element on the walk upwards during a capture: its identity, and the
/// theme it is if it is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeLink {
    pub element: u64,
    /// `None` for an element that is not an [`InheritedTheme`] -- most of
    /// them. The walk visits every ancestor, not only the themes.
    pub theme_type: Option<&'static str>,
}

impl ThemeLink {
    pub fn theme(element: u64, theme_type: &'static str) -> ThemeLink {
        ThemeLink {
            element,
            theme_type: Some(theme_type),
        }
    }

    pub fn plain(element: u64) -> ThemeLink {
        ThemeLink {
            element,
            theme_type: None,
        }
    }
}

/// Upstream `CapturedThemes`: a frozen list of themes to wrap a widget in.
///
/// Frozen is the word upstream uses, and the consequence is stated twice in
/// its docs: **changes to the original themes are not seen by the wrapped
/// child** unless the capture is taken again. That is what a route pushed from
/// inside a themed subtree needs -- it renders in the overlay, far from where
/// it was created, and would otherwise pick up whatever theme happens to be
/// above the overlay instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapturedThemes {
    themes: Vec<ThemeLink>,
}

impl CapturedThemes {
    pub fn themes(&self) -> &[ThemeLink] {
        &self.themes
    }

    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }
}

/// Why a capture was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// Upstream: "The provided `to` context must be an ancestor of the `from`
    /// context."
    ToIsNotAnAncestor,
}

/// Upstream's `InheritedTheme.capture`.
///
/// `ancestors` is the walk from `from` upwards. Two rules come out of the
/// body:
///
/// * **`from == to` captures nothing.** There is no span between them, and an
///   empty capture is the honest answer rather than "everything".
/// * **Only the first theme of each type is kept**, because -- upstream's
///   comment -- "inherited themes completely shadow ancestors of the same
///   type". Keeping both would wrap the child twice and the outer one would
///   never be seen.
///
/// Upstream also warns that this "can be expensive if there are many widgets
/// between `from` and `to`", since it walks the element tree between them.
pub fn capture_themes(
    ancestors: &[ThemeLink],
    from: u64,
    to: Option<u64>,
) -> Result<CapturedThemes, CaptureError> {
    if Some(from) == to {
        return Ok(CapturedThemes::default());
    }
    let mut themes: Vec<ThemeLink> = Vec::new();
    let mut seen_types: Vec<&'static str> = Vec::new();
    let mut reached = to.is_none();
    for link in ancestors {
        if to == Some(link.element) {
            reached = true;
            break;
        }
        let Some(theme_type) = link.theme_type else {
            continue;
        };
        if !seen_types.contains(&theme_type) {
            seen_types.push(theme_type);
            themes.push(*link);
        }
    }
    if !reached {
        return Err(CaptureError::ToIsNotAnAncestor);
    }
    Ok(CapturedThemes { themes })
}

/// Upstream `LookupBoundary`: a widget that stops inherited lookups.
///
/// Its whole purpose is to let a widget's author say "nothing below here may
/// see past me". A `ViewAnchor`'s side view uses one, and so does anything
/// that renders a caller's widget in a place where finding the surrounding
/// tree would be wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LookupBoundary;

/// One element on the path upwards, for the boundary-aware lookups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AncestorEntry {
    pub element: u64,
    pub widget_type: &'static str,
    pub is_boundary: bool,
}

impl LookupBoundary {
    /// Upstream's `dependOnInheritedWidgetOfExactType`, and the first thing it
    /// does is the part nobody would guess:
    ///
    /// ```dart
    /// // The following call makes sure that context depends on something so
    /// // Element.didChangeDependencies is called when context moves in the
    /// // tree even when requested dependency remains unfulfilled (i.e. null
    /// // is returned).
    /// context.dependOnInheritedWidgetOfExactType<LookupBoundary>();
    /// ```
    ///
    /// **It depends on the boundary itself, unconditionally, even when the
    /// lookup finds nothing.** Otherwise a widget that found nothing would
    /// have no dependency at all, and moving it somewhere the answer *does*
    /// exist would never tell it.
    pub fn depend_on_inherited_widget_of_exact_type(
        ancestors: &[AncestorEntry],
        widget_type: &str,
    ) -> LookupOutcome {
        for entry in ancestors {
            if entry.is_boundary {
                break;
            }
            if entry.widget_type == widget_type {
                return LookupOutcome {
                    found: Some(entry.element),
                    depended_on_boundary: false,
                };
            }
        }
        LookupOutcome {
            found: None,
            depended_on_boundary: true,
        }
    }

    /// Upstream's `findAncestorWidgetOfExactType`, whose visitor returns
    /// `ancestor.widget.runtimeType != LookupBoundary` -- so the boundary is
    /// the **last** element visited rather than the first skipped. A lookup
    /// for the boundary type itself therefore finds it.
    pub fn find_ancestor_widget_of_exact_type(
        ancestors: &[AncestorEntry],
        widget_type: &str,
    ) -> Option<u64> {
        for entry in ancestors {
            if entry.widget_type == widget_type {
                return Some(entry.element);
            }
            if entry.is_boundary {
                return None;
            }
        }
        None
    }

    /// Upstream's `debugIsHidingAncestorWidgetOfExactType`, a debug-only
    /// question: is there one up there that a boundary is keeping from me?
    ///
    /// It exists for error messages. "No Material widget found" is much less
    /// use than the same message plus "there is one, but a LookupBoundary is
    /// in the way", and the second is only sayable if somebody looks.
    pub fn debug_is_hiding_ancestor_widget_of_exact_type(
        ancestors: &[AncestorEntry],
        widget_type: &str,
    ) -> bool {
        let mut past_boundary = false;
        for entry in ancestors {
            if entry.widget_type == widget_type {
                return past_boundary;
            }
            if entry.is_boundary {
                past_boundary = true;
            }
        }
        false
    }
}

/// What a boundary-aware lookup did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookupOutcome {
    pub found: Option<u64>,
    /// Whether a dependency was created on the boundary itself.
    pub depended_on_boundary: bool,
}

/// Upstream `LayoutChangedNotification`.
///
/// It carries **nothing**, and that is the design: a listener is being told
/// that something below it changed size, not what or by how much. Anything
/// more specific would be a promise the sender cannot keep, since the
/// notification bubbles through widgets that know nothing about the layout
/// that produced it.
///
/// Upstream's own doc warns that a listener must not mark anything dirty
/// during layout -- the notification arrives *while* laying out, so acting on
/// it means scheduling for the next frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutChangedNotification;

#[cfg(test)]
mod tests {
    use super::*;

    fn aspects(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    // -- The model's lookup walk -------------------------------------------

    #[test]
    fn the_walk_stops_at_the_first_model_that_answers_for_the_aspect() {
        let chain = vec![
            ModelLink::answering(10, vec!["b".to_string()]),
            ModelLink::answering(20, vec!["a".to_string()]),
            ModelLink::answering_everything(30),
        ];
        assert_eq!(
            find_models(&chain, &"a".to_string()),
            vec![10, 20],
            "10 does not answer for a, so the walk carries on -- and stops at 20"
        );
        assert_eq!(find_models(&chain, &"b".to_string()), vec![10]);
    }

    #[test]
    fn a_dependency_is_created_on_the_models_that_could_not_answer_too() {
        // Any of them could start supporting the aspect on a later build, and
        // a dependent registered only against the far one would never hear.
        let chain = vec![
            ModelLink::answering(10, Vec::<String>::new()),
            ModelLink::answering(20, Vec::new()),
            ModelLink::answering_everything(30),
        ];
        assert_eq!(find_models(&chain, &"a".to_string()), vec![10, 20, 30]);
    }

    #[test]
    fn a_model_answering_everything_ends_the_walk_at_once() {
        let chain = vec![
            ModelLink::answering_everything(10),
            ModelLink::answering_everything(20),
        ];
        assert_eq!(find_models(&chain, &"a".to_string()), vec![10]);
    }

    #[test]
    fn the_value_comes_from_the_nearest_model_that_actually_answers() {
        let chain = vec![
            ModelLink::answering(10, vec!["b".to_string()]),
            ModelLink::answering_everything(20),
        ];
        assert_eq!(inherit_from(&chain, Some(&"a".to_string())), Some(20));
        assert_eq!(inherit_from(&chain, Some(&"b".to_string())), Some(10));
    }

    #[test]
    fn naming_no_aspect_is_an_ordinary_inherited_lookup() {
        // Upstream says so outright: a dependent that named no aspect is
        // asking to be rebuilt for any change.
        let chain = vec![
            ModelLink::answering(10, vec!["b".to_string()]),
            ModelLink::answering_everything(20),
        ];
        assert_eq!(inherit_from(&chain, None), Some(10), "the nearest, flat");
    }

    #[test]
    fn no_model_of_the_type_at_all_gives_nothing() {
        let chain: Vec<ModelLink<String>> = Vec::new();
        assert_eq!(inherit_from(&chain, None), None);
        assert_eq!(inherit_from(&chain, Some(&"a".to_string())), None);
    }

    // -- The model element's dependency sets -------------------------------

    #[test]
    fn a_dependent_that_named_aspects_hears_only_about_those() {
        let mut element: InheritedModelElement<String> = InheritedModelElement::new();
        element.update_dependencies(1, Some("a".to_string()));
        element.update_dependencies(1, Some("b".to_string()));
        assert_eq!(element.dependencies_of(1), Some(&aspects(&["a", "b"])));

        assert!(element.notify_dependent(1, |deps| deps.contains("a")));
        assert!(!element.notify_dependent(1, |deps| deps.contains("c")));
    }

    #[test]
    fn once_a_dependent_asked_for_everything_an_aspect_cannot_narrow_it_back() {
        // A widget that called inheritFrom with no aspect and later with one
        // is still asking to hear about every change.
        let mut element: InheritedModelElement<String> = InheritedModelElement::new();
        element.update_dependencies(1, None);
        assert_eq!(element.dependencies_of(1), Some(&HashSet::new()));

        element.update_dependencies(1, Some("a".to_string()));
        assert_eq!(
            element.dependencies_of(1),
            Some(&HashSet::new()),
            "still empty, still everything"
        );
        assert!(
            element.notify_dependent(1, |_| false),
            "and it is notified regardless of what changed"
        );
    }

    #[test]
    fn naming_no_aspect_after_naming_some_widens_back_to_everything() {
        // The early return only fires for an already-empty set, so this
        // direction does go through.
        let mut element: InheritedModelElement<String> = InheritedModelElement::new();
        element.update_dependencies(1, Some("a".to_string()));
        element.update_dependencies(1, None);
        assert_eq!(element.dependencies_of(1), Some(&HashSet::new()));
    }

    #[test]
    fn a_dependent_nobody_recorded_is_not_notified_at_all() {
        // Which is different from one with an empty set, and the two are one
        // character apart in upstream's code.
        let element: InheritedModelElement<String> = InheritedModelElement::new();
        assert!(!element.notify_dependent(1, |_| true));
    }

    #[test]
    fn each_dependent_keeps_its_own_set() {
        let mut element: InheritedModelElement<String> = InheritedModelElement::new();
        element.update_dependencies(1, Some("a".to_string()));
        element.update_dependencies(2, Some("b".to_string()));

        assert!(element.notify_dependent(1, |deps| deps.contains("a")));
        assert!(!element.notify_dependent(2, |deps| deps.contains("a")));
    }

    // -- The notifier ------------------------------------------------------

    #[test]
    fn a_different_notifier_is_a_change_where_the_same_one_moving_is_not() {
        // The widget compares identity; the listener below handles the value.
        let notifier = InheritedNotifier::new(Some(7));
        assert!(notifier.update_should_notify(Some(8)));
        assert!(!notifier.update_should_notify(Some(7)));
        assert!(notifier.update_should_notify(None));
    }

    #[test]
    fn a_burst_of_notifications_inside_one_frame_is_one_rebuild() {
        // The flag is set as often as the value likes and cleared once, when
        // the frame gets around to it.
        let mut notifier = InheritedNotifier::new(Some(7));
        notifier.handle_update();
        notifier.handle_update();
        notifier.handle_update();
        assert!(notifier.is_dirty());

        assert!(notifier.build());
        assert_eq!(notifier.notifications(), 1);
        assert!(!notifier.is_dirty());

        assert!(!notifier.build(), "and a clean build says nothing");
        assert_eq!(notifier.notifications(), 1);
    }

    #[test]
    fn a_null_notifier_cannot_itself_send_notifications() {
        let mut notifier = InheritedNotifier::new(None);
        notifier.handle_update();
        assert!(!notifier.is_dirty());
        assert!(!notifier.build());
    }

    #[test]
    fn the_listener_moves_only_when_the_notifier_changed() {
        // Removing and re-adding the same listener every rebuild would be work
        // per frame for no change.
        let mut notifier = InheritedNotifier::new(Some(7));
        assert_eq!(notifier.listening_to(), Some(7));

        notifier.update(Some(7));
        assert_eq!(notifier.listening_to(), Some(7));

        notifier.update(Some(8));
        assert_eq!(notifier.listening_to(), Some(8));
    }

    #[test]
    fn unmounting_drops_the_listener_so_a_live_notifier_stops_calling_in() {
        let mut notifier = InheritedNotifier::new(Some(7));
        notifier.unmount();
        assert_eq!(notifier.listening_to(), None);
    }

    // -- Capturing themes --------------------------------------------------

    #[test]
    fn capturing_from_a_context_to_itself_captures_nothing() {
        // There is no span between them, and empty is the honest answer.
        let ancestors = vec![ThemeLink::theme(10, "Material")];
        let captured = capture_themes(&ancestors, 1, Some(1)).unwrap();
        assert!(captured.is_empty());
    }

    #[test]
    fn only_the_first_theme_of_each_type_is_kept() {
        // Inherited themes completely shadow ancestors of the same type;
        // keeping both would wrap the child twice and the outer would never be
        // seen.
        let ancestors = vec![
            ThemeLink::theme(10, "Material"),
            ThemeLink::theme(20, "Cupertino"),
            ThemeLink::theme(30, "Material"),
        ];
        let captured = capture_themes(&ancestors, 1, None).unwrap();
        assert_eq!(
            captured
                .themes()
                .iter()
                .map(|t| t.element)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
    }

    #[test]
    fn elements_that_are_not_themes_are_walked_past() {
        // The walk visits every ancestor, not only the themes.
        let ancestors = vec![
            ThemeLink::plain(5),
            ThemeLink::theme(10, "Material"),
            ThemeLink::plain(15),
        ];
        let captured = capture_themes(&ancestors, 1, None).unwrap();
        assert_eq!(captured.themes().len(), 1);
    }

    #[test]
    fn the_capture_stops_at_the_named_ancestor() {
        let ancestors = vec![
            ThemeLink::theme(10, "Material"),
            ThemeLink::plain(20),
            ThemeLink::theme(30, "Cupertino"),
        ];
        let captured = capture_themes(&ancestors, 1, Some(20)).unwrap();
        assert_eq!(captured.themes().len(), 1, "30 is above the stop");
    }

    #[test]
    fn a_to_that_is_not_an_ancestor_is_refused() {
        let ancestors = vec![ThemeLink::theme(10, "Material")];
        assert_eq!(
            capture_themes(&ancestors, 1, Some(99)),
            Err(CaptureError::ToIsNotAnAncestor)
        );
    }

    #[test]
    fn capturing_to_the_root_needs_no_ancestor_to_find() {
        let ancestors = vec![ThemeLink::theme(10, "Material")];
        assert!(capture_themes(&ancestors, 1, None).is_ok());
    }

    // -- The lookup boundary -----------------------------------------------

    fn path() -> Vec<AncestorEntry> {
        vec![
            AncestorEntry {
                element: 10,
                widget_type: "Padding",
                is_boundary: false,
            },
            AncestorEntry {
                element: 20,
                widget_type: "LookupBoundary",
                is_boundary: true,
            },
            AncestorEntry {
                element: 30,
                widget_type: "Material",
                is_boundary: false,
            },
        ]
    }

    #[test]
    fn a_lookup_cannot_reach_past_a_boundary() {
        let outcome = LookupBoundary::depend_on_inherited_widget_of_exact_type(&path(), "Material");
        assert_eq!(outcome.found, None, "there is one, but not for us");
    }

    #[test]
    fn a_failed_lookup_still_depends_on_the_boundary_itself() {
        // Otherwise a widget that found nothing would have no dependency at
        // all, and moving it somewhere the answer exists would never tell it.
        let outcome = LookupBoundary::depend_on_inherited_widget_of_exact_type(&path(), "Material");
        assert!(outcome.depended_on_boundary);

        let nothing_anywhere =
            LookupBoundary::depend_on_inherited_widget_of_exact_type(&[], "Material");
        assert_eq!(nothing_anywhere.found, None);
        assert!(
            nothing_anywhere.depended_on_boundary,
            "even with no boundary in sight"
        );
    }

    #[test]
    fn a_lookup_that_succeeds_before_the_boundary_needs_no_such_dependency() {
        let outcome = LookupBoundary::depend_on_inherited_widget_of_exact_type(&path(), "Padding");
        assert_eq!(outcome.found, Some(10));
        assert!(!outcome.depended_on_boundary);
    }

    #[test]
    fn the_boundary_is_the_last_element_visited_rather_than_the_first_skipped() {
        // Upstream's visitor returns `runtimeType != LookupBoundary`, so a
        // lookup for the boundary type itself finds it.
        assert_eq!(
            LookupBoundary::find_ancestor_widget_of_exact_type(&path(), "LookupBoundary"),
            Some(20)
        );
        assert_eq!(
            LookupBoundary::find_ancestor_widget_of_exact_type(&path(), "Material"),
            None
        );
        assert_eq!(
            LookupBoundary::find_ancestor_widget_of_exact_type(&path(), "Padding"),
            Some(10)
        );
    }

    #[test]
    fn the_debug_question_is_whether_one_is_being_hidden() {
        // "No Material widget found" is far less use than the same plus "there
        // is one, but a LookupBoundary is in the way".
        assert!(LookupBoundary::debug_is_hiding_ancestor_widget_of_exact_type(&path(), "Material"));
        assert!(
            !LookupBoundary::debug_is_hiding_ancestor_widget_of_exact_type(&path(), "Padding"),
            "that one is reachable"
        );
        assert!(
            !LookupBoundary::debug_is_hiding_ancestor_widget_of_exact_type(&path(), "Scaffold"),
            "and this one is not up there at all"
        );
    }

    // -- The notification --------------------------------------------------

    #[test]
    fn a_layout_change_notification_carries_nothing() {
        // A listener is told that something below it changed size, not what or
        // by how much -- anything more specific would be a promise the sender
        // cannot keep.
        assert_eq!(
            LayoutChangedNotification,
            LayoutChangedNotification::default()
        );
    }
}
