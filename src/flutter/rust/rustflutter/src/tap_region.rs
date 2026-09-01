// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Regions that want to know about taps outside themselves (upstream
//! `widgets/tap_region.dart`).
//!
//! The question a menu, a popover or a text field needs answered is not "was
//! I tapped" -- a hit test answers that -- but "was something *else* tapped",
//! which no hit test can answer on its own: a region that was not hit is not
//! on the path, and a thing that is not there cannot be told anything.
//!
//! Upstream's answer, and this one, is to turn it around. A
//! [`TapRegionSurface`] high in the tree keeps the list of every registered
//! region, and when a press lands it splits that list in two: the regions the
//! press went through, and all the rest. The first set hears `on_tap_inside`
//! and the second `on_tap_outside`. Registration is what makes an unhit
//! region reachable, and the whole file is that one idea.
//!
//! # Groups
//!
//! A text field and its selection toolbar are two regions and one thing: a
//! tap on the toolbar must not read as a tap outside the field. Upstream ties
//! them with a `groupId`, and a hit on any member counts as a hit on all of
//! them. That is what [`TextFieldTapRegion`] is -- a region in the group
//! upstream reserves for text editing.
//!
//! # Recorded divergences
//!
//! * `consumeOutsideTaps` is stored and reported but does not stop the tap.
//!   Upstream stops it by putting a dummy member into the gesture arena and
//!   declaring it the winner; this crate's arena is inside the router and has
//!   no entry point for a claim from outside it. [`TapRegionRegistry::last_dispatch_consumed`]
//!   is what a caller can read instead, and the arena hook is where this would
//!   be finished.
//! * Upstream's surface also listens on the semantics channel, so that a tap
//!   delivered by a screen reader classifies like a pointer one. The semantics
//!   wave is `E3`.
//! * `debugLabel` is not modelled: the diagnostics tree is P10.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, BuildContext, provide, single};
use crate::gestures::{PointerEvent, PointerHandlers};
use crate::render::{
    BoxConstraints, BoxedRender, HitTestBehavior, HitTestResult, Offset, PaintContext, RenderBox,
    RenderRef, Size, UpdateEffect,
};

/// Upstream `TapRegionCallback`, and `TapRegionUpCallback`, which have the
/// same shape here: both are handed the event that classified the region.
pub type TapRegionCallback = Rc<dyn Fn(&PointerEvent)>;

/// What one registered region told the surface about itself.
///
/// Upstream registers the render object and reads the fields off it. Here the
/// fields are copied in at registration, because the surface holds the list
/// across frames and a render object may be replaced under it.
#[derive(Clone)]
struct RegisteredRegion {
    id: u64,
    group_id: Option<u64>,
    consume_outside_taps: bool,
    on_tap_outside: Option<TapRegionCallback>,
    on_tap_inside: Option<TapRegionCallback>,
    on_tap_up_outside: Option<TapRegionCallback>,
    on_tap_up_inside: Option<TapRegionCallback>,
}

#[derive(Default)]
struct RegistryState {
    regions: Vec<RegisteredRegion>,
    /// The ids the last hit test walked through, innermost first. Written by
    /// the surface's `hit_test` and read by its pointer handler, which is
    /// upstream's `_cachedResults` under another name.
    last_hit: Vec<u64>,
    last_dispatch_consumed: bool,
}

/// Upstream `TapRegionRegistry`: what a [`TapRegion`] registers itself with.
///
/// Upstream is an abstract class that `RenderTapRegionSurface` implements;
/// here it is the shared state itself, handed down the tree by the surface
/// and found again with [`TapRegionRegistry::of`]. The indirection upstream
/// needs -- a registry that might be implemented by something else -- buys
/// nothing in a crate where the surface is the only implementation.
#[derive(Clone, Default)]
pub struct TapRegionRegistry(Rc<RefCell<RegistryState>>);

impl TapRegionRegistry {
    pub fn new() -> TapRegionRegistry {
        TapRegionRegistry::default()
    }

    /// Upstream `TapRegionRegistry.maybeOf`.
    pub fn maybe_of(context: &mut BuildContext) -> Option<TapRegionRegistry> {
        context
            .inherited::<TapRegionRegistry>()
            .map(|registry| (*registry).clone())
    }

