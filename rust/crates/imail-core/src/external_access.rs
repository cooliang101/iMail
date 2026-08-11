use serde::{Deserialize, Serialize};

use crate::{AccountRepository, ApplicationError};

const METADATA_KEY: &str = "external_access_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAccessSettings {
    pub gateway_enabled: bool,
    pub mcp_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExternalAccessChanges {
    pub gateway_enabled: Option<bool>,
    pub mcp_enabled: Option<bool>,
}

pub struct ExternalAccessService<'a, R: AccountRepository> {
    repository: &'a mut R,
}

impl<'a, R: AccountRepository> ExternalAccessService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn get(&self, user_id: &str) -> Result<ExternalAccessSettings, ApplicationError<R::Error>> {
        let stored = self
            .repository
            .user_metadata(user_id, METADATA_KEY)
            .map_err(ApplicationError::Repository)?;
        Ok(stored
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    pub fn update(
        &mut self,
        user_id: &str,
        changes: ExternalAccessChanges,
    ) -> Result<ExternalAccessSettings, ApplicationError<R::Error>> {
        if changes.gateway_enabled.is_none() && changes.mcp_enabled.is_none() {
            return Err(ApplicationError::Domain {
                code: "INVALID_EXTERNAL_ACCESS_UPDATE",
                status: 400,
                message: "至少提供一个要更新的外部接入设置",
            });
        }
        let mut current = self.get(user_id)?;
        if let Some(value) = changes.gateway_enabled {
            current.gateway_enabled = value;
        }
        if let Some(value) = changes.mcp_enabled {
            current.mcp_enabled = value;
        }
        let encoded = serde_json::to_string(&current).map_err(|_| ApplicationError::Domain {
            code: "EXTERNAL_ACCESS_SERIALIZATION_FAILED",
            status: 500,
            message: "无法保存外部接入设置",
        })?;
        self.repository
            .set_user_metadata(user_id, METADATA_KEY, &encoded)
            .map_err(ApplicationError::Repository)?;
        Ok(current)
    }
}
