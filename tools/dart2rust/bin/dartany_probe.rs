// work.md v2 step 0: can the *handle type* become `Rc<dyn DartAny>`?
//
// The v1 plan moved the four protocol methods onto `Object` and deleted the
// blanket `impl<T: 'static> Object for T`. That hit a coherence cliff and
// was withdrawn (ba3615b0). v2 leaves the blanket alone and changes what
// translated code holds instead: `Rc<dyn Object>` -> `Rc<dyn DartAny>`.
// `DartAny: Object + 'static`, so such a handle answers the whole protocol
// from its own vtable and no table is consulted.
//
// Three claims, each written so rustc answers. Everything between the
// CONTROL-CUT markers is removed for the control run, which must fail --
// otherwise this proves only that something compiles.

/// Stands for a translated trait. Every one has `DartAny` as a supertrait.
///
/// The control cuts *only* the supertrait, so the trait still exists and the
/// four functions below still name a real type: what breaks is exactly the
/// claim being made. Cutting the whole declaration would only prove that a
/// missing name is an error.
pub trait ProbeWidget:
    // CONTROL-CUT-BEGIN
    DartAny +
    // CONTROL-CUT-END
    std::fmt::Debug
{
}

/// Claim 1: `dyn DartAny` is object safe -- it can be spelled as a type at
/// all. (`dart_cast_to`/`dart_cast_any` are generic, but they live on
/// `DartCastExt`, not on `DartAny`, so they do not poison it.)
pub fn probe_object_safe(x: &dyn DartAny) -> String {
    x.dart_to_string()
}

/// Claim 2: a translated trait's handle upcasts to it, which is what every
/// `Object`-typed slot in generated code would become.
pub fn probe_upcast(x: std::rc::Rc<dyn ProbeWidget>) -> std::rc::Rc<dyn DartAny> {
    x
}

/// Claim 3: the closure case, which is the one thing v1 died on. A bare
/// `Function` slot holds a closure; put behind its own function handle
/// first, that handle is `DartAny` (`dart_any_fn!`) and `Rc<T>` forwards,
/// so it reaches the same slot without any blanket being involved.
pub fn probe_closure_handle(f: std::rc::Rc<dyn Fn(i64) -> i64>) -> std::rc::Rc<dyn DartAny> {
    std::rc::Rc::new(f)
}

/// Claim 4: the protocol really is answered from the handle, with no table
/// in the path -- these are the four calls `dart_any_eq`/`dart_any_hash`
/// reach by hashing a `TypeId`.
pub fn probe_protocol(a: &dyn DartAny, b: &dyn DartAny) -> (bool, i64, String) {
    (
        a.dart_eq_any(b.dart_any_ref()),
        a.dart_hash_any(),
        a.dart_to_string(),
    )
}
