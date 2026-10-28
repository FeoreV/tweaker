# Recovery journal rotation and checkpoint epochs (proposal)

This is a design to review, not an implemented protocol or a durability claim.
Current sessions rotate the snapshot journal after an incomplete tail only. A
fully committed recovery journal can reach a record or byte limit after failed
retries. Each rotation also consumes a checkpoint generation, currently capped
at `MAX_TRANSACTION_RECORDS + 1`. Neither case is solved by the current append
preflight. Application mutations must remain disabled.

## Proposed state and invariants

Keep the source journal immutable. A checkpoint binds its exact ordered records,
source ID, next-exclusive recovery position, and recovery snapshot journal ID.
Introduce an epoch number and a cryptographic digest of the entire committed
checkpoint and its predecessor in a new versioned pointer record. The pointer
identifies one immutable epoch checkpoint. Snapshot ID changes are valid only at
unchanged recovery progress. A checkpoint is authoritative only when its
publication record is committed and verified; never infer a commit from a
pending file, a filename alone, or a valid CRC. Preserve the previous committed
epoch until the new pointer is durable and independently readable.

Recovery snapshot journals must remain available as evidence of each attempted
write, including failures. Epoch compaction cannot delete them merely because
progress advanced. Retention, disk exhaustion and any safe archival policy need
an explicit decision. The protocol must work within the existing root-wide lock
and source-then-snapshot lock ordering and fail closed on unknown files,
corruption, conflicting pointers, missing referenced data, or counter overflow.

## Candidate publication sequence

1. Hold source and mutation locks, validate the currently committed epoch and
   snapshot journal, and reserve a unique new snapshot ID. Distinguish a typed
   capacity exhaustion result from all I/O, corruption and access errors.
2. Write a complete new checkpoint with unchanged progress and new snapshot ID
   to an exclusive pending file. Flush, close, reread, decode and compare it.
3. Publish the immutable checkpoint and then a pointer to it using a documented
   atomic publication mechanism. Flush and verify the pointer. No restore may
   use the new snapshot journal before that publication succeeds.
4. Reopen and validate the new epoch against the original records. Keep the
   previous pointer and checkpoint until a later independently verified cleanup
   protocol permits retirement. Resume from the newly bound snapshot ID.

The current pending-marker and `MoveFileExW(MOVEFILE_WRITE_THROUGH)` protocol is
a starting point, not proof that replacing a pointer or deleting an old epoch is
durable after power loss. A two-slot pointer requires a precisely specified
atomic replacement and recovery rule. In particular, two valid slots with
incomparable epochs must not be resolved by guessing. Define generation rollover,
idempotent retry after publication, and a bounded scheme for retiring epochs
before claiming that checkpoint generations cannot be exhausted.

## Compatibility and migration

Read existing v3/v4 checkpoints unchanged. Migration must first verify the
existing committed prefix and write a new epoch referencing its exact progress
and recovery ID. Until the new pointer is committed, the old checkpoint remains
authoritative. An older binary must fail closed when it encounters the new
namespace; it must never resume from an obsolete checkpoint. No migration may
rewrite or discard the source journal. The format, digest algorithm, version
negotiation and implementation dependencies remain to be reviewed.

## Required evidence

Exercise both registry and typed sessions with repeated failed restores until a
snapshot journal reaches its record and byte limits, then rotate and complete
recovery. Cover an individual recovery record near its format limit, 4096 source
records, repeated rotations, counter overflow, insufficient disk, identity
collision, and ordinary I/O errors that must not trigger rotation. Inject faults
before and after every create, flush, readback, publish and cleanup boundary;
kill the process at each boundary and reopen twice, checking that progress never
advances without a verified restore and no write occurs without a committed
pre-state. Test corrupted or conflicting pointers and downgrade attempts.

Host process termination tests are not power-loss evidence. Validate the final
protocol on disposable NTFS volumes with controlled power-cut tests and Win7
x86/x64 VMs, including external writers and path replacement. Until namespace
security and storage durability are established, epoch machinery alone cannot
authorize production mutations.
