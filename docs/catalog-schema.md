# Declarative catalog schema v1

`src/data/catalog.rs` defines the versioned serde model and byte decoder capped at 1 MiB. `config/catalog.v1.json` is a single source-traced review fixture for `WPFTweaksActivity`, which remains `Unverified`. The fixture is embedded only by its unit test; no production loader, UI/CLI route, or operation executor exists.

## Shape and invariants

- `TweakCatalog` carries a schema version, repository-relative source file path, SHA-256, and unique tweak definitions. Unknown JSON fields are rejected.
- Each definition includes an ID/title/summary, optional upstream ID, explicit support evidence, and one or more operations.
- Support is `unverified` unless an entry explicitly uses `verified` with an NTDDI minimum, optional maximum and nonempty evidence references. The schema does not claim that evidence is true; review must inspect it.
- `TweakDefinition::verified_operations()` returns no operations for `Unverified`, and the operations field is private to the catalog module. This prevents accidental direct reads through that struct API. `Verified` is still deserializable/public metadata and can be supplied by any catalog author; this method is not an authorization boundary or evidence verifier. A trusted loader must independently validate provenance/review policy before treating support as approved.
- The only operation variants are `set_registry_value`, `delete_registry_value`, and `set_service_start_type`. Registry values preserve exact native type and bytes using the existing engine model. Service start types are native SCM integers (0 through 4).
- No fixed `OriginalValue`, rollback value, script, shell command, package install, AppX, file deletion, DNS, or power-plan action can be represented. Rollback state is queried and persisted by the transaction engine at execution time.
- Validation rejects unsupported schema versions, malformed source hashes/paths, invalid or duplicate IDs, empty text/operation lists, invalid operation targets, invalid service start types, unsupported ranges and verified claims without evidence.

The fixture is intentionally not populated from `TWEAKS_INVENTORY.md`: that draft has 24 IDs absent from the frozen catalog, 24 source IDs absent from the draft, and unverified compatibility/undo claims. It copies only the three factual DWORD apply values from the verified upstream record; it does not copy the source's `<RemoveEntry>` pseudo-originals. Future B4 work must select each operation from traceable source facts and add evidence before marking support verified.

## Current boundary

Decoding and validating this fixture does not authorize mutation. There is no code that reads it outside its test. Any future caller must validate trusted support approval rather than trust the `Verified` JSON tag, route operations through managed transaction/recovery sessions, and satisfy the remaining namespace, power-loss, real SCM and Win7 VM release gates in `AGENT_TASKS.md`. Do not expose the schema through UI/CLI until those gates and an explicit product review are complete.
