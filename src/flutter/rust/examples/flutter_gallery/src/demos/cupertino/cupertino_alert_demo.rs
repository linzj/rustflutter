// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_alert_demo.dart` (flutter/
//! gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoAlertDemo` takes an `AlertDemoType` and shows one
//! variant per catalogue configuration, each variant a page whose centred
//! filled `CupertinoButton(child: Text(cupertinoShowAlert))` presents its
//! `CupertinoDialogRoute` (or, for the action sheet, `CupertinoModalPopupRoute`)
//! and reports the popped value below the button as
//! `dialogSelectedOption(value)`. The catalogue here is flattened to one
//! configuration per demo (PORTING.md: "demo options section is unreachable"),
//! so -- as `material/dialog_demo.rs` does -- the stage stacks all five
//! variants: a caption with the variant's upstream title and its SHOW ALERT
//! button. The nav bar therefore carries the demo's own title
//! (`demoCupertinoAlertsTitle`) rather than the per-variant `_title`.
//!
//! State: upstream's `_CupertinoAlertDemoState` holds five
//! `RestorableRouteFuture<String>`s and `lastSelectedValue`; [`AlertDemoState`]
//! holds the open variant and the last selection. `RestorationMixin` has no
//! counterpart and is not carried, as in every material demo port.
//!
//! The modal is part of the stage's own stack rather than `mod.rs`'s shared
//! `overlay()`: the overlay dispatch reads only `GalleryState`, and this
//! demo's state is its own (PORTING.md: per-demo state where upstream has a
//! per-demo `State`). The modal still covers the same demo-card area the
//! shared overlay would.
//!
//! Divergences, each also marked at its site:
//!
//! - **the action sheet is an alert dialog.** Upstream's `_modalRoute` builds
//!   a `CupertinoActionSheet` that slides up from the bottom with a detached
//!   cancel button; the framework's cupertino tier has no action sheet, so
//!   the same title, message and choices are presented in a
//!   [`CupertinoAlertDialog`], centred like the other variants.
//! - **no entrance animation.** Upstream's dialog routes fade and scale in
//!   over 250ms; the dialog here appears with the frame the tap schedules,
//!   the same presentation the material dialog port uses.
//! - **the scaffold is a fixed height.** Upstream's `DemoWrapper` gives the
//!   demo the page's content height; the demo page here renders each stage in
//!   a scrolling column at its intrinsic height, so the scaffold gets
//!   [`DEMO_HEIGHT`] to stand in for the screen's remainder.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Center, Empty, FullWidth, Pointer, Positioned, Stack};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The height the scaffold stands in for; see the module header.
const DEMO_HEIGHT: f32 = 700.0;

/// The barrier the modals sit over: cupertino/route.dart's
/// `kCupertinoModalBarrierColor`, resolved for the light appearance the demo
/// always runs in (`DemoWrapper` in `lib/pages/demo.dart`). Tapping it
/// dismisses without a selection, as upstream's barrier completes the route
/// with null and the demo's `String`-typed `onComplete` never fires for it.
const BARRIER_COLOR: Color = Color(0x3300_0000);

/// Upstream's `AlertDemoType` (`demo_types.rs` keeps the enum as metadata;
/// this is the value the demo actually runs on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AlertKind {
    Alert,
    AlertTitle,
    AlertButtons,
    AlertButtonsOnly,
    ActionSheet,
}

/// The five variants in upstream's catalogue order, with the titles
/// upstream's `_title` gives each variant's page.
const SECTIONS: [AlertKind; 5] = [
    AlertKind::Alert,
    AlertKind::AlertTitle,
    AlertKind::AlertButtons,
    AlertKind::AlertButtonsOnly,
    AlertKind::ActionSheet,
];

/// Upstream's `_title` for a variant.
fn variant_title(kind: AlertKind) -> &'static str {
    let l10n = GalleryLocalizations::en();
    match kind {
        AlertKind::Alert => l10n.demo_cupertino_alert_title(),
        AlertKind::AlertTitle => l10n.demo_cupertino_alert_with_title_title(),
        AlertKind::AlertButtons => l10n.demo_cupertino_alert_buttons_title(),
        AlertKind::AlertButtonsOnly => l10n.demo_cupertino_alert_buttons_only_title(),
        AlertKind::ActionSheet => l10n.demo_cupertino_action_sheet_title(),
    }
}

