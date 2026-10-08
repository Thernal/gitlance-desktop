//! GitLance — review commits and force-pushed branch versions without an IDE. See README.md.

mod git;
mod highlight;
mod search;
mod storage;
mod structural;
mod ui;
mod worddiff;

use std::path::PathBuf;

fn main() {
    // An explicit path, else the directory it was started in when that is a repository; with
    // neither, the window opens the most recent repository or offers to open one.
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .filter(|p| git::Repo::open(p).is_ok());
    ui::run(path);
}
