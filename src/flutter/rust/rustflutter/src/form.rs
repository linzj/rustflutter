//! Forms and their fields -- a port of upstream's `widgets/form.dart`.
//!
//! A form is a **registry**, not a container. Fields find it through the
//! context and put themselves on its list; the form never walks its own
//! subtree looking for them. That is what lets a field sit arbitrarily deep,
//! behind any amount of layout, and still be saved and validated with the
//! rest.
//!
//! The decision the file spends most effort on is **when to validate**. Doing
//! it on every keystroke tells a reader their half-typed email address is
//! wrong; doing it only on submit makes them fix five things at once. The five
//! `AutovalidateMode` values are five answers, and the two most useful ones --
//! `onUserInteraction` and `onUserInteractionIfError` -- both say *not until
//! they have done something*, differing only in whether an already-wrong field
//! keeps being checked as they fix it.

/// Upstream `AutovalidateMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutovalidateMode {
    /// Never automatically; only when something calls `validate`.
    #[default]
    Disabled,
    /// On every build, from the first one. A form that greets the reader with
    /// four red messages before they have typed anything is using this.
    Always,
    /// Once the reader has changed *any* field.
    OnUserInteraction,
    /// Once the reader has changed any field **and** something is already
    /// wrong. The difference from the above is the difference between "check
    /// as they go" and "stop complaining once they fix it" -- this one only
    /// speaks while there is something to say.
    OnUserInteractionIfError,
    /// When a field loses focus, which is the mode that validates one field at
    /// the moment the reader has finished with it and not before.
    OnUnfocus,
}

/// A field's identity and its current state, as the form sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormFieldState {
    pub id: u64,
    value: Option<String>,
    initial_value: Option<String>,
    error_text: Option<String>,
    /// Upstream's `forceErrorText`: an error imposed from outside, usually by
    /// a server that rejected the value.
    pub force_error_text: Option<String>,
    has_interacted_by_user: bool,
    pub has_focus: bool,
    saved: Vec<Option<String>>,
    resets: usize,
}

impl FormFieldState {
    pub fn new(id: u64, initial_value: Option<&str>) -> FormFieldState {
        FormFieldState {
            id,
            value: initial_value.map(str::to_string),
            initial_value: initial_value.map(str::to_string),
            error_text: None,
            force_error_text: None,
            has_interacted_by_user: false,
            has_focus: false,
            saved: Vec::new(),
            resets: 0,
        }
    }

    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    pub fn error_text(&self) -> Option<&str> {
        self.error_text.as_deref()
    }

    pub fn has_error(&self) -> bool {
        self.error_text.is_some()
    }

    /// Upstream's `hasInteractedByUser`, which its doc pins precisely: true
    /// only once `didChange` has been called, and back to false on `reset`.
    pub fn has_interacted_by_user(&self) -> bool {
        self.has_interacted_by_user
    }

    pub fn saved_values(&self) -> &[Option<String>] {
        &self.saved
    }

    pub fn resets(&self) -> usize {
        self.resets
    }

    /// Upstream's `isValid`, and its doc is emphatic about what it does
    /// **not** do: "This will not set errorText or hasError and it will not
    /// update error display."
    ///
    /// A passive question, for a caller that wants to enable a submit button
    /// without turning the form red while the reader is still typing.
    pub fn is_valid(&self, validator: impl Fn(Option<&str>) -> Option<String>) -> bool {
        self.force_error_text.is_none() && validator(self.value()).is_none()
    }

    /// Upstream's `_validate`, and the order is the point: **a forced error
    /// short-circuits the validator entirely**. A server saying "that username
    /// is taken" is not something the client-side validator can check or
    /// overrule, so it is not asked.
    pub fn validate_internal(
        &mut self,
        validator: Option<&dyn Fn(Option<&str>) -> Option<String>>,
    ) {
        if let Some(forced) = &self.force_error_text {
            self.error_text = Some(forced.clone());
            return;
        }
        self.error_text = match validator {
            Some(validator) => validator(self.value()),
            None => None,
        };
    }

    /// Upstream's `validate`: run the validator, show the result, and answer.
    pub fn validate(&mut self, validator: Option<&dyn Fn(Option<&str>) -> Option<String>>) -> bool {
        self.validate_internal(validator);
        !self.has_error()
    }

    /// Upstream's `save`, which hands the value to `onSaved` and **changes
    /// nothing**. Saving is not committing: the form is telling the caller
    /// what it holds, not putting it anywhere.
    pub fn save(&mut self) {
        self.saved.push(self.value.clone());
    }

