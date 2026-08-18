// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Maps to `lib/demos/reference/transformations_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TransformationsDemo`: an `InteractiveViewer` over a hex-tile
//! board, panned and zoomed by gesture, with a tap selecting a tile and two
//! footer buttons -- reset to the home transform, edit the selected tile's
//! color. [`TransformationsDemo`] is upstream's `_TransformationsDemoState`
//! (the widget holds nothing but the framework), [`BoardRender`] is
//! `_BoardPainter` plus its `CustomPaint`, and the `InteractiveViewer` is the
//! framework's (`rustflutter::interactive_viewer`), configured as upstream's
//! is: `boundaryMargin` one viewport on every side, `minScale` 0.01, the
//! default `maxScale` 2.5.
//!
//! This file replaces the pre-split layout stand-in the skeleton hosted here
//! (a flex sample and a main-axis alignment sampler); that stand-in is gone,
//! and with it the PORTING.md entry "2d-transformations is a flex demo".
//!
//! Divergences, each marked at its site as well:
//!
//! * **the viewport is a fixed window** ([`VIEWPORT_WIDTH`] x
//!   [`VIEWPORT_HEIGHT`]) -- upstream's `LayoutBuilder` sizes the scene to
//!   the screen. The demo page's stage does not guarantee bounded
//!   constraints, and the framework's `LayoutBuilder` equivalent
//!   (`RenderSizeReporter`) answers a frame late, which a one-frame `--png`
//!   render would catch before the home matrix existed. A fixed window is
//!   the same call the colors and typography demos make ("lists are
//!   height-bounded"), and it makes the home transform computable on the
//!   first frame, as upstream's first `LayoutBuilder` pass does. At 380 wide
//!   it is the size of a phone screen, which is the configuration upstream's
//!   demo is built around.
//! * **the edit panel is in the stage, not a modal bottom sheet** --
//!   upstream's `showModalBottomSheet` slides over the screen with a scrim.
//!   The demo page gives a demo one overlay slot, dispatched from
//!   `GalleryState`, and this demo's state is a per-demo
//!   `StatefulComponent`'s, so the sheet renders bottom-anchored over the
//!   board window itself (the call the Cupertino batch made for its modals:
//!   "modals render in the stage's own stack"). There is no barrier tap to
//!   dismiss; choosing a color closes it, as upstream's `Navigator.pop`
//!   does.
//! * **the footer icons keep their tooltips as titles** -- upstream's
//!   `IconButton(tooltip: ...)` labels are the buttons' accessible names;
//!   with no tooltip layer wired here they are the text under each button.
//! * **the reset animation lerps translation and scale** -- upstream's
//!   `Matrix4Tween` decomposes the matrices and lerps the pieces; every
//!   matrix here is a translation and a uniform scale, so lerping those two
//!   is the same interpolation.

use std::rc::Rc;

use rustflutter::framework::{single, BuildContext};
use rustflutter::gestures::PointerHandlers;
use rustflutter::interactive_viewer::{
    interactive_viewer, Affine2D, InteractiveViewer, TransformationController,
};
use rustflutter::painting::RenderPath;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, MainAxisAlignment, MainAxisSize, Offset,
    PaintContext, RenderBox, RenderFlex, RenderStack, Size, StackPosition, UpdateEffect,
};
use rustflutter::widgets::{Align, Pointer};

use crate::app::ids;
use crate::data::demos::{self, icon};

use super::transformations_demo_board::{Board, BoardPoint};
use super::transformations_demo_edit_board_point::{EditBoardPoint, BACKGROUND_COLOR};

/// The radius of a hexagon tile in pixels. Upstream's `_kHexagonRadius`.
const HEXAGON_RADIUS: f32 = 16.0;

/// The margin between hexagons. Upstream's `_kHexagonMargin`.
const HEXAGON_MARGIN: f32 = 1.0;

/// The radius of the entire board in hexagons, not including the center.
/// Upstream's `_kBoardRadius`.
const BOARD_RADIUS: i32 = 8;

