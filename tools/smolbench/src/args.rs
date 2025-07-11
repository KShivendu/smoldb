use clap::Parser;
use http::Uri;

fn parse_number_impl(n: &str) -> Option<u64> {
    let mut chars = n.chars();
    let mut result = chars.next()?.to_digit(10)? as u64;

    while let Some(c) = chars.next() {
        if let Some(v) = c.to_digit(10) {
            result = result.checked_mul(10)?.checked_add(v as u64)?;
        } else if c != '_' {
            let power = "kMBT".find(c)? as u32 + 1;
            let multiplier = match chars.next() {
                Some('i') => 1024u64.pow(power),
                Some(_) => return None,
                None => 1000u64.pow(power),
            };
            return result.checked_mul(multiplier);
        }
    }

    Some(result)
}

fn parse_number(n: &str) -> Result<usize, String> {
    parse_number_impl(n)
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| format!("Invalid number: {n}"))
}

#[derive(Parser, Debug)]
#[clap(version, about)]
pub struct Args {
    /// Smoldb URI
    #[clap(short, long, default_value = "http://localhost:9001")]
    pub uri: Uri,

    /// Name of the collection
    #[clap(short, long, default_value = "benchmark")]
    pub collection_name: String,

    /// Number of points to upload
    #[clap(short, long, default_value = "100k", value_parser = parse_number)]
    pub num_points: usize,

    /// Batch size for upsert operations
    #[clap(short, long, default_value = "1000", value_parser = parse_number)]
    pub batch_size: usize,

    /// Skip collection creation
    #[clap(long, default_value = "false")]
    pub skip_create: bool,

    /// Skip collection creation if it already exists
    #[clap(long, default_value = "false")]
    pub skip_if_exists: bool,

    /// Use if you don't want to upsert points by default
    #[clap(long, default_value = "false")]
    pub skip_upsert: bool,

    /// Check whether to query after upsert
    #[clap(long, default_value = "false")]
    pub skip_query: bool,

    /// Number of 9 digits to show in p99* results
    /// Defaults to 3, i.e. shows only p99
    #[clap(long, long, default_value_t = 2, value_parser = parse_number)]
    pub p9: usize,

    /// Delay between requests (batches) in milliseconds
    #[clap(short, long, default_value = None, value_parser = parse_number)]
    pub delay: Option<usize>,
}

pub fn parse_args() -> Args {
    Args::parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number() {
        assert_eq!(parse_number("100").unwrap(), 100);
        assert_eq!(parse_number("1k").unwrap(), 1000);
        assert_eq!(parse_number("1M").unwrap(), 1_000_000);
        assert_eq!(parse_number("1B").unwrap(), 1_000_000_000);

        // Error cases:
        assert!(parse_number("1K").is_err());
        assert!(parse_number("1b").is_err());
    }
}
