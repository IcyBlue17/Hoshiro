//! Word counting and usage statistics.

use crate::types::{HeatmapDay, Stats};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use std::collections::BTreeMap;

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F // Han
        | 0x3040..=0x30FF | 0x31F0..=0x31FF // kana
        | 0xAC00..=0xD7AF | 0x1100..=0x11FF // hangul
    )
}

/// Each CJK character counts as one word; runs of letters/digits count as one word each.
pub fn count_words(text: &str) -> u32 {
    let mut count = 0u32;
    let mut in_word = false;
    let mut prev_alnum = false;
    for c in text.chars() {
        if is_cjk(c) {
            count += 1;
            in_word = false;
            prev_alnum = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                count += 1;
                in_word = true;
            }
            prev_alnum = true;
        } else if (c == '\'' || c == '’') && prev_alnum && in_word {
            // contraction: keep the current word open
            prev_alnum = false;
        } else {
            in_word = false;
            prev_alnum = false;
        }
    }
    count
}

#[derive(Clone, Debug)]
pub struct StatRow {
    /// RFC 3339 timestamp.
    pub created_at: String,
    pub duration_secs: f64,
    pub words: u32,
}

/// (local date, duration, words)
type DayRow = (NaiveDate, f64, u32);

pub fn compute(rows: &[StatRow], today: NaiveDate) -> Stats {
    let day_rows: Vec<DayRow> = rows
        .iter()
        .filter_map(|r| {
            let dt = DateTime::parse_from_rfc3339(&r.created_at).ok()?;
            Some((dt.with_timezone(&Local).date_naive(), r.duration_secs, r.words))
        })
        .collect();
    compute_days(&day_rows, today)
}

pub fn compute_days(rows: &[DayRow], today: NaiveDate) -> Stats {
    let mut total_words = 0u64;
    let mut total_secs = 0f64;
    let mut per_day: BTreeMap<NaiveDate, (u64, u64)> = BTreeMap::new();
    for (date, secs, words) in rows {
        total_words += *words as u64;
        if secs.is_finite() && *secs > 0.0 {
            total_secs += *secs;
        }
        let e = per_day.entry(*date).or_default();
        e.0 += *words as u64;
        e.1 += 1;
    }

    let average_wpm = if total_secs > 0.0 { total_words as f64 / (total_secs / 60.0) } else { 0.0 };
    let typing_secs = total_words as f64 / 40.0 * 60.0;
    let time_saved = (typing_secs - total_secs).max(0.0);

    // Streaks
    let dates: Vec<NaiveDate> = per_day.keys().copied().collect();
    let mut longest = 0u32;
    let mut run = 0u32;
    let mut prev: Option<NaiveDate> = None;
    for d in &dates {
        run = match prev {
            Some(p) if *d - p == Duration::days(1) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        prev = Some(*d);
    }
    let mut current = 0u32;
    let mut cursor = if per_day.contains_key(&today) { Some(today) } else { today.pred_opt() };
    while let Some(d) = cursor {
        if per_day.contains_key(&d) {
            current += 1;
            cursor = d.pred_opt();
        } else {
            break;
        }
    }

    // 53 weeks, Sunday-based columns, oldest first, up to and including today.
    let week_start = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
    let start = week_start - Duration::weeks(52);
    let mut heatmap = Vec::with_capacity(53 * 7);
    let mut d = start;
    while d <= today {
        let (words, sessions) = per_day.get(&d).copied().unwrap_or((0, 0));
        heatmap.push(HeatmapDay { date: d.format("%Y-%m-%d").to_string(), words, sessions });
        d += Duration::days(1);
    }

    Stats {
        total_words,
        total_dictation_secs: total_secs,
        average_wpm,
        time_saved_secs: time_saved,
        active_days: per_day.len() as u32,
        current_streak: current,
        longest_streak: longest,
        heatmap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn counts_words() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("你好世界"), 4);
        assert_eq!(count_words("hello world"), 2);
        assert_eq!(count_words("用 Typeless 输入 3 次"), 1 + 1 + 2 + 1 + 1);
        assert_eq!(count_words("don't stop, it's 3.5"), 2 + 1 + 2);
        assert_eq!(count_words("こんにちは"), 5);
        assert_eq!(count_words("  ,.!? "), 0);
    }

    #[test]
    fn totals_and_wpm() {
        let today = d(2026, 10, 10);
        let rows = vec![(today, 30.0, 100u32), (today, 30.0, 20u32)];
        let s = compute_days(&rows, today);
        assert_eq!(s.total_words, 120);
        assert_eq!(s.total_dictation_secs, 60.0);
        assert!((s.average_wpm - 120.0).abs() < 1e-9);
        // typing 120 words at 40 wpm = 180 s, minus 60 s dictated
        assert!((s.time_saved_secs - 120.0).abs() < 1e-9);
        assert_eq!(s.active_days, 1);
    }

    #[test]
    fn empty_is_zero() {
        let s = compute_days(&[], d(2026, 10, 10));
        assert_eq!(s.total_words, 0);
        assert_eq!(s.average_wpm, 0.0);
        assert_eq!(s.time_saved_secs, 0.0);
        assert_eq!(s.current_streak, 0);
        assert_eq!(s.longest_streak, 0);
        assert!(!s.heatmap.is_empty());
    }

    #[test]
    fn time_saved_never_negative() {
        let today = d(2026, 10, 10);
        let s = compute_days(&[(today, 600.0, 10)], today);
        assert_eq!(s.time_saved_secs, 0.0);
    }

    #[test]
    fn streaks() {
        let today = d(2026, 10, 10);
        let rows = vec![
            (d(2026, 10, 10), 1.0, 1u32),
            (d(2026, 10, 9), 1.0, 1),
            (d(2026, 10, 8), 1.0, 1),
            (d(2026, 10, 5), 1.0, 1),
            (d(2026, 10, 4), 1.0, 1),
            (d(2026, 10, 3), 1.0, 1),
            (d(2026, 10, 2), 1.0, 1),
            (d(2026, 9, 30), 1.0, 1),
        ];
        let s = compute_days(&rows, today);
        assert_eq!(s.current_streak, 3);
        assert_eq!(s.longest_streak, 4);
        assert_eq!(s.active_days, 8);
    }

    #[test]
    fn current_streak_counts_from_yesterday() {
        let today = d(2026, 10, 10);
        let rows = vec![(d(2026, 10, 9), 1.0, 1u32), (d(2026, 10, 8), 1.0, 1)];
        assert_eq!(compute_days(&rows, today).current_streak, 2);
        let rows = vec![(d(2026, 10, 7), 1.0, 1u32)];
        assert_eq!(compute_days(&rows, today).current_streak, 0);
    }

    #[test]
    fn heatmap_shape() {
        let today = d(2026, 10, 10); // Saturday
        let s = compute_days(&[(today, 5.0, 7)], today);
        assert_eq!(s.heatmap.len(), 52 * 7 + 7);
        assert_eq!(s.heatmap.first().unwrap().date, "2025-10-05"); // a Sunday
        let last = s.heatmap.last().unwrap();
        assert_eq!(last.date, "2026-10-10");
        assert_eq!(last.words, 7);
        assert_eq!(last.sessions, 1);
    }
}
