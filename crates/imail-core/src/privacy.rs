use imail_protocol::ClearMailDataResult;

use crate::{ApplicationError, PrivacyRepository};

pub struct PrivacyService<'a, R: PrivacyRepository> {
    repository: &'a mut R,
}

impl<'a, R: PrivacyRepository> PrivacyService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn clear_mail_data(
        &mut self,
        user_id: &str,
    ) -> Result<ClearMailDataResult, ApplicationError<R::Error>> {
        self.repository
            .clear_user_mail_data(user_id)
            .map_err(ApplicationError::Repository)
    }
}
