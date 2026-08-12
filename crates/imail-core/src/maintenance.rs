use std::error::Error;

use imail_protocol::{DataBackupResult, DataRestoreResult};

use crate::ApplicationError;

pub trait DataMaintenancePort {
    type Error: Error + Send + Sync + 'static;

    fn create_backup(
        &self,
        data_root: &str,
        backup_root: &str,
        service_version: &str,
        created_at: &str,
    ) -> Result<DataBackupResult, Self::Error>;

    fn prepare_restore(
        &self,
        backup_root: &str,
        restore_root: &str,
    ) -> Result<DataRestoreResult, Self::Error>;
}

pub struct DataMaintenanceService<'a, P: DataMaintenancePort> {
    port: &'a P,
}

impl<'a, P: DataMaintenancePort> DataMaintenanceService<'a, P> {
    pub fn new(port: &'a P) -> Self {
        Self { port }
    }

    pub fn create_backup(
        &self,
        data_root: &str,
        backup_root: &str,
        service_version: &str,
        created_at: &str,
    ) -> Result<DataBackupResult, ApplicationError<P::Error>> {
        self.port
            .create_backup(data_root, backup_root, service_version, created_at)
            .map_err(ApplicationError::Repository)
    }

    pub fn prepare_restore(
        &self,
        backup_root: &str,
        restore_root: &str,
    ) -> Result<DataRestoreResult, ApplicationError<P::Error>> {
        self.port
            .prepare_restore(backup_root, restore_root)
            .map_err(ApplicationError::Repository)
    }
}
