//! File and folder icons (#95): IntelliJ Community's, in colour (`assets/icons/intellij`, Apache
//! 2.0, its NOTICE beside them), a file's by its whole name, then by its extension (the longest
//! first: `gradle.kts` before `kts`), a folder's by rules on its path. The user's own win, from
//! the app's `icons/` folder, read again when it changes as the themes folder is:
//! - `<name>.svg` (and `<name>_dark.svg` for dark themes): a file's whole name (`Dockerfile.svg`),
//!   an extension (`kt.svg`), or one of the built-in icons replaced (`folder.svg`, `kotlin.svg`);
//! - `files.txt`, `folders.txt`: a regular expression on the path from the repository's root, then
//!   the icon (a built-in name or one of the folder's `.svg` files), a line each; the first that
//!   matches wins, `#` starts a comment.

use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, RwLock};

macro_rules! icon {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../assets/icons/intellij/", $name, ".svg")).as_slice(),
            include_bytes!(concat!("../assets/icons/intellij/", $name, "_dark.svg")).as_slice(),
        )
    };
    // One drawn the same on light and dark themes.
    ($name:literal, both) => {
        (
            $name,
            include_bytes!(concat!("../assets/icons/intellij/", $name, ".svg")).as_slice(),
            include_bytes!(concat!("../assets/icons/intellij/", $name, ".svg")).as_slice(),
        )
    };
}

/// The built-in icons: (name, light, dark).
const BUILT_IN: &[(&str, &[u8], &[u8])] = &[
    icon!("anyType"),
    icon!("annotationFolder"),
    icon!("archive"),
    icon!("bazel"),
    icon!("binaryData"),
    icon!("c"),
    icon!("config"),
    icon!("cpp"),
    icon!("Csharp"),
    icon!("css"),
    icon!("csv"),
    icon!("docker"),
    icon!("editorConfig"),
    icon!("excludeRoot"),
    icon!("folder"),
    icon!("folderGithub"),
    icon!("font"),
    icon!("generatedTestRoot"),
    icon!("gitignore", both),
    icon!("gradle"),
    icon!("graphql"),
    icon!("groovy"),
    icon!("h"),
    icon!("homeFolder"),
    icon!("html"),
    icon!("http", both),
    icon!("ignored"),
    icon!("image"),
    icon!("java"),
    icon!("javaClass"),
    icon!("javaScript"),
    icon!("jenkins", both),
    icon!("json"),
    icon!("jsonSchema"),
    icon!("jupyter"),
    icon!("kotlin"),
    icon!("kotlinGradleScript"),
    icon!("kotlinScript"),
    icon!("libraryFolder"),
    icon!("logFolder"),
    icon!("manifest"),
    icon!("markdown"),
    icon!("microsoftWindows"),
    icon!("module"),
    icon!("patch"),
    icon!("perl"),
    icon!("properties"),
    icon!("python"),
    icon!("resourcesRoot"),
    icon!("rst"),
    icon!("shell"),
    icon!("sourceMap"),
    icon!("sourceRoot"),
    icon!("sql"),
    icon!("swiftLang"),
    icon!("terraform"),
    icon!("testResourcesRoot"),
    icon!("testRoot"),
    icon!("text"),
    icon!("toml"),
    icon!("typeScript"),
    icon!("unknown"),
    icon!("vue", both),
    icon!("webFolder"),
    icon!("xhtml"),
    icon!("xml"),
    icon!("xsd"),
    icon!("yaml"),
];

/// Files known by their whole name.
const BY_NAME: &[(&str, &str)] = &[
    ("Dockerfile", "docker"),
    ("Containerfile", "docker"),
    ("Jenkinsfile", "jenkins"),
    (".gitignore", "gitignore"),
    (".gitattributes", "gitignore"),
    (".gitmodules", "gitignore"),
    (".dockerignore", "ignored"),
    (".npmignore", "ignored"),
    (".editorconfig", "editorConfig"),
    ("BUILD", "bazel"),
    ("BUILD.bazel", "bazel"),
    ("WORKSPACE", "bazel"),
    ("MODULE.bazel", "bazel"),
    ("MANIFEST.MF", "manifest"),
    ("gradlew", "shell"),
    ("gradlew.bat", "microsoftWindows"),
    ("gradle.properties", "gradle"),
    ("CODEOWNERS", "text"),
    ("LICENSE", "text"),
];

