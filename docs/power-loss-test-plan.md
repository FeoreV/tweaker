# Journal crash and power-loss verification plan

Status: **plan only; none of the VM or physical-storage interruption tests below has been executed.** Host fault injection and child-process termination tests are evidence for their stated boundaries only.

## Current storage scope

The Windows journal backend now fails closed unless the root is on a fixed NTFS volume reporting `FILE_PERSISTENT_ACLS`. The current host project volume `D:` was observed as fixed NTFS; the complete host test suite passed against it. This is a filesystem-policy gate, not a durability claim. Other volume classes and file systems are unsupported until separately reviewed and tested.

The backend queries volume capabilities with Win32 APIs available on Windows 7: [`GetVolumePathNameW`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumepathnamew), [`GetDriveTypeW`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew), and [`GetVolumeInformationW`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationw).

## Evidence already available

- Host fault injection covers record/checkpoint create, write, flush, readback, and marker publication failures and reopens the journal to check the committed prefix.
- A child process signals an out-of-process controller over TCP at 31 cuts: 11 record/commit-marker boundaries, 10 initial checkpoint/marker boundaries, and the same 10 boundaries while rotating the recovery snapshot ID. It then blocks with handles open. The host test kills it with `Child::kill`, checks raw files and markers, and reopens the journal. Separate `prepare`, `arm`, and `verify` modes support an external VM controller.
- These tests run on host NTFS. Process exit still allows the host operating system and storage stack to flush cached writes; they do **not** simulate loss of guest memory, filesystem metadata, controller cache, or drive cache.

## Guest-OS crash matrix

Purpose: verify Windows guest reboot recovery on supported NTFS configurations. This is distinct from physical power-loss durability.

1. Use disposable Windows 7 SP1 x86 and x64 VMs with snapshot/checkpoint rollback. Add a current Windows x64 VM as a separate row. Store only generated journal test data on a disposable NTFS virtual disk.
2. Build the probe for the exact target/toolchain and record the binary SHA-256, OS build, architecture, NTFS version, virtual controller, virtual disk mode, and hypervisor version.
3. Use the existing deterministic test-only probe. It pauses and sends the boundary name to an out-of-guest controller over TCP; no timing sleeps are used. It writes journal files only under the supplied test root and does not access the registry or SCM.
4. For each boundary, the controller waits for the signal, verifies that the VM is running, issues a hard power-off (no guest shutdown), verifies the VM is off, then starts it from the same virtual disk. Reopen and scan the journal and save the raw directory listing, marker/record hashes, scan result, and recovery result.
5. Run the record and checkpoint boundary sets below. For checkpoint snapshot-ID rotation, use `RUSTWEAKER_CRASH_PROBE_KIND=rotation`; the probe first commits generation 0, then cuts the generation-1 rotation. The verifier checks the raw files, published recovery ID, and resumed progress.
6. Repeat every boundary 20 times from a clean VM checkpoint. Preserve each run's controller log and VM log; a pass requires every artifact to agree with the journal's commit marker and recovery checkpoint. Incomplete tails must fail closed and must not be interpreted as committed operations.

Run the probe inside the VM using the same locked checkout/toolchain for all phases. Replace the example `R:` with the drive letter of the disposable fixed NTFS virtual disk. The root must be absent for `prepare`, then remain on that disk across each hard-off/reboot. Supply a fresh transaction ID for each arm/verify cycle.

```powershell
# Prepare a new journal root once.
$env:RUSTWEAKER_CRASH_PROBE_MODE = 'prepare'
$env:RUSTWEAKER_CRASH_PROBE_ROOT = 'R:\journal-probe'
$env:RUSTWEAKER_CRASH_PROBE_TRANSACTION = 'unused_prepare_id'
cargo test --locked --lib --features experimental-ui -- --exact data::file_store::tests::journal_crash_child_process_probe --nocapture

# Arm one boundary. The controller must be listening at this host-reachable endpoint.
$env:RUSTWEAKER_CRASH_PROBE_MODE = 'arm'
$env:RUSTWEAKER_CRASH_PROBE_KIND = 'record'
$env:RUSTWEAKER_CRASH_PROBE_TRANSACTION = 'tx_vm_cut_0001'
$env:RUSTWEAKER_CRASH_PROBE_BOUNDARY = 'CommitMarkerPublish'
$env:RUSTWEAKER_CRASH_PROBE_SIGNAL = '192.0.2.10:48127'
cargo test --locked --lib --features experimental-ui -- --exact data::file_store::tests::journal_crash_child_process_probe --nocapture

# After reboot, run the verifier with the same root, transaction, and boundary.
$env:RUSTWEAKER_CRASH_PROBE_MODE = 'verify'
$env:RUSTWEAKER_CRASH_PROBE_KIND = 'record'
cargo test --locked --lib --features experimental-ui -- --exact data::file_store::tests::journal_crash_child_process_probe --nocapture
```

