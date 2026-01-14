use crate::storage::segment::index::vector::DimType;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// SIMD-accelerated cosine similarity for unit vectors.
/// Assumes both vectors are normalized to unit length.
/// For unit vectors: cosine_similarity(a, b) = a · b (just the dot product)
pub fn cosine_similarity_unit_simd(a: &[DimType], b: &[DimType]) -> f64 {
    debug_assert_eq!(a.len(), b.len(), "Vectors must have the same length");

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx") && is_x86_feature_detected!("fma") {
            return unsafe { avx_dot_product_f64(a, b) };
        }
    }

    // Scalar fallback: dot product for unit vectors
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// AVX + FMA accelerated dot product for f64 vectors.
///
/// Uses 256-bit YMM registers to process 4 f64 values at a time.
/// FMA (Fused Multiply-Add) computes a*b+c in a single instruction.
///
/// Register usage:
/// - YMM0-YMM3: Accumulators (4 accumulators to hide latency, unrolled 4x)
/// - YMM4-YMM7: Loaded values from v1
/// - YMM8-YMM11: Loaded values from v2
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
#[target_feature(enable = "fma")]
unsafe fn avx_dot_product_f64(v1: &[DimType], v2: &[DimType]) -> f64 {
    debug_assert_eq!(v1.len(), v2.len());

    let len = v1.len();
    let mut ptr1 = v1.as_ptr();
    let mut ptr2 = v2.as_ptr();

    // 4 accumulators to exploit instruction-level parallelism
    // Each accumulator holds 4 x f64 partial sums
    let mut acc0 = _mm256_setzero_pd();
    let mut acc1 = _mm256_setzero_pd();
    let mut acc2 = _mm256_setzero_pd();
    let mut acc3 = _mm256_setzero_pd();

    // Process 16 f64s per iteration (4 accumulators × 4 f64s each)
    let chunks_16 = len / 16;
    for _ in 0..chunks_16 {
        // Load 4 chunks of 4 f64s from each vector
        let a0 = _mm256_loadu_pd(ptr1);
        let b0 = _mm256_loadu_pd(ptr2);
        let a1 = _mm256_loadu_pd(ptr1.add(4));
        let b1 = _mm256_loadu_pd(ptr2.add(4));
        let a2 = _mm256_loadu_pd(ptr1.add(8));
        let b2 = _mm256_loadu_pd(ptr2.add(8));
        let a3 = _mm256_loadu_pd(ptr1.add(12));
        let b3 = _mm256_loadu_pd(ptr2.add(12));

        // FMA: acc = a * b + acc
        acc0 = _mm256_fmadd_pd(a0, b0, acc0);
        acc1 = _mm256_fmadd_pd(a1, b1, acc1);
        acc2 = _mm256_fmadd_pd(a2, b2, acc2);
        acc3 = _mm256_fmadd_pd(a3, b3, acc3);

        ptr1 = ptr1.add(16);
        ptr2 = ptr2.add(16);
    }

    // Handle remaining chunks of 4
    let remaining = len % 16;
    let chunks_4 = remaining / 4;
    for _ in 0..chunks_4 {
        let a = _mm256_loadu_pd(ptr1);
        let b = _mm256_loadu_pd(ptr2);
        acc0 = _mm256_fmadd_pd(a, b, acc0);
        ptr1 = ptr1.add(4);
        ptr2 = ptr2.add(4);
    }

    // Combine all 4 accumulators into one
    acc0 = _mm256_add_pd(acc0, acc1);
    acc2 = _mm256_add_pd(acc2, acc3);
    acc0 = _mm256_add_pd(acc0, acc2);

    // Horizontal sum of the 4 f64s in acc0
    // acc0 = [a, b, c, d]
    // After hadd: [a+b, a+b, c+d, c+d] (hadd adds adjacent pairs)
    let sum1 = _mm256_hadd_pd(acc0, acc0); // [a+b, a+b, c+d, c+d]

    // Extract high and low 128-bit lanes and add them
    let low = _mm256_castpd256_pd128(sum1); // [a+b, a+b]
    let high = _mm256_extractf128_pd(sum1, 1); // [c+d, c+d]
    let sum128 = _mm_add_pd(low, high); // [a+b+c+d, a+b+c+d]

    let mut result = 0.0f64;
    _mm_store_sd(&mut result, sum128);

    // Handle remaining elements (0-3) with scalar operations
    let tail_start = len - (remaining % 4);
    for i in tail_start..len {
        result += v1.get_unchecked(i) * v2.get_unchecked(i);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_unit_simd() {
        // Test with unit vectors
        let a: Vec<f64> = vec![1.0, 0.0, 0.0, 0.0];
        let b: Vec<f64> = vec![1.0, 0.0, 0.0, 0.0];
        let result = cosine_similarity_unit_simd(&a, &b);
        assert!(
            (result - 1.0).abs() < 1e-10,
            "Same unit vectors should have similarity 1.0"
        );

        // Orthogonal unit vectors
        let a: Vec<f64> = vec![1.0, 0.0, 0.0, 0.0];
        let b: Vec<f64> = vec![0.0, 1.0, 0.0, 0.0];
        let result = cosine_similarity_unit_simd(&a, &b);
        assert!(
            (result - 0.0).abs() < 1e-10,
            "Orthogonal vectors should have similarity 0.0"
        );

        // Opposite unit vectors
        let a: Vec<f64> = vec![1.0, 0.0, 0.0, 0.0];
        let b: Vec<f64> = vec![-1.0, 0.0, 0.0, 0.0];
        let result = cosine_similarity_unit_simd(&a, &b);
        assert!(
            (result - (-1.0)).abs() < 1e-10,
            "Opposite vectors should have similarity -1.0"
        );
    }

    #[test]
    fn test_cosine_similarity_unit_simd_larger() {
        // Test with larger vectors (exercises the SIMD path more thoroughly)
        let n = 128;
        let norm = (n as f64).sqrt();
        let a: Vec<f64> = (0..n).map(|_| 1.0 / norm).collect();
        let b: Vec<f64> = (0..n).map(|_| 1.0 / norm).collect();

        let simd_result = cosine_similarity_unit_simd(&a, &b);
        let scalar_result: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();

        assert!(
            (simd_result - scalar_result).abs() < 1e-10,
            "SIMD result {} should match scalar result {}",
            simd_result,
            scalar_result
        );
    }

    #[test]
    fn test_cosine_similarity_unit_simd_odd_length() {
        // Test with non-aligned vector lengths
        for len in [1, 3, 5, 7, 13, 17, 31, 33, 63, 65] {
            let norm = (len as f64).sqrt();
            let a: Vec<f64> = (0..len).map(|_| 1.0 / norm).collect();
            let b: Vec<f64> = (0..len).map(|_| 1.0 / norm).collect();

            let simd_result = cosine_similarity_unit_simd(&a, &b);
            let scalar_result: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();

            assert!(
                (simd_result - scalar_result).abs() < 1e-9,
                "Length {}: SIMD {} != scalar {}",
                len,
                simd_result,
                scalar_result
            );
        }
    }
}
