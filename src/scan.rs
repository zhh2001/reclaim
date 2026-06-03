use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub enum Anchor {
    // matches wherever the name appears (a tool-specific cache name)
    Anywhere,
    // matches when one of these files sits next to the directory
    Sibling(Vec<String>),
    // matches when one of these files sits inside the directory (venv's pyvenv.cfg)
    Child(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub label: String,
    pub dir: String,
    pub anchor: Anchor,
}

impl Rule {
    fn matches(&self, name: &str, path: &Path) -> bool {
        if self.dir != name {
            return false;
        }
        match &self.anchor {
            Anchor::Anywhere => true,
            Anchor::Sibling(files) => path
                .parent()
                .map(|p| files.iter().any(|f| p.join(f).is_file()))
                .unwrap_or(false),
            Anchor::Child(files) => files.iter().any(|f| path.join(f).is_file()),
        }
    }
}

pub fn builtin_rules() -> Vec<Rule> {
    let sibling = |label: &str, dir: &str, anchors: &[&str]| Rule {
        label: label.into(),
        dir: dir.into(),
        anchor: Anchor::Sibling(anchors.iter().map(|s| s.to_string()).collect()),
    };
    let anywhere = |label: &str, dir: &str| Rule {
        label: label.into(),
        dir: dir.into(),
        anchor: Anchor::Anywhere,
    };
    let child = |label: &str, dir: &str, files: &[&str]| Rule {
        label: label.into(),
        dir: dir.into(),
        anchor: Anchor::Child(files.iter().map(|s| s.to_string()).collect()),
    };

    vec![
        sibling("node_modules", "node_modules", &["package.json"]),
        sibling("target", "target", &["Cargo.toml", "pom.xml"]),
        anywhere("__pycache__", "__pycache__"),
        child("venv", ".venv", &["pyvenv.cfg"]),
        child("venv", "venv", &["pyvenv.cfg"]),
        anywhere("pytest_cache", ".pytest_cache"),
        anywhere("mypy_cache", ".mypy_cache"),
        anywhere("ruff_cache", ".ruff_cache"),
        sibling("next", ".next", &["package.json"]),
        sibling("nuxt", ".nuxt", &["package.json"]),
        sibling("turbo", ".turbo", &["package.json"]),
        sibling("svelte-kit", ".svelte-kit", &["package.json"]),
        sibling("parcel-cache", ".parcel-cache", &["package.json"]),
        sibling(
            "gradle",
            ".gradle",
            &[
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
            ],
        ),
        sibling("tox", ".tox", &["tox.ini"]),
    ]
}

// distinct labels in rule order; used for --only validation and --help
pub fn labels(rules: &[Rule]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in rules {
        if !out.contains(&r.label) {
            out.push(r.label.clone());
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Found {
    // full path as walked, used for deletion; rel is for display
    pub path: PathBuf,
    pub rel: PathBuf,
    pub kind: String,
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

pub fn scan(root: &Path, rules: &[Rule], mode: SizeMode) -> Vec<Found> {
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
        if let Some(label) = classify(path, rules) {
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            let (size, modified) = measure(path, mode);
            out.push(Found {
                path: path.to_path_buf(),
                rel,
                kind: label.to_string(),
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

fn classify<'a>(path: &Path, rules: &'a [Rule]) -> Option<&'a str> {
    let name = path.file_name()?.to_str()?;
    rules
        .iter()
        .find(|r| r.matches(name, path))
        .map(|r| r.label.as_str())
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

    fn run(root: &Path) -> Vec<Found> {
        scan(root, &builtin_rules(), SizeMode::Disk)
    }

    fn kinds(found: &[Found]) -> Vec<String> {
        let mut k: Vec<String> = found.iter().map(|f| f.kind.clone()).collect();
        k.sort();
        k
    }

    #[test]
    fn finds_node_modules_only_with_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app/package.json"), 10);
        touch(&root.join("app/node_modules/left-pad/index.js"), 100);
        // a node_modules with no package.json sibling: not ours
        touch(&root.join("random/node_modules/stuff.txt"), 100);

        let found = run(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "node_modules");
        assert_eq!(found[0].rel, PathBuf::from("app/node_modules"));
    }

    #[test]
    fn finds_rust_target_only_with_cargo_toml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("crate/Cargo.toml"), 10);
        touch(&root.join("crate/target/debug/bin"), 500);
        touch(&root.join("notrust/target/whatever"), 500);

        let found = run(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "target");
    }

    #[test]
    fn target_matches_maven_via_pom_xml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("svc/pom.xml"), 10);
        touch(&root.join("svc/target/classes/A.class"), 100);

        let found = run(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "target");
    }

    #[test]
    fn target_ignored_without_cargo_or_pom() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("plain/target/stuff"), 100);
        assert!(run(dir.path()).is_empty());
    }

    #[test]
    fn js_framework_caches_need_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app/package.json"), 10);
        for d in [".next", ".nuxt", ".turbo", ".svelte-kit", ".parcel-cache"] {
            touch(&root.join(format!("app/{d}/f")), 20);
        }

        assert_eq!(
            kinds(&run(root)),
            vec!["next", "nuxt", "parcel-cache", "svelte-kit", "turbo"]
        );
    }

    #[test]
    fn js_framework_caches_ignored_without_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for d in [".next", ".nuxt", ".turbo", ".svelte-kit", ".parcel-cache"] {
            touch(&root.join(format!("nope/{d}/f")), 20);
        }
        assert!(run(root).is_empty());
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

            let found = run(root);
            assert_eq!(found.len(), 1, "anchor {anchor}");
            assert_eq!(found[0].kind, "gradle");
        }
    }

    #[test]
    fn gradle_ignored_without_build_file() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("proj/.gradle/x"), 50);
        assert!(run(dir.path()).is_empty());
    }

    #[test]
    fn tox_needs_tox_ini() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("proj/tox.ini"), 10);
        touch(&dir.path().join("proj/.tox/py311/x"), 50);
        let hit = run(dir.path());
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].kind, "tox");

        let other = tempdir().unwrap();
        touch(&other.path().join("proj/.tox/py311/x"), 50);
        assert!(run(other.path()).is_empty());
    }

    #[test]
    fn ruff_cache_matches_anywhere() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("a/b/c/.ruff_cache/0.1.0/x"), 30);
        let found = run(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "ruff_cache");
    }

    #[test]
    fn new_type_does_not_descend() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app/package.json"), 10);
        // a __pycache__ buried inside .next must not show up on its own
        touch(&root.join("app/.next/cache/__pycache__/x.pyc"), 40);

        let found = run(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "next");
    }

    #[test]
    fn labels_cover_all_builtin_types() {
        let all = labels(&builtin_rules());
        for l in [
            "node_modules",
            "target",
            "__pycache__",
            "venv",
            "pytest_cache",
            "mypy_cache",
            "ruff_cache",
            "next",
            "nuxt",
            "turbo",
            "svelte-kit",
            "parcel-cache",
            "gradle",
            "tox",
        ] {
            assert!(all.iter().any(|x| x == l), "missing {l}");
        }
        // venv is one label even though .venv and venv are separate rules
        assert_eq!(all.iter().filter(|x| *x == "venv").count(), 1);
    }

    #[test]
    fn venv_needs_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join(".venv/pyvenv.cfg"), 20);
        touch(&root.join(".venv/lib/python3/site.py"), 80);
        // a plain dir named venv without the marker
        touch(&root.join("venv/notes.txt"), 50);

        let found = run(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "venv");
        assert_eq!(found[0].rel, PathBuf::from(".venv"));
    }

    #[test]
    fn cache_dirs_match_anywhere() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("pkg/__pycache__/mod.pyc"), 30);
        touch(&root.join("a/b/.pytest_cache/v/lastfailed"), 30);
        touch(&root.join(".mypy_cache/3.11/x.json"), 30);

        assert_eq!(
            kinds(&run(root)),
            vec!["__pycache__", "mypy_cache", "pytest_cache"]
        );
    }

    #[test]
    fn does_not_descend_into_match() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("p/package.json"), 10);
        touch(&root.join("p/node_modules/dep/__pycache__/x.pyc"), 40);

        let found = run(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "node_modules");
    }

    #[test]
    fn size_sums_all_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("p/package.json"), 10);
        touch(&root.join("p/node_modules/a.js"), 100);
        touch(&root.join("p/node_modules/sub/b.js"), 200);

        let found = scan(root, &builtin_rules(), SizeMode::Apparent);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 300);
    }

    #[test]
    fn root_itself_is_never_classified() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("__pycache__");
        touch(&root.join("m.pyc"), 30);
        assert!(run(&root).is_empty());
    }

    #[test]
    fn empty_dir_finds_nothing() {
        let dir = tempdir().unwrap();
        assert!(run(dir.path()).is_empty());
    }

    #[test]
    fn missing_path_yields_empty() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("does-not-exist");
        assert!(run(&gone).is_empty());
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
        let found = run(root);
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
        let disk = scan(root, &builtin_rules(), SizeMode::Disk);
        assert_eq!(disk[0].size, one_block); // a and b share the inode

        let apparent = scan(root, &builtin_rules(), SizeMode::Apparent);
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
        set_file_mtime(root.join("__pycache__"), old).unwrap();
        set_file_mtime(root.join("__pycache__/sub"), old).unwrap();
        set_file_mtime(&nested, new).unwrap();

        let found = run(root);
        let expect = fs::symlink_metadata(&nested).unwrap().modified().unwrap();
        assert_eq!(found[0].modified, Some(expect));
    }

    #[test]
    fn custom_rule_with_anchor_matches_only_with_anchor() {
        let rules = {
            let mut r = builtin_rules();
            r.push(Rule {
                label: "cocoapods".into(),
                dir: "Pods".into(),
                anchor: Anchor::Sibling(vec!["Podfile".into()]),
            });
            r
        };
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("ios/Podfile"), 10);
        touch(&root.join("ios/Pods/lib/a"), 100);
        // a Pods dir with no Podfile next to it: ignored
        touch(&root.join("other/Pods/b"), 100);

        let found = scan(root, &rules, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "cocoapods");
        assert_eq!(found[0].rel, PathBuf::from("ios/Pods"));
    }

    #[test]
    fn custom_anywhere_rule_matches_nested() {
        let rules = {
            let mut r = builtin_rules();
            r.push(Rule {
                label: "mytool-cache".into(),
                dir: ".mytool".into(),
                anchor: Anchor::Anywhere,
            });
            r
        };
        let dir = tempdir().unwrap();
        touch(&dir.path().join("a/b/c/.mytool/x"), 30);

        let found = scan(dir.path(), &rules, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "mytool-cache");
    }

    #[test]
    fn custom_match_does_not_descend() {
        let rules = {
            let mut r = builtin_rules();
            r.push(Rule {
                label: "mytool-cache".into(),
                dir: ".mytool".into(),
                anchor: Anchor::Anywhere,
            });
            r
        };
        let dir = tempdir().unwrap();
        let root = dir.path();
        // a builtin cache nested inside a custom match must not show separately
        touch(&root.join(".mytool/__pycache__/x.pyc"), 40);

        let found = scan(root, &rules, SizeMode::Disk);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "mytool-cache");
    }
}
