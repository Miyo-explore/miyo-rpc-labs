//! Entry point for the `miyo-rpc` binary.

use clap::Parser;
use miyo_rpc::{BatchArgs, CheckArgs, Cli, DiagnosticReport, OutputFormat};
use std::io::Read;
use std::process::ExitCode;
use std::time::Duration;

/// Maximum accepted size for `--params-file` (1 MiB).
///
/// The size is checked via `metadata` *before* the file is opened, so an
/// oversized file is rejected without buffering it into memory. The read is
/// additionally capped (`take`) as defense in depth against a file that grows
/// between the metadata check and the read.
const MAX_PARAMS_FILE_BYTES: u64 = 1024 * 1024;

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        miyo_rpc::Commands::Check(args) => run_check(args),
        miyo_rpc::Commands::Batch(args) => run_batch(args),
    }
}

/// Read a `--params-file` with an explicit size cap.
///
/// Rejects unreadable paths, non-regular files (directories, etc.), and files
/// larger than [`MAX_PARAMS_FILE_BYTES`] — the size is checked via `metadata`
/// before any content is read. A capped read (`take`) backs this up in case the
/// file grew after the metadata check.
fn read_params_file(path: &str) -> Result<String, String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot read params file '{path}': {e}"))?;
    if !meta.is_file() {
        return Err(format!("params file '{path}' is not a regular file"));
    }
    if meta.len() > MAX_PARAMS_FILE_BYTES {
        return Err(format!(
            "params file '{path}' is too large: {} bytes (max {MAX_PARAMS_FILE_BYTES} bytes)",
            meta.len()
        ));
    }
    let file =
        std::fs::File::open(path).map_err(|e| format!("cannot read params file '{path}': {e}"))?;
    let mut buf = String::new();
    let mut limited = file.take(MAX_PARAMS_FILE_BYTES + 1);
    limited
        .read_to_string(&mut buf)
        .map_err(|e| format!("cannot read params file '{path}': {e}"))?;
    if buf.len() as u64 > MAX_PARAMS_FILE_BYTES {
        return Err(format!(
            "params file '{path}' is too large (max {MAX_PARAMS_FILE_BYTES} bytes)"
        ));
    }
    Ok(buf)
}

/// Parse `--params` / `--params-file` into a JSON-RPC `params` value.
///
/// Accepts a JSON object, array, or null (null maps to `None`). Malformed JSON,
/// a scalar shape, an unreadable/oversized file, or an empty method name is a
/// usage error: prints a clear message and exits 2 **before** any network
/// request is made.
fn parse_params(args: &CheckArgs) -> Option<serde_json::Value> {
    let raw = if let Some(p) = &args.params {
        Some(p.clone())
    } else if let Some(f) = &args.params_file {
        match read_params_file(f) {
            Ok(s) => Some(s),
            Err(msg) => {
                eprintln!("error: {msg}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };

    let text = raw?;

    match miyo_rpc::validate_params(&text) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            std::process::exit(2);
        }
    }
}

fn run_check(args: CheckArgs) -> ExitCode {
    // Reject an empty/whitespace --method before doing any network work.
    if let Some(m) = &args.method {
        if m.trim().is_empty() {
            eprintln!("error: --method must not be empty");
            return ExitCode::from(2);
        }
    }

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

    // Validate params up-front (exit 2 on bad input) before any request.
    let params = parse_params(&args);
    let params_given = args.params.is_some() || args.params_file.is_some();

    // A single arbitrary-method probe does not run the built-in checks, so
    // network-identity / cross-endpoint options have nothing to act on.
    let arbitrary = args
        .method
        .as_deref()
        .map(|m| !miyo_rpc::is_builtin_method(m))
        .unwrap_or(false);
    if arbitrary {
        if args.expected_network.is_some() {
            eprintln!(
                "warning: --expected-network has no effect on a single arbitrary-method probe; not applied."
            );
        }
        if args.compare.is_some() {
            eprintln!(
                "warning: --compare has no effect on a single arbitrary-method probe; not applied."
            );
        }
    } else if params_given {
        eprintln!(
            "warning: --params/--params-file is ignored: --method does not name an arbitrary method (built-in checks take no params)."
        );
    }

    // We need a tokio runtime for async HTTP calls.
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let timeout = Duration::from_secs(args.timeout_secs);
    let runs = std::cmp::min(args.runs.max(1), 10) as usize;

    let report = if arbitrary {
        let method = args.method.as_deref().expect("arbitrary implies --method");
        rt.block_on(async {
            DiagnosticReport::run_arbitrary_probe(&args.url, timeout, method, params, runs).await
        })
    } else {
        let mut report = rt.block_on(async {
            DiagnosticReport::run_all(&args.url, timeout, args.method.as_deref(), runs).await
        });
        report.enforce_expected_network(args.expected_network.as_deref());

        if let Some(compare_url) = &args.compare {
            let compare_report = rt.block_on(async {
                DiagnosticReport::run_all(compare_url, timeout, None, runs).await
            });
            report.compare_with(&compare_report);
        }
        report
    };

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