/// The demo body for the `cupertino-alerts` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(AlertDemo)
}

/// Upstream's `CupertinoAlertDemo`.
struct AlertDemo;

/// Upstream's `_CupertinoAlertDemoState`, minus the restoration wrappers.
#[derive(Default)]
struct AlertDemoState {
    /// Which variant's dialog is up, if one is. Upstream keeps one
    /// `RestorableRouteFuture` per variant; only one route can be up at a
    /// time, so the open variant is the whole of it.
    open: Option<AlertKind>,
    /// Upstream's `lastSelectedValue`.
    last_selected: Option<&'static str>,
    /// The held launch button, for its pressed fade.
    pressed: Option<u64>,
}

/// Records a dialog's popped value and closes it: upstream's
/// `_setSelectedValue` as the routes' `onComplete`.
fn choose(state: &mut AlertDemoState, value: &'static str) {
    state.last_selected = Some(value);
    state.open = None;
}

// One `fn` per pop value, because `CupertinoAlertAction::wired` takes a plain
// `fn` and each action pops with its own label upstream. The labels are the
// l10n getters of the same names.
fn choose_discard(state: &mut AlertDemoState) {
    choose(state, GalleryLocalizations::en().cupertino_alert_discard());
}
fn choose_cancel(state: &mut AlertDemoState) {
    choose(state, GalleryLocalizations::en().cupertino_alert_cancel());
}
fn choose_dont_allow(state: &mut AlertDemoState) {
    choose(
        state,
        GalleryLocalizations::en().cupertino_alert_dont_allow(),
    );
}
fn choose_allow(state: &mut AlertDemoState) {
    choose(state, GalleryLocalizations::en().cupertino_alert_allow());
}
fn choose_cheesecake(state: &mut AlertDemoState) {
    choose(
        state,
        GalleryLocalizations::en().cupertino_alert_cheesecake(),
    );
}
fn choose_tiramisu(state: &mut AlertDemoState) {
    choose(state, GalleryLocalizations::en().cupertino_alert_tiramisu());
}
fn choose_apple_pie(state: &mut AlertDemoState) {
    choose(
        state,
        GalleryLocalizations::en().cupertino_alert_apple_pie(),
    );
}
fn choose_chocolate_brownie(state: &mut AlertDemoState) {
    choose(
        state,
        GalleryLocalizations::en().cupertino_alert_chocolate_brownie(),
    );
}

// The launch buttons open a variant; same plain-`fn` constraint.
fn open_alert(state: &mut AlertDemoState) {
    state.open = Some(AlertKind::Alert);
}
fn open_alert_title(state: &mut AlertDemoState) {
    state.open = Some(AlertKind::AlertTitle);
}
fn open_alert_buttons(state: &mut AlertDemoState) {
    state.open = Some(AlertKind::AlertButtons);
}
fn open_alert_buttons_only(state: &mut AlertDemoState) {
    state.open = Some(AlertKind::AlertButtonsOnly);
}
fn open_action_sheet(state: &mut AlertDemoState) {
    state.open = Some(AlertKind::ActionSheet);
}

/// The launcher for a variant.
fn open_action(kind: AlertKind) -> fn(&mut AlertDemoState) {
    match kind {
        AlertKind::Alert => open_alert,
        AlertKind::AlertTitle => open_alert_title,
        AlertKind::AlertButtons => open_alert_buttons,
        AlertKind::AlertButtonsOnly => open_alert_buttons_only,
        AlertKind::ActionSheet => open_action_sheet,
    }
}

impl StatefulComponent for AlertDemo {
    type State = AlertDemoState;

