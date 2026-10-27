# Compatibility and undo claims in the draft inventory

Reviewed 2026-10-28 by joining only exact IDs in `docs/inventory-source-comparison.json` to the frozen `tweaks.json` declarations. This audit asks what the catalog itself can support. It does not establish actual Windows behavior or prove a claim false.

## Compatibility

The 42 exact-ID draft rows make these claims:

| Draft claim | Rows |
|---|---:|
| `Compatible` (including five decorated variants) | 25 |
| `Win10+` | 11 |
| `Win11 only` | 6 |

Recursive inspection of those 42 upstream JSON objects found no explicit OS/version/build support matrix or target-version fields. One combo label says `Disabled (Compatibility)`, but that names a UI choice and does not identify Windows versions. The catalog has labels, categories, registry/service/script declarations and links, but those do not establish supported Windows versions. Scripts and handler code may encode platform assumptions, but they need per-entry inspection and runtime evidence. Therefore, none of the draft compatibility claims is verified by this catalog comparison.

## Undo

Among the 42 exact-ID rows, 39 say `Undo Support: Yes` and 3 say `No`. The source facts show different mechanisms: registry `OriginalValue`, service `OriginalType`, script `UndoScript`, or combinations. These are declared values/actions; the dispatcher does not snapshot the machine's actual pre-change state. A fixed upstream `OriginalValue`/`OriginalType` cannot stand in for the Rustweaker transaction snapshot contract. An `UndoScript` also needs review for symmetry, partial failure and destructive side effects.

One ordinary dispatcher limitation matters for interpreting declarations: registry records with a `Values` property are excluded from the generic registry apply/undo loop and are handled via separate combo UI code. Merely finding such records or their defaults in JSON does not prove that ordinary toggle dispatch applies or restores them.

## Required follow-up before catalog implementation

For every candidate operation, determine exact supported OS/build and architecture from an authoritative source or a disposable-VM observation; inspect all relevant script/helper paths; define rollback from a captured pre-state; and add mocked tests for forward, backup failure, partial failure and restore. Keep actions that cannot meet those requirements unsupported. The generated comparison is a triage aid, not approval to implement the 39 “undo yes” rows.
