// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Selection controls, navigation surfaces and overlays.
//!
//! The second half of the component library. [`crate::components`] has the
//! pieces a page is built from -- surfaces, text, buttons; this has the pieces
//! a page is *operated* by.
//!
//! # Overlays are the framework's now
//!
//! This module used to say the opposite, and it is worth keeping the reason it
//! gave: a dialog, a sheet and a snackbar all mean "draw this over everything
//! and take the taps first", which is a `Stack` with the overlay as its last
//! child, and an application already knows how to write one. What it could not
//! write was an overlay that escapes the caller's `Stack` -- past a clip, past
//! a transform, above whatever else is on screen -- or one put up from a
//! callback with no build context to hand.
//!
//! [`crate::theatre`] is the manager, and every application has one:
//! `app.rs` installs an `Overlay` between the `MediaQuery` and the application.
//! The pieces here still go *in* one, and they are still perfectly usable in a
//! caller's own `Stack`; what has changed is that they no longer have to be.

use std::cell::RefCell;
use std::rc::Rc;

use crate::components::theme_of;
use crate::engine::{Color, TextStyle};
use crate::framework::{AnyWidget, BuildContext, Component, StateHandle, leaf, many, single};
use crate::gestures::PointerHandlers;
use crate::render::{
    Alignment, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisAlignment, MainAxisSize,
    RenderFlex, Size,
};
use crate::widgets::{Align, Center, Column, Container, Empty, Pointer, Row, SizedBox, Text};

// -- Selection controls -------------------------------------------------------

/// A box that is ticked or not.
pub struct Checkbox {
    id: u64,
    checked: bool,
    enabled: bool,
    label: Option<String>,
    handlers: PointerHandlers,
}

impl Checkbox {
    pub fn new(id: u64, checked: bool) -> Checkbox {
        Checkbox {
            id,
            checked,
            enabled: true,
            label: None,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The handlers directly, for a caller that already has them -- a control
    /// inside a list tile, where the tile and the control answer the same tap.
    /// [`Checkbox::wired`] is the same thing for the common case.
    pub fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        self.handlers = handlers;
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, toggle: fn(&mut S)) -> Self {
        if self.enabled {
            self.handlers = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(move |state| toggle(state));
            });
        }
        self
    }
}

impl Component for Checkbox {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let checked = self.checked;
        let enabled = self.enabled;
        let id = self.id;
        let handlers = self.handlers.clone();
        let label = self.label.clone();
        let body = theme.body();
        // Upstream's `Checkbox.build`: the fill, the tick and the side each
        // come off `CheckboxTheme.of(context)` resolved against the states
        // this checkbox is in, and fall back to the scheme.
        let mut states = crate::widget_state::WidgetStates::NONE;
        if checked {
            states = states.with(crate::widget_state::WidgetState::Selected);
        }
        if !enabled {
            states = states.with(crate::widget_state::WidgetState::Disabled);
        }
        let resolved = crate::component_themes::ResolvedCheckbox::of(context, states);
        // What a reader is told: that this is a checkbox and whether it is
        // ticked. `SemanticsProperties::check` is the last of this module's
        // twelve property constructors to have had no caller outside its own
        // tests -- so a checkbox reached a screen reader as a box with a word
        // beside it, indistinguishable from a label.
        //
        // `Some(checked)` rather than `None`: `None` is upstream's `mixed`,
        // which a `Checkbox` passes **only when `tristate` is set**, and this
        // one has no third state to be in. Sending it would announce every
        // plain checkbox as partly checked.
        let described = {
            let mut properties = crate::semantics::SemanticsProperties::check(
                label.clone().unwrap_or_default(),
                Some(checked),
            );
            properties.flags.is_enabled = enabled;
            if !enabled {
                properties.actions = 0;
            }
            let tap = self.handlers.on_tap.clone();
            let node = crate::semantics::node_id_for(id);
            move |inner: crate::framework::AnyWidget| {
                crate::semantics::tappable(node, properties.clone(), inner, tap.clone())
            }
        };
        let fill = resolved.fill;
        let border = resolved.side.color;
        let border_width = resolved.side.width;
        let tick = resolved.check;
        let spacing = theme.spacing;

        let ticked = leaf(move || {
            // The tick is two strokes rather than a glyph: a font that has no
            // check mark would silently draw nothing, and this is two lines.
            let mark = if checked {
                Container::new()
                    .with_size(10.0, 5.0)
                    .with_border(2.0, tick)
                    .with_corner_radius(1.0)
            } else {
                Container::new().with_size(10.0, 5.0)
            };
            let box_widget = Container::new()
                // Upstream's `_kEdgeSize`: an 18-by-18 box with 2-radius
                // corners.
                .with_size(18.0, 18.0)
                .with_color(fill)
                .with_corner_radius(2.0)
                .with_border(border_width, border)
                .with_child(Center::new(mark));

            let content: crate::widgets::BoxedWidget = match &label {
                Some(text) => crate::render::RenderRef::new(
                    Row::new()
                        .with_spacing(spacing)
                        .push(box_widget)
                        .push(Text::new(text.clone()).with_style(body.clone())),
                ),
                None => crate::render::RenderRef::new(box_widget),
            };
            Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::all(spacing * 0.75))
                    .with_child(content),
            )
            .with_handlers(handlers.clone())
        });
        described(ticked)
    }
}

/// One of a set, only one of which can be chosen.
pub struct Radio {
    id: u64,
    selected: bool,
    /// A radio that cannot be chosen. Mirrors [`Checkbox`]: no handlers, the
    /// ring and dot drawn in the outline colour.
    enabled: bool,
    label: Option<String>,
    handlers: PointerHandlers,
}

impl Radio {
    pub fn new(id: u64, selected: bool) -> Radio {
        Radio {
            id,
            selected,
            enabled: true,
            label: None,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The handlers directly -- see [`Checkbox::with_handlers`].
    pub fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        if self.enabled {
            self.handlers = handlers;
        }
        self
    }

    /// `choose` is given the state and sets whatever field records the choice.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, choose: fn(&mut S)) -> Self {
        if self.enabled {
            self.handlers = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(move |state| choose(state));
            });
        }
        self
    }
}

impl Component for Radio {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let enabled = self.enabled;
        let id = self.id;
        let handlers = self.handlers.clone();
        let label = self.label.clone();
        let body = theme.body();
        let spacing = theme.spacing;
        // Upstream's `Radio.build`: the fill, the ring and the dot's radius all
        // come off `RadioTheme.of(context)` resolved against the states this
        // radio is in, and fall back to the scheme. The same three steps
        // [`crate::controls::Checkbox`] takes.
        let mut states = crate::widget_state::WidgetStates::NONE;
        if selected {
            states = states.with(crate::widget_state::WidgetState::Selected);
        }
        if !enabled {
            states = states.with(crate::widget_state::WidgetState::Disabled);
        }
        let resolved = crate::component_themes::ResolvedRadio::of(context, states);
        let fill = resolved.fill;
        let ring_color = resolved.side.color;
        let ring_width = resolved.side.width;
        let outer = resolved.outer_radius;
        let inner = resolved.inner_radius;
        let background = resolved.background;

        // What a reader is told: that this is one of a set of choices, which
        // one is chosen, and -- on Apple's platforms only -- the hint that
        // tells an unselected one apart from a control that does nothing.
        //
        // The rule is `SemanticsProperties::radio`, written and tested since
        // it landed and reaching nothing: a radio arrived at a screen reader
        // as a plain box with a word beside it. The platform comes from the
        // theme for the reason the slider's does -- a desktop asked to behave
        // like a phone gets the phone's vocabulary too.
        let described = {
            use crate::localizations::WidgetsLocalizations;
            let properties = crate::semantics::SemanticsProperties::radio(
                label.clone().unwrap_or_default(),
                selected,
                crate::theme::ThemeData::of(context).platform,
                crate::localizations::DefaultWidgetsLocalizations.radio_button_unselected_label(),
            );
            let mut properties = properties;
            properties.flags.is_enabled = enabled;
            if !enabled {
                properties.actions = 0;
            }
            let tap = self.handlers.on_tap.clone();
            let node = crate::semantics::node_id_for(id);
            move |inner: crate::framework::AnyWidget| {
                crate::semantics::tappable(node, properties.clone(), inner, tap.clone())
            }
        };

        let body = leaf(move || {
            // The dot's size is the resolved radius doubled, and an unselected
            // radio resolves to zero -- so there is one path here and not two.
            let dot = Container::new()
                .with_size(inner * 2.0, inner * 2.0)
                .with_color(fill)
                .with_corner_radius(inner);
            let mut ring = Container::new()
                .with_size(outer * 2.0, outer * 2.0)
                .with_corner_radius(outer)
                .with_border(ring_width, ring_color)
                .with_child(Center::new(dot));
            if let Some(background) = background {
                ring = ring.with_color(background);
            }

            let content: crate::widgets::BoxedWidget = match &label {
                Some(text) => crate::render::RenderRef::new(
                    Row::new()
                        .with_spacing(spacing)
                        .push(ring)
                        .push(Text::new(text.clone()).with_style(body.clone())),
                ),
                None => crate::render::RenderRef::new(ring),
            };
            Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::all(spacing * 0.75))
                    .with_child(content),
            )
            .with_handlers(handlers.clone())
        });
        described(body)
    }
}

// -- What a reader hears from a radio -----------------------------------------

#[cfg(test)]
mod radio_semantics_tests {
    use super::*;

    /// The node a radio produces, through the real walk.
    fn radio_node(
        selected: bool,
        enabled: bool,
        platform: crate::editable_text::TargetPlatform,
    ) -> crate::semantics::SemanticsNode {
        crate::semantics::set_enabled(true);
        let theme = crate::theme::ThemeData {
            platform,
            ..crate::theme::ThemeData::light()
        };
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            crate::framework::component(
                Radio::new(1, selected)
                    .with_label("Medium")
                    .with_enabled(enabled),
            ),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .find(|node| node.properties.flags.is_in_mutually_exclusive_group)
            .cloned()
            .expect("a radio said it was one")
    }

