use crate::args::Args;
use raft::{prelude::{Entry, EntryType, Message}, storage::MemStorage, Config, RawNode};
use slog::{Drain, o};
use std::{
    collections::HashMap,
    error::Error,
    sync::mpsc::{RecvTimeoutError, channel},
    time::Duration,
};
use tokio::time::Instant;

const RAFT_TICK_INTERVAL: Duration = Duration::from_millis(100);

pub async fn init_consensus(_args: &Args) -> Result<RawNode<MemStorage>, Box<dyn Error>> {
    let config = Config {
        id: 1,
        ..Default::default()
    };

    let logger = slog::Logger::root(slog_stdlog::StdLog.fuse(), o!());
    let storage = MemStorage::new_with_conf_state((vec![1], vec![]));
    let node = RawNode::new(&config, storage, &logger)?;

    Ok(node)
}

enum Msg {
    Propose {
        id: u8,
        callback: Box<dyn Fn() + Send>,
    },
    #[expect(dead_code)]
    Raft(Message),
}

pub async fn run_consensus(node: &mut RawNode<MemStorage>) {
    run_consensus_receiver_loop(node).await;
    run_consensus_sender_loop(node).await;
}

async fn run_consensus_sender_loop(node: &mut RawNode<MemStorage>) {
    let (tx, rx) = channel();
    let mut remaining_timeout = RAFT_TICK_INTERVAL;

    let _ = tx.send(Msg::Propose {
        id: 1,
        callback: Box::new(|| ()),
    });

    let mut cbs = HashMap::new();
    loop {
        let now = Instant::now();

        match rx.recv_timeout(remaining_timeout) {
            Ok(Msg::Propose { id, callback }) => {
                cbs.insert(id, callback);
                let result = node.propose(vec![], vec![id]);
                println!("Propose result: {:?}", result);
            }
            Ok(Msg::Raft(m)) => {
                let result = node.step(m);
                println!("Raft result: {:?}", result);
            }
            Err(RecvTimeoutError::Timeout) => (),
            Err(RecvTimeoutError::Disconnected) => unimplemented!(),
        }

        let elapsed = now.elapsed();
        if elapsed >= remaining_timeout {
            remaining_timeout = RAFT_TICK_INTERVAL;
            // We drive Raft every 100ms.
            node.tick();
        } else {
            remaining_timeout -= elapsed;
        }
        break;
    }
}

async fn run_consensus_receiver_loop(node: &mut RawNode<MemStorage>) {
    loop {
        if !node.has_ready() {
            return;
        }

        // The Raft is ready, we can do something now.
        let mut ready = node.ready();

        // ToDo: Consensus snapshots

        //
        if !ready.messages().is_empty() {
            for msg in ready.take_messages() {
                println!("Sending message: {:?}", msg);
            }
        }

        let mut _last_apply_index = 0;
        for entry in ready.take_committed_entries() {
                // Mostly, you need to save the last apply index to resume applying
                // after restart. Here we just ignore this because we use a Memory storage.
                _last_apply_index = entry.index;

                if entry.data.is_empty() {
                    // Emtpy entry, when the peer becomes Leader it will send an empty entry.
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
