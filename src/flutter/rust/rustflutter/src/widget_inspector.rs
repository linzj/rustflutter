//! The widget inspector's bookkeeping -- a port of upstream's
//! `widgets/widget_inspector.dart`.
//!
//! The service exists because a debugger on the other end of a socket cannot
//! hold a Dart object. It can only hold a **string**, so the inspector hands
//! out ids and keeps a table. That one constraint shapes everything here:
//!
//! * Ids are grouped, so a tool can drop everything it was looking at in one
//!   call rather than leaking a reference per inspected widget.
//! * References are counted, because the same widget can be in two groups at
//!   once and neither group may free it out from under the other.
//! * The table holds objects **weakly**, so inspecting a widget does not keep
//!   the widget alive.
//!
//! The other half of the file is the summary tree: which nodes a tool sees by
//! default. The rule is that a reader debugging their own application wants
//! their own widgets, not the forty framework widgets each one expands into.
//!
//! ## What is not here
//!
//! The VM service extension registrations, the JSON encoding, and the overlay
//! that paints the selection are all absent -- this crate has no service
//! protocol and no `Element` tree of upstream's shape. What is ported is the
//! reference table and its counting, the selection state machine, the
//! local-project test with its cache, and the summary-tree filter.

use crate::diagnostics::{
    DiagnosticLevel, DiagnosticsProperty, DiagnosticsSerializationDelegate, PropertyValue,
};
use crate::engine::Color;
use crate::icon_data::IconData;
use std::collections::{HashMap, HashSet};

/// Upstream `WeakMap`: a map that does not keep its keys alive.
///
/// Upstream splits it in two, and the split is not an optimisation -- Dart's
/// `Expando` **refuses** strings, numbers and booleans as keys, so those have
/// to live in an ordinary map beside it. The consequence is worth stating:
/// primitive keys are held **strongly** and object keys are not.
///
/// This crate has no garbage collector, so nothing here is ever collected;
/// what is ported is the interface and the primitive/object distinction that
/// callers can observe.
#[derive(Debug, Clone)]
pub struct WeakMap<K, V> {
    objects: HashMap<K, V>,
    primitives: HashMap<K, V>,
}

impl<K, V> Default for WeakMap<K, V> {
    fn default() -> WeakMap<K, V> {
        WeakMap {
            objects: HashMap::new(),
            primitives: HashMap::new(),
        }
    }
}

impl<K: std::hash::Hash + Eq, V> WeakMap<K, V> {
    pub fn new() -> WeakMap<K, V> {
        WeakMap::default()
    }

    fn table(&mut self, is_primitive: bool) -> &mut HashMap<K, V> {
        if is_primitive {
            &mut self.primitives
        } else {
            &mut self.objects
        }
    }

    pub fn get(&self, key: &K, is_primitive: bool) -> Option<&V> {
        if is_primitive {
            self.primitives.get(key)
        } else {
            self.objects.get(key)
        }
    }

    pub fn insert(&mut self, key: K, value: V, is_primitive: bool) {
        self.table(is_primitive).insert(key, value);
    }

    pub fn remove(&mut self, key: &K, is_primitive: bool) -> Option<V> {
        if is_primitive {
            self.primitives.remove(key)
        } else {
            self.objects.remove(key)
        }
    }

    /// Upstream replaces the whole `Expando` here rather than emptying it,
    /// which is the same thing observed from outside and cheaper than walking
    /// keys it cannot enumerate.
    pub fn clear(&mut self) {
        self.objects.clear();
        self.primitives.clear();
    }

    pub fn len(&self) -> usize {
        self.objects.len() + self.primitives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Whether a value is one of the kinds upstream cannot hold weakly.
pub fn is_primitive_key(value: &str) -> bool {
    value.parse::<f64>().is_ok() || value == "true" || value == "false"
}

/// Upstream `InspectorReferenceData`: one entry in the inspector's table.
///
/// The reference count is per **group membership**, not per request: a tool
/// asking for the same widget's id twice within one group gets the same id and
/// the count does not move. It only moves when a *second* group takes an
/// interest, which is exactly when dropping one group must not free the
/// object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorReferenceData {
    /// The id handed to the tool.
    pub id: String,
    /// How many groups hold this reference.
    pub count: usize,
    /// The value, held **strongly only when it is a primitive** -- upstream's
    /// `WeakReference` rejects strings, numbers and booleans, so those are
    /// kept by value while everything else is kept weakly and may vanish.
    value: Option<String>,
    strongly_held: bool,
}

impl InspectorReferenceData {
    pub fn new(value: impl Into<String>, id: impl Into<String>) -> InspectorReferenceData {
        let value = value.into();
        let strongly_held = is_primitive_key(&value);
        InspectorReferenceData {
            id: id.into(),
            count: 1,
            value: Some(value),
            strongly_held,
        }
    }

    /// Upstream's `value` getter: `_ref?.target ?? _value`. A weakly held
    /// object that has been collected reads as `None` while its id is still in
    /// the table, which is the state a tool sees as "that widget is gone".
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    pub fn is_strongly_held(&self) -> bool {
        self.strongly_held
    }

    /// Simulates the referent being collected. Only possible for the weakly
    /// held ones.
    pub fn collect(&mut self) {
        if !self.strongly_held {
            self.value = None;
        }
    }
}

/// Why a lookup failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorError {
    /// Upstream throws `FlutterError('Id does not exist.')`.
    IdDoesNotExist,
    /// Upstream throws `FlutterError('Id is not in group')`. Separate from the
    /// above on purpose: an id that exists but belongs to somebody else is a
    /// tool bug, where an id that does not exist is a stale reference.
    IdNotInGroup,
}

