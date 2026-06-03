use std::time::Duration;

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

// rough buckets, good enough for a fuzzy "modified N ago" column
pub fn human_age(since: Duration) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    // average month/year so the buckets don't drift over long spans
    const MONTH: u64 = 2_629_746;
    const YEAR: u64 = 31_556_952;

    let s = since.as_secs();
    if s < MIN {
        return "just now".into();
    }
    let (n, unit) = if s < HOUR {
        (s / MIN, "minute")
    } else if s < DAY {
        (s / HOUR, "hour")
    } else if s < MONTH {
        (s / DAY, "day")
    } else if s < YEAR {
        (s / MONTH, "month")
    } else {
        (s / YEAR, "year")
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{plural} ago")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0 GiB");
        // ~1.2 GiB
        assert_eq!(human_size(1_288_490_189), "1.2 GiB");
    }

    #[test]
    fn ages() {
        assert_eq!(human_age(Duration::from_secs(5)), "just now");
        assert_eq!(human_age(Duration::from_secs(60)), "1 minute ago");
        assert_eq!(human_age(Duration::from_secs(180)), "3 minutes ago");
        assert_eq!(human_age(Duration::from_secs(3600)), "1 hour ago");
        assert_eq!(human_age(Duration::from_secs(86400)), "1 day ago");
        assert_eq!(human_age(Duration::from_secs(3 * 86400)), "3 days ago");
        assert_eq!(
            human_age(Duration::from_secs(3 * 2_629_746)),
            "3 months ago"
        );
        assert_eq!(
            human_age(Duration::from_secs(2 * 31_556_952)),
            "2 years ago"
        );
    }
}
