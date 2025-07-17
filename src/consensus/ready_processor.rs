use crate::consensus::{
    debuggables::{DebuggableEntry, DebuggableReady},
    Consensus, ConsensusOperation, ProposalId,
};
use protobuf::Message as PbMessage;
use raft::{
    prelude::{ConfChange, Entry, EntryType, Snapshot},
    SoftState,
};
use std::collections::HashMap;

impl Consensus {
    /// Tries to process raft's ready state. Should be called on each tick.
    ///
    /// [`raft::Ready`] is the outstanding work that the application needs to handle.
    /// [`raft::LightReady`] that has the committed entries and messages but no commit index.
    pub async fn on_ready(
        &mut self,
        _cbs: &mut HashMap<ProposalId, Box<dyn Fn() + Send>>,
        with_logging: bool,
    ) {
        loop {
            if !self.raft_node.has_ready() {
                return;
            }

            let store = self.raft_node.raft.raft_log.store.clone();

            // The Raft is ready, we can do something now.
            let mut ready = self.raft_node.ready();

            if with_logging {
                let debuggable_ready = DebuggableReady::from(&ready);
                debuggable_ready.log("\n\n\n=====> Raft node is ready, processing ready state");
            }

            // ToDo: Consensus snapshots

            if !ready.messages().is_empty() {
                self.send_messages(ready.take_messages()).await;
            }

            // Apply snapshot if it exists.
            if *ready.snapshot() != Snapshot::default() {
                println!("Found a snapshot in ready state: {:?}", ready.snapshot());
                let s = ready.snapshot().clone();
                if let Err(e) = store.apply_snapshot(s) {
                    println!("Failed to apply snapshot: {e}. should retry or panic");
                } else {
                    println!("Successfully applied snapshot: {:?}", ready.snapshot());
                }
            }

            let mut last_apply_index = 0; // ToDo: Should be stored globally in a state?
            self.handle_committed_entries(ready.take_committed_entries(), &mut last_apply_index);

            if !ready.entries().is_empty() {
                // Append entries to the Raft log.
                store.append_entries(ready.entries()).unwrap();
            }

            if let Some(updated_hs) = ready.hs() {
                if with_logging {
                    println!("Raft hard state changed to: {updated_hs:?}");
                }
                // Raft HardState changed, and we need to persist it.
                self.handle_hard_state_change(updated_hs);
            }

            if let Some(ss_change) = ready.ss() {
                self.handle_soft_state_change(ss_change);
            }

            if !ready.persisted_messages().is_empty() {
                // Send out the persisted messages come from the node.
                if with_logging {
                    println!("Persisted messages: {:?}", ready.persisted_messages());
                }
                self.send_messages(ready.take_persisted_messages()).await;
            }

            // Advance the Raft.
            let mut light_ready = self.raft_node.advance(ready);
            // Update commit index.
            if let Some(commit) = light_ready.commit_index() {
                self.handle_hard_state_commit_change(commit);
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

            if with_logging {
                // println!("<====== Raft node processed a ready state.");
                // println!("Last apply index: {last_apply_index}");
            }
        }
    }

    /// ToDo: This function should actually apply the committed entries to the state machine.
    ///
    /// However it currently pushes forwards ones to other peers via gRPC
    fn handle_committed_entries(&mut self, entries: Vec<Entry>, last_apply_index: &mut u64) {
        // println!("Handling {} committed entries", entries.len());

        for entry in entries {
            // Mostly, you need to save the last apply index to resume applying
            // after restart. Here we just ignore this because we use a Memory storage.
            *last_apply_index = entry.index;

            if entry.data.is_empty() {
                println!("Empty entry found. This means a new leader was elected.");
                continue;
            }

            match entry.get_entry_type() {
                EntryType::EntryNormal => self.handle_normal(entry),
                EntryType::EntryConfChange => self.handle_conf_change(entry),
                EntryType::EntryConfChangeV2 => self.handle_conf_change_v2(entry),
            }
        }
    }

    /// Handle soft state change.
    fn handle_soft_state_change(&self, new_soft_state: &SoftState) {
        println!("Raft node role changed to: {new_soft_state:?}");
        let new_role = format!("{:?}", new_soft_state.raft_state);
        let new_leader = new_soft_state.leader_id;

        // Update consensus state with new role
        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        extra_runtime.spawn(async move {
            let mut consensus_state = consensus_state.persistent.write().await;
            consensus_state.raft_info.role = new_role;
            consensus_state.raft_info.leader = new_leader;
        });
    }

    fn handle_hard_state_change(&self, new_hard_state: &raft::eraftpb::HardState) {
        println!("Raft hard state changed to: {new_hard_state:?}");

        // Update consensus state with new hard state
        let hs = new_hard_state.clone();
        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        extra_runtime.spawn(async move {
            let mut consensus_state = consensus_state.persistent.write().await;
            consensus_state.raft_info.term = hs.term;
            consensus_state.raft_info.commit = hs.commit;
            // consensus_state.raft_info.last_applied = ; // ToDo??
        });

        self.raft_node.store().set_hardstate(new_hard_state.clone());
    }

    fn handle_hard_state_commit_change(&self, commit: u64) {
        println!("Raft hard state commit changed to: {commit}");

        // Update consensus state with new commit index
        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        extra_runtime.spawn(async move {
            let mut consensus_state = consensus_state.persistent.write().await;
            consensus_state.raft_info.commit = commit;
        });

        self.raft_node.store().set_hardstate_commit(commit);
    }

    fn handle_normal(&self, entry: Entry) {
        // For normal proposals, extract the key-value pair and then
        // insert them into the kv engine.

        let data = ConsensusOperation::from_entry(&entry).expect("Entry data should be decodable");
        println!("Handled Normal entry data: {data}");
    }

    fn handle_conf_change(&mut self, entry: Entry) {
        let debuggable_entry = DebuggableEntry::from(&entry);
        debuggable_entry.log("Handling conf change entry:");

        let mut cc = ConfChange::default();
        PbMessage::merge_from_bytes(&mut cc, &entry.data).unwrap();
        let cs = self.raft_node.apply_conf_change(&cc).unwrap();
        self.raft_node.raft.store().set_conf_state(cs);

        // ToDo: Extract peer id and push to local state
        // for data in entry.data {
        //     println!("Conf change data: {data:?}");
        // }
    }

    fn handle_conf_change_v2(&self, _entry: Entry) {
        unimplemented!("Not implemented yet for conf change v2");
    }
}
