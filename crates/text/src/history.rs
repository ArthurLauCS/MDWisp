//! Labels for the history panel: sizes, line deltas, relative timestamps.

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use mdwisp_core::i18n::{t, tf};

/// Byte size at a readable width — the exact count only matters when small.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    // Rounded by hand: `{:.1}` rounds ties to even, the TS `toFixed` rounds them up.
    if value < 10.0 {
        format!("{:.1} {}", (value * 10.0).round() / 10.0, units[unit])
    } else {
        format!("{} {}", value.round(), units[unit])
    }
}

/// Revision kind (`auto` / `manual` / `restore`) as shown in the list.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "manual" => t("手动"),
        "restore" => t("还原"),
        _ => t("自动"),
    }
}

/// An empty document is 0 lines, not the 1 a naive `split` would report.
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() { 0 } else { text.matches('\n').count() + 1 }
}

/// How much a revision changed, measured in lines: `(before, after, delta)`.
///
/// Deliberately a line count rather than a real diff: the list only needs a
/// magnitude, and diffing 200 snapshots against their predecessors would make
/// opening the panel quadratic in the document size.
pub fn line_delta(before: &str, after: &str) -> (usize, usize, i64) {
    let (a, b) = (count_lines(before), count_lines(after));
    (a, b, b as i64 - a as i64)
}

/// `+12 行` / `−3 行` / `行数不变`.
pub fn format_delta(delta: i64) -> String {
    if delta == 0 {
        return t("行数不变").to_string();
    }
    tf("{0}{1} 行", &[&if delta > 0 { '+' } else { '−' }, &delta.abs()])
}

fn local(ms: i64) -> DateTime<Local> {
    Local.timestamp_millis_opt(ms).unwrap()
}

fn clock(d: &DateTime<Local>) -> String {
    format!("{:02}:{:02}", d.hour(), d.minute())
}

/// Relative timestamps for the history list, from Unix milliseconds.
///
/// The recent past is relative because that is how people think about their own
/// edits a minute ago; anything older than a day is named by the calendar,
/// because "27 小时前" makes the reader do arithmetic that a date does not.
pub fn relative_time(at: i64, now: i64) -> String {
    let diff = now - at;
    if diff < 60_000 {
        return t("刚刚").to_string();
    }
    if diff < 3_600_000 {
        return tf("{0} 分钟前", &[&(diff / 60_000)]);
    }
    if diff < 86_400_000 {
        return tf("{0} 小时前", &[&(diff / 3_600_000)]);
    }
    let (d, today) = (local(at), local(now));
    match (today.date_naive() - d.date_naive()).num_days() {
        1 => tf("昨天 {0}", &[&clock(&d)]),
        2 => tf("前天 {0}", &[&clock(&d)]),
        days if days < 7 => {
            let weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
            format!("{} {}", t(weekdays[d.weekday().num_days_from_sunday() as usize]), clock(&d))
        }
        _ if d.year() == today.year() => tf("{0} 月 {1} 日 {2}", &[&d.month(), &d.day(), &clock(&d)]),
        _ => tf("{0} 年 {1} 月 {2} 日", &[&d.year(), &d.month(), &d.day()]),
    }
}

/// Full timestamp for the tooltip — the relative form is lossy.
pub fn absolute_time(at: i64) -> String {
    let d = local(at);
    format!("{}-{:02}-{:02} {}", d.year(), d.month(), d.day(), clock(&d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1280), "1.3 KB");
        assert_eq!(format_bytes(9_900), "9.7 KB");
        assert_eq!(format_bytes(50_000), "49 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(20 * 1024 * 1024), "20 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn lines() {
        assert_eq!([count_lines(""), count_lines("one"), count_lines("a\nb"), count_lines("a\n")], [0, 1, 2, 2]);
        assert_eq!(line_delta("a\nb", "a\nb\nc\nd"), (2, 4, 2));
        assert_eq!(line_delta("a\nb\nc", "a"), (3, 1, -2));
        assert_eq!(line_delta("a\nb", "x\ny"), (2, 2, 0));
        assert_eq!(line_delta("", "a\nb\nc"), (0, 3, 3));
        assert_eq!(format_delta(12), "+12 行");
        assert_eq!(format_delta(-3), "−3 行");
        assert_eq!(format_delta(0), "行数不变");
        assert_eq!([kind_label("auto"), kind_label("manual"), kind_label("restore")], ["自动", "手动", "还原"]);
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().timestamp_millis()
    }

    #[test]
    fn relative() {
        let now = at(2026, 5, 20, 12, 0);
        assert_eq!(relative_time(now, now), "刚刚");
        assert_eq!(relative_time(now - 59_000, now), "刚刚");
        assert_eq!(relative_time(now + 5_000, now), "刚刚");
        assert_eq!(relative_time(now - 60_000, now), "1 分钟前");
        assert_eq!(relative_time(now - 59 * 60_000, now), "59 分钟前");
        assert_eq!(relative_time(now - 3_600_000, now), "1 小时前");
        assert_eq!(relative_time(now - 23 * 3_600_000, now), "23 小时前");
        assert_eq!(relative_time(at(2026, 5, 19, 12, 0), now), "昨天 12:00");
        assert_eq!(relative_time(at(2026, 5, 18, 12, 0), now), "前天 12:00");
        // 2026-05-15 is a Friday.
        assert_eq!(relative_time(at(2026, 5, 15, 12, 0), now), "周五 12:00");
        assert_eq!(relative_time(at(2026, 5, 14, 12, 0), now), "周四 12:00");
        assert_eq!(relative_time(at(2026, 5, 8, 12, 0), now), "5 月 8 日 12:00");
        assert_eq!(relative_time(at(2025, 12, 31, 12, 0), now), "2025 年 12 月 31 日");
        assert_eq!(relative_time(at(2026, 5, 19, 9, 5), now), "昨天 09:05");
        assert_eq!(absolute_time(at(2026, 5, 8, 9, 5)), "2026-05-08 09:05");
    }
}
