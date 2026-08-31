use imail_protocol::{normalize_message_id, MessageReadModel};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub(super) fn fingerprint(message: &MessageReadModel) -> [u8; 32] {
    Sha256::digest(
        serde_json::to_vec(&(&message.text, &message.html, &message.attachments))
            .expect("message content is serializable"),
    )
    .into()
}

/// Header links only, with virtual ancestors for parents outside the local cache.
/// Conflicting duplicate IDs are isolated. Identical copies remain separate records.
pub(super) fn related(
    messages: Vec<MessageReadModel>,
    selected: &str,
    fingerprints: &HashMap<String, [u8; 32]>,
) -> Option<Vec<MessageReadModel>> {
    let selected_index = messages.iter().position(|message| message.id == selected)?;
    let ids = messages
        .iter()
        .map(|message| message.message_id.as_deref().and_then(normalize_message_id))
        .collect::<Vec<_>>();
    let mut first = HashMap::<&str, usize>::new();
    let mut ambiguous = HashSet::<&str>::new();
    for (index, id) in ids.iter().enumerate() {
        if let Some(id) = id {
            if let Some(previous) = first.get(id.as_str()) {
                let a = &messages[*previous];
                let b = &messages[index];
                if a.from != b.from
                    || a.to != b.to
                    || a.subject != b.subject
                    || a.date != b.date
                    || a.headers != b.headers
                    || fingerprints.get(&a.id) != fingerprints.get(&b.id)
                {
                    ambiguous.insert(id.as_str());
                }
            } else {
                first.insert(id, index);
            }
        }
    }
    let mut parents = (0..messages.len()).collect::<Vec<_>>();
    let mut identifiers = HashMap::<String, usize>::new();
    for (index, message) in messages.iter().enumerate() {
        if ids[index]
            .as_deref()
            .is_some_and(|id| ambiguous.contains(id))
        {
            continue;
        }
        let headers = &message.headers.reply;
        let links = ids[index].iter().cloned().chain(
            headers
                .references
                .iter()
                .chain(&headers.in_reply_to)
                .take(200)
                .filter_map(|id| normalize_message_id(id)),
        );
        for id in links {
            if ambiguous.contains(id.as_str()) {
                continue;
            }
            if let Some(other) = identifiers.get(&id) {
                let a = root(&mut parents, index);
                let b = root(&mut parents, *other);
                if a != b {
                    parents[a] = b;
                }
            } else {
                identifiers.insert(id, index);
            }
        }
    }
    let selected_root = root(&mut parents, selected_index);
    let mut result = messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| {
            (root(&mut parents, index) == selected_root).then_some(message)
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));
    Some(result)
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str, header: &str, references: &[&str]) -> MessageReadModel {
        serde_json::from_value(json!({
            "id":id,"accountId":"account","mailbox":"INBOX","mailboxRole":"inbox","uid":1,
            "messageId":header,"from":{"address":"sender@example.test"},"to":[],
            "subject":"Same subject","preview":"","text":"","html":null,
            "date":"2026-08-31T00:00:00Z","unread":true,"flagged":false,
            "hasAttachments":false,"attachments":[],"labels":[],"snoozedUntil":null,
            "references":references
        }))
        .unwrap()
    }

    fn ids(messages: Vec<MessageReadModel>, selected: &str) -> Vec<String> {
        let fingerprints = messages
            .iter()
            .map(|message| (message.id.clone(), fingerprint(message)))
            .collect();
        related(messages, selected, &fingerprints)
            .unwrap()
            .into_iter()
            .map(|message| message.id)
            .collect()
    }

    #[test]
    fn groups_reply_chain_and_keeps_each_account_folder_copy() {
        let original = message("a", "<a@example.test>", &[]);
        let reply = message("b", "<b@example.test>", &["<a@example.test>"]);
        let mut copy = reply.clone();
        copy.id = "c".into();
        copy.account_id = "other-account".into();
        copy.mailbox = "Sent".into();
        assert_eq!(
            ids(
                vec![
                    original,
                    reply,
                    copy,
                    message("unrelated", "<other@example.test>", &[])
                ],
                "a"
            ),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn handles_missing_parents_cycles_and_bad_headers_without_subject_fallback() {
        let a = message(
            "a",
            "<a@example.test>",
            &["<missing@example.test>", "<b@example.test>"],
        );
        let b = message("b", "<b@example.test>", &["<a@example.test>"]);
        let c = message("c", "invalid", &["<missing@example.test>"]);
        let bad = message("bad", "invalid", &["not an id"]);
        assert_eq!(
            ids(vec![a.clone(), b.clone(), c.clone(), bad.clone()], "a"),
            ["a", "b", "c"]
        );
        assert_eq!(ids(vec![a, b, c, bad], "bad"), ["bad"]);
    }

    #[test]
    fn conflicting_duplicate_ids_never_join_or_bridge_threads() {
        let a = message("a", "<duplicate@example.test>", &["<first@example.test>"]);
        let mut b = a.clone();
        b.id = "b".into();
        b.subject = "Different message".into();
        let reply = message(
            "reply",
            "<reply@example.test>",
            &["<duplicate@example.test>"],
        );
        assert_eq!(ids(vec![a.clone(), b.clone(), reply.clone()], "a"), ["a"]);
        assert_eq!(ids(vec![a, b, reply], "reply"), ["reply"]);
    }

    #[test]
    fn long_chain_is_complete_without_recursion_or_subject_only_matches() {
        let mut messages = (0..10_000)
            .map(|index| {
                let parent = format!("<{}@example.test>", index - 1);
                let parent_ref = parent.as_str();
                message(
                    &index.to_string(),
                    &format!("<{index}@example.test>"),
                    if index == 0 {
                        &[]
                    } else {
                        std::slice::from_ref(&parent_ref)
                    },
                )
            })
            .collect::<Vec<_>>();
        messages.push(message("unrelated", "<unrelated@example.test>", &[]));
        let result = ids(messages, "9999");
        assert_eq!(result.len(), 10_000);
        assert!(!result.contains(&"unrelated".to_string()));
    }

    #[test]
    fn duplicate_ids_with_identical_metadata_but_different_bodies_are_isolated() {
        let a = message("a", "<same@example.test>", &[]);
        let mut b = a.clone();
        b.id = "b".into();
        b.text = "different content".into();
        assert_eq!(ids(vec![a, b], "a"), ["a"]);
    }
}