    /// Upstream's `didChange`: the reader changed the value.
    pub fn did_change(&mut self, value: Option<&str>) {
        self.value = value.map(str::to_string);
        self.has_interacted_by_user = true;
    }

    /// Upstream's `setValue`, which is `@protected` and documented as being
    /// for subclasses updating during a build, "when calling setState is
    /// prohibited".
    ///
    /// It deliberately does **not** set `hasInteractedByUser` or tell the
    /// form: a value the widget worked out for itself is not the reader
    /// having done something.
    pub fn set_value(&mut self, value: Option<&str>) {
        self.value = value.map(str::to_string);
    }

    /// Upstream's `reset`, back to the initial value with the error cleared.
    pub fn reset(&mut self) {
        self.value.clone_from(&self.initial_value);
        self.clear_error_internal();
        self.resets += 1;
    }

    /// Upstream's `clearError`, which clears the message **without touching
    /// the value**.
    ///
    /// Its doc carries the caveat that makes it honest: under
    /// `AutovalidateMode.always` "the error may reappear immediately because
    /// the field will trigger a new validation cycle during the next build".
    /// Clearing an error is not fixing what caused it.
    pub fn clear_error(&mut self) {
        self.clear_error_internal();
    }

    fn clear_error_internal(&mut self) {
        self.error_text = None;
        self.has_interacted_by_user = false;
    }
}

/// Upstream `FormState`: the registry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormState {
    fields: Vec<FormFieldState>,
    pub autovalidate_mode: AutovalidateMode,
    has_interacted_by_user: bool,
    /// Upstream's `_generation`, bumped to force every field to rebuild.
    generation: usize,
    changes: usize,
    /// The error message that would be announced to a screen reader.
    announced: Option<String>,
}

impl FormState {
    pub fn new(autovalidate_mode: AutovalidateMode) -> FormState {
        FormState {
            fields: Vec::new(),
            autovalidate_mode,
            has_interacted_by_user: false,
            generation: 0,
            changes: 0,
            announced: None,
        }
    }

    pub fn fields(&self) -> &[FormFieldState] {
        &self.fields
    }

    pub fn field_mut(&mut self, id: u64) -> Option<&mut FormFieldState> {
        self.fields.iter_mut().find(|field| field.id == id)
    }

    pub fn generation(&self) -> usize {
        self.generation
    }

    /// How many times upstream's `onChanged` would have fired.
    pub fn changes(&self) -> usize {
        self.changes
    }

    pub fn announced(&self) -> Option<&str> {
        self.announced.as_deref()
    }

    pub fn has_interacted_by_user(&self) -> bool {
        self.has_interacted_by_user
    }

    pub fn has_error(&self) -> bool {
        self.fields.iter().any(|field| field.has_error())
    }

    /// Upstream's `_register`, called from the field's **build** rather than
    /// its `initState`: the form is found through the context, and a field
    /// moved under a different form has to re-register with that one.
    pub fn register(&mut self, field: FormFieldState) {
        if self.fields.iter().any(|held| held.id == field.id) {
            return;
        }
        self.fields.push(field);
    }

    /// Upstream's `_unregister`, called from `deactivate` -- before the field
    /// is disposed, so a form validating during teardown does not reach a dead
    /// field.
    pub fn unregister(&mut self, id: u64) {
        self.fields.retain(|field| field.id != id);
    }

    /// Upstream's `_fieldDidChange`, and it does three things in order:
    /// announce, **recompute the interacted flag from the fields**, and force
    /// a rebuild.
    ///
    /// Recomputing rather than merely setting it is what makes `reset` work: a
    /// form whose fields have all been reset goes back to not-interacted,
    /// which it could not do if the flag were latched.
    ///
    /// The rebuild is of **every** field, and upstream's comment says why:
    /// "useful if form fields have interdependencies". A confirm-password
    /// field has to be revalidated when the password changes.
    pub fn field_did_change(&mut self) {
        self.changes += 1;
        self.has_interacted_by_user = self
            .fields
            .iter()
            .any(|field| field.has_interacted_by_user());
        self.force_rebuild();
    }

    fn force_rebuild(&mut self) {
        self.generation += 1;
    }

    /// Upstream's `save`.
    pub fn save(&mut self) {
        for field in self.fields.iter_mut() {
            field.save();
        }
    }

    /// Upstream's `reset`, which resets every field and then calls
    /// `_fieldDidChange` -- so `onChanged` fires, and under
    /// `AutovalidateMode.always` everything is revalidated on the way out.
    pub fn reset(&mut self) {
        for field in self.fields.iter_mut() {
            field.reset();
        }
        self.has_interacted_by_user = false;
        self.field_did_change();
    }

