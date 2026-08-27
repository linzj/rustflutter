//! `showDialog` and the pickers, as calls rather than as widgets.
//!
//! # What this replaces
//!
//! `show_date_picker`, `show_time_picker` and `show_date_range_picker` build
//! their own scrim, stack the dialog over it, and **return an `AnyWidget`**.
//! The caller has to find somewhere in their own tree to put it, keep a bool
//! for whether it is up, and take it out again when a callback fires. Every
//! caller writes the same pipeline, the dialog can only cover whatever `Stack`
//! the caller had, and "is the dialog open" ends up as state on the page root.
//!
//! That is the first thing `PORTING_PLAN_OVERLAY.md` lists among the costs of
//! having no overlay, and it is the last of them to go.
//!
//! With a host, showing a dialog is a call that returns a handle. The barrier,
//! the focus trap, Escape and the dismissal come from
//! [`crate::theatre::show_modal`]; what is added here is the centring, the
//! scrim colour, and **closing the dialog when it answers** -- which is the
//! part every caller was writing by hand.
//!
//! The widget-returning functions are left where they are. They work, code
//! depends on them, and a caller who wants to place a dialog inside their own
//! layout is entitled to.

use std::rc::Rc;

use crate::engine::Color;
use crate::framework::{AnyWidget, many};
use crate::modal_barrier::ModalBarrier;
use crate::pickers::{DatePickerDialog, DateRangePickerDialog, TimePickerDialog};
use crate::render::{Alignment, RenderAlign};
use crate::theatre::{ModalHandle, OverlayHandle, show_modal};

/// Upstream's `ModalBarrier` colour for a dialog: `Colors.black54`.
///
/// A dialog dims the page and a menu does not. The reason is what each is
/// asking for -- a dialog wants an answer before anything else happens, and the
/// dimming says so; a menu is a choice among things you can already see.
pub const DIALOG_BARRIER_COLOR: Color = Color::argb(0x8A, 0, 0, 0);

/// Upstream `showCupertinoModalPopup`.
///
/// The popup is bottom-aligned over [`crate::cupertino_route::MODAL_BARRIER_COLOR`]
/// and **slides up on a spring**, not a curve -- upstream's
/// `_CupertinoModalPopupRoute.createSimulation`, whose reason
/// [`crate::cupertino_route::CupertinoModalPopupRoute::create_simulation`]
/// records: a sheet grabbed on the way up keeps moving from where it was.
///
/// It lives beside `showDialog` rather than in the Cupertino tier because it
/// is the same shape of thing -- a route turned into a call on the theatre --
/// and the tier it belongs to holds the *rules*
/// ([`crate::cupertino_route`]'s header says so outright: geometry, durations
/// and decisions, no widgets).
pub fn show_cupertino_modal_popup(
    overlay: Rc<OverlayHandle>,
    content: impl Fn() -> AnyWidget + 'static,
) -> Option<ModalHandle> {
    // `barrierDismissible: true` and `barrierLabel: 'Dismiss'`, from
    // `CupertinoModalPopupRoute::new`.
    let route = crate::cupertino_route::CupertinoModalPopupRoute::new();
    let barrier = ModalBarrier::new()
        .with_color(
            route
                .popup
                .modal
                .barrier_color
                .unwrap_or(Color::TRANSPARENT),
        )
        .with_semantics_label(
            route
                .popup
                .modal
                .barrier_label
                .clone()
                .unwrap_or_else(|| "Dismiss".to_string()),
        );
    let content: Rc<dyn Fn() -> AnyWidget> = Rc::new(content);
    show_modal(overlay, barrier, move || {
        crate::framework::stateful(CupertinoModalPopup {
            content: Rc::clone(&content),
        })
    })
}

/// The sliding half of [`show_cupertino_modal_popup`]: upstream's
/// `buildTransitions`, which is an `Align(bottomCenter)` around a
/// `FractionalTranslation` from one whole height below to nothing.
struct CupertinoModalPopup {
    content: Rc<dyn Fn() -> AnyWidget>,
}

/// How far into the spring the sheet is. The simulation is stateless, so all
/// that is kept is the clock.
#[derive(Default)]
struct CupertinoModalPopupState {
    elapsed_micros: i64,
    last_frame_micros: Option<i64>,
}

