use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use regex::Regex;
use std::str::FromStr;

/// `reset` e.g. "2:30am (Asia/Saigon)" or "4pm". `now_utc` is the current instant.
/// `default_tz` is used when the string has no "(Zone)". Returns the epoch (secs)
/// of the next occurrence of that wall-clock time, or None if unparseable.
pub fn parse_reset(reset: &str, now_utc: DateTime<Utc>, default_tz: Tz) -> Option<i64> {
    let tz = Regex::new(r"\(([^)]+)\)").unwrap()
        .captures(reset)
        .and_then(|c| Tz::from_str(c.get(1).unwrap().as_str()).ok())
        .unwrap_or(default_tz);
    let tm = Regex::new(r"(?i)(\d{1,2})(?::(\d{2}))?\s*(am|pm)").unwrap().captures(reset)?;
    let mut hour: u32 = tm.get(1).unwrap().as_str().parse().ok()?;
    hour %= 12;
    let minute: u32 = tm.get(2).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
    if tm.get(3).unwrap().as_str().eq_ignore_ascii_case("pm") {
        hour += 12;
    }
    let now_local = now_utc.with_timezone(&tz);
    let mut cand = tz
        .with_ymd_and_hms(now_local.year(), now_local.month(), now_local.day(), hour, minute, 0)
        .single()?;
    if cand <= now_local {
        cand += chrono::Duration::days(1);
    }
    Some(cand.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Asia::Saigon as SG;

    fn now(h: u32, m: u32) -> DateTime<Utc> {
        SG.with_ymd_and_hms(2026, 5, 30, h, m, 0).single().unwrap().with_timezone(&Utc)
    }

    #[test]
    fn later_today() {
        let e = parse_reset("2:30am (Asia/Saigon)", now(1, 0), SG).unwrap();
        assert_eq!(SG.timestamp_opt(e, 0).single().unwrap(),
                   SG.with_ymd_and_hms(2026, 5, 30, 2, 30, 0).single().unwrap());
    }
    #[test]
    fn rolls_next_day() {
        let e = parse_reset("2:30am (Asia/Saigon)", now(3, 0), SG).unwrap();
        assert_eq!(SG.timestamp_opt(e, 0).single().unwrap(),
                   SG.with_ymd_and_hms(2026, 5, 31, 2, 30, 0).single().unwrap());
    }
    #[test]
    fn pm_hour() {
        let e = parse_reset("4pm (Asia/Saigon)", now(1, 0), SG).unwrap();
        assert_eq!(SG.timestamp_opt(e, 0).single().unwrap().hour(), 16);
    }
    #[test]
    fn noon_midnight() {
        assert_eq!(SG.timestamp_opt(parse_reset("12pm (Asia/Saigon)", now(1,0), SG).unwrap(),0).single().unwrap().hour(), 12);
        assert_eq!(SG.timestamp_opt(parse_reset("12am (Asia/Saigon)", now(1,0), SG).unwrap(),0).single().unwrap().hour(), 0);
    }
    #[test]
    fn no_tz_uses_default() {
        let e = parse_reset("5:50am", now(1, 0), SG).unwrap();
        assert_eq!(SG.timestamp_opt(e,0).single().unwrap().hour(), 5);
        assert_eq!(SG.timestamp_opt(e,0).single().unwrap().minute(), 50);
    }
    #[test]
    fn unparseable_is_none() {
        assert!(parse_reset("whenever soon", now(1, 0), SG).is_none());
    }
}