    /// Upstream `TapRegionRegistry.of`, which throws when there is no
    /// [`TapRegionSurface`] above. A region with nobody to register with can
    /// never be told about a tap outside it, and upstream would rather say so
    /// than fail quietly -- so this panics with the same message in spirit.
    pub fn of(context: &mut BuildContext) -> TapRegionRegistry {
        TapRegionRegistry::maybe_of(context).expect(
            "TapRegionRegistry::of() was called with a context that has no TapRegionSurface above it",
        )
    }

    /// Upstream `registerTapRegion`. Registering twice under the same id
    /// replaces the entry rather than adding a second: upstream's registry is
    /// a set of render objects, and the render object here is its id.
    fn register(&self, region: RegisteredRegion) {
        let mut state = self.0.borrow_mut();
        match state.regions.iter_mut().find(|other| other.id == region.id) {
            Some(existing) => *existing = region,
            None => state.regions.push(region),
        }
    }

    /// Upstream `unregisterTapRegion`.
    fn unregister(&self, id: u64) {
        self.0.borrow_mut().regions.retain(|region| region.id != id);
    }

    /// How many regions are registered. Upstream's set has no public size;
    /// this is here because a registry that quietly keeps a region after its
    /// widget is gone is the failure this whole file can have, and a test
    /// needs to be able to look.
    pub fn len(&self) -> usize {
        self.0.borrow().regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().regions.is_empty()
    }

    /// Whether the last press landed outside a region that asked to consume
    /// outside taps. See the module's recorded divergences: this is what the
    /// crate reports instead of stopping the tap.
    pub fn last_dispatch_consumed(&self) -> bool {
        self.0.borrow().last_dispatch_consumed
    }

    fn remember_hit(&self, ids: Vec<u64>) {
        self.0.borrow_mut().last_hit = ids;
    }

    /// Upstream's `_classifyRegions`: the regions the press went through, and
    /// all the rest.
    ///
    /// A hit on any member of a group counts as a hit on every member, which
    /// is the whole of what a group is for.
    fn classify(&self) -> (Vec<RegisteredRegion>, Vec<RegisteredRegion>) {
        let state = self.0.borrow();
        let hit: Vec<&RegisteredRegion> = state
            .regions
            .iter()
            .filter(|region| state.last_hit.contains(&region.id))
            .collect();
        let hit_groups: Vec<u64> = hit.iter().filter_map(|region| region.group_id).collect();
        let hit_ids: Vec<u64> = hit.iter().map(|region| region.id).collect();
        let inside: Vec<RegisteredRegion> = state
            .regions
            .iter()
            .filter(|region| {
                hit_ids.contains(&region.id)
                    || region
                        .group_id
                        .is_some_and(|group| hit_groups.contains(&group))
            })
            .cloned()
            .collect();
        let inside_ids: Vec<u64> = inside.iter().map(|region| region.id).collect();
        let outside: Vec<RegisteredRegion> = state
            .regions
            .iter()
            .filter(|region| !inside_ids.contains(&region.id))
            .cloned()
            .collect();
        (inside, outside)
    }

    /// Upstream's `handleEvent`: the outside regions are told first, then the
    /// inside ones.
    ///
    /// Outside first is upstream's order and it matters: a menu closing on a
    /// tap outside it, and a button under that tap being pressed, should
    /// happen in that order, or the button acts in a tree that is about to
    /// change under it.
    fn dispatch(&self, event: &PointerEvent, is_down: bool) {
        if self.0.borrow().regions.is_empty() {
            return;
        }
        let (inside, outside) = self.classify();
        let mut consumed = false;
        for region in &outside {
            let callback = if is_down {
                &region.on_tap_outside
            } else {
                &region.on_tap_up_outside
            };
            if let Some(callback) = callback {
                callback(event);
            }
            if region.consume_outside_taps {
                consumed = true;
            }
        }
        for region in &inside {
            let callback = if is_down {
                &region.on_tap_inside
            } else {
                &region.on_tap_up_inside
            };
            if let Some(callback) = callback {
                callback(event);
            }
        }
        self.0.borrow_mut().last_dispatch_consumed = consumed && is_down;
    }
}

