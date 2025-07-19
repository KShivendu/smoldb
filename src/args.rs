use clap::Parser;
use http::Uri;

fn parse_uri(s: &str) -> Result<Uri, http::uri::InvalidUri> {
    if s.contains("://") {
        s.parse::<Uri>()
    } else {
        format!("http://{s}").parse::<Uri>()
    }
}

#[derive(Parser)]
#[clap(version, about)]
pub struct Args {
    /// Url of the bootstrap node
    #[clap(short, long, value_parser = parse_uri)]
    pub bootstrap: Option<Uri>,
    /// Url of the node
    #[clap(short, long, default_value = "0.0.0.0:9000", value_parser = parse_uri)]
    pub url: Uri,
    /// Url of the node
    #[clap(short, long, default_value = "0.0.0.0:5000", value_parser = parse_uri)]
    pub p2p_url: Uri,
    /// Peer id
    #[clap(long)]
    pub peer_id: Option<u64>,
}

pub fn parse_args() -> Args {
    Args::parse()
}
