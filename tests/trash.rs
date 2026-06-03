// Exercises the real trash crate. It works on a Linux box with an XDG trash;
// on a headless/no-trash machine mark this #[ignore] instead.
use std::fs;

use reclaim::delete::{Remover, TrashRemover};

#[test]
fn trashing_removes_the_original_path() {
    let unique = format!("reclaim_trash_test_{}", std::process::id());
    let dir = std::env::temp_dir().join(&unique);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/file.txt"), b"data").unwrap();
    assert!(dir.exists());

    TrashRemover
        .remove(&dir)
        .expect("trash should work on this platform");
    assert!(!dir.exists(), "original path must be gone after trashing");

    cleanup(&unique);
}

// best-effort: purge what this test put in the trash so we don't grow it
#[cfg(target_os = "linux")]
fn cleanup(name: &str) {
    if let Ok(items) = trash::os_limited::list() {
        let mine: Vec<_> = items
            .into_iter()
            .filter(|i| format!("{:?}", i.name).contains(name))
            .collect();
        let _ = trash::os_limited::purge_all(mine);
    }
}

#[cfg(not(target_os = "linux"))]
fn cleanup(_name: &str) {}
