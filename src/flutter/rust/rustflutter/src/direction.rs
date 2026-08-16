// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Which way text runs, and everything that follows from that.
//!
//! Upstream this is two things: `TextDirection` in `dart:ui`, and the
//! `Directionality` widget in `widgets/basic.dart` that publishes one to a
//! subtree. The two exist separately because a direction is both a shaping
//! input -- the paragraph's base direction that bidi resolution, line
//! breaking and `TextAlign.start`/`end` are measured against -- and a layout
//! input, since a row, a padding or a leading edge means one side in one
//! direction and the other side in the other.
//!
//! The widget half is a `provide` rather than a component of its own for the
//! same reason [`crate::media_query::MediaQuery`] is: publishing a value is
//! all it does, and the generic provider already knows how.
//!
//! # The direction in force while a subtree's render objects are being built
//!
//! Text is shaped at layout time, long after the walk that built the render
//! tree has finished, so a paragraph cannot go looking for its directionality
//! when it needs it: it has to have been told. Upstream answers this the same
//! way it answers the text scale -- `Text.build` reads
//! `Directionality.of(context)` and hands the result to `RenderParagraph` as
//! a field. The equivalent here is the render walk pushing the direction as
//! it descends through a `directionality`, exactly as it pushes the scale
//! through a `MediaQuery`; see [`crate::media_query`] for the precedent.

use std::cell::Cell;

use crate::framework::{AnyWidget, BuildContext, provide};

/// Whether text runs left-to-right or right-to-left.
///
/// Upstream's `dart:ui` spells the variants `ltr` and `rtl` and puts `rtl`
/// first; they are ordered the other way round here so that the default --
/// what a tree with no `directionality` in it gets, and what every
/// left-to-right language needs -- is the zero value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Text flows from the left edge to the right (English, French, Chinese
    /// set left-to-right, ...). The default, upstream's app-level assumption.
    #[default]
    Ltr,
    /// Text flows from the right edge to the left (Arabic, Hebrew, ...).
    Rtl,
}

thread_local! {
    static CURRENT_DIRECTION: Cell<Option<TextDirection>> = const { Cell::new(None) };
}

/// The direction a paragraph built right now should be shaped in.
///
/// Outside any [`directionality`] -- a render object built on its own in a
/// test, say -- this is [`TextDirection::Ltr`], which is what upstream's
/// `debugCheckHasDirectionality` is there to guarantee every real tree says
/// some other way: there the root `WidgetsApp` wraps everything in one, and a
/// tree without it is a mistake in the app.
pub fn current_direction() -> TextDirection {
    CURRENT_DIRECTION.with(|direction| direction.get()).unwrap_or(TextDirection::Ltr)
}

