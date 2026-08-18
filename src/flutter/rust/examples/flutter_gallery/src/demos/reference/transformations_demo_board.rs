// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Maps to `lib/demos/reference/transformations_demo_board.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! The entire state of the hex board and the abstraction to get information
//! about it: [`Board`] is upstream's `Board` (its `IterableMixin` is a slice
//! here), [`BoardPoint`] is upstream's `BoardPoint`, and the geometry --
//! `size`, `pointToBoardPoint`, `boardPointToPoint`,
//! `getVerticesForBoardPoint`, the axial/cube coordinate math -- is ported
//! formula for formula.
//!
//! Divergences, each marked at its site as well:
//!
//! * **Vertices -> corner list.** Upstream's `getVerticesForBoardPoint`
//!   returns a `dart:ui` `Vertices` in `triangleFan` mode, nine positions with
//!   the fan's repeated vertices. The canvas here draws paths, not vertex
//!   fans, so [`Board::vertices_for_board_point`] returns the same nine
//!   translated positions and the painter walks the six distinct corners
//!   ([`Board::HEXAGON_CORNERS`]); the fill that comes out is the same
//!   hexagon.
//! * **BoardPoint equality by location only** is upstream's `==`; color
//!   comparisons upstream does explicitly (`boardPoint.color == color`) stay
//!   explicit here.

use rustflutter::engine::Color;
use rustflutter::render::{Offset, Size};

/// The color a fresh board point gets. Upstream's `BoardPoint` default,
/// `Color(0xFFCDCDCD)`.
pub const DEFAULT_POINT_COLOR: Color = Color(0xFFCDCDCD);

/// A range of q/r board coordinate values. Upstream's `_Range`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Range {
    min: i32,
    max: i32,
}

/// A location on the board in axial coordinates.
///
/// Axial coordinates use two integers, q and r, to locate a hexagon on a
/// grid: <https://www.redblobgames.com/grids/hexagons/#coordinates-axial>.
/// Upstream's `BoardPoint`.
#[derive(Clone, Copy, Debug)]
pub struct BoardPoint {
    pub q: i32,
    pub r: i32,
    pub color: Color,
}

impl BoardPoint {
    pub fn new(q: i32, r: i32) -> BoardPoint {
        BoardPoint {
            q,
            r,
            color: DEFAULT_POINT_COLOR,
        }
    }

    /// `BoardPoint.copyWithColor`.
    pub fn copy_with_color(&self, next_color: Color) -> BoardPoint {
        BoardPoint {
            q: self.q,
            r: self.r,
            color: next_color,
        }
    }

    /// Convert from q,r axial coords to x,y,z cube coords. Upstream's
    /// `cubeCoordinates`.
    fn cube_coordinates(&self) -> (i32, i32, i32) {
        (self.q, self.r, -self.q - self.r)
    }
}

/// Upstream's `operator ==`: "only compares by location".
impl PartialEq for BoardPoint {
    fn eq(&self, other: &BoardPoint) -> bool {
        self.q == other.q && self.r == other.r
    }
}

impl Eq for BoardPoint {}

/// The entire state of the hex board and abstraction to get information about
/// it. Upstream's `Board`.
#[derive(Clone, Debug)]
pub struct Board {
    /// Number of hexagons from center to edge.
    pub board_radius: i32,
    /// Pixel radius of a hexagon (center to vertex).
    pub hexagon_radius: f32,
    /// Margin between hexagons.
    pub hexagon_margin: f32,
    /// `positionsForHexagonAtOrigin`: the triangle-fan positions for a hexagon
    /// centered on the origin, computed from the padded radius.
    positions_for_hexagon_at_origin: [Offset; 9],
    /// `selected`.
    pub selected: Option<BoardPoint>,
    points: Vec<BoardPoint>,
}