impl std::fmt::Debug for TapRegionRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TapRegionRegistry")
            .field("regions", &self.0.borrow().regions.len())
            .finish()
    }
}

impl PartialEq for TapRegionRegistry {
    fn eq(&self, other: &TapRegionRegistry) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Upstream `RenderTapRegionSurface`.
///
/// A proxy that remembers what the last hit test walked through and, when a
/// press arrives, hands that to the registry to classify. Upstream caches the
/// hit result against the hit-test entry with an `Expando`; the cache here is
/// in the registry, because that is the object the handler closure can reach.
pub struct RenderTapRegionSurface {
    id: u64,
    registry: TapRegionRegistry,
    handlers: Rc<PointerHandlers>,
    child: BoxedRender,
    size: Size,
}

impl RenderTapRegionSurface {
    pub fn new(
        id: u64,
        registry: TapRegionRegistry,
        child: impl RenderBox + 'static,
    ) -> RenderTapRegionSurface {
        let down = registry.clone();
        let up = registry.clone();
        let handlers = PointerHandlers::new()
            .with_pointer_down(move |event| down.dispatch(event, true))
            .with_pointer_up(move |event| up.dispatch(event, false));
        RenderTapRegionSurface {
            id,
            registry,
            handlers: Rc::new(handlers),
            child: RenderRef::new(child),
            size: Size::ZERO,
        }
    }
}

impl RenderBox for RenderTapRegionSurface {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh
            .as_any_mut()
            .downcast_mut::<RenderTapRegionSurface>()?;
        self.id = fresh.id;
        // The registry outlives the frame: keeping the fresh one would throw
        // away every region that registered against the old one and had no
        // reason to register again.
        if self.registry != fresh.registry {
            self.registry = fresh.registry.clone();
            self.handlers = Rc::clone(&fresh.handlers);
        }
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout_child(constraints, true);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child.dry_layout(constraints)
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let before = result.path.len();
        let mut hit_target = false;
        if self.size.contains(position) {
            hit_target = self.child.hit_test(position, result);
            // Translucent: the surface joins the path whether or not anything
            // under it was hit, because a press on empty space is exactly the
            // one every registered region needs to hear about.
            self.registry.remember_hit(
                result.path[before..]
                    .iter()
                    .map(|entry| entry.target)
                    .collect(),
            );
            result.add_with_handlers(self.id, position, Some(Rc::clone(&self.handlers)));
        }
        hit_target
    }

    /// Upstream `RenderProxyBox`: a box that wraps another without changing
    /// its size answers every intrinsic with the child's.
    ///
    /// The default on the trait is `0.0`, which is what a box with **no**
    /// child should say -- and a proxy that never overrode it said the same,
    /// so an `IntrinsicWidth` above one measured zero and laid its subject out
    /// with no width at all. See PORTING_STATUS.md, tick 467.
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    /// Upstream `RenderProxyBox.computeDistanceToActualBaseline`, which is
    /// `child?.getDistanceToActualBaseline(baseline)`.
    ///
    /// The default on the trait is `None`, which means "this box has no
    /// baseline" -- true of a box with no text in it, and false of one that
    /// merely wraps something with text. A row aligning on the baseline treats
    /// a `None` child as having none and lines it up by its top instead, so a
    /// label inside an `Opacity` or a clip sat a few pixels off from the label
    /// beside it, and nothing said why. See PORTING_STATUS.md, tick 468.
    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

/// Upstream `RenderTapRegion`.
///
/// It registers with the surface when it is laid out and unregisters when it
/// is dropped, which is upstream's `attach`/`detach` pair told by the two
/// moments this crate has.
pub struct RenderTapRegion {
    id: u64,
    registry: Option<TapRegionRegistry>,
    enabled: bool,
    consume_outside_taps: bool,
    group_id: Option<u64>,
    on_tap_outside: Option<TapRegionCallback>,
    on_tap_inside: Option<TapRegionCallback>,
    on_tap_up_outside: Option<TapRegionCallback>,
    on_tap_up_inside: Option<TapRegionCallback>,
    behavior: HitTestBehavior,
    child: BoxedRender,
    size: Size,
}

impl RenderTapRegion {
    pub fn new(id: u64, child: impl RenderBox + 'static) -> RenderTapRegion {
        RenderTapRegion {
            id,
            registry: None,
            enabled: true,
            consume_outside_taps: false,
            group_id: None,
            on_tap_outside: None,
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
            behavior: HitTestBehavior::DeferToChild,
            child: RenderRef::new(child),
            size: Size::ZERO,
        }
    }

