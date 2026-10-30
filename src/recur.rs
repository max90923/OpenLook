//! Recurring appointments (series): the pattern is stored in
//! `events.recurrence` as `freq;interval;weekday-mask;until`, e.g.
//! `weekly;1;5;2027-03-31` (Mon+Wed, mask bit 0 = Monday) – and expanded
//! into single occurrences for the visible range.

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{json, Value};

use crate::models::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recurrence {
    pub freq: Freq,
    pub interval: u32,
    /// Weekly only: bit 0 = Monday … bit 6 = Sunday.
    pub weekdays: u8,
    pub until: Option<NaiveDate>,
}

pub const WEEKDAY_NAMES: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

impl Recurrence {
    pub fn parse(s: &str) -> Option<Self> {
        let mut p = s.split(';');
        let freq = match p.next()? {
            "daily" => Freq::Daily,
            "weekly" => Freq::Weekly,
            "monthly" => Freq::Monthly,
            "yearly" => Freq::Yearly,
            _ => return None,
        };
        let interval = p.next().and_then(|x| x.parse().ok()).unwrap_or(1).max(1);
        let weekdays = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let until = p
            .next()
            .and_then(|x| NaiveDate::parse_from_str(x, "%Y-%m-%d").ok());
        Some(Recurrence {
            freq,
            interval,
            weekdays,
            until,
        })
    }

    pub fn format(&self) -> String {
        let f = match self.freq {
            Freq::Daily => "daily",
            Freq::Weekly => "weekly",
            Freq::Monthly => "monthly",
            Freq::Yearly => "yearly",
        };
        format!(
            "{f};{};{};{}",
            self.interval,
            self.weekdays,
            self.until
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        )
    }

    /// Human readable summary, e.g. "Every week on Monday, Wednesday".
    pub fn describe(&self) -> String {
        const DAYS: [&str; 7] = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        let every = |one: &str, many: &str| {
            if self.interval == 1 {
                one.to_string()
            } else {
                format!("Every {} {many}", self.interval)
            }
        };
        let mut s = match self.freq {
            Freq::Daily => every("Every day", "days"),
            Freq::Weekly => {
                let days: Vec<&str> = (0..7)
                    .filter(|i| self.weekdays & (1 << i) != 0)
                    .map(|i| DAYS[i])
                    .collect();
                format!("{} on {}", every("Every week", "weeks"), days.join(", "))
            }
            Freq::Monthly => every("Every month", "months"),
            Freq::Yearly => every("Every year", "years"),
        };
        if let Some(u) = self.until {
            s.push_str(&format!(" until {}", u.format("%d/%m/%Y")));
        }
        s
    }

    /// Microsoft Graph `recurrence` object for an event starting on `start`.
    pub fn to_graph(&self, start: NaiveDate) -> Value {
        let mut pattern = json!({ "interval": self.interval });
        match self.freq {
            Freq::Daily => pattern["type"] = json!("daily"),
            Freq::Weekly => {
                pattern["type"] = json!("weekly");
                pattern["daysOfWeek"] = json!((0..7)
                    .filter(|i| self.weekdays & (1 << i) != 0)
                    .map(|i| WEEKDAY_NAMES[i])
                    .collect::<Vec<_>>());
            }
            Freq::Monthly => {
                pattern["type"] = json!("absoluteMonthly");
                pattern["dayOfMonth"] = json!(start.day());
            }
            Freq::Yearly => {
                pattern["type"] = json!("absoluteYearly");
                pattern["dayOfMonth"] = json!(start.day());
                pattern["month"] = json!(start.month());
            }
        }
        let range = match self.until {
            Some(u) => {
                json!({ "type": "endDate", "startDate": start.format("%Y-%m-%d").to_string(), "endDate": u.format("%Y-%m-%d").to_string() })
            }
            None => json!({ "type": "noEnd", "startDate": start.format("%Y-%m-%d").to_string() }),
        };
        json!({ "pattern": pattern, "range": range })
    }
}

fn local(s: &str) -> Option<NaiveDateTime> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Local).naive_local())
}

fn utc(dt: NaiveDateTime) -> String {
    Local
        .from_local_datetime(&dt)
        .earliest()
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| chrono::Utc.from_utc_datetime(&dt))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn add_months(d: NaiveDate, months: i64) -> Option<NaiveDate> {
    let total = d.year() as i64 * 12 + d.month0() as i64 + months;
    NaiveDate::from_ymd_opt((total / 12) as i32, (total % 12) as u32 + 1, d.day())
}

