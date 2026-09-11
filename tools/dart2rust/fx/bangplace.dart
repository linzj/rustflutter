/// `x!` on the *left* of an assignment throws when `x` is null.
///
/// `xs!.add(1)` and `m[k]!.add(1)` are Dart's null check operator in a place:
/// a null there throws `TypeError`, and `try { .. } catch (e)` around it is
/// ordinary Dart. This compiler lowered the place to `.as_mut().unwrap()` and
/// `.get_mut(&k).unwrap()`, which are *panics*: the program lost a path it
/// still had.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Bag {
  List<int>? xs;
  Map<String, List<int>> m = <String, List<int>>{};
}

String field() {
  final b = Bag();
  final out = <String>[];
  try {
    b.xs!.add(1);
    out.add('added');
  } catch (_) {
    out.add('xs-null');
  }
  b.xs = <int>[];
  b.xs!.add(2);
  out.add('${b.xs}');
  return out.join(',');
}

String local() {
  List<int>? xs;
  final out = <String>[];
  try {
    xs!.add(1);
    out.add('added');
  } catch (_) {
    out.add('local-null');
  }
  xs = <int>[];
  xs!.add(3);
  out.add('$xs');
  return out.join(',');
}

String indexed() {
  final b = Bag();
  final out = <String>[];
  try {
    b.m['a']!.add(1);
    out.add('added');
  } catch (_) {
    out.add('key-absent');
  }
  b.m['a'] = <int>[];
  b.m['a']!.add(4);
  out.add('${b.m}');
  return out.join(',');
}

String use() {
  final out = <String>[];
  out.add(field());
  out.add(local());
  out.add(indexed());
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
