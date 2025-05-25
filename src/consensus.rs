use std::{error::Error, time::Duration};
use raft::{Config, RawNode, storage::MemStorage};
use slog::{Drain, o};
use crate::args::Args;

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

pub async fn run_consensus_loop(node: &mut RawNode<MemStorage>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        println!("Ticking");
        node.tick();
    }
}