    /// What a reader hears named as a route, from any surface.
    fn routes_named(
        surface: crate::framework::AnyWidget,
        platform: crate::editable_text::TargetPlatform,
    ) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let theme = crate::theme::ThemeData {
            platform,
            ..crate::theme::ThemeData::light()
        };
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(theme, surface));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| node.properties.flags.names_route)
            .map(|node| node.properties.label.clone())
            .collect()
    }

    /// The kinds every stop declares, with the label that goes with each.
    fn kinds_on(
        surface: crate::framework::AnyWidget,
        platform: crate::editable_text::TargetPlatform,
    ) -> Vec<(crate::semantics::SemanticsRole, String)> {
        crate::semantics::set_enabled(true);
        let theme = crate::theme::ThemeData {
            platform,
            ..crate::theme::ThemeData::light()
        };
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(theme, surface));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| node.properties.role.is_set())
            .map(|node| (node.properties.role, node.properties.label.clone()))
            .collect()
    }

    #[test]
    fn an_alert_says_it_is_an_alert_and_a_simple_dialog_says_it_is_a_dialog() {
        // Checked against `dialog.dart` rather than guessed from the names.
        // `Dialog.semanticsRole` defaults to `SemanticsRole.dialog` and
        // `AlertDialog.build` is the **only** override in the file
        // (`dialog.dart:953`); `SimpleDialog` returns a bare `Dialog`
        // (`dialog.dart:1372`) and takes the default. There is no
        // "simple dialog" role, and the pair of names invites the guess that
        // there is.
        //
        // The difference is what a platform does with it: an alert interrupts,
        // a dialog is somewhere the reader has been moved to.
        use crate::semantics::SemanticsRole;
        assert_eq!(
            kinds_on(
                crate::framework::component(AlertDialog::new().with_title("Delete this?")),
                crate::editable_text::TargetPlatform::Android
            ),
            vec![(SemanticsRole::AlertDialog, "Alert".to_string())]
        );
        assert_eq!(
            kinds_on(
                crate::framework::component(SimpleDialog::new().with_title("Pick one")),
                crate::editable_text::TargetPlatform::Android
            ),
            vec![(SemanticsRole::Dialog, "Dialog".to_string())]
        );
    }

    #[test]
    fn a_dialog_nobody_named_is_still_a_dialog() {
        // The case that made the role a separate test from the label. On Apple
        // this port names no route -- VoiceOver's focus lands on the title, so
        // saying the label too is one word too many -- and the whole node used
        // to be skipped with it. Upstream puts `role:` on the `Dialog`'s own
        // `Semantics` whatever names the route, so an unlabelled dialog is
        // silent there but not shapeless. Folded into the label's branch it
        // would have crossed as an anonymous box.
        use crate::semantics::SemanticsRole;
        assert_eq!(
            kinds_on(
                crate::framework::component(AlertDialog::new().with_title("Delete this?")),
                crate::editable_text::TargetPlatform::IOS
            ),
            vec![(SemanticsRole::AlertDialog, String::new())],
            "a kind with no words, which is what upstream leaves"
        );
    }

    #[test]
    fn a_surface_that_announces_itself_is_not_automatically_an_alert() {
        // The wrapper is shared by three surfaces, so the kind has to be each
        // caller's to say. This one is upstream's plain default -- and the
        // test is here because the first version of this round asserted the
        // rule about a `BottomSheet`, which never reaches this wrapper at all:
        // a mutation making every surface an alert stayed green, because
        // nothing was measuring a surface that goes through it.
        use crate::semantics::SemanticsRole;
        assert_eq!(
            kinds_on(
                crate::framework::component(Dialog::new("Sign in")),
                crate::editable_text::TargetPlatform::Android
            ),
            vec![(SemanticsRole::Dialog, "Dialog".to_string())]
        );
    }

    #[test]
    fn the_two_dialogs_are_announced_by_their_own_names() {
        // Not one rule written twice: `modal_surface_label` is shared and the
        // two pass different fallbacks, which is upstream's own distinction --
        // an alert interrupts, a dialog asks. Getting them the same way round
        // would tell a reader the wrong thing about how much attention is
        // being demanded.
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            routes_named(
                crate::framework::component(AlertDialog::new().with_title("Delete this?")),
                TargetPlatform::Android
            ),
            vec!["Alert".to_string()]
        );
        assert_eq!(
            routes_named(
                crate::framework::component(SimpleDialog::new().with_title("Pick one")),
                TargetPlatform::Android
            ),
            vec!["Dialog".to_string()],
            "a simple dialog asks rather than interrupts"
        );
    }

    /// The node a spinner produces, through the real walk.
    fn spinner_node(spinner: Spinner) -> crate::semantics::SemanticsNode {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(spinner),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(200.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .find(|node| node.id == crate::semantics::node_id_for(SPINNER_SEMANTICS_ID))
            .cloned()
            .expect("the spinner")
    }

    #[test]
    fn a_spinner_says_what_the_waiting_is_for() {
        // It said nothing, so a reader had no way to know the application was
        // busy: an arc that turns says "wait" only to people who can see it.
        let node = spinner_node(Spinner::new(0.4).with_semantic_label("Loading photos"));
        assert_eq!(node.properties.label, "Loading photos");
    }

    #[test]
    fn a_spinner_never_reads_its_rotation_out_as_progress() {
        // The trap this round existed to avoid. `Spinner::value` is the phase
        // of the rotation -- the constructor's doc says to feed it from a
        // looping controller -- so announcing it would report "40", then
        // "88", then "4": a progress report on nothing, and **worse than
        // silence**, because a reader would act on it.
        for phase in [0.0, 0.4, 0.88] {
            let node = spinner_node(Spinner::new(phase).with_semantic_label("Loading"));
            assert_eq!(
                node.properties.value, "",
                "the phase must not become a value: phase = {phase}"
            );
        }
    }

    #[test]
    fn a_caller_may_still_say_where_in_a_sequence_it_is() {
        // Upstream keeps `semanticsValue` on the indeterminate branch too: the
        // widget cannot work progress out, but the caller may know it. "Step 2
        // of 5" while the arc spins is a thing worth saying.
        let node = spinner_node(
            Spinner::new(0.4)
                .with_semantic_label("Importing")
                .with_semantic_value("Step 2 of 5"),
        );
        assert_eq!(node.properties.value, "Step 2 of 5");
    }

    #[test]
    fn a_sheets_drag_handle_can_be_found_and_says_what_it_does() {
        // Without this a reader met a 32-by-4 rectangle with nothing to say:
        // the one affordance for putting the sheet away was the one thing they
        // could not find. A bar that says "drag me" says it only to people who
        // can see it.
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(BottomSheet::new(crate::framework::leaf(|| {
                crate::widgets::SizedBox::new(10.0, 10.0)
            }))),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);

        let handle = nodes
            .iter()
            .find(|node| node.properties.flags.is_button)
            .expect("the handle is something to press");
        assert_eq!(
            handle.properties.label,
            crate::material_app::DefaultMaterialLocalizations::MODAL_BARRIER_DISMISS_LABEL,
            "the barrier's word, because dismissing is what it does -- upstream              reuses the string rather than inventing a second name for one action"
        );
    }

    #[test]
    fn the_handle_keeps_the_same_identity_between_frames() {
        // What the reserved constant is *for*. The platform keys its own
        // accessibility node on this id, so an id drawn from a counter --
        // `take_text_id`, say -- would be a different handle every frame: the
        // node a reader had focused would vanish under them each time the
        // sheet rebuilt.
        //
        // A mutation swapping the constant for another constant does not
        // break this and should not: the value is arbitrary, the *stability*
        // is the claim, and that is what this asserts.
        let ids = |_: ()| {
            crate::semantics::set_enabled(true);
            let mut tree = crate::framework::ElementTree::new();
            tree.rebuild(crate::theme::MaterialTheme::new(
                crate::theme::ThemeData::light(),
                crate::framework::component(BottomSheet::new(crate::framework::leaf(|| {
                    crate::widgets::SizedBox::new(10.0, 10.0)
                }))),
            ));
            let mut root = tree.build_render_tree().expect("mounted");
            crate::render::RenderBox::layout(
                &mut root,
                crate::render::BoxConstraints::loose(400.0, 400.0),
            );
            crate::semantics::mark_needs_update();
            let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
                .unwrap_or_default();
            crate::semantics::set_enabled(false);
            nodes
                .iter()
                .find(|node| node.properties.flags.is_button)
                .map(|node| node.id)
                .expect("the handle")
        };
        assert_eq!(ids(()), ids(()), "the same handle, frame after frame");
    }

    #[test]
    fn the_third_surface_announces_itself_as_a_dialog_not_an_alert() {
        // `Dialog` here is not upstream's `Dialog`. Upstream's is a bare
        // container that names no route because whatever it wraps does; this
        // one has a title, a body and actions of its own and nothing wraps it,
        // so it is a modal surface in its own right. A name that promises a
        // container and delivers a dialog is exactly the trap this port keeps
        // meeting.
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            routes_named(
                crate::framework::component(Dialog::new("Sign in")),
                TargetPlatform::Android
            ),
            vec!["Dialog".to_string()],
            "it asks rather than interrupts, so not \"Alert\""
        );
        assert_eq!(
            routes_named(
                crate::framework::component(
                    Dialog::new("Sign in").with_semantic_label("Sign in to continue")
                ),
                TargetPlatform::Android
            ),
            vec!["Sign in to continue".to_string()]
        );
        assert!(
            routes_named(
                crate::framework::component(Dialog::new("Sign in")),
                TargetPlatform::IOS
            )
            .is_empty(),
            "and the same Apple rule, because the wrapper is shared"
        );
    }

    #[test]
    fn a_simple_dialog_follows_the_same_apple_rule() {
        // The shared wrapper, so the Apple branch cannot drift between the
        // two: an unlabelled dialog names no route there because VoiceOver
        // already lands on the title.
        use crate::editable_text::TargetPlatform;
        assert!(
            routes_named(
                crate::framework::component(SimpleDialog::new().with_title("Pick one")),
                TargetPlatform::IOS
            )
            .is_empty()
        );
        assert_eq!(
            routes_named(
                crate::framework::component(
                    SimpleDialog::new()
                        .with_title("Pick one")
                        .with_semantic_label("Choose a size")
                ),
                TargetPlatform::IOS
            ),
            vec!["Choose a size".to_string()]
        );
    }

    /// What a reader hears from a dialog, in order.
    fn dialog_read_as(
        dialog: AlertDialog,
        platform: crate::editable_text::TargetPlatform,
    ) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let theme = crate::theme::ThemeData {
            platform,
            ..crate::theme::ThemeData::light()
        };
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            crate::framework::component(dialog),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| node.properties.flags.names_route)
            .map(|node| node.properties.label.clone())
            .collect()
    }

    #[test]
    fn a_dialog_announces_itself() {
        // It said nothing at all: a reader was handed a page that had changed
        // under them with no word that a modal had opened -- and
        // `resolved_semantic_label`, the rule for what to call it, was written
        // and called by nothing.
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            dialog_read_as(
                AlertDialog::new().with_title("Delete this?"),
                TargetPlatform::Android
            ),
            vec!["Alert".to_string()],
            "an alert interrupts, and says so before its contents"
        );
    }

    #[test]
    fn a_dialog_with_its_own_words_uses_them() {
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            dialog_read_as(
                AlertDialog::new()
                    .with_title("Delete this?")
                    .with_semantic_label("Confirm deletion"),
                TargetPlatform::Android
            ),
            vec!["Confirm deletion".to_string()]
        );
    }

    #[test]
    fn apple_is_left_to_announce_the_dialog_itself() {
        // Upstream's `label` is `semanticLabel` on Apple and
        // `semanticLabel ?? alertDialogLabel` everywhere else, so an
        // unlabelled dialog names no route there -- VoiceOver lands on the
        // title and saying "Alert" as well would be one word too many.
        use crate::editable_text::TargetPlatform;
        assert!(
            dialog_read_as(
                AlertDialog::new().with_title("Delete this?"),
                TargetPlatform::IOS
            )
            .is_empty(),
            "nothing names the route, deliberately"
        );
        assert_eq!(
            dialog_read_as(
                AlertDialog::new()
                    .with_title("Delete this?")
                    .with_semantic_label("Confirm deletion"),
                TargetPlatform::IOS
            ),
            vec!["Confirm deletion".to_string()],
            "but a label the caller wrote is still used"
        );
    }

    /// What a reader hears from a tab bar, in order.
    fn tabs_read_as(selected: usize) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(TabBar::new(
                40,
                vec!["Home".to_string(), "Search".to_string(), "You".to_string()],
                selected,
            )),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .map(|node| node.properties.label.clone())
            .filter(|label| !label.is_empty())
            .collect()
    }

    /// What each component actually says to a screen reader.
    ///
    /// # A report, not a check
    ///
    /// It asserts nothing and cannot fail, which is deliberate and is the shape
    /// `tools/descent.py` already has. The survey in tick 369 counted the wrong
    /// thing -- whether a component's `build` mentions `semantics` -- and tick
    /// 377 found `Badge` on that list while being perfectly audible: its count
    /// is a `Text`, which the walk annotates by itself. **Mentioning semantics
    /// and reaching a reader are different questions**, and only the second
    /// matters.
    ///
    /// So this mounts each one, runs the real walk, and prints what comes out.
    /// Silence is a gap only where a component has a **role or state its
    /// children cannot carry**: a container saying nothing of its own is
    /// correct, and a `Scaffold` announcing itself would be noise.
    ///
    /// Read it with `python tools/spoken.py`.
    #[test]
    fn spoken_census() {
        use crate::components::{Badge, CircleAvatar, Label, ListTile, ProgressBar, Scaffold};
        use crate::framework::component;
        let d = super::Destination::new;

        /// One mounted widget, and every node the walk gives back that says
        /// anything at all -- words, a value, a flag or an action.
        fn spoken_by(widget: crate::framework::AnyWidget) -> Vec<String> {
            crate::semantics::set_enabled(true);
            let mut tree = crate::framework::ElementTree::new();
            tree.rebuild(crate::theme::MaterialTheme::new(
                crate::theme::ThemeData::light(),
                widget,
            ));
            let said = match tree.build_render_tree() {
                Some(mut root) => {
                    crate::render::RenderBox::layout(
                        &mut root,
                        crate::render::BoxConstraints::loose(400.0, 400.0),
                    );
                    crate::semantics::mark_needs_update();
                    crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
                        .unwrap_or_default()
                        .iter()
                        .filter(|node| {
                            !node.properties.label.is_empty()
                                || !node.properties.value.is_empty()
                                || node.properties.role.is_set()
                                // The tip counts, for the reason the value was
                                // added in round 378: a report that shows a
                                // control with something to say as a blank
                                // line is the sort of table it replaced.
                                || !node.properties.tooltip.is_empty()
                                || node.properties.flags != Default::default()
                                || node.properties.actions != 0
                        })
                        .map(|node| {
                            let mut what = format!("{:?}", node.properties.label);
                            if !node.properties.value.is_empty() {
                                what.push_str(&format!(" ={:?}", node.properties.value));
                            }
                            if node.properties.role.is_set() {
                                what.push_str(&format!(" role={:?}", node.properties.role));
                            }
                            if !node.properties.tooltip.is_empty() {
                                what.push_str(&format!(" tip={:?}", node.properties.tooltip));
                            }
                            if node.properties.flags != Default::default() {
                                what.push_str(" +flags");
                            }
                            if node.properties.actions != 0 {
                                what.push_str(" +actions");
                            }
                            what
                        })
                        .collect()
                }
                None => vec!["did not mount".to_string()],
            };
            crate::semantics::set_enabled(false);
            said
        }

        println!(
            "SPOKEN {:<20} -> {}",
            "Badge on a tile",
            spoken_by(component(
                Badge::new("3").with_child(component(ListTile::new("Inbox")))
            ))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Scaffold",
            spoken_by(component(Scaffold::new(component(ListTile::new("Body"))))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "CircleAvatar",
            spoken_by(component(CircleAvatar::new().label_of("AB"))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "ListTile",
            spoken_by(component(ListTile::new("Inbox").with_subtitle("12 unread"))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Chip",
            spoken_by(component(super::Chip::new(1, "Sport"))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Checkbox",
            spoken_by(component(
                super::Checkbox::new(2, true).with_label("Remember me")
            ))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Radio",
            spoken_by(component(super::Radio::new(3, true).with_label("Medium"))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "TabBar",
            spoken_by(component(super::TabBar::new(
                40,
                vec!["Home".to_string(), "You".to_string()],
                0
            )))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "BottomNavigation",
            spoken_by(component(super::BottomNavigation::new(
                60,
                vec![d("Home", "H"), d("Saved", "S")],
                0
            )))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "NavigationRail",
            spoken_by(component(super::NavigationRail::new(
                70,
                vec![d("Home", "H"), d("Saved", "S")],
                0
            )))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Snackbar",
            spoken_by(component(super::Snackbar::new(80, "Saved"))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Banner",
            spoken_by(component(super::Banner::new("You are offline"))).join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "MaterialBanner",
            spoken_by(component(crate::components::MaterialBanner::new(
                component(Label::new("You are offline")),
                Vec::new()
            )))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "DataTable",
            spoken_by(component(super::DataTable::new(vec![
                "Name".to_string(),
                "Size".to_string()
            ])))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Section",
            spoken_by(component(super::Section::new(
                "Settings",
                component(ListTile::new("Wi-Fi"))
            )))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "SimpleDialogOption",
            // **With a callback**, because an option without one is not a
            // choice a reader can make and the census would be measuring a
            // dead control. The first version of this row left it off and so
            // reported a bare "Delete" for a shape that never occurs in a
            // dialog -- the same way round 377's `Badge` row answered a
            // question nobody had asked.
            spoken_by(component(
                super::SimpleDialogOption::new(90, || component(Label::new("Delete")))
                    .with_on_pressed(|| {})
            ))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "TooltipTrigger",
            // **With a message**, the way round 381 learned to build the
            // census's controls: a trigger with no tip is not a tooltip, and a
            // row measuring one answers a question nobody asked.
            spoken_by(component(
                super::TooltipTrigger::new(100, component(Label::new("Save")))
                    .with_message("Save to your drive")
            ))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "Spinner",
            spoken_by(component(
                super::Spinner::new(0.4).with_semantic_label("Loading")
            ))
            .join(", ")
        );
        println!(
            "SPOKEN {:<20} -> {}",
            "ProgressBar",
            spoken_by(component(ProgressBar::new(0.6))).join(", ")
        );
    }

    /// Whether a widget announces itself, and with what words.
    fn announced_as(widget: crate::framework::AnyWidget) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            widget,
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| node.properties.flags.is_live_region)
            .map(|node| node.properties.label.clone())
            .collect()
    }

    /// Every stop a reader would meet, as `(label, tooltip)`.
    fn stops_of(widget: crate::framework::AnyWidget) -> Vec<(String, String)> {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            widget,
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| !node.properties.label.is_empty() || !node.properties.tooltip.is_empty())
            .map(|node| {
                (
                    node.properties.label.clone(),
                    node.properties.tooltip.clone(),
                )
            })
            .collect()
    }

    /// Every stop a reader would meet, as `(label, role)`.
    fn roles_of(
        widget: crate::framework::AnyWidget,
    ) -> Vec<(String, crate::semantics::SemanticsRole)> {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            widget,
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| !node.properties.label.is_empty())
            .map(|node| (node.properties.label.clone(), node.properties.role))
            .collect()
    }

    #[test]
    fn a_column_header_says_that_it_heads_a_column() {
        // Upstream's `_buildHeadingCell` opens with
        // `Semantics(role: SemanticsRole.columnHeader, ...)`. The case that
        // shows why a role is not a flag: a header has no state and nothing to
        // press, so no flag has anything to say about it, and a reader who is
        // told only the word "Name" has no reason to think it names a column.
        //
        // Every node in this port crossed to the engine as `kNone` until now,
        // so this is the first role it has ever sent.
        use crate::semantics::SemanticsRole;
        assert_eq!(
            roles_of(crate::framework::component(
                DataTable::new(vec!["Name".to_string(), "Size".to_string()])
                    .push_row(vec!["notes.txt".to_string(), "2 KB".to_string()])
            )),
            vec![
                ("Name".to_string(), SemanticsRole::ColumnHeader),
                ("Size".to_string(), SemanticsRole::ColumnHeader),
                ("notes.txt".to_string(), SemanticsRole::None),
                ("2 KB".to_string(), SemanticsRole::None),
            ],
            "the headers head and the cells do not"
        );
    }

    #[test]
    fn the_nearest_role_is_the_one_a_reader_hears() {
        // Upstream's merge: `if (role == SemanticsRole.none) role = node._role`.
        // A merging node is one thing, so it has one kind, and the claim
        // nearest the top wins -- a header folded into a region that merges
        // for its own reasons keeps the region's kind, not the header's.
        use crate::semantics::SemanticsRole;
        let folded = roles_of(crate::semantics::announces_itself(
            crate::framework::component(DataTable::new(vec!["Name".to_string()])),
        ));
        assert_eq!(
            folded,
            vec![("Name".to_string(), SemanticsRole::ColumnHeader)],
            "with nothing above claiming a kind, the header's rises"
        );
    }

    #[test]
    fn a_kind_claimed_higher_up_is_not_overwritten_from_below() {
        // The other half of upstream's `if (role == SemanticsRole.none) role =
        // node._role`. A merging node is **one thing**, so it has one kind,
        // and the claim nearest the top is the one that describes what the
        // reader has actually stopped on. Taken the other way round, a region
        // holding a table would be announced as a column header.
        use crate::semantics::SemanticsRole;
        let inside_a_region = roles_of(crate::framework::single(
            crate::framework::component(DataTable::new(vec!["Name".to_string()])),
            |inner| {
                crate::render::RenderMergeSemanticsBox::new(inner).with_properties(
                    crate::semantics::SemanticsProperties {
                        role: SemanticsRole::Region,
                        ..crate::semantics::SemanticsProperties::label("")
                    },
                )
            },
        ));
        assert_eq!(
            inside_a_region,
            vec![("Name".to_string(), SemanticsRole::Region)],
            "the header's kind overwrote the region's"
        );
    }

    #[test]
    fn a_config_carries_its_kind_across_the_seam_to_the_collector() {
        // The same seam round 382 found dropping the tooltip, and the same
        // reason for testing it directly: `to_properties` copies field by
        // field, so a field added to both ends and forgotten in the middle is
        // silently lost. No widget reaches the collector through this seam
        // yet -- the rule is here and its producer is still to come, which the
        // test says rather than dresses up.
        let mut config = crate::semantics::SemanticsConfiguration::default();
        config.role = crate::semantics::SemanticsRole::ColumnHeader;
        assert_eq!(
            config.to_properties().role,
            crate::semantics::SemanticsRole::ColumnHeader
        );
    }

    #[test]
    fn a_tip_is_said_beside_the_thing_it_is_about() {
        // The gap: a screen reader **neither hovers nor long-presses**, so
        // every way this widget offers to raise the bubble is a way the reader
        // does not have. Upstream's answer is `Semantics(tooltip: message)` on
        // the trigger itself (`raw_tooltip.dart`), which stands the tip beside
        // the control whether or not the bubble is on screen. Without it the
        // tip's words existed only for people who could see them.
        assert_eq!(
            stops_of(crate::framework::component(
                TooltipTrigger::new(
                    1,
                    crate::framework::component(crate::components::Label::new("Save"))
                )
                .with_message("Save to your drive")
            )),
            vec![("Save".to_string(), "Save to your drive".to_string())],
            "one stop, carrying both"
        );
    }

    #[test]
    fn a_tip_is_not_the_controls_name() {
        // The reason it is a field of its own rather than more label: a reader
        // announces the two separately, and a tip run onto the label would be
        // read out as what the control is called -- "Save Save to your drive".
        let stops = stops_of(crate::framework::component(
            TooltipTrigger::new(
                1,
                crate::framework::component(crate::components::Label::new("Save")),
            )
            .with_message("Save to your drive"),
        ));
        assert_eq!(stops[0].0, "Save");
    }

    #[test]
    fn a_trigger_with_nothing_to_add_adds_nothing() {
        // Upstream's `RawTooltip.build` returns the bare child for an empty
        // message, and treats null and empty alike when deciding to exclude.
        // A node with an empty tip tells a reader there is more to hear and
        // then says nothing.
        for trigger in [
            TooltipTrigger::new(
                1,
                crate::framework::component(crate::components::Label::new("Save")),
            ),
            TooltipTrigger::new(
                1,
                crate::framework::component(crate::components::Label::new("Save")),
            )
            .with_message(""),
            TooltipTrigger::new(
                1,
                crate::framework::component(crate::components::Label::new("Save")),
            )
            .with_message("Save to your drive")
            .excluded_from_semantics(true),
        ] {
            assert_eq!(
                stops_of(crate::framework::component(trigger)),
                vec![("Save".to_string(), String::new())]
            );
        }
    }

    #[test]
    fn a_trigger_with_nothing_to_add_does_not_reshape_the_tree_either() {
        // The stronger half of the rule above, and the one a node-by-node
        // check cannot see: the tip is carried by a **fold**, so publishing an
        // empty one would not merely say nothing -- it would gather two
        // separate stops into one. A reader who had two things to step through
        // would find one.
        // A shape the census already shows as two stops: `Section -> "Settings",
        // "Wi-Fi"`.
        let two_stops = || {
            crate::framework::component(Section::new(
                "Settings",
                crate::framework::component(crate::components::ListTile::new("Wi-Fi")),
            ))
        };
        let expected = vec![
            ("Settings".to_string(), String::new()),
            ("Wi-Fi".to_string(), String::new()),
        ];
        assert_eq!(
            stops_of(crate::framework::component(TooltipTrigger::new(
                1,
                two_stops()
            ))),
            expected,
            "an untipped trigger left the tree alone"
        );
        assert_eq!(
            stops_of(crate::framework::component(
                TooltipTrigger::new(1, two_stops()).with_message("")
            )),
            expected,
            "an empty message folded two stops into one"
        );
    }

    #[test]
    fn a_tip_inside_a_merge_rises_to_the_stop_that_holds_it() {
        // A trigger does not have to be the outermost thing. Folded into a
        // region that merges for its own reasons -- a banner, say -- the tip
        // has to travel up to the node that ends up holding the words, or the
        // control keeps a tip on a node that no longer exists.
        assert_eq!(
            stops_of(crate::semantics::announces_itself(
                crate::framework::component(
                    TooltipTrigger::new(
                        1,
                        crate::framework::component(crate::components::Label::new("Save"))
                    )
                    .with_message("Save to your drive")
                )
            )),
            vec![("Save".to_string(), "Save to your drive".to_string())]
        );
    }

    #[test]
    fn a_config_carries_its_tip_across_the_seam_to_the_collector() {
        // `SemanticsConfiguration::to_properties` is the hand-off between the
        // upstream-shaped config and the collector this crate walks with, and
        // it listed every string but this one -- so a tip that survived every
        // merge rule above would be dropped on the way out. Tested directly
        // because no widget reaches the collector through that seam yet: the
        // rule is real, the producer is still to come, and a test that pretends
        // otherwise would be measuring nothing.
        let mut config = crate::semantics::SemanticsConfiguration::default();
        config.tooltip = "Save to your drive".to_string();
        assert_eq!(config.to_properties().tooltip, "Save to your drive");
    }

    #[test]
    fn the_nearer_tip_is_the_one_a_reader_hears() {
        // Upstream's `SemanticsConfiguration.absorb` takes a child's tooltip
        // only when it has none of its own, and does **not** join two the way
        // it joins labels: a tip is one sentence about one control, and a pair
        // run together would be a sentence about neither.
        let stops = stops_of(crate::framework::component(
            TooltipTrigger::new(
                1,
                crate::framework::component(
                    TooltipTrigger::new(
                        2,
                        crate::framework::component(crate::components::Label::new("Save")),
                    )
                    .with_message("the inner tip"),
                ),
            )
            .with_message("the outer tip"),
        ));
        assert_eq!(
            stops,
            vec![("Save".to_string(), "the outer tip".to_string())]
        );
    }

    #[test]
    fn a_banner_announces_itself_even_though_it_will_not_go_away() {
        // The judgement worth recording: a snack bar's case is easy, since it
        // is gone in four seconds and a reader who has to hunt for it has
        // already lost it. A banner **stays**, which makes the case look
        // weaker -- and upstream gives it the flag anyway (`banner.dart:443`).
        // A thing that appeared unbidden is worth telling someone about
        // whether or not it will leave on its own.
        assert_eq!(
            announced_as(crate::framework::component(Banner::new("You are offline"))),
            vec!["You are offline".to_string()]
        );
    }

    #[test]
    fn upstreams_own_banner_shape_announces_itself_too() {
        // Two banners live here: this crate's simple one above, and the full
        // upstream shape in `components`. Both appear on their own, so both
        // say so -- through one rule, so they cannot drift apart.
        assert_eq!(
            announced_as(crate::framework::component(
                crate::components::MaterialBanner::new(
                    crate::framework::component(crate::components::Label::new("You are offline")),
                    Vec::new(),
                )
            )),
            vec!["You are offline".to_string()],
            "and once, not twice -- see `a_label_folded_into_a_merge_says_its_words_once`"
        );
    }

    #[test]
    fn a_bar_dropped_straight_into_a_column_is_still_announced() {
        // The case that moved the announcement onto the widget. Four of the
        // gallery's demos build a `Snackbar` and push it into a column,
        // never touching the messenger -- so with the flag on the messenger's
        // door those bars appeared with nothing to draw a reader's attention,
        // and were gone four seconds later.
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(Snackbar::new(7, "Saved")),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 300.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 300.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);

        let live: Vec<&crate::semantics::SemanticsNode> = nodes
            .iter()
            .filter(|node| node.properties.flags.is_live_region)
            .collect();
        assert_eq!(live.len(), 1, "one live region: {live:?}");
        assert_eq!(
            live[0].properties.label, "Saved",
            "and it is the node with the words -- an empty live region              announces nothing"
        );
    }

    #[test]
    fn all_three_bars_of_one_of_n_announce_themselves_alike() {
        // The rail was the one that drifted: four bare stops, no position and
        // no sense of which page you are on, while the tab bar and the bottom
        // bar had both been wired. `spoken_census` found it on its first run,
        // which is what that report is for.
        use crate::semantics::SemanticsTristate;
        let heard = |widget: crate::framework::AnyWidget| {
            crate::semantics::set_enabled(true);
            let mut tree = crate::framework::ElementTree::new();
            tree.rebuild(crate::theme::MaterialTheme::new(
                crate::theme::ThemeData::light(),
                widget,
            ));
            let mut root = tree.build_render_tree().expect("mounted");
            crate::render::RenderBox::layout(
                &mut root,
                crate::render::BoxConstraints::loose(400.0, 400.0),
            );
            crate::semantics::mark_needs_update();
            let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 400.0), &root)
                .unwrap_or_default();
            crate::semantics::set_enabled(false);
            nodes
                .iter()
                .filter(|node| node.properties.label.starts_with("Tab "))
                // The position as well as the flag: asserting only which one
                // is current lets a bar number its choices backwards and say
                // nothing about it.
                .map(|node| {
                    (
                        node.properties
                            .label
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string(),
                        node.properties.flags.selected,
                    )
                })
                .collect::<Vec<_>>()
        };
        let expected = vec![
            ("Tab 1 of 2".to_string(), SemanticsTristate::False),
            ("Tab 2 of 2".to_string(), SemanticsTristate::True),
        ];
        let rail = crate::framework::component(NavigationRail::new(
            70,
            vec![
                Destination::new("Home", "H"),
                Destination::new("Saved", "S"),
            ],
            1,
        ));
        assert_eq!(heard(rail), expected, "the rail");
        let bar = crate::framework::component(BottomNavigation::new(
            60,
            vec![
                Destination::new("Home", "H"),
                Destination::new("Saved", "S"),
            ],
            1,
        ));
        assert_eq!(heard(bar), expected, "the bottom bar");
        let tabs = crate::framework::component(TabBar::new(
            40,
            vec!["Home".to_string(), "Saved".to_string()],
            1,
        ));
        assert_eq!(heard(tabs), expected, "the tab bar");
    }

    #[test]
    fn each_destination_says_where_it_is_and_which_one_you_are_on() {
        // The tab bar's loss on a phone's primary navigation. Same rule, and
        // deliberately the same function -- two bars of choose-one-of-N should
        // not drift into announcing themselves differently.
        use crate::semantics::SemanticsTristate;
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(BottomNavigation::new(
                60,
                vec![
                    // `new(label, mark)`: the words first, the glyph second.
                    Destination::new("Home", "H"),
                    Destination::new("Saved", "S"),
                ],
                1,
            )),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);

        let heard: Vec<(String, SemanticsTristate)> = nodes
            .iter()
            .filter(|node| node.properties.label.contains("Tab "))
            .map(|node| {
                (
                    node.properties.label.clone(),
                    node.properties.flags.selected,
                )
            })
            .collect();
        assert_eq!(
            heard,
            vec![
                (
                    "Tab 1 of 2
H
Home"
                        .to_string(),
                    SemanticsTristate::False
                ),
                (
                    "Tab 2 of 2
S
Saved"
                        .to_string(),
                    SemanticsTristate::True
                ),
            ],
            "one stop each, and the second is the one you are on"
        );
    }

    #[test]
    fn each_tab_says_where_it_is_in_the_set() {
        // `tab_label` was written, documented and called by nothing, so a bar
        // of tabs reached a reader as three words with no positions.
        let heard = tabs_read_as(0);
        assert_eq!(
            heard,
            vec![
                "Tab 1 of 3
Home"
                    .to_string(),
                "Tab 2 of 3
Search"
                    .to_string(),
                "Tab 3 of 3
You"
                .to_string(),
            ],
            "one stop each, position and words together"
        );
    }

    #[test]
    fn the_tab_you_are_on_sounds_different_from_the_others() {
        // The loss this fixes: without `selected` every tab in the bar is the
        // same announcement, so a reader cannot tell which page they are on --
        // the filter chip's problem, on a control that navigates.
        use crate::semantics::SemanticsTristate;
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(TabBar::new(
                40,
                vec!["Home".to_string(), "Search".to_string()],
                1,
            )),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);

        let chosen: Vec<(bool, SemanticsTristate)> = nodes
            .iter()
            .filter(|node| node.properties.label.contains("Tab "))
            .map(|node| {
                (
                    node.properties.label.contains("Search"),
                    node.properties.flags.selected,
                )
            })
            .collect();
        assert_eq!(
            chosen,
            vec![
                (false, SemanticsTristate::False),
                (true, SemanticsTristate::True)
            ],
            "the second one is the one you are on"
        );
    }

    /// The node a chip produces, through the real walk.
    fn chip_node(style: ChipStyle, tappable: bool) -> crate::semantics::SemanticsNode {
        crate::semantics::set_enabled(true);
        let mut chip = Chip::new(3, "Sport").with_style(style);
        if tappable {
            chip = chip.on_tap(|_| {});
        }
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(chip),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .find(|node| node.properties.label.contains("Sport"))
            .cloned()
            .expect("a chip said its word")
    }

    #[test]
    fn a_chosen_chip_sounds_different_from_an_unchosen_one() {
        // A chip had no semantics at all, so a filter that is on and one that
        // is off reached a reader as the same plain box with the same word in
        // it -- which is the one distinction a filter chip exists to make.
        use crate::semantics::SemanticsTristate;
        assert_eq!(
            chip_node(ChipStyle::Selected, true)
                .properties
                .flags
                .selected,
            SemanticsTristate::True
        );
        assert_eq!(
            chip_node(ChipStyle::Filter, true).properties.flags.selected,
            SemanticsTristate::False
        );
    }

    #[test]
    fn a_chip_nobody_listens_to_is_not_announced_as_a_button() {
        // Upstream's `button: widget.tapEnabled`. A chip used as a plain label
        // should not invite a press that does nothing -- and it has no enabled
        // state either, which is a third answer rather than "disabled".
        let pressable = chip_node(ChipStyle::Action, true);
        assert!(pressable.properties.flags.is_button);
        assert!(
            pressable
                .properties
                .has(crate::semantics::SemanticsAction::Tap)
        );
        assert!(pressable.properties.flags.is_enabled);

        let label = chip_node(ChipStyle::Action, false);
        assert!(!label.properties.flags.is_button);
        assert!(!label.properties.has(crate::semantics::SemanticsAction::Tap));
        assert!(
            !label.properties.flags.has_enabled_state,
            "no enabled state at all, rather than disabled"
        );
    }

    #[test]
    fn a_chip_is_one_stop() {
        // `container: true`. The word inside a chip is what the chip says, not
        // a separate thing beside it.
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(Chip::new(3, "Sport").on_tap(|_| {})),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        let spoken: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(spoken, vec!["Sport"], "one stop: {spoken:?}");
    }

    /// The node a checkbox produces, through the real walk.
    fn checkbox_node(checked: bool, enabled: bool) -> crate::semantics::SemanticsNode {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(
                Checkbox::new(2, checked)
                    .with_label("Remember me")
                    .with_enabled(enabled),
            ),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .find(|node| node.properties.label == "Remember me")
            .cloned()
            .expect("a checkbox said its label")
    }

    #[test]
    fn a_checkbox_says_whether_it_is_ticked() {
        // The last of this module's twelve property constructors to have had
        // no caller: a checkbox reached a screen reader as a box with a word
        // beside it, which is a label.
        use crate::semantics::SemanticsCheckState;
        let on = checkbox_node(true, true);
        assert_eq!(on.properties.flags.checked, SemanticsCheckState::Checked);
        assert!(on.properties.has(crate::semantics::SemanticsAction::Tap));

        let off = checkbox_node(false, true);
        assert_eq!(off.properties.flags.checked, SemanticsCheckState::Unchecked);
    }

    #[test]
    fn a_plain_checkbox_is_never_partly_checked() {
        // `None` is upstream's `mixed`, which a `Checkbox` passes **only** when
        // `tristate` is set. This one has no third state, so sending `None`
        // would announce every plain checkbox as partly checked -- and
        // "partly" is a real answer a reader acts on.
        use crate::semantics::SemanticsCheckState;
        for checked in [true, false] {
            assert_ne!(
                checkbox_node(checked, true).properties.flags.checked,
                SemanticsCheckState::Mixed,
                "checked = {checked}"
            );
        }
    }

    #[test]
    fn a_checkbox_that_cannot_be_ticked_says_so_and_offers_nothing() {
        let node = checkbox_node(false, false);
        assert!(!node.properties.flags.is_enabled);
        assert!(node.properties.flags.has_enabled_state);
        assert!(!node.properties.has(crate::semantics::SemanticsAction::Tap));
    }

    #[test]
    fn a_radio_says_it_is_one_of_a_set() {
        // The rule has been written and tested since `RawRadio` landed and
        // reached nothing: `SemanticsProperties::radio` had only its own tests
        // for callers, so a radio arrived at a screen reader as a plain box
        // with a word beside it.
        use crate::editable_text::TargetPlatform;
        let node = radio_node(true, true, TargetPlatform::Android);
        assert!(node.properties.flags.is_in_mutually_exclusive_group);
        assert_eq!(node.properties.label, "Medium");
        assert_eq!(
            node.properties.flags.checked,
            crate::semantics::SemanticsCheckState::Checked
        );
    }

    #[test]
    fn only_apple_hears_selected_and_the_hint() {
        // The same fact in two properties, because the two screen readers read
        // different ones -- and setting `selected` everywhere is not neutral:
        // TalkBack would announce a radio as selected *and* checked.
        use crate::editable_text::TargetPlatform;
        let android = radio_node(false, true, TargetPlatform::Android);
        assert_eq!(
            android.properties.flags.selected,
            crate::semantics::SemanticsTristate::None,
            "silence, not `false`"
        );
        assert_eq!(android.properties.hint, "", "and no hint");

        let apple = radio_node(false, true, TargetPlatform::IOS);
        assert_eq!(
            apple.properties.flags.selected,
            crate::semantics::SemanticsTristate::False
        );
        assert_eq!(
            apple.properties.hint, "Not selected",
            "the unselected one needs telling; silence there reads as a              control that does nothing"
        );

        let apple_chosen = radio_node(true, true, TargetPlatform::IOS);
        assert_eq!(apple_chosen.properties.hint, "", "said once, not twice");
    }

    #[test]
    fn a_radio_that_cannot_be_chosen_says_so_and_offers_nothing() {
        use crate::editable_text::TargetPlatform;
        let node = radio_node(false, false, TargetPlatform::Android);
        assert!(!node.properties.flags.is_enabled);
        assert!(node.properties.flags.has_enabled_state);
        assert!(!node.properties.has(crate::semantics::SemanticsAction::Tap));
    }
}

// -- Chips --------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChipStyle {
    /// Plain, for a label that is also a filter.
    #[default]
    Filter,
    /// Selected filter.
    Selected,
    /// An action rather than a state.
    Action,
}

/// A compact label that can be tapped.
pub struct Chip {
    id: u64,
    label: String,
    style: ChipStyle,
    handlers: PointerHandlers,
}

impl Chip {
    pub fn new(id: u64, label: impl Into<String>) -> Chip {
        Chip {
            id,
            label: label.into(),
            style: ChipStyle::default(),
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_style(mut self, style: ChipStyle) -> Self {
        self.style = style;
        self
    }

    /// Named `with_selected` rather than `selected` because the getter
    /// upstream's `SelectableChipAttributes` declares has that name, and a
    /// builder and a getter cannot both be `selected`. This is also the shape
    /// every other builder in the crate already has.
    pub fn with_selected(self, selected: bool) -> Self {
        self.with_style(if selected {
            ChipStyle::Selected
        } else {
            ChipStyle::Filter
        })
    }

    /// What pressing it does, as the closure it is.
    ///
    /// [`Chip::wired`] is the convenience over this for the common case of
    /// writing into a state; this is what it builds. A chip with nothing here
    /// is a label rather than a button, and says so to a reader -- see
    /// [`crate::semantics::SemanticsProperties::chip`].
    pub fn on_tap(mut self, on_tap: impl Fn(crate::gestures::TapEvent) + 'static) -> Self {
        self.handlers = PointerHandlers::new().with_tap(on_tap);
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| action(state));
        });
        self
    }
}

