use crate::{error::SmolBenchError, types::ApiSuccessResponse};

pub async fn log_latencies<T>(
    batch_responses: &[ApiSuccessResponse<T>],
    p9: usize,
    for_what: &str,
) -> Result<(), SmolBenchError> {
    let mut latencies = batch_responses
        .iter()
        .map(|res| res.time * 1000.0) // Convert s to ms
        .map(|latency| latency as f64)
        .collect::<Vec<_>>();

    latencies.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());

    let avg_latency = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let min_latency = latencies.first().unwrap();
    let max_latency = latencies.last().unwrap();
    let p50_latency = latencies[(latencies.len() as f32 * 0.50) as usize];

    println!("Avg {for_what} latency: {avg_latency:.2} ms");
    println!("Min {for_what} latency: {min_latency:.2} ms");
    println!("p50 {for_what} latency: {p50_latency:.2} ms");

    let p95_latency = latencies[(latencies.len() as f32 * 0.95) as usize];
    println!("p95 {for_what} latency: {p95_latency:.2} ms");

    for digits in 2..=p9 {
        let factor = 1.0 - 1.0 * 0.1f64.powf(digits as f64);
        let index = ((latencies.len() as f64 * factor) as usize).min(latencies.len() - 1);
        let nines = "9".repeat(digits);
        let time = latencies[index];
        println!("p{} {for_what} latency: {:.2} ms", nines, time);
    }

    println!("Max {for_what} latency: {max_latency:.2} ms");

    Ok(())
}
