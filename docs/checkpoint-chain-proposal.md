# Checkpoint chain: candidate protocol

Status: proposal for review. This protocol is not implemented or proved. It is a
candidate for removing the current 4,097-generation checkpoint limit without
retiring source records, legacy checkpoints, or snapshot journals. It does not
make application mutations safe to enable. `docs/recovery-rotation-design.md`
contains the broader problem and outstanding evidence gates.

## Model and assumptions requiring validation

The intended first milestone is recovery from process termination on a fixed
NTFS volume, with cooperating processes obeying the existing root-wide mutation
barrier and source-then-snapshot lock ordering. It relies on the visibility and
atomicity of a same-directory, no-replace publication operation: observers
must see either no final marker or its complete, previously flushed bytes. The
current `MoveFileExW(..., MOVEFILE_WRITE_THROUGH)` call is a candidate, not
proof of that property. Its behavior, error ambiguity, sharing semantics and
Win7 x86/x64 compatibility must be established before implementation is relied
upon. A successful flush or readback is not proof of durability through power
loss. If the assumed publication property cannot be established, fail closed;
do not infer a commit from a valid-looking payload.

Protected root/ancestor validation and locks are part of the existing contract,
not a defense against a same-user or privileged external writer, namespace
replacement, malicious media, or arbitrary corruption. Hashes detect accidental
mismatch and link content; they are not authentication. This proposal does not
assume power-loss ordering or claim recovery after power loss. Storage and
namespace security gates and physical power-cut tests remain open.

## Names and downgrade barrier

For a valid source transaction ID `S`, let `P` be the existing three-decimal
byte-length prefix followed by `-S-recovery-`. All proposed names are in the
same journal root and contain only ASCII. Use these exact forms:

* `Pepoch.json`: permanent downgrade guard.
* `Pchain-<id>.json`: immutable node payload, initially created exclusively.
* `Pchain-<id>.commit.pending`: staged marker.
* `Pchain-<id>.commit`: published marker.

`<id>` is exactly 32 lowercase hexadecimal digits representing a candidate
128-bit ID. It is an identity, not an ordering or counter. Generate IDs using a
reviewed Win7-compatible source; reserve a node ID by exclusively creating its payload, require both marker names to be absent, and
never overwrite, reuse, or infer priority from an ID. A collision retries with
a fresh ID up to a documented finite attempt limit; failure stops recovery.
Creation of a snapshot transaction ID separately uses the existing reservation
contract and must also be collision checked against all existing transaction
artifacts. New discovery and transaction-ID creation must understand every
proposed name and retain existing locks; unknown entries remain an error.

Under the source lock and mutation barrier, after validating the complete legacy
state, exclusively create the guard as the first migration action. Its presence,
including zero-length or partially written content, prevents the **current**
legacy `FileRecoveryCheckpoint::open` from accepting the transaction: its
`.json` enumeration rejects `epoch` as a malformed 20-digit generation.
Current `discover_transactions` also rejects that name. Flush and read back the
guard before publishing any chain marker or using a chain snapshot. The new
reader treats a regular, non-reparse guard of bounded size as a migration fence,
not as a commit or a source of state. Never remove or rename the guard, even
after completed recovery. Its initial creation may leave a partial file after a
crash, so the new reader must be able to resume migration with it present.

This is a candidate barrier for the inspected implementation only. Test the
actual shipped older binaries and every direct entrypoint, including registry
and typed recovery, checkpoint APIs, discovery, transaction creation and any
path bypassing discovery. A guard cannot stop a binary that ignores unknown
names or a caller that performs direct writes without the recovery checkpoint.
Do not claim downgrade safety for such versions; require an enforceable external
version barrier or leave migration disabled if any relevant entrypoint can
mutate using obsolete state. In-flight older processes must also be excluded by
the same locks before guard creation, and locking behavior must be tested.

In particular, the inspected legacy `sequences_for` skips `recovery-` names.
Direct transaction locking, journal scanning and low-level journal writers do
not establish this guard as a barrier. A caller that resumes or writes through
those APIs without opening the recovery checkpoint may bypass it. Test those
paths against the actual supported old binaries and callers; checkpoint and
discovery rejection alone is not evidence of downgrade protection.

## Node and marker

