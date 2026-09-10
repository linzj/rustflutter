// `dart:io`'s two exceptions, which the prelude did not have.
//
// `IOClient.send` catches both and tests for one of them:
//
//     } on SocketException catch (error) { throw _ClientSocketException(..); }
//       on HttpException catch (error) { throw ClientException(error.message,
//                                                              error.uri); }
//
// and `err is HttpException` on the response stream's errors. `is` against a
// class the prelude does not have is refused ("`is` against `HttpException`,
// which was not translated"), and that refusal took the whole of
// `IOClient.send` with it.
//
// `SocketException` was already a struct here; it had never been registered
// for `is`. `HttpException` is new, with the two members the uses read.
//
// What this fixture pins is that the two are told apart -- from each other,
// from an unrelated exception, and by `catch`'s own type test -- and that
// the members read back what was thrown.

import 'dart:io';

String describe(Object thrown) {
  try {
    throw thrown;
  } on HttpException catch (e) {
    return 'http/${e.message}/${e.uri}';
  } on SocketException catch (e) {
    return 'socket/${e.message}';
  } on FormatException catch (e) {
    return 'format/${e.message}';
  }
}

String use() {
  final List<String> out = <String>[];
  out.add(describe(const HttpException('gone')));
  out.add(describe(HttpException('moved', uri: Uri.parse('http://x/y'))));
  out.add(describe(const SocketException('refused')));
  out.add(describe(const FormatException('bad')));
  // ..and the `is` test the client makes on a stream error.
  final Object err = const HttpException('e');
  out.add('${err is HttpException}/${err is SocketException}');
  return out.join('|');
}
