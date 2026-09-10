// A type parameter whose *bound* is nullable, promoted by a null check.
// `T extends String?` is spelled at its bound (`_spelledAsBound`), so `T`
// is an `Option<String>`; after `if (input == null || ..) return input;`
// Dart promotes `input` to a non-null `T`, and dropping the parameter's own
// nullability is not enough -- the bound's has to go too, or the promoted
// read is still an `Option<String>` and `substring` is not on one.
//
// intl's `toBeginningOfSentenceCase` is written exactly this way:
//
//     T toBeginningOfSentenceCase<T extends String?>(T input, [String? l]) {
//       if (input == null || input.isEmpty) return input;
//       return '${_upperCaseLetter(input[0], l)}${input.substring(1)}' as T;
//     }
String upperFirst(String input, String? locale) =>
    locale == 'tr' ? input.toUpperCase() : input.toUpperCase();

T sentence<T extends String?>(T input, [String? locale]) {
  if (input == null || input.isEmpty) {
    return input;
  }
  return '${upperFirst(input[0], locale)}${input.substring(1)}' as T;
}

String use() {
  final String? none = sentence<String?>(null);
  // Only nullable instantiations: `sentence<String>('hello')` does not
  // compile, and the reason is a *different* defect from the one this
  // fixture is about -- a nullable bound is spelled `Option<String>` in the
  // signature whatever `T` is, so a non-nullable instantiation cannot be
  // called at all. Written down under 已知欠账 rather than left here, where
  // it would hide the promotion this is the acceptance test for.
  return '$none/${sentence<String?>('')}'
      '/${sentence<String?>('world', 'tr')}';
}
