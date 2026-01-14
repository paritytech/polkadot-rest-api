use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to .env file (e.g., .env.polkadot)
    #[arg(short, long, default_value = ".env")]
    pub env_file: String,
}

impl Args {
    /// Parse command line arguments
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
