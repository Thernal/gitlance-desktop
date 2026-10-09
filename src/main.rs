//! GitLance — review commits and force-pushed branch versions without an IDE. See README.md.

mod dock;
mod editor;
mod generated;
mod git;
mod highlight;
mod mr;
mod review;
mod search;
mod storage;
mod structural;
mod ui;
mod worddiff;

use std::path::PathBuf;

fn main() {
    // An explicit path opens just that repository; without one the window comes back as it was
    // left (the tabs of the last session), or offers to open one.
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| git::Repo::open(p).is_ok());
    ui::run(path);
}