/// Files known by their extension, lower case; a longer one is tried first.
const BY_EXTENSION: &[(&str, &str)] = &[
    ("gradle.kts", "kotlinGradleScript"),
    ("schema.json", "jsonSchema"),
    ("d.ts", "typeScript"),
    ("kt", "kotlin"),
    ("kts", "kotlinScript"),
    ("java", "java"),
    ("class", "javaClass"),
    ("groovy", "groovy"),
    ("gvy", "groovy"),
    ("gradle", "gradle"),
    ("py", "python"),
    ("pyi", "python"),
    ("ipynb", "jupyter"),
    ("js", "javaScript"),
    ("mjs", "javaScript"),
    ("cjs", "javaScript"),
    ("jsx", "javaScript"),
    ("ts", "typeScript"),
    ("tsx", "typeScript"),
    ("mts", "typeScript"),
    ("cts", "typeScript"),
    ("vue", "vue"),
    ("swift", "swiftLang"),
    ("c", "c"),
    ("h", "h"),
    ("hpp", "h"),
    ("hh", "h"),
    ("cpp", "cpp"),
    ("cc", "cpp"),
    ("cxx", "cpp"),
    ("cs", "Csharp"),
    ("pl", "perl"),
    ("pm", "perl"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("fish", "shell"),
    ("bat", "microsoftWindows"),
    ("cmd", "microsoftWindows"),
    ("ps1", "microsoftWindows"),
    ("sql", "sql"),
    ("graphql", "graphql"),
    ("gql", "graphql"),
    ("tf", "terraform"),
    ("tfvars", "terraform"),
    ("http", "http"),
    ("rest", "http"),
    ("json", "json"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("xml", "xml"),
    ("xsd", "xsd"),
    ("html", "html"),
    ("htm", "html"),
    ("xhtml", "xhtml"),
    ("css", "css"),
    ("scss", "css"),
    ("sass", "css"),
    ("less", "css"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("rst", "rst"),
    ("txt", "text"),
    ("log", "text"),
    ("csv", "csv"),
    ("tsv", "csv"),
    ("yml", "yaml"),
    ("yaml", "yaml"),
    ("toml", "toml"),
    ("properties", "properties"),
    ("ini", "config"),
    ("cfg", "config"),
    ("conf", "config"),
    ("env", "config"),
    ("pro", "config"),
    ("lock", "config"),
    ("bazel", "bazel"),
    ("bzl", "bazel"),
    ("patch", "patch"),
    ("diff", "patch"),
    ("map", "sourceMap"),
    ("png", "image"),
    ("jpg", "image"),
    ("jpeg", "image"),
    ("gif", "image"),
    ("webp", "image"),
    ("svg", "image"),
    ("ico", "image"),
    ("bmp", "image"),
    ("ttf", "font"),
    ("otf", "font"),
    ("woff", "font"),
    ("woff2", "font"),
    ("jar", "archive"),
    ("aar", "archive"),
    ("war", "archive"),
    ("zip", "archive"),
    ("tar", "archive"),
    ("gz", "archive"),
    ("tgz", "archive"),
    ("bz2", "archive"),
    ("xz", "archive"),
    ("7z", "archive"),
    ("rar", "archive"),
    ("apk", "archive"),
    ("bin", "binaryData"),
    ("so", "binaryData"),
    ("dylib", "binaryData"),
    ("dll", "binaryData"),
    ("exe", "binaryData"),
];

/// Folders known by their path, before the user's `folders.txt` falls back to them.
const FOLDERS: &[(&str, &str)] = &[(r"(^|/)\.github$", "folderGithub")];

/// The icon of a file with no other.
const FILE: &str = "anyType";
const FOLDER: &str = "folder";

/// Where an icon comes from: a built-in one (its place in `BUILT_IN`), or the user's file.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    BuiltIn(usize),
    User {
        light: PathBuf,
        dark: Option<PathBuf>,
    },
}

/// What the icons folder holds.
#[derive(Default)]
struct User {
    /// Each `.svg` by its name without the extension (and `_dark`): (light, dark).
    svgs: HashMap<String, (Option<PathBuf>, Option<PathBuf>)>,
    files: Vec<(Regex, String)>,
    folders: Vec<(Regex, String)>,
    /// Lines of the rule files that could not be read.
    problems: Vec<String>,
    stamp: u128,
}

static USER: LazyLock<RwLock<User>> = LazyLock::new(|| RwLock::new(read_user()));
/// The icon found for each (folder, path) asked about since the folder was last read.
static FOUND: LazyLock<Mutex<HashMap<(bool, String), Source>>> = LazyLock::new(Default::default);
/// Each icon read, for light (false) or dark (true).
type Images = HashMap<(Source, bool), Arc<gpui::Image>>;
static IMAGES: LazyLock<Mutex<Images>> = LazyLock::new(Default::default);

/// The folder of the user's icons.
pub fn folder() -> Option<PathBuf> {
    Some(crate::platform::data_dir()?.join("icons"))
}

/// Makes the icons folder, with the two rule files explained and empty, the first time.
pub fn start_folder(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    for (file, example) in [
        ("folders.txt", r"(^|/)src/test$ testRoot"),
        ("files.txt", r"(^|/)proguard-rules\.pro$ config"),
    ] {
        let path = dir.join(file);
        if !path.exists() {
            let _ = std::fs::write(
                path,
                format!(
                    "# A regular expression on the path from the repository's root, then the icon:\n\
                     # a built-in one (folder, testRoot, sourceRoot, resourcesRoot, kotlin, …) or an\n\
                     # .svg of this folder. The first line that matches wins.\n# {example}\n"
                ),
            );
        }
    }
}

/// What the rule files of the icons folder got wrong, a line each.
pub fn problems() -> Vec<String> {
    USER.read().map(|u| u.problems.clone()).unwrap_or_default()
}

/// How many icons and rules the icons folder gives.
pub fn counts() -> (usize, usize) {
    USER.read()
        .map(|u| (u.svgs.len(), u.files.len() + u.folders.len()))
        .unwrap_or_default()
}

/// Reads the icons folder again when it changed; true when it did.
pub fn reload_if_changed() -> bool {
    let stamp = folder_stamp();
    if USER.read().is_ok_and(|u| u.stamp == stamp) {
        return false;
    }
    if let Ok(mut user) = USER.write() {
        *user = read_user();
    }
    FOUND.lock().unwrap_or_else(|e| e.into_inner()).clear();
    IMAGES.lock().unwrap_or_else(|e| e.into_inner()).clear();
    true
}

/// The icon of the file at `path` (from the repository's root), `dark` for a dark theme.
pub fn file(path: &str, dark: bool) -> Option<Arc<gpui::Image>> {
    image(&find(false, path), dark)
}

/// The icon of the folder at `path`.
pub fn folder_icon(path: &str, dark: bool) -> Option<Arc<gpui::Image>> {
    image(&find(true, path), dark)
}

fn find(is_folder: bool, path: &str) -> Source {
    let key = (is_folder, path.to_owned());
    if let Some(found) = FOUND.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return found.clone();
    }
    let found = match USER.read() {
        Ok(user) => resolve(&user, is_folder, path),
        Err(_) => resolve(&User::default(), is_folder, path),
    };
    FOUND
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, found.clone());
    found
}

