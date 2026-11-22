pub fn fmt_secs(secs: f64) -> String {
    let total = if secs.is_finite() {
        secs.floor().max(0.0) as u64
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

pub fn fmt_ms(ms: u64) -> String {
    fmt_secs((ms / 1000) as f64)
}

pub fn hm(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub fn ago(at_ms: i64, now_ms: i64) -> String {
    let seconds = (now_ms - at_ms) as f64 / 1000.0;
    if seconds < 60.0 {
        "just now".into()
    } else if seconds < 3600.0 {
        format!("{} min ago", (seconds / 60.0).floor())
    } else if seconds < 86400.0 {
        format!("{} h ago", (seconds / 3600.0).floor())
    } else {
        format!("{} d ago", (seconds / 86400.0).floor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_time() {
        assert_eq!(fmt_secs(0.0), "0:00");
        assert_eq!(fmt_secs(65.9), "1:05");
        assert_eq!(fmt_secs(-3.0), "0:00");
        assert_eq!(fmt_secs(f64::NAN), "0:00");
        assert_eq!(fmt_ms(215_000), "3:35");
        assert_eq!(hm(59.0), "0m");
        assert_eq!(hm(3720.0), "1h 2m");
        assert_eq!(ago(0, 30_000), "just now");
        assert_eq!(ago(0, 5 * 60_000), "5 min ago");
        assert_eq!(ago(0, 3 * 3_600_000), "3 h ago");
        assert_eq!(ago(0, 2 * 86_400_000), "2 d ago");
    }
}