/// The window the board is viewed through. Upstream's is the screen; see the
/// module header.
const VIEWPORT_WIDTH: f32 = 380.0;
const VIEWPORT_HEIGHT: f32 = 480.0;

/// How long the reset-to-home animation runs. Upstream's
/// `_controllerReset.duration`, 400ms.
const RESET_DURATION_MICROS: i64 = 400_000;

/// The footer buttons' glyphs. Upstream's `Icons.replay` and `Icons.edit`;
/// the codepoints are the font's `replay_baseline` and `edit_baseline`,
/// drawn from the registered Material Icons font the way the demo page's
/// chrome draws its icons. The catalogue's own icon module is generated, so
/// these live here.
const REPLAY_ICON: &str = "\u{e523}";
const EDIT_ICON: &str = "\u{e21a}";

/// The demo body for the `2d-transformations` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(TransformationsDemo)
}

/// Upstream's `TransformationsDemo` widget. Everything it carried is in the
/// state.
struct TransformationsDemo;

/// Upstream's `_TransformationsDemoState`.
struct TransformationsDemoState {
    /// `_board`.
    board: Board,
    /// `_transformationController`.
    transformer: TransformationController,
    /// `_homeMatrix`: the transform that centers the board in the viewport.
    home_matrix: Affine2D,
    /// The running reset-to-home animation: upstream's `_animationReset` over
    /// `_controllerReset`, as the matrices it tweens between and when the
    /// first frame after the tap ran.
    reset: Option<ResetAnimation>,
    /// Whether the edit panel is showing. Upstream's modal bottom sheet route.
    editing: bool,
    /// Which footer button is held down, for the pressed highlight the demo
    /// page's own icon buttons show.
    pressed: Option<u64>,
}

/// The reset animation in flight: `_animationReset`'s begin and end, and the
/// frame clock when it first advanced.
#[derive(Clone, Copy, Debug)]
struct ResetAnimation {
    from: Affine2D,
    to: Affine2D,
    started_micros: Option<i64>,
}

impl TransformationsDemoState {
    fn new() -> TransformationsDemoState {
        let board = Board::new(BOARD_RADIUS, HEXAGON_RADIUS, HEXAGON_MARGIN, None, None);
        // Upstream computes `_homeMatrix` in the first `LayoutBuilder` pass:
        // the board centered in the viewport. The viewport is a fixed window
        // here (module header), so the same centering is computable up front.
        let board_size = board.size();
        let home_matrix = Affine2D::IDENTITY.translate_by_double(
            VIEWPORT_WIDTH / 2.0 - board_size.width / 2.0,
            VIEWPORT_HEIGHT / 2.0 - board_size.height / 2.0,
        );
        TransformationsDemoState {
            board,
            transformer: TransformationController::with_value(home_matrix),
            home_matrix,
            reset: None,
            editing: false,
            pressed: None,
        }
    }
}

impl Default for TransformationsDemoState {
    fn default() -> TransformationsDemoState {
        TransformationsDemoState::new()
    }
}

impl StatefulComponent for TransformationsDemo {
    type State = TransformationsDemoState;

    fn initial_state(&self) -> TransformationsDemoState {
        TransformationsDemoState::new()
    }

    fn advance(&self, state: &mut TransformationsDemoState, frame_time_micros: i64) -> bool {
        // `_onAnimateReset`: the controller's ticks write the tween's value
        // into the transformation controller.
        let Some(reset) = &mut state.reset else {
            return false;
        };
        let started = *reset.started_micros.get_or_insert(frame_time_micros);
        let elapsed = (frame_time_micros - started).max(0);
        let t = (elapsed as f32 / RESET_DURATION_MICROS as f32).clamp(0.0, 1.0);
        // `Matrix4Tween` on a translate+uniform-scale matrix is the lerp of
        // the translation and the scale (module header). No curve: upstream's
        // `_controllerReset` runs linear.
        let from = (reset.from.translation(), reset.from.max_scale_on_axis());
        let to = (reset.to.translation(), reset.to.max_scale_on_axis());
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let scale = lerp(from.1, to.1);
        state.transformer.set_value(Affine2D([
            scale,
            0.0,
            0.0,
            scale,
            lerp(from.0.dx, to.0.dx),
            lerp(from.0.dy, to.0.dy),
        ]));
        if t >= 1.0 {
            // `_onAnimateReset`'s `!_controllerReset.isAnimating` branch: the
            // animation is done.
            state.reset = None;
        }
        // The frame that lands on home still has to be drawn.
        true
    }

