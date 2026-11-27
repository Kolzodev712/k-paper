//! Main system binary: entry point for the paper exchange.
//!
//! Runs the server (Phase 2). For Phase 1, use examples or tests.

use paper_exchange_server::run;

fn main() {
    if let Err(e) = run() {
        eprintln!("paper exchange failed: {e}");
        std::process::exit(1);
    }
}