impl Component for Chip {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let label = self.label.clone();
        let id = self.id;
        let handlers = self.handlers.clone();
        // Upstream's `RawChip.build` resolves the fill through
        // `ChipTheme.of(context)`: the Material 3 `color` state property
        // first, then the flag-specific colours, then the plain background.
        // The crate's three styles are the states that pick between them.
        let states = match self.style {
            ChipStyle::Selected => crate::widget_state::WidgetStates::NONE
                .with(crate::widget_state::WidgetState::Selected),
            _ => crate::widget_state::WidgetStates::NONE,
        };
        let (default_fill, text_color, border) = match self.style {
            ChipStyle::Filter => (Color::TRANSPARENT, theme.text, Some(theme.outline)),
            ChipStyle::Selected => (
                theme.primary.with_alpha(0x33),
                theme.primary,
                Some(theme.primary),
            ),
            ChipStyle::Action => (theme.surface_variant, theme.text, None),
        };
        // The crate's three styles are this control's own defaults, which is
        // upstream's last fallback: a theme that says nothing leaves them be.
        let chip = crate::component_themes::ResolvedChip::of(context, states, default_fill);
        let fill = chip.fill;
        let border = match chip.side {
            Some(side) => Some((side.width, side.color)),
            None => border.map(|color| (1.0, color)),
        };
        let size = theme.body_size - 1.0;

        // A chip had no semantics at all: it reached a screen reader as a
        // plain box with a word in it, so a filter that is on and one that is
        // off sounded the same. Upstream wraps it in `Semantics(button:
        // tapEnabled, container: true, selected: ..., enabled: ...)`.
        //
        // `tapEnabled` here is "somebody is listening": a chip built with no
        // handler is a label, and announcing it as a button would invite a
        // press that does nothing.
        let tappable = self.handlers.on_tap.is_some();
        let described = {
            let properties = crate::semantics::SemanticsProperties::chip(
                self.label.clone(),
                matches!(self.style, ChipStyle::Selected),
                tappable,
                tappable,
            );
            let tap = self.handlers.on_tap.clone();
            let node = crate::semantics::node_id_for(id);
            move |inner: crate::framework::AnyWidget| {
                // `container: true` needs nothing extra here, and reaching for
                // a merge box is how that goes wrong: wrapping the annotation
                // in one folds its **flags** away, because the folded node
                // takes only labels. A chip carries its own label, so the
                // enclosing-label rule already keeps the `Text` inside it from
                // being read a second time -- one stop, and the flags stay on
                // it. The `Card` needed merging because it labels nothing.
                crate::semantics::tappable(node, properties.clone(), inner, tap.clone())
            }
        };

        let chip_body = leaf(move || {
            let mut container = Container::new()
                .with_height(32.0)
                .with_color(fill)
                .with_corner_radius(16.0)
                .with_padding(EdgeInsets::symmetric(14.0, 0.0))
                .with_child(
                    Align::new(
                        Alignment::CENTER,
                        Text::new(label.clone())
                            .with_size(size)
                            .with_weight(600)
                            .with_color(text_color),
                    )
                    // Shrink-wrapped, the way upstream's chip is sized by its
                    // content rather than by its offer. An `Align` with no
                    // factors fills whatever it is given, and a chip is
                    // normally in a `Wrap`: without this every chip is as wide
                    // as the row and one chip fills a line.
                    //
                    // Only the width factor decides anything -- the container
                    // above fixes the height at 32 -- but both are written,
                    // for the same reason as the button in `components.rs`.
                    .with_factors(Some(1.0), Some(1.0)),
                );
            if let Some((width, color)) = border {
                container = container.with_border(width, color);
            }
            Pointer::new(id, container).with_handlers(handlers.clone())
        });
        described(chip_body)
    }
}

// -- Tabs ---------------------------------------------------------------------

/// A row of tabs with an underline under the selected one.
pub struct TabBar {
    first_id: u64,
    labels: Vec<String>,
    selected: usize,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl TabBar {
    /// This bar's colours and metrics, with the theme and the defaults folded
    /// in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedTabBar {
        crate::component_themes::ResolvedTabBar::of(context)
    }

    /// `first_id` is the hit-test identity of the first tab; the rest follow
    /// consecutively, so a caller reserves a small range rather than a set.
    pub fn new(first_id: u64, labels: Vec<String>, selected: usize) -> TabBar {
        TabBar {
            first_id,
            labels,
            selected,
            handlers: RefCell::new(Vec::new()),
        }
    }

    /// `select` is given the state and the index that was tapped.
    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> TabBar {
        let handlers = (0..self.labels.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for TabBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let handlers = self.handlers.borrow().clone();
        let labels = self.labels.clone();
        // The bar used to size its labels from `theme.body_size` and weight
        // them 700 or 500 by hand, and take its two colours from the older
        // `Theme` -- so a theme that named a label style got nothing, and the
        // five careful steps `ResolvedTabBar` takes to work out the two
        // colours reached nothing either.
        let bar = self.resolved(context);
        let primary = bar.label_color;
        let muted = bar.unselected_label_color;
        let chosen = bar.label_style.clone();
        let quiet = bar.unselected_label_style.clone();
        let outline = theme.outline;
        let size = theme.body_size;

        let count = labels.len();

        leaf(move || {
            // Stretch on the cross axis fills whatever height the bar is
            // offered, which for a top-level tab bar is the whole page. The
            // outer Container below is what turns that into 46.
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, label) in labels.iter().enumerate() {
                let active = index == selected;
                let tab = Container::new().with_height(46.0).with_child(
                    Column::expanded()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push_flex(FlexChild::expanded(
                            Align::new(Alignment::CENTER, {
                                // Upstream draws the label in the
                                // resolved style with the resolved colour
                                // over it: the style says the size, the
                                // weight and the family, and the colour
                                // is worked out separately because a
                                // theme may name it in either place.
                                let role = if active { &chosen } else { &quiet };
                                let ink = if active { primary } else { muted };
                                match role {
                                    Some(style) => Text::new(label.clone()).with_style(TextStyle {
                                        color: ink,
                                        ..style.clone()
                                    }),
                                    None => Text::new(label.clone())
                                        .with_size(size)
                                        .with_weight(if active { 700 } else { 500 })
                                        .with_color(ink),
                                }
                            }),
                            1,
                        ))
                        // The indicator is a child of the tab rather than a
                        // separate positioned layer, so it moves with the
                        // tab's own layout instead of being placed twice.
                        // Two pixels is the default weight upstream's
                        // `TabBar` underlines its tabs with.
                        .push(Container::new().with_height(2.0).with_color(if active {
                            primary
                        } else {
                            outline.with_alpha(0x30)
                        })),
                );
                let region = match handlers.get(index) {
                    Some(handlers) => {
                        Pointer::new(first_id + index as u64, tab).with_handlers(handlers.clone())
                    }
                    None => Pointer::new(first_id + index as u64, tab),
                };
                // One stop per tab, carrying which tab it is and whether it is
                // the current one. The merge is genuinely needed here, unlike
                // the chip's: a tab's words come from the `Text` inside it, so
                // there is no annotation of its own for them to fold into --
                // the folded node is where both the words and the flags meet.
                let region = one_of_many(region, index, count, active);
                row = row.push_flex(FlexChild::expanded(region, 1));
            }
            Container::new().with_height(46.0).with_child(row)
        })
    }
}

// -- Navigation surfaces ------------------------------------------------------

/// One destination in a [`BottomNavigation`] or [`NavigationRail`].
#[derive(Clone, Debug)]
pub struct Destination {
    pub label: String,
    /// A one- or two-character mark standing in for an icon. There is no icon
    /// font yet, and a missing glyph would draw nothing at all.
    pub mark: String,
}

impl Destination {
    pub fn new(label: impl Into<String>, mark: impl Into<String>) -> Destination {
        Destination {
            label: label.into(),
            mark: mark.into(),
        }
    }
}

/// Wraps one choice in a bar of them -- a tab, a navigation destination -- in
/// the node that says **which one it is and whether it is the one you are on**.
///
/// Two bars of choose-one-of-N should not drift into announcing themselves
/// differently, and they had already been written twice word for word by the
/// time this was pulled out. The merge is what carries the flags to the node
/// that holds the words: a choice's words come from the `Text` inside it, so
/// there is no annotation of its own for them to fold into.
fn one_of_many(
    region: impl crate::render::RenderBox + 'static,
    index: usize,
    count: usize,
    active: bool,
) -> crate::render::RenderMergeSemanticsBox {
    crate::render::RenderMergeSemanticsBox::new(region).with_properties(
        crate::semantics::SemanticsProperties::tab(index, count, active),
    )
}

/// A bar of destinations along the bottom.
pub struct BottomNavigation {
    first_id: u64,
    destinations: Vec<Destination>,
    selected: usize,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl BottomNavigation {
    pub fn new(first_id: u64, destinations: Vec<Destination>, selected: usize) -> Self {
        BottomNavigation {
            first_id,
            destinations,
            selected,
            handlers: RefCell::new(Vec::new()),
        }
    }

    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        let handlers = (0..self.destinations.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for BottomNavigation {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let handlers = self.handlers.borrow().clone();
        let destinations = self.destinations.clone();
        let count = destinations.len();
        let surface = theme.surface;
        let outline = theme.outline;
        let primary = theme.primary;
        let muted = theme.text_muted;
        // The bar grows by whatever the gesture bar covers and pads its
        // contents up by the same amount, so the destinations stay reachable
        // and the surface still reaches the bottom edge of the screen.
        // Upstream's `BottomNavigationBar` calls this `additionalBottomPadding`
        // and takes it from `viewPadding` rather than `padding`: the gesture
        // bar is there whether or not a keyboard is over it.
        let bottom = crate::media_query::media_query_of(context)
            .view_padding
            .bottom;

        leaf(move || {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, destination) in destinations.iter().enumerate() {
                let active = index == selected;
                let color = if active { primary } else { muted };
                let item = Container::new().with_child(Center::new(
                    // `MainAxisSize.min` is upstream's own choice here:
                    // `_BottomNavigationTile.build` wraps its icon and label
                    // in `Column(mainAxisSize: MainAxisSize.min)`, and the
                    // `Center` above is what centres the pair in the bar.
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_spacing(3.0)
                        .push(
                            Container::new()
                                .with_size(34.0, 22.0)
                                .with_color(if active {
                                    primary.with_alpha(0x33)
                                } else {
                                    Color::TRANSPARENT
                                })
                                .with_corner_radius(11.0)
                                .with_child(Align::new(
                                    Alignment::CENTER,
                                    Text::new(destination.mark.clone())
                                        .with_size(12.0)
                                        .with_weight(700)
                                        .with_color(color),
                                )),
                        )
                        .push(
                            Text::new(destination.label.clone())
                                .with_size(11.0)
                                .with_weight(if active { 700 } else { 500 })
                                .with_color(color),
                        ),
                ));
                let region = match handlers.get(index) {
                    Some(handlers) => {
                        Pointer::new(first_id + index as u64, item).with_handlers(handlers.clone())
                    }
                    None => Pointer::new(first_id + index as u64, item),
                };
                // The same loss the tab bar had, on the control that is a
                // phone's primary navigation: without `selected`, every
                // destination in the bar makes an identical announcement and a
                // reader cannot tell which page they are on.
                //
                // One stop each, folded, because a destination's words come
                // from the `Text` inside it -- the mark and the label met
                // separately are two things to land on where the screen shows
                // one button.
                let region = one_of_many(region, index, count, active);
                row = row.push_flex(FlexChild::expanded(region, 1));
            }
            Container::new()
                // Upstream's `kBottomNavigationBarHeight`; the safe-area inset
                // rides on top of it.
                .with_height(56.0 + bottom)
                .with_color(surface)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, bottom))
                .with_child(row)
        })
    }
}

/// A column of destinations along the left edge, for wide windows.
pub struct NavigationRail {
    first_id: u64,
    destinations: Vec<Destination>,
    selected: usize,
    extended: bool,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl NavigationRail {
    /// This rail's appearance, with the theme and the M3 defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedNavigationRail {
        crate::component_themes::ResolvedNavigationRail::of(context)
    }

    /// Upstream's constructor assert, run against the resolved label type --
    /// see [`crate::component_themes::ResolvedNavigationRail::check`].
    pub fn check(&self, context: &mut crate::framework::BuildContext) -> Result<(), &'static str> {
        self.resolved(context).check(self.extended)
    }

    pub fn new(first_id: u64, destinations: Vec<Destination>, selected: usize) -> Self {
        NavigationRail {
            first_id,
            destinations,
            selected,
            extended: false,
            handlers: RefCell::new(Vec::new()),
        }
    }

    /// Shows the labels beside the marks rather than under them.
    pub fn extended(mut self, extended: bool) -> Self {
        self.extended = extended;
        self
    }

    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        let handlers = (0..self.destinations.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for NavigationRail {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let extended = self.extended;
        let handlers = self.handlers.borrow().clone();
        let destinations = self.destinations.clone();
        let count = destinations.len();
        let surface = theme.surface;
        let outline = theme.outline;
        let primary = theme.primary;
        let muted = theme.text_muted;
        let spacing = theme.spacing;

        leaf(move || {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(4.0);
            for (index, destination) in destinations.iter().enumerate() {
                let active = index == selected;
                let color = if active { primary } else { muted };
                let mark = Container::new()
                    .with_size(34.0, 30.0)
                    .with_color(if active {
                        primary.with_alpha(0x33)
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_corner_radius(10.0)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(destination.mark.clone())
                            .with_size(12.0)
                            .with_weight(700)
                            .with_color(color),
                    ));
                let content: crate::widgets::BoxedWidget = if extended {
                    crate::render::RenderRef::new(
                        Row::new().with_spacing(spacing).push(mark).push(
                            Text::new(destination.label.clone())
                                .with_size(13.0)
                                .with_weight(if active { 700 } else { 500 })
                                .with_color(color),
                        ),
                    )
                } else {
                    crate::render::RenderRef::new(
                        Column::new().with_spacing(2.0).push(mark).push(
                            Text::new(destination.label.clone())
                                .with_size(10.0)
                                .with_color(color),
                        ),
                    )
                };
                let item = Container::new()
                    .with_padding(EdgeInsets::symmetric(spacing, spacing * 0.75))
                    .with_child(if extended {
                        Align::new(Alignment::CENTER_LEFT, content)
                    } else {
                        Align::new(Alignment::CENTER, content)
                    });
                let region = match handlers.get(index) {
                    Some(handlers) => {
                        Pointer::new(first_id + index as u64, item).with_handlers(handlers.clone())
                    }
                    None => Pointer::new(first_id + index as u64, item),
                };
                // The third bar of choose-one-of-N, and the one that had
                // drifted: it announced four bare stops -- the glyph and the
                // words of each destination, separately, with no sense of
                // which one you are on. The census in `spoken_census` is what
                // caught it, on its first run.
                column = column.push(one_of_many(region, index, count, active));
            }
            Container::new()
                // Upstream `NavigationRail`'s widths: 80 collapsed, 256 with
                // the labels out.
                .with_width(if extended { 256.0 } else { 80.0 })
                .with_color(surface)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::symmetric(0.0, spacing))
                .with_child(column)
        })
    }
}

// -- Overlays -----------------------------------------------------------------

/// A scrim: a translucent sheet over the page that swallows taps.
///
/// Put it under a dialog. Without one a tap goes to whatever is behind the
/// dialog, which is the classic modal bug.
pub struct Scrim {
    id: u64,
    handlers: PointerHandlers,
}

impl Scrim {
    pub fn new(id: u64) -> Scrim {
        Scrim {
            id,
            handlers: PointerHandlers::new(),
        }
    }

    /// Dismisses when tapped.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, dismiss: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| dismiss(state));
        });
        self
    }
}

impl Component for Scrim {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let handlers = self.handlers.clone();
        leaf(move || {
            Pointer::new(id, Container::new().with_color(Color::argb(0x8A, 0, 0, 0)))
                .with_handlers(handlers.clone())
        })
    }
}

/// A modal card. Put it in a `Stack` over a [`Scrim`].
pub struct Dialog {
    title: String,
    body: Option<String>,
    actions: RefCell<Vec<AnyWidget>>,
    width: f32,
    /// Upstream's `semanticLabel`, falling back to "Dialog" rather than
    /// "Alert" -- see [`Dialog::resolved_semantic_label`].
    semantic_label: Option<String>,
}

impl Dialog {
    /// What a screen reader is told this surface is.
    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Dialog {
        self.semantic_label = Some(label.into());
        self
    }

    /// Upstream's `semanticLabel ?? dialogLabel`, with the platform rule in
    /// [`crate::material_app::DefaultMaterialLocalizations::modal_surface_label`].
    ///
    /// **"Dialog" and not "Alert"**: this surface asks rather than interrupts,
    /// the same distinction [`SimpleDialog`] makes and the opposite of
    /// [`AlertDialog`]'s.
    ///
    /// # This is not upstream's `Dialog`
    ///
    /// Upstream's `Dialog` is a bare container -- a shape and a shadow with a
    /// child in it -- and it names no route, because whatever it wraps does:
    /// `AlertDialog` and `SimpleDialog` each add their own. **This one is a
    /// composite**, with a title, a body and actions of its own; nothing wraps
    /// it and nothing in this crate builds one inside another dialog. So it is
    /// a modal surface in its own right and announces itself like one.
    ///
    /// The name is the trap. Comparing the two by name would promise a
    /// container and deliver a dialog -- the "same name, different thing" this
    /// port keeps meeting, written down here rather than left to be
    /// rediscovered.
    pub fn resolved_semantic_label(
        &self,
        platform: crate::editable_text::TargetPlatform,
    ) -> Option<String> {
        use crate::material_app::DefaultMaterialLocalizations as L10n;
        L10n::modal_surface_label(platform, self.semantic_label.as_deref(), L10n::DIALOG_LABEL)
    }

    pub fn new(title: impl Into<String>) -> Dialog {
        Dialog {
            title: title.into(),
            body: None,
            actions: RefCell::new(Vec::new()),
            // Material 3's dialog is 280 across at the least; a wider one is
            // `with_width`'s to ask for.
            width: 280.0,
            semantic_label: None,
        }
    }

    /// This dialog's appearance and placement, with the theme and the M3
    /// defaults folded in -- including the keyboard's insets, which are added
    /// to the margin rather than replacing it.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedDialog {
        crate::component_themes::ResolvedDialog::of(context)
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }
}

impl Component for Dialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let platform = crate::theme::ThemeData::of(context).platform;
        let title = self.title.clone();
        let body = self.body.clone();
        let width = self.width;
        let surface = theme.surface;
        let outline = theme.outline;
        // Material 3's dialog shape: a 28-radius corner all round.
        let radius = 28.0;
        let spacing = theme.spacing;
        // Upstream's `titleTextStyle` and `contentTextStyle`: the theme's,
        // then `headlineSmall` and `bodyMedium`. This used to be
        // `theme.title()` and `theme.muted()` -- styles this crate makes up
        // -- so `DialogThemeData`'s two fields reached nothing and
        // `headlineSmall` had no reader anywhere in the port.
        let dialog = self.resolved(context);
        let title_style = dialog
            .title_text_style
            .clone()
            .unwrap_or_else(|| theme.title());
        let muted = dialog
            .content_text_style
            .clone()
            .unwrap_or_else(|| theme.muted());

        let actions = self.actions.borrow().clone();
        let has_actions = !actions.is_empty();

        let mut children = vec![leaf(move || {
            let mut column = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(spacing);
            column = column.push(Text::new(title.clone()).with_style(title_style.clone()));
            if let Some(body) = &body {
                column = column.push(Text::new(body.clone()).with_style(muted.clone()));
            }
            column
        })];
        children.extend(actions);

        let surface_widget = many(children, move |mut rendered| {
            let header = if rendered.is_empty() {
                crate::render::RenderRef::new(Empty) as crate::widgets::BoxedWidget
            } else {
                rendered.remove(0)
            };
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing * 2.0)
                .push(header);
            if has_actions {
                let mut actions_row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::End)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(spacing);
                for action in rendered {
                    actions_row = actions_row.push(action);
                }
                column = column.push(actions_row);
            }
            Box::new(
                Container::new()
                    .with_width(width)
                    .with_color(surface)
                    .with_corner_radius(radius)
                    // Material 3's dialog elevation. It is over a scrim, and
                    // the shadow is what separates it from what it dims.
                    .with_elevation(6)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::all(spacing * 2.5))
                    .with_child(column),
            )
        });
        // The plain kind, which is upstream's default: `Dialog.semanticsRole`
        // is `SemanticsRole.dialog` unless a subclass says otherwise, and only
        // `AlertDialog` does.
        announced(
            surface_widget,
            self.resolved_semantic_label(platform),
            crate::semantics::SemanticsRole::Dialog,
        )
    }
}

/// The identifier a bottom sheet's drag handle is keyed on.
///
/// Reserved for the same reason [`DIALOG_SEMANTICS_ID`] is: the handle is part
/// of the sheet's furniture rather than a control a caller named, and the
/// platform keys its accessibility node on this.
const DRAG_HANDLE_SEMANTICS_ID: u64 = 0xD_2A6;

/// A panel anchored to the bottom edge.
pub struct BottomSheet {
    title: Option<String>,
    child: RefCell<Option<AnyWidget>>,
    /// Upstream's `enableDrag`, **true by default**: a sheet the reader cannot
    /// push away is the exception and has to be asked for.
    enable_drag: bool,
    /// Upstream's `showDragHandle`. `None` defers to the theme -- but see
    /// [`crate::component_themes::ResolvedBottomSheet`], where the theme's
    /// answer is *and*-ed with `enable_drag` rather than simply taken.
    show_drag_handle: Option<bool>,
}

impl BottomSheet {
    /// This sheet's appearance, with the theme and the defaults folded in.
    ///
    /// `is_modal` is not a field on the sheet because it is not a property of
    /// the sheet: the same sheet shown two ways resolves differently, and which
    /// way it is being shown is the caller's to say.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        is_modal: bool,
    ) -> crate::component_themes::ResolvedBottomSheet {
        crate::component_themes::ResolvedBottomSheet::of(
            context,
            is_modal,
            self.show_drag_handle,
            self.enable_drag,
        )
    }

    pub fn new(child: AnyWidget) -> BottomSheet {
        BottomSheet {
            title: None,
            child: RefCell::new(Some(child)),
            enable_drag: true,
            show_drag_handle: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_enable_drag(mut self, enable_drag: bool) -> Self {
        self.enable_drag = enable_drag;
        self
    }

    /// Upstream's `showDragHandle`, which is the only thing that can put a
    /// handle on a sheet that cannot be dragged -- a caller saying it outright
    /// has taken responsibility for it.
    pub fn with_drag_handle(mut self, show: bool) -> Self {
        self.show_drag_handle = Some(show);
        self
    }
}

impl Component for BottomSheet {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| leaf(|| Empty));
        let surface = theme.surface;
        let outline = theme.outline;
        let spacing = theme.spacing;
        let title_style = theme.title();
        let handle_label =
            crate::material_app::DefaultMaterialLocalizations::MODAL_BARRIER_DISMISS_LABEL
                .to_string();

        crate::framework::single(child, move |inner| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            // The grab handle: a short bar that says the sheet can be dragged,
            // even though dragging it is the caller's to wire up. Thirty-two
            // by four is upstream's drag handle.
            //
            // Upstream wraps it in `Semantics(label: modalBarrierDismissLabel,
            // container: true, button: true, onTap: ...)`, and without that a
            // reader meets a 32-by-4 rectangle with nothing to say -- the one
            // affordance for putting the sheet away is the one thing they
            // cannot find. **A bar that says "you may drag me" says it only to
            // people who can see it**, so the words are what carry the same
            // affordance to everyone else.
            //
            // The label is the barrier's, not a word of its own: dismissing is
            // what the handle does, and upstream reuses the string rather than
            // inventing a second name for one action.
            column = column.push(Box::new(crate::semantics::RenderSemantics::new(
                crate::semantics::node_id_for(DRAG_HANDLE_SEMANTICS_ID),
                crate::semantics::SemanticsProperties {
                    flags: crate::semantics::SemanticsFlags {
                        is_button: true,
                        ..crate::semantics::SemanticsFlags::default()
                    },
                    ..crate::semantics::SemanticsProperties::label(handle_label.clone())
                },
                Align::new(
                    Alignment::CENTER,
                    Container::new()
                        .with_size(32.0, 4.0)
                        .with_color(outline)
                        .with_corner_radius(2.0),
                ),
            )));
            if let Some(title) = &title {
                column = column.push(Box::new(
                    Text::new(title.clone()).with_style(title_style.clone()),
                ));
            }
            column = column.push(inner);
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(theme_sheet_radius())
                    .with_elevation(1)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::only(
                        spacing * 2.0,
                        spacing,
                        spacing * 2.0,
                        spacing * 2.5,
                    ))
                    .with_child(column),
            )
        })
    }
}

/// Sheets are rounded only conceptually at the top; the renderer has one
/// radius, so the bottom corners are rounded too and fall off the screen.
/// Twenty-eight is Material 3's sheet shape; the top-only half of it waits on
/// per-corner radii.
fn theme_sheet_radius() -> f32 {
    28.0
}

/// A brief message along the bottom.
pub struct Snackbar {
    message: String,
    action: Option<String>,
    id: u64,
    handlers: PointerHandlers,
}

