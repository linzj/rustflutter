// `lib/coerce.dart`'s type algebra.
//
// These are the pure functions the whole compiler decides coercions by --
// `sameRust` is asked whether two types need an adapter between them, and
// answers for every argument, return and field in the gallery. They had no
// test of any kind. The cases here are the ones the rules are *about*: the
// normalisations (`Iterable` is `List`, `num` is `double`, `dynamic` is
// `Object`), the two spellings of an `Option` layer, and `void?`, which is
// the one type that is not nullable however it is spelled.
library;

import '../lib/coerce.dart';
import '../lib/ir.dart';
import 'check.dart';

const _int = IrType('int');
const _double = IrType('double');
const _num = IrType('num');
const _string = IrType('String');
const _object = IrType('Object');

IrType _list(IrType of) => IrType('List', arguments: [of]);
IrType _nullable(IrType t) =>
    IrType(t.name, nullable: true, arguments: t.arguments);
IrType _option(IrType t) => IrType('Option', arguments: [t]);

void main() {
  group('rustScalar');
  // The one that the comment above the map in `backend_rust.dart` disagreed
  // with for months: `num` is `f64`, not `f32`.
  expect(rustScalar('num'), 'f64', 'num is a double-precision float');
  expect(rustScalar('double'), 'f64', 'so is double');
  expect(rustScalar('int'), 'i64', 'int is 64-bit');
  expect(rustScalar('bool'), 'bool', 'bool is itself');
  expect(rustScalar('String'), 'String', 'so is String');
  expect(rustScalar('Widget'), 'Widget', 'anything else is left alone');

  group('normalName');
  // Since ws908 an `Iterable` is a trait of its own (`Rc<dyn DartIterable<T>>`)
  // and no longer another spelling of `List`: the name stands.
  expect(normalName('Iterable'), 'Iterable', 'an Iterable is its own trait');
  expect(normalName('LinkedHashSet'), 'Set', "but dart:'s set aliases are Set");
  expect(normalName('dynamic'), 'Object', 'and dynamic is Object');
  expect(normalName('num'), 'double', 'and num is double');
  expect(normalName('Set'), 'Set', 'a Set is not a List');

  group('sameRust');
  expectTrue(sameRust(_int, _int), 'a type is itself');
  expectTrue(!sameRust(_int, _double), 'int and double are not the same Rust');
  expectTrue(sameRust(_num, _double), 'but num and double are');
  expectTrue(
    !sameRust(_list(_double), IrType('Iterable', arguments: [_num])),
    'a List is not an Iterable: `Vec<f64>` against `Rc<dyn DartIterable<f64>>`',
  );
  expectTrue(
    sameRust(const IrType('List'), _list(_string)),
    'a bare List is List<anything>: the backend has one spelling',
  );
  expectTrue(
    !sameRust(_list(_int), _list(_string)),
    'but two named element types must match',
  );
  expectTrue(
    !sameRust(_int, _nullable(_int)),
    'an Option layer is part of the type',
  );
  expectTrue(
    sameRust(_option(_int), _nullable(_int)),
    'and its two spellings are one type',
  );
  expectTrue(
    sameRust(_option(_nullable(_int)), _option(_nullable(_int))),
    'a doubled Option stays doubled -- Dart collapses it and Rust does not',
  );
  expectTrue(
    !sameRust(_option(_nullable(_int)), _nullable(_int)),
    'so one layer is not two',
  );

  group('sameRust on function types');
  final f1 = IrType.function(const [_int], _string);
  final f2 = IrType.function(const [_num], _string);
  expectTrue(!sameRust(f1, f2), 'int and num parameters differ');
  expectTrue(
    sameRust(
      IrType.function(const [_num], _string),
      IrType.function(const [_double], _string),
    ),
    'num and double parameters do not',
  );
  expectTrue(!sameRust(f1, _string), 'a function is not its return type');

  group('isNullable');
  expectTrue(!isNullable(_int), 'a plain type is not');
  expectTrue(isNullable(_nullable(_int)), 'the flag says so');
  expectTrue(isNullable(_option(_int)), 'and so does the wrapper');
  expectTrue(
    !isNullable(IrType('void', nullable: true)),
    "`void?` is `void`: the prelude's unit is never an Option",
  );
  expectTrue(
    !isNullable(IrType('()', nullable: true)),
    'and neither is the unit spelled as Rust spells it',
  );

  group('stripNull and withNull');
  expect(stripNull(_nullable(_int)).name, 'int', 'the flag comes off');
  expectTrue(!isNullable(stripNull(_option(_int))), 'and so does the wrapper');
  expectTrue(isNullable(withNull(_int)), 'withNull adds a layer');
  expectTrue(
    sameRust(withNull(_nullable(_int)), _option(_nullable(_int))),
    'and a second layer is the explicit wrapper',
  );
  expectTrue(
    sameRust(stripNull(withNull(_list(_string))), _list(_string)),
    'the two are inverse on a type that has arguments',
  );

  group('projectionDiffers');
  const projected = IrType('T', nullable: true, projected: true);
  const plain = IrType('T', nullable: true);
  expectTrue(
    projectionDiffers(projected, plain),
    '`<T as DartNullable>::Or` is not `Option<T>` at an edge',
  );
  expectTrue(!projectionDiffers(plain, plain), 'and matching ones do not');
  expectTrue(
    projectionDiffers(
      IrType.function(const [projected], _string),
      IrType.function(const [plain], _string),
    ),
    'a projection inside a function type needs an adapter too',
  );

  group('coerceInto');
  final world = _FakeWorld();
  // Dart's rule about the *literal*, which holds whoever declares the slot.
  final widened = coerceInto(
    IrLiteral('3', _int)..rustType = _int,
    _double,
    world,
  );
  expect(
    widened is IrLiteral ? widened.value : '$widened',
    '3.0_f64',
    'an int literal in a double slot is a double literal',
  );
  final untyped = IrLocal('x');
  expectTrue(
    identical(coerceInto(untyped, _double, world), untyped),
    'a value with no recorded type is left alone',
  );
  final same = IrLocal('y')..rustType = _string;
  expectTrue(
    identical(coerceInto(same, _string, world), same),
    'and so is one already in its slot',
  );
  final upcast = coerceInto(IrLocal('w')..rustType = _string, _object, world);
  expectTrue(
    !identical(upcast, same) && upcast.rustType != null,
    'a scalar into an Object slot is adapted, not passed through',
  );

  report('coerce');
}

/// A world where nothing is translated: `Object` is the only trait.
class _FakeWorld implements TypeWorld {
  @override
  bool isTrait(String name) => name == 'Object';
  @override
  bool isEnum(String name) => false;
  @override
  bool isCounted(String name) => false;
  @override
  bool isStruct(String name) => false;
  @override
  bool isBelow(String sub, String sup) => sup == 'Object';
  @override
  bool isGenericValueStruct(String name) => false;
  @override
  bool isTypeParameter(String name) => false;
}
