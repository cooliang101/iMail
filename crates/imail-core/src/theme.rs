use imail_protocol::CustomTheme;

use crate::{AccountRepository, ApplicationError};

const METADATA_KEY: &str = "mcp_custom_theme_v1";

pub fn read_custom_theme<R: AccountRepository>(
    repository: &R,
    user_id: &str,
) -> Result<CustomTheme, ApplicationError<R::Error>> {
    let stored = repository
        .user_metadata(user_id, METADATA_KEY)
        .map_err(ApplicationError::Repository)?;
    Ok(stored
        .as_deref()
        .and_then(|value| serde_json::from_str::<CustomTheme>(value).ok())
        .filter(valid_theme)
        .unwrap_or_default())
}

pub fn update_custom_theme<R: AccountRepository>(
    repository: &mut R,
    user_id: &str,
    mut theme: CustomTheme,
) -> Result<CustomTheme, ApplicationError<R::Error>> {
    theme.name = theme.name.trim().to_string();
    if !valid_theme(&theme) {
        return Err(ApplicationError::Domain {
            code: "CUSTOM_THEME_INVALID",
            status: 400,
            message: "自定义主题字段无效",
        });
    }
    let encoded = serde_json::to_string(&theme).expect("custom theme is serializable");
    repository
        .set_user_metadata(user_id, METADATA_KEY, &encoded)
        .map_err(ApplicationError::Repository)?;
    Ok(theme)
}

pub struct CustomThemeService<'a, R: AccountRepository> {
    repository: &'a mut R,
}

impl<'a, R: AccountRepository> CustomThemeService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn read(&self, user_id: &str) -> Result<CustomTheme, ApplicationError<R::Error>> {
        read_custom_theme(self.repository, user_id)
    }

    pub fn update(
        &mut self,
        user_id: &str,
        theme: CustomTheme,
    ) -> Result<CustomTheme, ApplicationError<R::Error>> {
        update_custom_theme(self.repository, user_id, theme)
    }
}

fn valid_theme(theme: &CustomTheme) -> bool {
    !theme.name.is_empty()
        && theme.name.encode_utf16().count() <= 40
        && [
            &theme.canvas,
            &theme.surface,
            &theme.surface_subtle,
            &theme.rail,
            &theme.text,
            &theme.text_secondary,
            &theme.border,
            &theme.accent,
            &theme.accent_subtle,
        ]
        .into_iter()
        .all(|color| valid_color(color))
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}
