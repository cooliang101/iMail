mod auth;
mod error;
mod hme;
mod model;
mod protocol;
mod srp;
mod transport;

pub use auth::{
    AppleAuthClient, LoginRequest, LoginResult, MemoryPendingLoginStore, PendingLoginStore,
};
pub use error::{AppleHmeError, AppleHmeErrorCode};
pub use hme::{AppleHmeClient, CreateChannel, CreateHmeRequest, CreateHmeResult};
pub use model::{
    AppleCookie, AppleSession, HmeAddress, LoginState, LoginStateKind, LoginStatus, TwoFactorMethod,
};
pub use transport::{HttpMethod, HttpRequest, HttpResponse, HttpTransport, UreqTransport};

pub const DEFAULT_ICLOUD_WEB_CLIENT_ID: &str =
    "d39ba9916b7251055b22c7f910e2ea796ee65e98b2ddecea8f5dde8d9d1a815d";