/// The icon for `path`: the user's rules, then their icons by whole name and by extension, then
/// the built-in ones; a name is the user's `.svg` of that name before the built-in one.
fn resolve(user: &User, is_folder: bool, path: &str) -> Source {
    let named = |name: &str| -> Option<Source> {
        let name = name.strip_suffix(".svg").unwrap_or(name);
        if let Some((light, dark)) = user.svgs.get(name) {
            return Some(Source::User {
                light: light.clone().or_else(|| dark.clone())?,
                dark: dark.clone(),
            });
        }
        BUILT_IN
            .iter()
            .position(|(n, _, _)| *n == name)
            .map(Source::BuiltIn)
    };
    let rules = if is_folder {
        &user.folders
    } else {
        &user.files
    };
    if let Some(found) = rules
        .iter()
        .find(|(re, _)| re.is_match(path))
        .and_then(|(_, icon)| named(icon))
    {
        return found;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if is_folder {
        return FOLDERS
            .iter()
            .find(|(re, _)| Regex::new(re).is_ok_and(|re| re.is_match(path)))
            .and_then(|(_, icon)| named(icon))
            .or_else(|| named(FOLDER))
            .unwrap_or(Source::BuiltIn(0));
    }
    let lower = name.to_lowercase();
    // `a.b.c`: `b.c`, then `c` (a leading dot is part of the name, not an extension).
    let extensions: Vec<&str> = lower
        .char_indices()
        .filter(|&(i, c)| c == '.' && i > 0)
        .map(|(i, _)| &lower[i + 1..])
        .collect();
    let mine = |stem: &str| {
        user.svgs.get(stem).and_then(|(light, dark)| {
            Some(Source::User {
                light: light.clone().or_else(|| dark.clone())?,
                dark: dark.clone(),
            })
        })
    };
    mine(name)
        .or_else(|| extensions.iter().find_map(|e| mine(e)))
        .or_else(|| {
            BY_NAME
                .iter()
                .find(|(n, _)| *n == name)
                .and_then(|(_, icon)| named(icon))
        })
        .or_else(|| {
            extensions.iter().find_map(|e| {
                BY_EXTENSION
                    .iter()
                    .find(|(x, _)| x == e)
                    .and_then(|(_, icon)| named(icon))
            })
        })
        .or_else(|| named(FILE))
        .unwrap_or(Source::BuiltIn(0))
}

fn image(source: &Source, dark: bool) -> Option<Arc<gpui::Image>> {
    let key = (source.clone(), dark);
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(image) = images.get(&key) {
        return Some(image.clone());
    }
    let bytes = match source {
        Source::BuiltIn(i) => {
            let (_, light, dark_svg) = BUILT_IN.get(*i)?;
            (if dark { dark_svg } else { light }).to_vec()
        }
        Source::User {
            light,
            dark: dark_file,
        } => {
            let file = dark_file.as_ref().filter(|_| dark).unwrap_or(light);
            std::fs::read(file).ok()?
        }
    };
    let image = Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Svg, bytes));
    images.insert(key, image.clone());
    Some(image)
}

