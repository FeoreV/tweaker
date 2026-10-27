# Journal persistence: Windows evidence

Checked 2026-10-27. No disk backend is enabled yet.

Microsoft documents that [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/fileio/flushing-system-buffered-i-o-data-to-disk) requests a flush of buffered file data. The [CreateFile documentation](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea) explains write-through caching, NTFS metadata flushing for changes resulting from a request, and notes that hardware may not support write-through. The [MoveFileEx documentation](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexa) describes MOVEFILE_WRITE_THROUGH, including copy/delete flushing; these statements do not by themselves establish every crash-consistency guarantee for every filesystem, volume or storage device.

A production implementation must explicitly constrain supported storage, validate failures and interruption behavior on the supported Win7 configurations, protect against reparse points and concurrent writers, and define journal discovery and incomplete files. Until then, no implementation should satisfy the `JournalStorage::sync` contract by assertion alone or enable system mutations.
