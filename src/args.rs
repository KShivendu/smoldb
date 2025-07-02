use clap::Parser;

#[derive(Parser)]
#[clap(version, about)]
pub struct Args {
    /// Url of the bootstrap node
    #[clap(short, long)]
    pub bootstrap: Option<String>,
    /// Url of the node
    #[clap(short, long, default_value = "0.0.0.0:9900")]
    pub url: String,
}

pub fn parse_args() -> Args {
    Args::parse()
}
