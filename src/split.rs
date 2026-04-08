// Volume splitting and reassembly.
//
// `tdg split archive.tg --size 4G` splits at block boundaries.
// `tdg join part1.tg part2.tg -o archive.tg` reassembles.
//
// Each volume has its own header. The last volume has the index and footer.

// Stubbed for v0.1.
pub(crate) fn _stub() {}
