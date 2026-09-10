// The members that change their receiver, in one place.
//
// This knowledge was written out five times -- `alias_mutation.dart`'s
// `_mutatingNames`, `frontend_kernel.dart`'s `_mutatingListNames` and the
// inline set in `_ThisWriteFinder`, `backend_rust.dart`'s `_inPlace`, and
// `_WalkSelf`'s `_mutatingListMethods` -- and the five had drifted apart.
// Four were gathered here on 2026-09-09; the fifth was missed that day
// because it is spelled in *Rust* names and so did not look like the others.
// It is [mutatingWalkSelfRustNames] below. Nothing said so, and nothing could: each
// copy was a bare list of strings with no way to compare it against the
// others, so a name added to one was silently absent from the rest.
//
// What the copies actually disagreed about, measured 2026-09-09:
//
//   `_ThisWriteFinder`  lacks `removeRange`, `updateAll` and every
//                       `ByteData` setter
//   `_inPlace`          lacks `update_all`
//   `_mutatingListNames` lacks the `Map` members and the `ByteData` setters,
//                       and adds `length` -- the only one of the four
//                       differences that is a decision (`list.length = n`
//                       is a write, and this set is a *list* receiver's)
//
// One set per reason here, composed into the four sets the compiler asks
// for, and `test/member_names_test.dart` pins every difference that remains.
// A gap is written down as a gap rather than closed: closing one changes
// what the compiler emits, which is a measured round against the chain, not
// an edit to a table.
library;

/// `List`, `Set` and `Queue` members that change the receiver, by Dart name.
///
/// Without `removeRange`, which `_ThisWriteFinder` is missing -- named apart
/// so that the gap is a name rather than an omission.
const mutatingSequenceCoreNames = {
  '[]=',
  'add',
  'addAll',
  'insert',
  'insertAll',
  'remove',
  'removeAt',
  'removeLast',
  'removeWhere',
  'retainWhere',
  'clear',
  'setRange',
  'fillRange',
  'replaceRange',
  'setAll',
  'sort',
  'shuffle',
  'addFirst',
  'addLast',
  'removeFirst',
};

/// Every sequence member that changes the receiver.
const mutatingSequenceNames = {...mutatingSequenceCoreNames, 'removeRange'};

/// `Map` members that change the receiver, by Dart name.
///
/// `[]=` is a sequence member here as well as a map one; it is listed once,
/// above, because every consumer of either set wants it.
const mutatingMapNames = {'putIfAbsent', 'update', 'updateAll'};

/// `ByteData`'s setters: a byte view written in place (`WriteBuffer.
/// putUint16` on its `_eightBytes`, run509).
const mutatingByteDataNames = {
  'setInt8',
  'setUint8',
  'setInt16',
  'setUint16',
  'setInt32',
  'setUint32',
  'setInt64',
  'setUint64',
  'setFloat32',
  'setFloat64',
};

/// Every Dart name that changes its receiver -- what the alias-mutation scan
/// asks for, since a mutation through an alias is one whatever the receiver's
/// class.
const mutatingNames = {
  ...mutatingSequenceNames,
  ...mutatingMapNames,
  ...mutatingByteDataNames,
};

/// What `_ThisWriteFinder` looks for: `this.field.add(x)` is a write to the
/// object, as an assignment is.
///
/// Three groups short of [mutatingNames], and none of the three by decision:
/// `removeRange`, `updateAll` and the `ByteData` setters are gaps. Closing
/// them widens which methods take `&mut self`, which the chain has to
/// measure, so they stay named here until a round does.
const mutatingThisFieldNames = {
  ...mutatingSequenceCoreNames,
  'putIfAbsent',
  'update',
};

/// What `_listReceiver` looks for: a mutating member's receiver is the place
/// itself, never a narrowing copy.
///
/// The sequence members and `length` -- `list.length = n` truncates or grows
/// in place -- and no map or byte-view member, because the receiver here is
/// a list.
const mutatingListNames = {...mutatingSequenceNames, 'length'};

/// Dart names with no Rust method of the same name behind them.
///
/// `[]=` is Rust's index assignment, emitted as one; `updateAll` has no
/// prelude method yet, which is why `_inPlace` never had it.
const noRustMutatorNames = {'[]=', 'updateAll'};

/// Rust names of receiver-changing methods with no Dart member behind them:
/// the prelude's own spellings, and the `Vec`/`VecDeque` methods the backend
/// emits directly.
const mutatingRustOnlyNames = {
  'push',
  'pop',
  'push_back',
  'push_front',
  'pop_back',
  'pop_front',
  'extend',
  'retain',
  'retain_all',
  'remove_all',
  'remove_value',
  'truncate',
  'drain',
  'reverse',
  'swap',
  'sort_by',
  'sort_natural',
  'add_entries',
  // `list.last = v` writes through the last slot, so the receiver must be
  // the place and not the clone a read takes.
  'set_last',
  // The prelude's `Map::remove`, distinguished from `Vec::remove` by the
  // marker the backend puts in front of it.
  '!map_remove',
};

/// Rust names `_WalkSelf` treats as changing their receiver.
///
/// A different question from the one `_inPlace` answers, asked of the same
/// knowledge: which local needs `let mut`, and which method therefore needs
/// `&mut self`. Rust says both out loud where Dart says nothing.
///
/// Ten of the fifty-one names `_inPlace` composes, and two more that it has
/// never carried: `!insert` and `!remove_at` are markers the backend spells
/// in front of a method to say which of two same-named prelude methods it
/// means, and only `!map_remove` ever reached [mutatingRustOnlyNames].
///
/// Both differences are gaps rather than decisions, and stay written down as
/// gaps. Widening this set widens which locals carry `mut` and which methods
/// take `&mut self` -- a measured round against the chain, not an edit to a
/// table. `test/member_names_test.dart` pins the distance.
const mutatingWalkSelfRustNames = {
  'push',
  'extend',
  'clear',
  'pop',
  'insert',
  'remove',
  '!map_remove',
  '!insert',
  '!remove_at',
  // The ordered `Map`'s own mutators. `put_if_absent` may write, so it takes
  // `&mut self`, and its receiver needs to say so; so does `update`, and
  // `sort_natural` sorts in place.
  'put_if_absent',
  'update',
  'sort_natural',
};