    fn build(
        &self,
        state: &AlertDemoState,
        handle: StateHandle<AlertDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let theme = cupertino_theme_of(context);

        // One section per variant: the variant's upstream title as a caption,
        // then its centred filled button -- the five configurations stacked,
        // as `material/dialog_demo.rs` stacks its four.
        let mut children: Vec<AnyWidget> = Vec::new();
        for (index, kind) in SECTIONS.iter().enumerate() {
            let kind = *kind;
            let id = ids::DEMO_LOCAL + index as u64;
            let caption = theme.resolve(CupertinoColors::SECONDARY_LABEL);
            children.push(leaf(move || {
                Text::new(variant_title(kind)).with_style(TextStyle {
                    font_size: 13.0,
                    font_weight: 600,
                    color: caption,
                    ..TextStyle::default()
                })
            }));
            let button = component(
                CupertinoButton::filled(id, l10n.cupertino_show_alert())
                    .with_pressed(state.pressed == Some(id))
                    .wired(
                        handle.clone(),
                        |state| &mut state.pressed,
                        open_action(kind),
                    ),
            );
            children.push(single(button, |button| {
                Box::new(FullWidth::new(Center::new(button)))
            }));
        }

        // Upstream's `if (lastSelectedValue.value != null)` row: the popped
        // value, padded 16 and centred, in the theme's text style.
        if let Some(selected) = state.last_selected {
            let text = l10n.dialog_selected_option(selected);
            let style = theme.text_style();
            children.push(leaf(move || {
                Container::new()
                    .with_padding(EdgeInsets::all(16.0))
                    .with_child(FullWidth::new(Center::new(
                        Text::new(text.clone())
                            .with_style(style.clone())
                            .with_align(TextAlign::Center),
                    )))
            }));
        }

        let body = many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(12.0);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::all(24.0))
                    .with_child(column),
            )
        });

        let scaffold = component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // The demo's own title rather than the per-variant `_title`;
                // see the module header.
                CupertinoNavigationBar::new().with_middle(l10n.demo_cupertino_alerts_title()),
            )),
        );

        // The fixed height stands in for the content height; see the module
        // header.
        let stage = single(scaffold, |scaffold| {
            Box::new(
                Container::new()
                    .with_height(DEMO_HEIGHT)
                    .with_child(scaffold),
            )
        });

        // The open variant's modal over the stage: barrier, then the dialog
        // centred. Presentation is the app's (the framework tier documents
        // this); see the module header for why this is not `mod.rs`'s
        // `overlay()`.
        let content = match state.open {
            None => stage,
            Some(kind) => {
                let barrier_handle = handle.clone();
                let barrier = leaf(move || {
                    Pointer::new(ids::SCRIM, Container::new().with_color(BARRIER_COLOR))
                        .with_handlers(rustflutter::gestures::PointerHandlers::new().with_tap({
                            let handle = barrier_handle.clone();
                            // Barrier dismiss pops no value (see
                            // BARRIER_COLOR), so the last selection stays.
                            move |_| {
                                handle.set_state(|state| state.open = None);
                            }
                        }))
                });
                let dialog = dialog(kind, handle);
                many(vec![stage, barrier, dialog], |mut rendered| {
                    let dialog = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    let barrier = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    let stage = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    Box::new(
                        Stack::new()
                            .push(stage)
                            .push_positioned(barrier, Positioned::fill())
                            .push(Center::new(dialog)),
                    )
                })
            }
        };

        // Upstream's `DemoWrapper` wraps every demo in a light
        // `CupertinoTheme` (`lib/pages/demo.dart`), whatever the app's
        // brightness.
        provide(CupertinoTheme::light(), content)
    }
}

/// One dialog action that pops with its own label.
fn action(
    id: u64,
    label: &str,
    pop: fn(&mut AlertDemoState),
    handle: &StateHandle<AlertDemoState>,
) -> CupertinoAlertAction {
    CupertinoAlertAction::new(id, label).wired(handle.clone(), pop)
}

