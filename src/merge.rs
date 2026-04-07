/// Content-addressed archive merging.
///
/// `tdg merge a.tg b.tg -o merged.tg`
///
/// Block union: all unique blocks (by hash) from both archives are included.
/// File tree merge: union of all paths. Conflicts resolved by newer mtime
/// (last-writer-wins). If mtimes equal, left archive wins.
///
/// Dedup is automatic: shared blocks are stored once.

// Stubbed for v0.1 — core format supports it via content-addressed blocks.

pub(crate) fn _stub() {}
