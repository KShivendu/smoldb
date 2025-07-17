use prost_for_raft::Message;
use raft::prelude::{ConfChange, ConfChangeV2, Entry, EntryType, Snapshot};

use crate::consensus::ConsensusOperation;

#[derive(Debug, serde::Serialize)]
pub struct DebuggableEntry {
    index: u64,
    term: u64,
    data: String,
    context: String,
}

impl From<&Entry> for DebuggableEntry {
    fn from(entry: &Entry) -> Self {
        let context = str::from_utf8(&entry.context).unwrap_or("Invalid UTF-8");

        let data = match entry.get_entry_type() {
            EntryType::EntryNormal => {
                let data = ConsensusOperation::from_entry(entry)
                    .map(|e| format!("{e:?}"))
                    .unwrap_or("Entry data should be decodable".to_string());

                data
            }
            EntryType::EntryConfChange => {
                let data = ConfChange::decode(&*entry.data)
                    .map(|cc| format!("{cc:?}"))
                    .unwrap_or("Entry data should be decodable".to_string());

                data
            }
            EntryType::EntryConfChangeV2 => {
                let data = ConfChangeV2::decode(&*entry.data)
                    .map(|cc| format!("{cc:?}"))
                    .unwrap_or("Entry data should be decodable".to_string());

                data
            }
        };

        DebuggableEntry {
            index: entry.index,
            term: entry.term,
            data,
            context: context.to_string(),
        }
    }
}

impl DebuggableEntry {
    pub fn log(&self, prefix: &str) {
        let debuggable_entry =
            serde_json::to_string(self).expect("Failed to serialize entry to JSON");
        println!("{prefix}: {debuggable_entry}");
    }
}

#[derive(Debug, serde::Serialize)]
pub struct DebuggableHardState {
    term: u64,
    vote: u64,
    commit: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct DebuggableSnapshot {
    metadata: String,
}

#[derive(Debug, serde::Serialize)]
pub struct DebuggableMessage {
    msg_type: String,
    from: u64,
    to: u64,
    term: u64,
    index: u64,
    commit: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entries: Vec<DebuggableEntry>,
    rejected: bool,
    // snapshot: Option<DebuggableSnapshot>,
}

impl From<raft::eraftpb::Message> for DebuggableMessage {
    fn from(msg: raft::eraftpb::Message) -> Self {
        DebuggableMessage {
            msg_type: format!("{:?}", msg.get_msg_type()),
            from: msg.from,
            to: msg.to,
            term: msg.term,
            index: msg.index,
            commit: msg.commit,
            entries: msg.entries.iter().map(DebuggableEntry::from).collect(),
            rejected: msg.reject,
            // snapshot: None, // ToDo: Handle snapshot if needed
        }
    }
}

impl DebuggableMessage {
    pub fn log(&self, prefix: &str) {
        if (self.msg_type == "MsgHeartbeat" || self.msg_type == "MsgHeartbeatResponse")
            && !self.rejected
        {
            return;
        }

        let debuggable_msg =
            serde_json::to_string(&self).expect("Failed to serialize message to JSON");
        println!("{prefix}: {debuggable_msg}",);
    }
}

#[derive(Debug)]
#[allow(dead_code)]
#[derive(serde::Serialize)]
pub struct DebuggableReady {
    number: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    leader_or_role_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hard_state_change: Option<DebuggableHardState>,
    // read_states: Vec<raft::ReadState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    msgs_to_send: Vec<DebuggableMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    committed_entries: Vec<DebuggableEntry>,
    /// Specifies entries to be saved to stable storage
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entries_to_save: Vec<DebuggableEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<DebuggableSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    persisted_messages: Vec<DebuggableMessage>,
    // light_ready: raft::LightReady,
    // must_sync: bool,
}

impl DebuggableReady {
    pub fn log(&self, prefix: &str) {
        let all_non_msg_to_send_empty = self.committed_entries.is_empty()
            && self.entries_to_save.is_empty()
            && self.snapshot.is_none()
            && self.persisted_messages.is_empty();

        let are_heartbeats = self
            .msgs_to_send
            .iter()
            .all(|msg| msg.msg_type == "MsgHeartbeat" || msg.msg_type == "MsgHeartbeatResponse");

        if all_non_msg_to_send_empty && are_heartbeats {
            // No messages to log, return early
            return;
        }

        let debuggable_ready =
            serde_json::to_string(self).expect("Failed to serialize ready state to JSON");
        println!("{prefix}: {debuggable_ready}");
    }
}

impl From<&raft::Ready> for DebuggableReady {
    fn from(ready: &raft::Ready) -> Self {
        let number = ready.number();
        let msgs_to_send = ready.messages().to_vec();
        let committed_entries = ready
            .committed_entries()
            .iter()
            .map(DebuggableEntry::from)
            .collect::<Vec<_>>();
        let entries_to_save = ready.entries().to_vec();
        let hard_state_change = ready.hs().cloned();
        let leader_or_role_change = ready.ss().map(|s| format!("{s:?}"));
        let persisted_messages = ready.persisted_messages().to_vec();

        let snapshot = if ready.snapshot() == Snapshot::default_ref() {
            None
        } else {
            Some(ready.snapshot().clone())
        };

        let hard_state_change = hard_state_change.map(|hs| DebuggableHardState {
            term: hs.term,
            vote: hs.vote,
            commit: hs.commit,
        });

        let msgs_to_send = msgs_to_send
            .into_iter()
            .map(DebuggableMessage::from)
            .collect();

        let entries_to_save = entries_to_save.iter().map(DebuggableEntry::from).collect();

        let snapshot = snapshot.map(|s| DebuggableSnapshot {
            metadata: format!("{:?}", s.metadata),
        });

        let persisted_messages = persisted_messages
            .into_iter()
            .map(DebuggableMessage::from)
            .collect();

        Self {
            number,
            leader_or_role_change,
            hard_state_change,
            msgs_to_send,
            committed_entries,
            entries_to_save,
            snapshot,
            persisted_messages,
            // light_ready: ready,
        }
    }
}
