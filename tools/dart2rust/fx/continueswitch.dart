// Dart's `continue <label>;` inside a `switch`: control leaves this case and
// runs *another* one. Rust's `match` runs one arm and is done.
//
// The switch becomes a labelled `loop` whose arms are numbered, the number is
// a `let mut` the loop matches on, and `continue` sets the number and goes
// round again. `LicenseEntryWithLineBreaks.paragraphs` is written as a state
// machine this way and was refused whole.
enum Phase { start, middle, end }

String use() {
  final steps = <String>[];
  for (final Phase from in Phase.values) {
    var hops = 0;
    Phase state = from;
    // A second switch, to be sure the loop labels do not collide.
    switch (state) {
      case Phase.start:
        steps.add('s');
        continue mid;
      mid:
      case Phase.middle:
        steps.add('m');
        hops += 1;
        if (hops < 2) {
          continue done;
        }
        steps.add('again');
      done:
      case Phase.end:
        steps.add('e');
    }
    switch (from) {
      case Phase.start:
        steps.add('|a');
      case Phase.middle:
        steps.add('|b');
      case Phase.end:
        steps.add('|c');
    }
  }
  return steps.join(',');
}