For checkpoint-boundary runs, set `RUSTWEAKER_CRASH_PROBE_KIND` to `checkpoint`, use a fresh original transaction ID (the probe derives the matching `${transaction}_undo` recovery ID), and choose a checkpoint boundary:

```powershell
$env:RUSTWEAKER_CRASH_PROBE_KIND = 'checkpoint'
$env:RUSTWEAKER_CRASH_PROBE_TRANSACTION = 'tx_vm_checkpoint_0001'
$env:RUSTWEAKER_CRASH_PROBE_BOUNDARY = 'CheckpointMarkerPublish'
$env:RUSTWEAKER_CRASH_PROBE_MODE = 'arm'
cargo test --locked --lib --features experimental-ui -- --exact data::file_store::tests::journal_crash_child_process_probe --nocapture

# Hard-power-off and restart the VM, then verify the same state.
$env:RUSTWEAKER_CRASH_PROBE_MODE = 'verify'
cargo test --locked --lib --features experimental-ui -- --exact data::file_store::tests::journal_crash_child_process_probe --nocapture
```

The `prepare` root is reused across runs; use a new transaction ID each run.

For rotation runs, set kind to `rotation` in both `arm` and `verify`, use a fresh transaction ID, and choose one of the ten `Checkpoint...` boundaries. For example, use `tx_vm_rotation_0001` and `CheckpointMarkerPublish`. The first checkpoint generation is committed before the selected boundary fires on generation 1.

The 21 distinct accepted names are `RecordCreate`, `RecordWrite`, `RecordFlush`, `RecordSyncFlush`, `RecordReadback`, `CommitMarkerCreate`, `CommitMarkerWrite`, `CommitMarkerFlush`, `CommitMarkerReadback`, `CommitMarkerPublish`, `CommitMarkerFinalReadback`, `CheckpointCreate`, `CheckpointWrite`, `CheckpointFlush`, `CheckpointReadback`, `CheckpointMarkerCreate`, `CheckpointMarkerWrite`, `CheckpointMarkerFlush`, `CheckpointMarkerReadback`, `CheckpointMarkerPublish`, and `CheckpointMarkerFinalReadback`. The host test exercises 31 combinations because the ten checkpoint boundaries each run for both initial publication and recovery-ID rotation. `RecordCreate` expects no record/tail; other record boundaries before marker publication expect an incomplete tail; `CommitMarkerFinalReadback` expects one committed record. For initial checkpoint and rotation runs, cuts before final marker readback retain generation 0 as the published generation and resume its progress/recovery ID; final marker readback publishes generation 1 and its new recovery ID. `192.0.2.10` is a documentation-only address; replace it with the isolated controller's reachable address.

Hard-powering off a guest validates guest process/OS recovery through a hypervisor virtual disk. It does not prove loss of host page cache or hypervisor/controller/device volatile write cache. Do not label this matrix “physical power-loss proof.”

## Storage-stack power-loss matrix

Purpose: test the durability claim for a defined physical or explicitly cache-emulating storage stack.

- Use an isolated test host/device or a storage fault-injection rig whose documentation establishes which volatile caches are actually discarded. A host reboot, guest hard-off, VM snapshot revert, or terminating the journal process is not sufficient evidence by itself.
- Record the exact OS/NTFS build, driver, controller, device model/firmware, write-cache settings, power-loss protection, and test rig. Keep test data on a disposable volume; never point the harness at the developer's system journal or real tweak targets.
- Run the same named-boundary workload and post-restart recovery verifier as the guest matrix, with a real power interruption at each publication boundary. Repeat from a clean baseline and retain raw logs and disk images for any failure.
- A failure, unclassified state, or mismatch between the commit marker and recovered record blocks the durability claim. Preserve data for diagnosis; never silently repair or mark the journal restored.

## Environment finding for this checkout

The host reports `D:` as fixed NTFS, and the code/test path is executable there. Hyper-V cmdlets are present, but `Get-VM` is denied by the current authorization policy. `VBoxManage`, `vmrun`, `qemu-system-x86_64`, and `qemu-img` were not found on `PATH` or the checked standard install directories. The optional-feature query also required elevation. No VM inventory, Windows 7 image, guest hard-off test, or storage power interruption was verified in this turn.

## Release decision

Keep all UI/CLI system-changing entrypoints disabled. Close the guest crash matrix and Win7 VM compatibility separately from the physical-storage power-loss claim. If no controlled physical storage interruption rig is available, keep the power-loss limitation explicit; API write-through/flush requests and green host tests do not establish behavior of the complete filesystem/controller/device stack.
