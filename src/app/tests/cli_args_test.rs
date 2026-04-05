use bendclaw::cli::CliArgs;
use bendclaw::cli::CliCommand;
use bendclaw::cli::OutputFormat;
use clap::Parser;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[test]
fn parse_prompt_mode_args() -> TestResult {
    let args = CliArgs::try_parse_from([
        "bendclaw",
        "--verbose",
        "--max-turns",
        "3",
        "--append-system-prompt",
        "be concise",
        "-p",
        "hello",
        "--resume",
        "session-1",
        "--output-format",
        "stream-json",
        "--model",
        "claude-sonnet-4-20250514",
    ])?;

    assert_eq!(args.prompt.as_deref(), Some("hello"));
    assert!(args.verbose);
    assert_eq!(args.resume.as_deref(), Some("session-1"));
    assert!(matches!(args.output_format, OutputFormat::StreamJson));
    assert_eq!(args.max_turns, Some(3));
    assert_eq!(args.append_system_prompt.as_deref(), Some("be concise"));
    assert_eq!(args.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert!(args.command.is_none());
    Ok(())
}

#[test]
fn parse_server_subcommand_args() -> TestResult {
    let args =
        CliArgs::try_parse_from(["bendclaw", "--model", "gpt-4o", "server", "--port", "9090"])?;

    assert!(args.prompt.is_none());
    assert_eq!(args.model.as_deref(), Some("gpt-4o"));

    match args.command {
        Some(CliCommand::Server(server)) => {
            assert_eq!(server.port, Some(9090));
        }
        Some(CliCommand::Repl) => {
            return Err(std::io::Error::other("unexpected repl subcommand").into());
        }
        None => {
            return Err(std::io::Error::other("missing server subcommand").into());
        }
    }

    Ok(())
}

#[test]
fn parse_repl_subcommand_args() -> TestResult {
    let args = CliArgs::try_parse_from(["bendclaw", "--model", "gpt-4o", "repl"])?;
    assert!(args.prompt.is_none());
    assert_eq!(args.model.as_deref(), Some("gpt-4o"));
    assert!(matches!(args.command, Some(CliCommand::Repl)));
    Ok(())
}

#[test]
fn parse_default_repl_args() -> TestResult {
    let args = CliArgs::try_parse_from(["bendclaw"])?;
    assert!(args.prompt.is_none());
    assert!(args.command.is_none());
    Ok(())
}
