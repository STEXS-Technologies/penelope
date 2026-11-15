//! Hosted task I/O delegates to the unchanged synchronous journal on its worker.
use crate::{SqliteTaskJournal, worker::DatabaseWorker};
use async_trait::async_trait;
use penelope_core::task::{
    runtime::{TaskJournal, TaskPortError},
    *,
};
use penelope_ports::{PortError, tasks::AsyncTaskJournal};

/// Bounded asynchronous view of the native task journal. This owns the same
/// journal implementation, cache, validation and transactions on a worker thread.
/// Opening/migration and conversion remain synchronous setup operations.
pub struct SqliteAsyncTaskJournal {
    worker: DatabaseWorker<SqliteTaskJournal>,
    max_blob_bytes: u32,
}
const fn worker_error(error: PortError) -> TaskPortError {
    match error {
        PortError::QuotaExceeded => TaskPortError::Exhausted,
        PortError::Unavailable
        | PortError::Conflict
        | PortError::Unauthorized
        | PortError::TimedOut
        | PortError::Cancelled
        | PortError::Ambiguous
        | PortError::Invariant => TaskPortError::Unavailable,
    }
}
impl SqliteTaskJournal {
    /// Moves this exact journal and its connection into the shared bounded worker.
    /// No second connection or task evaluator is created. A dropped response may
    /// lose an ACK after commit; use authoritative reads to resolve uncertainty.
    /// # Errors
    /// Refuses invalid queue capacity or failure to create the worker thread.
    pub fn into_async(self, queue_capacity: u16) -> Result<SqliteAsyncTaskJournal, TaskPortError> {
        let max_blob_bytes = self.limits.max_blob_bytes;
        Ok(SqliteAsyncTaskJournal {
            worker: DatabaseWorker::new(self, queue_capacity).map_err(worker_error)?,
            max_blob_bytes,
        })
    }
}
impl SqliteAsyncTaskJournal {
    #[must_use]
    pub const fn queue_capacity(&self) -> u16 {
        self.worker.capacity()
    }
}
#[async_trait]
impl AsyncTaskJournal for SqliteAsyncTaskJournal {
    async fn read(
        &self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.worker
            .run(move |journal| Ok(journal.read(scope, sequence)))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn commit(
        &self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        if blob.is_some_and(|bytes| bytes.len() as u128 > u128::from(self.max_blob_bytes)) {
            return Err(TaskPortError::Exhausted);
        }
        let record = *record;
        let blob = blob.map(<[u8]>::to_vec);
        self.worker
            .run(move |journal| Ok(journal.commit(&record, blob.as_deref())))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn read_blob(
        &self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        let len = usize::try_from(blob.bytes).map_err(|_size| TaskPortError::Exhausted)?;
        if blob.bytes > self.max_blob_bytes || len > buffer.len() {
            return Err(TaskPortError::Exhausted);
        }
        let bytes = self
            .worker
            .run(move |journal| {
                let result = (|| {
                    let mut bytes = vec![0; len];
                    if journal.read_blob(scope, blob, &mut bytes)? != len {
                        return Err(TaskPortError::Corrupt);
                    }
                    Ok(bytes)
                })();
                Ok(result)
            })
            .map_err(worker_error)?
            .await
            .map_err(worker_error)??;
        buffer
            .get_mut(..len)
            .ok_or(TaskPortError::Exhausted)?
            .copy_from_slice(&bytes);
        Ok(len)
    }
}

#[cfg(test)]
#[path = "async_task_tests.rs"]
mod tests;
