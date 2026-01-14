use crate::storage::segment::index::vector::DimType;

pub fn cosine_similarity(a: &[DimType], b: &[DimType]) -> f64 {
    let dot_product = a.iter().zip(b.iter()).map(|(a, b)| a * b).sum::<f64>();
    let a_norm = a.iter().map(|a| a * a).sum::<f64>().sqrt();
    let b_norm = b.iter().map(|b| b * b).sum::<f64>().sqrt();
    dot_product / (a_norm * b_norm)
}