impl Board {
    /// The indices into the nine fan positions that walk the hexagon's six
    /// distinct corners in order; the fan repeats three of them (2 repeats 1's
    /// edge endpoint -- positions 2/3, 4/5 and 6/7 are the same points). See
    /// the module header.
    pub const HEXAGON_CORNERS: [usize; 6] = [0, 1, 2, 4, 6, 8];

    /// Upstream's constructor: given no points it generates a fresh board,
    /// spiralling out from `BoardPoint(-boardRadius, 0)` through
    /// `_getNextBoardPoint`.
    pub fn new(
        board_radius: i32,
        hexagon_radius: f32,
        hexagon_margin: f32,
        selected: Option<BoardPoint>,
        board_points: Option<Vec<BoardPoint>>,
    ) -> Board {
        assert!(board_radius > 0);
        assert!(hexagon_radius > 0.0);
        assert!(hexagon_margin >= 0.0);

        // Set up the positions for the center hexagon where the entire board
        // is centered on the origin.
        // Start point of hexagon (top vertex).
        let hex_start = Offset::new(0.0, -hexagon_radius);
        let hexagon_radius_padded = hexagon_radius - hexagon_margin;
        let center_to_flat = 3.0f32.sqrt() / 2.0 * hexagon_radius_padded;
        let positions_for_hexagon_at_origin = [
            Offset::new(hex_start.dx, hex_start.dy),
            Offset::new(
                hex_start.dx + center_to_flat,
                hex_start.dy + 0.5 * hexagon_radius_padded,
            ),
            Offset::new(
                hex_start.dx + center_to_flat,
                hex_start.dy + 1.5 * hexagon_radius_padded,
            ),
            Offset::new(
                hex_start.dx + center_to_flat,
                hex_start.dy + 1.5 * hexagon_radius_padded,
            ),
            Offset::new(hex_start.dx, hex_start.dy + 2.0 * hexagon_radius_padded),
            Offset::new(hex_start.dx, hex_start.dy + 2.0 * hexagon_radius_padded),
            Offset::new(
                hex_start.dx - center_to_flat,
                hex_start.dy + 1.5 * hexagon_radius_padded,
            ),
            Offset::new(
                hex_start.dx - center_to_flat,
                hex_start.dy + 1.5 * hexagon_radius_padded,
            ),
            Offset::new(
                hex_start.dx - center_to_flat,
                hex_start.dy + 0.5 * hexagon_radius_padded,
            ),
        ];

        let points = match board_points {
            Some(points) => points,
            None => {
                // Generate boardPoints for a fresh board.
                let mut points = Vec::new();
                let mut board_point = Self::next_board_point(board_radius, None);
                while let Some(point) = board_point {
                    points.push(point);
                    board_point = Self::next_board_point(board_radius, Some(point));
                }
                points
            }
        };

        Board {
            board_radius,
            hexagon_radius,
            hexagon_margin,
            positions_for_hexagon_at_origin,
            selected,
            points,
        }
    }

    /// The board points, in generation order. Upstream's `iterator`.
    pub fn points(&self) -> &[BoardPoint] {
        &self.points
    }

    /// For a given q axial coordinate, get the range of possible r values.
    /// Upstream's `_getRRangeForQ`.
    fn r_range_for_q(&self, q: i32) -> Range {
        let (r_start, r_end) = if q <= 0 {
            (-self.board_radius - q, self.board_radius)
        } else {
            (-self.board_radius, self.board_radius - q)
        };
        Range {
            min: r_start,
            max: r_end,
        }
    }

