use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn cruft() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cruft"))
}

// one __pycache__ with a ~2 KiB file
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("proj/__pycache__");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("m.pyc"), vec![b'x'; 2000]).unwrap();
    dir
}

#[test]
fn delete_abort_exits_zero() {
    let dir = fixture();
    let mut child = cruft()
        .arg("--delete")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Aborted"));
    assert!(dir.path().join("proj/__pycache__").exists());
}

#[test]
fn delete_filtered_to_empty_does_not_prompt() {
    let dir = fixture();
    let out = cruft()
        .arg("--delete")
        .arg("--min-size")
        .arg("999G")
        .arg(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("No matching directories."));
    assert!(!stdout.contains("to trash?"));
    assert!(dir.path().join("proj/__pycache__").exists());
}

#[test]
fn invalid_min_size_is_an_error() {
    let dir = fixture();
    let out = cruft()
        .arg("--min-size")
        .arg("abc")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn help_lists_every_type() {
    let out = cruft().arg("--help").output().unwrap();
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for label in cruft::scan::labels(&cruft::scan::builtin_rules()) {
        assert!(help.contains(&label), "help is missing {label}");
    }
}

#[test]
fn unknown_only_type_lists_valid_values() {
    let dir = fixture();
    let out = cruft()
        .arg("--only")
        .arg("bogus")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("node_modules"));
}

#[test]
fn min_size_filters_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("a/__pycache__");
    fs::create_dir_all(&big).unwrap();
    fs::write(big.join("x"), vec![b'x'; 200_000]).unwrap();
    let small = dir.path().join("b/.mypy_cache");
    fs::create_dir_all(&small).unwrap();
    fs::write(small.join("x"), vec![b'x'; 10]).unwrap();

    let out = cruft()
        .arg("--min-size")
        .arg("100K")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("__pycache__"));
    assert!(!stdout.contains("mypy_cache"));
}

// a big __pycache__ and a small .mypy_cache
fn two_caches() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("a/__pycache__");
    fs::create_dir_all(&big).unwrap();
    fs::write(big.join("x"), vec![b'x'; 200_000]).unwrap();
    let small = dir.path().join("b/.mypy_cache");
    fs::create_dir_all(&small).unwrap();
    fs::write(small.join("x"), vec![b'x'; 100]).unwrap();
    dir
}

#[test]
fn limit_caps_the_table() {
    let dir = two_caches();
    let out = cruft()
        .arg("--limit")
        .arg("1")
        .arg("--sort")
        .arg("size")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("__pycache__")); // the bigger one
    assert!(!stdout.contains("mypy_cache"));
}

#[test]
fn limit_zero_is_rejected() {
    let dir = fixture();
    let out = cruft()
        .arg("--limit")
        .arg("0")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn total_only_prints_only_the_total() {
    let dir = two_caches();
    let out = cruft()
        .arg("--total-only")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Reclaimable total:"));
    assert!(!stdout.contains("PATH"));
    assert!(!stdout.contains("__pycache__"));
}

#[test]
fn total_only_json_is_just_the_total() {
    let dir = two_caches();
    let out = cruft()
        .arg("--total-only")
        .arg("--json")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("total_bytes"));
    assert!(!stdout.contains("entries"));
}

#[test]
fn yes_and_interactive_conflict() {
    let dir = fixture();
    let out = cruft()
        .arg("--delete")
        .arg("-y")
        .arg("-i")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("mutually exclusive"));
    assert!(dir.path().join("proj/__pycache__").exists());
}

#[test]
fn interactive_without_delete_errors() {
    let dir = fixture();
    let out = cruft().arg("-i").arg(dir.path()).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn total_only_with_delete_errors() {
    let dir = fixture();
    let out = cruft()
        .arg("--total-only")
        .arg("--delete")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be combined with --delete"));
}

fn write_config(text: &str) -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn config_custom_rule_is_applied() {
    let (_cfg_dir, cfg) = write_config(
        r#"
        [[rules]]
        name = "cocoapods"
        dir = "Pods"
        anchors = ["Podfile"]
    "#,
    );
    let fix = tempfile::tempdir().unwrap();
    fs::create_dir_all(fix.path().join("ios/Pods/lib")).unwrap();
    fs::write(fix.path().join("ios/Podfile"), "x").unwrap();
    fs::write(fix.path().join("ios/Pods/lib/a"), vec![b'x'; 5000]).unwrap();
    // a Pods without Podfile must stay ignored
    fs::create_dir_all(fix.path().join("nope/Pods")).unwrap();
    fs::write(fix.path().join("nope/Pods/b"), vec![b'x'; 5000]).unwrap();

    let out = cruft()
        .arg("--config")
        .arg(&cfg)
        .arg(fix.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("cocoapods"));
    assert!(stdout.contains("ios/Pods"));
    assert!(!stdout.contains("nope/Pods"));
}

#[test]
fn config_custom_type_is_selectable_with_only() {
    let (_cfg_dir, cfg) = write_config(
        r#"
        [[rules]]
        name = "cocoapods"
        dir = "Pods"
        anchors = ["Podfile"]
    "#,
    );
    let fix = tempfile::tempdir().unwrap();
    fs::create_dir_all(fix.path().join("ios/Pods")).unwrap();
    fs::write(fix.path().join("ios/Podfile"), "x").unwrap();
    fs::write(fix.path().join("ios/Pods/a"), vec![b'x'; 5000]).unwrap();
    fs::create_dir_all(fix.path().join("py/__pycache__")).unwrap();
    fs::write(fix.path().join("py/__pycache__/a"), vec![b'x'; 5000]).unwrap();

    let out = cruft()
        .arg("--config")
        .arg(&cfg)
        .arg("--only")
        .arg("cocoapods")
        .arg(fix.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("cocoapods"));
    assert!(!stdout.contains("__pycache__"));
}

#[test]
fn missing_config_path_errors() {
    let fix = fixture();
    let out = cruft()
        .arg("--config")
        .arg("/no/such/config.toml")
        .arg(fix.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("config not found"));
}

#[test]
fn invalid_config_rule_errors() {
    let (_cfg_dir, cfg) = write_config(
        r#"
        [[rules]]
        name = "bad"
        dir = "Bad"
    "#,
    );
    let fix = fixture();
    let out = cruft()
        .arg("--config")
        .arg(&cfg)
        .arg(fix.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("anchors or anywhere"));
}

#[test]
fn total_only_ignores_limit() {
    let dir = two_caches();
    let with_limit = cruft()
        .arg("--total-only")
        .arg("--json")
        .arg("--limit")
        .arg("1")
        .arg(dir.path())
        .output()
        .unwrap();
    let without = cruft()
        .arg("--total-only")
        .arg("--json")
        .arg(dir.path())
        .output()
        .unwrap();

    // the total spans both caches regardless of --limit
    assert_eq!(with_limit.stdout, without.stdout);
}