/// The open variant's dialog. Upstream's `_alertDemoDialog`,
/// `_alertWithTitleDialog`, `_alertWithButtonsDialog`,
/// `_alertWithButtonsOnlyDialog` and `_modalRoute`.
fn dialog(kind: AlertKind, handle: StateHandle<AlertDemoState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    // Action ids start clear of the launch buttons'.
    let base = ids::DEMO_LOCAL + 10;
    let dialog = match kind {
        // Upstream's `_alertDemoDialog`: a title, a destructive Discard and a
        // default Cancel.
        AlertKind::Alert => CupertinoAlertDialog::new()
            .with_title(l10n.dialog_discard_title())
            .with_action(stateful(
                action(
                    base,
                    l10n.cupertino_alert_discard(),
                    choose_discard,
                    &handle,
                )
                .destructive(),
            ))
            .with_action(stateful(
                action(
                    base + 1,
                    l10n.cupertino_alert_cancel(),
                    choose_cancel,
                    &handle,
                )
                .default_action(),
            )),
        // Upstream's `_alertWithTitleDialog`.
        AlertKind::AlertTitle => CupertinoAlertDialog::new()
            .with_title(l10n.cupertino_alert_location_title())
            .with_content(l10n.cupertino_alert_location_description())
            .with_action(stateful(action(
                base,
                l10n.cupertino_alert_dont_allow(),
                choose_dont_allow,
                &handle,
            )))
            .with_action(stateful(action(
                base + 1,
                l10n.cupertino_alert_allow(),
                choose_allow,
                &handle,
            ))),
        // Upstream's `_alertWithButtonsDialog`: the dessert dialog with its
        // title and content.
        AlertKind::AlertButtons => dessert_dialog(&handle)
            .with_title(l10n.cupertino_alert_favorite_dessert())
            .with_content(l10n.cupertino_alert_dessert_description()),
        // Upstream's `_alertWithButtonsOnlyDialog`: the same dialog bare.
        AlertKind::AlertButtonsOnly => dessert_dialog(&handle),
        // Upstream's `_modalRoute`, approximated: a `CupertinoActionSheet`
        // slides up from the bottom with the three desserts and a detached,
        // default-styled Cancel. The framework tier has no action sheet, so
        // the alert surface presents the same choices (see the module
        // header); Cancel keeps its `isDefaultAction` styling.
        AlertKind::ActionSheet => CupertinoAlertDialog::new()
            .with_title(l10n.cupertino_alert_favorite_dessert())
            .with_content(l10n.cupertino_alert_dessert_description())
            .with_action(stateful(action(
                base,
                l10n.cupertino_alert_cheesecake(),
                choose_cheesecake,
                &handle,
            )))
            .with_action(stateful(action(
                base + 1,
                l10n.cupertino_alert_tiramisu(),
                choose_tiramisu,
                &handle,
            )))
            .with_action(stateful(action(
                base + 2,
                l10n.cupertino_alert_apple_pie(),
                choose_apple_pie,
                &handle,
            )))
            .with_action(stateful(
                action(
                    base + 3,
                    l10n.cupertino_alert_cancel(),
                    choose_cancel,
                    &handle,
                )
                .default_action(),
            )),
    };
    component(dialog)
}

