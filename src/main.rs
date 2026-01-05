pub mod api;
pub mod args;
pub mod channel_service;
pub mod consensus;
pub mod error;
pub mod storage;
pub mod types;

use crate::api::{dispatcher::Dispatcher, start_http_server, start_p2p_server};
use crate::channel_service::ChannelService;
use crate::consensus::{manager::ConsensusManager, Consensus, ConsensusState};
use crate::storage::toc::TableOfContent;
use args::parse_args;
use slog::{o, Drain};
use slog_scope::GlobalLoggerGuard;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn setup_logging() -> GlobalLoggerGuard {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog::Filter::new(drain, |record| record.module().starts_with("smoldb")).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());

    let logger_guard = slog_scope::set_global_logger(logger);
    slog_stdlog::init_with_level(log::Level::Debug).unwrap();

    logger_guard
}

#[cfg(feature = "chrome-tracing")]
fn setup_chrome_tracing() {
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let (chrome_layer, guard) = ChromeLayerBuilder::new()
        .file("bench_trace.json")
        .include_args(true)
        .build();

    tracing_subscriber::registry().with(chrome_layer).init();

    // Leak the guard so traces are flushed when the process exits
    std::mem::forget(guard);
}

fn main() -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    color_backtrace::install();
    let _logger_guard = setup_logging();

    #[cfg(feature = "chrome-tracing")]
    setup_chrome_tracing();

    let args = parse_args();

    // Create a dedicated threadpool for internal gRPC and HTTP services while we also run the consensus loop
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(8) // TODO: Use number of CPUs for worker threads?
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("general-{}", id)
        })
        .build()
        .expect("Failed to create Tokio runtime");

    let consensus_async_runtime = rt.handle().clone();

    rt.block_on(async {
        let consensus_state = Arc::new(ConsensusState::new(args.p2p_url.clone(), args.peer_id));
        let channel_service = ChannelService::new(
            consensus_state.get_peer_id().await,
            consensus_state.peer_address_by_id.clone(),
        );

        let toc = TableOfContent::load(channel_service);
        let toc_arc = Arc::new(toc);

        let sender = Consensus::start(
            consensus_state.persistent.read().await.peer_id,
            args.bootstrap.clone(),
            consensus_state.clone(),
            toc_arc.clone(),
            consensus_async_runtime,
        )
        .expect("Failed to start consensus thread and loop");

        let consensus_manager = ConsensusManager::new(consensus_state.clone(), sender);

        let dispatcher = Dispatcher::from(toc_arc, Some(Arc::new(consensus_manager)));
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
    });

    Ok(())
}