Version 1 node payload is a bounded, strictly decoded format with explicit
version, source ID, node ID, predecessor (`null` only for genesis), full typed
checkpoint, and a SHA-256 digest of the exact ordered source record bytes as
committed. The predecessor contains both predecessor ID and SHA-256 of its exact
published node bytes. The checkpoint retains the exact ordered source records,
`next_exclusive`, and snapshot transaction ID; validate it against the actual
immutable committed source prefix using the existing typed validation. Define
canonical byte encoding, fixed digest representation, maximum lengths, duplicate
field rejection and version negotiation before coding. Hash exact stored bytes,
not an implementation-dependent reserialization. The algorithm and its
Win7-compatible implementation/dependency require review; a digest is not an
access control mechanism.

Specify an unambiguous framed encoding for the source-record digest before
implementation: define record order, length framing, and exactly which stored
record and commit-marker bytes are included. Concatenating variable-length
files without framing is not a sufficient specification. The existing
recovery checkpoint requires a nonempty committed source; empty source journals
are outside this candidate's migration scope unless a separately reviewed
contract is added.

Genesis also binds the exact last committed legacy checkpoint and marker bytes
by digest and identifies its generation, progress and recovery ID. A separate
explicit genesis variant is required when there is no committed legacy checkpoint:
record that fact and the initial progress and reserved snapshot ID. It must not
pretend an uncommitted legacy tail is committed. A marker contains version,
source ID, node ID and SHA-256 of the exact payload bytes; strict validation
must reject extra fields, malformed encoding and mismatched names or contents.
No generation count or sortable timestamp determines authority.

For each subsequent node the predecessor must be the unique current tip.
Progress may only decrease after a restore has succeeded and been verified by
the recovery contract; a snapshot ID change must keep progress unchanged. A
node may not combine a changed snapshot ID with changed progress. The initial
legacy-to-genesis state is exactly equal, including the snapshot ID and progress.
Validate the bound snapshot journal under its lock before use. Each restore must
first publish and verify its own pre-state record in that snapshot journal.
Never advance progress solely because a node or snapshot exists. Specify and
enforce the permitted progress delta and its correspondence to the recovery
operation; a decreasing index and a valid hash chain alone do not prove that a
restore succeeded or was verified. Review how the recovery runner supplies that
evidence before allowing publication of a progress transition.

## Publication, admission and reopening

1. Hold the existing source, mutation and applicable snapshot locks in the
   established order. Scan and validate the entire source, legacy metadata and
   chain. Reserve a fresh node ID; when rotating, reserve a fresh snapshot ID
   without deleting any previous snapshot or its incomplete tail.
2. Exclusively create the final-named payload, write complete bytes, flush and
   close, then bounded-read and compare exact bytes and validate all bindings.
   An uncommitted payload is staging even if it decodes perfectly.
3. Exclusively create the pending marker, write, flush, close, bounded-read and
   compare it. Publish by the validated same-directory no-replace operation.
   Independently reopen and validate the final marker and its payload. Only then
   may this process use the new snapshot or report the new progress committed.
   Any failure or ambiguous result poisons the in-memory session: release locks,
   reopen and rescan before any further restore. Never delete or overwrite a
   possibly published marker to make a retry work.
4. On reopen, enumerate the entire bounded namespace under locks. Every final
   marker must have exactly one valid payload; a malformed, unreadable, missing
   or mismatched committed object is an error. Validate all predecessor links,
   genesis/legacy binding, transition rules, snapshot identities and the exact
   source prefix. Require one genesis, one connected acyclic linear chain and
   one tip. Missing predecessors, cycles, duplicate IDs, multiple genesis nodes,
   two children of one node, unknown versions or incompatible state fail closed.
   Never select the largest ID, the last enumerated file, or the legacy state
   when a committed chain is inconsistent.

Staging admission is intentionally narrow. An exact-name payload without a final
marker is an orphan; it may contain partial bytes. A pending marker without a
final marker may be partial, but must have a matching payload. These artifacts
have no authority and are retained as evidence. A pending marker together with
its final marker, a marker without a payload, non-regular or reparse entries,
malformed names, duplicate/case-colliding names, unexpected files, or objects
exceeding their individual bounds fail closed. Enumerate and bound staging as
well as committed objects. No inferred commit, overwrite, promotion, cleanup or
reuse is allowed on reopen. After a pre-publication crash a fresh attempt uses
a new ID. Once a final marker is visible and validates, it participates in the
chain even if the writer did not receive publication success. On any conflict,
stop for explicit offline diagnosis; do not guess which branch is correct.