/// Upstream's `CupertinoDessertDialog`: the four desserts, then a destructive
/// Cancel. Title and content are the caller's -- the buttons-only variant has
/// neither.
fn dessert_dialog(handle: &StateHandle<AlertDemoState>) -> CupertinoAlertDialog {
    let l10n = GalleryLocalizations::en();
    let base = ids::DEMO_LOCAL + 10;
    CupertinoAlertDialog::new()
        .with_action(stateful(action(
            base,
            l10n.cupertino_alert_cheesecake(),
            choose_cheesecake,
            handle,
        )))
        .with_action(stateful(action(
            base + 1,
            l10n.cupertino_alert_tiramisu(),
            choose_tiramisu,
            handle,
        )))
        .with_action(stateful(action(
            base + 2,
            l10n.cupertino_alert_apple_pie(),
            choose_apple_pie,
            handle,
        )))
        .with_action(stateful(action(
            base + 3,
            l10n.cupertino_alert_chocolate_brownie(),
            choose_chocolate_brownie,
            handle,
        )))
        .with_action(stateful(
            action(
                base + 4,
                l10n.cupertino_alert_cancel(),
                choose_cancel,
                handle,
            )
            .destructive(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::light(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn the_five_variants_are_upstreams_in_order() {
        assert_eq!(
            SECTIONS.map(variant_title),
            [
                "Alert",
                "Alert With Title",
                "Alert With Buttons",
                "Alert Buttons Only",
                "Action Sheet"
            ]
        );
    }

    #[test]
    fn choosing_a_value_closes_the_dialog_and_remembers_it() {
        let mut state = AlertDemoState::default();
        open_alert_buttons(&mut state);
        assert_eq!(state.open, Some(AlertKind::AlertButtons));
        choose_tiramisu(&mut state);
        assert_eq!(state.open, None);
        assert_eq!(state.last_selected, Some("Tiramisu"));
    }

    #[test]
    fn a_barrier_dismiss_keeps_the_last_selection() {
        // Upstream's barrier completes the route with null, which the
        // demo's non-nullable `onComplete` never sees; the previous
        // selection stays on screen.
        let mut state = AlertDemoState::default();
        choose_cancel(&mut state);
        open_alert(&mut state);
        state.open = None; // what the barrier's tap does
        assert_eq!(state.last_selected, Some("Cancel"));
    }

    #[test]
    fn the_selected_option_text_matches_upstream() {
        // `dialogSelectedOption`, the row a pop leaves behind.
        let text = GalleryLocalizations::en().dialog_selected_option("Cancel");
        assert_eq!(text, "You selected: \"Cancel\"");
    }

    #[test]
    fn every_variant_dialog_lays_out() {
        // The two-action variants get the side-by-side row, the dessert ones
        // the stacked actions; the check is that all five build and lay out.
        // The actions are built unwired, which is what the framework's own
        // dialog tests do.
        for kind in SECTIONS {
            let size = lay_out(component(unwired_dialog(kind)), 428.0);
            assert!(
                size.width <= 428.0 && size.height > 0.0,
                "{kind:?}: {size:?}"
            );
        }
    }

    /// The variant's dialog with its actions unwired, for layout tests.
    fn unwired_dialog(kind: AlertKind) -> CupertinoAlertDialog {
        let l10n = GalleryLocalizations::en();
        match kind {
            AlertKind::Alert => CupertinoAlertDialog::new()
                .with_title(l10n.dialog_discard_title())
                .with_action(stateful(CupertinoAlertAction::new(
                    1,
                    l10n.cupertino_alert_discard(),
                )))
                .with_action(stateful(CupertinoAlertAction::new(
                    2,
                    l10n.cupertino_alert_cancel(),
                ))),
            AlertKind::AlertTitle => CupertinoAlertDialog::new()
                .with_title(l10n.cupertino_alert_location_title())
                .with_content(l10n.cupertino_alert_location_description())
                .with_action(stateful(CupertinoAlertAction::new(
                    1,
                    l10n.cupertino_alert_dont_allow(),
                )))
                .with_action(stateful(CupertinoAlertAction::new(
                    2,
                    l10n.cupertino_alert_allow(),
                ))),
            AlertKind::AlertButtons => unwired_dessert_dialog()
                .with_title(l10n.cupertino_alert_favorite_dessert())
                .with_content(l10n.cupertino_alert_dessert_description()),
            AlertKind::AlertButtonsOnly => unwired_dessert_dialog(),
            AlertKind::ActionSheet => CupertinoAlertDialog::new()
                .with_title(l10n.cupertino_alert_favorite_dessert())
                .with_content(l10n.cupertino_alert_dessert_description())
                .with_action(stateful(CupertinoAlertAction::new(
                    1,
                    l10n.cupertino_alert_cheesecake(),
                )))
                .with_action(stateful(CupertinoAlertAction::new(
                    2,
                    l10n.cupertino_alert_tiramisu(),
                )))
                .with_action(stateful(CupertinoAlertAction::new(
                    3,
                    l10n.cupertino_alert_apple_pie(),
                )))
                .with_action(stateful(CupertinoAlertAction::new(
                    4,
                    l10n.cupertino_alert_cancel(),
                ))),
        }
    }

    fn unwired_dessert_dialog() -> CupertinoAlertDialog {
        let l10n = GalleryLocalizations::en();
        CupertinoAlertDialog::new()
            .with_action(stateful(CupertinoAlertAction::new(
                1,
                l10n.cupertino_alert_cheesecake(),
            )))
            .with_action(stateful(CupertinoAlertAction::new(
                2,
                l10n.cupertino_alert_tiramisu(),
            )))
            .with_action(stateful(CupertinoAlertAction::new(
                3,
                l10n.cupertino_alert_apple_pie(),
            )))
            .with_action(stateful(CupertinoAlertAction::new(
                4,
                l10n.cupertino_alert_chocolate_brownie(),
            )))
            .with_action(stateful(CupertinoAlertAction::new(
                5,
                l10n.cupertino_alert_cancel(),
            )))
    }

    #[test]
    fn the_stage_is_a_scaffold_at_the_stand_in_height() {
        let size = lay_out(stage(), 428.0);
        assert_eq!(size.height, DEMO_HEIGHT);
        assert_eq!(size.width, 428.0);
    }
}
