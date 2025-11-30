use crate::{
    consensus::{
        debuggables::DebuggableEntry, utils::add_peer_to_toc_and_consensus_state, Consensus,
        ConsensusOperation, ProposalId,
    },
    error::ConsensusResult,
};
use http::Uri;
use log::{info, warn};
use protobuf::Message as ProtobufMessage;
use raft::{
    prelude::{ConfChange, Entry, EntryType, Snapshot},
    SoftState,
};
use std::collections::HashMap;

impl Consensus {
    /// Tries to process raft's ready state. Should be called on each tick.
    ///
    /// [`raft::Ready`] is the outstanding work that the application needs to handle.
    ///
    /// [`raft::LightReady`] is the result of processing the ready state that might need further processing.
    pub async fn on_ready(
        &mut self,
        _cbs: &mut HashMap<ProposalId, Box<dyn Fn() + Send>>,
    ) -> ConsensusResult<()> {
        loop {
            if !self.raft_node.has_ready() {
                return Ok(());
            }

            let store = self.raft_node.raft.raft_log.store.clone();
            let mut ready = self.raft_node.ready();

            // let debuggable_ready = DebuggableReady::from(&ready);
            // debuggable_ready.log("\n\n\n=====> Raft node is ready, processing ready state");

            if !ready.messages().is_empty() {
                // Send out messages to other peers.
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
            self.handle_committed_entries(ready.take_committed_entries(), &mut last_apply_index)?;

            if !ready.entries().is_empty() {
                // These are not committed entries yet, but need to be persisted to storage first.
                store.append_entries(ready.entries())?;
            }

            if let Some(updated_hs) = ready.hs() {
                // Handle hard state was updated probably because of new term or commit.
                self.handle_hard_state_change(updated_hs)?;
            }

            if let Some(ss_change) = ready.ss() {
                // Handle soft state change probably because of role or leader change.
                self.handle_soft_state_change(ss_change);
            }

            if !ready.persisted_messages().is_empty() {
                // Send out persisted messages to other peers.
                self.send_messages(ready.take_persisted_messages()).await;
            }

            // Advance the Raft node internal state with the ready state.
            let mut light_ready = self.raft_node.advance(ready);
            // Process the light ready state if it suggest further actions.
            if let Some(commit) = light_ready.commit_index() {
                self.handle_hard_state_commit_change(commit)?;
            }
            self.send_messages(light_ready.take_messages()).await;
            self.handle_committed_entries(
                light_ready.take_committed_entries(),
                &mut last_apply_index,
            )?;
            self.raft_node.advance_apply();
        }
    }

    /// ToDo: This function should actually apply the committed entries to the state machine.
    ///
    /// However it currently pushes forwards ones to other peers via gRPC
    fn handle_committed_entries(
        &mut self,
        entries: Vec<Entry>,
        last_apply_index: &mut u64,
    ) -> ConsensusResult<()> {
        for entry in entries {
            // ToDo: Save the last apply index to resume applying after restart.
            // Here we just ignore this because we use a Memory storage.
            *last_apply_index = entry.index;

            if entry.data.is_empty() {
                // Empty entry found. This means a new leader was elected.
                continue;
            }

            match entry.get_entry_type() {
                EntryType::EntryNormal => self.handle_normal(entry),
                EntryType::EntryConfChange => self.handle_conf_change(entry),
                EntryType::EntryConfChangeV2 => self.handle_conf_change_v2(entry),
            }?
        }

        Ok(())
    }

    /// Handle soft state change.
    fn handle_soft_state_change(&self, new_soft_state: &SoftState) {
        info!("Raft node soft state changed to: {new_soft_state:?}");
        let new_role = format!("{:?}", new_soft_state.raft_state);
        let new_leader = new_soft_state.leader_id;

        // Update consensus state with new role
        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        extra_runtime.spawn(async move {
            let mut consensus_state = consensus_state.write_persistent();
            consensus_state.raft_info.role = new_role;
            consensus_state.raft_info.leader = new_leader;
        });
    }

    fn handle_hard_state_change(
        &self,
        new_hard_state: &raft::eraftpb::HardState,
    ) -> ConsensusResult<()> {
        info!("Raft hard state changed to: {new_hard_state:?}");

        // Update consensus state with new hard state
        let hs = new_hard_state.clone();
        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        extra_runtime.spawn(async move {
            let mut consensus_state = consensus_state.write_persistent();
            consensus_state
                .apply_state_update(|s| {
                    s.raft_info.term = hs.term;
                    s.raft_info.commit = hs.commit;
                })
                .unwrap();
            // consensus_state.raft_info.term = hs.term;
            // consensus_state.raft_info.commit = hs.commit;
            // consensus_state.raft_info.last_applied = ; // ToDo??
        });

        self.raft_node.store().set_hardstate(new_hard_state.clone())
    }

    fn handle_hard_state_commit_change(&self, commit: u64) -> ConsensusResult<()> {
        info!("Raft hard state commit changed to: {commit}"); // why not happening??

        // Update consensus state with new commit index
        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        extra_runtime.spawn(async move {
            let mut consensus_state = consensus_state
                .persistent
                .write()
                .expect("Failed to acquire persistent state write lock");
            consensus_state.raft_info.commit = commit;
            consensus_state
                .save()
                .expect("Failed to save persistent state");
        });

        self.raft_node.store().set_hardstate_commit(commit)
    }

    fn handle_normal(&self, entry: Entry) -> ConsensusResult<()> {
        let operation =
            ConsensusOperation::from_entry(&entry).expect("Entry data should be decodable");
        info!("Operation to apply: {operation:?}");

        let extra_runtime = self.runtime.clone();
        let consensus_state = self.consensus_state.clone();
        let toc = self.toc.clone();

        match operation {
            ConsensusOperation::AddPeer { peer_id, uri } => {
                let uri = uri.parse::<Uri>().expect("Failed to parse URI");
                extra_runtime.spawn(async move {
                    add_peer_to_toc_and_consensus_state(&consensus_state, &toc, peer_id, uri, true)
                        .await
                        .expect("Failed to add peer to consensus state and TOC");
                })
            }
            ConsensusOperation::CollectionOp(op) => extra_runtime.spawn(async move {
                // ToDo: Notify leader if the operation gives an error?
                toc.perform_collection_op(op)
                    .await
                    .expect("Failed to handle collection operation");
            }),
            _ => extra_runtime.spawn(async move {
                warn!("Ignored consensus operation: {operation:?}");
            }),
        };

        Ok(())
    }

    fn handle_conf_change(&mut self, entry: Entry) -> ConsensusResult<()> {
        let debuggable_entry = DebuggableEntry::from(&entry);
        debuggable_entry.log("Handling conf change entry");

        let mut cc = ConfChange::default();
        ProtobufMessage::merge_from_bytes(&mut cc, &entry.data).unwrap();
        let cs = self.raft_node.apply_conf_change(&cc).unwrap();
        self.raft_node.raft.store().set_conf_state(cs)?;
        Ok(())
    }

    fn handle_conf_change_v2(&self, _entry: Entry) -> ConsensusResult<()> {
        unimplemented!("Not implemented yet for conf change v2");
    }
}
