// A local function's omitted optional arguments get their declared
// defaults, as a method call's do.
//
// The local-call path built its arguments from the *invocation*'s function
// type, which says nothing about a parameter the call left out. Flutter's
// `animations` package writes
//
//     void takeMeasurementsInSourceRoute([Duration? _]) { .. }
//     ..
//     if (delay) { binding.addPostFrameCallback(takeMeasurementsInSourceRoute); }
//     else       { takeMeasurementsInSourceRoute(); }
//
// and the second branch came out as a no-argument call against a
// one-parameter closure. `_instanceInvocation` had already grown the same
// fix for methods, and says so in its own comment.
//
// Both call shapes are here: the one that omits the optional, and the one
// that supplies it -- so the rule cannot be "always consult the
// declaration", which would drop a supplied argument.

String use() {
  final List<String> log = <String>[];

  void measure([String? tag]) {
    log.add(tag ?? 'none');
  }

  // Omitted: the default has to be filled in.
  measure();
  // ..and supplied, which already worked and must keep working.
  measure('given');

  // A second one with two optionals, to pin partial application.
  String pair([int a = 1, int b = 2]) => '$a-$b';

  return '${log.join(",")}/${pair()}/${pair(7)}/${pair(7, 9)}';
}