/// Upstream `WidgetInspectorService`: the table a debugging tool talks to.
#[derive(Debug, Default)]
pub struct WidgetInspectorService {
    groups: HashMap<String, HashSet<String>>,
    id_to_reference: HashMap<String, InspectorReferenceData>,
    value_to_id: HashMap<String, String>,
    next_id: u64,
    pub_root_directories: Option<Vec<String>>,
    /// Upstream's `_isLocalCreationCache`, memoising a string test that runs
    /// once per node of every tree the tool asks for.
    local_creation_cache: HashMap<String, bool>,
    pub selection: InspectorSelection,
    /// Upstream's `isWidgetCreationTracked`: whether the compiler recorded
    /// where each widget was constructed. Without it the summary tree cannot
    /// be computed at all.
    pub widget_creation_tracked: bool,
    pub select_mode: bool,
}

impl WidgetInspectorService {
    /// Upstream's id prefix.
    pub const ID_PREFIX: &'static str = "inspector-";

    pub fn new() -> WidgetInspectorService {
        WidgetInspectorService {
            widget_creation_tracked: true,
            ..WidgetInspectorService::default()
        }
    }

    /// Upstream's `toId`.
    ///
    /// The count moves only when the reference joins a **new** group. Asking
    /// twice in one group is the common case -- a tool serialising a tree that
    /// mentions the same widget in two places -- and counting it would leave a
    /// reference that `disposeGroup` could never bring back to zero.
    pub fn to_id(&mut self, value: Option<&str>, group_name: &str) -> Option<String> {
        let value = value?;
        let existing = self.value_to_id.get(value).cloned();
        let id = match existing {
            Some(id) => {
                let group = self.groups.entry(group_name.to_string()).or_default();
                if group.insert(id.clone()) {
                    if let Some(reference) = self.id_to_reference.get_mut(&id) {
                        reference.count += 1;
                    }
                }
                id
            }
            None => {
                let id = format!("{}{}", Self::ID_PREFIX, self.next_id);
                self.next_id += 1;
                self.value_to_id.insert(value.to_string(), id.clone());
                self.id_to_reference
                    .insert(id.clone(), InspectorReferenceData::new(value, id.clone()));
                self.groups
                    .entry(group_name.to_string())
                    .or_default()
                    .insert(id.clone());
                id
            }
        };
        Some(id)
    }

    /// Upstream's `toObject`, which **throws** on an unknown id rather than
    /// returning null. Null is already a meaningful answer here: it is a
    /// weakly held object that has been collected.
    pub fn to_object(&self, id: Option<&str>) -> Result<Option<&str>, InspectorError> {
        let Some(id) = id else {
            return Ok(None);
        };
        let reference = self
            .id_to_reference
            .get(id)
            .ok_or(InspectorError::IdDoesNotExist)?;
        Ok(reference.value())
    }

    /// Upstream's `toObjectForSourceLocation`, which swaps an `Element` for
    /// the `Widget` that configured it: the reader asked where this thing came
    /// from, and the widget's class is the answer they meant. The element's
    /// class is a framework detail they did not write.
    pub fn to_object_for_source_location(
        &self,
        id: &str,
        element_to_widget: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<String>, InspectorError> {
        let object = self.to_object(Some(id))?;
        let Some(object) = object else {
            return Ok(None);
        };
        Ok(Some(
            element_to_widget(object).unwrap_or_else(|| object.to_string()),
        ))
    }

    /// Upstream's `disposeGroup`. Objects still referenced from another group
    /// stay alive, ids and all.
    pub fn dispose_group(&mut self, name: &str) {
        let Some(references) = self.groups.remove(name) else {
            return;
        };
        for id in references {
            self.decrement_reference_count(&id);
        }
    }

    pub fn dispose_all_groups(&mut self) {
        let names: Vec<String> = self.groups.keys().cloned().collect();
        for name in names {
            self.dispose_group(&name);
        }
    }

    /// Upstream's `disposeId`, which distinguishes the two failures: an id
    /// that is not in the table at all, and one that is but belongs to another
    /// group.
    pub fn dispose_id(&mut self, id: Option<&str>, group_name: &str) -> Result<(), InspectorError> {
        let Some(id) = id else {
            return Ok(());
        };
        if !self.id_to_reference.contains_key(id) {
            return Err(InspectorError::IdDoesNotExist);
        }
        let removed = self
            .groups
            .get_mut(group_name)
            .map(|group| group.remove(id))
            .unwrap_or(false);
        if !removed {
            return Err(InspectorError::IdNotInGroup);
        }
        self.decrement_reference_count(id);
        Ok(())
    }

    fn decrement_reference_count(&mut self, id: &str) {
        let Some(reference) = self.id_to_reference.get_mut(id) else {
            return;
        };
        debug_assert!(reference.count > 0, "reference count went below zero");
        reference.count -= 1;
        if reference.count > 0 {
            return;
        }
        // Upstream removes the value->id entry only if the value is still
        // there: a collected referent has nothing left to look up by, and its
        // entry in the forward map was dropped with it.
        if let Some(value) = reference.value().map(str::to_string) {
            self.value_to_id.remove(&value);
        }
        self.id_to_reference.remove(id);
    }

    /// How many groups hold this id, for tests and for the assertion above.
    pub fn reference_count(&self, id: &str) -> Option<usize> {
        self.id_to_reference.get(id).map(|data| data.count)
    }

    pub fn group_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.groups.keys().cloned().collect();
        names.sort();
        names
    }

