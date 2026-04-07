/// Incremental/differential archive support.
///
/// An incremental archive stores only blocks not present in the base archive.
/// The header has the INCREMENTAL flag and stores the base's root hash.
/// BlockRefs with BLOCKREF_FLAG_EXTERNAL point to blocks in the base.
///
/// Creation: `tdg create --incremental base.tg diff.tg ./path`
/// Extraction: `tdg extract diff.tg --base base.tg -o dest`

// Stubbed for v0.1 — format supports it, implementation follows.

pub(crate) fn _stub() {}