/// When the icons folder last changed (its files' times, summed).
fn folder_stamp() -> u128 {
    let Some(Ok(entries)) = folder().map(std::fs::read_dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .fold(1, |a, b| a.wrapping_add(b))
}

fn read_user() -> User {
    let mut user = User {
        stamp: folder_stamp(),
        ..User::default()
    };
    // A test reads none of the user's icons.
    if cfg!(test) {
        return user;
    }
    let Some(dir) = folder() else {
        return user;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return user;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_none_or(|e| e != "svg") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let (stem, is_dark) = match stem.strip_suffix("_dark") {
            Some(s) => (s.to_owned(), true),
            None => (stem.to_owned(), false),
        };
        let entry = user.svgs.entry(stem).or_default();
        if is_dark {
            entry.1 = Some(path);
        } else {
            entry.0 = Some(path);
        }
    }
    for (file, is_folder) in [("files.txt", false), ("folders.txt", true)] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
        let (rules, problems) = parse_rules(&text, file);
        if is_folder {
            user.folders = rules;
        } else {
            user.files = rules;
        }
        user.problems.extend(problems);
    }
    user
}

/// A rule file's lines: a regular expression, then the icon after the last space. What could not
/// be read, by file and line.
fn parse_rules(text: &str, file: &str) -> (Vec<(Regex, String)>, Vec<String>) {
    let (mut rules, mut problems) = (Vec::new(), Vec::new());
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((pattern, icon)) = line.rsplit_once(char::is_whitespace) else {
            problems.push(format!(
                "{file}:{}: a pattern, a space, then the icon",
                n + 1
            ));
            continue;
        };
        match Regex::new(pattern.trim()) {
            Ok(re) => rules.push((re, icon.to_owned())),
            Err(e) => problems.push(format!("{file}:{}: {e}", n + 1)),
        }
    }
    (rules, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(source: &Source) -> &str {
        match source {
            Source::BuiltIn(i) => BUILT_IN[*i].0,
            Source::User { .. } => "user",
        }
    }

    #[test]
    fn a_file_is_known_by_its_name_then_its_longest_extension() {
        let none = User::default();
        let icon = |path: &str| name_of(&resolve(&none, false, path)).to_owned();
        assert_eq!(icon("app/src/main/LoginViewModel.kt"), "kotlin");
        assert_eq!(icon("app/build.gradle.kts"), "kotlinGradleScript");
        assert_eq!(icon("scripts/release.main.kts"), "kotlinScript");
        assert_eq!(icon("docker/Dockerfile"), "docker");
        assert_eq!(icon(".gitignore"), "gitignore");
        assert_eq!(icon("README.MD"), "markdown");
        assert_eq!(icon("Makefile.unknown-thing"), "anyType");
        assert_eq!(icon("noextension"), "anyType");
    }

    #[test]
    fn folders_follow_the_rules_then_the_built_in_ones() {
        let (folders, problems) = parse_rules(
            "# tests\n(^|/)src/test$ testRoot\n^app/src/main/res$  resourcesRoot\n([ broken\n",
            "folders.txt",
        );
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("folders.txt:4:"));
        let user = User {
            folders,
            ..User::default()
        };
        let icon = |path: &str| name_of(&resolve(&user, true, path)).to_owned();
        assert_eq!(icon("core/src/test"), "testRoot");
        assert_eq!(icon("app/src/main/res"), "resourcesRoot");
        assert_eq!(icon(".github"), "folderGithub");
        assert_eq!(icon("core"), "folder");
    }

    #[test]
    fn the_users_icons_win_by_name_and_by_extension() {
        let svg = |s: &str| (Some(PathBuf::from(format!("/icons/{s}.svg"))), None);
        let user = User {
            svgs: [("kt", svg("kt")), ("folder", svg("folder"))]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            ..User::default()
        };
        assert_eq!(name_of(&resolve(&user, false, "A.kt")), "user");
        assert_eq!(name_of(&resolve(&user, true, "core")), "user");
        assert_eq!(name_of(&resolve(&user, false, "A.java")), "java");
    }
}
