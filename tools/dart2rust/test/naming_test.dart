// How a Dart name becomes a Rust one.
//
// Every rule here was put in by a round that had a crate fail to parse or a
// name redefine another, and each is one line deep in a 13k-line file where
// nothing checked it. The cases are the ones named in those rules' comments.
library;

import '../lib/backend_rust.dart';
import '../lib/ir.dart' show stdOperators;
import 'check.dart';

void main() {
  group('snakeRaw');
  expect(snakeRaw('addAll'), 'add_all', 'a camel hump is a break');
  expect(snakeRaw('setUint16'), 'set_uint16', 'a digit is not a hump');
  expect(
    snakeRaw('RenderBox'),
    'render_box',
    'a leading capital is not a break',
  );
  expect(snakeRaw('x'), 'x', 'one letter is itself');
  expect(snakeRaw('type'), 'type', 'and the keyword escape is not its job');

  group('snake');
  expect(snake('type'), 'r#type', 'a Rust keyword is spelled raw');
  expect(snake('box'), 'r#box', 'reserved counts: `box.left` did not parse');
  expect(snake('crate'), 'crate_', 'what cannot be raw gets a suffix');
  expect(snake('self'), 'self_', 'and so does `self`');
  expect(snake('addAll'), 'add_all', 'an ordinary name is just snaked');
  expect(
    snake(r'_#wc0#formal'),
    '__wc0_formal',
    'a synthetic parameter name loses the characters Rust has no place for',
  );
  expect(snake('9lives'), '_9lives', 'and a leading digit gains an underscore');

  group('screamingSnake');
  // Rust's keywords are lowercase, so an upper-cased name is never one --
  // but it still needs the character-level clean.
  expect(screamingSnake('maxWidth'), 'MAX_WIDTH', 'a constant is upper snake');
  expect(screamingSnake('type'), 'TYPE', 'with no keyword escape');
  // An already-screaming name is snaked again on the way through, which is
  // ugly and legal: what the rule is for is the `$`, which rustc rejects.
  expect(
    screamingSnake(r'_$ADD_EVENT'),
    '___A_D_D__E_V_E_N_T',
    r'a name that was already upper-cased keeps no `$`',
  );

  group('variantName');
  expect(variantName('spaceBetween'), 'SpaceBetween', 'only the first letter');
  expect(
    variantName('rtl'),
    'Rtl',
    'the rest stays searchable against upstream',
  );
  expect(variantName(''), '', 'and an empty name is left alone');

  group('the operator tables');
  // `ir.dart`'s `stdOperators` decides propagation (an `impl Add` keeps the
  // trait's signature, so a call of one never returns `Result`);
  // `backend_rust.dart`'s `operatorTraits` writes the impls. Two lists of the
  // same operators, in two files, with a comment in each pointing at the
  // other -- this is the check that comment was standing in for.
  expect(
    operatorTraits.keys.toSet(),
    stdOperators,
    'every operator with a Rust trait is one the front end knows about',
  );
  expect(operatorTraits['+'], ('Add', 'add'), 'and each names its trait');
  expect(operatorTraits['unary-'], ('Neg', 'neg'), 'including the unary one');
  expectTrue(
    !stdOperators.contains('~/') && !stdOperators.contains('[]'),
    'the operators Rust has no trait for are methods, and may fail',
  );

  report('naming');
}
