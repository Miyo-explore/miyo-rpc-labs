//! Entry point for the `miyo-rpc` binary.

use clap::Parser;
use miyo_rpc::{BatchArgs, CheckArgs, Cli, DiagnosticReport, OutputFormat};
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        miyo_rpc::Commands::Check(args) => run_check(args),
        miyo_rpc::Commands::Batch(args) => run_batch(args),
    }
}

fn run_check(args: CheckArgs) -> ExitCode {
    // Validate --expected-network before doing any network work.
    if let Some(net) = &args.expected_network {
        if miyo_rpc::network_passphrase(net).is_none() {
            eprintln!(
                "error: unknown network '{net}'. Valid values: {}.",
                miyo_rpc::known_networks().join(", ")
            );
            return ExitCode::from(2);
        }
    }

    // We need a tokio runtime for async HTTP calls.
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let timeout = Duration::from_secs(args.timeout_secs);

    let mut report = rt.block_on(async {
        DiagnosticReport::run_all(
            &args.url,
            timeout,
            args.method.as_deref(),
            std::cmp::min(args.runs.max(1), 10) as usize,
        )
        .await
    });

    report.enforce_expected_network(args.expected_network.as_deref());

    if let Some(compare_url) = &args.compare {
        let compare_report = rt.block_on(async {
            DiagnosticReport::run_all(
                compare_url,
                timeout,
                None, // run all checks on the comparison endpoint
                std::cmp::min(args.runs.max(1), 10) as usize,
            )
            .await
        });
        report.compare_with(&compare_report);
    }

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

fn run_batch(args: BatchArgs) -> ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let report = rt.block_on(async { miyo_rpc::run_batch(&args).await });
    match args.output {
        OutputFormat::Human => print!("{}", miyo_rpc::format_batch_human(&report)),
        OutputFormat::Json => match miyo_rpc::format_batch_json(&report) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("Failed to serialize batch report to JSON: {}", e);
                return ExitCode::from(2);
            }
        },
    }
    ExitCode::from(report.summary.exit_code)
}
