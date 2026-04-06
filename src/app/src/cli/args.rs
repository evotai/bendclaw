use clap::Args;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;

#[derive(Parser, Debug)]
#[command(name = "bendclaw", about = "Self-evolving AI agent runtime")]
pub struct CliArgs {
    #[arg(short = 'p', long)]
    pub prompt: Option<String>,

    #[arg(long)]
    pub verbose: bool,

    #[arg(long)]
    pub resume: Option<String>,

    #[arg(long, default_value_t = OutputFormat::Text)]
    pub output_format: OutputFormat,

    #[arg(long, default_value_t = 512)]
    pub max_turns: u32,

    #[arg(long, default_value_t = 100_000_000)]
    pub max_tokens: u64,

    #[arg(long, value_name = "SECS", default_value_t = 3600)]
    pub max_duration: u64,

    #[arg(long)]
    pub append_system_prompt: Option<String>,

    #[arg(long)]
    pub model: Option<String>,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    Repl,
    Server(ServerArgs),
}

#[derive(Args, Debug)]
pub struct ServerArgs {
    #[arg(long)]
    pub port: Option<u16>,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    StreamJson,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::StreamJson => write!(f, "stream-json"),
        }
    }
}
