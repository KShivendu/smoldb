use crate::{error::SmolBenchError, types::ApiSuccessResponse};

pub async fn log_latencies<T>(
    batch_responses: &[ApiSuccessResponse<T>],
) -> Result<(), SmolBenchError> {
    let latencies = batch_responses
        .iter()
        .map(|res| res.time * 1000.0) // Convert s to ms
        .map(|latency| latency as f64)
        .collect::<Vec<_>>();

    let avg_latency = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let min_latency = latencies.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_latency = latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    println!("Avg batch server latency: {:.2} ms", avg_latency);
    println!("Min batch server latency: {:.2} ms", min_latency);
    println!("Max batch server latency: {:.2} ms", max_latency);

    Ok(())
}
