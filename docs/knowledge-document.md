# M5A:B1 — bounded local knowledge document

This is an application-layer offline example, not a GUI-core API or a connected
knowledge service. `examples/support/knowledge_document.rs` owns schema v1,
validation and storage; `examples/knowledge_document.rs` exercises the complete
serialize → parse → atomic save → load path with deterministic synthetic notes.
No clipboard, credentials, private data, SQLite, RAG, cloud or network is used by
the example. Serde dependencies are example/test-only dev dependencies.

## Schema v1 contract

- `schema_version` must be integer `1`. Other versions are rejected, not migrated.
  Unknown fields at every object level, duplicate JSON fields, missing required
  fields, malformed JSON and invalid UTF-8 are rejected. `parent: null` means no
  parent; serde also treats an omitted optional `parent` as null.
- `source` is the exact original string. No Unicode normalization, newline
  conversion or whitespace trimming occurs, including CRLF, blank lines, NUL and
  a missing final newline. JSON escape spelling is not preserved, string bytes are.
- `paragraphs` is the canonical list of half-open **absolute UTF-8 byte** ranges
  produced by `split_inclusive('\n')`: one paragraph per line, including its LF
  and any preceding CR. Blank lines remain paragraphs; empty source has none;
  a terminal LF does not create an extra empty paragraph. The entire list must
  match the source exactly; users cannot supply gaps, overlaps or invented spans.
- `root` names one node. `nodes` is an ordered list with stable application IDs
  (not framework `NodeId`); serialization never regenerates IDs or reorders nodes.
  Each node has `id`, `parent`, `title`, `expanded`, and `source_refs`. Parent links
  define the rooted tree; child order is the order in `nodes`. Exactly the named
  root has no parent. Missing parents, disconnected trees, self-links and cycles
  fail validation. `expanded` is retained even for leaves.
- Each source reference has a zero-based `paragraph` and `bytes: {start, end}`.
  The range must be nonempty, wholly inside that paragraph, and on UTF-8 codepoint
  boundaries. A reference can cover a partial paragraph, including its newline.
  Multiple nodes can cite the same text. Classification containers can have an
  empty reference list; no source is fabricated. This validates provenance ranges,
  not whether a generated title is factually supported by its citations.
- The complete source and last accepted tree are saved together. Parse/load returns
  a validated document or an error without replacing any caller-held valid model.
  Future UI/request logic must keep its previous valid document on failure; no
  pending request or failed generated result is persisted by this slice.

## Deliberately small limits

| Item | Limit |
| --- | --- |
| JSON input/output | 256 KiB |
| Exact source | 64 KiB UTF-8 |
| Paragraphs | 1,024 |
| Nodes | 128 |
| Root-to-leaf depth, including root | 32 |
| References per node | 16 |
| Title | 1–256 UTF-8 bytes |
| ID | 1–64 ASCII letters, digits, `_` or `-` |

Load reads at most file limit + 1 byte, then rejects overflow. Parse checks the
byte cap before serde decoding; serde's default recursion limit remains enabled.
Validation caps all variable collections and strings before traversing the tree.
Pretty JSON can exceed the file cap even for a valid model (for example many
escaped control characters); serialization rejects it before opening a temp file.
No chunking, partial truncation or silent downgrade is performed. Error messages
identify the failure category without embedding source text.

## Explicit local path and safe replacement

```sh
cargo run --locked --example knowledge_document -- create /tmp/synthetic-knowledge.json
cargo run --locked --example knowledge_document -- load /tmp/synthetic-knowledge.json
cargo test --locked --example knowledge_document
```

`create` explicitly replaces the selected path with the built-in offline fixture;
`load` only reads it. No default user directory or implicit autosave is selected.
The parent directory must exist. The caller must choose a trusted writable local
directory, and must serialize saves to the same destination (concurrent writers
are not coordinated; last rename wins).

Save validates and encodes first, creates a unique sibling temp with `create_new`,
writes all bytes, calls `sync_all` on the temp, closes it, then renames over the
selected destination. Up to 32 colliding temp names are retried without touching
existing files. All pre-rename errors leave the destination byte-for-byte intact;
a drop guard attempts to remove only the temp created by this save. No fallible
operation follows successful rename, so an error is never reported after replacing
the valid destination. Linux same-filesystem rename is the verified target.
There is no parent-directory fsync or power-loss durability guarantee, locking,
backup history, hostile-directory/symlink defense, permission/metadata preservation,
or untested Windows replacement guarantee. Temp cleanup is best effort if the
filesystem itself refuses removal; abrupt process termination may leave a temp.
These limits are distinct from the tested normal/injected I/O failure cleanup.

## Verification scope

The example's test harness is enabled in `Cargo.toml`, so `cargo test --workspace
--locked` includes it. Tests cover exact Unicode/CRLF/NUL/empty-source round trips,
IDs, citations and expansion restore, successful replacement, size/depth boundaries,
unknown fields at every level, corrupt/oversized files, invalid schema/tree/ranges,
and validation/create/write/partial-write/sync/pre-rename failures. Deterministic
fault injection runs the real storage path up to each named stage, including a
partially written temp; real missing-parent and rename-to-directory errors also
verify cleanup. Injection tests the error handling contract, not actual disk-full
or power-loss behavior. GUI layout, clipboard and gateway behavior are not claimed.
Local commands, candidate SHA, clean archive verification and outcomes are recorded
in `.local/verification/M5A-B1-report.md` (intentionally not committed).
