/// A list *literal* of a subclass, written into a slot typed by the base.
///
/// `List` into `List` was the value as it stands -- right for a list that
/// is a reference someone else holds, and wrong for a fresh literal, whose
/// Rust element type is fixed by what was written. Rust unsizes the handle,
/// not the `Vec` around it, so `<TextSpan>[..]` into a `List<InlineSpan>?`
/// read:
///
///     expected `Vec<Rc<dyn InlineSpan>>`, found `Vec<Rc<TextSpan>>`
///
/// `TextEditingController.buildTextSpan` is the shape (1 stub at ws1099).
///
/// The nullable slot and the plain one are both here, because the `Option`
/// layer goes on after the elements are mapped, not before.
library;

abstract class Span {
  String get label;
}

class Word implements Span {
  Word(this.label);

  @override
  final String label;
}

class Gap implements Span {
  @override
  String get label => '_';
}

class Line {
  Line({this.children, List<Span>? extra}) : extra = extra ?? const <Span>[];

  final List<Span>? children;
  final List<Span> extra;

  String show() =>
      '${(children ?? const <Span>[]).map((s) => s.label).join()}'
      '/${extra.map((s) => s.label).join()}';
}

String use() {
  final out = <String>[];
  // A literal of one subclass into a nullable base-typed slot.
  out.add(Line(children: <Word>[Word('a'), Word('b')]).show());
  // ..and into the plain one beside it.
  out.add(Line(extra: <Gap>[Gap(), Gap()]).show());
  // A literal already of the base type still goes as it is.
  out.add(Line(children: <Span>[Word('c'), Gap()]).show());
  // An empty one, which has no element to map.
  out.add(Line(children: <Word>[]).show());
  return out.join('|');
}
