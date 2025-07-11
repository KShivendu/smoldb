mod utils;

use self::utils::start_peer;
use http::Uri;
use std::str::FromStr;
use temp_dir::TempDir;

#[tokio::test]
async fn test_smoldb_consecutive_writes() -> Result<(), crate::error::SmolBenchError> {
    let peer_dir = TempDir::new().expect("Failed to create temp dir");
    let _ = start_peer(peer_dir.path(), "test_peer.log", 101, 9001, 5001, None);

    let uri = Uri::from_str("http://localhost:9001").unwrap();
    let collection_name = "benchmark".to_string();
    let num_points: usize = 100_000;
    let batch_size: usize = 100;
    let delay = None;

    if let Ok(create_response) = crate::apis::create_collection(&uri, &collection_name).await {
        println!("Result: {}", &create_response.result);
        assert!(create_response.result.is_object());
    };

    let expected_batch_count = num_points.div_ceil(batch_size);

    let upsert_response =
        crate::apis::upsert_points(&uri, &collection_name, num_points, batch_size, delay).await?;

    assert_eq!(upsert_response.len(), expected_batch_count);

    let get_points = crate::apis::retrieve_points(&uri, &collection_name, None).await?;
    assert_eq!(get_points.result.points.len(), num_points);

    Ok(())
}