    pub fn with_registry(mut self, registry: TapRegionRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_consume_outside_taps(mut self, consume: bool) -> Self {
        self.consume_outside_taps = consume;
        self
    }

    pub fn with_group_id(mut self, group_id: u64) -> Self {
        self.group_id = Some(group_id);
        self
    }

    pub fn with_behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    pub fn with_on_tap_outside(mut self, callback: TapRegionCallback) -> Self {
        self.on_tap_outside = Some(callback);
        self
    }

    pub fn with_on_tap_inside(mut self, callback: TapRegionCallback) -> Self {
        self.on_tap_inside = Some(callback);
        self
    }

    pub fn with_on_tap_up_outside(mut self, callback: TapRegionCallback) -> Self {
        self.on_tap_up_outside = Some(callback);
        self
    }

    pub fn with_on_tap_up_inside(mut self, callback: TapRegionCallback) -> Self {
        self.on_tap_up_inside = Some(callback);
        self
    }

    fn entry(&self) -> RegisteredRegion {
        RegisteredRegion {
            id: self.id,
            group_id: self.group_id,
            consume_outside_taps: self.consume_outside_taps,
            on_tap_outside: self.on_tap_outside.clone(),
            on_tap_inside: self.on_tap_inside.clone(),
            on_tap_up_outside: self.on_tap_up_outside.clone(),
            on_tap_up_inside: self.on_tap_up_inside.clone(),
        }
    }

    /// Upstream's `layout` override, which is where it registers: a region
    /// that is disabled is not in the list at all, rather than in the list
    /// and skipped.
    fn sync_registration(&self) {
        let Some(registry) = &self.registry else {
            return;
        };
        if self.enabled {
            registry.register(self.entry());
        } else {
            registry.unregister(self.id);
        }
    }
}

impl Drop for RenderTapRegion {
    fn drop(&mut self) {
        if let Some(registry) = &self.registry {
            registry.unregister(self.id);
        }
    }
}

impl RenderBox for RenderTapRegion {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderTapRegion>()?;
        self.id = fresh.id;
        self.registry = fresh.registry.clone();
        self.enabled = fresh.enabled;
        self.consume_outside_taps = fresh.consume_outside_taps;
        self.group_id = fresh.group_id;
        self.on_tap_outside = fresh.on_tap_outside.clone();
        self.on_tap_inside = fresh.on_tap_inside.clone();
        self.on_tap_up_outside = fresh.on_tap_up_outside.clone();
        self.on_tap_up_inside = fresh.on_tap_up_inside.clone();
        self.behavior = fresh.behavior;
        // The callbacks are what a rebuild almost always changes, and the
        // registry is holding the old ones.
        self.sync_registration();
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.sync_registration();
        self.size = self.child.layout_child(constraints, true);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child.dry_layout(constraints)
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let mut hit_target = false;
        if self.size.contains(position) {
            hit_target = self.child.hit_test(position, result) || self.hit_test_self(position);
            if hit_target || self.behavior == HitTestBehavior::Translucent {
                result.add(self.id, position);
            }
        }
        hit_target
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        self.behavior == HitTestBehavior::Opaque
    }

    fn hit_test_id(&self) -> u64 {
        self.id
    }

