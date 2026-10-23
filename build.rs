//! The commit a build is made from, for the version a report carries (#118): `GITLANCE_COMMIT`,
//! else `git rev-parse --short HEAD`, else "unknown".

fn main() {
    let commit = std::env::var("GITLANCE_COMMIT").ok().or_else(|| {
        std::process::Command::new("git")
            .args(["rev-parse", "--short=7", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    });
    println!(
        "cargo:rustc-env=GITLANCE_COMMIT={}",
        commit.unwrap_or_else(|| "unknown".to_owned())
    );
    println!("cargo:rerun-if-env-changed=GITLANCE_COMMIT");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
