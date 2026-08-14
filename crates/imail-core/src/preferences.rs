use imail_protocol::{
    AppPreferences, AppPreferencesPatch, MessageView, NotificationKinds, ShortcutBindings,
    ShortcutBindingsPatch, StartupView, ThemeId,
};
use serde::Deserialize;

use crate::theme::{read_custom_theme, update_custom_theme};
use crate::{AccountRepository, ApplicationError};

const METADATA_KEY: &str = "app_preferences_v1";

pub struct PreferencesService<'a, R: AccountRepository> {
    repository: &'a mut R,
}

impl<'a, R: AccountRepository> PreferencesService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn read(&self, user_id: &str) -> Result<AppPreferences, ApplicationError<R::Error>> {
        let raw = self
            .repository
            .user_metadata(user_id, METADATA_KEY)
            .map_err(ApplicationError::Repository)?;
        let mut preferences = raw
            .as_deref()
            .and_then(parse_stored)
            .unwrap_or_else(AppPreferences::default);
        preferences.custom_theme = read_custom_theme(self.repository, user_id)?;
        Ok(preferences)
    }

    pub fn update(
        &mut self,
        user_id: &str,
        patch: AppPreferencesPatch,
    ) -> Result<AppPreferences, ApplicationError<R::Error>> {
        if patch_is_empty(&patch) {
            return Err(domain(
                "PREFERENCES_UPDATE_EMPTY",
                400,
                "至少提供一个要更新的设置",
            ));
        }
        if patch
            .shortcut_bindings
            .as_ref()
            .is_some_and(|shortcuts| !valid_shortcut_patch(shortcuts))
        {
            return Err(domain(
                "PREFERENCES_SHORTCUT_INVALID",
                400,
                "快捷键长度不能超过 60 个字符",
            ));
        }
        let mut next = self.read(user_id)?;
        if let Some(value) = patch.theme {
            next.theme = value;
        }
        if let Some(value) = patch.custom_theme {
            next.custom_theme = update_custom_theme(self.repository, user_id, value)?;
        }
        if let Some(value) = patch.startup_view {
            next.startup_view = value;
        }
        if let Some(value) = patch.mark_read_on_open {
            next.mark_read_on_open = value;
        }
        if let Some(value) = patch.default_message_view {
            next.default_message_view = value;
        }
        if let Some(value) = patch.notification_kinds {
            if let Some(field) = value.unread {
                next.notification_kinds.unread = field;
            }
            if let Some(field) = value.snooze {
                next.notification_kinds.snooze = field;
            }
            if let Some(field) = value.error {
                next.notification_kinds.error = field;
            }
        }
        if let Some(value) = patch.shortcut_bindings {
            apply_shortcut_patch(&mut next.shortcut_bindings, value);
        }
        let encoded = serde_json::to_string(&next).expect("preferences are serializable");
        self.repository
            .set_user_metadata(user_id, METADATA_KEY, &encoded)
            .map_err(ApplicationError::Repository)?;
        Ok(next)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPreferences {
    theme: Option<ThemeId>,
    startup_view: Option<StartupView>,
    mark_read_on_open: Option<bool>,
    default_message_view: Option<MessageView>,
    notification_kinds: Option<NotificationKinds>,
    shortcut_bindings: Option<ShortcutBindings>,
}

fn parse_stored(raw: &str) -> Option<AppPreferences> {
    let stored = serde_json::from_str::<StoredPreferences>(raw).ok()?;
    if stored
        .shortcut_bindings
        .as_ref()
        .is_some_and(|bindings| !valid_shortcuts(bindings))
    {
        return None;
    }
    let mut preferences = AppPreferences::default();
    if let Some(value) = stored.theme {
        preferences.theme = value;
    }
    if let Some(value) = stored.startup_view {
        preferences.startup_view = value;
    }
    if let Some(value) = stored.mark_read_on_open {
        preferences.mark_read_on_open = value;
    }
    if let Some(value) = stored.default_message_view {
        preferences.default_message_view = value;
    }
    if let Some(value) = stored.notification_kinds {
        preferences.notification_kinds = value;
    }
    if let Some(value) = stored.shortcut_bindings {
        preferences.shortcut_bindings = value;
    }
    Some(preferences)
}

fn patch_is_empty(patch: &AppPreferencesPatch) -> bool {
    patch.theme.is_none()
        && patch.custom_theme.is_none()
        && patch.startup_view.is_none()
        && patch.mark_read_on_open.is_none()
        && patch.default_message_view.is_none()
        && patch.notification_kinds.is_none()
        && patch.shortcut_bindings.is_none()
}

fn shortcut_length(value: &str) -> usize {
    value.encode_utf16().count()
}

fn valid_shortcut_patch(patch: &ShortcutBindingsPatch) -> bool {
    [
        &patch.focus_search,
        &patch.compose,
        &patch.sync,
        &patch.next_message,
        &patch.previous_message,
        &patch.reply,
        &patch.forward,
        &patch.toggle_star,
        &patch.mark_unread,
        &patch.archive,
        &patch.delete,
        &patch.open_shortcut_settings,
    ]
    .into_iter()
    .flatten()
    .all(|value| shortcut_length(value) <= 60)
}

fn valid_shortcuts(value: &ShortcutBindings) -> bool {
    [
        &value.focus_search,
        &value.compose,
        &value.sync,
        &value.next_message,
        &value.previous_message,
        &value.reply,
        &value.forward,
        &value.toggle_star,
        &value.mark_unread,
        &value.archive,
        &value.delete,
        &value.open_shortcut_settings,
    ]
    .into_iter()
    .all(|value| shortcut_length(value) <= 60)
}

fn apply_shortcut_patch(target: &mut ShortcutBindings, patch: ShortcutBindingsPatch) {
    macro_rules! assign {
        ($field:ident) => {
            if let Some(value) = patch.$field {
                target.$field = value;
            }
        };
    }
    assign!(focus_search);
    assign!(compose);
    assign!(sync);
    assign!(next_message);
    assign!(previous_message);
    assign!(reply);
    assign!(forward);
    assign!(toggle_star);
    assign!(mark_unread);
    assign!(archive);
    assign!(delete);
    assign!(open_shortcut_settings);
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
