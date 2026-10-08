//! GitLance — review changes without an IDE. See README.md for the MVP.

fn main() {
    let target = std::env::args().nth(1).unwrap_or_else(|| "the working tree against HEAD".to_owned());
    println!("gitlance: reviewing {target} — not implemented yet");
}
