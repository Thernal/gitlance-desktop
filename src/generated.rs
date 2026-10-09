//! Which files are generated or locked, so a diff can fold them: nobody reviews a 20 000-line
//! lock file by reading it. Names only; nothing is read from the repository.

/// Files whose whole name says they are written by a tool.
const NAMES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "Podfile.lock",
    "Gemfile.lock",
    "poetry.lock",
    "composer.lock",
    "go.sum",
    "pubspec.lock",
    "Package.resolved",
    "flake.lock",
    "mix.lock",
    "gradle.lockfile",
];

/// Endings that mark a generated or minified file.
const ENDINGS: &[&str] = &[
    ".lock",
    ".min.js",
    ".min.css",
    ".map",
    ".pb.go",
    "_pb2.py",
    ".g.dart",
    ".freezed.dart",
    ".designer.cs",
];

/// A file with fewer changed lines than this is shown even if it is generated.
pub const FOLD_ABOVE: usize = 40;

pub fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    NAMES.contains(&name)
        || ENDINGS.iter().any(|e| name.ends_with(e))
        || name.contains(".generated.")
        || path.contains("/generated/")
        || path.contains("/__generated__/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_and_generated_files_are_recognised() {
        for path in [
            "Cargo.lock",
            "web/package-lock.json",
            "app/build/generated/Foo.kt",
            "dist/app.min.js",
            "api/user.pb.go",
            "lib/model.g.dart",
            "src/schema.generated.ts",
        ] {
            assert!(is_generated(path), "{path}");
        }
    }

    #[test]
    fn ordinary_files_are_not() {
        for path in [
            "src/main.rs",
            "README.md",
            "lockfile_docs.md",
            "src/generator.rs",
        ] {
            assert!(!is_generated(path), "{path}");
        }
    }
}