    /// Upstream `RenderProxyBox`: a box that wraps another without changing
    /// its size answers every intrinsic with the child's.
    ///
    /// The default on the trait is `0.0`, which is what a box with **no**
    /// child should say -- and a proxy that never overrode it said the same,
    /// so an `IntrinsicWidth` above one measured zero and laid its subject out
    /// with no width at all. See PORTING_STATUS.md, tick 467.
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    /// Upstream `RenderProxyBox.computeDistanceToActualBaseline`, which is
    /// `child?.getDistanceToActualBaseline(baseline)`.
    ///
    /// The default on the trait is `None`, which means "this box has no
    /// baseline" -- true of a box with no text in it, and false of one that
    /// merely wraps something with text. A row aligning on the baseline treats
    /// a `None` child as having none and lines it up by its top instead, so a
    /// label inside an `Opacity` or a clip sat a few pixels off from the label
    /// beside it, and nothing said why. See PORTING_STATUS.md, tick 468.
    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

/// Upstream `TapRegionSurface`: installs a registry and the render object
/// that classifies presses against it.
pub struct TapRegionSurface;

impl TapRegionSurface {
    /// Builds a surface with a registry of its own.
    pub fn new(id: u64, child: AnyWidget) -> AnyWidget {
        TapRegionSurface::with_registry(id, TapRegionRegistry::new(), child)
    }

