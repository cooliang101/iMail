use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStateKind {
    ICloudWeb,
    AppleAccount,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TwoFactorMethod {
    #[default]
    TrustedDevice,
    Phone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStatus {
    Connected,
    AuthRequired,
    Error,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires_at: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginState {
    pub kind: LoginStateKind,
    pub host: String,
    pub origin: String,
    pub cookies: Vec<AppleCookie>,
    pub scnt: Option<String>,
    pub session_id: Option<String>,
    pub api_key: Option<String>,
    pub data_access_token: Option<String>,
    pub user_agent: String,
    pub saved_at: DateTime<Utc>,
    pub manage_expires_at: Option<DateTime<Utc>>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_check_ok: bool,
    pub last_status_message: Option<String>,
    #[serde(default)]
    pub last_successful_keepalive_at: Option<DateTime<Utc>>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSession {
    pub apple_id: String,
    pub dsid: Option<String>,
    pub client_id: Option<String>,
    pub client_build_number: Option<String>,
    pub client_mastering_number: Option<String>,
    pub premium_mail_base_url: Option<String>,
    pub mail_gateway_base_url: Option<String>,
    pub mail_base_url: Option<String>,
    pub host: String,
    pub is_icloud_plus: bool,
    pub can_create_hme: bool,
    pub login_states: Vec<LoginState>,
    pub saved_at: DateTime<Utc>,
}

impl AppleSession {
    pub fn empty(apple_id: impl Into<String>) -> Self {
        Self {
            apple_id: apple_id.into(),
            dsid: None,
            client_id: None,
            client_build_number: None,
            client_mastering_number: None,
            premium_mail_base_url: None,
            mail_gateway_base_url: None,
            mail_base_url: None,
            host: String::new(),
            is_icloud_plus: false,
            can_create_hme: false,
            login_states: Vec::new(),
            saved_at: Utc::now(),
        }
    }

    pub fn state(&self, kind: LoginStateKind) -> Option<&LoginState> {
        self.login_states.iter().find(|state| state.kind == kind)
    }

    pub fn state_mut(&mut self, kind: LoginStateKind) -> Option<&mut LoginState> {
        self.login_states
            .iter_mut()
            .find(|state| state.kind == kind)
    }

    pub fn put_state(&mut self, next: LoginState) {
        if let Some(state) = self.state_mut(next.kind) {
            *state = next;
        } else {
            self.login_states.push(next);
        }
        self.saved_at = Utc::now();
    }

    pub fn merge(mut self, incoming: Self) -> Self {
        if !incoming.apple_id.trim().is_empty() {
            self.apple_id = incoming.apple_id;
        }
        macro_rules! replace_some {
            ($field:ident) => {
                if incoming.$field.is_some() {
                    self.$field = incoming.$field;
                }
            };
        }
        replace_some!(dsid);
        replace_some!(client_id);
        replace_some!(client_build_number);
        replace_some!(client_mastering_number);
        replace_some!(premium_mail_base_url);
        replace_some!(mail_gateway_base_url);
        replace_some!(mail_base_url);
        if !incoming.host.trim().is_empty() {
            self.host = incoming.host;
        }
        self.is_icloud_plus |= incoming.is_icloud_plus;
        self.can_create_hme |= incoming.can_create_hme;
        for state in incoming.login_states {
            self.put_state(state);
        }
        self.saved_at = Utc::now();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HmeAddress {
    pub anonymous_id: String,
    pub email: String,
    pub label: String,
    pub note: String,
    pub forward_to_email: String,
    pub active: bool,
    pub origin: String,
    pub created_at: Option<DateTime<Utc>>,
}