    /// Upstream's `clearError`: every field's message goes, every field's
    /// value stays.
    pub fn clear_error(&mut self) {
        for field in self.fields.iter_mut() {
            field.clear_error_internal();
        }
        self.field_did_change();
    }

    /// Upstream's `validate`.
    ///
    /// It sets `_hasInteractedByUser` to **true** before validating, which
    /// reads oddly for a programmatic call -- nobody interacted. But it is
    /// what makes `onUserInteraction` behave after an explicit validate: the
    /// reader has now been shown errors, so continuing to check as they fix
    /// them is the helpful thing.
    pub fn validate(&mut self, validators: &dyn Fn(u64, Option<&str>) -> Option<String>) -> bool {
        self.has_interacted_by_user = true;
        self.force_rebuild();
        self.validate_internal(validators).is_empty()
    }

    /// Upstream's `validateGranularly`, which returns the invalid fields
    /// rather than a bool -- for a caller that wants to scroll to the first
    /// one or highlight all of them.
    pub fn validate_granularly(
        &mut self,
        validators: &dyn Fn(u64, Option<&str>) -> Option<String>,
    ) -> Vec<u64> {
        self.has_interacted_by_user = true;
        self.force_rebuild();
        self.validate_internal(validators)
    }

    /// Upstream's `_validate`, returning the ids that failed.
    ///
    /// Two things in the body are worth naming.
    ///
    /// **Only the first error message is announced.** Upstream's comment says
    /// so outright, and it is right: a screen reader reading four failures in
    /// a row tells the reader less than one failure they can act on.
    ///
    /// And upstream's per-field guard is a **tautology**:
    ///
    /// ```dart
    /// if (!validateOnFocusChange || !hasFocus || (validateOnFocusChange && hasFocus))
    /// ```
    ///
    /// Read it as `!A || !B || (A && B)` -- false only if `A && B && !(A && B)`,
    /// which cannot happen. Every field is validated whatever its focus. The
    /// condition is ported as what it computes, with this note, because
    /// copying it verbatim would suggest a focus rule that is not there.
    pub fn validate_internal(
        &mut self,
        validators: &dyn Fn(u64, Option<&str>) -> Option<String>,
    ) -> Vec<u64> {
        let mut invalid = Vec::new();
        let mut first_error: Option<String> = None;
        for field in self.fields.iter_mut() {
            let id = field.id;
            let validator = |value: Option<&str>| validators(id, value);
            let ok = field.validate(Some(&validator));
            if !ok {
                invalid.push(id);
                if first_error.is_none() {
                    first_error = field.error_text().map(str::to_string);
                }
            }
        }
        self.announced = first_error;
        invalid
    }

