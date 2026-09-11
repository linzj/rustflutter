/// A subclass that does not override a member should not be given a copy of
/// it.
///
/// `/tmp/work_size.md`'s A asks for exactly this shape: a base with a getter,
/// three subclasses that inherit it and a fourth that overrides. The four
/// answers have to agree with Dart's, and the *shape* of what comes out is
/// what the size plan is about -- one body, not four.
library;

class Base {
  String get label => 'base';
  String describe() => 'I am ${label}';
}

class First extends Base {}

class Second extends Base {}

class Third extends Base {}

class Fourth extends Base {
  @override
  String get label => 'fourth';
}

String use() {
  final out = <String>[];
  for (final Base b in <Base>[Base(), First(), Second(), Third(), Fourth()]) {
    out.add(b.describe());
  }
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
