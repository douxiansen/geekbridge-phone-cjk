// SPDX-License-Identifier: GPL-3.0-or-later
//! Turning times and durations into the short strings the lists and threads show. Pure: the
//! clock and the time zone are arguments, so the tests do not depend on where they run.

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, Offset, TimeZone, Timelike, Utc};

/// Unix seconds now.
pub fn unix_now() -> i64 {
    Utc::now().timestamp()
}

/// The local UTC offset in force at `ts`.
pub fn local_offset(ts: i64) -> FixedOffset {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| *d.offset())
        .unwrap_or_else(|| Utc.fix())
}

fn at(ts: i64, off: FixedOffset) -> Option<DateTime<FixedOffset>> {
    DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&off))
}

/// How far a time is from "now", in calendar days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayKind {
    Today,
    Yesterday,
    /// Within the last week: shown as a weekday.
    ThisWeek,
    SameYear,
    Older,
    /// A time from the future (clock skew): treated like today.
    Future,
}

fn day_kind(then: NaiveDate, now: NaiveDate) -> DayKind {
    let days = (now - then).num_days();
    match days {
        d if d < 0 => DayKind::Future,
        0 => DayKind::Today,
        1 => DayKind::Yesterday,
        2..=6 => DayKind::ThisWeek,
        _ if then.year() == now.year() => DayKind::SameYear,
        _ => DayKind::Older,
    }
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn weekday(d: &DateTime<FixedOffset>) -> &'static str {
    WEEKDAYS[d.weekday().num_days_from_monday() as usize % 7]
}

fn month(d: &DateTime<FixedOffset>) -> &'static str {
    MONTHS[d.month0() as usize % 12]
}

fn clock(d: &DateTime<FixedOffset>) -> String {
    format!("{:02}:{:02}", d.hour(), d.minute())
}

/// The time on the right of a list row: "14:05" today, "Yesterday", "Mon", "12 Oct", "12 Oct 2024".
pub fn list_time(ts: i64, now: i64, off: FixedOffset) -> String {
    let (Some(then), Some(now)) = (at(ts, off), at(now, off)) else {
        return String::new();
    };
    match day_kind(then.date_naive(), now.date_naive()) {
        DayKind::Today | DayKind::Future => clock(&then),
        DayKind::Yesterday => "Yesterday".into(),
        DayKind::ThisWeek => weekday(&then).into(),
        DayKind::SameYear => format!("{} {}", then.day(), month(&then)),
        DayKind::Older => format!("{} {} {}", then.day(), month(&then), then.year()),
    }
}

/// The centred stamp above a run of messages: "Today 14:05", "Yesterday 09:30", "Mon 09:30",
/// "12 Oct 09:30", "12 Oct 2024 09:30".
pub fn thread_stamp(ts: i64, now: i64, off: FixedOffset) -> String {
    let (Some(then), Some(now)) = (at(ts, off), at(now, off)) else {
        return String::new();
    };
    let time = clock(&then);
    match day_kind(then.date_naive(), now.date_naive()) {
        DayKind::Today | DayKind::Future => format!("Today {time}"),
        DayKind::Yesterday => format!("Yesterday {time}"),
        DayKind::ThisWeek => format!("{} {time}", weekday(&then)),
        DayKind::SameYear => format!("{} {} {time}", then.day(), month(&then)),
        DayKind::Older => format!("{} {} {} {time}", then.day(), month(&then), then.year()),
    }
}

/// A call length for the recents list: "0:42", "12:05", "1:02:03".
pub fn duration_short(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The running timer of a call: "00:07", "12:05", "1:02:03".
pub fn call_timer(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// "1 message", "3 messages".
pub fn count_noun(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off() -> FixedOffset {
        FixedOffset::east_opt(3600).unwrap()
    }
    /// Unix time of a local (UTC+1) date and time.
    fn t(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        off()
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .unwrap()
            .timestamp()
    }

    #[test]
    fn list_times_follow_the_calendar_not_24_hours() {
        let now = t(2026, 11, 8, 0, 30);
        assert_eq!(list_time(t(2026, 11, 8, 0, 5), now, off()), "00:05");
        // 25 minutes ago but on the previous calendar day.
        assert_eq!(list_time(t(2026, 11, 7, 23, 59), now, off()), "Yesterday");
        assert_eq!(list_time(t(2026, 11, 5, 12, 0), now, off()), "Thu");
        assert_eq!(list_time(t(2026, 11, 1, 12, 0), now, off()), "1 Nov");
        assert_eq!(list_time(t(2026, 3, 9, 12, 0), now, off()), "9 Mar");
        assert_eq!(list_time(t(2025, 12, 31, 12, 0), now, off()), "31 Dec 2025");
    }

    #[test]
    fn a_time_in_the_future_reads_as_today() {
        let now = t(2026, 11, 8, 12, 0);
        assert_eq!(list_time(t(2026, 11, 8, 12, 5), now, off()), "12:05");
        assert_eq!(
            thread_stamp(t(2026, 11, 9, 8, 0), now, off()),
            "Today 08:00"
        );
    }

    #[test]
    fn thread_stamps_carry_the_clock() {
        let now = t(2026, 11, 8, 15, 0);
        assert_eq!(
            thread_stamp(t(2026, 11, 8, 14, 5), now, off()),
            "Today 14:05"
        );
        assert_eq!(
            thread_stamp(t(2026, 11, 7, 9, 30), now, off()),
            "Yesterday 09:30"
        );
        assert_eq!(thread_stamp(t(2026, 11, 4, 9, 30), now, off()), "Wed 09:30");
        assert_eq!(
            thread_stamp(t(2026, 10, 12, 9, 30), now, off()),
            "12 Oct 09:30"
        );
        assert_eq!(
            thread_stamp(t(2024, 10, 12, 9, 30), now, off()),
            "12 Oct 2024 09:30"
        );
    }

    #[test]
    fn the_zone_decides_the_day() {
        // 23:30 UTC is already the next day at UTC+1.
        let ts = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 7, 23, 30, 0)
            .unwrap()
            .timestamp();
        let now = t(2026, 11, 8, 12, 0);
        assert_eq!(list_time(ts, now, off()), "00:30");
    }

    #[test]
    fn durations() {
        assert_eq!(duration_short(0), "0:00");
        assert_eq!(duration_short(42), "0:42");
        assert_eq!(duration_short(725), "12:05");
        assert_eq!(duration_short(3723), "1:02:03");
        assert_eq!(call_timer(7), "00:07");
        assert_eq!(call_timer(725), "12:05");
        assert_eq!(call_timer(3723), "1:02:03");
    }

    #[test]
    fn plurals() {
        assert_eq!(count_noun(1, "message", "messages"), "1 message");
        assert_eq!(count_noun(0, "message", "messages"), "0 messages");
    }

    #[test]
    fn local_offset_is_a_sane_zone() {
        let o = local_offset(1_700_000_000);
        assert!(o.local_minus_utc().abs() <= 14 * 3600);
    }
}