impl Snackbar {
    pub fn new(id: u64, message: impl Into<String>) -> Snackbar {
        Snackbar {
            message: message.into(),
            action: None,
            id,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| action(state));
        });
        self
    }

    /// The action's handler as a closure, for one that has to carry something.
    ///
    /// [`Snackbar::wired`] takes a `fn` and not a closure, which is what makes
    /// it short: there is nothing to capture and nothing to allocate. That is
    /// the right trade for an action that flips one named field and the wrong
    /// one for an action that has to reach a `Messenger` -- upstream's own
    /// `SnackBarAction.onPressed` is an arbitrary callback. The same pair
    /// [`Switch::wired`] and [`Switch::with_handlers`] make.
    pub fn on_action(mut self, action: impl Fn() + 'static) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| action());
        self
    }
}

impl Component for Snackbar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let message = self.message.clone();
        let action = self.action.clone();
        let id = self.id;
        let handlers = self.handlers.clone();
        // A snackbar inverts the surface so it reads as separate from the page
        // rather than as part of it.
        let background = theme.text;
        let foreground = theme.background;
        let accent = theme.primary;
        let spacing = theme.spacing;
        let size = theme.body_size - 1.0;

        let bar = leaf(move || {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push_flex(FlexChild::expanded(
                    Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(message.clone())
                            .with_size(size)
                            .with_color(foreground),
                    ),
                    1,
                ));
            if let Some(label) = &action {
                row = row.push(
                    Text::new(label.clone())
                        .with_size(size)
                        .with_weight(700)
                        .with_color(accent),
                );
            }
            Pointer::new(
                id,
                Container::new()
                    // Upstream's snackbar is at least 48 tall with a
                    // 4-radius corner.
                    .with_height(48.0)
                    .with_color(background)
                    .with_corner_radius(4.0)
                    // A snack bar floats over whatever it interrupts; six is
                    // the elevation upstream gives it.
                    .with_elevation(6)
                    .with_padding(EdgeInsets::symmetric(spacing * 2.0, 0.0))
                    .with_child(row),
            )
            .with_handlers(handlers.clone())
        });

        // The announcement is **here, on the widget**, which is where upstream
        // puts it (`_SnackBarState.build` wraps itself in `Semantics(container:
        // true, liveRegion: true, ...)`).
        //
        // It used to be on the messenger's door instead, and that was the
        // wrong place: four of the gallery's demos build a `Snackbar` and push
        // it straight into a column, never touching the messenger, so those
        // bars appeared with nothing to draw a reader's attention. A bar that
        // is gone in four seconds is a bar nobody finds by hunting for it.
        //
        crate::semantics::announces_itself(bar)
    }
}

/// A coloured strip across the top, for something that needs acknowledging.
pub struct Banner {
    message: String,
    actions: RefCell<Vec<AnyWidget>>,
}

impl Banner {
    pub fn new(message: impl Into<String>) -> Banner {
        Banner {
            message: message.into(),
            actions: RefCell::new(Vec::new()),
        }
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }
}

impl Component for Banner {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let message = self.message.clone();
        // Upstream's `MaterialBanner.build`: the fill, the rule under it and
        // the padding come off `MaterialBannerTheme.of(context)` first.
        let banner = crate::component_themes::MaterialBannerTheme::of(context);
        let surface = banner.background_color.unwrap_or(theme.surface_variant);
        let outline = banner.divider_color.unwrap_or(theme.outline);
        let body = theme.body();
        let spacing = theme.spacing;
        let padding = banner
            .padding
            .map(|padding| padding.resolve(crate::direction::current_direction()))
            .unwrap_or(EdgeInsets::symmetric(spacing * 2.0, spacing * 1.5));

        let actions = self.actions.borrow().clone();
        let has_actions = !actions.is_empty();
        let mut children = vec![leaf(move || {
            Text::new(message.clone()).with_style(body.clone())
        })];
        children.extend(actions);

        let banner_body = many(children, move |mut rendered| {
            let text = if rendered.is_empty() {
                crate::render::RenderRef::new(Empty) as crate::widgets::BoxedWidget
            } else {
                rendered.remove(0)
            };
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(spacing)
                .push_flex(FlexChild::expanded(text, 1));
            if has_actions {
                for action in rendered {
                    row = row.push(action);
                }
            }
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_border(1.0, outline)
                    .with_padding(padding)
                    .with_child(row),
            )
        });
        // A banner appears without being asked for, so a reader elsewhere on
        // the page has to be told. It does **not** dismiss itself the way a
        // snack bar does, and it would be reasonable to guess that a permanent
        // thing should not interrupt -- upstream gives it the flag anyway
        // (`banner.dart:443`), and the reason survives the difference. See
        // [`crate::semantics::announces_itself`].
        crate::semantics::announces_itself(banner_body)
    }
}

// -- Collections --------------------------------------------------------------

/// A fixed-column grid.
///
/// Rows are built eagerly, like [`crate::widgets::ListView`]: there is no
/// sliver protocol yet, so a grid of a thousand tiles lays out a thousand
/// tiles.
pub struct GridList {
    columns: usize,
    spacing: f32,
    aspect_ratio: f32,
    children: RefCell<Vec<AnyWidget>>,
}

impl GridList {
    pub fn new(columns: usize) -> GridList {
        GridList {
            columns: columns.max(1),
            spacing: 12.0,
            aspect_ratio: 1.0,
            children: RefCell::new(Vec::new()),
        }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Width divided by height for each tile.
    pub fn with_aspect_ratio(mut self, ratio: f32) -> Self {
        self.aspect_ratio = ratio.max(0.05);
        self
    }

    pub fn push(self, child: AnyWidget) -> Self {
        self.children.borrow_mut().push(child);
        self
    }
}

impl Component for GridList {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let columns = self.columns;
        let spacing = self.spacing;
        let aspect = self.aspect_ratio;
        let children = self.children.borrow().clone();

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);

            let mut tiles = rendered.into_iter().peekable();
            while tiles.peek().is_some() {
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(spacing);
                for _ in 0..columns {
                    // A short last row is padded with empties rather than left
                    // short, so its tiles keep the width the rest have instead
                    // of stretching to fill.
                    let cell = tiles
                        .next()
                        .unwrap_or_else(|| crate::render::RenderRef::new(Empty));
                    row = row.push_flex(FlexChild::expanded(cell, 1));
                }
                column = column.push(Box::new(AspectRow {
                    row,
                    aspect,
                    columns,
                    spacing,
                }));
            }
            Box::new(column)
        })
    }
}

/// A row that reports a height derived from its width and an aspect ratio.
struct AspectRow {
    row: RenderFlex,
    aspect: f32,
    columns: usize,
    spacing: f32,
}

impl crate::render::RenderBox for AspectRow {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> Size {
        // Each cell is (width - gaps) / columns wide, and its height follows
        // from the aspect ratio. That is what makes a grid a grid rather than a
        // column of rows that each pick their own height.
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let gaps = self.spacing * (self.columns.saturating_sub(1)) as f32;
        let cell = ((width - gaps) / self.columns as f32).max(0.0);
        let height = (cell / self.aspect).max(0.0);
        self.row
            .layout(crate::render::BoxConstraints::tight(width, height))
    }

    fn size(&self) -> Size {
        self.row.size()
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        self.row.paint(context, offset);
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.row.hit_test(position, result)
    }
}

/// A table of text, for showing structured data.
pub struct DataTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl DataTable {
    /// This table's metrics, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedDataTable {
        crate::component_themes::ResolvedDataTable::of(context)
    }

    pub fn new(headers: Vec<String>) -> DataTable {
        DataTable {
            headers,
            rows: Vec::new(),
        }
    }

    pub fn push_row(mut self, row: Vec<String>) -> Self {
        self.rows.push(row);
        self
    }
}

impl Component for DataTable {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let headers = self.headers.clone();
        let rows = self.rows.clone();
        let outline = theme.outline;
        let text = theme.text;
        let muted = theme.text_muted;
        let size = theme.body_size - 1.0;
        let spacing = theme.spacing;

        leaf(move || {
            let cell = |content: &str, bold: bool, color: Color| {
                Container::new()
                    .with_padding(EdgeInsets::symmetric(spacing, spacing * 0.9))
                    .with_child(Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(content.to_string())
                            .with_size(size)
                            .with_weight(if bold { 700 } else { 400 })
                            .with_color(color),
                    ))
            };

            let mut table = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

            // Center rather than Stretch: a stretched row takes the height it
            // is offered, and a table in a tall page would make every row as
            // tall as the page.
            let mut header_row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for name in &headers {
                // Upstream's `_buildHeadingCell` opens with
                // `Semantics(role: SemanticsRole.columnHeader, ...)`, and this
                // is the case that shows why a role is not a flag: a header
                // has no state and nothing to press, so no flag has anything
                // to say about it. Without the role a reader meets the word
                // "Name" with no reason to think it names a column -- and a
                // table read as a run of loose words is not a table.
                //
                // A fold rather than an annotation, for the reason round 381
                // gives: the words are the cell's, and a header that said its
                // own would stand a blank stop beside them.
                let header = crate::render::RenderMergeSemanticsBox::new(cell(name, true, muted))
                    .with_properties(crate::semantics::SemanticsProperties {
                        role: crate::semantics::SemanticsRole::ColumnHeader,
                        ..crate::semantics::SemanticsProperties::label("")
                    });
                header_row = header_row.push_flex(FlexChild::expanded(header, 1));
            }
            table = table.push(header_row);
            table = table.push(Container::new().with_height(1.0).with_color(outline));

            for row in &rows {
                let mut data_row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                for index in 0..headers.len() {
                    let value = row.get(index).map(String::as_str).unwrap_or("");
                    data_row = data_row.push_flex(FlexChild::expanded(cell(value, false, text), 1));
                }
                table = table.push(data_row);
                table = table.push(
                    Container::new()
                        .with_height(1.0)
                        .with_color(outline.with_alpha(0x60)),
                );
            }
            table
        })
    }
}

/// The label bubble of a tooltip: a grey pill with the message, in upstream's
/// default decoration (`tooltip.dart`'s `defaultDecoration` -- grey 700 at 90%
/// on a light theme, white at 90% on a dark one).
///
/// The surface half of upstream's `Tooltip` (`material/tooltip.dart`), and only
/// that. The trigger half -- what shows and hides it -- is [`TooltipTrigger`],
/// and the whole assembly, with the bubble in an `OverlayPortal` placed against
/// the target, is [`crate::tooltip::Tooltip`]. Use that one unless you are
/// building the bubble into something of your own.
///
/// It was called `Tooltip` while there was nothing to host it and the caller
/// composed the two halves in their own `Stack`. There is a host now, so the
/// name went to the thing that matches upstream's.
pub struct TooltipBubble {
    message: String,
}

impl TooltipBubble {
    pub fn new(message: impl Into<String>) -> TooltipBubble {
        TooltipBubble {
            message: message.into(),
        }
    }
}

impl Component for TooltipBubble {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // Upstream's default bubble (`tooltip.dart`'s `defaultDecoration` and
        // `defaultTextStyle`): grey 700 at 90% with white text on a light
        // theme, white at 90% with black text on a dark one, corner radius 4,
        // padding 8 across and 4 up and down, the desktop font size 12.
        let brightness = crate::theme::ThemeData::of(context).color_scheme.brightness;
        let message = self.message.clone();
        let (background, foreground) = match brightness {
            crate::platform::Brightness::Dark => (Color::WHITE.with_alpha(0xE6), Color::BLACK),
            crate::platform::Brightness::Light => (
                crate::colors::Colors::GREY
                    .shade(700)
                    .expect("grey has a 700")
                    .with_alpha(0xE6),
                Color::WHITE,
            ),
        };
        leaf(move || {
            Container::new()
                .with_color(background)
                .with_corner_radius(4.0)
                .with_padding(EdgeInsets::symmetric(8.0, 4.0))
                .with_child(
                    Text::new(message.clone())
                        .with_size(12.0)
                        .with_color(foreground),
                )
        })
    }
}

/// Upstream `TooltipTriggerMode`, declared with the thing it describes in
/// [`crate::raw_tooltip`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::raw_tooltip::TooltipTriggerMode;

/// The trigger half of a tooltip: wraps a child and reports when the bubble
/// should be visible. The bubble is [`Tooltip`]; putting the bubble over the
/// child is the application's `Stack`, as with every overlay here.
///
/// Ported from the trigger semantics of `material/tooltip.dart`'s `Tooltip`
/// and `widgets/raw_tooltip.dart`'s `RawTooltipState`: hovering in shows and
/// hovering out hides (`_handleMouseEnter`/`_handleMouseExit`), a long press
/// shows (`_handleLongPress`), and a tap shows in tap mode (`_handleTap`).
///
/// The timers upstream runs on top of those events are not ported, because
/// the frame scheduler has no delayed callback:
///
/// - `waitDuration`/`hoverDelay` defaults to zero upstream, so showing on
///   hover *immediately* is the default behavior, ported as is;
/// - `dismissDelay` (100ms from hover-exit to hide) has no clock to run on,
///   so a hover-exit hides at once;
/// - `touchDelay`/`showDuration` (the 1500ms a touch-shown tooltip lingers)
///   likewise: a touch-shown tooltip stays until the application hides it,
///   which its next tap handler is the usual place to do.
///
/// The exclusivity upstream gets from `_ExclusiveMouseRegion` and the
/// `_openedTooltips` list -- one hovered tooltip at a time -- is likewise the
/// application's: the state these callbacks write holds at most one visible
/// tooltip if the application writes it that way.
pub struct TooltipTrigger {
    id: u64,
    child: RefCell<Option<AnyWidget>>,
    trigger_mode: TooltipTriggerMode,
    on_show: Option<Rc<dyn Fn(bool)>>,
    /// Upstream's `Tooltip.message`, kept here even though the words are
    /// painted by [`TooltipBubble`], because this is the only place they can
    /// be *said*.
    ///
    /// A screen reader neither hovers nor long-presses, so every trigger this
    /// widget offers is a way in the reader does not have. Upstream's answer
    /// is `Semantics(tooltip: message)` on the trigger itself
    /// (`raw_tooltip.dart`), which puts the tip beside the control whether or
    /// not the bubble is on screen. Without it the tip's words exist only for
    /// people who can see them.
    message: Option<String>,
    /// Upstream's `Tooltip.excludeFromSemantics`, defaulting to false.
    ///
    /// It is for an application that says the same thing itself in a better
    /// place -- upstream's doc calls it "going to provide its own custom
    /// semantics label" -- and it exists so that saying it twice is a choice
    /// rather than the only option.
    excluded_from_semantics: bool,
}

impl TooltipTrigger {
    pub fn new(id: u64, child: AnyWidget) -> TooltipTrigger {
        TooltipTrigger {
            id,
            child: RefCell::new(Some(child)),
            trigger_mode: TooltipTriggerMode::default(),
            on_show: None,
            message: None,
            excluded_from_semantics: false,
        }
    }

    /// Upstream's `Tooltip.triggerMode`.
    pub fn with_trigger_mode(mut self, mode: TooltipTriggerMode) -> Self {
        self.trigger_mode = mode;
        self
    }

    /// Upstream's `Tooltip.message`: the words the tip says. See
    /// [`TooltipTrigger::message`] for why the trigger holds them when the
    /// bubble paints them.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Upstream's `Tooltip.excludeFromSemantics`.
    pub fn excluded_from_semantics(mut self, excluded: bool) -> Self {
        self.excluded_from_semantics = excluded;
        self
    }

    /// What a screen reader is told the tip says, or `None` for a trigger with
    /// nothing to add.
    ///
    /// Empty is the same as absent, which is upstream's rule twice over:
    /// `RawTooltip.build` returns the bare child when the message is empty,
    /// and treats a null and an empty string alike when deciding whether to
    /// exclude. A node carrying an empty tip would tell a reader there is
    /// something more to hear and then say nothing.
    pub fn semantic_tooltip(&self) -> Option<&str> {
        if self.excluded_from_semantics {
            return None;
        }
        self.message.as_deref().filter(|words| !words.is_empty())
    }

    /// Runs `show(state, visible)` when the tooltip should appear or
    /// disappear. The application shows a [`Tooltip`] while the state says
    /// visible.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, show: fn(&mut S, bool)) -> Self {
        self.on_show = Some(Rc::new(move |visible| {
            handle.set_state(move |state| show(state, visible));
        }));
        self
    }

    /// The region's handlers for the current trigger mode. Built at build
    /// time rather than in `wired` so that `with_trigger_mode` may come
    /// after it in the chain.
    fn handlers(&self) -> PointerHandlers {
        let Some(on_show) = &self.on_show else {
            return PointerHandlers::new();
        };
        let hover = on_show.clone();
        // Hover shows and hides regardless of the trigger mode; upstream's
        // `hoverDelay` is zero by default, so there is nothing to wait for.
        let mut handlers =
            PointerHandlers::new().with_hover_change(move |hovering| hover(hovering));
        match self.trigger_mode {
            TooltipTriggerMode::LongPress => {
                let show = on_show.clone();
                handlers = handlers.with_long_press(move |_| show(true));
            }
            TooltipTriggerMode::Tap => {
                let show = on_show.clone();
                handlers = handlers.with_tap(move |_| show(true));
            }
            TooltipTriggerMode::Manual => {}
        }
        handlers
    }
}

impl Component for TooltipTrigger {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let handlers = self.handlers();
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| leaf(|| Empty));
        // Upstream wraps the child in a `_ExclusiveMouseRegion` around a
        // `Listener(onPointerDown:)`; the trigger gestures here arrive through
        // the region's own handlers, so the wrapper is the region.
        let triggered = single(child, move |inner| {
            Pointer::new(id, inner).with_handlers(handlers.clone())
        });
        let Some(message) = self.semantic_tooltip() else {
            return triggered;
        };
        // **A fold, not an annotation**, for the reason round 381 learned on
        // the ink well: a tip belongs *to* the control it describes, and an
        // annotation with no words of its own would stand a blank node beside
        // the labelled one rather than adding to it. Upstream reaches the same
        // single node by config merging -- its `Semantics(tooltip: ...)` is
        // not a container.
        let properties = crate::semantics::SemanticsProperties {
            tooltip: message.to_string(),
            ..crate::semantics::SemanticsProperties::label("")
        };
        single(triggered, move |inner| {
            crate::render::RenderMergeSemanticsBox::new(inner).with_properties(properties.clone())
        })
    }
}

/// A circular progress spinner, drawn as an arc that advances with `value`.
pub struct Spinner {
    value: f32,
    size: f32,
    /// Upstream's `semanticsLabel`: what the waiting is *for*.
    semantic_label: Option<String>,
    /// Upstream's `semanticsValue`, which a caller may set even on an
    /// indeterminate indicator -- "step 2 of 5" while the arc spins. It is
    /// **never derived** here; see [`Spinner::semantic_value`].
    semantic_value: Option<String>,
}

impl Spinner {
    /// `value` is 0..1. Feed it from an [`crate::animation::Controller`] set to
    /// loop for an indeterminate spinner.
    pub fn new(value: f32) -> Spinner {
        Spinner {
            value: value.clamp(0.0, 1.0),
            size: 36.0,
            semantic_label: None,
            semantic_value: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Upstream's `semanticsLabel`.
    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Upstream's `semanticsValue`, for a caller who has something to say
    /// about progress that this widget cannot work out.
    pub fn with_semantic_value(mut self, value: impl Into<String>) -> Self {
        self.semantic_value = Some(value.into());
        self
    }

    /// What a reader is told about how far along this is: **only what a caller
    /// said**, and never a number derived from [`Spinner::value`].
    ///
    /// # Why this is not the progress bar's rule
    ///
    /// Upstream's indicators take `value: double?`, and the null is the whole
    /// distinction: null means indeterminate and spins, a number means
    /// determinate and shows an arc. `_buildSemanticsWrapper` branches on
    /// exactly that -- a determinate one sends `'${(value * 100).round()}'`
    /// with bounds beside it, an indeterminate one sends only what the caller
    /// gave.
    ///
    /// **This `Spinner` cannot make that distinction.** Its `value` is not
    /// progress; the constructor's own doc says to feed it from a looping
    /// [`crate::animation::Controller`], so it is the *phase of the rotation*.
    /// Reading it out would announce a spinner as "0", then "37", then "88",
    /// then "4" -- a progress report on nothing, and **worse than silence**,
    /// because a reader would act on it.
    ///
    /// So the determinate branch is not merely unported, it is
    /// **unreachable**: there is no state of this widget that means "60% done".
    /// Giving it one means giving `value` an `Option`, which changes every
    /// caller and is a round of its own.
    pub fn semantic_value(&self) -> Option<&str> {
        self.semantic_value.as_deref()
    }
}

impl Component for Spinner {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let value = self.value;
        let size = self.size;
        let track = theme.surface_variant;
        let fill = theme.primary;
        // A spinner said nothing, so a reader had no way to know the
        // application was busy at all -- and an arc that turns says "wait"
        // only to people who can see it turning.
        //
        // The label and a caller's value; never the phase. See
        // [`Spinner::semantic_value`].
        let properties = crate::semantics::SemanticsProperties {
            value: self.semantic_value().unwrap_or_default().to_string(),
            ..crate::semantics::SemanticsProperties::label(
                self.semantic_label.clone().unwrap_or_default(),
            )
        };
        let arc = leaf(move || ArcSpinner {
            value,
            extent: size,
            track,
            fill,
            laid_out: Size::ZERO,
        });
        crate::framework::single(arc, move |inner| {
            crate::semantics::RenderSemantics::new(
                crate::semantics::node_id_for(SPINNER_SEMANTICS_ID),
                properties.clone(),
                inner,
            )
        })
    }
}

/// The identifier a spinner's semantics node is keyed on. Reserved for the
/// reason [`DIALOG_SEMANTICS_ID`] is: the platform keys its node on this, so
/// it has to be the same on every frame.
const SPINNER_SEMANTICS_ID: u64 = 0x5719;

/// The arc itself. A render object rather than a composition because an arc is
/// one draw call and there is no widget that draws one.
struct ArcSpinner {
    value: f32,
    extent: f32,
    track: Color,
    fill: Color,
    laid_out: Size,
}

impl crate::render::RenderBox for ArcSpinner {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> Size {
        self.laid_out = constraints.constrain(Size::square(self.extent));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        use crate::engine::{Paint, Rect, Style};
        let stroke = (self.extent * 0.11).max(2.0);
        let inset = stroke / 2.0;
        let bounds = Rect::ltrb(
            offset.dx + inset,
            offset.dy + inset,
            offset.dx + self.laid_out.width - inset,
            offset.dy + self.laid_out.height - inset,
        );
        let track_paint = Paint::new(self.track).with_style(Style::Stroke { width: stroke });
        context.canvas().draw_oval(bounds, &track_paint);

        let sweep = 360.0 * self.value;
        if sweep > 0.0 {
            let fill_paint = Paint::new(self.fill)
                .with_style(Style::Stroke { width: stroke })
                .with_stroke_cap(crate::painting::StrokeCap::Round);
            // Twelve o'clock is -90 degrees, which is where a spinner should
            // start rather than at three o'clock.
            context
                .canvas()
                .draw_arc(bounds, -90.0, sweep, false, &fill_paint);
        }
    }
}

/// A vertical spacer that pushes everything after it to the far end of a flex.
pub fn spacer() -> AnyWidget {
    leaf(|| SizedBox::new(0.0, 0.0))
}

/// A titled section: a small caption over some content.
pub struct Section {
    title: String,
    child: RefCell<Option<AnyWidget>>,
}

impl Section {
    pub fn new(title: impl Into<String>, child: AnyWidget) -> Section {
        Section {
            title: title.into(),
            child: RefCell::new(Some(child)),
        }
    }
}

impl Component for Section {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| leaf(|| Empty));
        let spacing = theme.spacing;
        let caption = TextStyle {
            font_size: theme.body_size - 2.0,
            color: theme.text_muted,
            font_weight: 700,
            ..TextStyle::default()
        };

        let heading = leaf(move || Text::new(title.clone()).with_style(caption.clone()));
        many(vec![heading, child], move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(spacing);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        })
    }
}

// -- What a chip can do -------------------------------------------------------

/// Upstream's six chip attribute interfaces (`material/chip.dart`) say what
/// each kind of chip supports, and they exist because upstream has six chip
/// widgets that overlap: `Chip`, `InputChip`, `ChoiceChip`, `FilterChip`,
/// `ActionChip` and `RawChip`. Each declares the combination it implements,
/// and the documentation for a field is written once on the interface rather
/// than six times on the widgets.
///
/// **This crate has one [`Chip`] with a [`ChipStyle`] where upstream has six
/// widgets.** So the interfaces are ported for what they actually are here:
/// the vocabulary that says what a chip *can be asked*, with each style
/// answering for itself. A style that gains an ability implements another
/// trait rather than growing a field nobody can find.
///
/// Every method is a question with a default, because every one of upstream's
/// fields is nullable and null means "the theme decides". A chip only
/// overrides what it has an answer for.
///
/// # The members that are not here
///
/// Upstream's `ChipAttributes` has twenty getters. The ones left out are
/// those with no type in this crate to answer them: `avatar`, `focusNode`,
/// `autofocus`, `visualDensity`, `iconTheme`, `avatarBoxConstraints` and
/// `mouseCursor`. Naming them here is the same choice
/// [`crate::pickers::CalendarDelegate`] makes about its formatting half --
/// adding one later should be an addition, not a correction.
pub trait ChipAttributes {
    /// Upstream's `label`, the only field with no default: a chip without one
    /// is not a chip.
    fn label(&self) -> String;

    fn label_style(&self) -> Option<TextStyle> {
        None
    }

    /// Upstream's `side`: the outline, which for a chip is load-bearing
    /// rather than decorative -- an unfilled chip is *only* its outline.
    fn side(&self) -> Option<crate::borders::BorderSide> {
        None
    }

    fn shape(&self) -> Option<crate::borders::ShapeBorder> {
        None
    }

    fn clip_behavior(&self) -> crate::painting::ClipBehavior {
        crate::painting::ClipBehavior::None
    }