    /// Upstream's `addPubRootDirectories`.
    pub fn add_pub_root_directories(&mut self, directories: &[&str]) {
        let roots = self.pub_root_directories.get_or_insert_with(Vec::new);
        for directory in directories {
            if !roots.iter().any(|held| held == directory) {
                roots.push((*directory).to_string());
            }
        }
        // Any cached answer was computed under the old roots.
        self.local_creation_cache.clear();
    }

    pub fn remove_pub_root_directories(&mut self, directories: &[&str]) {
        if let Some(roots) = self.pub_root_directories.as_mut() {
            roots.retain(|held| !directories.contains(&held.as_str()));
        }
        self.local_creation_cache.clear();
    }

    pub fn reset_pub_root_directories(&mut self) {
        self.pub_root_directories = None;
        self.local_creation_cache.clear();
    }

    pub fn pub_root_directories(&self) -> Option<&[String]> {
        self.pub_root_directories.as_deref()
    }

    /// Upstream's `_isLocalCreationLocationImpl`.
    ///
    /// With no roots configured the test is **"not inside `packages/flutter/`"**
    /// -- a guess, and upstream says so with a TODO pointing at
    /// flutter/flutter#32660. It is the right guess for the common case: a
    /// reader running one application wants everything that is not the
    /// framework, and enumerating what *is* theirs needs a build system to say
    /// so.
    pub fn is_local_creation_location(&mut self, location_uri: &str) -> bool {
        if let Some(cached) = self.local_creation_cache.get(location_uri) {
            return *cached;
        }
        let answer = match self.pub_root_directories.as_ref() {
            None => !location_uri.contains("packages/flutter/"),
            Some(roots) => roots.iter().any(|root| location_uri.starts_with(root)),
        };
        self.local_creation_cache
            .insert(location_uri.to_string(), answer);
        answer
    }

    /// Upstream's `_shouldShowInSummaryTree`, and every branch but the last
    /// says **include it**.
    ///
    /// That default matters: the summary tree is a filter, and a filter that
    /// guesses wrong should hide nothing. An error node always shows; a value
    /// that is not diagnosable always shows; and when the compiler did not
    /// record creation locations, *everything* shows, because there is no way
    /// to tell whose widget it is.
    pub fn should_show_in_summary_tree(
        &mut self,
        level: DiagnosticLevel,
        is_diagnosticable: bool,
        is_element: bool,
        creation_location: Option<&str>,
    ) -> bool {
        if level == DiagnosticLevel::Error {
            return true;
        }
        if !is_diagnosticable {
            return true;
        }
        if !is_element || !self.widget_creation_tracked {
            return true;
        }
        match creation_location {
            Some(location) => self.is_local_creation_location(location),
            None => true,
        }
    }

    /// Upstream's `setSelection`.
    pub fn set_selection(&mut self, render_object: Option<u64>) -> bool {
        if self.selection.current() == render_object {
            return false;
        }
        self.selection.set_current(render_object);
        true
    }

    /// Upstream's `setSelectionById`.
    pub fn set_selection_by_id(&mut self, id: Option<&str>) -> Result<bool, InspectorError> {
        let value = self.to_object(id)?;
        let render_object = value.and_then(|value| value.parse::<u64>().ok());
        Ok(self.set_selection(render_object))
    }

    /// Upstream's `resetAllState`, which is all three: the groups, the
    /// selection, and the roots. A tool reconnecting should not inherit the
    /// last one's idea of what was selected.
    pub fn reset_all_state(&mut self) {
        self.dispose_all_groups();
        self.selection.clear();
        self.reset_pub_root_directories();
    }
}

/// Upstream `InspectorSelection`: what the reader tapped, and what else was
/// under their finger.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InspectorSelection {
    candidates: Vec<u64>,
    index: usize,
    current: Option<u64>,
    /// Upstream's `_current!.attached`: a render object detached from the tree
    /// is a stale selection.
    attached: bool,
    notifications: usize,
}

impl InspectorSelection {
    pub fn new() -> InspectorSelection {
        InspectorSelection::default()
    }

    pub fn candidates(&self) -> &[u64] {
        &self.candidates
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's `current` getter, which returns null unless [`Self::active`]
    /// -- a selection whose render object has left the tree is not a selection
    /// any more, even though the field still holds it.
    pub fn current(&self) -> Option<u64> {
        if self.active() { self.current } else { None }
    }

    /// Upstream's `active`.
    pub fn active(&self) -> bool {
        self.current.is_some() && self.attached
    }

    pub fn set_attached(&mut self, attached: bool) {
        self.attached = attached;
    }

    /// Setting the candidates resets the index and recomputes the selection.
    pub fn set_candidates(&mut self, candidates: Vec<u64>) {
        self.candidates = candidates;
        self.index = 0;
        self.compute_current();
    }

    /// Upstream's `index` setter: the reader stepping through what was under
    /// their finger.
    pub fn set_index(&mut self, index: usize) {
        self.index = index;
        self.compute_current();
    }

    pub fn set_current(&mut self, current: Option<u64>) {
        if self.current != current {
            self.current = current;
            self.attached = current.is_some();
            self.notifications += 1;
        }
    }

    /// Upstream's `clear`: everything goes.
    pub fn clear(&mut self) {
        self.candidates.clear();
        self.index = 0;
        self.compute_current();
    }

    /// Upstream's `clearCandidates`, which is **not** `clear`: it drops the
    /// hit-test candidates and leaves the selection alone.
    ///
    /// It exists for the case where the selection came from DevTools rather
    /// than from a tap on the device. The stale candidates are whatever the
    /// reader last touched on screen, and drawing them around a widget chosen
    /// in a different window would be highlighting the wrong thing.
    pub fn clear_candidates(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.candidates.clear();
        self.index = 0;
    }

    fn compute_current(&mut self) {
        if self.index < self.candidates.len() {
            self.current = Some(self.candidates[self.index]);
            self.attached = true;
        } else {
            self.current = None;
            self.attached = false;
        }
        self.notifications += 1;
    }
}

/// Upstream `WidgetInspector`: the widget that turns taps into selections.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WidgetInspector {
    pub selection: InspectorSelection,
    /// Where the pointer last was, remembered across the gesture.
    ///
    /// Upstream keeps it because the selection is made on the **up** event,
    /// not the down: the reader is allowed to move their finger around the
    /// screen watching the highlight follow before committing.
    pub last_pointer_location: Option<(f32, f32)>,
    pub is_select_mode: bool,
}

