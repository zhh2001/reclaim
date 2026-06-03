use std::time::{Duration, SystemTime};

use crate::scan::{Found, Kind};

#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub min_size: Option<u64>,
    pub older_than: Option<Duration>,
    pub kinds: Option<Vec<Kind>>,
}

impl Filters {
    pub fn active(&self) -> bool {
        self.min_size.is_some() || self.older_than.is_some() || self.kinds.is_some()
    }

    // boundary: --min-size keeps size >= threshold; --older-than keeps items whose
    // newest mtime is at or before now-duration (i.e. age >= duration, inclusive).
    pub fn keep(&self, f: &Found, now: SystemTime) -> bool {
        if let Some(min) = self.min_size {
            if f.size < min {
                return false;
            }
        }
        if let Some(d) = self.older_than {
            let old_enough = match (now.checked_sub(d), f.modified) {
                (Some(cutoff), Some(m)) => m <= cutoff,
                // can't establish age -> drop, so --older-than never deletes something undated
                _ => false,
            };
            if !old_enough {
                return false;
            }
        }
        if let Some(kinds) = &self.kinds {
            if !kinds.contains(&f.kind) {
                return false;
            }
        }
        true
    }

    pub fn apply(&self, found: Vec<Found>, now: SystemTime) -> Vec<Found> {
        found.into_iter().filter(|f| self.keep(f, now)).collect()
    }
}

pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let last = s.chars().last().ok_or("empty size")?;
    let (num, mult) = if last.is_ascii_alphabetic() {
        let mult: u64 = match last.to_ascii_lowercase() {
            'k' => 1024,
            'm' => 1024u64.pow(2),
            'g' => 1024u64.pow(3),
            't' => 1024u64.pow(4),
            _ => return Err(format!("unknown size suffix '{last}' (use K, M, G, T)")),
        };
        (s[..s.len() - last.len_utf8()].trim(), mult)
    } else {
        (s, 1)
    };

    let value: f64 = num
        .parse()
        .map_err(|_| format!("invalid size '{s}' (e.g. 100, 500K, 1.5G)"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!("invalid size '{s}'"));
    }
    Ok((value * mult as f64) as u64)
}

pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let unit = s.chars().last().ok_or("empty duration")?;
    let per = match unit {
        'h' | 'H' => 3600u64,
        'd' | 'D' => 86_400,
        'w' | 'W' => 604_800,
        // m is intentionally rejected: minutes vs months is ambiguous
        _ => return Err(format!("unknown duration unit '{unit}' (use h, d, or w)")),
    };
    let num = &s[..s.len() - unit.len_utf8()];
    let n: u64 = num
        .parse()
        .map_err(|_| format!("invalid duration '{s}' (e.g. 30d, 2w, 12h)"))?;
    n.checked_mul(per)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("duration '{s}' is too large"))
}

pub fn parse_kinds(s: &str) -> Result<Vec<Kind>, String> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match Kind::from_label(p) {
            Some(k) => {
                if !out.contains(&k) {
                    out.push(k);
                }
            }
            None => {
                return Err(format!(
                    "unknown type '{p}' (valid: {})",
                    Kind::labels_joined()
                ))
            }
        }
    }
    if out.is_empty() {
        return Err(format!("no types given (valid: {})", Kind::labels_joined()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn found(kind: Kind, size: u64, modified: Option<SystemTime>) -> Found {
        Found {
            path: PathBuf::from("x"),
            rel: PathBuf::from("x"),
            kind,
            size,
            modified,
        }
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("100K").unwrap(), 100 * 1024);
        assert_eq!(parse_size("1.5M").unwrap(), (1.5 * 1024.0 * 1024.0) as u64);
        assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("1g").unwrap(), 1024 * 1024 * 1024); // case-insensitive
        assert!(parse_size("abc").is_err());
        assert!(parse_size("10X").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(
            parse_duration("12h").unwrap(),
            Duration::from_secs(12 * 3600)
        );
        assert_eq!(
            parse_duration("30d").unwrap(),
            Duration::from_secs(30 * 86400)
        );
        assert_eq!(
            parse_duration("2w").unwrap(),
            Duration::from_secs(2 * 604800)
        );
        assert!(parse_duration("5m").is_err()); // minutes/months ambiguous
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10").is_err()); // no unit
    }

    #[test]
    fn parses_kinds() {
        assert_eq!(parse_kinds("target").unwrap(), vec![Kind::Target]);
        assert_eq!(
            parse_kinds("node_modules,target").unwrap(),
            vec![Kind::NodeModules, Kind::Target]
        );
        // __pycache__ is the on-screen label, so it must be accepted verbatim
        assert_eq!(parse_kinds("__pycache__").unwrap(), vec![Kind::Pycache]);
        let err = parse_kinds("bogus").unwrap_err();
        assert!(err.contains("bogus") && err.contains("node_modules"));
    }

    #[test]
    fn min_size_keeps_at_the_threshold() {
        let now = SystemTime::UNIX_EPOCH;
        let f = Filters {
            min_size: Some(1000),
            ..Default::default()
        };
        assert!(f.keep(&found(Kind::Target, 1000, None), now)); // == threshold
        assert!(f.keep(&found(Kind::Target, 1001, None), now));
        assert!(!f.keep(&found(Kind::Target, 999, None), now));
    }

    #[test]
    fn older_than_keeps_only_old_with_inclusive_boundary() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let f = Filters {
            older_than: Some(Duration::from_secs(100)),
            ..Default::default()
        };
        let cutoff = now - Duration::from_secs(100);

        let older = found(Kind::Target, 1, Some(cutoff - Duration::from_secs(1)));
        let at_boundary = found(Kind::Target, 1, Some(cutoff));
        let newer = found(Kind::Target, 1, Some(cutoff + Duration::from_secs(1)));

        assert!(f.keep(&older, now));
        assert!(f.keep(&at_boundary, now)); // inclusive
        assert!(!f.keep(&newer, now));
        // undated entries are dropped under --older-than
        assert!(!f.keep(&found(Kind::Target, 1, None), now));
    }

    #[test]
    fn only_keeps_listed_kinds() {
        let now = SystemTime::UNIX_EPOCH;
        let f = Filters {
            kinds: Some(vec![Kind::Target, Kind::Venv]),
            ..Default::default()
        };
        assert!(f.keep(&found(Kind::Target, 1, None), now));
        assert!(f.keep(&found(Kind::Venv, 1, None), now));
        assert!(!f.keep(&found(Kind::NodeModules, 1, None), now));
    }

    #[test]
    fn combined_filters_are_anded() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let old = Some(now - Duration::from_secs(1000));
        let recent = Some(now - Duration::from_secs(1));
        let f = Filters {
            min_size: Some(500),
            older_than: Some(Duration::from_secs(100)),
            kinds: Some(vec![Kind::Target]),
        };

        // satisfies all three
        assert!(f.keep(&found(Kind::Target, 500, old), now));
        // wrong kind
        assert!(!f.keep(&found(Kind::Venv, 500, old), now));
        // too small
        assert!(!f.keep(&found(Kind::Target, 499, old), now));
        // too new
        assert!(!f.keep(&found(Kind::Target, 500, recent), now));
    }
}