    /// Get the BoardPoint that comes after the given BoardPoint. If given
    /// `None`, returns the origin BoardPoint. If the given BoardPoint is the
    /// last, returns `None`. Upstream's `_getNextBoardPoint`, static here
    /// because the constructor runs it before there is a board.
    fn next_board_point(board_radius: i32, board_point: Option<BoardPoint>) -> Option<BoardPoint> {
        // If before the first element.
        let Some(board_point) = board_point else {
            return Some(BoardPoint::new(-board_radius, 0));
        };

        let r_range_for = |q: i32| {
            if q <= 0 {
                Range {
                    min: -board_radius - q,
                    max: board_radius,
                }
            } else {
                Range {
                    min: -board_radius,
                    max: board_radius - q,
                }
            }
        };
        let r_range = r_range_for(board_point.q);

        // If at or after the last element.
        if board_point.q >= board_radius && board_point.r >= r_range.max {
            return None;
        }

        // If wrapping from one q to the next.
        if board_point.r >= r_range.max {
            return Some(BoardPoint::new(
                board_point.q + 1,
                r_range_for(board_point.q + 1).min,
            ));
        }

        // Otherwise we're just incrementing r.
        Some(BoardPoint::new(board_point.q, board_point.r + 1))
    }

    /// Check if the board point is actually on the board. Upstream's
    /// `_validateBoardPoint`.
    fn validate_board_point(&self, board_point: BoardPoint) -> bool {
        let center = BoardPoint::new(0, 0);
        let distance_from_center = Board::get_distance(center, board_point);
        distance_from_center <= self.board_radius
    }

    /// Get the size in pixels of the entire board. Upstream's `size` getter.
    pub fn size(&self) -> Size {
        let center_to_flat = 3.0f32.sqrt() / 2.0 * self.hexagon_radius;
        Size::new(
            (self.board_radius * 2 + 1) as f32 * center_to_flat * 2.0,
            2.0 * (self.hexagon_radius + self.board_radius as f32 * 1.5 * self.hexagon_radius),
        )
    }

    /// Get the distance between two BoardPoints. Upstream's `getDistance`.
    pub fn get_distance(a: BoardPoint, b: BoardPoint) -> i32 {
        let a3 = a.cube_coordinates();
        let b3 = b.cube_coordinates();
        ((a3.0 - b3.0).abs() + (a3.1 - b3.1).abs() + (a3.2 - b3.2).abs()) / 2
    }

    /// Return the q,r BoardPoint for a point in the scene, where the origin is
    /// in the center of the board in both coordinate systems. If there is no
    /// BoardPoint at the location, returns `None`. Upstream's
    /// `pointToBoardPoint`.
    pub fn point_to_board_point(&self, point: Offset) -> Option<BoardPoint> {
        let size = self.size();
        let point_centered = Offset::new(point.dx - size.width / 2.0, point.dy - size.height / 2.0);
        let board_point = BoardPoint::new(
            ((3.0f32.sqrt() / 3.0 * point_centered.dx - 1.0 / 3.0 * point_centered.dy)
                / self.hexagon_radius)
                .round() as i32,
            ((2.0 / 3.0 * point_centered.dy) / self.hexagon_radius).round() as i32,
        );

        if !self.validate_board_point(board_point) {
            return None;
        }

        self.points.iter().copied().find(|board_point_i| {
            board_point_i.q == board_point.q && board_point_i.r == board_point.r
        })
    }

    /// Return a scene point for the center of a hexagon given its q,r point.
    /// Upstream's `boardPointToPoint`.
    pub fn board_point_to_point(&self, board_point: BoardPoint) -> Offset {
        let size = self.size();
        Offset::new(
            3.0f32.sqrt() * self.hexagon_radius * board_point.q as f32
                + 3.0f32.sqrt() / 2.0 * self.hexagon_radius * board_point.r as f32
                + size.width / 2.0,
            1.5 * self.hexagon_radius * board_point.r as f32 + size.height / 2.0,
        )
    }

    /// The nine triangle-fan positions for the given BoardPoint, translated to
    /// its center. Upstream's `getVerticesForBoardPoint`, minus the color:
    /// `Vertices` couples the positions with a per-vertex color, and the
    /// painter here draws one filled path per point instead, taking the color
    /// itself and walking [`Board::HEXAGON_CORNERS`].
    pub fn vertices_for_board_point(&self, board_point: BoardPoint) -> [Offset; 9] {
        let center_of_hex_zero_center = self.board_point_to_point(board_point);
        self.positions_for_hexagon_at_origin
            .map(|offset| offset.plus(center_of_hex_zero_center))
    }

