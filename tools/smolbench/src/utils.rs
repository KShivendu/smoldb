use crate::{error::SmolBenchError, types::ApiSuccessResponse};

pub async fn log_latencies<T>(
    batch_responses: &[ApiSuccessResponse<T>],
    for_what: &str,
) -> Result<(), SmolBenchError> {
    let latencies = batch_responses
        .iter()
        .map(|res| res.time * 1000.0) // Convert s to ms
        .map(|latency| latency as f64)
        .collect::<Vec<_>>();

    let avg_latency = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let min_latency = latencies.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_latency = latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    println!("Avg batch server {for_what} latency: {avg_latency:.2} ms");
    println!("Min batch server {for_what} latency: {min_latency:.2} ms");
    println!("Max batch server {for_what} latency: {max_latency:.2} ms");

    Ok(())
}