impl WidgetInspector {
    pub fn new() -> WidgetInspector {
        WidgetInspector {
            is_select_mode: true,
            ..WidgetInspector::default()
        }
    }

    /// A pointer moved; the highlight follows but nothing is committed.
    pub fn handle_pointer_move(&mut self, position: (f32, f32), hits: Vec<u64>) {
        if !self.is_select_mode {
            return;
        }
        self.last_pointer_location = Some(position);
        self.selection.set_candidates(hits);
    }

    /// The finger lifted: this is the selection.
    pub fn handle_pointer_up(&mut self) -> Option<u64> {
        if !self.is_select_mode {
            return None;
        }
        self.last_pointer_location = None;
        self.selection.current()
    }
}

/// Upstream `EnableWidgetInspectorScope`: everything below this is reported.
///
/// The pair exists because a widget can be *structurally* part of the
/// application while being something the reader never wrote -- the inspector's
/// own overlay and buttons, for one. Wrapping those in a disable scope keeps
/// them out of the tree the reader is inspecting, and this one lets a genuine
/// subtree back in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnableWidgetInspectorScope;

/// Upstream `DisableWidgetInspectorScope`: everything below this is hidden
/// from the inspector until an [`EnableWidgetInspectorScope`] turns it back
/// on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisableWidgetInspectorScope;

/// Upstream `InspectorButtonVariant`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorButtonVariant {
    /// A solid background with a contrasting icon.
    Filled,
    /// On or off, and it shows which.
    Toggle,
    /// The icon and nothing else.
    IconOnly,
}

/// Upstream `InspectorButton`: the base every design's inspector button
/// extends.
///
/// It is abstract with two colour hooks and a `build`, which is the whole
/// reason it exists: the inspector ships in both Material and Cupertino
/// applications and must not drag one design's theme into the other.
pub trait InspectorButton {
    /// Upstream's `buttonSize`.
    const BUTTON_SIZE: f32 = 32.0;
    /// Upstream's `buttonIconSize`, for the variants where the icon shares the
    /// button with a background.
    const BUTTON_ICON_SIZE: f32 = 18.0;

    fn variant(&self) -> InspectorButtonVariant;
    fn semantics_label(&self) -> &str;
    fn icon(&self) -> IconData;

    /// `Some` only for [`InspectorButtonVariant::Toggle`]; upstream's named
    /// constructors leave it null for the other two rather than defaulting it,
    /// so a filled button cannot be read as "toggled off".
    fn toggled_on(&self) -> Option<bool>;

    fn foreground_color(&self) -> Color;
    fn background_color(&self) -> Color;

    /// Upstream's `iconSizeForVariant`: an icon-only button has no background
    /// to sit inside, so the icon takes the whole button.
    fn icon_size_for_variant(&self) -> f32 {
        match self.variant() {
            InspectorButtonVariant::IconOnly => Self::BUTTON_SIZE,
            InspectorButtonVariant::Filled | InspectorButtonVariant::Toggle => {
                Self::BUTTON_ICON_SIZE
            }
        }
    }
}

/// Upstream `DevToolsDeepLinkProperty`: a link into DevTools, shown in an
/// error's diagnostics.
///
/// Note the empty name: the property is rendered by its `description` alone,
/// because a label in front of a URL in an error dump is noise the reader has
/// to read past to get to the link.
#[derive(Debug, Clone, PartialEq)]
pub struct DevToolsDeepLinkProperty {
    pub property: DiagnosticsProperty,
}

impl DevToolsDeepLinkProperty {
    pub fn new(description: impl Into<String>, url: impl Into<String>) -> DevToolsDeepLinkProperty {
        let mut property = DiagnosticsProperty::new(Some(""), PropertyValue::Text(url.into()));
        property.description = Some(description.into());
        property.default_level = DiagnosticLevel::Info;
        DevToolsDeepLinkProperty { property }
    }

    pub fn url(&self) -> Option<&str> {
        match &self.property.value {
            PropertyValue::Text(url) => Some(url),
            _ => None,
        }
    }
}

/// Upstream `InspectorSerializationDelegate`: how a diagnostics tree is turned
/// into what a tool sees.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorSerializationDelegate {
    pub base: DiagnosticsSerializationDelegate,
    /// Upstream's `groupName`. Its presence is what makes the serialisation
    /// **interactive**: without it, no live ids go into the output, and the
    /// tool gets a snapshot it cannot click on.
    pub group_name: Option<String>,
    pub summary_tree: bool,
    /// Upstream's `maxDescendantsTruncatableNode`, `-1` meaning no limit.
    pub max_descendants_truncatable_node: i32,
    pub in_disable_widget_inspector_scope: bool,
    nodes_created_by_local_project: Vec<String>,
}