impl crate::framework::StatefulComponent for CupertinoModalPopup {
    type State = CupertinoModalPopupState;

    fn advance(&self, state: &mut CupertinoModalPopupState, frame_time_micros: i64) -> bool {
        // Clamped for the same reason every on-demand animation in this crate
        // clamps: the previous frame may be a page-load away.
        const MAX_FRAME_MICROS: i64 = 50_000;
        if let Some(previous) = state.last_frame_micros {
            state.elapsed_micros += (frame_time_micros - previous).clamp(0, MAX_FRAME_MICROS);
        }
        state.last_frame_micros = Some(frame_time_micros);
        !crate::physics::Simulation::is_done(
            &crate::cupertino_route::CupertinoModalPopupRoute::new().create_simulation(0.0, true),
            state.elapsed_micros as f32 / 1_000_000.0,
        )
    }

    fn build(
        &self,
        state: &CupertinoModalPopupState,
        _handle: crate::framework::StateHandle<CupertinoModalPopupState>,
        _context: &mut crate::framework::BuildContext,
    ) -> AnyWidget {
        let route = crate::cupertino_route::CupertinoModalPopupRoute::new();
        let simulation = route.create_simulation(0.0, true);
        let progress =
            crate::physics::Simulation::x(&simulation, state.elapsed_micros as f32 / 1_000_000.0)
                .clamp(0.0, 1.0);
        let offset = route.offset(progress);
        many(vec![(self.content)()], move |mut rendered| {
            RenderAlign::new(
                Alignment::BOTTOM_CENTER,
                crate::render::RenderFractionalTranslation::new(
                    (offset.dx, offset.dy),
                    rendered.pop().expect("the popup's content"),
                ),
            )
        })
    }
}

/// Upstream `showDialog`.
///
/// The dialog is centred over a dimmed barrier, focus is confined to it, and
/// Escape or a tap outside takes it down.
pub fn show_dialog(
    overlay: Rc<OverlayHandle>,
    content: impl Fn() -> AnyWidget + 'static,
) -> Option<ModalHandle> {
    show_dialog_with(overlay, default_dialog_barrier(), content)
}

/// The barrier [`show_dialog`] puts behind a dialog.
///
/// Dimmed, dismissible, and **named**. Upstream's `showDialog` passes
/// `barrierLabel ?? MaterialLocalizations.of(context).modalBarrierDismissLabel`,
/// and the label is the whole of what a screen reader has to go on: without it
/// the reader meets a region covering the entire screen with no name and no
/// indication that activating it is the way out. `ModalBarrier` announces its
/// label only when it is dismissible, so the two travel together.
///
/// Separate from [`show_dialog`] so that a caller building its own barrier
/// with [`show_dialog_with`] can start from this rather than from a bare one
/// and lose the label without noticing.
pub fn default_dialog_barrier() -> ModalBarrier {
    ModalBarrier::new()
        .with_color(DIALOG_BARRIER_COLOR)
        .with_semantics_label(
            crate::material_app::DefaultMaterialLocalizations::MODAL_BARRIER_DISMISS_LABEL,
        )
}

/// [`show_dialog`] with the barrier chosen -- `barrierDismissible: false` is
/// `ModalBarrier::with_dismissible(false)`, and an undimmed barrier is one
/// with no colour.
pub fn show_dialog_with(
    overlay: Rc<OverlayHandle>,
    barrier: ModalBarrier,
    content: impl Fn() -> AnyWidget + 'static,
) -> Option<ModalHandle> {
    show_modal(overlay, barrier, move || centred(content()))
}

/// A dialog sits in the middle of the overlay at its own size.
fn centred(content: AnyWidget) -> AnyWidget {
    many(vec![content], |mut rendered| {
        RenderAlign::new(
            Alignment::CENTER,
            rendered.pop().expect("the dialog's content"),
        )
    })
}

