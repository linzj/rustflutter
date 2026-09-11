/// A local function declared inside a constructor's **initializer list**,
/// then handed on as a value.
///
/// `lends` (`frontend_kernel/statements.dart`) asks whether the member's
/// body ever reads the binding as a value: one that is only *called* may
/// borrow, and is bound as a bare Rust closure; one that escapes is the
/// `Rc<dyn Fn>` every Dart function value is. The scan looked at
/// `function.body` alone -- and a constructor's initializer list is not in
/// it. `TextFormField`'s `onChangedHandler` is written inside the
/// `builder:` closure of a super-initializer and handed to
/// `TextField.onChanged`; the scan saw no read at all, lent the binding,
/// and the tear-off did not compile (1 stub at ws1098).
///
/// The two shapes below are the same declaration in the two places, so
/// AGREE here means the answer does not depend on which one it was
/// written in.
library;

String apply(String Function(String) f, String v) => f(v);

class Base {
  Base({required this.builder});

  final String Function(String) builder;

  String run(String s) => builder(s);
}

/// The local function lives in a closure inside the *initializer list*.
class FromInitializer extends Base {
  FromInitializer(String tag)
    : super(
        builder: (String value) {
          String shout(String v) => '$v!$tag';
          // Called *and* handed on: the tear-off is what needs the handle.
          return '${shout(value)}/${apply(shout, value)}';
        },
      );
}

/// The same declaration, in a body, where the scan always looked.
class FromBody extends Base {
  FromBody(this.tag) : super(builder: _never);

  static String _never(String v) => v;

  final String tag;

  @override
  String run(String value) {
    String shout(String v) => '$v!$tag';
    return '${shout(value)}/${apply(shout, value)}';
  }
}

String use() => '${FromInitializer('i').run('a')}|${FromBody('b').run('a')}';
