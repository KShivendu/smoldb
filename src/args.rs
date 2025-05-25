use clap::Parser;

#[derive(Parser)]
#[clap(version, about)]
pub struct Args {
    /// Url of the bootstrap node
    #[clap(short, long)]
    pub boostrap: Option<String>,
    /// Url of the node
    #[clap(short, long)]
    pub url: String,
}

pub fn parse_args() -> Args {
    Args::parse()
}
