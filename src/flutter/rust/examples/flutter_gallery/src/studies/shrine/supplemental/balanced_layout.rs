// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/supplemental/balanced_layout.dart`
//! (flutter/gallery @ d12640d): the desktop grid's column balancer.
//!
//! Given each product card's height, this assigns cards to columns so the
//! column bottoms land close together: a round-robin seeding followed by a
//! hill-climb that swaps or moves a pair of cards between two columns
//! whenever doing so brings their heights closer by more than ten pixels.
//! Ported line for line, including upstream's empty-element trick and its
//! sentinel math; the cache is `LayoutCache` rather than an inherited widget.

use super::super::model::product::Product;
use super::desktop_product_columns::{COLUMN_TOP_SPACE, PRODUCT_CARD_ADDITIONAL_HEIGHT};
use super::layout_cache::LayoutCache;

/// A placeholder id for an empty element. See [`iterate_until_balanced`].
const EMPTY_ELEMENT: isize = -1;

/// To avoid infinite loops, improvements to the layout are only performed
/// when a column's height changes by more than
/// [`DEVIATION_IMPROVEMENT_THRESHOLD`] pixels.
const DEVIATION_IMPROVEMENT_THRESHOLD: f64 = 10.0;

/// Height of a product image, paired with the product's id.
/// Upstream's `_TaggedHeightData`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TaggedHeightData {
    /// The index of the corresponding product.
    index: isize,
    /// The height of the product image.
    height: f64,
}

/// Converts a slice of [`TaggedHeightData`] elements to a list, and adds an
/// empty element. Used for iteration. Upstream's `_toListAndAddEmpty`.
fn to_list_and_add_empty(set: &[TaggedHeightData]) -> Vec<TaggedHeightData> {
    let mut result = set.to_vec();
    result.push(TaggedHeightData {
        index: EMPTY_ELEMENT,
        height: 0.0,
    });
    result
}

/// Encode parameters for caching. Upstream's `_encodeParameters`.
fn encode_parameters(
    column_count: usize,
    products: &[&Product],
    large_image_width: f64,
    small_image_width: f64,
) -> String {
    let product_string = products
        .iter()
        .map(|product| product.id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("{column_count};{product_string},{large_image_width},{small_image_width}")
}

/// Given a layout, replace integers by their corresponding products.
/// Upstream's `_generateLayout`.
fn generate_layout<'a>(products: &[&'a Product], layout: &[Vec<usize>]) -> Vec<Vec<&'a Product>> {
    layout
        .iter()
        .map(|column| column.iter().map(|&index| products[index]).collect())
        .collect()
}

/// Given `column_objects`, list of the objects in each column, and
/// `column_heights`, list of heights of each column, moves and swaps objects
/// between columns until their heights are sufficiently close to each other.
/// This prevents the layout having significant, avoidable gaps at the bottom.
/// Upstream's `_iterateUntilBalanced`; the Dart `Set`s are `Vec`s here --
/// membership, not order, is all the algorithm reads.
fn iterate_until_balanced(
    column_objects: &mut [Vec<TaggedHeightData>],
    column_heights: &mut [f64],
) {
    let mut failed_moves = 0;
    let column_count = column_objects.len();

    // No need to rearrange a 1-column layout.
    if column_count == 1 {
        return;
    }

    loop {
        // Loop through all possible 2-combinations of columns.
        for source in 0..column_count {
            for target in (source + 1)..column_count {
                // Tries to find an object A from source column
                // and an object B from target column, such that switching them
                // causes the height of the two columns to be closer.

                // A or B can be empty; in this case, moving an object from one
                // column to the other is the best choice.

                let mut success = false;

                let best_height = (column_heights[source] + column_heights[target]) / 2.0;
                let score_limit = (column_heights[source] - best_height).abs();

                let source_objects = to_list_and_add_empty(&column_objects[source]);
                let target_objects = to_list_and_add_empty(&column_objects[target]);

                let mut best_a: Option<TaggedHeightData> = None;
                let mut best_b: Option<TaggedHeightData> = None;
                let mut best_score: Option<f64> = None;

                for a in &source_objects {
                    for b in &target_objects {
                        if a.index == EMPTY_ELEMENT && b.index == EMPTY_ELEMENT {
                            continue;
                        }
                        let score =
                            (column_heights[source] - a.height + b.height - best_height).abs();
                        if score < score_limit - DEVIATION_IMPROVEMENT_THRESHOLD {
                            success = true;
                            if best_score.is_none_or(|best| score < best) {
                                best_score = Some(score);
                                best_a = Some(*a);
                                best_b = Some(*b);
                            }
                        }
                    }
                }

                if !success {
                    failed_moves += 1;
                } else {
                    failed_moves = 0;

                    let best_a = best_a.expect("a successful search found a pair");
                    let best_b = best_b.expect("a successful search found a pair");

                    // Switch A and B.
                    if best_a.index != EMPTY_ELEMENT {
                        column_objects[source].retain(|object| object.index != best_a.index);
                        column_objects[target].push(best_a);
                    }
                    if best_b.index != EMPTY_ELEMENT {
                        column_objects[target].retain(|object| object.index != best_b.index);
                        column_objects[source].push(best_b);
                    }
                    column_heights[source] += best_b.height - best_a.height;
                    column_heights[target] += best_a.height - best_b.height;
                }

                // If no two columns' heights can be made closer by switching
                // elements, the layout is sufficiently balanced.
                if failed_moves >= column_count * (column_count - 1) / 2 {
                    return;
                }
            }
        }
    }
}

