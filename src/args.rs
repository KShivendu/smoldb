use clap::Parser;
use http::Uri;

#[derive(Parser)]
#[clap(version, about)]
pub struct Args {
    /// Url of the bootstrap node
    #[clap(short, long)]
    pub bootstrap: Option<Uri>,
    /// Url of the node
    #[clap(short, long, default_value = "http://0.0.0.0:9900")]
    pub url: Uri,
    /// Url of the node
    #[clap(short, long, default_value = "http://0.0.0.0:9920")]
    pub p2p_url: Uri,
}

pub fn parse_args() -> Args {
    Args::parse()
}
