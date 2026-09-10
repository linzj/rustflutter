/// A member read or called through a `dynamic`, dispatched by who declares it.
///
/// `demo.slug` in the gallery's `Demos.asSlugToDemoMap` is this: the closure
/// `LinkedHashMap.fromIterable` takes declares its parameter `dynamic`, so
/// nothing at the call names a struct. The closed world does know who
/// declares `slug`, and that list is short.
///
/// What this pins down is the part a downcast chain can get wrong: the right
/// arm has to win when two classes declare the same name, a nullable field
/// has to come back as Dart's `null` rather than as an absent value, an
/// argument has to reach the method it was passed to, and an object that
/// declares nothing of the name has to raise `NoSuchMethodError` -- not
/// answer with the wrong class's member.
library;

class Demo {
  const Demo(this.slug, this.label);
  final String? slug;
  final String label;
  String describe(String prefix) => '$prefix/$label';
}

class Page {
  const Page(this.slug);
  final String? slug;
  String describe(String prefix) => 'page:$prefix';
}

class Bare {
  const Bare();
}

/// A member whose Dart return type is `dynamic`. Dart calls that type
/// nullable, but its value here is already the object a `dynamic` holds, so
/// nothing has to be done to it on the way out -- reading Dart's nullability
/// instead of the lowered type wrapped an `Rc<dyn DartAny>` in
/// `dart_option_object` and cost a stub (ws1055).
class Payload {
  const Payload(this.tag);
  final String tag;
  dynamic toJson() => tag == 'none' ? null : <String, String>{'tag': tag};
}

String read(dynamic thing) {
  final dynamic got = thing.slug;
  return got == null ? 'null' : '$got';
}

String call(dynamic thing) => '${thing.describe('x')}';

String encode(dynamic thing) {
  final dynamic json = thing.toJson();
  return json == null ? 'null' : '$json';
}

String use() {
  final List<String> out = <String>[];
  out.add(read(const Demo('a', 'A')));
  out.add(read(const Page('b')));
  out.add(read(const Demo(null, 'C')));
  out.add(call(const Demo('a', 'A')));
  out.add(call(const Page('b')));
  out.add(encode(const Payload('p')));
  out.add(encode(const Payload('none')));
  // ..and an object that declares neither name.
  try {
    out.add(read(const Bare()));
  } on NoSuchMethodError catch (_) {
    out.add('nosuch');
  }
  return out.join('|');
}
