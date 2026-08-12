use std::{cmp::Ordering, collections::HashMap};

use imail_protocol::{ContactReadModel, MessageReadModel};

use crate::{AccountRecord, ApplicationError, LocalRepository, LogoFetchAttemptRecord};

pub trait RegistrableDomainResolver {
    fn registrable_domain(&self, hostname: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PublicSuffixDomainResolver;

impl RegistrableDomainResolver for PublicSuffixDomainResolver {
    fn registrable_domain(&self, hostname: &str) -> Option<String> {
        psl::domain_str(hostname).map(str::to_string)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactDomain {
    pub hostname: String,
    pub registrable: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactLogoKeys {
    pub exact: String,
    pub root: String,
}

pub struct ContactsService<'a, R: LocalRepository, D: RegistrableDomainResolver> {
    repository: &'a mut R,
    domains: &'a D,
}

impl<'a, R: LocalRepository, D: RegistrableDomainResolver> ContactsService<'a, R, D> {
    pub fn new(repository: &'a mut R, domains: &'a D) -> Self {
        Self {
            repository,
            domains,
        }
    }

    pub fn reconcile(
        &mut self,
        user_id: &str,
    ) -> Result<Vec<ContactReadModel>, ApplicationError<<R as crate::AccountRepository>::Error>>
    {
        let accounts = self
            .repository
            .list_accounts(user_id)
            .map_err(ApplicationError::Repository)?;
        let messages = self
            .repository
            .list_messages(user_id)
            .map_err(ApplicationError::Repository)?;
        let previous = self
            .repository
            .list_contacts(user_id)
            .map_err(ApplicationError::Repository)?;
        let contacts = reconcile_contacts(user_id, &accounts, &messages, &previous, self.domains);
        self.repository
            .replace_contacts(user_id, &contacts)
            .map_err(ApplicationError::Repository)?;
        Ok(contacts)
    }

    pub fn should_attempt_logo(
        &self,
        user_id: &str,
        target: &str,
    ) -> Result<bool, ApplicationError<<R as crate::AccountRepository>::Error>> {
        let normalized = target.trim().to_lowercase();
        let attempts = self
            .repository
            .list_logo_fetch_attempts(user_id)
            .map_err(ApplicationError::Repository)?;
        Ok(!attempts
            .iter()
            .any(|attempt| attempt.target.to_lowercase() == normalized))
    }
}

pub fn contact_domain<D: RegistrableDomainResolver>(
    address: &str,
    domains: &D,
) -> Option<ContactDomain> {
    let hostname = address
        .rsplit_once('@')?
        .1
        .trim()
        .trim_end_matches('.')
        .to_lowercase();
    if hostname.is_empty()
        || !hostname
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return None;
    }
    let registrable = domains
        .registrable_domain(&hostname)
        .unwrap_or_else(|| hostname.clone());
    Some(ContactDomain {
        hostname,
        registrable,
    })
}

pub fn contact_logo_keys<D: RegistrableDomainResolver>(
    address: &str,
    domains: &D,
) -> Option<ContactLogoKeys> {
    let domain = contact_domain(address, domains)?;
    Some(ContactLogoKeys {
        exact: format!("domain:{}", domain.hostname),
        root: format!("domain:{}", domain.registrable),
    })
}

pub fn contacts_need_logo_update<D: RegistrableDomainResolver>(
    contacts: &[ContactReadModel],
    address: &str,
    logo: &ContactReadModel,
    domains: &D,
) -> bool {
    let Some(keys) = contact_logo_keys(address, domains) else {
        return false;
    };
    let use_root = logo.logo_key.as_deref() == Some(keys.root.as_str());
    contacts.iter().any(|contact| {
        let Some(contact_keys) = contact_logo_keys(&contact.address, domains) else {
            return false;
        };
        let matches = if use_root {
            contact_keys.root == keys.root
        } else {
            contact_keys.exact == keys.exact
        };
        matches && !same_logo(contact, logo)
    })
}

pub fn reconcile_contacts<D: RegistrableDomainResolver>(
    user_id: &str,
    accounts: &[AccountRecord],
    messages: &[MessageReadModel],
    previous: &[ContactReadModel],
    domains: &D,
) -> Vec<ContactReadModel> {
    let own_addresses = accounts
        .iter()
        .map(|account| account.email.trim().to_lowercase())
        .collect::<Vec<_>>();
    reconcile_contacts_for_addresses(user_id, &own_addresses, messages, previous, domains)
}

pub fn reconcile_contacts_for_addresses<D: RegistrableDomainResolver>(
    user_id: &str,
    own_addresses: &[String],
    messages: &[MessageReadModel],
    previous: &[ContactReadModel],
    domains: &D,
) -> Vec<ContactReadModel> {
    let own_addresses = own_addresses
        .iter()
        .map(|address| address.trim().to_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let previous_by_address = previous
        .iter()
        .map(|contact| (contact.address.to_lowercase(), contact))
        .collect::<HashMap<_, _>>();
    let logo_by_key = previous
        .iter()
        .filter_map(|contact| contact.logo_key.as_ref().map(|key| (key.clone(), contact)))
        .collect::<HashMap<_, _>>();
    let mut contacts = HashMap::<String, ContactReadModel>::new();

    for message in messages {
        let mut seen = std::collections::HashSet::new();
        for (name, address) in participants(message) {
            let address = address.trim().to_string();
            let key = address.to_lowercase();
            if address.is_empty() || own_addresses.contains(&key) || !seen.insert(key.clone()) {
                continue;
            }
            let current = contacts.get(&key);
            let previous_contact = previous_by_address.get(&key).copied();
            let latest = current.map_or(true, |item| message.date > item.last_contact_at);
            let prior_logo = current
                .filter(|item| item.logo_key.is_some())
                .or_else(|| previous_contact.filter(|item| item.logo_key.is_some()));
            let selected_logo = contact_logo_keys(&address, domains)
                .and_then(|keys| {
                    logo_by_key
                        .get(&keys.exact)
                        .copied()
                        .or_else(|| {
                            prior_logo.filter(|logo| logo.logo_key.as_deref() == Some(&keys.exact))
                        })
                        .or_else(|| logo_by_key.get(&keys.root).copied())
                })
                .or(prior_logo);
            let mut contact = ContactReadModel {
                owner_id: user_id.to_string(),
                address: if latest {
                    address
                } else {
                    current.expect("non-latest contact exists").address.clone()
                },
                name: if latest {
                    let trimmed = name.trim();
                    if trimmed.is_empty() {
                        current
                            .map(|item| item.name.clone())
                            .or_else(|| previous_contact.map(|item| item.name.clone()))
                            .unwrap_or_default()
                    } else {
                        trimmed.to_string()
                    }
                } else {
                    current.expect("non-latest contact exists").name.clone()
                },
                message_count: current.map_or(1, |item| item.message_count + 1),
                last_contact_at: current
                    .map(|item| item.last_contact_at.as_str())
                    .filter(|date| *date > message.date.as_str())
                    .unwrap_or(&message.date)
                    .to_string(),
                logo_key: None,
                logo_content_type: None,
                logo_source_url: None,
                logo_fetched_at: None,
            };
            copy_logo(&mut contact, selected_logo);
            contacts.insert(key, contact);
        }
    }
    let mut values = contacts.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .last_contact_at
            .cmp(&left.last_contact_at)
            .then_with(|| right.message_count.cmp(&left.message_count))
            .then_with(|| {
                left.address
                    .partial_cmp(&right.address)
                    .unwrap_or(Ordering::Equal)
            })
    });
    values
}

fn participants(message: &MessageReadModel) -> Vec<(String, String)> {
    let mut values = Vec::new();
    push_participant(&mut values, &message.from);
    if let Some(recipients) = message.to.as_array() {
        for recipient in recipients {
            push_participant(&mut values, recipient);
        }
    }
    values
}

fn push_participant(target: &mut Vec<(String, String)>, value: &serde_json::Value) {
    if let Some(address) = value.get("address").and_then(|field| field.as_str()) {
        target.push((
            value
                .get("name")
                .and_then(|field| field.as_str())
                .unwrap_or_default()
                .to_string(),
            address.to_string(),
        ));
    }
}

fn copy_logo(target: &mut ContactReadModel, source: Option<&ContactReadModel>) {
    if let Some(source) = source {
        target.logo_key.clone_from(&source.logo_key);
        target
            .logo_content_type
            .clone_from(&source.logo_content_type);
        target.logo_source_url.clone_from(&source.logo_source_url);
        target.logo_fetched_at.clone_from(&source.logo_fetched_at);
    }
}

fn same_logo(left: &ContactReadModel, right: &ContactReadModel) -> bool {
    left.logo_key == right.logo_key
        && left.logo_content_type == right.logo_content_type
        && left.logo_source_url == right.logo_source_url
        && left.logo_fetched_at == right.logo_fetched_at
}

pub fn logo_attempt_is_terminal(attempt: &LogoFetchAttemptRecord) -> bool {
    matches!(attempt.status.as_str(), "success" | "failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_public_suffix_resolver_handles_multilevel_and_private_suffixes() {
        let resolver = PublicSuffixDomainResolver;
        assert_eq!(
            resolver.registrable_domain("mail.example.co.uk").as_deref(),
            Some("example.co.uk")
        );
        assert_eq!(
            resolver.registrable_domain("tenant.appspot.com").as_deref(),
            Some("tenant.appspot.com")
        );
    }
}
