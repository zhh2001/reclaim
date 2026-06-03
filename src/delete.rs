use std::path::{Path, PathBuf};

use crate::scan::Found;

// Pulled behind a trait so tests can drive the whole flow without touching a
// real trash can.
pub trait Remover {
    fn remove(&self, path: &Path) -> Result<(), String>;
}

pub struct TrashRemover;

impl Remover for TrashRemover {
    fn remove(&self, path: &Path) -> Result<(), String> {
        trash::delete(path).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeleteOpts {
    pub dry_run: bool,
    pub yes: bool,
}

#[derive(Debug, Default)]
pub struct DeleteOutcome {
    pub aborted: bool,
    pub dry_run: bool,
    pub moved: usize,
    pub freed: u64,
    pub failures: Vec<(PathBuf, String)>,
}

// `confirm` is only consulted in the interactive case, so dry-run and --yes
// never read stdin (and tests can assert it's left untouched).
pub fn delete_targets<R, F>(
    targets: &[Found],
    opts: DeleteOpts,
    confirm: F,
    remover: &R,
) -> DeleteOutcome
where
    R: Remover,
    F: FnOnce() -> bool,
{
    // dry-run beats --yes: if both are set we still delete nothing
    if opts.dry_run {
        return DeleteOutcome {
            dry_run: true,
            ..Default::default()
        };
    }
    if !opts.yes && !confirm() {
        return DeleteOutcome {
            aborted: true,
            ..Default::default()
        };
    }

    let mut out = DeleteOutcome::default();
    for t in targets {
        match remover.remove(&t.path) {
            Ok(()) => {
                out.moved += 1;
                out.freed += t.size;
            }
            Err(e) => out.failures.push((t.path.clone(), e)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::Kind;
    use std::cell::RefCell;
    use std::time::SystemTime;

    struct FakeRemover {
        fail_on: Vec<PathBuf>,
        calls: RefCell<Vec<PathBuf>>,
    }

    impl FakeRemover {
        fn new() -> Self {
            FakeRemover {
                fail_on: Vec::new(),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn failing(paths: &[&str]) -> Self {
            FakeRemover {
                fail_on: paths.iter().map(PathBuf::from).collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<PathBuf> {
            self.calls.borrow().clone()
        }
    }

    impl Remover for FakeRemover {
        fn remove(&self, path: &Path) -> Result<(), String> {
            self.calls.borrow_mut().push(path.to_path_buf());
            if self.fail_on.iter().any(|p| p == path) {
                Err("denied".into())
            } else {
                Ok(())
            }
        }
    }

    fn found(path: &str, size: u64) -> Found {
        Found {
            path: PathBuf::from(path),
            rel: PathBuf::from(path),
            kind: Kind::Pycache,
            size,
            modified: Some(SystemTime::UNIX_EPOCH),
        }
    }

    fn opts(dry_run: bool, yes: bool) -> DeleteOpts {
        DeleteOpts { dry_run, yes }
    }

    #[test]
    fn confirm_yes_removes_exactly_the_targets() {
        let targets = vec![found("a/node_modules", 100), found("b/target", 200)];
        let r = FakeRemover::new();
        let out = delete_targets(&targets, opts(false, false), || true, &r);

        assert_eq!(out.moved, 2);
        assert_eq!(out.freed, 300);
        assert!(out.failures.is_empty());
        assert!(!out.aborted);
        assert_eq!(
            r.calls(),
            vec![PathBuf::from("a/node_modules"), PathBuf::from("b/target")]
        );
    }

    #[test]
    fn declining_aborts_without_touching_anything() {
        let targets = vec![found("a/node_modules", 100)];
        let r = FakeRemover::new();
        let out = delete_targets(&targets, opts(false, false), || false, &r);

        assert!(out.aborted);
        assert_eq!(out.moved, 0);
        assert!(r.calls().is_empty());
    }

    #[test]
    fn yes_skips_the_prompt() {
        let targets = vec![found("a/node_modules", 100)];
        let r = FakeRemover::new();
        let out = delete_targets(
            &targets,
            opts(false, true),
            || panic!("must not prompt"),
            &r,
        );

        assert_eq!(out.moved, 1);
        assert_eq!(r.calls().len(), 1);
    }

    #[test]
    fn dry_run_deletes_nothing() {
        let targets = vec![found("a/node_modules", 100)];
        let r = FakeRemover::new();
        let out = delete_targets(
            &targets,
            opts(true, false),
            || panic!("must not prompt"),
            &r,
        );

        assert!(out.dry_run);
        assert_eq!(out.moved, 0);
        assert!(r.calls().is_empty());
    }

    #[test]
    fn dry_run_wins_over_yes() {
        let targets = vec![found("a/node_modules", 100)];
        let r = FakeRemover::new();
        let out = delete_targets(&targets, opts(true, true), || panic!("must not prompt"), &r);

        assert!(out.dry_run);
        assert!(r.calls().is_empty());
    }

    #[test]
    fn filtered_set_is_exactly_what_gets_removed() {
        use crate::filter::Filters;

        let all = vec![
            Found {
                path: PathBuf::from("big1"),
                rel: PathBuf::from("big1"),
                kind: Kind::Target,
                size: 5000,
                modified: None,
            },
            Found {
                path: PathBuf::from("small"),
                rel: PathBuf::from("small"),
                kind: Kind::Pycache,
                size: 10,
                modified: None,
            },
            Found {
                path: PathBuf::from("big2"),
                rel: PathBuf::from("big2"),
                kind: Kind::NodeModules,
                size: 2000,
                modified: None,
            },
        ];
        let filters = Filters {
            min_size: Some(1000),
            ..Default::default()
        };
        let kept = filters.apply(all, SystemTime::UNIX_EPOCH);

        let r = FakeRemover::new();
        let out = delete_targets(&kept, opts(false, true), || true, &r);

        assert_eq!(out.moved, 2);
        // the remover sees the kept set and nothing else
        assert_eq!(
            r.calls(),
            vec![PathBuf::from("big1"), PathBuf::from("big2")]
        );
    }

    #[test]
    fn sort_then_limit_then_delete_removes_exactly_top_n() {
        use crate::sort::{sort_found, SortKey};

        let mut items = vec![
            Found {
                path: PathBuf::from("mid"),
                rel: PathBuf::from("mid"),
                kind: Kind::Target,
                size: 200,
                modified: None,
            },
            Found {
                path: PathBuf::from("big"),
                rel: PathBuf::from("big"),
                kind: Kind::Target,
                size: 500,
                modified: None,
            },
            Found {
                path: PathBuf::from("small"),
                rel: PathBuf::from("small"),
                kind: Kind::Target,
                size: 10,
                modified: None,
            },
        ];
        sort_found(&mut items, SortKey::Size, false);
        items.truncate(2); // the top 2 by size

        let r = FakeRemover::new();
        let out = delete_targets(&items, opts(false, true), || true, &r);

        assert_eq!(out.moved, 2);
        // exactly the two largest, nothing else
        assert_eq!(r.calls(), vec![PathBuf::from("big"), PathBuf::from("mid")]);
    }

    #[test]
    fn a_failure_does_not_stop_the_rest() {
        let targets = vec![found("a", 100), found("b", 200), found("c", 400)];
        let r = FakeRemover::failing(&["b"]);
        let out = delete_targets(&targets, opts(false, true), || true, &r);

        assert_eq!(out.moved, 2);
        assert_eq!(out.freed, 500); // a + c, not b
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].0, PathBuf::from("b"));
        // everything was still attempted
        assert_eq!(r.calls().len(), 3);
    }
}