If guard exists but there is no committed genesis, validate the unchanged legacy
state again and retry genesis with a new ID. If a genesis exists, do not resume
legacy. During migration the old checkpoint remains untouched and no system
restore is performed before genesis and the required snapshot binding are
published and verified. If there was no legacy checkpoint, the initial snapshot
reservation and initial state must be persisted in genesis before any restore;
otherwise fail closed. The scan must account for abandoned snapshot reservations
and never reuse them. This protocol preserves all legacy files, snapshots and
chain history; it defines no deletion, compaction, repair or archival policy.

Migration needs a read-only legacy validator that can inspect the last permitted
generation and incomplete tails without allocating another generation or creating
any checkpoint. The current `FileRecoveryCheckpoint::open` can load the last
permitted committed generation, but a subsequent persistence is rejected at the
limit; when no marker exists, `open` itself persists an initial checkpoint. Do
not use that side-effecting operation as the migration validator.

## Resource and failure policy

Do not impose the legacy 4,097-generation ceiling on the chain. Still impose
explicit checked limits for individual files, ID collision attempts, total
entries, total bytes, allocation, hashing, and scan time. Reaching any configured
budget, disk-full, integer overflow, access error, I/O error, unexpected object
or unavailable referenced journal is an explicit fail-closed error; never
silently fall back to an older tip. Document limits and report actionable
resource exhaustion distinctly from corruption and ordinary I/O errors. A
bounded system retaining every attempt cannot promise infinite retries. Only a
typed preflight result proving that the next record fits an empty snapshot but
not the current snapshot can request rotation; all other errors stop.

No state transition may depend on deleting orphan files to make room. A future
retention or repair design needs its own proof, crash protocol, authorization
and tests; it cannot be implied by this proposal. No successful process-crash
test implies power-loss safety.

Repeating the full source records in every node can consume quadratic total
space as source size and transition count grow together. Rescanning the entire
retained chain for each publication can likewise produce quadratic cumulative
work in the number of transitions. Concrete scan, allocation and storage budgets
can therefore be exhausted well before an operation completes. Measure and
specify these limits, report exhaustion without falling back, and resolve the
costs before adoption; this proposal does not yet solve them.

## Required tests before adoption

* Fixture legacy v3/v4, reject empty and exercise populated source journals, 4,096 records,
  the last permitted legacy generation, incomplete checkpoint and snapshot
  tails, and both registry and mixed typed recovery. Confirm legacy semantics
  before guard, and run actual supported old binaries against every public
  entrypoint after guard creation, including a zero-byte guard and concurrent
  old processes holding or contending for locks.
* Inject failure and separately kill a child process before and after each guard,
  snapshot reservation, node and marker create/write/flush/close/readback,
  publication and final readback boundary. Reopen twice, retry, and check that
  all old files survive, progress never advances without a verified restore,
  and every attempted system write has a committed verified pre-state. Include
  ambiguous publication result and a crash during retry.
* Force repeated failed restores until both byte and record snapshot capacity
  limits are reached, rotate repeatedly, and complete recovery. Cover a record
  near format size limits, fresh-snapshot incompatibility, snapshot-ID and
  node-ID collisions, exhaustion of collision attempts, disk full, unreadable
  files, ordinary I/O errors, checked arithmetic overflow and scan budgets.
* Exercise orphan payloads at every partial byte boundary, partial pending
  markers, orphan reservations, valid committed markers after reported failure,
  marker without payload, pending plus final, corruption of each committed byte,
  missing ancestor, invalid digest, multiple genesis, forks, cycles, wrong
  source/progress/snapshot, stale predecessor, unknown version, malformed or
  case-colliding names and unexpected filesystem objects. Each must follow the
  admission rules above and never choose an obsolete state.
* Verify actual same-directory no-replace publication behavior and exclusion on
  supported systems, then separately run controlled power-cut tests on disposable
  NTFS volumes, Win7 x86/x64 VM tests, and external-writer/path replacement
  security tests. Record outcomes and unresolved races; do not turn host process
  termination into a power-loss claim.