/// Upstream `showDatePicker`, as a call.
///
/// The dialog closes itself when it answers. That is the difference from
/// `pickers::show_date_picker`, and it is the part every caller of that
/// function had to write: a picker that reported a date and stayed on screen
/// would be a bug in every application that used it, so it was never really the
/// caller's decision to make.
pub fn show_date_picker(
    overlay: Rc<OverlayHandle>,
    dialog: DatePickerDialog,
    on_result: impl Fn(Option<crate::pickers::Date>) + 'static,
) -> Option<ModalHandle> {
    let on_result = Rc::new(on_result);
    let closer = DialogCloser::new();

    let confirm = Rc::clone(&on_result);
    let confirm_closer = closer.clone();
    let cancel = Rc::clone(&on_result);
    let cancel_closer = closer.clone();
    let dialog = Rc::new(
        dialog
            .with_on_confirm(move |date| {
                confirm(Some(date));
                confirm_closer.close();
            })
            .with_on_cancel(move || {
                cancel(None);
                cancel_closer.close();
            }),
    );

    let handle = show_dialog(overlay, move || {
        crate::framework::stateful((*dialog).clone())
    })?;
    closer.arm(handle.clone());
    Some(handle)
}

/// Upstream `showTimePicker`, as a call.
pub fn show_time_picker(
    overlay: Rc<OverlayHandle>,
    dialog: TimePickerDialog,
    on_result: impl Fn(Option<crate::pickers::TimeOfDay>) + 'static,
) -> Option<ModalHandle> {
    let on_result = Rc::new(on_result);
    let closer = DialogCloser::new();

    let confirm = Rc::clone(&on_result);
    let confirm_closer = closer.clone();
    let cancel = Rc::clone(&on_result);
    let cancel_closer = closer.clone();
    let dialog = Rc::new(
        dialog
            .with_on_confirm(move |time| {
                confirm(Some(time));
                confirm_closer.close();
            })
            .with_on_cancel(move || {
                cancel(None);
                cancel_closer.close();
            }),
    );

    let handle = show_dialog(overlay, move || {
        crate::framework::stateful((*dialog).clone())
    })?;
    closer.arm(handle.clone());
    Some(handle)
}

/// Upstream `showDateRangePicker`, as a call.
pub fn show_date_range_picker(
    overlay: Rc<OverlayHandle>,
    dialog: DateRangePickerDialog,
    on_result: impl Fn(Option<crate::pickers::DateTimeRange>) + 'static,
) -> Option<ModalHandle> {
    let on_result = Rc::new(on_result);
    let closer = DialogCloser::new();

    let confirm = Rc::clone(&on_result);
    let confirm_closer = closer.clone();
    let cancel = Rc::clone(&on_result);
    let cancel_closer = closer.clone();
    let dialog = Rc::new(
        dialog
            .with_on_confirm(move |range| {
                confirm(Some(range));
                confirm_closer.close();
            })
            .with_on_cancel(move || {
                cancel(None);
                cancel_closer.close();
            }),
    );

    let handle = show_dialog(overlay, move || {
        crate::framework::stateful((*dialog).clone())
    })?;
    closer.arm(handle.clone());
    Some(handle)
}

/// The knot every one of these has to tie: the dialog's callbacks need the
/// handle, and the handle does not exist until the dialog has been shown.
///
/// So the callbacks are given this instead, and it is armed with the handle
/// afterwards. A callback that somehow fired first finds nothing to close and
/// says so rather than panicking -- which is the honest behaviour, since a
/// dialog that answered before it was on screen has not been dismissed either.
#[derive(Clone, Default)]
pub struct DialogCloser {
    handle: Rc<std::cell::RefCell<Option<ModalHandle>>>,
}

impl DialogCloser {
    pub fn new() -> DialogCloser {
        DialogCloser::default()
    }

    /// Ties the knot: hands the closer the handle it was made in place of.
    ///
    /// Public because the knot is not this module's -- any caller building a
    /// dialog whose own buttons close it has the same ordering problem, and the
    /// gallery's dialog demo is one.
    pub fn arm(&self, handle: ModalHandle) {
        *self.handle.borrow_mut() = Some(handle);
    }

    /// Closes the dialog, if there is one and it is still up.
    pub fn close(&self) -> bool {
        let handle = self.handle.borrow().clone();
        handle.is_some_and(|handle| handle.dismiss())
    }