    /// Whether this build should validate, from the mode.
    pub fn should_autovalidate_on_build(&self) -> bool {
        match self.autovalidate_mode {
            AutovalidateMode::Always => true,
            AutovalidateMode::OnUserInteraction => self.has_interacted_by_user,
            AutovalidateMode::OnUserInteractionIfError => {
                self.has_interacted_by_user && self.has_error()
            }
            // `onUnfocus` validates when a field loses focus, which is not a
            // build; `disabled` never does.
            AutovalidateMode::OnUnfocus | AutovalidateMode::Disabled => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A validator that rejects anything not starting with "ok".
    fn strict(_id: u64, value: Option<&str>) -> Option<String> {
        match value {
            Some(value) if value.starts_with("ok") => None,
            _ => Some("bad".to_string()),
        }
    }

    fn form(mode: AutovalidateMode) -> FormState {
        let mut form = FormState::new(mode);
        form.register(FormFieldState::new(1, Some("ok one")));
        form.register(FormFieldState::new(2, Some("ok two")));
        form
    }

    // -- The registry ------------------------------------------------------

    #[test]
    fn a_field_registers_itself_rather_than_being_found() {
        // Which is what lets it sit arbitrarily deep behind any amount of
        // layout and still be saved with the rest.
        let mut form = FormState::new(AutovalidateMode::Disabled);
        assert!(form.fields().is_empty());

        form.register(FormFieldState::new(1, None));
        form.register(FormFieldState::new(2, None));
        assert_eq!(form.fields().len(), 2);

        form.unregister(1);
        assert_eq!(form.fields().len(), 1);
    }

    #[test]
    fn registering_the_same_field_twice_adds_it_once() {
        // Register runs from build, which happens more than once.
        let mut form = FormState::new(AutovalidateMode::Disabled);
        form.register(FormFieldState::new(1, None));
        form.register(FormFieldState::new(1, None));
        assert_eq!(form.fields().len(), 1);
    }

    // -- Save, reset, clear ------------------------------------------------

    #[test]
    fn saving_reports_the_values_and_changes_nothing() {
        // Saving is not committing: the form is telling the caller what it
        // holds, not putting it anywhere.
        let mut form = form(AutovalidateMode::Disabled);
        form.field_mut(1).unwrap().did_change(Some("edited"));
        form.save();

        assert_eq!(
            form.fields()[0].saved_values(),
            &[Some("edited".to_string())]
        );
        assert_eq!(form.fields()[0].value(), Some("edited"), "still there");
    }

    #[test]
    fn resetting_goes_back_to_the_initial_value_and_forgets_the_interaction() {
        let mut form = form(AutovalidateMode::Disabled);
        form.field_mut(1).unwrap().did_change(Some("edited"));
        form.field_did_change();
        assert!(form.has_interacted_by_user());

        form.reset();
        assert_eq!(form.fields()[0].value(), Some("ok one"));
        assert!(!form.has_interacted_by_user());
        assert!(!form.fields()[0].has_interacted_by_user());
    }

    #[test]
    fn the_interacted_flag_is_recomputed_from_the_fields_rather_than_latched() {
        // Which is what makes reset work: a form whose fields have all been
        // reset goes back to not-interacted.
        let mut form = form(AutovalidateMode::Disabled);
        form.field_mut(1).unwrap().did_change(Some("x"));
        form.field_did_change();
        assert!(form.has_interacted_by_user());

        form.field_mut(1).unwrap().reset();
        form.field_did_change();
        assert!(!form.has_interacted_by_user(), "recomputed, not latched");
    }

    #[test]
    fn clearing_the_error_leaves_the_value_alone() {
        let mut form = form(AutovalidateMode::Disabled);
        form.field_mut(1).unwrap().did_change(Some("bad value"));
        form.validate(&strict);
        assert!(form.has_error());

        form.clear_error();
        assert!(!form.has_error());
        assert_eq!(
            form.fields()[0].value(),
            Some("bad value"),
            "the cause is untouched"
        );
    }

    #[test]
    fn every_field_rebuilds_when_one_changes() {
        // Upstream: "useful if form fields have interdependencies" -- a
        // confirm-password field has to be revalidated when the password
        // changes.
        let mut form = form(AutovalidateMode::Disabled);
        let before = form.generation();
        form.field_did_change();
        assert_eq!(form.generation(), before + 1);
        assert_eq!(form.changes(), 1);
    }

    // -- Validation --------------------------------------------------------

    #[test]
    fn validate_reports_pass_and_fail() {
        let mut form = form(AutovalidateMode::Disabled);
        assert!(form.validate(&strict));

        form.field_mut(2).unwrap().did_change(Some("nope"));
        assert!(!form.validate(&strict));
        assert_eq!(form.fields()[1].error_text(), Some("bad"));
    }

    #[test]
    fn the_granular_form_names_the_fields_that_failed() {
        // For a caller that wants to scroll to the first one or highlight all
        // of them.
        let mut form = form(AutovalidateMode::Disabled);
        form.field_mut(1).unwrap().did_change(Some("no"));
        form.field_mut(2).unwrap().did_change(Some("also no"));
        assert_eq!(form.validate_granularly(&strict), vec![1, 2]);
    }

    #[test]
    fn only_the_first_error_is_announced() {
        // A screen reader reading four failures in a row tells the reader less
        // than one failure they can act on.
        let mut form = FormState::new(AutovalidateMode::Disabled);
        form.register(FormFieldState::new(1, Some("no")));
        form.register(FormFieldState::new(2, Some("also no")));

        let per_field = |id: u64, _value: Option<&str>| Some(format!("bad {id}"));
        form.validate(&per_field);
        assert_eq!(form.announced(), Some("bad 1"));
    }

    #[test]
    fn a_programmatic_validate_counts_as_the_reader_having_interacted() {
        // Which reads oddly until you see what it buys: after an explicit
        // validate, onUserInteraction keeps checking as they fix things.
        let mut form = form(AutovalidateMode::OnUserInteraction);
        assert!(!form.should_autovalidate_on_build());

        form.validate(&strict);
        assert!(form.has_interacted_by_user());
        assert!(form.should_autovalidate_on_build());
    }

    #[test]
    fn a_forced_error_short_circuits_the_validator_entirely() {
        // A server saying "that username is taken" is not something the
        // client-side validator can check or overrule, so it is not asked.
        let mut field = FormFieldState::new(1, Some("ok value"));
        field.force_error_text = Some("taken".to_string());

        let never_called = |_: Option<&str>| -> Option<String> {
            panic!("the validator must not run when an error is forced");
        };
        assert!(!field.validate(Some(&never_called)));
        assert_eq!(field.error_text(), Some("taken"));
    }

    #[test]
    fn is_valid_asks_without_showing_anything() {
        // For a caller enabling a submit button without turning the form red
        // while the reader is still typing.
        let field = FormFieldState::new(1, Some("nope"));
        assert!(!field.is_valid(|value| strict(1, value)));
        assert!(!field.has_error(), "and nothing was displayed");
        assert_eq!(field.error_text(), None);
    }

    #[test]
    fn is_valid_is_false_whenever_an_error_is_forced() {
        let mut field = FormFieldState::new(1, Some("ok value"));
        assert!(field.is_valid(|value| strict(1, value)));
        field.force_error_text = Some("taken".to_string());
        assert!(!field.is_valid(|value| strict(1, value)));
    }

    // -- When to validate ---------------------------------------------------

    #[test]
    fn always_validates_from_the_very_first_build() {
        // A form that greets the reader with four red messages before they
        // have typed anything is using this.
        let form = form(AutovalidateMode::Always);
        assert!(form.should_autovalidate_on_build());
    }

    #[test]
    fn on_user_interaction_waits_until_they_have_done_something() {
        let mut form = form(AutovalidateMode::OnUserInteraction);
        assert!(!form.should_autovalidate_on_build());

        form.field_mut(1).unwrap().did_change(Some("x"));
        form.field_did_change();
        assert!(form.should_autovalidate_on_build());
    }

    #[test]
    fn on_user_interaction_if_error_only_speaks_while_there_is_something_to_say() {
        // The difference between "check as they go" and "stop complaining once
        // they fix it".
        let mut form = form(AutovalidateMode::OnUserInteractionIfError);
        form.field_mut(1).unwrap().did_change(Some("ok still"));
        form.field_did_change();
        assert!(
            !form.should_autovalidate_on_build(),
            "interacted, but nothing is wrong"
        );

        form.field_mut(1)
            .unwrap()
            .validate_internal(Some(&|value| strict(1, value)));
        assert!(!form.has_error(), "still fine");

        form.field_mut(1).unwrap().did_change(Some("wrong"));
        form.field_mut(1)
            .unwrap()
            .validate_internal(Some(&|value| strict(1, value)));
        form.field_did_change();
        assert!(form.should_autovalidate_on_build(), "and now it speaks");
    }

    #[test]
    fn neither_unfocus_nor_disabled_validates_during_a_build() {
        // onUnfocus validates when a field loses focus, which is not a build.
        for mode in [AutovalidateMode::OnUnfocus, AutovalidateMode::Disabled] {
            let mut form = form(mode);
            form.field_mut(1).unwrap().did_change(Some("wrong"));
            form.field_did_change();
            assert!(!form.should_autovalidate_on_build(), "{mode:?}");
        }
    }

    #[test]
    fn every_field_is_validated_whatever_its_focus() {
        // Upstream's per-field guard is a tautology, and this is what it
        // computes.
        let mut form = form(AutovalidateMode::OnUnfocus);
        form.field_mut(1).unwrap().did_change(Some("wrong"));
        form.field_mut(1).unwrap().has_focus = true;
        form.field_mut(2).unwrap().did_change(Some("also wrong"));
        form.field_mut(2).unwrap().has_focus = false;

        assert_eq!(
            form.validate_granularly(&strict),
            vec![1, 2],
            "the focused one too"
        );
    }

    // -- The field's own bookkeeping ---------------------------------------

    #[test]
    fn set_value_is_not_the_reader_having_done_something() {
        // A value the widget worked out for itself during a build.
        let mut field = FormFieldState::new(1, Some("a"));
        field.set_value(Some("b"));
        assert_eq!(field.value(), Some("b"));
        assert!(!field.has_interacted_by_user());

        field.did_change(Some("c"));
        assert!(field.has_interacted_by_user());
    }

    #[test]
    fn clearing_a_fields_error_also_forgets_the_interaction() {
        // Which is what lets an onUserInteraction form go quiet again.
        let mut field = FormFieldState::new(1, Some("nope"));
        field.did_change(Some("still nope"));
        field.validate(Some(&|value| strict(1, value)));
        assert!(field.has_error() && field.has_interacted_by_user());

        field.clear_error();
        assert!(!field.has_error());
        assert!(!field.has_interacted_by_user());
    }

    #[test]
    fn a_field_with_no_validator_at_all_is_never_in_error() {
        let mut field = FormFieldState::new(1, Some("anything"));
        assert!(field.validate(None));
        assert_eq!(field.error_text(), None);
    }
}
