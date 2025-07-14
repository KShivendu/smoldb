use std::collections::HashMap;

use raft::prelude::{Entry, EntryType};

use crate::consensus::Consensus;

impl Consensus {
    /// Tries to process raft's ready state. Should be called on each tick.
    ///
    /// [`raft::Ready`] is the outstanding work that the application needs to handle.
    /// [`raft::LightReady`] that has the committed entries and messages but no commit index.
    pub async fn on_ready(&mut self, _cbs: &mut HashMap<u8, Box<dyn Fn() + Send>>) {
        loop {
            if !self.raft_node.has_ready() {
                return;
            }

            println!("Raft node is ready, processing ready state output...");

            let store = self.raft_node.raft.raft_log.store.clone();

            // The Raft is ready, we can do something now.
            let mut ready = self.raft_node.ready();

            // ToDo: Consensus snapshots

            if !ready.messages().is_empty() {
                // Send messages to other peers.
                println!("Found messages");
                self.send_messages(ready.take_messages()).await;
            }

            let mut last_apply_index = 0;
            self.handle_committed_entries(ready.take_committed_entries(), &mut last_apply_index);

            if !ready.entries().is_empty() {
                // Append entries to the Raft log.
                store.wl().append(ready.entries()).unwrap();
            }

            if let Some(updated_hs) = ready.hs() {
                println!("Changing hard state: {updated_hs:?}");
                // Raft HardState changed, and we need to persist it.
                store.wl().set_hardstate(updated_hs.clone());
            }

            let role_change = ready.ss().map(|ss| ss.raft_state);

            if let Some(new_role) = role_change {
                self.handle_role_change(new_role);
            }

            if !ready.persisted_messages().is_empty() {
                // Send out the persisted messages come from the node.
                println!("Found persisted messages");
                self.send_messages(ready.take_persisted_messages()).await;
            }

            // Advance the Raft.
            let mut light_ready = self.raft_node.advance(ready);
            // Update commit index.
            if let Some(commit) = light_ready.commit_index() {
                store.wl().mut_hard_state().set_commit(commit);
            }
            // Send out the messages to other peers.
            self.send_messages(light_ready.take_messages()).await;
            // Apply all committed entries.
            self.handle_committed_entries(
                light_ready.take_committed_entries(),
                &mut last_apply_index,
            );
            // Advance the apply index.
            self.raft_node.advance_apply();

            println!("Raft node processed a ready state.");
        }
    }

    /// ToDo: This function should actually apply the committed entries to the state machine.
    ///
    /// However it currently pushes forwards ones to other peers via gRPC
    fn handle_committed_entries(&self, entries: Vec<Entry>, last_apply_index: &mut u64) {
        println!("Handling committed entries");

        for entry in entries {
            // Mostly, you need to save the last apply index to resume applying
            // after restart. Here we just ignore this because we use a Memory storage.
            *last_apply_index = entry.index;

            if entry.data.is_empty() {
                // Empty entry, when the peer becomes Leader it will send an empty entry.
                continue;
            }

            match entry.get_entry_type() {
                EntryType::EntryNormal => self.handle_normal(entry),
                // It's recommended to always use `EntryType::EntryConfChangeV2.
                EntryType::EntryConfChange => self.handle_conf_change(entry),
                EntryType::EntryConfChangeV2 => self.handle_conf_change_v2(entry),
            }
        }
    }

    fn handle_role_change(&self, new_role: raft::StateRole) {
        println!("Raft node role changed to: {:?}", new_role);
    }

    fn handle_normal(&self, entry: Entry) {
        println!("Handle normal entry: {entry:?}");
    }

    fn handle_conf_change(&self, entry: Entry) {
        println!("Handle conf change entry: {entry:?}");
    }

    fn handle_conf_change_v2(&self, entry: Entry) {
        println!("Handle conf change v2 entry: {entry:?}");
    }
}
