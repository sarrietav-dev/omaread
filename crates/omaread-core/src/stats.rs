//! Reading time: how long you read each day and in which book, streaks,
//! and the books you finished. The app logs a page's time as you turn it
//! (or as it's read aloud); idle stretches don't count.

use std::collections::HashMap;

use rusqlite::params;
use serde::Serialize;

use crate::db::{Db, Error};
use crate::library::Book;

/// A page that stays open longer than this wasn't being read.
pub const IDLE_SECONDS: f64 = 600.0;

impl Db {
    /// Add time and pages to a book's day (`YYYY-MM-DD`, local).
    pub fn log_reading(&self, day: &str, identity: &str, seconds: f64, pages: i64) -> Result<(), Error> {
        self.conn()
            .prepare_cached(
                "INSERT INTO reading_log (day, identity, seconds, pages) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(day, identity) DO UPDATE SET seconds = seconds + excluded.seconds, pages = pages + excluded.pages",
            )?
            .execute(params![day, identity, seconds, pages])?;
        Ok(())
    }

    /// Note a book as finished, the first time only.
    pub fn mark_finished(&self, identity: &str, at: i64) -> Result<(), Error> {
        self.conn().prepare_cached("INSERT OR IGNORE INTO finished (identity, finished_at) VALUES (?1, ?2)")?.execute(params![identity, at])?;
        Ok(())
    }

    fn reading_log(&self) -> Result<Vec<(String, String, f64, i64)>, Error> {
        let mut stmt = self.conn().prepare_cached("SELECT day, identity, seconds, pages FROM reading_log")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()
    }