    /// Upstream's `color`: the Material 3 state property, which is consulted
    /// *before* `backgroundColor` and the flag-specific colours. One property
    /// answering for every state is how M3 replaced the four separate colour
    /// fields M2 needed.
    fn color(&self) -> Option<crate::widget_state::StateProperty<Color>> {
        None
    }

    fn background_color(&self) -> Option<Color> {
        None
    }

    fn padding(&self) -> Option<EdgeInsets> {
        None
    }

    fn label_padding(&self) -> Option<EdgeInsets> {
        None
    }

    fn material_tap_target_size(&self) -> Option<crate::widget_state::MaterialTapTargetSize> {
        None
    }

    fn elevation(&self) -> Option<f32> {
        None
    }

    fn shadow_color(&self) -> Option<Color> {
        None
    }

    fn surface_tint_color(&self) -> Option<Color> {
        None
    }

    fn chip_animation_style(&self) -> Option<ChipAnimationStyle> {
        None
    }
}

/// Upstream `DeletableChipAttributes`: a chip with an ✕ on it.
///
/// The tooltip is part of the interface rather than an afterthought, and
/// upstream's reason is worth keeping: the delete affordance is a small
/// target with no label, so the only thing that says *what* it deletes is the
/// tooltip. A chip that is deletable and says nothing about it is one a
/// reader has to guess at.
pub trait DeletableChipAttributes {
    /// Upstream's `onDeleted`. `None` means the chip shows no delete
    /// affordance at all -- the icon is not merely disabled, it is absent,
    /// because an ✕ that does nothing invites a press that does nothing.
    fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
        None
    }

    fn delete_icon_color(&self) -> Option<Color> {
        None
    }

    fn delete_button_tooltip_message(&self) -> Option<String> {
        None
    }

    /// Whether this chip currently offers deletion, which is exactly whether
    /// it has a callback.
    fn is_deletable(&self) -> bool {
        self.on_deleted().is_some()
    }

    /// What the delete affordance actually says, which is upstream's
    /// `deleteButtonTooltipMessage ?? MaterialLocalizations.of(context)
    /// .deleteButtonTooltip` guarded by `_wrapWithTooltip`'s `enabled`.
    ///
    /// The trait doc above has said since it was written that a chip which is
    /// deletable and says nothing about it is one a reader has to guess at.
    /// The message was declared and the fallback never was, so every chip that
    /// did not name its own tooltip had none -- which is the case the doc was
    /// describing.
    ///
    /// Two ways to get nothing, and they are different. A chip with no
    /// `onDeleted` shows no cross at all, so there is nothing to describe; a
    /// disabled chip shows one and upstream deliberately drops the tooltip,
    /// because a tooltip on something that cannot be pressed is an
    /// explanation of an action the reader cannot take.
    fn delete_button_tooltip(&self, is_enabled: bool) -> Option<String> {
        if !is_enabled || !self.is_deletable() {
            return None;
        }
        Some(self.delete_button_tooltip_message().unwrap_or_else(|| {
            crate::material_app::DefaultMaterialLocalizations::DELETE_BUTTON_TOOLTIP.to_string()
        }))
    }
}

/// Upstream `CheckmarkableChipAttributes`: a chip that shows a tick when
/// selected.
///
/// Separate from [`SelectableChipAttributes`] because the two are genuinely
/// different questions. A chip can be selectable and show its selection by
/// colour alone -- upstream's `ChoiceChip` does -- and a filter chip shows a
/// tick because a filter's *set* of selections has to be readable at a
/// glance, where a single choice does not.
pub trait CheckmarkableChipAttributes {
    /// `None` is upstream's "let the theme decide", not "no".
    fn show_checkmark(&self) -> Option<bool> {
        None
    }

    fn checkmark_color(&self) -> Option<Color> {
        None
    }
}

/// Upstream `SelectableChipAttributes`: a chip that is on or off.
pub trait SelectableChipAttributes {
    fn selected(&self) -> bool;

    /// Upstream's `onSelected`. Note it is handed the *new* value rather than
    /// being a bare notification: a selectable chip does not own its own
    /// selection -- whoever holds the filter does -- so the callback has to
    /// say which way it went.
    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        None
    }

    /// How far the chip lifts while pressed. Shared with
    /// [`TappableChipAttributes`], because both are about a press.
    fn press_elevation(&self) -> Option<f32> {
        None
    }

    fn selected_color(&self) -> Option<Color> {
        None
    }

    fn selected_shadow_color(&self) -> Option<Color> {
        None
    }

    fn tooltip(&self) -> Option<String> {
        None
    }
}

/// Upstream `DisabledChipAttributes`: a chip that can be greyed out.
pub trait DisabledChipAttributes {
    /// Upstream's `isEnabled`, which it derives from whether any callback was
    /// given -- a chip with nothing to do is disabled whether or not anyone
    /// said so.
    fn is_enabled(&self) -> bool;

    fn disabled_color(&self) -> Option<Color> {
        None
    }
}

/// Upstream `TappableChipAttributes`: a chip that is a button.
///
/// Distinct from [`SelectableChipAttributes`] because pressing one *does*
/// something and pressing the other *is* something. An action chip that
/// stayed lit after a press would be lying about having a state.
pub trait TappableChipAttributes {
    fn on_pressed(&self) -> Option<Rc<dyn Fn()>> {
        None
    }

    fn press_elevation(&self) -> Option<f32> {
        None
    }

    fn tooltip(&self) -> Option<String> {
        None
    }
}

/// Upstream `ChipAnimationStyle`: the four animations a chip runs, each
/// overridable on its own.
///
/// Four rather than one because they are four different events with four
/// different right speeds: becoming enabled, becoming selected, and the two
/// drawers -- the avatar sliding in and the delete icon sliding out. Upstream's
/// own constants bear that out (195ms to select, 150ms for a drawer opening
/// and 100ms for it closing, 75ms to disable), so a single knob would have to
/// be wrong for three of them.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ChipAnimationStyle {
    pub enable_animation: Option<crate::animation::AnimationStyle>,
    pub select_animation: Option<crate::animation::AnimationStyle>,
    pub avatar_drawer_animation: Option<crate::animation::AnimationStyle>,
    pub delete_drawer_animation: Option<crate::animation::AnimationStyle>,
}

impl ChipAnimationStyle {
    /// Upstream's `_kSelectDuration`.
    pub const SELECT_MICROS: i64 = 195_000;
    /// Upstream's `_kDrawerDuration`, for a drawer opening.
    pub const DRAWER_MICROS: i64 = 150_000;
    /// Upstream's `_kReverseDrawerDuration`. Shorter than opening, which is
    /// the usual asymmetry: a thing arriving is worth watching and a thing
    /// leaving is in the way.
    pub const REVERSE_DRAWER_MICROS: i64 = 100_000;
    /// Upstream's `_kDisableDuration`.
    pub const DISABLE_MICROS: i64 = 75_000;

    pub fn new() -> ChipAnimationStyle {
        ChipAnimationStyle::default()
    }
}

impl ChipAttributes for Chip {
    fn label(&self) -> String {
        self.label.clone()
    }

    /// The crate's three styles are this control's own defaults, which is
    /// upstream's last fallback -- a theme that says nothing leaves them be.
    /// See [`Chip::build`](Component::build).
    fn background_color(&self) -> Option<Color> {
        None
    }
}

impl SelectableChipAttributes for Chip {
    fn selected(&self) -> bool {
        self.style == ChipStyle::Selected
    }
}

impl TappableChipAttributes for Chip {}

impl DisabledChipAttributes for Chip {
    /// Upstream derives this from whether any callback was given, and so does
    /// this: a chip with no tap handler has nothing to do, whatever its style
    /// says.
    fn is_enabled(&self) -> bool {
        !self.handlers.is_empty()
    }
}

// -- The two shaped dialogs ---------------------------------------------------

/// Upstream's `_scalePadding`: how much of its padding a dialog keeps at a
/// given text scale.
///
/// **The padding shrinks as the reader's text grows** -- to a third of itself
/// by the time text is at 2×, and no further. That looks backwards until the
/// alternative is considered: a dialog is a fixed, small box, and padding that
/// stayed put while the text doubled would push the content off the bottom.
/// The room has to come from somewhere, and whitespace is the part a reader
/// who asked for larger text was not asking for.
///
/// Clamped at both ends: below 1× nothing grows (a reader with *smaller* text
/// does not get a roomier dialog than the design), and past 2× nothing shrinks
/// further (a third is as tight as it may get before the text touches the
/// edges).
pub fn scale_dialog_padding(text_scale: f32) -> f32 {
    let clamped = text_scale.clamp(1.0, 2.0);
    1.0 + (1.0 / 3.0 - 1.0) * (clamped - 1.0)
}

/// Upstream `SimpleDialogOption`: one choice in a [`SimpleDialog`].
///
/// It is an ink well with padding and nothing else, and the padding is the
/// point: 24 across and 8 down, so the options run edge to edge of the dialog
/// and the splash does too. An option inset from the dialog's sides would read
/// as a button in a list rather than as a row of the list.
pub struct SimpleDialogOption {
    id: u64,
    /// A *builder* rather than a widget, for the reason [`crate::ink::Ink`]
    /// gives: the ink well rebuilds from the same widget instance whenever its
    /// own state changes, so a child handed over once would be gone on the
    /// second build.
    build_child: Box<dyn Fn() -> AnyWidget>,
    padding: Option<EdgeInsets>,
    on_pressed: Option<Rc<dyn Fn()>>,
}

impl SimpleDialogOption {
    /// Upstream's default padding.
    pub const DEFAULT_PADDING: EdgeInsets = EdgeInsets::symmetric(24.0, 8.0);

    pub fn new(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> SimpleDialogOption {
        SimpleDialogOption {
            id,
            build_child: Box::new(build_child),
            padding: None,
            on_pressed: None,
        }
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(Rc::new(on_pressed));
        self
    }

    pub fn padding(&self) -> EdgeInsets {
        self.padding.unwrap_or(SimpleDialogOption::DEFAULT_PADDING)
    }
}

impl Component for SimpleDialogOption {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let padding = self.padding();
        let child = (self.build_child)();
        // The padding is *inside* the well, so the splash covers the padded
        // row rather than only the label -- an option is a row of the list,
        // and a splash that stopped at the text would read as a button.
        let padded = crate::framework::single(child, move |inner| {
            Box::new(Container::new().with_padding(padding).with_child(inner))
        });
        let padded = RefCell::new(Some(padded));
        let mut well = crate::ink_well::InkWell::new(self.id, move || {
            padded
                .borrow_mut()
                .take()
                .unwrap_or_else(|| leaf(|| crate::widgets::Empty))
        });
        if let Some(on_pressed) = self.on_pressed.clone() {
            well = well.with_on_tap(move || on_pressed());
        }
        crate::framework::stateful(well)
    }
}

/// Upstream `SimpleDialog`: a title and a list of choices.
///
/// The difference from [`AlertDialog`] is what the reader is being asked. An
/// alert states something and offers actions at the bottom; a simple dialog
/// *is* its choices, so they fill the body and there is no action row at all.
pub struct SimpleDialog {
    title: Option<String>,
    children: RefCell<Vec<AnyWidget>>,
    background_color: Option<Color>,
    /// Upstream's `semanticLabel`. What `None` means depends on the platform
    /// -- see [`SimpleDialog::resolved_semantic_label`].
    semantic_label: Option<String>,
}

impl SimpleDialog {
    /// Upstream's `titlePadding`, `EdgeInsets.fromLTRB(24, 24, 24, 0)`. No
    /// bottom inset when there are children: the first option supplies its own
    /// top padding, and two would read as a gap.
    pub const TITLE_PADDING: EdgeInsets = EdgeInsets::only(24.0, 24.0, 24.0, 0.0);
    /// Upstream's `contentPadding`, `EdgeInsets.fromLTRB(0, 12, 0, 16)` --
    /// nothing at the sides, because the options run edge to edge.
    pub const CONTENT_PADDING: EdgeInsets = EdgeInsets::only(0.0, 12.0, 0.0, 16.0);

    pub fn new() -> SimpleDialog {
        SimpleDialog {
            title: None,
            children: RefCell::new(Vec::new()),
            background_color: None,
            semantic_label: None,
        }
    }

    /// Upstream's `SimpleDialog.semanticLabel`.
    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// What a screen reader is told this dialog is, by
    /// [`crate::material_app::DefaultMaterialLocalizations::modal_surface_label`]
    /// -- so an unnamed one is "Dialog" everywhere but iOS and macOS, where it
    /// is nothing.
    pub fn resolved_semantic_label(
        &self,
        platform: crate::editable_text::TargetPlatform,
    ) -> Option<String> {
        use crate::material_app::DefaultMaterialLocalizations as L10n;
        L10n::modal_surface_label(platform, self.semantic_label.as_deref(), L10n::DIALOG_LABEL)
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_child(self, child: AnyWidget) -> Self {
        self.children.borrow_mut().push(child);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Upstream's title padding, scaled -- and with the bottom inset kept
    /// unscaled when there are children, which is upstream's one asymmetry
    /// here: that inset is the gap to the first option, and a gap that shrank
    /// with the text would close up exactly when the text needed separating.
    pub fn title_padding(&self, text_scale: f32) -> EdgeInsets {
        let scale = scale_dialog_padding(text_scale);
        let base = SimpleDialog::TITLE_PADDING;
        let has_children = !self.children.borrow().is_empty();
        EdgeInsets {
            left: base.left * scale,
            right: base.right * scale,
            top: base.top * scale,
            bottom: if has_children {
                base.bottom
            } else {
                base.bottom * scale
            },
        }
    }
}

impl Default for SimpleDialog {
    fn default() -> SimpleDialog {
        SimpleDialog::new()
    }
}

impl Component for SimpleDialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let scale = crate::media_query::current_text_scale();
        let title_padding = self.title_padding(scale);
        let content_padding = SimpleDialog::CONTENT_PADDING;
        let title = self.title.clone();
        let title_style = theme.title();
        let background = self.background_color.unwrap_or(theme.surface);
        let children = self.children.borrow().clone();

        let platform = crate::theme::ThemeData::of(context).platform;
        let dialog = many(children, move |boxed| {
            let mut column = Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start);
            if let Some(title) = &title {
                column = column.push(
                    Container::new()
                        .with_padding(title_padding)
                        .with_child(Text::new(title.clone()).with_style(title_style.clone())),
                );
            }
            let mut body = Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in boxed {
                body = body.push(child);
            }
            column = column.push(
                Container::new()
                    .with_padding(content_padding)
                    .with_child(body),
            );
            Container::new()
                .with_color(background)
                .with_corner_radius(28.0)
                .with_child(column)
        });
        // `SemanticsRole::Dialog`, the plain one. Upstream's `SimpleDialog`
        // returns a bare `Dialog` (`dialog.dart:1372`) and so takes that
        // widget's default -- there is no "simple dialog" role, and the name
        // invites the guess that there is.
        announced(
            dialog,
            self.resolved_semantic_label(platform),
            crate::semantics::SemanticsRole::Dialog,
        )
    }
}

/// Upstream `AlertDialog`: something to say, and what to do about it.
///
/// The shape is upstream's four optional bands -- icon, title, content,
/// actions -- and the ordering rule worth stating is what the *icon* does to
/// the alignment: an alert with an icon centres its title, because the icon is
/// above it and a left-aligned title under a centred icon reads as a mistake.
pub struct AlertDialog {
    title: Option<String>,
    content: Option<String>,
    actions: RefCell<Vec<AnyWidget>>,
    icon: RefCell<Option<AnyWidget>>,
    background_color: Option<Color>,
    /// Upstream's `semanticLabel`, whose fallback is "Alert" rather than
    /// "Dialog" -- see [`AlertDialog::resolved_semantic_label`].
    semantic_label: Option<String>,
}

impl AlertDialog {
    /// Upstream's default `titlePadding` when there is no icon.
    pub const TITLE_PADDING: EdgeInsets = EdgeInsets::only(24.0, 24.0, 24.0, 0.0);
    /// Upstream's default `contentPadding` for Material 3.
    pub const CONTENT_PADDING: EdgeInsets = EdgeInsets::only(24.0, 16.0, 24.0, 24.0);
    /// Upstream's default `actionsPadding` for Material 3.
    pub const ACTIONS_PADDING: EdgeInsets = EdgeInsets::only(24.0, 0.0, 24.0, 24.0);

    pub fn new() -> AlertDialog {
        AlertDialog {
            title: None,
            content: None,
            actions: RefCell::new(Vec::new()),
            icon: RefCell::new(None),
            background_color: None,
            semantic_label: None,
        }
    }

    /// Upstream's `AlertDialog.semanticLabel`.
    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// What a screen reader is told this dialog is. The same rule as
    /// [`SimpleDialog::resolved_semantic_label`] with a different fallback:
    /// **"Alert" rather than "Dialog"**, because an alert interrupts where a
    /// dialog asks, and the reader is told which before hearing the contents.
    pub fn resolved_semantic_label(
        &self,
        platform: crate::editable_text::TargetPlatform,
    ) -> Option<String> {
        use crate::material_app::DefaultMaterialLocalizations as L10n;
        L10n::modal_surface_label(
            platform,
            self.semantic_label.as_deref(),
            L10n::ALERT_DIALOG_LABEL,
        )
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }

    pub fn with_icon(self, icon: AnyWidget) -> Self {
        *self.icon.borrow_mut() = Some(icon);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Whether the title is centred, which upstream ties to having an icon.
    pub fn centres_title(&self) -> bool {
        self.icon.borrow().is_some()
    }

    /// Any of upstream's paddings at the reader's text scale.
    pub fn scaled(padding: EdgeInsets, text_scale: f32) -> EdgeInsets {
        let scale = scale_dialog_padding(text_scale);
        EdgeInsets {
            left: padding.left * scale,
            right: padding.right * scale,
            top: padding.top * scale,
            bottom: padding.bottom * scale,
        }
    }
}

impl Default for AlertDialog {
    fn default() -> AlertDialog {
        AlertDialog::new()
    }
}

impl Component for AlertDialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let platform = crate::theme::ThemeData::of(context).platform;
        let scale = crate::media_query::current_text_scale();
        let title = self.title.clone();
        let content = self.content.clone();
        let title_style = theme.title();
        let body_style = theme.body();
        let background = self.background_color.unwrap_or(theme.surface);
        let centred = self.centres_title();
        let title_padding = AlertDialog::scaled(AlertDialog::TITLE_PADDING, scale);
        let content_padding = AlertDialog::scaled(AlertDialog::CONTENT_PADDING, scale);
        let actions_padding = AlertDialog::scaled(AlertDialog::ACTIONS_PADDING, scale);

        let icon = self.icon.borrow().clone();
        let has_icon = icon.is_some();
        let actions = self.actions.borrow().clone();
        let action_count = actions.len();
        let mut children = Vec::new();
        children.extend(icon);
        children.extend(actions);

        let dialog = many(children, move |mut boxed| {
            let mut boxed = boxed.drain(..);
            let icon = if has_icon { boxed.next() } else { None };
            let actions: Vec<_> = boxed.take(action_count).collect();

            let mut column = Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(if centred {
                    CrossAxisAlignment::Center
                } else {
                    CrossAxisAlignment::Start
                });
            if let Some(icon) = icon {
                column = column.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(24.0, 24.0, 24.0, 16.0))
                        .with_child(icon),
                );
            }
            if let Some(title) = &title {
                column = column.push(
                    Container::new()
                        .with_padding(if has_icon {
                            EdgeInsets::only(24.0, 0.0, 24.0, 0.0)
                        } else {
                            title_padding
                        })
                        .with_child(Text::new(title.clone()).with_style(title_style.clone())),
                );
            }
            if let Some(content) = &content {
                column = column.push(
                    Container::new()
                        .with_padding(content_padding)
                        .with_child(Text::new(content.clone()).with_style(body_style.clone())),
                );
            }
            if !actions.is_empty() {
                // Upstream lays the actions out in an `OverflowBar` so that a
                // pair of long labels stacks rather than overflowing the
                // dialog -- which is precisely the case a small fixed box
                // runs into.
                let mut bar = crate::overflow_bar::OverflowBar::new()
                    .with_spacing(8.0)
                    .with_alignment(crate::render::MainAxisAlignment::End);
                for action in actions {
                    bar = bar.push_boxed(action);
                }
                column = column.push(
                    Container::new()
                        .with_padding(actions_padding)
                        .with_child(bar),
                );
            }
            Container::new()
                .with_color(background)
                .with_corner_radius(28.0)
                .with_child(column)
        });

        // `AlertDialog`, not `Dialog`: upstream's `AlertDialog.build` is the
        // one place that overrides the default, passing
        // `semanticsRole: SemanticsRole.alertDialog` (`dialog.dart:953`). The
        // difference is what the platform does with it -- an alert interrupts,
        // a dialog is somewhere the reader has been moved to.
        announced(
            dialog,
            self.resolved_semantic_label(platform),
            crate::semantics::SemanticsRole::AlertDialog,
        )
    }
}

/// Wraps a modal surface in the node that **announces it**.
///
/// Upstream wraps every dialog in `Semantics(scopesRoute: true,
/// explicitChildNodes: true, namesRoute: true, label: label)`. Without it a
/// reader is handed a page that has silently changed under them, the focus
/// somewhere new, and no word that a modal has opened.
///
/// Two of upstream's four flags carry over and two do not:
///
/// * `namesRoute` and the label **are** the announcement, and both are here.
/// * `explicitChildNodes` is a no-op in this walk. It asks that descendants
///   keep nodes of their own rather than folding into this one, and that is
///   already what happens -- folding happens only where something asks for it
///   ([`crate::render::RenderMergeSemanticsBox`]).
/// * **`scopesRoute` has no counterpart at all**: no field on
///   [`crate::semantics::SemanticsFlags`], no bit in `RfSemanticsNode`,
///   nothing on the engine's side of this branch. It is what tells a platform
///   that focus is now confined to this subtree, so what is missing is a flag
///   *and* its whole crossing -- the same shape as `is_link`, and a round of
///   its own rather than a line here.
///
/// A `None` label is a surface that names no route, which upstream reaches on
/// Apple for a dialog the caller did not label: VoiceOver's focus lands on the
/// title, and saying the label as well is one word too many. The two dialog
/// callers differ only in what they fall back to -- "Alert" against "Dialog" --
/// and that difference belongs to them rather than here.
///
/// # The kind is not the announcement
///
/// `role` is separate from `label`, and **a role alone is enough to make the
/// node**. Upstream puts `role: semanticsRole` on the `Dialog`'s own
/// `Semantics` regardless of what names the route, so a dialog nobody labelled
/// is still a dialog. Folded into the label's `if`, an unlabelled dialog on
/// Apple would have crossed to the platform as an anonymous box -- silent
/// *and* shapeless, where upstream leaves it only silent.
///
/// It is a parameter rather than a constant because not every surface that
/// announces itself is a dialog: a [`BottomSheet`] passes
/// [`crate::semantics::SemanticsRole::None`], which is upstream's answer too
/// (`bottom_sheet.dart` names no role at all).
fn announced(
    surface: AnyWidget,
    label: Option<String>,
    role: crate::semantics::SemanticsRole,
) -> AnyWidget {
    if label.is_none() && !role.is_set() {
        return surface;
    }
    crate::framework::single(surface, move |inner| {
        crate::semantics::RenderSemantics::new(
            crate::semantics::node_id_for(DIALOG_SEMANTICS_ID),
            crate::semantics::SemanticsProperties {
                flags: crate::semantics::SemanticsFlags {
                    names_route: label.is_some(),
                    ..crate::semantics::SemanticsFlags::default()
                },
                role,
                ..crate::semantics::SemanticsProperties::label(label.clone().unwrap_or_default())
            },
            inner,
        )
    })
}

/// The identifier a dialog's own semantics node is keyed on.
///
/// A dialog has no caller-chosen id the way a button does -- it is a surface
/// rather than a control -- so one is reserved here. It has to be **stable**:
/// the platform keys its accessibility node on it, and an id that changed
/// between frames would be, to a screen reader, a new dialog appearing every
/// frame.
const DIALOG_SEMANTICS_ID: u64 = 0xD1A_106;

// -- The four chip variants ---------------------------------------------------

/// The fields every chip variant carries, so the four below differ only in
/// what they *implement* rather than in what they store.
///
/// Upstream repeats the whole field list in each of the four files, because a
/// Dart class implementing an interface has to declare every member of it.
/// The interfaces are the shared vocabulary and this is the shared storage;
/// keeping them apart is what makes the four variants readable as four
/// *combinations* rather than four near-copies.
#[derive(Default)]
pub struct ChipParts {
    pub label: String,
    pub selected: bool,
    pub enabled: bool,
    pub show_checkmark: Option<bool>,
    pub background_color: Option<Color>,
    pub selected_color: Option<Color>,
    pub disabled_color: Option<Color>,
    pub tooltip: Option<String>,
    pub press_elevation: Option<f32>,
    pub on_pressed: Option<Rc<dyn Fn()>>,
    pub on_selected: Option<Rc<dyn Fn(bool)>>,
    pub on_deleted: Option<Rc<dyn Fn()>>,
    /// Upstream's `.elevated` constructors, which are a *shape* rather than a
    /// number: an elevated chip sits on its own surface instead of being an
    /// outline on the page. The four variants each have one.
    pub elevated: bool,
}

impl ChipParts {
    fn new(label: impl Into<String>) -> ChipParts {
        ChipParts {
            label: label.into(),
            enabled: true,
            ..ChipParts::default()
        }
    }
}

/// Upstream `ActionChip` (`material/action_chip.dart`): a chip that *does*
/// something.
///
/// Implements [`ChipAttributes`], [`TappableChipAttributes`] and
/// [`DisabledChipAttributes`] -- and notably **not**
/// [`SelectableChipAttributes`]. That is the distinction the taxonomy exists
/// to make: an action chip has no state to be in. Pressing it starts
/// something and it looks the same afterwards, so a lit-up action chip would
/// be claiming a state it does not have.
pub struct ActionChip(pub ChipParts);

