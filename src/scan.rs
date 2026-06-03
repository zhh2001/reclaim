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
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::NodeModules => "node_modules",
            Kind::Target => "target",
            Kind::Pycache => "__pycache__",
            Kind::Venv => "venv",
            Kind::PytestCache => "pytest_cache",
            Kind::MypyCache => "mypy_cache",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub rel: PathBuf,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub fn scan(root: &Path) -> Vec<Found> {
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
            out.push(Found {
                rel,
                kind,
                size: dir_size(path),
                modified: fs::metadata(path).ok().and_then(|m| m.modified().ok()),
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
        "node_modules" => parent_has(path, "package.json").then_some(Kind::NodeModules),
        "target" => parent_has(path, "Cargo.toml").then_some(Kind::Target),
        "__pycache__" => Some(Kind::Pycache),
        ".pytest_cache" => Some(Kind::PytestCache),
        ".mypy_cache" => Some(Kind::MypyCache),
        ".venv" | "venv" => path.join("pyvenv.cfg").is_file().then_some(Kind::Venv),
        _ => None,
    }
}

fn parent_has(path: &Path, sibling: &str) -> bool {
    path.parent()
        .map(|p| p.join(sibling).is_file())
        .unwrap_or(false)
}

// jwalk runs this in parallel, which is where the time actually goes on a fat
// node_modules. len() is the apparent file size, not on-disk block usage.
fn dir_size(path: &Path) -> u64 {
    jwalk::WalkDir::new(path)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
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

        let found = scan(root);
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

        let found = scan(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Target);
    }

    #[test]
    fn venv_needs_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join(".venv/pyvenv.cfg"), 20);
        touch(&root.join(".venv/lib/python3/site.py"), 80);
        // a plain dir named venv without the marker
        touch(&root.join("venv/notes.txt"), 50);

        let found = scan(root);
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

        let found = scan(root);
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

        let found = scan(root);
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

        let found = scan(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 300);
    }

    #[test]
    fn empty_dir_finds_nothing() {
        let dir = tempdir().unwrap();
        assert!(scan(dir.path()).is_empty());
    }

    #[test]
    fn missing_path_yields_empty() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("does-not-exist");
        assert!(scan(&gone).is_empty());
    }
}