    /// Builds a surface over a registry the caller keeps, which is what a
    /// test -- or anything that wants to ask what is registered -- needs.
    pub fn with_registry(id: u64, registry: TapRegionRegistry, child: AnyWidget) -> AnyWidget {
        let installed = registry.clone();
        provide(
            registry,
            single(child, move |child| {
                RenderTapRegionSurface::new(id, installed.clone(), child)
            }),
        )
    }
}

/// Upstream `TapRegion`: a region that hears about taps inside and outside
/// itself.
pub struct TapRegion {
    id: u64,
    enabled: bool,
    consume_outside_taps: bool,
    group_id: Option<u64>,
    behavior: HitTestBehavior,
    on_tap_outside: Option<TapRegionCallback>,
    on_tap_inside: Option<TapRegionCallback>,
    on_tap_up_outside: Option<TapRegionCallback>,
    on_tap_up_inside: Option<TapRegionCallback>,
}

impl TapRegion {
    /// `id` is what the hit test records and what the registry keys on, so it
    /// has to be the same across rebuilds of the same region -- the same
    /// contract every other identified render object in this crate has.
    pub fn new(id: u64) -> TapRegion {
        TapRegion {
            id,
            enabled: true,
            consume_outside_taps: false,
            group_id: None,
            behavior: HitTestBehavior::DeferToChild,
            on_tap_outside: None,
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_consume_outside_taps(mut self, consume: bool) -> Self {
        self.consume_outside_taps = consume;
        self
    }

    pub fn with_group_id(mut self, group_id: u64) -> Self {
        self.group_id = Some(group_id);
        self
    }

    pub fn with_behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    pub fn with_on_tap_outside(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_tap_outside = Some(Rc::new(callback));
        self
    }

    pub fn with_on_tap_inside(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_tap_inside = Some(Rc::new(callback));
        self
    }

    pub fn with_on_tap_up_outside(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_tap_up_outside = Some(Rc::new(callback));
        self
    }

    pub fn with_on_tap_up_inside(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_tap_up_inside = Some(Rc::new(callback));
        self
    }

    /// Wraps `child`, registering with the surface above.
    ///
    /// The registry is read out of the context here rather than inside the
    /// render object, because the context is what a widget has and a render
    /// object is not built with one.
    pub fn build(self, context: &mut BuildContext, child: AnyWidget) -> AnyWidget {
        let registry = TapRegionRegistry::maybe_of(context);
        single(child, move |child| {
            let mut region = RenderTapRegion::new(self.id, child)
                .with_enabled(self.enabled)
                .with_consume_outside_taps(self.consume_outside_taps)
                .with_behavior(self.behavior);
            if let Some(registry) = &registry {
                region = region.with_registry(registry.clone());
            }
            if let Some(group) = self.group_id {
                region = region.with_group_id(group);
            }
            if let Some(callback) = &self.on_tap_outside {
                region = region.with_on_tap_outside(Rc::clone(callback));
            }
            if let Some(callback) = &self.on_tap_inside {
                region = region.with_on_tap_inside(Rc::clone(callback));
            }
            if let Some(callback) = &self.on_tap_up_outside {
                region = region.with_on_tap_up_outside(Rc::clone(callback));
            }
            if let Some(callback) = &self.on_tap_up_inside {
                region = region.with_on_tap_up_inside(Rc::clone(callback));
            }
            region
        })
    }
}

/// Upstream `TextFieldTapRegion`: a [`TapRegion`] in the group upstream keeps
/// for text editing.
///
/// It exists so that the parts of a text field that are not the field --
/// a selection toolbar, a magnifier, a spell-check menu -- do not read as
/// "somewhere else" and dismiss the very thing they belong to. Upstream keys
/// the group on the `EditableText` type; the key here is a reserved number,
/// because a group id here is a number.
pub struct TextFieldTapRegion;

impl TextFieldTapRegion {
    /// The group every part of a text field shares.
    pub const GROUP: u64 = 1;

    pub fn new(id: u64) -> TapRegion {
        TapRegion::new(id).with_group_id(TextFieldTapRegion::GROUP)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_tap_region_reports_the_intrinsics_of_what_it_wraps() {
        // A region is a proxy: it hears about presses and changes nothing
        // about size. Answering the trait's `0.0` default made an
        // `IntrinsicWidth` above one measure nothing -- which is how a menu
        // panel came to be drawn with no width at all. See PORTING_STATUS.md,
        // tick 467.
        let region = RenderTapRegion::new(
            7001,
            crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints::tight(
                64.0, 18.0,
            )),
        );
        assert_eq!(region.max_intrinsic_width(f32::INFINITY), 64.0);
        assert_eq!(region.min_intrinsic_width(f32::INFINITY), 64.0);
        assert_eq!(region.max_intrinsic_height(f32::INFINITY), 18.0);
        assert_eq!(region.min_intrinsic_height(f32::INFINITY), 18.0);
    }

    #[test]
    fn a_tap_region_keeps_its_child_s_baseline() {
        // The other default that lies for a wrapper: `None` means "no
        // baseline", which is true of an empty box and false of one holding a
        // line of text. A row aligning on the baseline lines a `None` child up
        // by its top instead. See PORTING_STATUS.md, tick 468.
        struct Lettered;
        impl crate::render::RenderBox for Lettered {
            fn layout(
                &mut self,
                constraints: crate::render::BoxConstraints,
            ) -> crate::render::Size {
                constraints.smallest()
            }
            fn size(&self) -> crate::render::Size {
                crate::render::Size::ZERO
            }
            fn paint(
                &self,
                _context: &mut crate::render::PaintContext,
                _offset: crate::render::Offset,
            ) {
            }
            fn distance_to_baseline(&self) -> Option<f32> {
                Some(11.0)
            }
        }
        let region = RenderTapRegion::new(7002, Lettered);
        assert_eq!(region.distance_to_baseline(), Some(11.0));
    }
    use super::*;
    use crate::framework::{Component, ElementTree, component, leaf};
    use crate::gestures::{GestureRouter, PointerChange, PointerKind, SignalKind};
    use crate::render::Alignment;
    use crate::widgets::SizedBox;

    /// A component that wraps a fixed box in a tap region, so the region can
    /// read the registry out of a real context.
    struct Region {
        region: RefCell<Option<TapRegion>>,
        width: f32,
    }

    impl Component for Region {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let width = self.width;
            self.region
                .borrow_mut()
                .take()
                .expect("built once")
                .build(context, leaf(move || SizedBox::new(width, 20.0)))
        }
    }

    fn at(x: f32, change: PointerChange) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: 1,
            time_stamp_micros: 0,
            position: Offset::new(x, 10.0),
            delta: Offset::ZERO,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(x, 10.0),
        }
    }

    fn press(router: &mut GestureRouter, root: &dyn RenderBox, x: f32, change: PointerChange) {
        router.dispatch(root, &at(x, change));
    }

