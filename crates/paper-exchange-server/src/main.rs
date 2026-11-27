//! Binary entry for the paper exchange server (Phase 2).
//! Delegates to library run() so root binary and this binary share the same logic.

fn main() {
    if let Err(e) = paper_exchange_server::run() {
        eprintln!("paper exchange failed: {e}");
        std::process::exit(1);
    }
}
