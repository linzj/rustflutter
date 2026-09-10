// A type parameter bounded by a *prelude value type*.
//
// Rust cannot spell `T: DateTime` as a bound -- `DateTime` is a struct
// here, not a trait -- so a `T` carries none of `DateTime`'s members and
// `dateA.year` found no `year` on a `T`
// (`CalendarDelegate<T extends DateTime>.isSameDay`, and `isSameMonth`,
// and the `==` beside them).
//
// Every prelude value type has a `FromDynamic`, so the object can be asked
// for the bound Dart promised: the value goes to `Object` and comes back as
// the bound. Three layers had to agree before the group fell:
//
// * the receiver is narrowed to the bound at all (`_receiver` used to stop
//   at a bound that is not a translated abstract class);
// * the coercion into a *prelude value type* exists (a translated struct or
//   enum must NOT go this way -- that cost two stubs at ws1048, where a
//   `Map<SlotType, ..>` key was converted to the one instantiation there
//   is);
// * a null-aware binding is a *reference*, and `DartAny` is implemented for
//   the value, so the box takes a clone (`dateA?.year` lowers to
//   `.as_ref().map(|it| ..)`).
//
// What this pins is the answer, not just the compile: the dates below
// differ in each field in turn, so reading the wrong one shows.

class Days<T extends DateTime> {
  const Days();

  bool sameDay(T? a, T? b) =>
      a?.year == b?.year && a?.month == b?.month && a?.day == b?.day;

  bool sameMonth(T? a, T? b) => a?.year == b?.year && a?.month == b?.month;

  // The bound's members on a non-null `T`, beside the null-aware reads.
  String show(T a) => '${a.year}-${a.month}-${a.day}';
}

String use() {
  const Days<DateTime> days = Days<DateTime>();
  final DateTime a = DateTime(2026, 9, 11);
  final DateTime sameDay = DateTime(2026, 9, 11);
  final DateTime otherDay = DateTime(2026, 9, 12);
  final DateTime otherMonth = DateTime(2026, 8, 11);
  final DateTime otherYear = DateTime(2025, 9, 11);
  return '${days.sameDay(a, sameDay)}'
      '/${days.sameDay(a, otherDay)}'
      '/${days.sameDay(a, otherMonth)}'
      '/${days.sameDay(a, otherYear)}'
      '/${days.sameMonth(a, otherDay)}'
      '/${days.sameMonth(a, otherMonth)}'
      '/${days.sameDay(a, null)}'
      '/${days.sameDay(null, null)}'
      '/${days.show(a)}';
}