impl Default for InspectorSerializationDelegate {
    fn default() -> InspectorSerializationDelegate {
        InspectorSerializationDelegate::new()
    }
}

impl InspectorSerializationDelegate {
    pub fn new() -> InspectorSerializationDelegate {
        InspectorSerializationDelegate {
            base: DiagnosticsSerializationDelegate::new().with_subtree_depth(1),
            group_name: None,
            summary_tree: false,
            max_descendants_truncatable_node: -1,
            in_disable_widget_inspector_scope: false,
            nodes_created_by_local_project: Vec::new(),
        }
    }

    pub fn with_group_name(mut self, group_name: impl Into<String>) -> Self {
        self.group_name = Some(group_name.into());
        self
    }

    pub fn with_summary_tree(mut self, summary_tree: bool) -> Self {
        self.summary_tree = summary_tree;
        self
    }

    pub fn with_subtree_depth(mut self, depth: usize) -> Self {
        self.base = self.base.with_subtree_depth(depth);
        self
    }

    /// Upstream's `_interactive`.
    pub fn is_interactive(&self) -> bool {
        self.group_name.is_some()
    }

    /// Records that a node was created by the reader's own project, which
    /// [`Self::property_filter_level`] later consults.
    pub fn note_created_by_local_project(&mut self, node: impl Into<String>) {
        self.nodes_created_by_local_project.push(node.into());
    }

    /// Upstream's `filterProperties` threshold, and it is a real decision:
    /// **your own widgets show more of themselves than the framework's.** A
    /// node the reader wrote is filtered at `fine`, so nearly every property
    /// survives; everything else is filtered at `info`. The reader is debugging
    /// their code, not `RenderFlex`.
    pub fn property_filter_level(&self, owner: &str) -> DiagnosticLevel {
        if self
            .nodes_created_by_local_project
            .iter()
            .any(|node| node == owner)
        {
            DiagnosticLevel::Fine
        } else {
            DiagnosticLevel::Info
        }
    }