    fn build(
        &self,
        state: &TransformationsDemoState,
        handle: StateHandle<TransformationsDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);

        // The scene: upstream's `CustomPaint(size: _board.size, painter:
        // _BoardPainter)`, grown to the viewport by its `SizedBox.expand` --
        // the viewer is `constrained`, so the child takes the viewport's
        // constraints and `BoardRender` sizes itself to them.
        let board = state.board.clone();
        let scene = move || {
            let board = board.clone();
            leaf(move || BoardRender::new(board.clone()))
        };

        // `_onScaleStart`: an interaction cancels a running reset animation.
        let on_interaction_start = {
            let handle = handle.clone();
            move || {
                handle.set_state(|state| {
                    state.reset = None;
                });
            }
        };

        let viewer = interactive_viewer(
            InteractiveViewer::new(ids::DEMO_LOCAL + 1, scene)
                // `boundaryMargin: EdgeInsets.symmetric(horizontal:
                // viewportSize.width, vertical: viewportSize.height)`.
                .with_boundary_margin(EdgeInsets::symmetric(VIEWPORT_WIDTH, VIEWPORT_HEIGHT))
                .with_min_scale(0.01)
                .with_transformation_controller(state.transformer.clone())
                .with_on_interaction_start(on_interaction_start),
        );

        // Upstream's `GestureDetector(behavior: opaque, onTapUp: _onTapUp)`
        // around the InteractiveViewer: a tap that did not become a pan
        // selects the board point under it, in scene coordinates through the
        // controller, and a tap off the board deselects.
        let on_tap = {
            let handle = handle.clone();
            let transformer = state.transformer.clone();
            move |event: rustflutter::gestures::TapEvent| {
                let scene_point = transformer.to_scene(event.local_position);
                handle.set_state(move |state| {
                    let board_point = state.board.point_to_board_point(scene_point);
                    state.board = state.board.copy_with_selected(board_point);
                });
            }
        };
        let viewport = single(viewer, move |rendered| {
            Box::new(
                Pointer::new(ids::DEMO_LOCAL, rendered)
                    .with_handlers(PointerHandlers::new().with_tap(on_tap.clone())),
            )
        });

        // `Container(color: backgroundColor, ...)` around the scene, sized to
        // the fixed window (module header).
        let window = single(viewport, move |rendered| {
            Box::new(
                Container::new()
                    .with_size(VIEWPORT_WIDTH, VIEWPORT_HEIGHT)
                    .with_color(BACKGROUND_COLOR)
                    .with_child(rendered),
            )
        });