/// Start dates of all occurrences of a series from `first` up to `to`.
pub fn occurrences(r: &Recurrence, first: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let end = r.until.map_or(to, |u| u.min(to));
    let mut out = vec![];
    let step = r.interval as i64;
    match r.freq {
        Freq::Daily => {
            let mut d = first;
            while d <= end && out.len() < 5000 {
                out.push(d);
                d += Duration::days(step);
            }
        }
        Freq::Weekly => {
            let mask = if r.weekdays == 0 {
                1 << first.weekday().num_days_from_monday()
            } else {
                r.weekdays
            };
            let monday = first - Duration::days(first.weekday().num_days_from_monday() as i64);
            let mut week = 0;
            loop {
                let base = monday + Duration::days(7 * step * week);
                if base > end || out.len() >= 5000 {
                    break;
                }
                for wd in 0..7 {
                    let d = base + Duration::days(wd);
                    if mask & (1 << wd) != 0 && d >= first && d <= end {
                        out.push(d);
                    }
                }
                week += 1;
            }
        }
        Freq::Monthly | Freq::Yearly => {
            let months = if r.freq == Freq::Monthly {
                step
            } else {
                12 * step
            };
            let mut k = 0;
            while let Some(base) = add_months(first.with_day(1).unwrap(), k * months) {
                if base > end || out.len() >= 5000 {
                    break;
                }
                // Months without that day (e.g. 31st) are skipped, like Outlook's absolute pattern.
                if let Some(d) = base.with_day(first.day()) {
                    if d >= first && d <= end {
                        out.push(d);
                    }
                }
                k += 1;
            }
        }
    }
    out
}

/// Expand series in `events` into the occurrences between `from` and `to` (UTC strings).
pub fn expand(events: Vec<Event>, from: &str, to: &str) -> Vec<Event> {
    let (Some(from_l), Some(to_l)) = (local(from), local(to)) else {
        return events;
    };
    let mut out = vec![];
    for e in events {
        let (Some(r), Some(s), Some(en)) = (
            Recurrence::parse(&e.recurrence),
            local(&e.start),
            local(&e.end),
        ) else {
            out.push(e);
            continue;
        };
        let dur = en - s;
        for d in occurrences(&r, s.date(), to_l.date()) {
            let start = d.and_time(s.time());
            let end = start + dur;
            if end >= from_l && start <= to_l {
                out.push(Event {
                    start: utc(start),
                    end: utc(end),
                    ..e.clone()
                });
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn roundtrip_and_describe() {
        let r = Recurrence {
            freq: Freq::Weekly,
            interval: 2,
            weekdays: 0b101,
            until: Some(d(2027, 3, 31)),
        };
        assert_eq!(Recurrence::parse(&r.format()), Some(r.clone()));
        assert_eq!(
            r.describe(),
            "Every 2 weeks on Monday, Wednesday until 31/03/2027"
        );
        assert!(Recurrence::parse("").is_none());
    }

    #[test]
    fn weekly_mon_wed() {
        // 2026-11-02 is a Monday.
        let r = Recurrence {
            freq: Freq::Weekly,
            interval: 1,
            weekdays: 0b101,
            until: None,
        };
        assert_eq!(
            occurrences(&r, d(2026, 11, 2), d(2026, 11, 11)),
            vec![
                d(2026, 11, 2),
                d(2026, 11, 4),
                d(2026, 11, 9),
                d(2026, 11, 11)
            ]
        );
    }

    #[test]
    fn monthly_skips_short_months_and_respects_until() {
        let r = Recurrence {
            freq: Freq::Monthly,
            interval: 1,
            weekdays: 0,
            until: Some(d(2027, 4, 1)),
        };
        assert_eq!(
            occurrences(&r, d(2027, 1, 31), d(2027, 12, 31)),
            vec![d(2027, 1, 31), d(2027, 3, 31)]
        );
    }

    #[test]
    fn daily_interval() {
        let r = Recurrence {
            freq: Freq::Daily,
            interval: 3,
            weekdays: 0,
            until: None,
        };
        assert_eq!(
            occurrences(&r, d(2026, 1, 1), d(2026, 1, 8)),
            vec![d(2026, 1, 1), d(2026, 1, 4), d(2026, 1, 7)]
        );
    }

    #[test]
    fn graph_pattern() {
        let r = Recurrence {
            freq: Freq::Weekly,
            interval: 1,
            weekdays: 0b10,
            until: None,
        };
        let g = r.to_graph(d(2026, 11, 3));
        assert_eq!(g["pattern"]["daysOfWeek"][0], "tuesday");
        assert_eq!(g["range"]["type"], "noEnd");
    }
}