/// Runs `body` with `direction` as the ambient direction, restoring whatever
/// was in force before. Called by the render walk; not public API.
pub(crate) fn with_direction<R>(direction: TextDirection, body: impl FnOnce() -> R) -> R {
    let previous = CURRENT_DIRECTION.with(|current| current.replace(Some(direction)));
    // The restore has to happen even if `body` unwinds, or one panicking
    // subtree would leave every later frame shaping at its direction.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    CURRENT_DIRECTION.with(|current| current.set(previous));
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Publishes `direction` to `child` and everything below it.
///
/// Upstream's `Directionality` widget. A subtree in [`TextDirection::Rtl`]
/// shapes its paragraphs right-to-left, resolves `TextAlign.start` and `end`
/// against the right and left edges respectively, and -- once the render-tree
/// half lands -- lays out rows and paddings from the right.
pub fn directionality(direction: TextDirection, child: AnyWidget) -> AnyWidget {
    provide(direction, child)
}

/// What the nearest enclosing [`directionality`] says.
///
/// Falls back to [`TextDirection::Ltr`] rather than panicking when there is
/// none, the same choice [`crate::media_query::media_query_of`] makes: the
/// root always has a direction in a real application, so the fallback is only
/// reachable from a test that mounted a widget on its own.
pub fn direction_of(context: &BuildContext) -> TextDirection {
    *context.inherited_or_default::<TextDirection>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, component, leaf, many};
    use crate::widgets::SizedBox;

    // -- The thread-local -----------------------------------------------------

    #[test]
    fn without_a_directionality_text_runs_left_to_right() {
        assert_eq!(current_direction(), TextDirection::Ltr);
    }

    #[test]
    fn with_direction_changes_what_current_direction_reports() {
        with_direction(TextDirection::Rtl, || {
            assert_eq!(current_direction(), TextDirection::Rtl);
        });
        // And it is not still in force afterwards.
        assert_eq!(current_direction(), TextDirection::Ltr);
    }

    #[test]
    fn nested_directions_restore_in_order() {
        with_direction(TextDirection::Rtl, || {
            with_direction(TextDirection::Ltr, || {
                assert_eq!(current_direction(), TextDirection::Ltr);
            });
            assert_eq!(current_direction(), TextDirection::Rtl, "the inner direction leaked out");
        });
        assert_eq!(current_direction(), TextDirection::Ltr);
    }

    // -- The widget -----------------------------------------------------------

    /// A leaf that records the direction in force where it was built.
    ///
    /// Which is the whole question: a paragraph made inside this closure
    /// takes the same value, and shapes in it later.
    fn direction_probe(into: std::rc::Rc<Cell<TextDirection>>) -> AnyWidget {
        leaf(move || {
            into.set(current_direction());
            SizedBox::new(1.0, 1.0)
        })
    }

    #[test]
    fn a_subtree_is_built_at_its_own_directionalitys_direction() {
        let seen = std::rc::Rc::new(Cell::new(TextDirection::Ltr));
        let mut tree = ElementTree::new();
        tree.rebuild(directionality(TextDirection::Rtl, direction_probe(std::rc::Rc::clone(&seen))));
        let _ = tree.build_render_tree();
        assert_eq!(seen.get(), TextDirection::Rtl);
    }

    #[test]
    fn a_nested_directionality_only_changes_its_own_subtree() {
        let outer = std::rc::Rc::new(Cell::new(TextDirection::Ltr));
        let inner = std::rc::Rc::new(Cell::new(TextDirection::Ltr));
        let after = std::rc::Rc::new(Cell::new(TextDirection::Ltr));

        let (o, i, a) = (std::rc::Rc::clone(&outer), std::rc::Rc::clone(&inner), std::rc::Rc::clone(&after));
        let mut tree = ElementTree::new();
        tree.rebuild(directionality(
            TextDirection::Rtl,
            many(
                vec![
                    direction_probe(o),
                    directionality(TextDirection::Ltr, direction_probe(i)),
                    // Built after the nested one, so it is what catches a
                    // direction that was pushed and never popped.
                    direction_probe(a),
                ],
                |children| {
                    let mut flex = crate::render::RenderFlex::column();
                    for child in children {
                        flex = flex.push(child);
                    }
                    Box::new(flex)
                },
            ),
        ));
        let _ = tree.build_render_tree();
        assert_eq!(outer.get(), TextDirection::Rtl);
        assert_eq!(inner.get(), TextDirection::Ltr);
        assert_eq!(after.get(), TextDirection::Rtl, "the inner direction leaked out of its subtree");
    }

    // -- Reading it from a build ----------------------------------------------

    thread_local! {
        static SEEN: Cell<TextDirection> = const { Cell::new(TextDirection::Ltr) };
    }

    /// A component that records the direction its `build` was run in.
    struct DirectionReader;

    impl crate::framework::Component for DirectionReader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            SEEN.with(|seen| seen.set(direction_of(context)));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    #[test]
    fn direction_of_reads_the_nearest_provider() {
        let mut tree = ElementTree::new();
        tree.rebuild(directionality(TextDirection::Rtl, component(DirectionReader)));
        let _ = tree.build_render_tree();
        assert_eq!(SEEN.with(|seen| seen.get()), TextDirection::Rtl);
    }

    #[test]
    fn direction_of_defaults_to_ltr_without_a_provider() {
        let mut tree = ElementTree::new();
        tree.rebuild(component(DirectionReader));
        let _ = tree.build_render_tree();
        assert_eq!(SEEN.with(|seen| seen.get()), TextDirection::Ltr);
    }
}
