// The members that change their receiver, in one place.
//
// This knowledge was written out four times -- `alias_mutation.dart`'s
// `_mutatingNames`, `frontend_kernel.dart`'s `_mutatingListNames` and the
// inline set in `_ThisWriteFinder`, and `backend_rust.dart`'s `_inPlace` --
// and the four had drifted apart. Nothing said so, and nothing could: each
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
  // The prelude's `Map::remove`, distinguished from `Vec::remove` by the
  // marker the backend puts in front of it.
  '!map_remove',
};
