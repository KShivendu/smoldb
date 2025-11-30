pub mod api;
pub mod args;
pub mod channel_service;
pub mod consensus;
pub mod error;
pub mod storage;
pub mod types;

use crate::api::{dispatcher::Dispatcher, start_http_server, start_p2p_server};
use crate::channel_service::ChannelService;
use crate::consensus::{state::ConsensusState, Consensus};
use crate::storage::toc::TableOfContent;
use args::parse_args;
use slog::{o, Drain};
use slog_scope::GlobalLoggerGuard;
use smoldb::storage::toc::STORAGE_DIR;
use std::{path::Path, sync::Arc};

fn setup_logging() -> GlobalLoggerGuard {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog::Filter::new(drain, |record| record.module().starts_with("smoldb")).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());

    let logger_guard = slog_scope::set_global_logger(logger);
    slog_stdlog::init_with_level(log::Level::Debug).unwrap(); // TODO: Rely on environment variable RUST_LOG for level

    logger_guard
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    color_backtrace::install();
    let _logger_guard = setup_logging();

    let args = parse_args();

    // Create a dedicated thread for internal gRPC service while we also run Actix Web server
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(8)
        .thread_name("general")
        .build()
        .expect("Failed to create Tokio runtime");

    let consensus_async_runtime = rt.handle().clone();

    let consensus_state = Arc::new(ConsensusState::new(
        Path::new(STORAGE_DIR).to_path_buf(),
        args.p2p_url.clone(),
        args.peer_id,
    ));
    let channel_service = ChannelService::new(
        consensus_state.get_peer_id(),
        consensus_state.peer_address_by_id.clone(),
    );

    let toc = TableOfContent::load(channel_service);
    let toc_arc = Arc::new(toc);

    let consensus_manager = Consensus::start(
        consensus_state.get_peer_id(),
        args.bootstrap.clone(),
        consensus_state.clone(),
        toc_arc.clone(),
        consensus_async_runtime,
    )
    .expect("Failed to start consensus thread and loop");

    let dispatcher = Dispatcher::from(toc_arc, Some(consensus_manager));
    let dispatcher_arc = Arc::new(dispatcher);

    let rt_http = rt.handle().clone();
    let http_dispatcher_arc = dispatcher_arc.clone();
    let http_handle = std::thread::spawn(move || {
        rt_http.block_on(async {
            start_http_server(args.url, http_dispatcher_arc)
                .await
                .expect("HTTP Server stopped")
        });
    });

    // Start p2p gRPC server on the same Tokio runtime
    let rt_p2p = rt.handle().clone();
    let p2p_handle = std::thread::spawn(move || {
        rt_p2p.block_on(async {
            start_p2p_server(args.p2p_url, dispatcher_arc)
                .await
                .expect("gRPC Server stopped")
        });
    });

    http_handle.join().expect("HTTP server thread panicked");
    p2p_handle.join().expect("gRPC server thread panicked");

    Ok(())
}