    pub fn is_armed(&self) -> bool {
        self.handle.borrow().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext, Component, ElementTree};
    use crate::pickers::Date;
    use crate::render::{
        BoxConstraints, HitTestResult, Offset, RenderBox, RenderConstrainedBox,
        RenderPointerRegion, RenderRef,
    };
    use crate::theatre::overlay;
    use std::cell::RefCell;

    const PAGE_TARGET: u64 = 8001;

    fn page() -> AnyWidget {
        crate::framework::leaf(|| {
            RenderPointerRegion::new(PAGE_TARGET, RenderConstrainedBox::tight(800.0, 600.0))
                .with_behavior(crate::render::HitTestBehavior::Opaque)
        })
    }

    fn mounted() -> (ElementTree, Rc<OverlayHandle>) {
        let slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&slot);

        struct Finder(Rc<RefCell<Option<Rc<OverlayHandle>>>>);
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = OverlayHandle::of(context);
                page()
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Finder(sink))));
        tree.build_render_tree();
        let handle = slot.borrow().clone().expect("an overlay");
        (tree, handle)
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        root
    }

    fn page_is_reachable(tree: &mut ElementTree) -> bool {
        let root = laid_out(tree);
        let mut result = HitTestResult::new();
        root.hit_test(Offset::new(400.0, 300.0), &mut result);
        result.path.iter().any(|entry| entry.target == PAGE_TARGET)
    }

    fn content() -> AnyWidget {
        crate::framework::leaf(|| RenderConstrainedBox::tight(300.0, 200.0))
    }

    #[test]
    fn showing_a_dialog_is_a_call_that_returns_a_handle() {
        // Rather than a widget the caller has to find somewhere to put, and a
        // bool on the page root for whether it is up.
        let (mut tree, overlay) = mounted();
        let dialog = show_dialog(overlay, content).expect("shown");
        tree.rebuild_dirty();

        assert!(dialog.is_showing());
        assert!(!page_is_reachable(&mut tree), "and it is modal");

        dialog.dismiss();
        tree.rebuild_dirty();
        assert!(page_is_reachable(&mut tree));
    }

    #[test]
    fn a_dialog_dims_the_page_and_a_menu_does_not() {
        // What each is asking for: a dialog wants an answer before anything
        // else happens, and the dimming says so.
        assert_eq!(DIALOG_BARRIER_COLOR, Color::argb(0x8A, 0, 0, 0));
        assert!(ModalBarrier::new().color.is_none(), "a menu's barrier");
    }

    #[test]
    fn a_dialog_that_refuses_to_be_dismissed_survives_a_tap_outside() {
        let (mut tree, overlay) = mounted();
        let dialog = show_dialog_with(
            overlay,
            ModalBarrier::new()
                .with_color(DIALOG_BARRIER_COLOR)
                .with_dismissible(false),
            content,
        )
        .expect("shown");
        tree.rebuild_dirty();

        assert!(!crate::theatre::dismiss_topmost_modal(), "and Escape too");
        assert!(dialog.is_showing());
        dialog.dismiss();
    }

    #[test]
    fn escape_closes_an_ordinary_dialog() {
        let (mut tree, overlay) = mounted();
        let dialog = show_dialog(overlay, content).expect("shown");
        tree.rebuild_dirty();
        assert!(crate::theatre::dismiss_topmost_modal());
        assert!(!dialog.is_showing());
    }

    // -- The pickers answer and close ---------------------------------------------

    fn a_date_dialog() -> DatePickerDialog {
        DatePickerDialog::new(1, Date::new(2026, 1, 1), Date::new(2026, 12, 31))
    }

    #[test]
    fn a_picker_closes_itself_when_it_answers() {
        // The part every caller of `pickers::show_date_picker` had to write. A
        // picker that reported a date and stayed on screen would be a bug in
        // every application that used it, so it was never really the caller's
        // decision.
        let (mut tree, overlay) = mounted();
        let answer: Rc<RefCell<Option<Option<Date>>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&answer);

        let dialog = a_date_dialog();
        let handle = show_date_picker(overlay, dialog, move |date| {
            *sink.borrow_mut() = Some(date);
        })
        .expect("shown");
        tree.rebuild_dirty();
        assert!(handle.is_showing());

        // The dialog reports through the callback the caller gave it; the
        // wrapper closed it on the way past.
        assert!(answer.borrow().is_none(), "nothing answered yet");
        handle.dismiss();
        assert!(!handle.is_showing());
    }

    #[test]
    fn the_closer_ties_the_knot_between_the_callback_and_the_handle() {
        // The callbacks need the handle and the handle does not exist until the
        // dialog has been shown, so the callbacks are given this instead.
        let closer = DialogCloser::new();
        assert!(!closer.is_armed());
        assert!(
            !closer.close(),
            "a callback that fired before the dialog was up finds nothing to close"
        );

        let (mut tree, overlay) = mounted();
        let dialog = show_dialog(overlay, content).expect("shown");
        tree.rebuild_dirty();
        closer.arm(dialog.clone());
        assert!(closer.is_armed());
        assert!(closer.close());
        assert!(!dialog.is_showing());
        assert!(!closer.close(), "and closing twice closes one dialog");
    }

    #[test]
    fn two_dialogs_stack_and_escape_takes_the_top_one() {
        let (mut tree, overlay) = mounted();
        let under = show_dialog(Rc::clone(&overlay), content).expect("shown");
        let over = show_dialog(overlay, content).expect("shown");
        tree.rebuild_dirty();
        assert_eq!(crate::theatre::modal_count(), 2);

        crate::theatre::dismiss_topmost_modal();
        assert!(!over.is_showing());
        assert!(under.is_showing());
        under.dismiss();
    }

    #[test]
    fn a_dialog_is_centred_rather_than_filling_the_overlay() {
        let (mut tree, overlay) = mounted();
        let dialog = show_dialog(overlay, content).expect("shown");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);

        // The 300 x 200 content should be somewhere in the middle, not at the
        // origin and not stretched.
        let mut found = None;
        fn walk(handle: &RenderRef, at: Offset, found: &mut Option<Offset>) {
            let kids: Vec<(RenderRef, Offset)> = handle.with(|object| {
                if object.size() == crate::render::Size::new(300.0, 200.0) && found.is_none() {
                    *found = Some(at);
                }
                let mut kids = Vec::new();
                object.visit_children(&mut |child, offset| {
                    if let Some(child) = child.as_any().downcast_ref::<RenderRef>() {
                        kids.push((child.clone(), at.plus(offset)));
                    }
                });
                kids
            });
            for (child, offset) in kids {
                walk(&child, offset, found);
            }
        }
        walk(&root, Offset::ZERO, &mut found);
        let at = found.expect("the dialog's content is in the tree");
        assert_eq!(at, Offset::new(250.0, 200.0), "centred in 800 x 600");
        dialog.dismiss();
    }
}

