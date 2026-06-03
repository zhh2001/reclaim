use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    NodeModules,
    Target,
    Pycache,
    Venv,
    PytestCache,
    MypyCache,
    RuffCache,
    Next,
    Nuxt,
    Turbo,
    SvelteKit,
    ParcelCache,
    Gradle,
    Tox,
}

impl Kind {
    pub const ALL: [Kind; 14] = [
        Kind::NodeModules,
        Kind::Target,
        Kind::Pycache,
        Kind::Venv,
        Kind::PytestCache,
        Kind::MypyCache,
        Kind::RuffCache,
        Kind::Next,
        Kind::Nuxt,
        Kind::Turbo,
        Kind::SvelteKit,
        Kind::ParcelCache,
        Kind::Gradle,
        Kind::Tox,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::NodeModules => "node_modules",
            Kind::Target => "target",
            Kind::Pycache => "__pycache__",
            Kind::Venv => "venv",
            Kind::PytestCache => "pytest_cache",
            Kind::MypyCache => "mypy_cache",
            Kind::RuffCache => "ruff_cache",
            Kind::Next => "next",
            Kind::Nuxt => "nuxt",
            Kind::Turbo => "turbo",
            Kind::SvelteKit => "svelte-kit",
            Kind::ParcelCache => "parcel-cache",
            Kind::Gradle => "gradle",
            Kind::Tox => "tox",
        }
    }

    pub fn from_label(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.label() == s)
    }

    pub fn labels_joined() -> String {
        Kind::ALL
            .iter()
            .map(|k| k.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    // full path as walked, used for deletion; rel is for display
    pub path: PathBuf,
    pub rel: PathBuf,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMode {
    // on-disk block usage, deduped by inode, like `du`
    Disk,
    // logical file bytes, every link counted
    Apparent,
}

pub fn scan(root: &Path, mode: SizeMode) -> Vec<Found> {
    let mut out = Vec::new();
    // walkdir doesn't read .gitignore, so node_modules/target get visited; it also
    // doesn't follow symlinks by default, which keeps us from escaping the tree.
    let mut it = WalkDir::new(root).into_iter();
    while let Some(res) = it.next() {
        let entry = match res {
            Ok(e) => e,
            Err(_) => continue, // unreadable dir/file: skip rather than abort the scan
        };
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if let Some(kind) = classify(path) {
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            let (size, modified) = measure(path, mode);
            out.push(Found {
                path: path.to_path_buf(),
                rel,
                kind,
                size,
                modified,
            });
            // don't descend: a reclaimable dir is counted whole, and nested
            // matches inside it would only double-count
            it.skip_current_dir();
        }
    }
    out
}

fn classify(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_str()?;
    match name {
        // anchored: only a match when the right manifest sits next to the dir
        "node_modules" => parent_has(path, &["package.json"]).then_some(Kind::NodeModules),
        "target" => parent_has(path, &["Cargo.toml", "pom.xml"]).then_some(Kind::Target),
        ".next" => parent_has(path, &["package.json"]).then_some(Kind::Next),
        ".nuxt" => parent_has(path, &["package.json"]).then_some(Kind::Nuxt),
        ".turbo" => parent_has(path, &["package.json"]).then_some(Kind::Turbo),
        ".svelte-kit" => parent_has(path, &["package.json"]).then_some(Kind::SvelteKit),
        ".parcel-cache" => parent_has(path, &["package.json"]).then_some(Kind::ParcelCache),
        ".gradle" => parent_has(
            path,
            &[
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
            ],
        )
        .then_some(Kind::Gradle),
        ".tox" => parent_has(path, &["tox.ini"]).then_some(Kind::Tox),
        ".venv" | "venv" => path.join("pyvenv.cfg").is_file().then_some(Kind::Venv),
        // tool-specific cache names, unambiguous wherever they appear
        "__pycache__" => Some(Kind::Pycache),
        ".pytest_cache" => Some(Kind::PytestCache),
        ".mypy_cache" => Some(Kind::MypyCache),
        ".ruff_cache" => Some(Kind::RuffCache),
        _ => None,
    }
}

fn parent_has(path: &Path, anchors: &[&str]) -> bool {
    path.parent()
        .map(|p| anchors.iter().any(|a| p.join(a).is_file()))
        .unwrap_or(false)
}

// One jwalk pass per candidate gives us both the size and the newest mtime in
// the subtree. Symlinks are skipped so we don't follow them out of the tree or
// count a link target's blocks.
#[cfg(unix)]
fn measure(path: &Path, mode: SizeMode) -> (u64, Option<SystemTime>) {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;

    let mut total = 0u64;
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    // seed with the candidate dir itself so its own mtime counts
    let mut newest = fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok());

    for entry in jwalk::WalkDir::new(path).skip_hidden(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_symlink() {
            continue;
        }
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if let Ok(m) = md.modified() {
            newest = Some(newest.map_or(m, |cur| cur.max(m)));
        }
        if md.is_file() {
            match mode {
                SizeMode::Apparent => total += md.len(),
                SizeMode::Disk => {
                    // one inode once, so pnpm-style hardlink farms aren't overcounted
                    if seen.insert((md.dev(), md.ino())) {
                        total += md.blocks() * 512;
                    }
                }
            }
        }
    }
    (total, newest)
}

// No block/inode info off Unix, so fall back to apparent bytes regardless of mode.
#[cfg(not(unix))]
fn measure(path: &Path, _mode: SizeMode) -> (u64, Option<SystemTime>) {
    let mut total = 0u64;
    let mut newest = fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok());

    for entry in jwalk::WalkDir::new(path).skip_hidden(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_symlink() {
            continue;
        }
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if let Ok(m) = md.modified() {
            newest = Some(newest.map_or(m, |cur| cur.max(m)));
        }
        if md.is_file() {
            total += md.len();
        }
    }
    (total, newest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    fn touch(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(path).unwrap();
        f.write_all(&vec![b'x'; bytes]).unwrap();
    }

    fn kinds(found: &[Found]) -> Vec<Kind> {
        let mut k: Vec<Kind> = found.iter().map(|f| f.kind).collect();
        k.sort_by_key(|k| k.label());
        k
    }

    #[test]
    fn finds_node_modules_only_with_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // legit JS project
        touch(&root.join("app/package.json"), 10);
        touch(&root.join("app/node_modules/left-pad/index.js"), 100);
        // a node_modules with no package.json sibling: not ours
        touch(&root.join("random/node_modules/stuff.txt"), 100);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::NodeModules);
        assert_eq!(found[0].rel, PathBuf::from("app/node_modules"));
    }

    #[test]
    fn finds_rust_target_only_with_cargo_toml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("crate/Cargo.toml"), 10);
        touch(&root.join("crate/target/debug/bin"), 500);
        touch(&root.join("notrust/target/whatever"), 500);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Target);
    }

    #[test]
    fn target_matches_maven_via_pom_xml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("svc/pom.xml"), 10);
        touch(&root.join("svc/target/classes/A.class"), 100);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Target);
    }

    #[test]
    fn target_ignored_without_cargo_or_pom() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("plain/target/stuff"), 100);
        assert!(scan(dir.path(), SizeMode::Disk).is_empty());
    }

    #[test]
    fn js_framework_caches_need_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app/package.json"), 10);
        for d in [".next", ".nuxt", ".turbo", ".svelte-kit", ".parcel-cache"] {
            touch(&root.join(format!("app/{d}/f")), 20);
        }

        let found = scan(root, SizeMode::Disk);
        assert_eq!(
            kinds(&found),
            vec![
                Kind::Next,
                Kind::Nuxt,
                Kind::ParcelCache,
                Kind::SvelteKit,
                Kind::Turbo,
            ]
        );
    }

    #[test]
    fn js_framework_caches_ignored_without_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for d in [".next", ".nuxt", ".turbo", ".svelte-kit", ".parcel-cache"] {
            touch(&root.join(format!("nope/{d}/f")), 20);
        }
        assert!(scan(root, SizeMode::Disk).is_empty());
    }

    #[test]
    fn gradle_matches_any_build_file() {
        for anchor in [
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            touch(&root.join(format!("proj/{anchor}")), 10);
            touch(&root.join("proj/.gradle/x"), 50);

            let found = scan(root, SizeMode::Disk);
            assert_eq!(found.len(), 1, "anchor {anchor}");
            assert_eq!(found[0].kind, Kind::Gradle);
        }
    }

    #[test]
    fn gradle_ignored_without_build_file() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("proj/.gradle/x"), 50);
        assert!(scan(dir.path(), SizeMode::Disk).is_empty());
    }

    #[test]
    fn tox_needs_tox_ini() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("proj/tox.ini"), 10);
        touch(&dir.path().join("proj/.tox/py311/x"), 50);
        let hit = scan(dir.path(), SizeMode::Disk);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].kind, Kind::Tox);

        let other = tempdir().unwrap();
        touch(&other.path().join("proj/.tox/py311/x"), 50);
        assert!(scan(other.path(), SizeMode::Disk).is_empty());
    }

    #[test]
    fn ruff_cache_matches_anywhere() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("a/b/c/.ruff_cache/0.1.0/x"), 30);
        let found = scan(dir.path(), SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::RuffCache);
    }

    #[test]
    fn new_type_does_not_descend() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app/package.json"), 10);
        // a __pycache__ buried inside .next must not show up on its own
        touch(&root.join("app/.next/cache/__pycache__/x.pyc"), 40);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Next);
    }

    #[test]
    fn labels_cover_new_types() {
        let all = Kind::labels_joined();
        for l in [
            "ruff_cache",
            "next",
            "nuxt",
            "turbo",
            "svelte-kit",
            "parcel-cache",
            "gradle",
            "tox",
        ] {
            assert!(all.contains(l), "missing {l}");
        }
        assert_eq!(Kind::from_label("tox"), Some(Kind::Tox));
        assert_eq!(Kind::from_label("svelte-kit"), Some(Kind::SvelteKit));
    }

    #[test]
    fn venv_needs_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join(".venv/pyvenv.cfg"), 20);
        touch(&root.join(".venv/lib/python3/site.py"), 80);
        // a plain dir named venv without the marker
        touch(&root.join("venv/notes.txt"), 50);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Venv);
        assert_eq!(found[0].rel, PathBuf::from(".venv"));
    }

    #[test]
    fn cache_dirs_match_anywhere() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("pkg/__pycache__/mod.pyc"), 30);
        touch(&root.join("a/b/.pytest_cache/v/lastfailed"), 30);
        touch(&root.join(".mypy_cache/3.11/x.json"), 30);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(
            kinds(&found),
            vec![Kind::Pycache, Kind::MypyCache, Kind::PytestCache]
        );
    }

    #[test]
    fn does_not_descend_into_match() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("p/package.json"), 10);
        // a __pycache__ nested inside node_modules must not be reported separately
        touch(&root.join("p/node_modules/dep/__pycache__/x.pyc"), 40);

        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::NodeModules);
    }

    #[test]
    fn size_sums_all_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("p/package.json"), 10);
        touch(&root.join("p/node_modules/a.js"), 100);
        touch(&root.join("p/node_modules/sub/b.js"), 200);

        let found = scan(root, SizeMode::Apparent);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 300);
    }

    #[test]
    fn root_itself_is_never_classified() {
        // scanning a dir that *is* a match (e.g. point straight at a __pycache__)
        // must not report the root; deletion relies on this.
        let dir = tempdir().unwrap();
        let root = dir.path().join("__pycache__");
        touch(&root.join("m.pyc"), 30);
        assert!(scan(&root, SizeMode::Disk).is_empty());
    }

    #[test]
    fn empty_dir_finds_nothing() {
        let dir = tempdir().unwrap();
        assert!(scan(dir.path(), SizeMode::Disk).is_empty());
    }

    #[test]
    fn missing_path_yields_empty() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("does-not-exist");
        assert!(scan(&gone, SizeMode::Disk).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn small_file_rounds_up_to_a_block() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let f = root.join("__pycache__/x.pyc");
        touch(&f, 1);

        let expect = fs::symlink_metadata(&f).unwrap().blocks() * 512;
        let found = scan(root, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert!(expect > 1, "a 1-byte file should still use a whole block");
        assert_eq!(found[0].size, expect);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_counted_once_on_disk_but_each_when_apparent() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let a = root.join("__pycache__/a.bin");
        touch(&a, 5000);
        let b = root.join("__pycache__/b.bin");
        fs::hard_link(&a, &b).unwrap();

        let one_block = fs::symlink_metadata(&a).unwrap().blocks() * 512;
        let disk = scan(root, SizeMode::Disk);
        assert_eq!(disk[0].size, one_block); // a and b share the inode

        let apparent = scan(root, SizeMode::Apparent);
        assert_eq!(apparent[0].size, 10000); // both links counted
    }

    #[test]
    fn modified_is_newest_in_subtree() {
        use filetime::{set_file_mtime, FileTime};

        let dir = tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("__pycache__/sub/new.pyc");
        touch(&nested, 10);

        let old = FileTime::from_unix_time(1_000_000_000, 0); // 2001
        let new = FileTime::from_unix_time(1_700_000_000, 0); // 2023
                                                              // candidate dir and everything else made old; one nested file made newer
        set_file_mtime(root.join("__pycache__"), old).unwrap();
        set_file_mtime(root.join("__pycache__/sub"), old).unwrap();
        set_file_mtime(&nested, new).unwrap();

        let found = scan(root, SizeMode::Disk);
        let expect = fs::symlink_metadata(&nested).unwrap().modified().unwrap();
        assert_eq!(found[0].modified, Some(expect));
    }
}
