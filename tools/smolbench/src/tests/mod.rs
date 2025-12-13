mod utils;

use self::utils::start_smoldb;
use http::Uri;
use std::io::Write;
use std::str::FromStr;
use temp_dir::TempDir;

#[tokio::test]
async fn test_smoldb_consecutive_writes() -> Result<(), crate::error::SmolBenchError> {
    let peer_dir = TempDir::new().expect("Failed to create temp dir");
    let _node = start_smoldb(peer_dir.path(), "test_peer.log", 101, 9001, 5001, None).await;

    let uri = Uri::from_str("http://localhost:9001").unwrap();
    let collection_name = "benchmark".to_string();
    let num_points: usize = 100_000;
    let batch_size: usize = 100;
    let delay = None;

    let create_response =
        crate::apis::create_collection(&uri, &collection_name, false, true).await?;

    println!("Result: {}", &create_response.result);
    assert!(create_response.result);

    let expected_batch_count = num_points.div_ceil(batch_size);

    let upsert_response =
        crate::apis::upsert_points(&uri, &collection_name, num_points, batch_size, delay).await?;

    assert_eq!(upsert_response.len(), expected_batch_count);

    let get_points = crate::apis::read_points(&uri, &collection_name, None).await?;
    assert_eq!(get_points.result.points.len(), num_points);

    Ok(())
}

/// Test that reproduces the bug where Raft consensus re-applies all operations
/// when a node is restarted.
///
/// The bug: When a node restarts, Raft returns all committed entries from the log,
/// but the "last applied index" is not persisted, so all entries are re-applied
/// even though they were already applied before the restart.
///
/// This test should FAIL because operations are re-applied on restart. Once the bug
/// is fixed (by persisting the last applied index), this test should pass.
#[tokio::test]
async fn test_raft_no_reapply_on_restart() -> Result<(), crate::error::SmolBenchError> {
    let peer_dir = TempDir::new().expect("Failed to create temp dir");

    let uri = Uri::from_str("http://localhost:9001").unwrap();
    let collection_name = "benchmark".to_string();
    let num_points: usize = 100_000;
    let batch_size: usize = 100;

    // Phase 1: Append operations to consensus
    {
        let mut node = start_smoldb(peer_dir.path(), "test_peer.log", 101, 9001, 5001, None).await;
        let create_response =
            crate::apis::create_collection(&uri, &collection_name, false, true).await?;

        println!("Result: {}", &create_response.result);
        assert!(create_response.result);
        node.stop();
    }

    let cwd = std::env::current_dir().expect("Failed to get current directory");
    let log_path = cwd.join("test_peer.log"); // ToDo: Add logging

    let file = std::fs::OpenOptions::new()
        .append(true)
        .open(log_path)
        .expect("Failed to open log file");

    writeln!(
        &file,
        "=== Restarting node to test Raft re-application bug ==="
    )
    .expect("Failed to write to log file");

    // Phase 2: Restart node
    {
        let _node = start_smoldb(peer_dir.path(), "test_peer.log", 101, 9001, 5001, None).await;
        let create_response =
            crate::apis::create_collection(&uri, &collection_name, false, true).await?;

        println!("Result: {}", &create_response.result);
        assert!(create_response.result); // Must fail
    }

    Ok(())
}