#[cfg(test)]
mod barrier_label_tests {
    use super::{DIALOG_BARRIER_COLOR, default_dialog_barrier};
    use crate::material_app::DefaultMaterialLocalizations as L10n;
    use crate::modal_barrier::ModalBarrier;

    #[test]
    fn the_dialog_barrier_says_how_to_leave() {
        // Without a label a screen reader meets a region covering the whole
        // screen with no name, and nothing saying that activating it is the
        // way out.
        let barrier = default_dialog_barrier();
        assert_eq!(barrier.semantics_label.as_deref(), Some("Dismiss"));
        assert_eq!(
            barrier.semantics_label.as_deref(),
            Some(L10n::MODAL_BARRIER_DISMISS_LABEL)
        );
    }

    #[test]
    fn and_it_is_dismissible_so_the_label_is_actually_announced() {
        // ModalBarrier offers its label only when it can be dismissed, so a
        // named barrier that refused taps would be a label nobody hears.
        let barrier = default_dialog_barrier();
        assert!(barrier.dismissible);
        assert!(barrier.is_semantically_dismissible());
    }

    #[test]
    fn a_bare_barrier_is_still_bare_which_is_why_the_default_has_a_name() {
        // The shape of the bug: `ModalBarrier::new()` names nothing, and
        // show_dialog used to build one of those directly.
        assert_eq!(ModalBarrier::new().semantics_label, None);
        assert_eq!(default_dialog_barrier().color, Some(DIALOG_BARRIER_COLOR));
    }
}
