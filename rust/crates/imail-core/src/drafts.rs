use imail_protocol::{DraftInput, DraftReadModel};

use crate::{ApplicationError, LocalRepository};

pub struct DraftService<'a, R: LocalRepository> {
    repository: &'a mut R,
}

impl<'a, R: LocalRepository> DraftService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn list(
        &self,
        user_id: &str,
    ) -> Result<Vec<DraftReadModel>, ApplicationError<<R as crate::AccountRepository>::Error>> {
        self.repository
            .list_drafts(user_id)
            .map_err(ApplicationError::Repository)
    }

    pub fn get(
        &self,
        user_id: &str,
        draft_id: &str,
    ) -> Result<DraftReadModel, ApplicationError<<R as crate::AccountRepository>::Error>> {
        self.list(user_id)?
            .into_iter()
            .find(|draft| draft.id == draft_id)
            .ok_or_else(|| domain("DRAFT_NOT_FOUND", 404, "草稿不存在"))
    }

    pub fn create(
        &mut self,
        user_id: &str,
        draft_id: &str,
        now: &str,
        input: DraftInput,
    ) -> Result<DraftReadModel, ApplicationError<<R as crate::AccountRepository>::Error>> {
        self.require_account(user_id, &input.account_id)?;
        let created_at = self
            .list(user_id)?
            .into_iter()
            .find(|draft| draft.id == draft_id)
            .map_or_else(|| now.to_string(), |draft| draft.created_at);
        let draft = model(draft_id, created_at, now, input);
        self.repository
            .upsert_draft(user_id, &draft)
            .map_err(ApplicationError::Repository)?;
        Ok(draft)
    }

    pub fn save_existing(
        &mut self,
        user_id: &str,
        draft_id: &str,
        now: &str,
        input: DraftInput,
    ) -> Result<DraftReadModel, ApplicationError<<R as crate::AccountRepository>::Error>> {
        self.require_account(user_id, &input.account_id)?;
        let existing = self.get(user_id, draft_id)?;
        let draft = model(draft_id, existing.created_at, now, input);
        self.repository
            .upsert_draft(user_id, &draft)
            .map_err(ApplicationError::Repository)?;
        Ok(draft)
    }

    pub fn delete(
        &mut self,
        user_id: &str,
        draft_id: &str,
    ) -> Result<bool, ApplicationError<<R as crate::AccountRepository>::Error>> {
        self.repository
            .delete_draft(user_id, draft_id)
            .map_err(ApplicationError::Repository)
    }

    fn require_account(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<(), ApplicationError<<R as crate::AccountRepository>::Error>> {
        if self
            .repository
            .account(user_id, account_id)
            .map_err(ApplicationError::Repository)?
            .is_some()
        {
            Ok(())
        } else {
            Err(domain("ACCOUNT_NOT_FOUND", 404, "发件邮箱不存在"))
        }
    }
}

fn model(id: &str, created_at: String, updated_at: &str, input: DraftInput) -> DraftReadModel {
    DraftReadModel {
        id: id.to_string(),
        account_id: input.account_id,
        to: input.to,
        cc: input.cc,
        subject: input.subject,
        text: input.text,
        html: input.html,
        attachments: input.attachments,
        created_at,
        updated_at: updated_at.to_string(),
    }
}

fn domain<E: std::error::Error + Send + Sync + 'static>(
    code: &'static str,
    status: u16,
    message: &'static str,
) -> ApplicationError<E> {
    ApplicationError::Domain {
        code,
        status,
        message,
    }
}
