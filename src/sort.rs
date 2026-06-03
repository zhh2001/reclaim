use std::cmp::Ordering;
use std::time::SystemTime;

use crate::scan::Found;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Size,
    Modified,
    Path,
}

// Path is always the tiebreaker, so equal sizes/mtimes still come out in a
// fixed order. --reverse flips the whole comparison, tiebreak included.
pub fn sort_found(items: &mut [Found], key: SortKey, reverse: bool) {
    items.sort_by(|a, b| {
        let primary = match key {
            SortKey::Size => b.size.cmp(&a.size),         // big first
            SortKey::Modified => mtime(a).cmp(&mtime(b)), // old first
            SortKey::Path => Ordering::Equal,
        };
        let ord = primary.then_with(|| a.rel.cmp(&b.rel));
        if reverse {
            ord.reverse()
        } else {
            ord
        }
    });
}

// undated entries sort as oldest, which surfaces them under the default mtime order
fn mtime(f: &Found) -> SystemTime {
    f.modified.unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn found(path: &str, size: u64, secs_ago_epoch: u64) -> Found {
        Found {
            path: PathBuf::from(path),
            rel: PathBuf::from(path),
            kind: "pycache".into(),
            size,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs_ago_epoch)),
        }
    }

    fn paths(items: &[Found]) -> Vec<String> {
        items.iter().map(|f| f.rel.display().to_string()).collect()
    }

    #[test]
    fn size_defaults_to_largest_first() {
        let mut v = vec![found("a", 100, 0), found("b", 300, 0), found("c", 200, 0)];
        sort_found(&mut v, SortKey::Size, false);
        assert_eq!(paths(&v), ["b", "c", "a"]);
    }

    #[test]
    fn modified_puts_oldest_first() {
        let mut v = vec![
            found("new", 1, 500),
            found("old", 1, 100),
            found("mid", 1, 300),
        ];
        sort_found(&mut v, SortKey::Modified, false);
        assert_eq!(paths(&v), ["old", "mid", "new"]);
    }

    #[test]
    fn path_sorts_ascending() {
        let mut v = vec![found("c", 1, 0), found("a", 1, 0), found("b", 1, 0)];
        sort_found(&mut v, SortKey::Path, false);
        assert_eq!(paths(&v), ["a", "b", "c"]);
    }

    #[test]
    fn ties_break_on_path_ascending() {
        // same size: order must be deterministic by path
        let mut v = vec![found("b", 100, 0), found("a", 100, 0), found("c", 100, 0)];
        sort_found(&mut v, SortKey::Size, false);
        assert_eq!(paths(&v), ["a", "b", "c"]);
    }

    #[test]
    fn reverse_flips_each_key() {
        let mut by_size = vec![found("a", 100, 0), found("b", 300, 0), found("c", 200, 0)];
        sort_found(&mut by_size, SortKey::Size, true);
        assert_eq!(paths(&by_size), ["a", "c", "b"]);

        let mut by_mtime = vec![
            found("new", 1, 500),
            found("old", 1, 100),
            found("mid", 1, 300),
        ];
        sort_found(&mut by_mtime, SortKey::Modified, true);
        assert_eq!(paths(&by_mtime), ["new", "mid", "old"]);

        let mut by_path = vec![found("a", 1, 0), found("b", 1, 0), found("c", 1, 0)];
        sort_found(&mut by_path, SortKey::Path, true);
        assert_eq!(paths(&by_path), ["c", "b", "a"]);
    }
}