        // `persistentFooterButtons: [resetButton, editButton]`.
        let reset_button = footer_button(
            ids::DEMO_LOCAL + 2,
            REPLAY_ICON,
            "Reset",
            true,
            state.pressed == Some(ids::DEMO_LOCAL + 2),
            &theme,
            {
                let handle = handle.clone();
                move |state: &mut TransformationsDemoState| {
                    // `_animateResetInitialize`.
                    state.reset = Some(ResetAnimation {
                        from: state.transformer.value(),
                        to: state.home_matrix,
                        started_micros: None,
                    });
                }
            },
            handle.clone(),
        );
        let edit_button = footer_button(
            ids::DEMO_LOCAL + 3,
            EDIT_ICON,
            "Edit",
            state.board.selected.is_some(),
            state.pressed == Some(ids::DEMO_LOCAL + 3),
            &theme,
            {
                let handle = handle.clone();
                move |state: &mut TransformationsDemoState| {
                    if state.board.selected.is_none() {
                        return;
                    }
                    state.editing = true;
                }
            },
            handle.clone(),
        );
        let footer = {
            many(vec![reset_button, edit_button], move |mut rendered| {
                let edit = rendered.pop().expect("the edit button");
                let reset = rendered.pop().expect("the reset button");
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::End)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(4.0);
                row = row.push(reset);
                row = row.push(edit);
                Box::new(row)
            })
        };

        // The edit panel, bottom-anchored over the board window the way
        // upstream's modal bottom sheet is bottom-anchored over the screen
        // (module header). Upstream's sheet is `Container(width:
        // double.infinity, height: 150, padding: 12, child: EditBoardPoint)`.
        let sheet_surface = theme.surface;
        let sheet_outline = theme.outline;
        let sheet = if state.editing {
            state.board.selected.map(|selected| {
                let on_color_selection = {
                    let handle = handle.clone();
                    move |color: Color| {
                        handle.set_state(move |state| {
                            // `onColorSelection`: repaint the point and pop.
                            state.board = state.board.copy_with_board_point_color(selected, color);
                            state.editing = false;
                        });
                    }
                };
                single(
                    component(EditBoardPoint::new(
                        ids::DEMO_LOCAL + 10,
                        selected,
                        on_color_selection,
                    )),
                    move |rendered| {
                        Box::new(
                            Container::new()
                                .with_height(150.0)
                                .with_color(sheet_surface)
                                .with_border(1.0, sheet_outline)
                                .with_padding(EdgeInsets::all(12.0))
                                .with_child(rendered),
                        )
                    },
                )
            })
        } else {
            None
        };

        // The sheet stacks over the window, bottom-anchored:
        // `Positioned(left: 0, right: 0, bottom: 0)`.
        let mut stack_children = vec![window];
        if let Some(sheet) = sheet {
            stack_children.push(sheet);
        }
        let window_with_sheet = many(stack_children, move |mut rendered| {
            let mut stack = RenderStack::new();
            let window = rendered.remove(0);
            stack = stack.push_boxed(window);
            if !rendered.is_empty() {
                let sheet = rendered.remove(0);
                stack = stack.push_positioned(
                    sheet,
                    StackPosition {
                        left: Some(0.0),
                        right: Some(0.0),
                        bottom: Some(0.0),
                        ..Default::default()
                    },
                );
            }
            Box::new(stack)
        });

        super::column(vec![window_with_sheet, footer], 8.0)
    }
}

