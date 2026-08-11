use std::{collections::BTreeMap, sync::OnceLock};

use imail_protocol::AppUserView;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::AuthRepository;

const REGISTRATION_MAXIMUM: u32 = 5;
const REGISTRATION_WINDOW_MS: u64 = 60 * 60_000;
const LOGIN_SOURCE_MAXIMUM: u32 = 30;
const LOGIN_ACCOUNT_MAXIMUM: u32 = 10;
const LOGIN_WINDOW_MS: u64 = 15 * 60_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationInput {
    pub login: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginInput {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUser {
    pub id: String,
    pub login: String,
    pub display_name: String,
}

impl From<AppUserView> for PublicUser {
    fn from(user: AppUserView) -> Self {
        Self {
            id: user.id,
            login: user.login,
            display_name: user.display_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationStatus {
    pub setup_required: bool,
    pub registration_open: bool,
    pub user: Option<PublicUser>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub user: PublicUser,
    pub raw_session: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError<E: std::error::Error + Send + Sync + 'static> {
    #[error("{message}")]
    Domain { status: u16, message: &'static str },
    #[error("尝试过多，请稍后再试")]
    Limited { retry_after: u64 },
    #[error(transparent)]
    Repository(E),
}

pub struct AuthenticationService<'a, R: AuthRepository> {
    repository: &'a mut R,
}

impl<'a, R: AuthRepository> AuthenticationService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn status(
        &mut self,
        raw_session: Option<&str>,
        registration_configured: bool,
    ) -> Result<AuthenticationStatus, AuthenticationError<R::Error>> {
        let setup_required = self
            .repository
            .setup_required()
            .map_err(AuthenticationError::Repository)?;
        let user = raw_session
            .map(|raw| self.repository.user_for_session(raw))
            .transpose()
            .map_err(AuthenticationError::Repository)?
            .flatten()
            .map(PublicUser::from);
        Ok(AuthenticationStatus {
            setup_required,
            registration_open: setup_required || registration_configured,
            user,
        })
    }

    pub fn session(
        &mut self,
        raw_session: &str,
    ) -> Result<PublicUser, AuthenticationError<R::Error>> {
        self.repository
            .user_for_session(raw_session)
            .map_err(AuthenticationError::Repository)?
            .map(PublicUser::from)
            .ok_or(AuthenticationError::Domain {
                status: 401,
                message: "请先登录",
            })
    }

    pub fn register(
        &mut self,
        input: Option<RegistrationInput>,
        actor: &str,
        registration_configured: bool,
    ) -> Result<AuthenticatedSession, AuthenticationError<R::Error>> {
        let setup_required = self
            .repository
            .setup_required()
            .map_err(AuthenticationError::Repository)?;
        if !setup_required && !registration_configured {
            self.audit("registration.denied", actor, None)?;
            return Err(AuthenticationError::Domain {
                status: 403,
                message: "此服务已关闭新用户注册",
            });
        }
        let limit = self
            .repository
            .consume_attempt(
                &format!("register:{actor}"),
                REGISTRATION_MAXIMUM,
                REGISTRATION_WINDOW_MS,
            )
            .map_err(AuthenticationError::Repository)?;
        if !limit.allowed {
            return Err(AuthenticationError::Limited {
                retry_after: limit.retry_after,
            });
        }
        let input = input.ok_or(AuthenticationError::Domain {
            status: 400,
            message: "请求参数无效",
        })?;
        let input = validate_registration(input)?;
        let Some(user) = self
            .repository
            .create_user_if_allowed(
                &input.login,
                &input.display_name,
                &input.password,
                registration_configured,
            )
            .map_err(AuthenticationError::Repository)?
        else {
            self.audit("registration.denied", actor, None)?;
            return Err(AuthenticationError::Domain {
                status: 403,
                message: "此服务已关闭新用户注册",
            });
        };
        self.audit("registration.succeeded", actor, Some(&user.id))?;
        let raw_session = self
            .repository
            .create_session(&user.id)
            .map_err(AuthenticationError::Repository)?;
        Ok(AuthenticatedSession {
            user: user.into(),
            raw_session,
        })
    }

    pub fn login(
        &mut self,
        input: Option<LoginInput>,
        actor: &str,
    ) -> Result<AuthenticatedSession, AuthenticationError<R::Error>> {
        let input = input.ok_or(AuthenticationError::Domain {
            status: 400,
            message: "请求参数无效",
        })?;
        let input = validate_login(input)?;
        let account_key = format!("login-account:{}", input.login.to_lowercase());
        let source_limit = self
            .repository
            .consume_attempt(
                &format!("login-ip:{actor}"),
                LOGIN_SOURCE_MAXIMUM,
                LOGIN_WINDOW_MS,
            )
            .map_err(AuthenticationError::Repository)?;
        if !source_limit.allowed {
            return Err(AuthenticationError::Limited {
                retry_after: source_limit.retry_after,
            });
        }
        let account_limit = self
            .repository
            .consume_attempt(&account_key, LOGIN_ACCOUNT_MAXIMUM, LOGIN_WINDOW_MS)
            .map_err(AuthenticationError::Repository)?;
        if !account_limit.allowed {
            return Err(AuthenticationError::Limited {
                retry_after: account_limit.retry_after,
            });
        }
        let Some(user) = self
            .repository
            .authenticate(&input.login, &input.password)
            .map_err(AuthenticationError::Repository)?
        else {
            self.audit("login.failed", actor, None)?;
            return Err(AuthenticationError::Domain {
                status: 401,
                message: "登录名或密码错误",
            });
        };
        self.repository
            .clear_attempt(&account_key)
            .map_err(AuthenticationError::Repository)?;
        self.audit("login.succeeded", actor, Some(&user.id))?;
        let raw_session = self
            .repository
            .create_session(&user.id)
            .map_err(AuthenticationError::Repository)?;
        Ok(AuthenticatedSession {
            user: user.into(),
            raw_session,
        })
    }

    pub fn logout(
        &mut self,
        raw_session: Option<&str>,
        actor: &str,
    ) -> Result<(), AuthenticationError<R::Error>> {
        if let Some(raw) = raw_session {
            if let Some(user) = self
                .repository
                .user_for_session(raw)
                .map_err(AuthenticationError::Repository)?
            {
                self.audit("logout", actor, Some(&user.id))?;
            }
            self.repository
                .delete_session(raw)
                .map_err(AuthenticationError::Repository)?;
        }
        Ok(())
    }

    fn audit(
        &mut self,
        event_type: &str,
        actor: &str,
        user_id: Option<&str>,
    ) -> Result<(), AuthenticationError<R::Error>> {
        self.repository
            .record_security_event(event_type, actor, user_id, &BTreeMap::new())
            .map_err(AuthenticationError::Repository)
    }
}

fn validate_registration<E: std::error::Error + Send + Sync + 'static>(
    mut input: RegistrationInput,
) -> Result<RegistrationInput, AuthenticationError<E>> {
    input.login = input.login.trim().to_string();
    input.display_name = input.display_name.trim().to_string();
    validate_credentials(&input.login, &input.password)?;
    match utf16_len(&input.display_name) {
        0 => Err(AuthenticationError::Domain {
            status: 400,
            message: "请填写显示名称",
        }),
        81.. => Err(AuthenticationError::Domain {
            status: 400,
            message: "显示名称最多 80 个字符",
        }),
        _ => Ok(input),
    }
}

fn validate_login<E: std::error::Error + Send + Sync + 'static>(
    mut input: LoginInput,
) -> Result<LoginInput, AuthenticationError<E>> {
    input.login = input.login.trim().to_string();
    validate_credentials(&input.login, &input.password)?;
    Ok(input)
}

fn validate_credentials<E: std::error::Error + Send + Sync + 'static>(
    login: &str,
    password: &str,
) -> Result<(), AuthenticationError<E>> {
    let invalid = |message| AuthenticationError::Domain {
        status: 400,
        message,
    };
    let login_length = utf16_len(login);
    if login_length < 3 {
        return Err(invalid("登录名至少 3 个字符"));
    }
    if login_length > 80 {
        return Err(invalid("登录名最多 80 个字符"));
    }
    static LOGIN_PATTERN: OnceLock<Regex> = OnceLock::new();
    let supported = LOGIN_PATTERN
        .get_or_init(|| Regex::new(r"^[\p{L}\p{N}_.@-]+$").expect("static login regex"));
    if !supported.is_match(login) {
        return Err(invalid("登录名包含不支持的字符"));
    }
    let password_length = utf16_len(password);
    if password_length < 8 {
        return Err(invalid("密码至少 8 个字符"));
    }
    if password_length > 256 {
        return Err(invalid("密码最多 256 个字符"));
    }
    Ok(())
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}