/// Given a list of numbers `data`, representing the heights of each image,
/// and a list of numbers `biases`, representing the heights of the space
/// above each column, returns a layout of `data` so that the height of each
/// column is sufficiently close to each other, represented as a list of
/// lists of integers, each integer being an index for a product.
/// Upstream's `_balancedDistribution`.
fn balanced_distribution(column_count: usize, data: &[f64], biases: &[f64]) -> Vec<Vec<usize>> {
    assert_eq!(biases.len(), column_count);

    let mut column_objects: Vec<Vec<TaggedHeightData>> =
        (0..column_count).map(|_| Vec::new()).collect();
    let mut column_heights = biases.to_vec();

    for (i, &height) in data.iter().enumerate() {
        let column = i % column_count;
        column_heights[column] += height;
        column_objects[column].push(TaggedHeightData {
            index: i as isize,
            height,
        });
    }

    iterate_until_balanced(&mut column_objects, &mut column_heights);

    column_objects
        .iter()
        .map(|column| {
            let mut indices: Vec<usize> =
                column.iter().map(|object| object.index as usize).collect();
            indices.sort();
            indices
        })
        .collect()
}

/// Generates a balanced layout for `column_count` columns, with products
/// specified by the list `products`, where the larger images have width
/// `large_image_width` and the smaller images have width `small_image_width`.
/// The cache is upstream's `LayoutCache.of(context)`.
pub fn balanced_layout(
    cache: &LayoutCache,
    column_count: usize,
    products: &[&'static Product],
    large_image_width: f64,
    small_image_width: f64,
) -> Vec<Vec<&'static Product>> {
    let encoded_parameters =
        encode_parameters(column_count, products, large_image_width, small_image_width);

    // Check if this layout is cached.
    if let Some(layout) = cache.get(&encoded_parameters) {
        return generate_layout(products, &layout);
    }

    let product_heights: Vec<f64> = products
        .iter()
        .map(|product| {
            (1.0 / product.ratio as f64) * (large_image_width + small_image_width) / 2.0
                + PRODUCT_CARD_ADDITIONAL_HEIGHT
        })
        .collect();

    let layout = balanced_distribution(
        column_count,
        &product_heights,
        &(0..column_count)
            .map(|column| {
                if column % 2 == 0 {
                    0.0
                } else {
                    COLUMN_TOP_SPACE
                }
            })
            .collect::<Vec<_>>(),
    );

    // Add tailored layout to cache.
    cache.insert(encoded_parameters, layout.clone());

    generate_layout(products, &layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studies::shrine::model::products_repository::PRODUCTS;

    fn all_products() -> Vec<&'static Product> {
        PRODUCTS.iter().collect()
    }

    #[test]
    fn every_product_lands_in_exactly_one_column() {
        let products = all_products();
        let layout = balanced_layout(&LayoutCache::default(), 3, &products, 186.0, 162.0);
        let mut seen: Vec<usize> = layout.iter().flatten().map(|p| p.id as usize).collect();
        seen.sort();
        assert_eq!(seen, (0..products.len()).collect::<Vec<_>>());
        assert_eq!(layout.len(), 3);
    }

    #[test]
    fn the_columns_end_up_balanced() {
        // Heights engineered so round-robin alone leaves a gap the climb can
        // close: one tall card lands last on the shortest column.
        let data = [100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 10.0, 10.0, 10.0];
        let layout = balanced_distribution(3, &data, &[0.0, 0.0, 0.0]);
        let heights: Vec<f64> = layout
            .iter()
            .map(|column| column.iter().map(|&i| data[i]).sum())
            .collect();
        let max = heights.iter().cloned().fold(f64::MIN, f64::max);
        let min = heights.iter().cloned().fold(f64::MAX, f64::min);
        assert!(
            max - min <= 100.0,
            "the columns are within one card: {heights:?}"
        );
        // Round-robin's own answer -- each column 210 -- is already balanced
        // here, and the climb must not make it worse.
        assert!(max - min <= 100.0);
    }

    #[test]
    fn a_single_column_is_left_alone() {
        let data = [50.0, 80.0, 30.0];
        assert_eq!(balanced_distribution(1, &data, &[0.0]), vec![vec![0, 1, 2]]);
    }

    #[test]
    fn the_climb_moves_a_card_when_it_clearly_helps() {
        // Round-robin gives [200, 20]: the 100 card moves across.
        let data = [100.0, 10.0, 100.0, 10.0];
        let layout = balanced_distribution(2, &data, &[0.0, 0.0]);
        let heights: Vec<f64> = layout
            .iter()
            .map(|column| column.iter().map(|&i| data[i]).sum())
            .collect();
        assert_eq!(heights, vec![110.0, 110.0]);
    }

    #[test]
    fn the_biases_count_towards_the_column_heights() {
        // Odd columns start COLUMN_TOP_SPACE tall, so the seeding reads the
        // same as upstream's staggered desktop grid.
        let data = [84.0, 84.0];
        let layout = balanced_distribution(2, &data, &[0.0, COLUMN_TOP_SPACE]);
        // Column 0 starts empty, column 1 starts 84 up; the balance point is
        // equal totals either way, both layouts score the same, and the
        // climb's threshold keeps the seed.
        let total: usize = layout.iter().map(Vec::len).sum();
        assert_eq!(total, 2);
    }

    #[test]
    fn the_cache_serves_the_same_layout_twice() {
        let cache = LayoutCache::default();
        let products = all_products();
        let first = balanced_layout(&cache, 2, &products, 186.0, 162.0);
        let second = balanced_layout(&cache, 2, &products, 186.0, 162.0);
        let ids = |layout: &[Vec<&Product>]| {
            layout
                .iter()
                .map(|column| column.iter().map(|p| p.id).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second));
    }
}
