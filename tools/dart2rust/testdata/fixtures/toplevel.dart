// A fixture for top-level constants.
//
// Dart has module-level names and so does Rust, so a `const` at the top of a
// file needs no owner on either side. Analyzer models one as a *synthetic*
// getter -- the same distinction that separates a field from a real getter --
// so a computed `get foo => ...` is not a constant and is refused.
//
// The computed getter below is here to be refused, and there is a test that
// checks it was.

const double kSpacing = 8.0;
const int kMaxItems = 10;
const bool kVerbose = false;

/// A `final`, not a `const`: still a module constant.
final double kDerived = kSpacing * 2.0;

/// Computed, not stored. Not a constant, and must not be emitted as one.
double get computed => kSpacing + 1.0;

class Layout {
  const Layout(this.count);

  final int count;

  double totalSpacing() {
    return kSpacing * 3.0;
  }

  /// Reads two different top-level constants, so one standing in for the other
  /// would show up.
  bool isFull() {
    return count >= kMaxItems;
  }

  double derived() {
    return kDerived;
  }

  /// Reads the computed getter, which is not a constant. This method must be
  /// refused -- without a reference, the refusal path is never taken and a
  /// mutation removing it would survive.
  double usesComputed() {
    return computed;
  }
}
