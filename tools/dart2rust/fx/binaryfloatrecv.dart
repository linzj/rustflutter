// A float receiver that is *arithmetic over literals*. Rust resolves a
// method before it defaults an unsuffixed float, so `(1.5 * 0.35).sin()` is
// every bit as unpinned as `1.5.sin()` -- nothing in it says which float it
// is ("can't call method `sin` on ambiguous numeric type `{float}`",
// E0689). ws956 taught the rule to see through a negation; this is the
// other spelling.
//
// `InkSparkle._updateFragmentShader` computes
// `1.5 + turbulencePhase * -0.0066 * math.sin(1.5 * 0.35)` and lands on it.
import 'dart:math' as math;

String use() {
  final double phase = 2.0;
  // Every operand a literal: nothing pins the type.
  final double a = math.sin(1.5 * 0.35);
  final double b = math.cos(0.5 + 0.25);
  // With a typed operand in it, inference already had it: kept so the
  // fixture says both are covered.
  final double c = math.sin(phase * 0.35);
  return '${a.toStringAsFixed(3)}/${b.toStringAsFixed(3)}'
      '/${c.toStringAsFixed(3)}';
}
