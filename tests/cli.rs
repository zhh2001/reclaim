use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn reclaim() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reclaim"))
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
    let mut child = reclaim()
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
    let out = reclaim()
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
    let out = reclaim()
        .arg("--min-size")
        .arg("abc")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn help_lists_every_type() {
    let out = reclaim().arg("--help").output().unwrap();
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for k in reclaim::scan::Kind::ALL {
        assert!(help.contains(k.label()), "help is missing {}", k.label());
    }
}

#[test]
fn unknown_only_type_lists_valid_values() {
    let dir = fixture();
    let out = reclaim()
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

    let out = reclaim()
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
