// A local declared *and* assigned inside a closure is `mut` by that
// closure's own reckoning.
//
// **This fixture does not reproduce ws1022's failure and is kept as a
// regression net only.** Two shapes were tried -- the closure returned from
// a function, and the closure in a constructor's field initialiser -- and
// both compile with or without the rule, because the enclosing member's
// `_assignedIn` already mentions the local (`_WalkSelf` descends into
// closures). The gallery's failing case had a nearly empty enclosing set
// (`set=1`, traced), which comes from an emission context that clears it --
// `emit_impl` does exactly that around a lazy field's accessor. Landing on
// that from a fixture was not worth a third guess; the rule's warrant is the
// chain's stub diff (`material_dialog.rs new`, 62 -> 61).
//
// `_closure` computed `_assignedIn(node.body)` all along and spent it only
// on the closure's parameters and captures, so a local declared inside the
// body was judged by whatever set the enclosing member happened to leave
// behind. `_DialogRoute`'s `pageBuilder` writes
//
//     Widget dialog = themes?.wrap(pageChild) ?? pageChild;
//     if (useSafeArea) { dialog = SafeArea(child: dialog); }
//
// and the second line would not compile: `let dialog` without `mut`.
//
// Unioned rather than replaced, because the closure still writes the locals
// it captured and those were decided outside -- `wrap` and `tag` here.

typedef Build = String Function(String);

// The closure lives in a *constructor's field initialiser*, which is where
// the gallery's does (`_DialogRoute(.., pageBuilder: (..) { .. })`). That is
// what makes the enclosing set the constructor's rather than a function
// body's -- the traced set had one element in it and did not mention the
// closure's own local.
class Sheet {
  Sheet(bool wrap, String tag)
    : build = ((String child) {
        // Declared and assigned inside the closure: the case that failed.
        String out = '[$child]';
        if (wrap) {
          out = '<$out>';
        }
        return '$tag$out';
      });

  final Build build;
}

String use() {
  final Sheet plain = Sheet(false, 'p');
  final Sheet wrapped = Sheet(true, 'w');
  return '${plain.build("a")}/${wrapped.build("c")}';
}
