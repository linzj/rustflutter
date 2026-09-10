/// A class's **static setter**, called from somewhere else.
///
/// The store builds the call as `set_` plus the property's name (see the
/// front end's `StaticSet`), while the declaration carries the property's
/// own name and a setter flag. The backend's "was it translated" check
/// compared the two spellings directly, so `ServicesBinding
/// .set_systemContextMenuClient` was refused as untranslated while the
/// function it names was emitted three modules along -- three refusals in
/// `SystemContextMenuController`, whose constructor is one of these stores.
library;

class Holder {
  static String _value = 'none';

  static set current(String value) {
    _value = value;
  }

  static String get current {
    return _value;
  }
}

class User {
  User(String tag) {
    Holder.current = 'from-$tag';
  }
}

String use() {
  Holder.current = 'first';
  final String a = Holder.current;
  User('ctor');
  final String b = Holder.current;
  return '$a/$b';
}
