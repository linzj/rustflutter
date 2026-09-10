/// An indexed read out of a field that lives in a cell.
///
/// A mutable field is held in a cell here, so reading `_list[i]` borrows the
/// cell, indexes it and clones the element. The clone is the value; the
/// borrow is not meant to outlive it. But a block's tail temporaries live to
/// the end of the *enclosing statement*, and where the cell handle is a local
/// of a shorter scope -- a closure's prologue clones each field it carries --
/// the borrow outlived the thing it borrowed.
///
/// `AppStateModel.subtotalCost` in the Shrine study is the shape: a `fold`
/// whose closure reads `_availableProducts[id]` and `_productsInCart[id]` in
/// one expression. The map read beside it has bound its borrow all along.
///
/// What this pins down is that the read still sees writes -- binding the
/// borrow must not turn it into a snapshot taken somewhere else.
library;

class Shelf {
  Shelf() : _rows = <int>[1, 2, 3], _byName = <String, int>{'a': 10, 'b': 20};

  List<int> _rows;
  Map<String, int> _byName;

  void bump(int at) {
    _rows[at] = _rows[at] + 100;
  }

  void add(int row) {
    _rows = <int>[..._rows, row];
  }

  /// Both reads in one expression, from a closure that carries the fields.
  int total(List<String> names) {
    return names.fold<int>(0, (int sum, String name) {
      return sum + _rows[_byName[name]! ~/ 10 - 1] + _byName[name]!;
    });
  }

  String get rows => _rows.join(',');
}

String use() {
  final List<String> out = <String>[];
  final Shelf s = Shelf();
  out.add('${s.total(<String>['a', 'b'])}');
  s.bump(0);
  out.add('${s.total(<String>['a', 'b'])}');
  s.add(4);
  out.add(s.rows);
  out.add('${s.total(<String>['b'])}');
  return out.join('|');
}