    fn finished_books(&self) -> Result<Vec<(String, i64)>, Error> {
        let mut stmt = self.conn().prepare_cached("SELECT identity, finished_at FROM finished ORDER BY finished_at DESC")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    }
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date for days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parse_day(day: &str) -> Option<i64> {
    let mut parts = day.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    Some(days_from_civil(y, m, d))
}

fn format_day(n: i64) -> String {
    let (y, m, d) = civil_from_days(n);
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Day {
    pub day: String,
    pub minutes: f64,
    pub pages: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BookTime {
    pub id: String,
    pub title: String,
    pub minutes: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finished {
    pub id: String,
    pub title: String,
    /// Unix seconds.
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub today: String,
    pub today_minutes: f64,
    /// The last seven days, today included.
    pub week_minutes: f64,
    pub total_minutes: f64,
    /// Days in a row with some reading, ending today (or yesterday, while
    /// today is still open).
    pub streak: i64,
    pub longest_streak: i64,
    /// The last `days` days, oldest first.
    pub days: Vec<Day>,
    /// The books read most in those days.
    pub books: Vec<BookTime>,
    pub finished_this_year: usize,
    /// The ten most recently finished.
    pub finished: Vec<Finished>,
}

/// A day counts toward a streak with at least this much reading.
const STREAK_MINUTES: f64 = 1.0;

/// What `omaread stats` and the stats card show. `today` is the local date.
pub fn report(db: &Db, today: &str, days: usize, books: &[Book]) -> Result<Report, Error> {
    let today_n = parse_day(today).unwrap_or(0);
    let titles: HashMap<&str, &str> = books.iter().map(|b| (b.identity.as_str(), b.title.as_str())).collect();
    let log = db.reading_log()?;

    let mut per_day: HashMap<i64, (f64, i64)> = HashMap::new();
    let mut per_book: HashMap<String, f64> = HashMap::new();
    let first = today_n - days.max(1) as i64 + 1;
    let mut total = 0.0;
    for (day, id, seconds, pages) in &log {
        let Some(n) = parse_day(day) else { continue };
        let entry = per_day.entry(n).or_default();
        entry.0 += seconds / 60.0;
        entry.1 += pages;
        total += seconds / 60.0;
        if n >= first && n <= today_n {
            *per_book.entry(id.clone()).or_default() += seconds / 60.0;
        }
    }
    let minutes_on = |n: i64| per_day.get(&n).map(|d| d.0).unwrap_or(0.0);

    let mut streak = 0;
    let mut n = if minutes_on(today_n) >= STREAK_MINUTES { today_n } else { today_n - 1 };
    while minutes_on(n) >= STREAK_MINUTES {
        streak += 1;
        n -= 1;
    }
    let mut read_days: Vec<i64> = per_day.iter().filter(|(_, d)| d.0 >= STREAK_MINUTES).map(|(n, _)| *n).collect();
    read_days.sort_unstable();
    let (mut longest, mut run) = (0, 0);
    for (i, n) in read_days.iter().enumerate() {
        run = if i > 0 && read_days[i - 1] == n - 1 { run + 1 } else { 1 };
        longest = longest.max(run);
    }

    let day_list: Vec<Day> = (first..=today_n)
        .map(|n| {
            let (minutes, pages) = per_day.get(&n).copied().unwrap_or_default();
            Day { day: format_day(n), minutes: round1(minutes), pages }
        })
        .collect();
    let mut book_list: Vec<BookTime> = per_book
        .into_iter()
        .map(|(id, minutes)| BookTime { title: titles.get(id.as_str()).map(|t| t.to_string()).unwrap_or_else(|| id.clone()), id, minutes: round1(minutes) })
        .filter(|b| b.minutes > 0.0)
        .collect();
    book_list.sort_by(|a, b| b.minutes.total_cmp(&a.minutes));
    book_list.truncate(10);

    let finished = db.finished_books()?;
    let year = civil_from_days(today_n).0;
    let finished_this_year = finished.iter().filter(|(_, at)| civil_from_days(at.div_euclid(86_400)).0 == year).count();
    let finished: Vec<Finished> = finished
        .into_iter()
        .take(10)
        .map(|(id, at)| Finished { title: titles.get(id.as_str()).map(|t| t.to_string()).unwrap_or_else(|| id.clone()), id, at })
        .collect();

    Ok(Report {
        today: today.to_string(),
        today_minutes: round1(minutes_on(today_n)),
        week_minutes: round1((today_n - 6..=today_n).map(minutes_on).sum()),
        total_minutes: round1(total),
        streak,
        longest_streak: longest,
        days: day_list,
        books: book_list,
        finished_this_year,
        finished,
    })
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// "1 h 05 min", "12 min".
pub fn minutes_text(m: f64) -> String {
    if m > 0.0 && m < 0.5 {
        return "<1 min".into();
    }
    let m = m.round() as i64;
    if m >= 60 { format!("{} h {:02} min", m / 60, m % 60) } else { format!("{m} min") }
}

/// A row of bars, one per day, scaled to the longest.
pub fn sparkline(days: &[Day]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = days.iter().map(|d| d.minutes).fold(0.0, f64::max);
    days.iter()
        .map(|d| if d.minutes <= 0.0 || max <= 0.0 { '·' } else { BARS[((d.minutes / max) * 7.0).round().clamp(0.0, 7.0) as usize] })
        .collect()
}

impl Report {
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "Today {}   this week {}   in all {}\nStreak {} day{} (longest {})\n\nLast {} days  {}\n",
            minutes_text(self.today_minutes),
            minutes_text(self.week_minutes),
            minutes_text(self.total_minutes),
            self.streak,
            if self.streak == 1 { "" } else { "s" },
            self.longest_streak,
            self.days.len(),
            sparkline(&self.days),
        );
        if !self.books.is_empty() {
            out.push_str("\nMost read\n");
            for b in &self.books {
                out.push_str(&format!("  {:>12}  {}\n", minutes_text(b.minutes), b.title));
            }
        }
        out.push_str(&format!("\nFinished this year: {}\n", self.finished_this_year));
        for f in &self.finished {
            let (y, m, d) = civil_from_days(f.at.div_euclid(86_400));
            out.push_str(&format!("  {y:04}-{m:02}-{d:02}  {}\n", f.title));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 26)), (2026, 11, 26));
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29) + 1), (2024, 3, 1));
        assert_eq!(parse_day("2026-11-26").map(format_day).as_deref(), Some("2026-11-26"));
        assert_eq!(parse_day("nonsense"), None);
    }

    #[test]
    fn time_adds_up_by_day_and_book_and_streaks_count_back_from_today() {
        let db = Db::in_memory().unwrap();
        for (day, secs) in [("2026-11-20", 600.0), ("2026-11-23", 300.0), ("2026-11-24", 120.0), ("2026-11-25", 900.0)] {
            db.log_reading(day, "a", secs, 3).unwrap();
        }
        db.log_reading("2026-11-25", "b", 300.0, 1).unwrap();
        db.log_reading("2026-11-25", "b", 300.0, 1).unwrap();
        db.mark_finished("b", days_from_civil(2026, 11, 25) * 86_400).unwrap();
        db.mark_finished("b", 0).unwrap();

        // Nothing yet today: the streak still runs through yesterday.
        let r = report(&db, "2026-11-26", 7, &[]).unwrap();
        assert_eq!(r.today_minutes, 0.0);
        assert_eq!(r.streak, 3);
        assert_eq!(r.longest_streak, 3);
        // Nov 20 to 26: 10 + 5 + 2 + 15 + 10 (book b).
        assert_eq!(r.week_minutes, 42.0);
        assert_eq!(r.days.len(), 7);
        assert_eq!(r.days.last().unwrap().day, "2026-11-26");
        assert_eq!(r.days[5], Day { day: "2026-11-25".into(), minutes: 25.0, pages: 5 });
        assert_eq!(r.books[0].id, "a");
        assert_eq!(r.books[0].minutes, 32.0);
        assert_eq!(r.finished_this_year, 1);
        assert_eq!(r.finished.len(), 1);

        // A gap of two days ends it.
        let r = report(&db, "2026-11-28", 7, &[]).unwrap();
        assert_eq!(r.streak, 0);
    }

    #[test]
    fn bars_scale_to_the_busiest_day() {
        let day = |m| Day { day: String::new(), minutes: m, pages: 0 };
        assert_eq!(sparkline(&[day(0.0), day(5.0), day(10.0)]), "·▅█");
        assert_eq!(minutes_text(65.0), "1 h 05 min");
        assert_eq!(minutes_text(12.4), "12 min");
        assert_eq!(minutes_text(0.1), "<1 min");
        assert_eq!(minutes_text(0.0), "0 min");
    }
}