/// A footer icon button: upstream's `IconButton` with a tooltip. The demo
/// page's chrome draws its icon buttons this same way (pages/demo.rs), and
/// the tooltip text stands as the label under the icon (module header).
fn footer_button(
    id: u64,
    glyph: &'static str,
    label: &'static str,
    enabled: bool,
    held: bool,
    theme: &Theme,
    action: impl Fn(&mut TransformationsDemoState) + 'static,
    handle: StateHandle<TransformationsDemoState>,
) -> AnyWidget {
    let ink_color = if enabled {
        theme.primary
    } else {
        theme.text_muted
    };
    let held_color = theme.primary.with_alpha(0x18);
    let action = Rc::new(action);
    let handlers = if enabled {
        PointerHandlers::new()
            .with_tap({
                let handle = handle.clone();
                let action = Rc::clone(&action);
                move |_| {
                    let action = Rc::clone(&action);
                    handle.set_state(move |state| (*action)(state));
                }
            })
            .with_press_change({
                let handle = handle.clone();
                move |down| {
                    handle.set_state(move |state| {
                        state.pressed = if down { Some(id) } else { None };
                    });
                }
            })
    } else {
        PointerHandlers::new()
    };
    let label_style = TextStyle {
        font_size: 10.0,
        color: theme.text_muted,
        ..TextStyle::default()
    };
    leaf(move || {
        Pointer::new(
            id,
            Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(2.0)
                .push(
                    Container::new()
                        .with_size(48.0, 48.0)
                        .with_corner_radius(24.0)
                        .with_color(if held { held_color } else { Color::TRANSPARENT })
                        .with_child(Align::new(
                            Alignment::CENTER,
                            Text::new(glyph)
                                .with_font_family(demos::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(ink_color),
                        )),
                )
                .push(Text::new(label).with_style(label_style.clone())),
        )
        .with_handlers(handlers.clone())
    })
}

/// Upstream's `_BoardPainter` and its `CustomPaint`: draws every board point
/// as a filled hexagon, the selected one at 70% opacity. The size is the
/// viewport's (upstream's `SizedBox.expand` around the `CustomPaint`), which
/// the viewer's `constrained` layout hands down.
struct BoardRender {
    board: Board,
    laid_out: Size,
}

impl BoardRender {
    fn new(board: Board) -> BoardRender {
        BoardRender {
            board,
            laid_out: Size::ZERO,
        }
    }

    /// Whether the two boards would draw differently. Upstream's
    /// `shouldRepaint`: `oldDelegate.board != board`.
    fn paints_differ(&self, other: &Board) -> bool {
        let board = &self.board;
        board.selected != other.selected
            || board.points().len() != other.points().len()
            || board
                .points()
                .iter()
                .zip(other.points())
                .any(|(a, b)| a != b || a.color != b.color)
    }
}

impl RenderBox for BoardRender {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<BoardRender>()?;
        let effect = UpdateEffect::repaint_if(self.paints_differ(&fresh.board));
        self.board = fresh.board.clone();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // `SizedBox.expand`: as big as the constraints allow.
        self.laid_out = constraints.biggest();
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let canvas = context.canvas();
        for board_point in self.board.points() {
            // `drawBoardPoint`: the selected point at 0.7 opacity.
            let color = if self.board.selected == Some(*board_point) {
                board_point.color.with_alpha(0xB2)
            } else {
                board_point.color
            };
            let vertices = self.board.vertices_for_board_point(*board_point);
            let mut path = RenderPath::new();
            let mut corners = Board::HEXAGON_CORNERS.iter();
            let first = vertices[*corners.next().expect("six corners")];
            path.move_to(offset.dx + first.dx, offset.dy + first.dy);
            for corner in corners {
                let vertex = vertices[*corner];
                path.line_to(offset.dx + vertex.dx, offset.dy + vertex.dy);
            }
            path.close();
            canvas.draw_path(&path, &Paint::new(color));
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_matrix_centers_the_board_in_the_viewport() {
        // Upstream's first `LayoutBuilder` pass: translate(viewport / 2 -
        // board.size / 2).
        let state = TransformationsDemoState::new();
        let board_size = state.board.size();
        let expected = Affine2D::IDENTITY.translate_by_double(
            VIEWPORT_WIDTH / 2.0 - board_size.width / 2.0,
            VIEWPORT_HEIGHT / 2.0 - board_size.height / 2.0,
        );
        assert_eq!(state.home_matrix, expected);
        assert_eq!(state.transformer.value(), expected);
        // The board's center lands on the viewport's center.
        let center =
            expected.transform_point(Offset::new(board_size.width / 2.0, board_size.height / 2.0));
        assert!((center.dx - VIEWPORT_WIDTH / 2.0).abs() < 1e-4);
        assert!((center.dy - VIEWPORT_HEIGHT / 2.0).abs() < 1e-4);
    }

    #[test]
    fn a_tap_maps_through_the_transform_to_a_board_point() {
        // The viewport's center under the home transform is the board's
        // center, which is the origin point (board's test
        // `the_boards_center_is_the_origin_point`).
        let state = TransformationsDemoState::new();
        let scene = state
            .transformer
            .to_scene(Offset::new(VIEWPORT_WIDTH / 2.0, VIEWPORT_HEIGHT / 2.0));
        assert_eq!(
            state.board.point_to_board_point(scene),
            Some(BoardPoint::new(0, 0))
        );
    }
}
