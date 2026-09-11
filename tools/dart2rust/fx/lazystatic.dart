/// A lazy static whose initialiser throws is *not* initialised, and the next
/// read runs it again.
///
/// Dart's rule, measured against the VM before this fixture was written: an
/// initialiser that throws twice and then succeeds is entered three times,
/// and only the value is remembered. That rules out
/// `LazyLock<Result<T, DartError>>`, which would hand back the same `Err`
/// for ever -- the prelude's `DartLazy` is what this pins.
///
/// The rule this fixture belongs to: **a panic is never a pass.** The
/// initialiser used to `.unwrap()` everything it called, so a `throw` inside
/// one killed the process where Dart hands the error to whoever read it.
library;

int _slotCalls = 0;
int _topCalls = 0;

List<int> _buildSlot() {
  _slotCalls++;
  if (_slotCalls < 3) throw StateError('slot not yet ($_slotCalls)');
  return <int>[_slotCalls];
}

List<int> _buildTop() {
  _topCalls++;
  if (_topCalls < 3) throw StateError('top not yet ($_topCalls)');
  return <int>[_topCalls];
}

class Holder {
  static final List<int> slot = _buildSlot();
}

final List<int> top = _buildTop();

String readSlot() {
  final out = <String>[];
  for (var i = 0; i < 4; i++) {
    try {
      out.add('${Holder.slot}');
    } catch (e) {
      out.add('$e');
    }
  }
  out.add('calls=$_slotCalls');
  return out.join(',');
}

String readTop() {
  final out = <String>[];
  for (var i = 0; i < 4; i++) {
    try {
      out.add('$top');
    } catch (e) {
      out.add('$e');
    }
  }
  out.add('calls=$_topCalls');
  return out.join(',');
}

String use() {
  final out = <String>[];
  out.add(readSlot());
  out.add(readTop());
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
