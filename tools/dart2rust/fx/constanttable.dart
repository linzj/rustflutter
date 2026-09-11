/// A list that is really a table becomes a table and a loop.
///
/// `/tmp/work_size.md`'s C: the gallery's code viewer is 126 functions
/// holding 13.8 MB, every one of them the same constructor per line of the
/// source it shows. The rule is a *shape* -- at least sixteen elements, the
/// same constructor, and each argument either constant, a string literal, or
/// one of a few choices -- and never a name.
///
/// Two halves here: twenty calls that fit the shape, and twenty that do not
/// because an argument is a *call* whose text differs every time. The second
/// must come out as it always did.
library;

class Piece {
  Piece(this.text, this.style, this.flag);

  final String text;
  final String style;
  final bool flag;

  @override
  String toString() => '$text:$style:${flag ? 1 : 0}';
}

String grow(String s) => '$s!';

List<Piece> table(String head, String body, String tail) => <Piece>[
  Piece('alpha', head, true),
  Piece('beta', body, false),
  Piece('gamma', tail, true),
  Piece('delta', head, false),
  Piece('epsilon', body, true),
  Piece('zeta', tail, false),
  Piece('eta', head, true),
  Piece('theta', body, false),
  Piece('iota', tail, true),
  Piece('kappa', head, false),
  Piece('lambda', body, true),
  Piece('mu', tail, false),
  Piece('nu', head, true),
  Piece('xi', body, false),
  Piece('omicron', tail, true),
  Piece('pi', head, false),
  Piece('rho', body, true),
  Piece('sigma', tail, false),
  Piece('tau', head, true),
  Piece('upsilon', body, false),
];

List<Piece> notTable(String head) => <Piece>[
  Piece(grow('alpha'), head, true),
  Piece(grow('beta'), head, true),
  Piece(grow('gamma'), head, true),
  Piece(grow('delta'), head, true),
  Piece(grow('epsilon'), head, true),
  Piece(grow('zeta'), head, true),
  Piece(grow('eta'), head, true),
  Piece(grow('theta'), head, true),
  Piece(grow('iota'), head, true),
  Piece(grow('kappa'), head, true),
  Piece(grow('lambda'), head, true),
  Piece(grow('mu'), head, true),
  Piece(grow('nu'), head, true),
  Piece(grow('xi'), head, true),
  Piece(grow('omicron'), head, true),
  Piece(grow('pi'), head, true),
  Piece(grow('rho'), head, true),
  Piece(grow('sigma'), head, true),
  Piece(grow('tau'), head, true),
  Piece(grow('upsilon'), head, true),
];

String use() {
  final out = <String>[];
  out.add(table('H', 'B', 'T').map((Piece p) => '$p').join(','));
  out.add(notTable('H').length.toString());
  out.add(notTable('H').first.toString());
  out.add(table('x', 'y', 'z').last.toString());
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