impl ActionChip {
    pub fn new(label: impl Into<String>) -> ActionChip {
        ActionChip(ChipParts::new(label))
    }

    /// Upstream's `ActionChip.elevated`.
    pub fn elevated(label: impl Into<String>) -> ActionChip {
        ActionChip(ChipParts {
            elevated: true,
            ..ChipParts::new(label)
        })
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.0.on_pressed = Some(Rc::new(on_pressed));
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.0.tooltip = Some(tooltip.into());
        self
    }
}

impl ChipAttributes for ActionChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl TappableChipAttributes for ActionChip {
    fn on_pressed(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_pressed.clone()
    }

    fn press_elevation(&self) -> Option<f32> {
        self.0.press_elevation
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl DisabledChipAttributes for ActionChip {
    /// Upstream derives this from `onPressed != null`, so an action chip with
    /// nothing to do is disabled whatever anyone said.
    fn is_enabled(&self) -> bool {
        self.0.enabled && self.0.on_pressed.is_some()
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

/// Upstream `ChoiceChip` (`material/choice_chip.dart`): one of a set, of which
/// the reader picks exactly one.
///
/// Selectable and checkmarkable but **not deletable**: a choice is one of a
/// fixed set, so there is nothing to remove -- picking another is how you stop
/// picking this one.
pub struct ChoiceChip(pub ChipParts);

impl ChoiceChip {
    pub fn new(label: impl Into<String>) -> ChoiceChip {
        ChoiceChip(ChipParts::new(label))
    }

    pub fn elevated(label: impl Into<String>) -> ChoiceChip {
        ChoiceChip(ChipParts {
            elevated: true,
            ..ChipParts::new(label)
        })
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.0.selected = selected;
        self
    }

    pub fn with_on_selected(mut self, on_selected: impl Fn(bool) + 'static) -> Self {
        self.0.on_selected = Some(Rc::new(on_selected));
        self
    }
}

impl ChipAttributes for ChoiceChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl SelectableChipAttributes for ChoiceChip {
    fn selected(&self) -> bool {
        self.0.selected
    }

    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        self.0.on_selected.clone()
    }

    fn selected_color(&self) -> Option<Color> {
        self.0.selected_color
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl CheckmarkableChipAttributes for ChoiceChip {
    /// Upstream's default for a choice chip is **no tick**, unlike a filter
    /// chip's: one choice out of a set is already told apart by its colour,
    /// and a tick on the single chosen one is a second way of saying the same
    /// thing.
    fn show_checkmark(&self) -> Option<bool> {
        self.0.show_checkmark.or(Some(false))
    }
}

impl DisabledChipAttributes for ChoiceChip {
    fn is_enabled(&self) -> bool {
        self.0.enabled && self.0.on_selected.is_some()
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

/// Upstream `FilterChip` (`material/filter_chip.dart`): one of a set, of which
/// the reader picks any number.
///
/// The same traits as a [`ChoiceChip`] plus [`DeletableChipAttributes`]. The
/// difference from a choice chip is not the widget but the *set*: several
/// filters can be on at once, so the reader has to be able to read which at a
/// glance -- which is why the tick is on by default here and off there.
pub struct FilterChip(pub ChipParts);

impl FilterChip {
    pub fn new(label: impl Into<String>) -> FilterChip {
        FilterChip(ChipParts::new(label))
    }

    pub fn elevated(label: impl Into<String>) -> FilterChip {
        FilterChip(ChipParts {
            elevated: true,
            ..ChipParts::new(label)
        })
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.0.selected = selected;
        self
    }

    pub fn with_on_selected(mut self, on_selected: impl Fn(bool) + 'static) -> Self {
        self.0.on_selected = Some(Rc::new(on_selected));
        self
    }

    pub fn with_on_deleted(mut self, on_deleted: impl Fn() + 'static) -> Self {
        self.0.on_deleted = Some(Rc::new(on_deleted));
        self
    }
}

impl ChipAttributes for FilterChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl SelectableChipAttributes for FilterChip {
    fn selected(&self) -> bool {
        self.0.selected
    }

    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        self.0.on_selected.clone()
    }

    fn selected_color(&self) -> Option<Color> {
        self.0.selected_color
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl CheckmarkableChipAttributes for FilterChip {
    /// Unset here means the theme decides, which for Material 3 means a tick.
    /// Several filters can be on at once, so the set has to be readable at a
    /// glance and colour alone is not enough.
    fn show_checkmark(&self) -> Option<bool> {
        self.0.show_checkmark
    }
}

impl DeletableChipAttributes for FilterChip {
    fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_deleted.clone()
    }
}

impl DisabledChipAttributes for FilterChip {
    fn is_enabled(&self) -> bool {
        self.0.enabled && self.0.on_selected.is_some()
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

/// Upstream `InputChip` (`material/input_chip.dart`): something the reader
/// themselves put there.
///
/// The only variant implementing **all six** interfaces, and that is the
/// point: an input chip is a piece of the reader's own input -- a recipient on
/// an email, a tag they typed -- so it can be pressed, chosen, ticked, deleted
/// and disabled. The other three are each a *subset* of it, which is why the
/// taxonomy is worth having rather than one chip with every field.
pub struct InputChip(pub ChipParts);

impl InputChip {
    pub fn new(label: impl Into<String>) -> InputChip {
        InputChip(ChipParts::new(label))
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.0.selected = selected;
        self
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.0.on_pressed = Some(Rc::new(on_pressed));
        self
    }

    pub fn with_on_selected(mut self, on_selected: impl Fn(bool) + 'static) -> Self {
        self.0.on_selected = Some(Rc::new(on_selected));
        self
    }

    pub fn with_on_deleted(mut self, on_deleted: impl Fn() + 'static) -> Self {
        self.0.on_deleted = Some(Rc::new(on_deleted));
        self
    }

    /// Upstream's `isEnabled`, which an input chip carries outright rather
    /// than deriving -- it is the one variant that can be meaningfully
    /// present and inert, because it is showing something the reader typed
    /// whether or not it can still be acted on.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.0.enabled = enabled;
        self
    }
}

impl ChipAttributes for InputChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl SelectableChipAttributes for InputChip {
    fn selected(&self) -> bool {
        self.0.selected
    }

    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        self.0.on_selected.clone()
    }

    fn selected_color(&self) -> Option<Color> {
        self.0.selected_color
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl CheckmarkableChipAttributes for InputChip {
    fn show_checkmark(&self) -> Option<bool> {
        self.0.show_checkmark
    }
}

impl DeletableChipAttributes for InputChip {
    fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_deleted.clone()
    }
}

impl TappableChipAttributes for InputChip {
    fn on_pressed(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_pressed.clone()
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl DisabledChipAttributes for InputChip {
    /// Carried, not derived -- see [`InputChip::with_enabled`].
    fn is_enabled(&self) -> bool {
        self.0.enabled
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::framework::{ElementTree, component, provide};
    use crate::render::{BoxConstraints, RenderBox};

    #[test]
    fn a_chip_is_as_wide_as_its_label_and_not_as_wide_as_the_row() {
        // A chip normally sits in a `Wrap`. Sized by its offer rather than its
        // content, every chip is as wide as the row and one chip fills a line.
        //
        // The port centred its label with an `Align` and no factors, which
        // fills whatever it is given -- the same shape the button had, found
        // by looking for it after the button turned up twice.
        let width_at = |offer: f32| {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(Theme::dark(), component(Chip::new(1, "hi"))));
            let root = tree.build_render_tree().expect("a root");
            crate::render::schedule_root_layout(&root, BoxConstraints::loose(offer, 200.0));
            crate::render::flush_layout();
            root.size().width
        };

        let wide = width_at(400.0);
        let narrow = width_at(120.0);
        assert_eq!(wide, narrow, "the offer does not decide the width");
        assert!(wide < 120.0, "and it is the label's own width: {wide}");

        // A longer label does widen it, which is what says the width comes
        // from the content rather than from a constant.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            component(Chip::new(1, "a much longer label")),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 200.0));
        crate::render::flush_layout();
        assert!(root.size().width > wide);
    }

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    #[test]
    fn a_checkbox_lays_out_at_a_sensible_size() {
        let size = lay_out(component(Checkbox::new(1, true)), 200.0, 200.0);
        assert!(size.width > 20.0 && size.width < 60.0, "{size:?}");
        assert!(size.height > 20.0 && size.height < 60.0, "{size:?}");
    }

    #[test]
    fn a_labelled_checkbox_is_wider_than_a_bare_one() {
        let bare = lay_out(component(Checkbox::new(1, false)), 300.0, 100.0);
        let labelled = lay_out(
            component(Checkbox::new(1, false).with_label("Remember me")),
            300.0,
            100.0,
        );
        assert!(labelled.width > bare.width);
    }

    #[test]
    fn a_tab_bar_fills_its_width() {
        let size = lay_out(
            component(TabBar::new(
                1,
                vec!["One".into(), "Two".into(), "Three".into()],
                0,
            )),
            400.0,
            200.0,
        );
        assert_eq!(size.width, 400.0);
        assert!(size.height > 40.0 && size.height < 60.0, "{size:?}");
    }

    #[test]
    fn a_grid_gives_every_row_the_same_height() {
        // Four tiles in two columns at 1:1 in a 200 wide box: two rows, each
        // (200 - 12) / 2 = 94 tall, plus the gap between them.
        let grid = GridList::new(2)
            .with_spacing(12.0)
            .push(leaf(|| Empty))
            .push(leaf(|| Empty))
            .push(leaf(|| Empty))
            .push(leaf(|| Empty));
        let size = lay_out(component(grid), 200.0, 1000.0);
        assert!((size.height - (94.0 * 2.0 + 12.0)).abs() < 1.0, "{size:?}");
    }

    #[test]
    fn a_short_last_row_keeps_the_tile_width() {
        // Three tiles in two columns: the second row has one tile and a gap,
        // and the row is still two rows tall.
        let grid = GridList::new(2)
            .with_spacing(12.0)
            .push(leaf(|| Empty))
            .push(leaf(|| Empty))
            .push(leaf(|| Empty));
        let size = lay_out(component(grid), 200.0, 1000.0);
        assert!((size.height - (94.0 * 2.0 + 12.0)).abs() < 1.0, "{size:?}");
    }

    #[test]
    fn a_data_table_grows_with_its_rows() {
        let one = lay_out(
            component(
                DataTable::new(vec!["A".into(), "B".into()]).push_row(vec!["1".into(), "2".into()]),
            ),
            300.0,
            1000.0,
        );
        let two = lay_out(
            component(
                DataTable::new(vec!["A".into(), "B".into()])
                    .push_row(vec!["1".into(), "2".into()])
                    .push_row(vec!["3".into(), "4".into()]),
            ),
            300.0,
            1000.0,
        );
        assert!(two.height > one.height);
    }

    #[test]
    fn a_spinner_is_square() {
        let size = lay_out(component(Spinner::new(0.4).with_size(48.0)), 200.0, 200.0);
        assert_eq!(size, Size::square(48.0));
    }

    #[test]
    fn a_navigation_rail_is_wider_when_extended() {
        let destinations = vec![
            Destination::new("Home", "H"),
            Destination::new("Settings", "S"),
        ];
        let narrow = lay_out(
            component(NavigationRail::new(1, destinations.clone(), 0)),
            400.0,
            400.0,
        );
        let wide = lay_out(
            component(NavigationRail::new(1, destinations, 0).extended(true)),
            400.0,
            400.0,
        );
        assert!(wide.width > narrow.width);
    }

    #[test]
    fn a_tooltip_trigger_passes_its_childs_size_through() {
        let trigger = TooltipTrigger::new(1, leaf(|| crate::widgets::SizedBox::new(30.0, 20.0)));
        let size = lay_out(component(trigger), 200.0, 200.0);
        assert_eq!(size, Size::new(30.0, 20.0));
    }

    #[test]
    fn hovering_shows_and_unhovering_hides() {
        let shown = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let record = shown.clone();
        let mut trigger = TooltipTrigger::new(1, leaf(|| Empty));
        trigger.on_show = Some(std::rc::Rc::new(move |visible| {
            record.borrow_mut().push(visible)
        }));
        let handlers = trigger.handlers();
        let on_hover = handlers
            .on_hover_change
            .clone()
            .expect("hover is always wired");
        on_hover(true);
        on_hover(false);
        assert_eq!(*shown.borrow(), vec![true, false]);
    }

    #[test]
    fn the_trigger_mode_picks_the_touch_gesture() {
        let mut trigger = TooltipTrigger::new(1, leaf(|| Empty));
        trigger.on_show = Some(std::rc::Rc::new(|_| {}));
        // Upstream's default, `_defaultTriggerMode`, is longPress.
        let handlers = trigger.handlers();
        assert!(handlers.on_long_press.is_some());
        assert!(handlers.on_tap.is_none());

        let trigger = trigger.with_trigger_mode(TooltipTriggerMode::Tap);
        let handlers = trigger.handlers();
        assert!(handlers.on_tap.is_some());
        assert!(handlers.on_long_press.is_none());

        // Manual wires no touch gesture, but hover is wired either way --
        // upstream's triggerMode "does not affect mouse devices".
        let trigger = trigger.with_trigger_mode(TooltipTriggerMode::Manual);
        let handlers = trigger.handlers();
        assert!(handlers.on_tap.is_none());
        assert!(handlers.on_long_press.is_none());
        assert!(handlers.on_hover_change.is_some());
    }

    #[test]
    fn an_unwired_trigger_has_no_handlers() {
        let trigger = TooltipTrigger::new(1, leaf(|| Empty));
        assert!(trigger.handlers().is_empty());
    }

    #[test]
    fn a_chip_answers_its_label_and_leaves_the_rest_to_the_theme() {
        // Every one of upstream's fields is nullable and null means "the
        // theme decides"; `label` is the only one with no default, because a
        // chip without one is not a chip.
        let chip = Chip::new(1, "Filter");
        assert_eq!(chip.label(), "Filter");
        assert_eq!(chip.background_color(), None);
        assert_eq!(chip.side(), None);
        assert_eq!(chip.elevation(), None);
        assert_eq!(chip.chip_animation_style(), None);
    }

    #[test]
    fn the_style_is_what_answers_for_selection() {
        // Upstream splits this across six widgets; here one chip with a style
        // answers, and `ChipStyle::Selected` is what makes it selected.
        assert!(!Chip::new(1, "x").with_selected(false).selected());
        assert!(Chip::new(1, "x").with_selected(true).selected());
        assert!(!Chip::new(1, "x").with_style(ChipStyle::Action).selected());
    }

    #[test]
    fn a_chip_with_nothing_to_do_is_disabled_whatever_its_style_says() {
        // Upstream derives `isEnabled` from whether any callback was given,
        // not from a flag -- so a chip nobody wired up is greyed out even if
        // it looks like an action.
        let inert = Chip::new(1, "x").with_style(ChipStyle::Action);
        assert!(!inert.is_enabled());

        struct Nothing;
        let handle: StateHandle<Nothing> = StateHandle::detached();
        let wired = Chip::new(1, "x").wired(handle, |_: &mut Nothing| {});
        assert!(wired.is_enabled());
    }

    #[test]
    fn deleting_is_absent_rather_than_disabled_when_there_is_no_callback() {
        // An x that does nothing invites a press that does nothing, so
        // upstream shows no delete affordance at all rather than a dead one.
        struct Undeletable;
        impl DeletableChipAttributes for Undeletable {}
        assert!(!Undeletable.is_deletable());

        struct Deletable;
        impl DeletableChipAttributes for Deletable {
            fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
                Some(Rc::new(|| {}))
            }
        }
        assert!(Deletable.is_deletable());
    }

    #[test]
    fn a_checkmark_left_unset_means_the_theme_decides_not_no() {
        // The distinction matters: `Some(false)` is a chip saying "do not
        // show one", and `None` is a chip saying nothing at all. A port that
        // collapsed them would make every filter chip's tick untheme-able.
        struct Quiet;
        impl CheckmarkableChipAttributes for Quiet {}
        assert_eq!(Quiet.show_checkmark(), None);

        struct Refuses;
        impl CheckmarkableChipAttributes for Refuses {
            fn show_checkmark(&self) -> Option<bool> {
                Some(false)
            }
        }
        assert_eq!(Refuses.show_checkmark(), Some(false));
        assert_ne!(Quiet.show_checkmark(), Refuses.show_checkmark());
    }

    #[test]
    fn selection_is_reported_with_the_value_it_became() {
        // A selectable chip does not own its own selection -- whoever holds
        // the filter does -- so the callback has to say which way it went
        // rather than merely that something happened.
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        struct Filter(Rc<dyn Fn(bool)>);
        impl SelectableChipAttributes for Filter {
            fn selected(&self) -> bool {
                false
            }
            fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
                Some(Rc::clone(&self.0))
            }
        }
        let sink = Rc::clone(&seen);
        let chip = Filter(Rc::new(move |value| sink.borrow_mut().push(value)));
        let callback = chip.on_selected().expect("a callback");
        callback(true);
        callback(false);
        assert_eq!(*seen.borrow(), vec![true, false]);
    }

    #[test]
    fn the_four_chip_animations_are_four_different_speeds() {
        // Which is why upstream has four knobs rather than one: a single one
        // would have to be wrong for three of them.
        assert_eq!(ChipAnimationStyle::SELECT_MICROS, 195_000);
        assert_eq!(ChipAnimationStyle::DRAWER_MICROS, 150_000);
        assert_eq!(ChipAnimationStyle::REVERSE_DRAWER_MICROS, 100_000);
        assert_eq!(ChipAnimationStyle::DISABLE_MICROS, 75_000);
        // A drawer closes faster than it opens: a thing arriving is worth
        // watching and a thing leaving is in the way.
        assert!(ChipAnimationStyle::REVERSE_DRAWER_MICROS < ChipAnimationStyle::DRAWER_MICROS);
        // And all four are distinct, which is the claim.
        let all = [
            ChipAnimationStyle::SELECT_MICROS,
            ChipAnimationStyle::DRAWER_MICROS,
            ChipAnimationStyle::REVERSE_DRAWER_MICROS,
            ChipAnimationStyle::DISABLE_MICROS,
        ];
        for (index, one) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(one, other);
            }
        }
    }

    #[test]
    fn each_animation_is_overridable_on_its_own() {
        // Four `Option`s rather than one style, so a caller who only cares
        // about the select animation does not have to restate the other three.
        let style = ChipAnimationStyle {
            select_animation: Some(crate::animation::AnimationStyle::NO_ANIMATION),
            ..ChipAnimationStyle::new()
        };
        assert!(style.select_animation.is_some());
        assert!(style.enable_animation.is_none());
        assert!(style.avatar_drawer_animation.is_none());
        assert!(style.delete_drawer_animation.is_none());
    }

    #[test]
    fn a_dialogs_padding_shrinks_as_the_readers_text_grows() {
        // Backwards until the alternative is considered: a dialog is a fixed,
        // small box, and padding that stayed put while the text doubled would
        // push the content off the bottom. The room has to come from
        // somewhere, and whitespace is the part a reader who asked for larger
        // text was not asking for.
        assert_eq!(scale_dialog_padding(1.0), 1.0, "the design's own padding");
        assert!((scale_dialog_padding(2.0) - 1.0 / 3.0).abs() < 0.001);
        assert!(
            scale_dialog_padding(1.5) < scale_dialog_padding(1.0),
            "and it only ever shrinks"
        );
    }

    #[test]
    fn the_padding_scale_is_clamped_at_both_ends() {
        // Below 1x nothing grows -- a reader with *smaller* text does not get
        // a roomier dialog than the design. Past 2x nothing shrinks further,
        // because a third is as tight as it may get before the text touches
        // the edges.
        assert_eq!(scale_dialog_padding(0.5), 1.0);
        assert_eq!(scale_dialog_padding(0.0), 1.0);
        assert_eq!(scale_dialog_padding(3.0), scale_dialog_padding(2.0));
        assert_eq!(scale_dialog_padding(100.0), scale_dialog_padding(2.0));
    }

    #[test]
    fn a_simple_dialogs_title_keeps_its_gap_to_the_first_option() {
        // Upstream's one asymmetry here: the bottom inset is the gap to the
        // first option, and a gap that shrank with the text would close up
        // exactly when the text needed separating. So it is *not* scaled --
        // but only when there is something below it to separate from.
        let with_options = SimpleDialog::new()
            .with_title("Pick one")
            .with_child(leaf(|| crate::widgets::Empty));
        let scaled = with_options.title_padding(2.0);
        assert!(
            scaled.top < SimpleDialog::TITLE_PADDING.top,
            "the top shrank"
        );
        assert_eq!(
            scaled.bottom,
            SimpleDialog::TITLE_PADDING.bottom,
            "the gap below did not"
        );

        // With nothing below, the bottom is scaled like the rest -- there is
        // no gap to protect.
        let bare = SimpleDialog::new().with_title("Nothing here");
        let bare_scaled = bare.title_padding(2.0);
        assert!(
            (bare_scaled.bottom - SimpleDialog::TITLE_PADDING.bottom * scale_dialog_padding(2.0))
                .abs()
                < 0.001
        );
    }

    #[test]
    fn a_simple_dialogs_options_run_edge_to_edge() {
        // Its content padding has nothing at the sides, and the option
        // supplies its own 24 -- so the splash reaches the dialog's edges. An
        // option inset from the sides would read as a button in a list rather
        // than as a row of it.
        assert_eq!(SimpleDialog::CONTENT_PADDING.left, 0.0);
        assert_eq!(SimpleDialog::CONTENT_PADDING.right, 0.0);
        assert_eq!(SimpleDialogOption::DEFAULT_PADDING.left, 24.0);
        assert_eq!(SimpleDialogOption::DEFAULT_PADDING.right, 24.0);
    }

    #[test]
    fn an_icon_is_what_centres_an_alert_dialogs_title() {
        // A left-aligned title under a centred icon reads as a mistake, so
        // upstream ties the two together rather than offering the alignment
        // separately.
        assert!(!AlertDialog::new().with_title("Delete?").centres_title());
        assert!(
            AlertDialog::new()
                .with_title("Delete?")
                .with_icon(leaf(|| crate::widgets::Empty))
                .centres_title()
        );
    }

    #[test]
    fn every_alert_padding_scales_the_same_way() {
        // One rule over all of them, so a dialog does not go lopsided at a
        // large text size.
        for base in [
            AlertDialog::TITLE_PADDING,
            AlertDialog::CONTENT_PADDING,
            AlertDialog::ACTIONS_PADDING,
        ] {
            let scaled = AlertDialog::scaled(base, 2.0);
            let factor = scale_dialog_padding(2.0);
            assert!((scaled.left - base.left * factor).abs() < 0.001);
            assert!((scaled.top - base.top * factor).abs() < 0.001);
            assert!((scaled.right - base.right * factor).abs() < 0.001);
            assert!((scaled.bottom - base.bottom * factor).abs() < 0.001);
        }
        // And at the default scale nothing moves at all.
        assert_eq!(
            AlertDialog::scaled(AlertDialog::CONTENT_PADDING, 1.0),
            AlertDialog::CONTENT_PADDING
        );
    }

    #[test]
    fn a_simple_dialog_option_builds_its_child_through_the_builder() {
        // The ink well rebuilds from the same widget instance whenever its own
        // state changes -- a splash is state -- so a child handed over once
        // would be gone on the second build. Mounting it proves the builder is
        // what the child comes through.
        let built = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = std::rc::Rc::clone(&built);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::framework::provide(
            Theme::dark(),
            component(SimpleDialogOption::new(1, move || {
                counter.set(counter.get() + 1);
                leaf(|| Container::new().with_size(100.0, 20.0))
            })),
        ));
        assert!(built.get() >= 1, "the builder ran");
        assert!(tree.build_render_tree().is_some(), "and produced a tree");
    }

    #[test]
    fn an_action_chip_has_no_state_to_be_in() {
        // The distinction the taxonomy exists to make. Pressing an action chip
        // starts something and it looks the same afterwards, so it implements
        // `TappableChipAttributes` and *not* `SelectableChipAttributes` -- a
        // lit-up action chip would be claiming a state it does not have.
        //
        // Written as a compile-time check: a function that only accepts
        // selectable chips would not take this one, and adding the trait to
        // `ActionChip` later would make this test's *absence* of a call the
        // only thing that noticed.
        fn only_selectable<T: SelectableChipAttributes>(chip: &T) -> bool {
            chip.selected()
        }
        assert!(!only_selectable(&ChoiceChip::new("A").with_selected(false)));
        assert!(!only_selectable(&FilterChip::new("B")));
        assert!(!only_selectable(&InputChip::new("C")));
        // And the action chip is tappable, which the other question is about.
        let pressed = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = std::rc::Rc::clone(&pressed);
        let action = ActionChip::new("Do it").with_on_pressed(move || flag.set(true));
        action.on_pressed().expect("a callback")();
        assert!(pressed.get());
    }

    #[test]
    fn an_input_chip_is_the_only_one_that_is_all_six() {
        // Which is the point: an input chip is a piece of the reader's own
        // input, so it can be pressed, chosen, ticked, deleted and disabled.
        // The other three are each a subset of it.
        fn needs_all_six<T>(_: &T)
        where
            T: ChipAttributes
                + DeletableChipAttributes
                + SelectableChipAttributes
                + CheckmarkableChipAttributes
                + DisabledChipAttributes
                + TappableChipAttributes,
        {
        }
        needs_all_six(&InputChip::new("someone@example.com"));
    }

    #[test]
    fn a_choice_chip_shows_no_tick_and_a_filter_chip_leaves_it_to_the_theme() {
        // The difference is not the widget but the *set*. One choice out of a
        // set is already told apart by its colour, so a tick on the single
        // chosen one says the same thing twice; several filters can be on at
        // once, so the set has to be readable at a glance and colour alone is
        // not enough.
        assert_eq!(ChoiceChip::new("Small").show_checkmark(), Some(false));
        assert_eq!(FilterChip::new("Vegan").show_checkmark(), None);
        assert_eq!(InputChip::new("tag").show_checkmark(), None);
    }

    #[test]
    fn a_choice_or_filter_chip_with_no_callback_is_disabled_but_an_input_chip_is_not() {
        // The three derive `isEnabled` from having something to do; an input
        // chip carries it, because it is showing something the reader typed
        // whether or not it can still be acted on.
        assert!(!ChoiceChip::new("A").is_enabled());
        assert!(ChoiceChip::new("A").with_on_selected(|_| {}).is_enabled());
        assert!(!FilterChip::new("B").is_enabled());
        assert!(!ActionChip::new("C").is_enabled());
        assert!(ActionChip::new("C").with_on_pressed(|| {}).is_enabled());

        assert!(
            InputChip::new("D").is_enabled(),
            "present and inert is a state"
        );
        assert!(!InputChip::new("D").with_enabled(false).is_enabled());
    }

    #[test]
    fn only_the_two_set_chips_can_be_deleted() {
        // A choice is one of a fixed set, so there is nothing to remove --
        // picking another is how you stop picking this one. An action chip
        // has nothing to remove either.
        assert!(!FilterChip::new("A").is_deletable());
        assert!(FilterChip::new("A").with_on_deleted(|| {}).is_deletable());
        assert!(InputChip::new("B").with_on_deleted(|| {}).is_deletable());
    }

    #[test]
    fn every_variant_has_an_elevated_form() {
        // Upstream's `.elevated` constructors, which are a shape rather than a
        // number: an elevated chip sits on its own surface instead of being an
        // outline on the page.
        assert!(!ActionChip::new("A").0.elevated);
        assert!(ActionChip::elevated("A").0.elevated);
        assert!(ChoiceChip::elevated("B").0.elevated);
        assert!(FilterChip::elevated("C").0.elevated);
    }

    #[test]
    fn a_no_splash_feature_presses_without_painting() {
        // It exists rather than being expressed as "no feature" because the
        // *gesture* still has to happen: a control whose theme asks for no
        // splash still presses and still fires its callback.
        use crate::ink::{InkFeatureKind, InteractiveInkFeatureFactory};
        use crate::render::{Offset, Size};

        let mut feature = InteractiveInkFeatureFactory::NoSplash.create(
            Size::new(100.0, 40.0),
            Offset::new(10.0, 10.0),
            Color::argb(0xFF, 0, 0, 0),
            true,
            0,
        );
        assert!(matches!(feature.kind, InkFeatureKind::None(_)));
        assert_eq!(feature.opacity(), 0.0, "nothing to paint, ever");
        assert_eq!(feature.ink_circle(Size::new(100.0, 40.0)), None);
        assert_eq!(feature.paint_color().alpha(), 0);

        // Alive while the finger is down -- the press is happening -- and gone
        // the moment it settles, because upstream's confirm and cancel both
        // dispose outright rather than fading.
        assert!(feature.alive());
        feature.confirm(1_000);
        assert!(!feature.alive());
    }

    #[test]
    fn the_no_splash_factory_is_a_third_choice_beside_the_other_two() {
        // A theme swaps every splash in an application at once, and "none" is
        // one of the things it can swap to.
        use crate::ink::InteractiveInkFeatureFactory;
        assert_ne!(
            InteractiveInkFeatureFactory::NoSplash,
            InteractiveInkFeatureFactory::default()
        );
        assert_ne!(
            InteractiveInkFeatureFactory::NoSplash,
            InteractiveInkFeatureFactory::Splash
        );
    }
}

#[cfg(test)]
mod radio_theme_tests {
    use crate::component_themes::{RadioTheme, RadioThemeData, ResolvedRadio};
    use crate::engine::Color;
    use crate::framework::{ElementTree, component, provide};
    use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

    const MINE: Color = Color::argb(0xFF, 0x12, 0x34, 0x56);

    fn selected() -> WidgetStates {
        WidgetStates::NONE.with(WidgetState::Selected)
    }

    /// Reads the resolution from inside a tree that has the theme installed.
    struct Reader {
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedRadio>>>,
        states: WidgetStates,
    }

    impl crate::framework::Component for Reader {
        fn build(
            &self,
            context: &mut crate::framework::BuildContext,
        ) -> crate::framework::AnyWidget {
            *self.seen.borrow_mut() = Some(ResolvedRadio::of(context, self.states));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(data: RadioThemeData, states: WidgetStates) -> ResolvedRadio {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(RadioTheme::new(
            data,
            crate::framework::component(Reader {
                seen: std::rc::Rc::clone(&seen),
                states,
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn an_unselected_radio_has_a_dot_of_no_size_at_all() {
        // Not a special case in the drawing: the same property answers zero for
        // the unselected state, which is why the dot can grow from nothing when
        // the radio is chosen.
        assert_eq!(
            resolve(RadioThemeData::new(), WidgetStates::NONE).inner_radius,
            0.0
        );
        assert_eq!(
            resolve(RadioThemeData::new(), selected()).inner_radius,
            ResolvedRadio::INNER_RADIUS
        );
    }

    #[test]
    fn the_ring_does_not_change_size_with_the_state() {
        // Only the dot has a radius per state. The ring staying put is what
        // makes a column of radios line up.
        for states in [WidgetStates::NONE, selected()] {
            assert_eq!(
                resolve(RadioThemeData::new(), states).outer_radius,
                ResolvedRadio::OUTER_RADIUS
            );
        }
    }

    #[test]
    fn choosing_a_radio_colours_the_ring_and_the_dot_together() {
        // Upstream paints the outline with the same default fill it paints the
        // dot with, so they cannot disagree.
        let chosen = resolve(RadioThemeData::new(), selected());
        assert_eq!(chosen.side.color, chosen.fill);

        let plain = resolve(RadioThemeData::new(), WidgetStates::NONE);
        assert_eq!(plain.side.color, plain.fill);
        assert_ne!(chosen.fill, plain.fill, "and the two states differ");
    }

    #[test]
    fn a_disabled_radio_is_the_same_colour_whether_it_is_chosen_or_not() {
        // Upstream's `_defaultFillColor` tests disabled first, so it wins over
        // selected -- a greyed-out group should not have one item shouting.
        let off = resolve(
            RadioThemeData::new(),
            WidgetStates::NONE.with(WidgetState::Disabled),
        );
        let on = resolve(
            RadioThemeData::new(),
            selected().with(WidgetState::Disabled),
        );
        assert_eq!(off.fill, on.fill);
        assert_ne!(on.fill, resolve(RadioThemeData::new(), selected()).fill);
    }

    #[test]
    fn the_theme_beats_the_defaults() {
        let mut data = RadioThemeData::new();
        data.fill_color = Some(StateProperty::resolve_with(|_| Some(MINE)));
        assert_eq!(resolve(data, selected()).fill, MINE);
    }

    #[test]
    fn a_themed_inner_radius_answers_for_every_state_including_the_empty_one() {
        // Which is the point of it being a property: a theme can make an
        // unselected radio show a small dot rather than none.
        let mut data = RadioThemeData::new();
        data.inner_radius = Some(StateProperty::resolve_with(|_| Some(3.0)));
        assert_eq!(resolve(data.clone(), WidgetStates::NONE).inner_radius, 3.0);
        assert_eq!(resolve(data, selected()).inner_radius, 3.0);
    }

    #[test]
    fn a_themed_side_replaces_the_ring_outright() {
        let mut data = RadioThemeData::new();
        data.side = Some(crate::borders::BorderSide {
            color: MINE,
            width: 7.0,
            ..crate::borders::BorderSide::NONE
        });
        let resolved = resolve(data, selected());
        assert_eq!(resolved.side.width, 7.0);
        assert_eq!(resolved.side.color, MINE);
        assert_ne!(resolved.fill, MINE, "the dot is not dragged along with it");
    }

    #[test]
    fn the_widget_resolves_against_its_own_state_and_not_a_blank_one() {
        // The resolution above is only worth having if the widget hands it the
        // right states. Observed through the extra rectangle a background
        // paints: a theme that gives one only to the chosen radio makes the
        // two draw different amounts, and a widget passing a blank state would
        // make them draw the same.
        fn rects_for(selected: bool, enabled: bool, wanted: WidgetState) -> u32 {
            let mut data = RadioThemeData::new();
            data.background_color =
                Some(StateProperty::resolve_with(move |states: WidgetStates| {
                    states.contains(wanted).then_some(MINE)
                }));
            crate::engine_test_stubs::reset_layer_calls();
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                RadioTheme::new(
                    data,
                    crate::framework::component(
                        super::Radio::new(1, selected).with_enabled(enabled),
                    ),
                ),
            ));
            use crate::render::RenderBox;
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(crate::render::BoxConstraints::tight(60.0, 60.0));
            let mut layers = crate::engine::LayerTree::new(60, 60);
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(60.0, 60.0),
                );
                root.paint(&mut context, crate::render::Offset::ZERO);
            }
            crate::engine_test_stubs::layer_calls().rects
        }

        let selected = |on| rects_for(on, true, WidgetState::Selected);
        assert_eq!(
            selected(true),
            selected(false) + 1,
            "the chosen radio paints a background the other does not"
        );

        // And the same for the other bit, which a widget reading only its value
        // would get wrong in exactly the same way.
        let disabled = |on| rects_for(true, on, WidgetState::Disabled);
        assert_eq!(
            disabled(false),
            disabled(true) + 1,
            "the disabled radio paints a background the live one does not"
        );
    }

    #[test]
    fn no_background_is_asked_for_unless_the_theme_asks() {
        assert_eq!(resolve(RadioThemeData::new(), selected()).background, None);
        let mut data = RadioThemeData::new();
        data.background_color = Some(StateProperty::resolve_with(|_| Some(MINE)));
        assert_eq!(resolve(data, selected()).background, Some(MINE));
    }
}

#[cfg(test)]
mod bottom_sheet_theme_tests {
    use super::*;
    use crate::component_themes::{BottomSheetTheme, BottomSheetThemeData, ResolvedBottomSheet};
    use crate::framework::{ElementTree, component, provide};

    struct Reader {
        sheet: std::cell::RefCell<Option<BottomSheet>>,
        is_modal: bool,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedBottomSheet>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let sheet = self.sheet.borrow_mut().take().expect("built once");
            *self.seen.borrow_mut() = Some(sheet.resolved(context, self.is_modal));
            leaf(|| Empty)
        }
    }

    fn resolve(
        sheet: BottomSheet,
        data: BottomSheetThemeData,
        is_modal: bool,
    ) -> ResolvedBottomSheet {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            BottomSheetTheme::new(
                data,
                crate::framework::component(Reader {
                    sheet: std::cell::RefCell::new(Some(sheet)),
                    is_modal,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn sheet() -> BottomSheet {
        BottomSheet::new(leaf(|| Empty))
    }

    const SHARED: Color = Color::argb(0xFF, 0x11, 0x11, 0x11);
    const MODAL: Color = Color::argb(0xFF, 0x22, 0x22, 0x22);

    #[test]
    fn a_shared_field_styles_both_kinds_and_a_modal_one_styles_only_modals() {
        // The four-step chain is what lets a theme say "sheets here look like
        // this" once, and separately say "and modal ones differently".
        let mut data = BottomSheetThemeData::new();
        data.background_color = Some(SHARED);
        assert_eq!(resolve(sheet(), data.clone(), false).background, SHARED);
        assert_eq!(resolve(sheet(), data.clone(), true).background, SHARED);

        data.modal_background_color = Some(MODAL);
        assert_eq!(
            resolve(sheet(), data.clone(), false).background,
            SHARED,
            "a persistent sheet never looks at the modal fields"
        );
        assert_eq!(resolve(sheet(), data, true).background, MODAL);
    }

    #[test]
    fn a_modal_only_field_does_not_leak_into_a_persistent_sheet() {
        let mut data = BottomSheetThemeData::new();
        data.modal_background_color = Some(MODAL);
        data.modal_elevation = Some(9.0);
        let persistent = resolve(sheet(), data, false);
        assert_ne!(persistent.background, MODAL);
        assert_eq!(persistent.elevation, ResolvedBottomSheet::ELEVATION);
    }

    #[test]
    fn the_barrier_is_only_a_modal_thing() {
        // A persistent sheet has nothing behind it to dim.
        let mut data = BottomSheetThemeData::new();
        data.modal_barrier_color = Some(MODAL);
        assert_eq!(
            resolve(sheet(), data.clone(), true).modal_barrier_color,
            Some(MODAL)
        );
        assert_eq!(resolve(sheet(), data, false).modal_barrier_color, None);
    }

    #[test]
    fn a_theme_asking_for_handles_does_not_put_one_on_a_sheet_that_cannot_be_dragged() {
        // A control promising something it does not do. The theme's answer is
        // *and*-ed with enableDrag, not merely defaulted to.
        let mut data = BottomSheetThemeData::new();
        data.show_drag_handle = Some(true);

        assert!(resolve(sheet(), data.clone(), false).show_drag_handle);
        assert!(
            !resolve(sheet().with_enable_drag(false), data.clone(), false).show_drag_handle,
            "the theme asked, and the sheet cannot be dragged"
        );

        // Only the sheet's own word overrides that.
        assert!(
            resolve(
                sheet().with_enable_drag(false).with_drag_handle(true),
                data,
                false
            )
            .show_drag_handle,
            "a caller saying it outright has taken responsibility"
        );
    }

    #[test]
    fn no_handle_unless_something_asks_even_when_dragging_is_on() {
        // Draggable is the default; a handle is not.
        assert!(!resolve(sheet(), BottomSheetThemeData::new(), false).show_drag_handle);
        assert!(
            !resolve(sheet(), BottomSheetThemeData::new(), true).show_drag_handle,
            "and a modal one is no different"
        );
    }

    #[test]
    fn a_sheet_can_be_told_to_hide_a_handle_the_theme_asked_for() {
        let mut data = BottomSheetThemeData::new();
        data.show_drag_handle = Some(true);
        assert!(!resolve(sheet().with_drag_handle(false), data, false).show_drag_handle);
    }

    #[test]
    fn both_elevations_default_to_one_because_the_scrim_does_the_separating() {
        // A persistent sheet is part of the page and barely lifted off it; a
        // modal one is separated by its barrier rather than by height.
        assert_eq!(
            resolve(sheet(), BottomSheetThemeData::new(), false).elevation,
            1.0
        );
        assert_eq!(
            resolve(sheet(), BottomSheetThemeData::new(), true).elevation,
            1.0
        );
    }

    #[test]
    fn the_shared_elevation_reaches_a_modal_sheet_when_the_modal_one_is_unset() {
        let mut data = BottomSheetThemeData::new();
        data.elevation = Some(4.0);
        assert_eq!(resolve(sheet(), data.clone(), true).elevation, 4.0);
        assert_eq!(resolve(sheet(), data, false).elevation, 4.0);
    }

    #[test]
    fn with_both_elevations_set_the_modal_one_wins_for_a_modal_sheet() {
        // The order inside the chain, which a test that sets only one of them
        // cannot see: modal first, then shared, then the default.
        let mut data = BottomSheetThemeData::new();
        data.elevation = Some(4.0);
        data.modal_elevation = Some(9.0);
        assert_eq!(resolve(sheet(), data.clone(), true).elevation, 9.0);
        assert_eq!(
            resolve(sheet(), data, false).elevation,
            4.0,
            "and the persistent one still takes the shared field"
        );
    }
}

#[cfg(test)]
mod dialog_theme_tests {
    use super::*;
    use crate::component_themes::{DialogTheme, DialogThemeData, ResolvedDialog};
    use crate::framework::{ElementTree, component, provide};
    use crate::render::EdgeInsets;

    struct Reader(std::rc::Rc<std::cell::RefCell<Option<ResolvedDialog>>>);

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.0.borrow_mut() = Some(Dialog::new("t").resolved(context));
            leaf(|| Empty)
        }
    }

    fn resolve(data: DialogThemeData) -> ResolvedDialog {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            DialogTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// [`resolve`] under a named `ThemeData`, which is what the two defaults
    /// tables are chosen by.
    fn resolve_under(data: DialogThemeData, theme: crate::theme::ThemeData) -> ResolvedDialog {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            DialogTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn the_keyboards_insets_are_added_to_the_margin_and_not_maxed_with_it() {
        // Taking the larger would leave the dialog resting on the keyboard:
        // right by the arithmetic, wrong to look at. The margin is not there to
        // clear the edge of the screen -- it is there so the dialog does not
        // touch whatever is beneath it.
        let resolved = resolve(DialogThemeData::new());
        let keyboard = EdgeInsets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 300.0,
        };
        let effective = resolved.effective_padding(keyboard);
        assert_eq!(
            effective.bottom,
            300.0 + ResolvedDialog::INSET_PADDING.bottom,
            "the keyboard plus the margin, not the larger of the two"
        );
        assert_eq!(
            effective.left,
            ResolvedDialog::INSET_PADDING.left,
            "and the sides are untouched"
        );
    }

    #[test]
    fn with_nothing_covering_the_view_the_margin_is_all_there_is() {
        let resolved = resolve(DialogThemeData::new());
        assert_eq!(
            resolved.effective_padding(EdgeInsets::ZERO),
            ResolvedDialog::INSET_PADDING
        );
    }

    #[test]
    fn the_default_margin_is_wider_at_the_sides_than_at_the_ends() {
        // Upstream's `symmetric(horizontal: 40, vertical: 24)`. A dialog is a
        // column of text, and text that runs the full width of a phone is hard
        // to read; there is no such reason to keep it short.
        assert_eq!(ResolvedDialog::INSET_PADDING.left, 40.0);
        assert_eq!(ResolvedDialog::INSET_PADDING.top, 24.0);
        assert!(ResolvedDialog::INSET_PADDING.left > ResolvedDialog::INSET_PADDING.top);
    }

    #[test]
    fn the_width_constraint_is_a_floor_and_not_a_size() {
        // A dialog narrower than this reads as a tooltip that got lost.
        let resolved = resolve(DialogThemeData::new());
        assert_eq!(resolved.constraints.min_width, ResolvedDialog::MIN_WIDTH);
        assert_eq!(
            resolved.constraints.max_width,
            f32::INFINITY,
            "and nothing stops a wide one being wide"
        );
    }

    #[test]
    fn a_dialog_is_centred_unless_the_theme_moves_it() {
        assert_eq!(
            resolve(DialogThemeData::new()).alignment,
            crate::render::Alignment::CENTER
        );

        let mut data = DialogThemeData::new();
        data.alignment = Some(crate::render::AlignmentGeometry::Absolute(
            crate::render::Alignment::TOP_LEFT,
        ));
        assert_eq!(resolve(data).alignment, crate::render::Alignment::TOP_LEFT);
    }

    #[test]
    fn the_theme_beats_the_defaults_field_by_field() {
        let mut data = DialogThemeData::new();
        data.elevation = Some(2.0);
        data.inset_padding = Some(EdgeInsets::all(5.0));
        let resolved = resolve(data);
        assert_eq!(resolved.elevation, 2.0);
        assert_eq!(resolved.inset_padding, EdgeInsets::all(5.0));
        assert_eq!(
            resolved.constraints.min_width,
            ResolvedDialog::MIN_WIDTH,
            "and what it did not set is untouched"
        );
    }

    #[test]
    fn a_themed_margin_is_what_the_keyboard_is_added_to() {
        let mut data = DialogThemeData::new();
        data.inset_padding = Some(EdgeInsets::all(5.0));
        let keyboard = EdgeInsets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 100.0,
        };
        assert_eq!(resolve(data).effective_padding(keyboard).bottom, 105.0);
    }

    #[test]
    fn the_dialog_draws_its_words_in_the_styles_the_theme_resolved() {
        // The lesson of tick 250, applied before the mutation run rather than
        // after it: the resolver's own tests watch what it answers, and only
        // a paint-level test watches whether the widget asks. Moving the app
        // bar off its hand-rolled style broke nothing, and putting it back
        // broke nothing either.
        //
        // The colour carries it. A named title style has its own ink, and the
        // made-up `theme.title()` takes the older `Theme`'s.
        const TITLE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        const BODY: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            DialogTheme::new(
                DialogThemeData {
                    title_text_style: Some(TextStyle {
                        color: TITLE,
                        ..TextStyle::default()
                    }),
                    content_text_style: Some(TextStyle {
                        color: BODY,
                        ..TextStyle::default()
                    }),
                    ..DialogThemeData::new()
                },
                crate::framework::component(
                    Dialog::new("Discard?").with_body("This cannot be undone."),
                ),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let ink = |wanted: &str| {
            crate::engine_test_stubs::drawn()
                .into_iter()
                .find_map(|call| match call {
                    crate::engine_test_stubs::Drawn::Paragraph { text, argb, .. }
                        if text == wanted =>
                    {
                        Some(argb)
                    }
                    _ => None,
                })
                .expect("the words")
        };
        assert_eq!(ink("Discard?"), TITLE.0);
        assert_eq!(ink("This cannot be undone."), BODY.0);
    }

    #[test]
    fn only_the_barrier_is_left_for_someone_else_to_answer() {
        // This test was called `nothing_is_invented_for_the_fields_upstream_leaves_null`
        // and asserted that four fields stay unset, on the reasoning that "a
        // colour made up here would be one the widget could not tell from a
        // real answer". Upstream leaves exactly one of the four unset.
        // `_DialogDefaultsM3` answers for the other three, and
        // `_DialogDefaultsM2` answers differently for two of them.
        //
        // The barrier is the real one, and for a reason worth keeping: a
        // barrier is not part of a dialog. It belongs to `showDialog`, which
        // is what puts the dialog on the screen, and its default lives there.
        let resolved = resolve(DialogThemeData::new());
        assert_eq!(resolved.barrier_color, None);
    }

    #[test]
    fn a_material_three_dialog_casts_no_shadow_and_takes_no_tint() {
        // Both transparent, and that is an answer rather than a gap. An M3
        // dialog is elevation 6 with no shadow and no tint: how far off the
        // page it is is said entirely by `surfaceContainerHigh`. Left unset,
        // anything downstream reads "nobody said" and draws the shadow
        // upstream turned off on purpose.
        let resolved = resolve(DialogThemeData::new());
        assert_eq!(resolved.elevation, 6.0, "and it is still lifted");
        assert_eq!(resolved.shadow_color, Some(Color::TRANSPARENT));
        assert_eq!(resolved.surface_tint_color, Some(Color::TRANSPARENT));

        // Material 2 casts the theme's own shadow, and has no notion of a
        // tint at all -- so that one field really is left alone.
        let mut two = crate::theme::ThemeData::light();
        two.use_material3 = false;
        let old = resolve_under(DialogThemeData::new(), two.clone());
        assert_eq!(old.shadow_color, Some(two.shadow_color));
        assert_ne!(old.shadow_color, Some(Color::TRANSPARENT));
        assert_eq!(old.surface_tint_color, None);
    }

    #[test]
    fn a_dialogs_title_is_headline_small_and_its_body_is_not() {
        // `headlineSmall` had no reader in this port at all. It is the
        // dialog title's Material 3 role, and the content's is `bodyMedium`
        // -- two different roles, because a dialog's question and its
        // explanation are not the same weight of thing.
        let theme = crate::theme::ThemeData::light();
        let resolved = resolve(DialogThemeData::new());
        assert_eq!(resolved.title_text_style, theme.text_theme.headline_small);
        assert_eq!(resolved.content_text_style, theme.text_theme.body_medium);
        assert_ne!(
            resolved.title_text_style, resolved.content_text_style,
            "the title is not the body"
        );

        // Material 2 uses `titleLarge` and `titleMedium` -- both of them a
        // rung down, and both of them different from Material 3's pair.
        let mut two = crate::theme::ThemeData::light();
        two.use_material3 = false;
        let old = resolve_under(DialogThemeData::new(), two.clone());
        assert_eq!(old.title_text_style, two.text_theme.title_large);
        assert_eq!(old.content_text_style, two.text_theme.title_medium);
        assert_ne!(old.title_text_style, resolved.title_text_style);
    }

    #[test]
    fn a_dialogs_icon_is_secondary_under_material_three_and_borrowed_under_two() {
        // The two tables disagree about *where the answer comes from*, not
        // only about what it is: Material 3 names a scheme colour, and
        // Material 2 takes whatever the surrounding icon theme is using -- so
        // an M2 dialog's icon matches the icons around it rather than
        // standing apart from them.
        let theme = crate::theme::ThemeData::light();
        assert_eq!(
            resolve(DialogThemeData::new()).icon_color,
            Some(theme.color_scheme.secondary)
        );

        let mut two = crate::theme::ThemeData::light();
        two.use_material3 = false;
        let old = resolve_under(DialogThemeData::new(), two);
        assert_ne!(old.icon_color, Some(theme.color_scheme.secondary));
    }
}

#[cfg(test)]
mod tab_bar_theme_tests {
    use super::*;
    use crate::component_themes::{ResolvedTabBar, TabBarTheme, TabBarThemeData};
    use crate::framework::{ElementTree, component, provide};

    struct Reader(std::rc::Rc<std::cell::RefCell<Option<ResolvedTabBar>>>);

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.0.borrow_mut() = Some(TabBar::new(1, Vec::new(), 0).resolved(context));
            leaf(|| Empty)
        }
    }

    fn resolve(data: TabBarThemeData) -> ResolvedTabBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            TabBarTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// [`resolve`] under a named `ThemeData`, which is what upstream's three
    /// tables are chosen by.
    fn resolve_under(data: TabBarThemeData, theme: crate::theme::ThemeData) -> ResolvedTabBar {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            TabBarTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn scheme() -> crate::color_scheme::ColorScheme {
        crate::theme::ThemeData::fallback().color_scheme
    }

    const IN_FIELD: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
    const IN_STYLE: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);

    // -- The words on a tab, tick 252 ---------------------------------------
    //
    // `ResolvedTabBar` worked out five colours, a divider height, a padding,
    // an indicator size, an alignment and an animation -- and passed both
    // label styles straight through with no default. Upstream's three tables
    // all answer, and `TextTheme::title_small` had no reader in this port.
    //
    // The widget asked for none of it either: `TabBar::build` sized its
    // labels with `theme.body_size` and weighted them 700 or 500 by hand, so
    // the two colours the resolver works out in five careful steps were not
    // the ones drawn.

    #[test]
    fn both_tab_labels_take_the_same_role_and_it_is_title_small() {
        // A selected tab is told apart by its colour and its underline, not
        // by being a different size -- so the two styles are the same role,
        // in all three of upstream's tables. What the tables disagree about
        // is which role.
        let theme = crate::theme::ThemeData::light();
        let resolved = resolve_under(TabBarThemeData::new(), theme.clone());
        assert_eq!(resolved.label_style, theme.text_theme.title_small);
        assert_eq!(
            resolved.unselected_label_style,
            theme.text_theme.title_small
        );

        // Material 2 reads `primaryTextTheme`, the scale for text drawn *on*
        // a primary-coloured surface -- which is what an M2 tab bar is, since
        // it sits in the app bar. Material 3's does not, so it reads the
        // ordinary scale.
        let mut two = crate::theme::ThemeData::light();
        two.use_material3 = false;
        let old = resolve_under(TabBarThemeData::new(), two.clone());
        assert_eq!(old.label_style, two.primary_text_theme.body_large);
        assert_ne!(
            old.label_style, resolved.label_style,
            "the two tables do not agree"
        );
        assert_ne!(
            two.primary_text_theme.body_large, two.text_theme.body_large,
            "and primaryTextTheme is not the ordinary one"
        );
    }

    #[test]
    fn a_named_style_beats_the_table_for_that_label_alone() {
        let mine = TextStyle {
            font_size: 41.0,
            ..TextStyle::default()
        };
        let resolved = resolve_under(
            TabBarThemeData {
                label_style: Some(mine.clone()),
                ..TabBarThemeData::new()
            },
            crate::theme::ThemeData::light(),
        );
        assert_eq!(
            resolved.label_style.map(|style| style.font_size),
            Some(41.0)
        );
        assert_ne!(
            resolved.unselected_label_style.map(|style| style.font_size),
            Some(41.0),
            "the other one still takes the table's"
        );
    }

    #[test]
    fn the_bar_draws_its_labels_in_the_styles_and_colours_it_resolved() {
        // The paint-level half, written before the mutation run. The
        // resolver's own tests watch what it answers; only this watches
        // whether the widget asks -- and it did not: it sized the labels from
        // `theme.body_size` and coloured them from the older `Theme`.
        const SELECTED: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        const QUIET: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            TabBarTheme::new(
                TabBarThemeData {
                    label_color: Some(SELECTED),
                    unselected_label_color: Some(QUIET),
                    ..TabBarThemeData::new()
                },
                crate::framework::component(TabBar::new(
                    1,
                    vec![String::from("Mail"), String::from("Files")],
                    0,
                )),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let ink = |wanted: &str| {
            crate::engine_test_stubs::drawn()
                .into_iter()
                .find_map(|call| match call {
                    crate::engine_test_stubs::Drawn::Paragraph { text, argb, .. }
                        if text == wanted =>
                    {
                        Some(argb)
                    }
                    _ => None,
                })
                .expect("the label")
        };
        assert_eq!(ink("Mail"), SELECTED.0, "the chosen tab");
        assert_eq!(ink("Files"), QUIET.0, "and the one beside it");
    }

    #[test]
    fn a_colour_inside_the_text_style_counts_but_counts_last() {
        // Upstream consults labelStyle.color after both explicit labelColors.
        // Moving it up would be a breaking change with no migration, so the
        // less specific place keeps the higher precedence.
        let mut style_only = TabBarThemeData::new();
        style_only.label_style = Some(crate::engine::TextStyle {
            color: IN_STYLE,
            ..Default::default()
        });
        assert_eq!(resolve(style_only.clone()).label_color, IN_STYLE);

        let mut both = style_only;
        both.label_color = Some(IN_FIELD);
        assert_eq!(
            resolve(both).label_color,
            IN_FIELD,
            "the field wins even though the style is the more specific place"
        );
    }

    #[test]
    fn the_unselected_label_has_the_same_chain_of_its_own() {
        let mut data = TabBarThemeData::new();
        data.unselected_label_style = Some(crate::engine::TextStyle {
            color: IN_STYLE,
            ..Default::default()
        });
        assert_eq!(resolve(data.clone()).unselected_label_color, IN_STYLE);

        data.unselected_label_color = Some(IN_FIELD);
        assert_eq!(resolve(data).unselected_label_color, IN_FIELD);
    }

    #[test]
    fn the_two_labels_do_not_borrow_from_each_other() {
        let mut data = TabBarThemeData::new();
        data.label_color = Some(IN_FIELD);
        let resolved = resolve(data);
        assert_eq!(resolved.label_color, IN_FIELD);
        assert_eq!(
            resolved.unselected_label_color,
            scheme().on_surface_variant(),
            "the unselected one keeps its own default"
        );
    }

    #[test]
    fn the_indicator_does_not_follow_the_label() {
        // `_TabsPrimaryDefaultsM3.indicatorColor` is the primary in its own
        // right: a theme that recolours the labels leaves the underline where
        // it was.
        let mut data = TabBarThemeData::new();
        data.label_color = Some(IN_FIELD);
        let resolved = resolve(data);
        assert_eq!(resolved.indicator_color, scheme().primary);
        assert_ne!(resolved.indicator_color, resolved.label_color);
    }

    #[test]
    fn the_pre_material_three_unselected_colour_is_the_selected_one_at_seventy_per_cent() {
        // The two labels are one colour said at two volumes, not two colours.
        let quiet = ResolvedTabBar::unselected_from(IN_FIELD);
        assert_eq!(quiet.alpha(), 0xB2);
        assert_eq!(quiet.red(), IN_FIELD.red());
        assert_ne!(
            quiet,
            resolve(TabBarThemeData::new()).unselected_label_color,
            "and it is not what Material 3 uses, which is why it is a function"
        );
    }

    #[test]
    fn the_metrics_have_upstreams_defaults() {
        let resolved = resolve(TabBarThemeData::new());
        assert_eq!(resolved.divider_height, 1.0);
        assert_eq!(resolved.divider_color, scheme().outline_variant());
        assert_eq!(resolved.label_padding.left, 16.0);
        assert_eq!(resolved.label_padding.top, 0.0);
    }

    #[test]
    fn the_styles_themselves_are_passed_through_unchanged() {
        // The colour is pulled out of them for the chain; the rest of the
        // style is the caller's and is not second-guessed.
        let mut data = TabBarThemeData::new();
        data.label_style = Some(crate::engine::TextStyle {
            color: IN_STYLE,
            font_size: 22.0,
            ..Default::default()
        });
        let resolved = resolve(data);
        assert_eq!(resolved.label_style.expect("kept").font_size, 22.0);
    }
}

#[cfg(test)]
mod navigation_rail_theme_tests {
    use super::*;
    use crate::component_themes::{
        NavigationRailLabelType, NavigationRailTheme, NavigationRailThemeData,
        ResolvedNavigationRail,
    };
    use crate::framework::{ElementTree, component, provide};

    struct Reader {
        extended: bool,
        seen: std::rc::Rc<
            std::cell::RefCell<Option<(ResolvedNavigationRail, Result<(), &'static str>)>>,
        >,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let rail = NavigationRail::new(1, Vec::new(), 0).extended(self.extended);
            let resolved = rail.resolved(context);
            let checked = rail.check(context);
            *self.seen.borrow_mut() = Some((resolved, checked));
            leaf(|| Empty)
        }
    }

    fn resolve(
        data: NavigationRailThemeData,
        extended: bool,
    ) -> (ResolvedNavigationRail, Result<(), &'static str>) {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            NavigationRailTheme::new(
                data,
                crate::framework::component(Reader {
                    extended,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn an_extended_rail_may_not_also_ask_for_labels() {
        // It puts every label beside its icon by definition, so "selected only"
        // on top of that is a contradiction rather than a preference.
        let mut data = NavigationRailThemeData::new();
        data.label_type = Some(NavigationRailLabelType::Selected);

        assert!(resolve(data.clone(), false).1.is_ok(), "not extended, fine");
        assert!(resolve(data, true).1.is_err());
    }

    #[test]
    fn the_check_runs_against_the_resolved_type_and_not_the_widgets_own() {
        // A rail that was extended and left the label type alone is still
        // wrong when the *theme* asks for labels -- and that is the case a
        // caller cannot see for themselves.
        let plain = NavigationRailThemeData::new();
        assert!(resolve(plain, true).1.is_ok(), "the default is none");

        let mut asking = NavigationRailThemeData::new();
        asking.label_type = Some(NavigationRailLabelType::All);
        assert!(resolve(asking, true).1.is_err());
    }

    #[test]
    fn the_group_alignment_is_the_top_and_not_the_middle() {
        // A rail is a list read downwards from the first item; centring it
        // would leave that item somewhere different on every screen height.
        assert_eq!(
            resolve(NavigationRailThemeData::new(), false)
                .0
                .group_alignment,
            -1.0
        );
    }

    #[test]
    fn the_two_widths_are_separate_numbers_and_not_one_scaled() {
        // Eighty is an icon with room around it; two hundred and fifty-six is a
        // column of text.
        let (resolved, _) = resolve(NavigationRailThemeData::new(), false);
        assert_eq!(resolved.width(false), 80.0);
        assert_eq!(resolved.width(true), 256.0);

        let mut data = NavigationRailThemeData::new();
        data.min_width = Some(100.0);
        let (resolved, _) = resolve(data, false);
        assert_eq!(resolved.width(false), 100.0);
        assert_eq!(
            resolved.width(true),
            256.0,
            "setting one leaves the other alone"
        );
    }

    #[test]
    fn labels_are_off_by_default_because_the_indicator_already_says_which() {
        let (resolved, _) = resolve(NavigationRailThemeData::new(), false);
        assert_eq!(resolved.label_type, NavigationRailLabelType::None);
        assert!(resolved.use_indicator, "and it is on");
    }

    #[test]
    fn the_theme_beats_the_defaults_field_by_field() {
        let mut data = NavigationRailThemeData::new();
        data.use_indicator = Some(false);
        data.group_alignment = Some(0.0);
        let (resolved, _) = resolve(data, false);
        assert!(!resolved.use_indicator);
        assert_eq!(resolved.group_alignment, 0.0);
        assert_eq!(
            resolved.min_width,
            ResolvedNavigationRail::MIN_WIDTH,
            "and what it did not set is untouched"
        );
    }

    #[test]
    fn nothing_is_invented_for_the_fields_upstream_leaves_null() {
        // A background, an indicator colour and the label styles that nobody
        // set stay unset: the widget above decides, and a value made up here
        // would be one it could not tell from a real answer.
        let (resolved, _) = resolve(NavigationRailThemeData::new(), false);
        assert_eq!(resolved.background_color, None);
        assert_eq!(resolved.indicator_color, None);
        assert_eq!(resolved.selected_label_style, None);
        assert_eq!(resolved.selected_icon_theme, None);
    }
}

#[cfg(test)]
mod data_table_theme_tests {
    use super::*;
    use crate::component_themes::{DataTableTheme, DataTableThemeData, ResolvedDataTable};
    use crate::framework::{ElementTree, component, provide};

    struct Reader(std::rc::Rc<std::cell::RefCell<Option<ResolvedDataTable>>>);

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.0.borrow_mut() = Some(DataTable::new(Vec::new()).resolved(context));
            leaf(|| Empty)
        }
    }

    fn resolve(data: DataTableThemeData) -> ResolvedDataTable {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            DataTableTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn a_default_row_is_fixed_because_both_bounds_are_the_same_number() {
        // The two fields exist to make a row flexible; until one of them moves
        // there is no flexibility to have.
        let resolved = resolve(DataTableThemeData::new());
        assert_eq!(resolved.data_row_min_height, ResolvedDataTable::ROW_HEIGHT);
        assert_eq!(resolved.data_row_max_height, ResolvedDataTable::ROW_HEIGHT);
        assert_eq!(resolved.data_row_min_height, resolved.data_row_max_height);
        assert!(resolved.check().is_ok());
    }

    #[test]
    fn raising_only_the_minimum_leaves_the_two_crossed() {
        // The case that bites from outside: both default to the same height, so
        // moving one alone is a contradiction written with a single field.
        let mut data = DataTableThemeData::new();
        data.data_row_min_height = Some(80.0);
        assert!(resolve(data.clone()).check().is_err());

        data.data_row_max_height = Some(120.0);
        assert!(resolve(data).check().is_ok(), "moving both is fine");
    }

    #[test]
    fn raising_only_the_maximum_is_what_makes_a_row_flexible() {
        let mut data = DataTableThemeData::new();
        data.data_row_max_height = Some(120.0);
        let resolved = resolve(data);
        assert_eq!(resolved.data_row_min_height, ResolvedDataTable::ROW_HEIGHT);
        assert_eq!(resolved.data_row_max_height, 120.0);
        assert!(resolved.check().is_ok());
    }

    #[test]
    fn a_heading_row_is_taller_than_a_data_row() {
        // Fifty-six against forty-eight. The heading is read once and the rows
        // many times; the extra eight are what stop the header reading as the
        // first entry.
        let resolved = resolve(DataTableThemeData::new());
        assert_eq!(resolved.heading_row_height, 56.0);
        assert!(resolved.heading_row_height > resolved.data_row_min_height);
    }

    #[test]
    fn a_checkbox_sits_in_the_tables_own_gutter_unless_given_one() {
        // Upstream falls back to the horizontal margin rather than a constant.
        let mut data = DataTableThemeData::new();
        data.horizontal_margin = Some(40.0);
        let resolved = resolve(data.clone());
        assert_eq!(resolved.checkbox_horizontal_margin, None);
        assert_eq!(resolved.checkbox_margin(), 40.0, "the table's own");

        data.checkbox_horizontal_margin = Some(4.0);
        assert_eq!(resolve(data).checkbox_margin(), 4.0);
    }

    #[test]
    fn the_spacing_defaults_are_upstreams() {
        let resolved = resolve(DataTableThemeData::new());
        assert_eq!(resolved.horizontal_margin, 24.0);
        assert_eq!(resolved.column_spacing, 56.0);
        assert_eq!(resolved.divider_thickness, 1.0);
    }

    #[test]
    fn the_theme_beats_the_defaults_field_by_field() {
        let mut data = DataTableThemeData::new();
        data.column_spacing = Some(8.0);
        let resolved = resolve(data);
        assert_eq!(resolved.column_spacing, 8.0);
        assert_eq!(
            resolved.horizontal_margin,
            ResolvedDataTable::HORIZONTAL_MARGIN,
            "and what it did not set is untouched"
        );
    }

    #[test]
    fn nothing_is_invented_for_the_styles_upstream_leaves_null() {
        let resolved = resolve(DataTableThemeData::new());
        assert_eq!(resolved.data_text_style, None);
        assert_eq!(resolved.heading_text_style, None);
        assert_eq!(resolved.decoration, None);
    }
}

#[cfg(test)]
mod delete_button_tooltip_tests {
    use super::DeletableChipAttributes;
    use std::rc::Rc;

    #[derive(Default)]
    struct Chip {
        deletable: bool,
        message: Option<String>,
    }

    impl DeletableChipAttributes for Chip {
        fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
            self.deletable.then(|| Rc::new(|| ()) as Rc<dyn Fn()>)
        }

        fn delete_button_tooltip_message(&self) -> Option<String> {
            self.message.clone()
        }
    }

    #[test]
    fn a_deletable_chip_that_named_nothing_still_says_delete() {
        // The gap: the message was declared and the fallback never was, so
        // every chip that did not name its own tooltip had none at all.
        let chip = Chip {
            deletable: true,
            message: None,
        };
        assert_eq!(chip.delete_button_tooltip(true).as_deref(), Some("Delete"));
    }

    #[test]
    fn and_one_that_named_something_says_that_instead() {
        let chip = Chip {
            deletable: true,
            message: Some("Remove filter".to_string()),
        };
        assert_eq!(
            chip.delete_button_tooltip(true).as_deref(),
            Some("Remove filter")
        );
    }

    #[test]
    fn the_two_ways_of_getting_nothing_are_different_reasons() {
        // No callback: no cross is drawn, so there is nothing to describe.
        let undeletable = Chip {
            deletable: false,
            message: Some("Remove filter".to_string()),
        };
        assert_eq!(
            undeletable.delete_button_tooltip(true),
            None,
            "a chip with no onDeleted shows no cross, named message or not"
        );

        // Disabled: the cross is drawn and upstream drops the tooltip, because
        // explaining an action the reader cannot take is worse than silence.
        let disabled = Chip {
            deletable: true,
            message: Some("Remove filter".to_string()),
        };
        assert_eq!(disabled.delete_button_tooltip(false), None);
    }

    #[test]
    fn enabling_a_chip_is_what_brings_the_tooltip_back() {
        // Through the argument rather than two fixtures, so the answer is
        // shown to follow the state rather than the construction.
        let chip = Chip {
            deletable: true,
            message: None,
        };
        assert_eq!(chip.delete_button_tooltip(false), None);
        assert_eq!(chip.delete_button_tooltip(true).as_deref(), Some("Delete"));
    }
}

#[cfg(test)]
mod modal_surface_label_tests {
    use super::{AlertDialog, SimpleDialog};
    use crate::drawer::Drawer;
    use crate::editable_text::TargetPlatform;
    use crate::framework::leaf;
    use crate::render::RenderConstrainedBox;

    const APPLE: [TargetPlatform; 2] = [TargetPlatform::IOS, TargetPlatform::MacOS];
    const REST: [TargetPlatform; 4] = [
        TargetPlatform::Android,
        TargetPlatform::Fuchsia,
        TargetPlatform::Linux,
        TargetPlatform::Windows,
    ];

    #[test]
    fn an_alert_is_announced_as_an_alert_and_a_dialog_as_a_dialog() {
        // Two words for two shapes: an alert interrupts, a dialog asks, and
        // the reader is told which before hearing the contents.
        for platform in REST {
            assert_eq!(
                AlertDialog::new()
                    .resolved_semantic_label(platform)
                    .as_deref(),
                Some("Alert"),
                "{platform:?}"
            );
            assert_eq!(
                SimpleDialog::new()
                    .resolved_semantic_label(platform)
                    .as_deref(),
                Some("Dialog"),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn and_on_the_apple_platforms_neither_says_anything() {
        for platform in APPLE {
            assert_eq!(
                AlertDialog::new().resolved_semantic_label(platform),
                None,
                "{platform:?}"
            );
            assert_eq!(
                SimpleDialog::new().resolved_semantic_label(platform),
                None,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_named_dialog_keeps_its_name_on_every_platform() {
        for platform in APPLE.iter().chain(REST.iter()) {
            assert_eq!(
                AlertDialog::new()
                    .with_semantic_label("Unsaved changes")
                    .resolved_semantic_label(*platform)
                    .as_deref(),
                Some("Unsaved changes"),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn all_three_modal_surfaces_follow_the_one_rule() {
        // Upstream writes the switch out three times. If they ever disagree
        // about the Apple case, one of the three has drifted.
        let drawer = || Drawer::new(leaf(|| RenderConstrainedBox::tight(10.0, 10.0)));
        for platform in APPLE {
            assert_eq!(drawer().resolved_semantic_label(platform), None);
            assert_eq!(AlertDialog::new().resolved_semantic_label(platform), None);
            assert_eq!(SimpleDialog::new().resolved_semantic_label(platform), None);
        }
        for platform in REST {
            assert!(drawer().resolved_semantic_label(platform).is_some());
            assert!(
                AlertDialog::new()
                    .resolved_semantic_label(platform)
                    .is_some()
            );
            assert!(
                SimpleDialog::new()
                    .resolved_semantic_label(platform)
                    .is_some()
            );
        }
    }

    #[test]
    fn and_the_three_fallbacks_are_three_different_words() {
        // Sharing the rule must not turn into sharing the word.
        let drawer = Drawer::new(leaf(|| RenderConstrainedBox::tight(10.0, 10.0)));
        let words = [
            drawer.resolved_semantic_label(TargetPlatform::Android),
            AlertDialog::new().resolved_semantic_label(TargetPlatform::Android),
            SimpleDialog::new().resolved_semantic_label(TargetPlatform::Android),
        ];
        assert_eq!(
            words,
            [
                Some("Navigation menu".to_string()),
                Some("Alert".to_string()),
                Some("Dialog".to_string()),
            ]
        );
    }
}

// -- What the spinner puts on the canvas --------------------------------------

#[cfg(test)]
mod spinner_paint_tests {
    use super::ArcSpinner;
    use crate::engine::{Color, LayerTree};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

    const TRACK: Color = Color(0xff303030);
    const FILL: Color = Color(0xff0066cc);
    const EXTENT: f32 = 36.0;

    /// The stub's `rf_canvas_draw_arc` had an empty body until this tick, so
    /// none of what is asserted below was a call any test could see. The
    /// spinner is one oval and one arc, and everything it says is in the
    /// angles.
    fn painted(value: f32, at: Offset) -> Vec<Drawn> {
        let mut spinner = ArcSpinner {
            value,
            extent: EXTENT,
            track: TRACK,
            fill: FILL,
            laid_out: Size::ZERO,
        };
        spinner.layout(BoxConstraints::loose(100.0, 100.0));
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            spinner.paint(&mut context, at);
        }
        drawn()
    }

    #[allow(clippy::type_complexity)]
    fn arcs(calls: &[Drawn]) -> Vec<((f32, f32, f32, f32), f32, f32, bool, u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Arc {
                    left,
                    top,
                    right,
                    bottom,
                    start_degrees,
                    sweep_degrees,
                    use_center,
                    argb,
                    ..
                } => Some((
                    (*left, *top, *right, *bottom),
                    *start_degrees,
                    *sweep_degrees,
                    *use_center,
                    *argb,
                )),
                _ => None,
            })
            .collect()
    }

    fn ovals(calls: &[Drawn]) -> Vec<((f32, f32, f32, f32), u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Oval {
                    left,
                    top,
                    right,
                    bottom,
                    argb,
                    ..
                } => Some(((*left, *top, *right, *bottom), *argb)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_spinner_starts_at_twelve_oclock_and_not_at_three() {
        // Zero on a canvas is three o'clock. A spinner that starts there looks
        // like it is a quarter of the way round before it has moved, and the
        // only thing that says otherwise is this number.
        let calls = painted(0.25, Offset::ZERO);
        let arc = arcs(&calls);
        assert_eq!(arc.len(), 1, "{calls:?}");
        assert_eq!(arc[0].1, -90.0);
    }

    #[test]
    fn the_sweep_is_the_value_as_a_fraction_of_the_whole_turn() {
        for (value, degrees) in [(0.25, 90.0), (0.5, 180.0), (1.0, 360.0)] {
            let arc = arcs(&painted(value, Offset::ZERO));
            assert_eq!(arc.len(), 1, "at {value}");
            assert_eq!(arc[0].2, degrees, "at {value}");
        }
    }

    #[test]
    fn a_spinner_at_zero_draws_no_arc_rather_than_an_arc_of_no_length() {
        // The arc has a round cap, so a sweep of zero is not nothing: it is a
        // dot at twelve o'clock. The guard is what stops a progress bar that
        // has not started from showing a mark saying it has.
        let calls = painted(0.0, Offset::ZERO);
        assert!(arcs(&calls).is_empty(), "{calls:?}");
        assert_eq!(ovals(&calls).len(), 1, "but the track is still drawn");
    }

    #[test]
    fn the_arc_rides_on_the_track_rather_than_inside_or_outside_it() {
        // Both are stroked at the same width, so sharing the bounds is what
        // makes them the same ring. Different bounds and the fill sits beside
        // the track in a groove of its own.
        let calls = painted(0.75, Offset::ZERO);
        let track = ovals(&calls);
        let arc = arcs(&calls);
        assert_eq!((track.len(), arc.len()), (1, 1));
        assert_eq!(track[0].0, arc[0].0);
        assert_eq!(track[0].1, TRACK.0, "the track is the track colour");
        assert_eq!(arc[0].4, FILL.0, "and the arc is the fill colour");
    }

    #[test]
    fn the_ring_is_inset_by_half_its_stroke_so_it_fits_the_box() {
        // A stroke is centred on the path, so a ring drawn on the edge of the
        // box loses its outer half to the clip. The inset is what keeps the
        // whole stroke inside, and it is half the width rather than the width.
        let stroke = (EXTENT * 0.11f32).max(2.0);
        let inset = stroke / 2.0;
        let calls = painted(1.0, Offset::ZERO);
        let (left, top, right, bottom) = ovals(&calls)[0].0;
        assert_eq!((left, top), (inset, inset));
        assert_eq!((right, bottom), (EXTENT - inset, EXTENT - inset));
        assert_eq!(right - left, EXTENT - stroke, "a whole stroke narrower");
    }

    #[test]
    fn the_arc_is_a_ring_segment_and_not_a_pie_wedge() {
        // `use_center` joins the two ends through the middle. A filled wedge
        // is a different control, and the flag is one character away.
        let arc = arcs(&painted(0.3, Offset::ZERO));
        assert!(!arc[0].3);
    }

    #[test]
    fn moving_the_spinner_moves_the_box_and_leaves_the_angles_alone() {
        let at = Offset::new(12.0, 30.0);
        let here = arcs(&painted(0.4, Offset::ZERO));
        let there = arcs(&painted(0.4, at));
        assert_eq!(there[0].0.0 - here[0].0.0, at.dx);
        assert_eq!(there[0].0.1 - here[0].0.1, at.dy);
        assert_eq!((there[0].1, there[0].2), (here[0].1, here[0].2));
    }
}
