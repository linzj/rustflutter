// work.md step 0: is the vtable route reachable at all?
//
// Appended to a copy of the prelude by `bin/vtable_probe.py` and compiled.
// Two claims the "registry -> vtable" plan rests on, written so that `rustc`
// answers them instead of a comment. That matters here specifically: two
// prelude comments about this are already stale -- `prelude.dart:393` claims
// a per-trait `impl Object for dyn X` that the generated code has zero of,
// and `:339` contradicts `:639` about whether a trait object can unsize to
// `dyn Object`. Reading is not evidence.
//
// The probe compiles and a control that drops the `DartAny` supertrait must
// fail, or the probe is proving nothing about *why* it works.

/// Stands for a translated trait: every one of them has `DartAny` as a
/// supertrait (`pub trait Key: DartAny + std::fmt::Debug`,
/// `StatelessWidget: DartAny + Debug + Widget`).
/// The control cuts *only* the supertrait, so the trait still exists and the
/// functions below still name a real type: what breaks is the claim itself.
/// Cutting the whole declaration would only prove a missing name is an error.
pub trait ProbeKey:
    // CONTROL-CUT-BEGIN
    DartAny +
    // CONTROL-CUT-END
    std::fmt::Debug
{
}

/// Claim 1: a `&dyn Trait` answers the protocol from its *own* vtable, with
/// no `TypeId` hash and no table probe anywhere.
pub fn probe_vtable_eq(a: &dyn ProbeKey, b: &dyn ProbeKey) -> bool {
    a.dart_eq_any(b.dart_any_ref())
}

pub fn probe_vtable_hash(a: &dyn ProbeKey) -> i64 {
    a.dart_hash_any()
}

pub fn probe_vtable_to_string(a: &dyn ProbeKey) -> String {
    a.dart_to_string()
}

/// Claim 2: `Rc<dyn Trait>` upcasts to `Rc<dyn Object>` (trait upcasting,
/// stable since 1.86; this toolchain is 1.98).
pub fn probe_upcast(x: std::rc::Rc<dyn ProbeKey>) -> std::rc::Rc<dyn Object> {
    x
}
