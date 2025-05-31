use crate::args::Args;
use raft::{
    Config, RawNode,
    prelude::{Entry, EntryType, Message},
    storage::MemStorage,
};
use slog::{Drain, Logger, o};
use std::{
    collections::HashMap,
    error::Error,
    sync::mpsc::{self, RecvTimeoutError, channel},
    thread,
    time::Duration,
};
use tokio::time::Instant;

const RAFT_TICK_INTERVAL: Duration = Duration::from_millis(100);

pub async fn init_consensus(
    _args: &Args,
) -> Result<(RawNode<MemStorage>, slog::Logger), Box<dyn Error>> {
    let storage = MemStorage::new_with_conf_state((vec![1], vec![]));
    let logger = slog::Logger::root(slog_stdlog::StdLog.fuse(), o!());

    let config = Config {
        id: 1, // The unique ID for the Raft node
        ..Default::default()
    };
    let raft = RawNode::new(&config, storage, &logger)?;

    Ok((raft, logger))
}

enum Msg {
    Propose {
        id: u8,
        callback: Box<dyn Fn() + Send>,
    },
    #[expect(dead_code)]
    Raft(Message),
}

/// Spawn a thread to **continuously** send a proposal to mpsc::Sender (eventually to Raft).
fn send_propose(logger: Logger, sender: mpsc::Sender<Msg>) {
    thread::spawn(move || {
        let mut counter = 0;
        loop {
            thread::sleep(Duration::from_secs(3));
            println!("proposed a request");
            counter += 1;

            // let (s1, r1) = mpsc::channel::<u8>();
            sender
                .send(Msg::Propose {
                    id: counter,
                    callback: Box::new(move || {
                        // s1.send(0).unwrap();
                        println!("Propose callback executed");
                    }),
                })
                .unwrap();
            // let n = r1.recv().unwrap();
            // assert_eq!(n, 0);

            println!("finished the proposal callback");
        }
    });
}

pub async fn run_consensus(logger: Logger, raft_node: &mut RawNode<MemStorage>) {
    let (sender, receiver) = channel();

    // If you don't clone sender, you get Disconnected error
    send_propose(logger.clone(), sender.clone());

    run_consensus_receiver_loop(raft_node, receiver).await
}

async fn run_consensus_receiver_loop(
    raft_node: &mut RawNode<MemStorage>,
    receiver: mpsc::Receiver<Msg>,
) {
    let mut t = Instant::now();
    let mut timeout = RAFT_TICK_INTERVAL;

    let mut cbs = HashMap::new();

    loop {
        // Wait for a message or timeout (whichever happens first) to proceed with Raft tick
        match receiver.recv_timeout(timeout) {
            Ok(Msg::Propose { id, callback }) => {
                println!("Received proposal with ID: {}", id);
                cbs.insert(id, callback);
                // ToDo: Data needs to be converted to CBOR format

                // Note: this returns ProposalDropped when the ID is repeated.
                raft_node
                    .propose(vec![], vec![id])
                    .expect(&format!("failed to propose entry with {id}"));
            }
            Ok(Msg::Raft(m)) => {
                raft_node.step(m).unwrap();
            }
            Err(RecvTimeoutError::Timeout) => {
                // Timeout occurred, checking Raft node...
            }
            Err(RecvTimeoutError::Disconnected) => {
                println!("ERROR: Receiver disconnected, exiting loop.");
                return;
            }
        }

        let d = t.elapsed();
        t = Instant::now();

        if d >= timeout {
            timeout = RAFT_TICK_INTERVAL;
            // We drive Raft every 100ms.
            raft_node.tick();
        } else {
            timeout -= d;
        }

        on_ready(raft_node, &mut cbs);
    }
}

fn on_ready(raft_node: &mut RawNode<MemStorage>, _cbs: &mut HashMap<u8, Box<dyn Fn() + Send>>) {
    loop {
        if !raft_node.has_ready() {
            return;
        }

        println!("Raft node is ready, processing message...");

        let store = raft_node.raft.raft_log.store.clone();

        // The Raft is ready, we can do something now.
        let mut ready = raft_node.ready();

        // ToDo: Consensus snapshots

        if !ready.messages().is_empty() {
            send_messages(ready.take_messages());
        }

        let mut last_apply_index = 0;
        handle_committed_entries(ready.take_committed_entries(), &mut last_apply_index);

        if !ready.entries().is_empty() {
            // Append entries to the Raft log.
            store.wl().append(ready.entries()).unwrap();
        }

        if let Some(hs) = ready.hs() {
            // Raft HardState changed, and we need to persist it.
            store.wl().set_hardstate(hs.clone());
        }

        if !ready.persisted_messages().is_empty() {
            // Send out the persisted messages come from the node.
            send_messages(ready.take_persisted_messages());
        }

        // Advance the Raft.
        let mut light_ready = raft_node.advance(ready);
        // Update commit index.
        if let Some(commit) = light_ready.commit_index() {
            store.wl().mut_hard_state().set_commit(commit);
        }
        // Send out the messages to other peers.
        send_messages(light_ready.take_messages());
        // Apply all committed entries.
        handle_committed_entries(light_ready.take_committed_entries(), &mut last_apply_index);
        // Advance the apply index.
        raft_node.advance_apply();

        println!("Raft node processed a ready state.");
    }
}

/// Send out the messages to other peers
fn send_messages(messages: Vec<Message>) {
    for msg in messages {
        println!("Sending message: {:?}", msg);
    }
}

/// Handle committed entries
fn handle_committed_entries(entries: Vec<Entry>, last_apply_index: &mut u64) {
    for entry in entries {
        // Mostly, you need to save the last apply index to resume applying
        // after restart. Here we just ignore this because we use a Memory storage.
        *last_apply_index = entry.index;

        if entry.data.is_empty() {
            // Empty entry, when the peer becomes Leader it will send an empty entry.
            continue;
        }

        match entry.get_entry_type() {
            EntryType::EntryNormal => handle_normal(entry),
            // It's recommended to always use `EntryType::EntryConfChangeV2.
            EntryType::EntryConfChange => handle_conf_change(entry),
            EntryType::EntryConfChangeV2 => handle_conf_change_v2(entry),
        }
    }
}

fn handle_normal(entry: Entry) {
    println!("Handle normal entry: {:?}", entry);
}

fn handle_conf_change(entry: Entry) {
    println!("Handle conf change entry: {:?}", entry);
}

fn handle_conf_change_v2(entry: Entry) {
    println!("Handle conf change v2 entry: {:?}", entry);
}
