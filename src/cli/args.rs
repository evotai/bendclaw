use clap::Parser;

#[derive(Debug, Clone, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    StreamJson,
}

#[derive(Parser, Debug)]
#[command(name = "bendclaw", about = "Self-evolving AI agent runtime")]
pub struct CliArgs {
    #[arg(short, long)]
    pub prompt: String,

    #[arg(long)]
    pub resume: Option<String>,

    #[arg(long, value_enum, default_value = "text")]
    pub output_format: OutputFormat,

    #[arg(long)]
    pub model: Option<String>,
}
