//! Service status/configuration queries and guarded typed start-type mutation.
use serde::Serialize;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_SERVICE_DOES_NOT_EXIST, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW,
    QueryServiceStatusEx, QUERY_SERVICE_CONFIGW, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_STATUS_PROCESS_INFO, SERVICE_CHANGE_CONFIG, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG,
    SERVICE_QUERY_STATUS, SERVICE_STATUS_PROCESS,
};

struct OwnedService(SC_HANDLE);
impl Drop for OwnedService {
    fn drop(&mut self) {
        // SAFETY: successful SCM/open-service calls return an owned handle;
        // this guard uniquely owns it and closes it exactly once.
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum QueryResult {
    Missing,
    Present {
        state: u32,
        service_type: u32,
        process_id: u32,
        win32_exit_code: u32,
        service_exit_code: u32,
    },
}
#[derive(Debug)]
pub enum QueryError {
    InvalidName,
    OpenManager(u32),
    OpenService(u32),
    Query(u32),
    InvalidHandle,
}

fn open_service(name: &str, access: u32) -> Result<Option<OwnedService>, QueryError> {
    if name.is_empty() || name.contains('\0') {
        return Err(QueryError::InvalidName);
    }
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: null arguments select the local machine and active database.
    let raw = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) };
    if raw.is_null() {
        return Err(QueryError::OpenManager(unsafe { GetLastError() }));
    }
    if raw == INVALID_HANDLE_VALUE {
        return Err(QueryError::InvalidHandle);
    }
    let manager = OwnedService(raw);
    // SAFETY: manager is a live SCM handle, wide is terminated, access is query-only.
    let raw = unsafe { OpenServiceW(manager.0, wide.as_ptr(), access) };
    if raw.is_null() {
        // SAFETY: GetLastError is called immediately after the failing Win32 call.
        let code = unsafe { GetLastError() };
        return if code == ERROR_SERVICE_DOES_NOT_EXIST {
            Ok(None)
        } else {
            Err(QueryError::OpenService(code))
        };
    }
    if raw == INVALID_HANDLE_VALUE {
        return Err(QueryError::InvalidHandle);
    }
    Ok(Some(OwnedService(raw)))
}

/// Query a local service by its internal service name without modifying it.
pub fn query(name: &str) -> Result<QueryResult, QueryError> {
    let service = match open_service(name, SERVICE_QUERY_STATUS)? {
        Some(service) => service,
        None => return Ok(QueryResult::Missing),
    };
    let mut status = SERVICE_STATUS_PROCESS {
        dwServiceType: 0,
        dwCurrentState: 0,
        dwControlsAccepted: 0,
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 0,
        dwProcessId: 0,
        dwServiceFlags: 0,
    };
    let mut needed = 0u32;
    // SAFETY: the buffer points to a correctly aligned writable status struct
    // and its actual byte size is supplied; service is alive for this call.
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(QueryError::Query(unsafe { GetLastError() }));
    }
    Ok(QueryResult::Present {
        state: status.dwCurrentState,
        service_type: status.dwServiceType,
        process_id: status.dwProcessId,
        win32_exit_code: status.dwWin32ExitCode,
        service_exit_code: status.dwServiceSpecificExitCode,
    })
}
/// Selected numeric configuration fields, not a complete reversible snapshot.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConfigResult {
    Missing,
    Present {
        service_type: u32,
        start_type: u32,
        error_control: u32,
    },
}