    /// Return a new board with the given BoardPoint selected. Upstream's
    /// `copyWithSelected`.
    pub fn copy_with_selected(&self, board_point: Option<BoardPoint>) -> Board {
        if self.selected == board_point {
            return self.clone();
        }
        Board::new(
            self.board_radius,
            self.hexagon_radius,
            self.hexagon_margin,
            board_point,
            Some(self.points.clone()),
        )
    }

    /// Return a new board where boardPoint has the given color. Upstream's
    /// `copyWithBoardPointColor`.
    pub fn copy_with_board_point_color(&self, board_point: BoardPoint, color: Color) -> Board {
        let next_board_point = board_point.copy_with_color(color);
        let board_point_index = self
            .points
            .iter()
            .position(|board_point_i| {
                board_point_i.q == board_point.q && board_point_i.r == board_point.r
            })
            .expect("a board point that is on the board");

        if self.points[board_point_index] == board_point && board_point.color == color {
            return self.clone();
        }

        let mut next_board_points = self.points.clone();
        next_board_points[board_point_index] = next_board_point;
        let selected_board_point = if self.selected == Some(board_point) {
            Some(next_board_point)
        } else {
            self.selected
        };
        Board::new(
            self.board_radius,
            self.hexagon_radius,
            self.hexagon_margin,
            selected_board_point,
            Some(next_board_points),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        // The demo's own constants: radius 8 of 16px hexagons.
        Board::new(8, 16.0, 1.0, None, None)
    }

    #[test]
    fn a_fresh_board_has_a_hexagons_worth_of_points() {
        // A hex board of radius r holds 1 + 3r(r+1) hexagons: 217 at r = 8.
        let board = board();
        assert_eq!(board.points().len(), 217);
        // The walk starts at (-r, 0) and ends at the far corner, as
        // `_getNextBoardPoint` orders it.
        assert_eq!(
            (
                board.points().first().unwrap().q,
                board.points().first().unwrap().r
            ),
            (-8, 0)
        );
        assert_eq!(
            (
                board.points().last().unwrap().q,
                board.points().last().unwrap().r
            ),
            (8, 0)
        );
        // Every point validates, and the center is among them.
        assert!(board
            .points()
            .iter()
            .all(|point| board.validate_board_point(*point)));
        assert!(board.points().contains(&BoardPoint::new(0, 0)));
    }

    #[test]
    fn the_boards_size_is_the_upstreams_formula() {
        let board = board();
        let size = board.size();
        let center_to_flat = 3.0f32.sqrt() / 2.0 * 16.0;
        assert_eq!(size.width, 17.0 * center_to_flat * 2.0);
        assert_eq!(size.height, 2.0 * (16.0 + 8.0 * 1.5 * 16.0));
    }

    #[test]
    fn the_boards_center_is_the_origin_point() {
        let board = board();
        let size = board.size();
        let center = Offset::new(size.width / 2.0, size.height / 2.0);
        assert_eq!(
            board.point_to_board_point(center),
            Some(BoardPoint::new(0, 0))
        );
        // And the origin point's center is the board's center.
        let point = board.board_point_to_point(BoardPoint::new(0, 0));
        assert!((point.dx - center.dx).abs() < 1e-4 && (point.dy - center.dy).abs() < 1e-4);
    }

    #[test]
    fn a_scene_point_round_trips_through_a_board_point() {
        let board = board();
        for (q, r) in [(0, 0), (3, -2), (-8, 0), (8, 0), (-4, 8), (5, -8)] {
            let point = BoardPoint::new(q, r);
            let scene = board.board_point_to_point(point);
            assert_eq!(board.point_to_board_point(scene), Some(point), "({q}, {r})");
        }
        // Off the board entirely: nothing to select.
        assert_eq!(board.point_to_board_point(Offset::new(-50.0, -50.0)), None);
    }

    #[test]
    fn distance_is_cube_coordinate_distance() {
        assert_eq!(
            Board::get_distance(BoardPoint::new(0, 0), BoardPoint::new(0, 0)),
            0
        );
        assert_eq!(
            Board::get_distance(BoardPoint::new(0, 0), BoardPoint::new(3, -1)),
            3
        );
        assert_eq!(
            Board::get_distance(BoardPoint::new(-8, 0), BoardPoint::new(8, 0)),
            16
        );
    }

    #[test]
    fn the_fan_positions_match_upstreams_formula() {
        // Upstream's `positionsForHexagonAtOrigin`: the fan's shared vertex is
        // the top vertex at the *unpadded* radius, every other position is
        // computed from the padded one, and positions 2/3, 4/5 and 6/7 are the
        // same points repeated.
        let board = board();
        let vertices = board.vertices_for_board_point(BoardPoint::new(0, 0));
        let center = board.board_point_to_point(BoardPoint::new(0, 0));
        let padded = 16.0f32 - 1.0;
        let center_to_flat = 3.0f32.sqrt() / 2.0 * padded;
        let at_origin = [
            Offset::new(0.0, -16.0),
            Offset::new(center_to_flat, -16.0 + 0.5 * padded),
            Offset::new(center_to_flat, -16.0 + 1.5 * padded),
            Offset::new(center_to_flat, -16.0 + 1.5 * padded),
            Offset::new(0.0, -16.0 + 2.0 * padded),
            Offset::new(0.0, -16.0 + 2.0 * padded),
            Offset::new(-center_to_flat, -16.0 + 1.5 * padded),
            Offset::new(-center_to_flat, -16.0 + 1.5 * padded),
            Offset::new(-center_to_flat, -16.0 + 0.5 * padded),
        ];
        for (index, (actual, expected)) in vertices.iter().zip(at_origin.iter()).enumerate() {
            let actual = actual.minus(center);
            assert!(
                (actual.dx - expected.dx).abs() < 1e-4 && (actual.dy - expected.dy).abs() < 1e-4,
                "position {index}: ({}, {}) != ({}, {})",
                actual.dx,
                actual.dy,
                expected.dx,
                expected.dy,
            );
        }
        // The painter's corner walk is the six distinct corners of the fan.
        let mut corners: Vec<Offset> = Board::HEXAGON_CORNERS
            .iter()
            .map(|corner| vertices[*corner])
            .collect();
        corners.dedup();
        assert_eq!(corners.len(), 6);
    }

    #[test]
    fn copy_with_selected_keeps_the_points() {
        let board = board();
        let selected = board.copy_with_selected(Some(BoardPoint::new(1, 1)));
        assert_eq!(selected.selected, Some(BoardPoint::new(1, 1)));
        assert_eq!(selected.points().len(), board.points().len());
        // Selecting what is already selected is the same board, upstream's
        // early return.
        assert!(selected
            .copy_with_selected(Some(BoardPoint::new(1, 1)))
            .selected
            .is_some());
    }

    #[test]
    fn copy_with_color_paints_the_point_and_follows_the_selection() {
        let board = board().copy_with_selected(Some(BoardPoint::new(2, 2)));
        let red = Color(0xFFFF0000);
        let painted = board.copy_with_board_point_color(BoardPoint::new(2, 2), red);
        let point = painted
            .points()
            .iter()
            .find(|point| point.q == 2 && point.r == 2)
            .unwrap();
        assert_eq!(point.color, red);
        // The selected point was the painted one, so the selection moved to
        // the recolored copy.
        assert_eq!(painted.selected.map(|point| point.color), Some(red));
        // A no-op recolor is the same board upstream; here, the same values.
        let again = painted.copy_with_board_point_color(BoardPoint::new(2, 2), red);
        assert_eq!(again.points().len(), painted.points().len());
    }
}