    /// Upstream's `delegateForNode`, and the comment on it names the trick:
    /// in the details tree the depth is **held above zero** until a node that
    /// is also in the summary tree is reached.
    ///
    /// The effect is that expanding one node in the details tree expands the
    /// whole run of framework widgets underneath it, down to the next widget
    /// the reader actually wrote. Spending the depth per level instead would
    /// make the reader click through six `RenderObjectWidget`s to see the next
    /// thing of theirs.
    pub fn delegate_for_node(&self, shows_in_summary_tree: bool) -> InspectorSerializationDelegate {
        if self.summary_tree || self.base.subtree_depth > 1 || shows_in_summary_tree {
            let mut next = self.clone();
            next.base = next.base.delegate_for_node();
            next
        } else {
            self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The reference table -----------------------------------------------

    #[test]
    fn asking_twice_in_one_group_hands_back_the_same_id_and_does_not_count_it_twice() {
        // A tool serialising a tree that mentions the same widget in two
        // places is the common case, and counting it would leave a reference
        // disposeGroup could never bring back to zero.
        let mut service = WidgetInspectorService::new();
        let first = service.to_id(Some("widget-a"), "tree").unwrap();
        let again = service.to_id(Some("widget-a"), "tree").unwrap();
        assert_eq!(first, again);
        assert_eq!(service.reference_count(&first), Some(1));
    }

    #[test]
    fn a_second_group_taking_an_interest_is_what_moves_the_count() {
        // And it is exactly the case where dropping one group must not free
        // the object out from under the other.
        let mut service = WidgetInspectorService::new();
        let id = service.to_id(Some("widget-a"), "tree").unwrap();
        service.to_id(Some("widget-a"), "details").unwrap();
        assert_eq!(service.reference_count(&id), Some(2));

        service.dispose_group("tree");
        assert_eq!(
            service.reference_count(&id),
            Some(1),
            "still held by the other group"
        );
        assert_eq!(service.to_object(Some(&id)), Ok(Some("widget-a")));

        service.dispose_group("details");
        assert_eq!(service.reference_count(&id), None);
        assert_eq!(
            service.to_object(Some(&id)),
            Err(InspectorError::IdDoesNotExist),
            "and now the id is stale"
        );
    }

    #[test]
    fn a_freed_object_gets_a_fresh_id_rather_than_the_old_one_back() {
        // The forward map went with it, so nothing connects the value to the
        // number a disconnected tool might still be holding.
        let mut service = WidgetInspectorService::new();
        let first = service.to_id(Some("widget-a"), "tree").unwrap();
        service.dispose_group("tree");
        let second = service.to_id(Some("widget-a"), "tree").unwrap();
        assert_ne!(first, second);
        assert!(second.starts_with(WidgetInspectorService::ID_PREFIX));
    }

    #[test]
    fn an_unknown_id_and_an_id_in_the_wrong_group_are_different_failures() {
        // One is a stale reference the tool should forget; the other is a tool
        // bug.
        let mut service = WidgetInspectorService::new();
        let id = service.to_id(Some("widget-a"), "tree").unwrap();

        assert_eq!(
            service.dispose_id(Some("inspector-999"), "tree"),
            Err(InspectorError::IdDoesNotExist)
        );
        assert_eq!(
            service.dispose_id(Some(&id), "some-other-group"),
            Err(InspectorError::IdNotInGroup)
        );
        assert_eq!(service.dispose_id(Some(&id), "tree"), Ok(()));
        assert_eq!(service.reference_count(&id), None);
    }

    #[test]
    fn disposing_an_id_leaves_it_valid_while_another_group_holds_it() {
        let mut service = WidgetInspectorService::new();
        let id = service.to_id(Some("widget-a"), "tree").unwrap();
        service.to_id(Some("widget-a"), "details").unwrap();

        assert_eq!(service.dispose_id(Some(&id), "tree"), Ok(()));
        assert_eq!(service.to_object(Some(&id)), Ok(Some("widget-a")));
    }

    #[test]
    fn a_null_id_is_not_an_error_in_either_direction() {
        // Upstream returns null from toId and does nothing in disposeId, so a
        // caller need not check before every call.
        let mut service = WidgetInspectorService::new();
        assert_eq!(service.to_id(None, "tree"), None);
        assert_eq!(service.to_object(None), Ok(None));
        assert_eq!(service.dispose_id(None, "tree"), Ok(()));
    }

    #[test]
    fn a_collected_referent_reads_as_gone_while_its_id_is_still_there() {
        // Which is the state a tool sees as "that widget no longer exists":
        // the id resolves, and resolves to nothing.
        let mut reference = InspectorReferenceData::new("widget-a", "inspector-0");
        assert_eq!(reference.value(), Some("widget-a"));
        reference.collect();
        assert_eq!(reference.value(), None);
    }

    #[test]
    fn a_primitive_is_held_strongly_because_a_weak_reference_refuses_it() {
        // The consequence is worth stating: a number in the inspector's table
        // is kept alive by the inspector, where a widget is not.
        let mut number = InspectorReferenceData::new("42", "inspector-0");
        assert!(number.is_strongly_held());
        number.collect();
        assert_eq!(number.value(), Some("42"), "nothing to collect");

        let mut boolean = InspectorReferenceData::new("true", "inspector-1");
        assert!(boolean.is_strongly_held());

        let widget = InspectorReferenceData::new("widget-a", "inspector-2");
        assert!(!widget.is_strongly_held());
    }

    #[test]
    fn disposing_everything_leaves_no_group_behind() {
        let mut service = WidgetInspectorService::new();
        service.to_id(Some("a"), "one");
        service.to_id(Some("b"), "two");
        assert_eq!(service.group_names(), vec!["one", "two"]);

        service.dispose_all_groups();
        assert!(service.group_names().is_empty());
        assert_eq!(service.reference_count("inspector-0"), None);
    }

    #[test]
    fn an_element_is_reported_by_the_widget_that_configured_it() {
        // The reader asked where this came from, and the widget's class is the
        // answer they meant; the element's class is a framework detail they
        // did not write.
        let mut service = WidgetInspectorService::new();
        let id = service.to_id(Some("Element#1"), "tree").unwrap();
        let resolved = service
            .to_object_for_source_location(&id, |value| {
                value
                    .strip_prefix("Element#")
                    .map(|n| format!("MyWidget#{n}"))
            })
            .unwrap();
        assert_eq!(resolved.as_deref(), Some("MyWidget#1"));
    }

    // -- Whose widget is it ------------------------------------------------

    #[test]
    fn with_no_roots_configured_local_means_not_the_framework() {
        // A guess, and upstream says so with a TODO. It is the right guess:
        // the reader wants everything that is not the framework, and saying
        // what *is* theirs needs a build system to answer.
        let mut service = WidgetInspectorService::new();
        assert!(service.is_local_creation_location("file:///home/me/app/lib/main.dart"));
        assert!(
            !service.is_local_creation_location(
                "file:///sdk/packages/flutter/lib/src/widgets/basic.dart"
            )
        );
    }

    #[test]
    fn a_configured_root_is_a_prefix_test_and_nothing_else() {
        let mut service = WidgetInspectorService::new();
        service.add_pub_root_directories(&["file:///home/me/app/"]);
        assert!(service.is_local_creation_location("file:///home/me/app/lib/main.dart"));
        assert!(
            !service.is_local_creation_location("file:///home/me/other/lib/main.dart"),
            "and the framework guess no longer applies at all"
        );
        assert!(!service.is_local_creation_location("file:///home/me/app.dart"));
    }

    #[test]
    fn changing_the_roots_throws_away_what_was_cached_under_the_old_ones() {
        // The cache is memoising a question whose answer just changed.
        let mut service = WidgetInspectorService::new();
        let path = "file:///home/me/app/lib/main.dart";
        assert!(service.is_local_creation_location(path));

        service.add_pub_root_directories(&["file:///elsewhere/"]);
        assert!(
            !service.is_local_creation_location(path),
            "the cached yes would have been wrong"
        );

        service.reset_pub_root_directories();
        assert!(service.is_local_creation_location(path));
    }

    #[test]
    fn every_branch_but_the_last_includes_the_node() {
        // A filter that guesses wrong should hide nothing.
        let mut service = WidgetInspectorService::new();
        let framework = Some("file:///sdk/packages/flutter/lib/src/widgets/basic.dart");

        assert!(
            service.should_show_in_summary_tree(DiagnosticLevel::Error, true, true, framework),
            "an error always shows"
        );
        assert!(
            service.should_show_in_summary_tree(DiagnosticLevel::Info, false, true, framework),
            "so does anything that is not diagnosable"
        );
        assert!(
            service.should_show_in_summary_tree(DiagnosticLevel::Info, true, false, framework),
            "and anything that is not an element"
        );
        assert!(
            !service.should_show_in_summary_tree(DiagnosticLevel::Info, true, true, framework),
            "only a framework element is filtered out"
        );
    }

    #[test]
    fn without_creation_tracking_the_summary_tree_shows_everything() {
        // There is no way to tell whose widget it is, so hiding any of them
        // would be guessing.
        let mut service = WidgetInspectorService::new();
        service.widget_creation_tracked = false;
        let framework = Some("file:///sdk/packages/flutter/lib/src/widgets/basic.dart");
        assert!(service.should_show_in_summary_tree(DiagnosticLevel::Info, true, true, framework));
    }

    // -- The selection -----------------------------------------------------

    #[test]
    fn a_selection_whose_render_object_left_the_tree_stops_being_one() {
        let mut selection = InspectorSelection::new();
        selection.set_candidates(vec![7, 8, 9]);
        assert_eq!(selection.current(), Some(7));
        assert!(selection.active());

        selection.set_attached(false);
        assert_eq!(
            selection.current(),
            None,
            "the field still holds it, but it is not a selection"
        );
        assert!(!selection.active());
    }

    #[test]
    fn stepping_through_the_candidates_is_what_the_index_is_for() {
        // Several widgets are under one finger; the reader walks outwards.
        let mut selection = InspectorSelection::new();
        selection.set_candidates(vec![7, 8, 9]);
        selection.set_index(2);
        assert_eq!(selection.current(), Some(9));

        selection.set_index(3);
        assert_eq!(selection.current(), None, "past the end selects nothing");
    }

    #[test]
    fn setting_the_candidates_resets_the_index() {
        let mut selection = InspectorSelection::new();
        selection.set_candidates(vec![7, 8, 9]);
        selection.set_index(2);
        selection.set_candidates(vec![1, 2]);
        assert_eq!(selection.index(), 0);
        assert_eq!(selection.current(), Some(1));
    }

    #[test]
    fn clearing_the_candidates_is_not_clearing_the_selection() {
        // The selection came from DevTools; the stale candidates are whatever
        // the reader last touched on screen, and drawing them around a widget
        // chosen in a different window highlights the wrong thing.
        let mut selection = InspectorSelection::new();
        selection.set_current(Some(42));
        selection.set_candidates(vec![7, 8]);
        selection.set_current(Some(42));

        selection.clear_candidates();
        assert!(selection.candidates().is_empty());
        assert_eq!(selection.current(), Some(42), "left alone");

        selection.clear();
        assert_eq!(selection.current(), None, "where clear takes it too");
    }

    #[test]
    fn clearing_candidates_that_are_already_empty_says_nothing() {
        let mut selection = InspectorSelection::new();
        selection.set_current(Some(42));
        let before = selection.notifications();
        selection.clear_candidates();
        assert_eq!(selection.notifications(), before);
    }

    #[test]
    fn selecting_what_is_already_selected_notifies_nobody() {
        // A tool polling the selection should not make every listener rebuild.
        let mut service = WidgetInspectorService::new();
        assert!(service.set_selection(Some(7)));
        assert!(!service.set_selection(Some(7)));
        assert!(service.set_selection(Some(8)));
    }

    #[test]
    fn a_selection_by_id_goes_through_the_same_table_as_everything_else() {
        let mut service = WidgetInspectorService::new();
        let id = service.to_id(Some("7"), "tree").unwrap();
        assert_eq!(service.set_selection_by_id(Some(&id)), Ok(true));
        assert_eq!(service.selection.current(), Some(7));

        assert_eq!(
            service.set_selection_by_id(Some("inspector-999")),
            Err(InspectorError::IdDoesNotExist)
        );
    }

    #[test]
    fn resetting_takes_the_groups_the_selection_and_the_roots() {
        // A tool reconnecting should not inherit the last one's idea of what
        // was selected.
        let mut service = WidgetInspectorService::new();
        service.to_id(Some("a"), "tree");
        service.set_selection(Some(7));
        service.add_pub_root_directories(&["file:///home/me/app/"]);

        service.reset_all_state();
        assert!(service.group_names().is_empty());
        assert_eq!(service.selection.current(), None);
        assert_eq!(service.pub_root_directories(), None);
    }

    // -- The widget --------------------------------------------------------

    #[test]
    fn the_selection_is_committed_when_the_finger_lifts_and_not_before() {
        // The reader is allowed to move around the screen watching the
        // highlight follow before deciding.
        let mut inspector = WidgetInspector::new();
        inspector.handle_pointer_move((10.0, 10.0), vec![1, 2]);
        assert_eq!(inspector.selection.current(), Some(1));
        assert_eq!(inspector.last_pointer_location, Some((10.0, 10.0)));

        inspector.handle_pointer_move((80.0, 40.0), vec![5, 6]);
        assert_eq!(inspector.selection.current(), Some(5), "it followed");

        assert_eq!(inspector.handle_pointer_up(), Some(5));
        assert_eq!(inspector.last_pointer_location, None);
    }

    #[test]
    fn nothing_is_selected_while_select_mode_is_off() {
        let mut inspector = WidgetInspector::new();
        inspector.is_select_mode = false;
        inspector.handle_pointer_move((10.0, 10.0), vec![1, 2]);
        assert!(inspector.selection.candidates().is_empty());
        assert_eq!(inspector.handle_pointer_up(), None);
    }

    // -- The buttons and the delegate --------------------------------------

    struct TestButton {
        variant: InspectorButtonVariant,
        toggled_on: Option<bool>,
    }

    impl InspectorButton for TestButton {
        fn variant(&self) -> InspectorButtonVariant {
            self.variant
        }
        fn semantics_label(&self) -> &str {
            "Select widget mode"
        }
        fn icon(&self) -> IconData {
            IconData::new(0xe000)
        }
        fn toggled_on(&self) -> Option<bool> {
            self.toggled_on
        }
        fn foreground_color(&self) -> Color {
            Color(0xFFFF_FFFF)
        }
        fn background_color(&self) -> Color {
            Color(0xFF00_0000)
        }
    }

    #[test]
    fn an_icon_only_button_gives_the_icon_the_whole_button() {
        // There is no background for it to sit inside.
        let icon_only = TestButton {
            variant: InspectorButtonVariant::IconOnly,
            toggled_on: None,
        };
        assert_eq!(icon_only.icon_size_for_variant(), 32.0);

        let filled = TestButton {
            variant: InspectorButtonVariant::Filled,
            toggled_on: None,
        };
        assert_eq!(filled.icon_size_for_variant(), 18.0);

        let toggle = TestButton {
            variant: InspectorButtonVariant::Toggle,
            toggled_on: Some(true),
        };
        assert_eq!(toggle.icon_size_for_variant(), 18.0);
    }

    #[test]
    fn only_a_toggle_has_an_answer_to_whether_it_is_on() {
        // Upstream's named constructors leave it null for the other two rather
        // than defaulting it, so a filled button cannot be read as toggled off.
        let filled = TestButton {
            variant: InspectorButtonVariant::Filled,
            toggled_on: None,
        };
        assert_eq!(filled.toggled_on(), None);
    }

    #[test]
    fn a_serialisation_without_a_group_is_a_snapshot_nobody_can_click_on() {
        let plain = InspectorSerializationDelegate::new();
        assert!(!plain.is_interactive(), "no live ids go into the output");
        assert!(
            InspectorSerializationDelegate::new()
                .with_group_name("tree")
                .is_interactive()
        );
    }

    #[test]
    fn your_own_widgets_show_more_of_themselves_than_the_frameworks() {
        // The reader is debugging their code, not RenderFlex.
        let mut delegate = InspectorSerializationDelegate::new();
        delegate.note_created_by_local_project("MyWidget");
        assert_eq!(
            delegate.property_filter_level("MyWidget"),
            DiagnosticLevel::Fine
        );
        assert_eq!(
            delegate.property_filter_level("RenderFlex"),
            DiagnosticLevel::Info
        );
    }

    #[test]
    fn the_details_tree_holds_its_depth_until_it_reaches_something_you_wrote() {
        // Expanding one node should expand the whole run of framework widgets
        // under it, down to the next widget the reader actually wrote --
        // otherwise they click through six RenderObjectWidgets to see the next
        // thing of theirs.
        let delegate = InspectorSerializationDelegate::new().with_subtree_depth(1);
        assert_eq!(delegate.base.subtree_depth, 1);

        let through_framework = delegate.delegate_for_node(false);
        assert_eq!(
            through_framework.base.subtree_depth, 1,
            "depth held while passing a node that is not in the summary tree"
        );

        let at_yours = delegate.delegate_for_node(true);
        assert_eq!(
            at_yours.base.subtree_depth, 0,
            "and spent when one of yours is reached"
        );
    }

    #[test]
    fn a_summary_tree_spends_depth_every_level_because_every_level_is_yours() {
        let delegate = InspectorSerializationDelegate::new()
            .with_summary_tree(true)
            .with_subtree_depth(1);
        assert_eq!(delegate.delegate_for_node(false).base.subtree_depth, 0);
    }

    #[test]
    fn plenty_of_depth_left_is_spent_normally_whatever_the_node_is() {
        let delegate = InspectorSerializationDelegate::new().with_subtree_depth(3);
        assert_eq!(delegate.delegate_for_node(false).base.subtree_depth, 2);
    }

    #[test]
    fn a_deep_link_is_rendered_by_its_description_and_not_by_a_label() {
        // A label in front of a URL in an error dump is noise the reader has
        // to read past to reach the link.
        let property = DevToolsDeepLinkProperty::new(
            "To inspect this widget, open DevTools",
            "https://devtools/#/inspector?id=7",
        );
        assert_eq!(property.property.name.as_deref(), Some(""));
        assert_eq!(property.url(), Some("https://devtools/#/inspector?id=7"));
        assert_eq!(property.property.default_level, DiagnosticLevel::Info);
    }

    // -- WeakMap -----------------------------------------------------------

    #[test]
    fn a_weak_map_keeps_primitives_apart_from_objects() {
        // Not an optimisation: Dart's Expando refuses strings, numbers and
        // booleans as keys, so they need a map of their own.
        let mut map: WeakMap<String, u32> = WeakMap::new();
        map.insert("42".to_string(), 1, true);
        map.insert("widget-a".to_string(), 2, false);

        assert_eq!(map.get(&"42".to_string(), true), Some(&1));
        assert_eq!(
            map.get(&"42".to_string(), false),
            None,
            "and the two tables do not see each other"
        );
        assert_eq!(map.len(), 2);

        assert_eq!(map.remove(&"widget-a".to_string(), false), Some(2));
        assert_eq!(map.len(), 1);

        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn what_counts_as_a_primitive_is_what_a_weak_reference_refuses() {
        assert!(is_primitive_key("42"));
        assert!(is_primitive_key("3.5"));
        assert!(is_primitive_key("true"));
        assert!(is_primitive_key("false"));
        assert!(!is_primitive_key("widget-a"));
    }
}