/// Read service start type and related numeric settings without modifying them.
pub fn query_config(name: &str) -> Result<ConfigResult, QueryError> {
    let service = match open_service(name, SERVICE_QUERY_CONFIG)? {
        Some(service) => service,
        None => return Ok(ConfigResult::Missing),
    };
    // QueryServiceConfigW supports up to 8 KiB. usize provides sufficient
    // alignment for QUERY_SERVICE_CONFIGW on both supported Windows targets.
    const BUFFER_BYTES: usize = 8192;
    let mut buffer = vec![0usize; BUFFER_BYTES / std::mem::size_of::<usize>()];
    let config = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    let mut needed = 0u32;
    // SAFETY: the buffer is aligned, writable for BUFFER_BYTES, and remains
    // alive through the call and numeric field reads. Embedded pointers are not read.
    if unsafe { QueryServiceConfigW(service.0, config, BUFFER_BYTES as u32, &mut needed) } == 0 {
        return Err(QueryError::Query(unsafe { GetLastError() }));
    }
    // SAFETY: successful QueryServiceConfigW initialized the struct at config.
    let data = unsafe { &*config };
    Ok(ConfigResult::Present {
        service_type: data.dwServiceType,
        start_type: data.dwStartType,
        error_control: data.dwErrorControl,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum MutationError {
    InvalidName,
    MissingService,
    OpenManager(u32),
    OpenService(u32),
    InvalidHandle,
    Query(u32),
    Change(u32),
    Conflict,
    VerificationMismatch,
}

fn mutation_service(name: &str) -> Result<OwnedService, MutationError> {
    match open_service(name, SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG) {
        Ok(Some(service)) => Ok(service),
        Ok(None) => Err(MutationError::MissingService),
        Err(QueryError::InvalidName) => Err(MutationError::InvalidName),
        Err(QueryError::OpenManager(code)) => Err(MutationError::OpenManager(code)),
        Err(QueryError::OpenService(code)) => Err(MutationError::OpenService(code)),
        Err(QueryError::Query(code)) => Err(MutationError::Query(code)),
        Err(QueryError::InvalidHandle) => Err(MutationError::InvalidHandle),
    }
}

/// Small SCM boundary so transaction and recovery behavior can be exercised
/// without changing a real service.
pub(crate) trait ScmApi {
    fn query_start_type(&mut self, name: &str) -> Result<u32, MutationError>;
    fn change_start_type(&mut self, name: &str, start_type: u32) -> Result<(), MutationError>;
}

#[derive(Default)]
struct WindowsScm;

impl ScmApi for WindowsScm {
    fn query_start_type(&mut self, name: &str) -> Result<u32, MutationError> {
        let service = mutation_service(name)?;
        // QueryServiceConfigW supports up to 8 KiB. `usize` elements provide
        // sufficient alignment for QUERY_SERVICE_CONFIGW on x86 and x64.
        const BUFFER_BYTES: usize = 8192;
        let mut buffer = vec![0usize; BUFFER_BYTES / std::mem::size_of::<usize>()];
        let config = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
        let mut needed = 0u32;
        // SAFETY: the aligned writable buffer is alive for the call and is large
        // enough for the documented maximum configuration structure.
        if unsafe { QueryServiceConfigW(service.0, config, BUFFER_BYTES as u32, &mut needed) } == 0
        {
            return Err(MutationError::Query(unsafe { GetLastError() }));
        }
        // SAFETY: QueryServiceConfigW succeeded and initialized the structure.
        Ok(unsafe { (*config).dwStartType })
    }

    fn change_start_type(&mut self, name: &str, start_type: u32) -> Result<(), MutationError> {
        let service = mutation_service(name)?;
        // Only dwStartType is changed. The Win32 contract specifies
        // SERVICE_NO_CHANGE for unrelated scalar fields and NULL for pointers
        // that are not being modified.
        // SAFETY: service is open with SERVICE_CHANGE_CONFIG; null pointers and
        // SERVICE_NO_CHANGE preserve all other configuration fields.
        if unsafe {
            ChangeServiceConfigW(
                service.0,
                SERVICE_NO_CHANGE,
                start_type,
                SERVICE_NO_CHANGE,
                null(),
                null(),
                null_mut(),
                null(),
                null(),
                null(),
                null(),
            )
        } == 0
        {
            return Err(MutationError::Change(unsafe { GetLastError() }));
        }
        Ok(())
    }
}

/// Service-only backend used by the typed Windows backend and mock SCM tests.
struct ScmBackend<A> {
    api: A,
    observed: Option<(crate::data::journal::ServiceTarget, u32)>,
}

impl<A> ScmBackend<A> {
    fn new(api: A) -> Self {
        Self {
            api,
            observed: None,
        }
    }
}

impl<A: ScmApi> crate::engine::transaction::Backend for ScmBackend<A> {
    type Target = crate::data::journal::ServiceTarget;
    type Change = u32;
    type Snapshot = u32;
    type Error = MutationError;

    fn query(&mut self, target: &Self::Target) -> Result<Self::Snapshot, Self::Error> {
        let start_type = self.api.query_start_type(&target.name)?;
        self.observed = Some((target.clone(), start_type));
        Ok(start_type)
    }

    fn apply(&mut self, target: &Self::Target, change: &Self::Change) -> Result<(), Self::Error> {
        let Some((observed_target, before)) = self.observed.take() else {
            return Err(MutationError::Conflict);
        };
        if observed_target != *target || self.api.query_start_type(&target.name)? != before {
            return Err(MutationError::Conflict);
        }
        self.api.change_start_type(&target.name, *change)?;
        let actual = self.api.query_start_type(&target.name)?;
        self.observed = Some((target.clone(), actual));
        if actual == *change {
            Ok(())
        } else {
            Err(MutationError::VerificationMismatch)
        }
    }
}

impl<A: ScmApi> crate::engine::recovery::RecoveryBackend for ScmBackend<A> {
    type Target = crate::data::journal::ServiceTarget;
    type State = u32;
    type Error = MutationError;

    fn query(&mut self, target: &Self::Target) -> Result<Self::State, Self::Error> {
        let start_type = self.api.query_start_type(&target.name)?;
        self.observed = Some((target.clone(), start_type));
        Ok(start_type)
    }

    fn restore(&mut self, target: &Self::Target, state: &Self::State) -> Result<(), Self::Error> {
        let Some((observed_target, current)) = self.observed.take() else {
            return Err(MutationError::Conflict);
        };
        if observed_target != *target || self.api.query_start_type(&target.name)? != current {
            return Err(MutationError::Conflict);
        }
        self.api.change_start_type(&target.name, *state)
    }
}

pub(crate) struct TypedScmBackend<A> {
    service: ScmBackend<A>,
}

impl<A> TypedScmBackend<A> {
    pub(crate) fn new(api: A) -> Self {
        Self {
            service: ScmBackend::new(api),
        }
    }
}

impl<A: ScmApi> crate::engine::transaction::Backend for TypedScmBackend<A> {
    type Target = crate::data::store::TypedTarget;
    type Change = crate::data::store::TypedState;
    type Snapshot = crate::data::store::TypedState;
    type Error = TypedBackendError;

    fn query(&mut self, target: &Self::Target) -> Result<Self::Snapshot, Self::Error> {
        match target {
            crate::data::store::TypedTarget::Service(target) => {
                crate::engine::transaction::Backend::query(&mut self.service, target)
                    .map(crate::data::store::TypedState::Service)
                    .map_err(TypedBackendError::Service)
            }
            crate::data::store::TypedTarget::Registry(_) => {
                Err(TypedBackendError::InvalidResourceKind)
            }
        }
    }

    fn apply(&mut self, target: &Self::Target, change: &Self::Change) -> Result<(), Self::Error> {
        match (target, change) {
            (
                crate::data::store::TypedTarget::Service(target),
                crate::data::store::TypedState::Service(change),
            ) => crate::engine::transaction::Backend::apply(&mut self.service, target, change)
                .map_err(TypedBackendError::Service),
            _ => Err(TypedBackendError::InvalidResourceKind),
        }
    }
}

impl<A: ScmApi> crate::engine::recovery::TypedRecoveryBackend for TypedScmBackend<A> {
    type Error = TypedBackendError;

    fn query_typed(
        &mut self,
        target: &crate::data::store::TypedTarget,
    ) -> Result<crate::data::store::TypedState, Self::Error> {
        crate::engine::transaction::Backend::query(self, target)
    }

    fn restore_typed(
        &mut self,
        target: &crate::data::store::TypedTarget,
        state: &crate::data::store::TypedState,
    ) -> Result<(), Self::Error> {
        match (target, state) {
            (
                crate::data::store::TypedTarget::Service(target),
                crate::data::store::TypedState::Service(state),
            ) => {
                crate::engine::recovery::RecoveryBackend::restore(&mut self.service, target, state)
                    .map_err(TypedBackendError::Service)
            }
            _ => Err(TypedBackendError::InvalidResourceKind),
        }
    }
}

/// Typed backend for existing-key registry values and existing service start
/// types. Construction is restricted to the managed file sessions, which hold
/// the shared mutation barrier while these methods run.
pub(crate) struct WindowsTypedBackend<'guard> {
    registry: crate::engine::registry::windows::WindowsRegistryBackend<'guard>,
    service: TypedScmBackend<WindowsScm>,
    _guard: &'guard crate::data::file_store::RegistryMutationLock,
}

impl<'guard> WindowsTypedBackend<'guard> {
    pub(crate) fn new(guard: &'guard crate::data::file_store::RegistryMutationLock) -> Self {
        Self {
            registry: crate::engine::registry::windows::WindowsRegistryBackend::new(guard),
            service: TypedScmBackend::new(WindowsScm),
            _guard: guard,
        }
    }

    fn query_resource(
        &mut self,
        target: &crate::data::store::TypedTarget,
    ) -> Result<crate::data::store::TypedState, TypedBackendError> {
        use crate::data::store::{TypedState, TypedTarget};
        match target {
            TypedTarget::Registry(target) => {
                crate::engine::transaction::Backend::query(&mut self.registry, target)
                    .map(TypedState::Registry)
                    .map_err(TypedBackendError::Registry)
            }
            TypedTarget::Service(target) => crate::engine::transaction::Backend::query(
                &mut self.service,
                &TypedTarget::Service(target.clone()),
            ),
        }
    }

    fn set_resource(
        &mut self,
        target: &crate::data::store::TypedTarget,
        state: &crate::data::store::TypedState,
    ) -> Result<(), TypedBackendError> {
        use crate::data::store::{TypedState, TypedTarget};
        match (target, state) {
            (TypedTarget::Registry(target), TypedState::Registry(state)) => {
                crate::engine::recovery::RecoveryBackend::restore(&mut self.registry, target, state)
                    .map_err(TypedBackendError::Registry)
            }
            (TypedTarget::Service(target), TypedState::Service(start_type)) => {
                crate::engine::recovery::TypedRecoveryBackend::restore_typed(
                    &mut self.service,
                    &TypedTarget::Service(target.clone()),
                    &TypedState::Service(*start_type),
                )
            }
            _ => Err(TypedBackendError::InvalidResourceKind),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TypedBackendError {
    Registry(crate::engine::registry::windows::WriteError),
    Service(MutationError),
    InvalidResourceKind,
    VerificationMismatch,
}

impl crate::engine::transaction::Backend for WindowsTypedBackend<'_> {
    type Target = crate::data::store::TypedTarget;
    type Change = crate::data::store::TypedState;
    type Snapshot = crate::data::store::TypedState;
    type Error = TypedBackendError;

    fn query(&mut self, target: &Self::Target) -> Result<Self::Snapshot, Self::Error> {
        self.query_resource(target)
    }

    fn apply(&mut self, target: &Self::Target, change: &Self::Change) -> Result<(), Self::Error> {
        self.set_resource(target, change)?;
        if self.query_resource(target)? == *change {
            Ok(())
        } else {
            Err(TypedBackendError::VerificationMismatch)
        }
    }
}

impl crate::engine::recovery::TypedRecoveryBackend for WindowsTypedBackend<'_> {
    type Error = TypedBackendError;

    fn query_typed(
        &mut self,
        target: &crate::data::store::TypedTarget,
    ) -> Result<crate::data::store::TypedState, Self::Error> {
        self.query_resource(target)
    }

    fn restore_typed(
        &mut self,
        target: &crate::data::store::TypedTarget,
        state: &crate::data::store::TypedState,
    ) -> Result<(), Self::Error> {
        self.set_resource(target, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::transaction::{Operation, SnapshotStore, TransactionError};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Clone, Copy)]
    enum ChangeBehavior {
        Succeed,
        FailBeforeChange,
        FailAfterChange,
        Ignore,
    }

    struct MockScm {
        state: Rc<Cell<u32>>,
        events: Rc<RefCell<Vec<&'static str>>>,
        query_fails: bool,
        change_behavior: ChangeBehavior,
        backup_marker: Option<std::path::PathBuf>,
        change_saw_backup: Option<Rc<Cell<bool>>>,
    }

    impl ScmApi for MockScm {
        fn query_start_type(&mut self, _: &str) -> Result<u32, MutationError> {
            self.events.borrow_mut().push("query");
            if self.query_fails {
                Err(MutationError::Query(5))
            } else {
                Ok(self.state.get())
            }
        }

        fn change_start_type(&mut self, _: &str, start_type: u32) -> Result<(), MutationError> {
            self.events.borrow_mut().push("change");
            if let (Some(marker), Some(saw_backup)) = (&self.backup_marker, &self.change_saw_backup)
            {
                saw_backup.set(marker.is_file());
            }
            match self.change_behavior {
                ChangeBehavior::Succeed => self.state.set(start_type),
                ChangeBehavior::FailBeforeChange => return Err(MutationError::Change(5)),
                ChangeBehavior::FailAfterChange => {
                    self.state.set(start_type);
                    return Err(MutationError::Change(5));
                }
                ChangeBehavior::Ignore => {}
            }
            Ok(())
        }
    }

    struct MockSnapshots {
        events: Rc<RefCell<Vec<&'static str>>>,
        records: Vec<(u32, u32)>,
        fail: bool,
        external_change: Option<(Rc<Cell<u32>>, u32)>,
    }

    impl SnapshotStore<crate::data::journal::ServiceTarget, u32, u32> for MockSnapshots {
        type Error = &'static str;

        fn persist(
            &mut self,
            _: &crate::data::journal::ServiceTarget,
            before: &u32,
            intended: &u32,
        ) -> Result<(), Self::Error> {
            self.events.borrow_mut().push("persist");
            if self.fail {
                return Err("snapshot failed");
            }
            self.records.push((*before, *intended));
            if let Some((state, external_value)) = &self.external_change {
                state.set(*external_value);
            }
            Ok(())
        }
    }

    type MockParts = (
        ScmBackend<MockScm>,
        MockSnapshots,
        Rc<Cell<u32>>,
        Rc<RefCell<Vec<&'static str>>>,
    );

    fn mock_parts(state: u32, change_behavior: ChangeBehavior) -> MockParts {
        let state = Rc::new(Cell::new(state));
        let events = Rc::new(RefCell::new(Vec::new()));
        let backend = ScmBackend::new(MockScm {
            state: Rc::clone(&state),
            events: Rc::clone(&events),
            query_fails: false,
            change_behavior,
            backup_marker: None,
            change_saw_backup: None,
        });
        let snapshots = MockSnapshots {
            events: Rc::clone(&events),
            records: Vec::new(),
            fail: false,
            external_change: None,
        };
        (backend, snapshots, state, events)
    }

    fn service_target() -> crate::data::journal::ServiceTarget {
        crate::data::journal::ServiceTarget {
            name: "ExampleService".into(),
        }
    }

    struct TestJournalRoot(std::path::PathBuf);

    impl TestJournalRoot {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let profile =
                std::env::var_os("USERPROFILE").expect("USERPROFILE is required for journal tests");
            let path = std::path::PathBuf::from(profile)
                .join(format!("rustweaker-scm-test-{}-{id}", std::process::id()));
            crate::data::file_store::FileJournalStorage::create_private(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestJournalRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn typed_file_session_commits_service_backup_before_mock_scm_change() {
        use crate::data::file_store::{FileJournalStorage, FileTypedTransactionError};
        use crate::data::store::{JournalStorage, TypedState, TypedTarget};

        let root = TestJournalRoot::new();
        let transaction = "tx_mock_scm";
        let marker = root.0.join(format!(
            "{:03}-{transaction}-{:020}.commit",
            transaction.len(),
            0
        ));
        let saw_backup = Rc::new(Cell::new(false));
        let state = Rc::new(Cell::new(3));
        let events = Rc::new(RefCell::new(Vec::new()));
        let backend = MockScm {
            state: Rc::clone(&state),
            events: Rc::clone(&events),
            query_fails: false,
            change_behavior: ChangeBehavior::FailAfterChange,
            backup_marker: Some(marker),
            change_saw_backup: Some(Rc::clone(&saw_backup)),
        };
        let mut backend = TypedScmBackend::new(backend);
        let mut session =
            FileJournalStorage::start_typed_transaction(&root.0, transaction).unwrap();
        let operation = Operation {
            target: TypedTarget::Service(service_target()),
            change: TypedState::Service(4),
        };

        assert!(matches!(
            session.apply(&mut backend, &operation),
            Err(FileTypedTransactionError::Operation(
                TransactionError::Apply(TypedBackendError::Service(MutationError::Change(5)))
            ))
        ));
        assert!(saw_backup.get());
        assert_eq!(state.get(), 4);
        assert_eq!(events.borrow().as_slice(), ["query", "query", "change"]);
        drop(session);

        let mut storage = FileJournalStorage::open(&root.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let scan = storage.scan(transaction).unwrap();
        assert_eq!(scan.typed_committed.len(), 1);
        assert!(matches!(
            &scan.typed_committed[0],
            crate::data::journal::TransactionRecord::Service(record)
                if record.before_start_type == 3 && record.intended_start_type == 4
        ));
    }

    #[test]
    fn typed_recovery_session_reopens_after_mock_scm_failure() {
        use crate::data::file_store::{FileJournalStorage, FileTypedRecoverySession};
        use crate::data::store::{JournalStorage, TypedState, TypedTarget};

        let root = TestJournalRoot::new();
        let transaction = "tx_mock_recovery";
        let recovery_id = "tx_mock_recovery_backup";
        let state = Rc::new(Cell::new(3));
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = TypedScmBackend::new(MockScm {
            state: Rc::clone(&state),
            events: Rc::clone(&events),
            query_fails: false,
            change_behavior: ChangeBehavior::Succeed,
            backup_marker: None,
            change_saw_backup: None,
        });
        let operation = Operation {
            target: TypedTarget::Service(service_target()),
            change: TypedState::Service(4),
        };
        let mut transaction_session =
            FileJournalStorage::start_typed_transaction(&root.0, transaction).unwrap();
        transaction_session.apply(&mut backend, &operation).unwrap();
        drop(transaction_session);
        assert_eq!(state.get(), 4);

        backend.service.api.change_behavior = ChangeBehavior::FailBeforeChange;
        let saw_recovery_backup = Rc::new(Cell::new(false));
        backend.service.api.backup_marker = Some(root.0.join(format!(
            "{:03}-{recovery_id}-{:020}.commit",
            recovery_id.len(),
            0
        )));
        backend.service.api.change_saw_backup = Some(Rc::clone(&saw_recovery_backup));
        let mut recovery =
            FileTypedRecoverySession::start_with_id(&root.0, transaction, recovery_id).unwrap();
        assert!(recovery.run(&mut backend).is_err());
        assert!(saw_recovery_backup.get());
        assert_eq!(state.get(), 4);
        assert_eq!(recovery.next_exclusive(), 1);
        drop(recovery);

        backend.service.api.change_behavior = ChangeBehavior::Succeed;
        let mut recovery = FileTypedRecoverySession::resume(&root.0, transaction).unwrap();
        assert_eq!(recovery.recovery_transaction_id(), recovery_id);
        recovery.run(&mut backend).unwrap();
        assert_eq!(state.get(), 3);
        assert_eq!(recovery.next_exclusive(), 0);
        drop(recovery);

        let mut recovery_storage = FileJournalStorage::open(&root.0)
            .unwrap()
            .lock_transaction(recovery_id)
            .unwrap();
        let scan = recovery_storage.scan(recovery_id).unwrap();
        assert_eq!(scan.typed_committed.len(), 2);
        assert!(scan.typed_committed.iter().all(|record| matches!(
            record,
            crate::data::journal::TransactionRecord::Service(record)
                if record.before_start_type == 4 && record.intended_start_type == 3
        )));
    }

    #[test]
    fn transaction_persists_before_change_and_verifies_success() {
        let (mut backend, mut snapshots, state, events) = mock_parts(3, ChangeBehavior::Succeed);
        let operation = Operation {
            target: service_target(),
            change: 4,
        };

        crate::engine::transaction::apply_one(&mut backend, &mut snapshots, &operation).unwrap();

        assert_eq!(
            events.borrow().as_slice(),
            ["query", "persist", "query", "change", "query"]
        );
        assert_eq!(snapshots.records, [(3, 4)]);
        assert_eq!(state.get(), 4);
    }

    #[test]
    fn snapshot_failure_prevents_scm_change() {
        let (mut backend, mut snapshots, state, events) = mock_parts(3, ChangeBehavior::Succeed);
        snapshots.fail = true;
        let operation = Operation {
            target: service_target(),
            change: 4,
        };

        assert!(matches!(
            crate::engine::transaction::apply_one(&mut backend, &mut snapshots, &operation),
            Err(TransactionError::Persist("snapshot failed"))
        ));
        assert_eq!(events.borrow().as_slice(), ["query", "persist"]);
        assert_eq!(state.get(), 3);
    }

    #[test]
    fn query_failure_prevents_snapshot_and_scm_change() {
        let (mut backend, mut snapshots, state, events) = mock_parts(3, ChangeBehavior::Succeed);
        backend.api.query_fails = true;
        let operation = Operation {
            target: service_target(),
            change: 4,
        };

        assert!(matches!(
            crate::engine::transaction::apply_one(&mut backend, &mut snapshots, &operation),
            Err(TransactionError::Query(MutationError::Query(5)))
        ));
        assert_eq!(events.borrow().as_slice(), ["query"]);
        assert!(snapshots.records.is_empty());
        assert_eq!(state.get(), 3);
    }

    #[test]
    fn partial_scm_failure_keeps_committed_pre_state_for_recovery() {
        let (mut backend, mut snapshots, state, events) =
            mock_parts(3, ChangeBehavior::FailAfterChange);
        let operation = Operation {
            target: service_target(),
            change: 4,
        };

        assert!(matches!(
            crate::engine::transaction::apply_one(&mut backend, &mut snapshots, &operation),
            Err(TransactionError::Apply(MutationError::Change(5)))
        ));
        assert_eq!(
            events.borrow().as_slice(),
            ["query", "persist", "query", "change"]
        );
        assert_eq!(snapshots.records, [(3, 4)]);
        assert_eq!(state.get(), 4);
    }

    #[test]
    fn external_change_after_snapshot_is_detected_before_scm_write() {
        let (mut backend, mut snapshots, state, events) = mock_parts(3, ChangeBehavior::Succeed);
        snapshots.external_change = Some((Rc::clone(&state), 2));
        let operation = Operation {
            target: service_target(),
            change: 4,
        };

        assert!(matches!(
            crate::engine::transaction::apply_one(&mut backend, &mut snapshots, &operation),
            Err(TransactionError::Apply(MutationError::Conflict))
        ));
        assert_eq!(events.borrow().as_slice(), ["query", "persist", "query"]);
        assert_eq!(snapshots.records, [(3, 4)]);
        assert_eq!(state.get(), 2);
    }

    #[test]
    fn recovery_conflict_does_not_snapshot_or_overwrite_external_state() {
        let (mut backend, mut snapshots, state, events) = mock_parts(2, ChangeBehavior::Succeed);
        let record = crate::engine::recovery::RecoveryRecord {
            target: service_target(),
            before: 3,
            intended: 4,
        };

        assert!(matches!(
            crate::engine::recovery::recover_one(&mut backend, &mut snapshots, &record),
            Err(crate::engine::recovery::RecoveryError::Conflict)
        ));
        assert_eq!(events.borrow().as_slice(), ["query"]);
        assert_eq!(state.get(), 2);
    }

    #[test]
    fn recovery_snapshot_failure_prevents_service_restore() {
        let (mut backend, mut snapshots, state, events) = mock_parts(4, ChangeBehavior::Succeed);
        snapshots.fail = true;
        let record = crate::engine::recovery::RecoveryRecord {
            target: service_target(),
            before: 3,
            intended: 4,
        };

        assert!(matches!(
            crate::engine::recovery::recover_one(&mut backend, &mut snapshots, &record),
            Err(crate::engine::recovery::RecoveryError::Persist(
                "snapshot failed"
            ))
        ));
        assert_eq!(events.borrow().as_slice(), ["query", "persist"]);
        assert_eq!(state.get(), 4);
    }

    #[test]
    fn external_change_after_recovery_snapshot_is_not_overwritten() {
        let (mut backend, mut snapshots, state, events) = mock_parts(4, ChangeBehavior::Succeed);
        snapshots.external_change = Some((Rc::clone(&state), 2));
        let record = crate::engine::recovery::RecoveryRecord {
            target: service_target(),
            before: 3,
            intended: 4,
        };

        assert!(matches!(
            crate::engine::recovery::recover_one(&mut backend, &mut snapshots, &record),
            Err(crate::engine::recovery::RecoveryError::Restore(
                MutationError::Conflict
            ))
        ));
        assert_eq!(events.borrow().as_slice(), ["query", "persist", "query"]);
        assert_eq!(state.get(), 2);
    }

    #[test]
    fn restore_failure_is_retryable_and_verification_runs_after_restore() {
        let (mut backend, mut snapshots, state, events) =
            mock_parts(4, ChangeBehavior::FailBeforeChange);
        let record = crate::engine::recovery::RecoveryRecord {
            target: service_target(),
            before: 3,
            intended: 4,
        };

        assert!(matches!(
            crate::engine::recovery::recover_one(&mut backend, &mut snapshots, &record),
            Err(crate::engine::recovery::RecoveryError::Restore(
                MutationError::Change(5)
            ))
        ));
        assert_eq!(state.get(), 4);

        backend.api.change_behavior = ChangeBehavior::Succeed;
        assert_eq!(
            crate::engine::recovery::recover_one(&mut backend, &mut snapshots, &record),
            Ok(crate::engine::recovery::RecoveryOutcome::Restored)
        );
        assert_eq!(state.get(), 3);
        assert_eq!(
            events.borrow().as_slice(),
            [
                "query", "persist", "query", "change", "query", "persist", "query", "change",
                "query"
            ]
        );
    }

    #[test]
    fn successful_scm_call_with_wrong_state_fails_verification() {
        let (mut backend, mut snapshots, state, events) = mock_parts(3, ChangeBehavior::Ignore);
        let operation = Operation {
            target: service_target(),
            change: 4,
        };

        assert!(matches!(
            crate::engine::transaction::apply_one(&mut backend, &mut snapshots, &operation),
            Err(TransactionError::Apply(MutationError::VerificationMismatch))
        ));
        assert_eq!(
            events.borrow().as_slice(),
            ["query", "persist", "query", "change", "query"]
        );
        assert_eq!(state.get(), 3);
    }

    #[test]
    fn rejects_empty_and_embedded_nul() {
        assert!(matches!(query(""), Err(QueryError::InvalidName)));
        assert!(matches!(
            query("invalid\0name"),
            Err(QueryError::InvalidName)
        ));
        assert!(matches!(
            mutation_service(""),
            Err(MutationError::InvalidName)
        ));
        assert!(matches!(
            mutation_service("invalid\0name"),
            Err(MutationError::InvalidName)
        ));
    }
    #[test]
    fn missing_service_is_reported_separately() {
        assert!(matches!(
            query("rustweaker-nonexistent-65e610e8-2427-4b37-922d-177df12c9f99"),
            Ok(QueryResult::Missing)
        ));
    }
    #[test]
    fn service_config_reports_start_type_and_missing_service() {
        assert!(matches!(
            query_config("EventLog"),
            Ok(ConfigResult::Present {
                start_type: 0..=4,
                ..
            })
        ));
        assert!(matches!(
            query_config("rustweaker-nonexistent-65e610e8-2427-4b37-922d-177df12c9f99"),
            Ok(ConfigResult::Missing)
        ));
    }
    #[test]
    fn event_log_can_be_queried_without_changes() {
        assert!(matches!(query("EventLog"), Ok(QueryResult::Present { .. })));
    }
}
