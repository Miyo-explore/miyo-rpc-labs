//! Entry point for the `miyo-rpc` binary.

use clap::Parser;
use miyo_rpc::{CheckArgs, Cli, DiagnosticReport, OutputFormat};
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        miyo_rpc::Commands::Check(args) => run_check(args),
    }
}

fn run_check(args: CheckArgs) -> ExitCode {
    // We need a tokio runtime for async HTTP calls.
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    let report = rt.block_on(async {
        let timeout = Duration::from_secs(args.timeout_secs);
        DiagnosticReport::run_all(&args.url, timeout, args.method.as_deref()).await
    });

    match args.output {
        OutputFormat::Human => {
            print!("{}", miyo_rpc::output::format_human(&report));
        }
        OutputFormat::Json => match miyo_rpc::output::format_json(&report) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("Failed to serialize report to JSON: {}", e);
                return ExitCode::from(2);
            }
        },
    }

    if report.summary.failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
