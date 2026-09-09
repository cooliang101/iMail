//! Injectable network dependencies and their production adapters.
use imail_mail::{ImapPort, MailConnectionConfig, SmtpPort};
use imail_mail_network::NetworkMailAdapter;
use imail_oauth::{OAuthProviderPort, OAuthProviderPortFactory};
use imail_oauth_http::OAuthHttpAdapter;

pub trait MailConnectionProbe: Send + Sync {
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String>;
}

impl<F> MailConnectionProbe for F
where
    F: Fn(&MailConnectionConfig) -> Result<(), String> + Send + Sync,
{
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String> {
        self(config)
    }
}

pub(super) struct NetworkConnectionProbe;

impl MailConnectionProbe for NetworkConnectionProbe {
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String> {
        let mut adapter =
            NetworkMailAdapter::new().map_err(|_| "邮箱网络运行时不可用".to_string())?;
        ImapPort::verify(&mut adapter, config).map_err(|error| error.to_string())?;
        SmtpPort::verify(&mut adapter, config).map_err(|error| error.to_string())
    }
}

pub trait MailTransportFactory: Send + Sync {
    fn create_imap(&self) -> Result<Box<dyn ImapPort>, String>;
    fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String>;
}

pub(super) struct NetworkMailTransportFactory;

impl MailTransportFactory for NetworkMailTransportFactory {
    fn create_imap(&self) -> Result<Box<dyn ImapPort>, String> {
        NetworkMailAdapter::new()
            .map(|adapter| Box::new(adapter) as Box<dyn ImapPort>)
            .map_err(|_| "邮箱网络运行时不可用".into())
    }

    fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String> {
        NetworkMailAdapter::new()
            .map(|adapter| Box::new(adapter) as Box<dyn SmtpPort>)
            .map_err(|_| "邮箱网络运行时不可用".into())
    }
}

pub(super) struct HttpOAuthProviderFactory;

impl OAuthProviderPortFactory for HttpOAuthProviderFactory {
    fn create(&self) -> Box<dyn OAuthProviderPort> {
        Box::new(OAuthHttpAdapter::new())
    }
}