    /// A surface over one region at the left of a wide row, so that a press
    /// can land either on it or well clear of it.
    fn one_region(registry: TapRegionRegistry, region: TapRegion) -> AnyWidget {
        TapRegionSurface::with_registry(
            1,
            registry,
            crate::framework::single(
                component(Region {
                    region: RefCell::new(Some(region)),
                    width: 40.0,
                }),
                |child| crate::render::RenderAlign::new(Alignment::CENTER_LEFT, child),
            ),
        )
    }

    #[test]
    fn a_press_clear_of_the_region_is_a_tap_outside_and_one_on_it_is_not() {
        let outside = Rc::new(RefCell::new(0));
        let inside = Rc::new(RefCell::new(0));
        let seen_out = Rc::clone(&outside);
        let seen_in = Rc::clone(&inside);
        let registry = TapRegionRegistry::new();
        let mut tree = ElementTree::new();
        tree.rebuild(one_region(
            registry.clone(),
            TapRegion::new(7)
                .with_behavior(HitTestBehavior::Opaque)
                .with_on_tap_outside(move |_| *seen_out.borrow_mut() += 1)
                .with_on_tap_inside(move |_| *seen_in.borrow_mut() += 1),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(200.0, 20.0));
        // Registration happens at layout, which is upstream's moment too.
        assert_eq!(registry.len(), 1);

        let mut router = GestureRouter::new();
        press(&mut router, &root, 100.0, PointerChange::Down);
        assert_eq!(
            *outside.borrow(),
            1,
            "a press clear of the region is outside"
        );
        assert_eq!(*inside.borrow(), 0);

        press(&mut router, &root, 10.0, PointerChange::Down);
        assert_eq!(*inside.borrow(), 1, "a press on the region is inside");
        assert_eq!(
            *outside.borrow(),
            1,
            "and it is not also outside -- the two sets are a partition"
        );
    }

    #[test]
    fn a_region_that_was_never_hit_still_hears_about_the_press() {
        // This is the whole point of the registry, and the thing a hit test
        // cannot do on its own: a region that is not under the finger is not
        // on the hit path, so without registration there would be nothing to
        // call.
        let heard = Rc::new(RefCell::new(false));
        let sink = Rc::clone(&heard);
        let registry = TapRegionRegistry::new();
        let mut tree = ElementTree::new();
        tree.rebuild(one_region(
            registry.clone(),
            TapRegion::new(7)
                .with_behavior(HitTestBehavior::Opaque)
                .with_on_tap_outside(move |_| *sink.borrow_mut() = true),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(200.0, 20.0));

        let mut router = GestureRouter::new();
        press(&mut router, &root, 180.0, PointerChange::Down);
        assert!(*heard.borrow());
    }

    #[test]
    fn a_disabled_region_is_not_in_the_list_at_all() {
        // Upstream unregisters rather than registering and skipping, so a
        // disabled region cannot be told anything -- not even that a tap
        // landed outside it.
        let heard = Rc::new(RefCell::new(false));
        let sink = Rc::clone(&heard);
        let registry = TapRegionRegistry::new();
        let mut tree = ElementTree::new();
        tree.rebuild(one_region(
            registry.clone(),
            TapRegion::new(7)
                .with_enabled(false)
                .with_on_tap_outside(move |_| *sink.borrow_mut() = true),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(200.0, 20.0));
        assert_eq!(registry.len(), 0);

        let mut router = GestureRouter::new();
        press(&mut router, &root, 180.0, PointerChange::Down);
        assert!(!*heard.borrow());
    }

    #[test]
    fn a_hit_on_one_member_of_a_group_counts_for_every_member() {
        // A text field and its toolbar are two regions and one thing. Without
        // the group, tapping the toolbar would read as a tap outside the
        // field and dismiss the very thing that was tapped.
        let registry = TapRegionRegistry::new();
        let field_outside = Rc::new(RefCell::new(0));
        let sink = Rc::clone(&field_outside);
        // The field is registered directly, standing in for one that is
        // somewhere else on screen; the toolbar is the one under the finger.
        registry.register(RegisteredRegion {
            id: 20,
            group_id: Some(TextFieldTapRegion::GROUP),
            consume_outside_taps: false,
            on_tap_outside: Some(Rc::new(move |_| *sink.borrow_mut() += 1)),
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        });
        registry.register(RegisteredRegion {
            id: 21,
            group_id: Some(TextFieldTapRegion::GROUP),
            consume_outside_taps: false,
            on_tap_outside: None,
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        });
        registry.remember_hit(vec![21]);
        let event = at(0.0, PointerChange::Down);
        registry.dispatch(&event, true);
        assert_eq!(
            *field_outside.borrow(),
            0,
            "the field is inside because its group-mate was hit"
        );

        // Take the group away and the same hit reads the other way round.
        registry.register(RegisteredRegion {
            id: 20,
            group_id: None,
            consume_outside_taps: false,
            on_tap_outside: Some(Rc::new({
                let sink = Rc::clone(&field_outside);
                move |_| *sink.borrow_mut() += 1
            })),
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        });
        registry.dispatch(&event, true);
        assert_eq!(*field_outside.borrow(), 1);
    }

    #[test]
    fn the_outside_regions_are_told_before_the_inside_ones() {
        // Upstream's order, and it matters: a menu closing on a tap outside
        // it and a button under that tap being pressed should happen in that
        // order, or the button acts in a tree that is about to change.
        let order = Rc::new(RefCell::new(Vec::new()));
        let registry = TapRegionRegistry::new();
        let out = Rc::clone(&order);
        registry.register(RegisteredRegion {
            id: 1,
            group_id: None,
            consume_outside_taps: false,
            on_tap_outside: Some(Rc::new(move |_| out.borrow_mut().push("outside"))),
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        });
        let inn = Rc::clone(&order);
        registry.register(RegisteredRegion {
            id: 2,
            group_id: None,
            consume_outside_taps: false,
            on_tap_outside: None,
            on_tap_inside: Some(Rc::new(move |_| inn.borrow_mut().push("inside"))),
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        });
        registry.remember_hit(vec![2]);
        registry.dispatch(&at(0.0, PointerChange::Down), true);
        assert_eq!(*order.borrow(), vec!["outside", "inside"]);
    }

    #[test]
    fn a_release_calls_the_up_callbacks_and_not_the_down_ones() {
        let downs = Rc::new(RefCell::new(0));
        let ups = Rc::new(RefCell::new(0));
        let registry = TapRegionRegistry::new();
        let down_sink = Rc::clone(&downs);
        let up_sink = Rc::clone(&ups);
        registry.register(RegisteredRegion {
            id: 1,
            group_id: None,
            consume_outside_taps: false,
            on_tap_outside: Some(Rc::new(move |_| *down_sink.borrow_mut() += 1)),
            on_tap_inside: None,
            on_tap_up_outside: Some(Rc::new(move |_| *up_sink.borrow_mut() += 1)),
            on_tap_up_inside: None,
        });
        registry.dispatch(&at(0.0, PointerChange::Down), true);
        assert_eq!((*downs.borrow(), *ups.borrow()), (1, 0));
        registry.dispatch(&at(0.0, PointerChange::Down), false);
        assert_eq!((*downs.borrow(), *ups.borrow()), (1, 1));
    }

    #[test]
    fn consuming_an_outside_tap_is_reported_rather_than_enforced() {
        // The recorded divergence, asserted so it cannot quietly become
        // something else: the flag reaches the registry and is readable, and
        // the tap is not actually stopped.
        let registry = TapRegionRegistry::new();
        registry.register(RegisteredRegion {
            id: 1,
            group_id: None,
            consume_outside_taps: true,
            on_tap_outside: None,
            on_tap_inside: None,
            on_tap_up_outside: None,
            on_tap_up_inside: None,
        });
        registry.dispatch(&at(0.0, PointerChange::Down), true);
        assert!(registry.last_dispatch_consumed());
        // A release does not consume: upstream only claims the arena on the
        // press.
        registry.dispatch(&at(0.0, PointerChange::Down), false);
        assert!(!registry.last_dispatch_consumed());
    }

    #[test]
    fn a_text_field_region_is_born_in_the_text_field_group() {
        let region = TextFieldTapRegion::new(3);
        assert_eq!(region.group_id, Some(TextFieldTapRegion::GROUP));
    }
}
