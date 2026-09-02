// The other half of `constinstance.dart`: a `const` instance the constructor
// *can* rebuild.
//
// No `// DIFFERS:` here, and that is the whole point. The analyzer front end
// sees `const Direct(3.0, 5.0)` and writes `Direct::new(3.0, 5.0)`; the Kernel
// front end sees the evaluated instance and has to arrive at the same text.
// Falling back to a struct literal would be just as correct by value -- which
// is why the values in the other fixture cannot catch it -- and would make the
// two front ends disagree here, which is what does.

class Direct {
  const Direct(this.width, this.height);

  final double width;
  final double height;

  static const Direct small = Direct(3.0, 5.0);
  static const Direct large = Direct(11.0, 19.0);

  double area() => width * height;
}
