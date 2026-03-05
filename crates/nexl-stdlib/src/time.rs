//! `time` module — time and duration functions.
//!
//! Time is represented as Unix milliseconds (i64). Duration helpers are simple
//! multipliers. Date extraction uses `chrono` (UTC only).

use std::rc::Rc;

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `time` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("now", now as fn(&[Value]) -> Result<Value, String>),
        ("millis", millis),
        ("monotonic", monotonic),
        ("since", since),
        ("elapsed", elapsed),
        ("to-iso", to_iso),
        ("from-iso", from_iso),
        ("year", year),
        ("month", month),
        ("day", day),
        ("hour", hour),
        ("minute", minute),
        ("second", second),
        ("day-of-week", day_of_week),
        ("format", format_time),
        ("parse", parse_time),
        ("seconds", seconds),
        ("minutes", minutes),
        ("hours", hours),
        ("days", days),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn adt(type_name: &str, ctor: &str, fields: Vec<Value>) -> Value {
    Value::Adt {
        type_name: Rc::from(type_name),
        ctor: Rc::from(ctor),
        fields: Rc::new(fields),
    }
}

fn ok(v: Value) -> Value { adt("Result", "Ok", vec![v]) }
fn err_val(msg: &str) -> Value { adt("Result", "Err", vec![Value::Str(Rc::from(msg))]) }

fn expect_int(name: &str, v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("`time/{name}` expected Int, got {other}")),
    }
}

fn expect_str<'a>(name: &str, v: &'a Value) -> Result<&'a str, String> {
    match v {
        Value::Str(s) => Ok(s.as_ref()),
        other => Err(format!("`time/{name}` expected Str, got {other}")),
    }
}

fn unix_ms_to_datetime(ms: i64) -> DateTime<Utc> {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("epoch is valid"))
}

// ---------------------------------------------------------------------------
// Original functions
// ---------------------------------------------------------------------------

/// `(time/now)` — current time as Unix milliseconds (Int).
fn now(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Time)?;
    if !args.is_empty() {
        return Err(format!("`time/now` takes no arguments, got {}", args.len()));
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(Value::Int(ms))
}

/// `(time/monotonic)` — monotonic clock reading in nanoseconds (Int).
fn monotonic(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`time/monotonic` takes no arguments, got {}", args.len()));
    }
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    let ns = start.elapsed().as_nanos() as i64;
    Ok(Value::Int(ns))
}

/// `(time/millis duration-int)` — identity; documents that the Int is in milliseconds.
fn millis(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => Ok(Value::Int(*n)),
        [other] => Err(format!("`time/millis` expected Int, got {}", other.type_name())),
        _ => Err(format!("`time/millis` requires exactly 1 argument, got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// New duration/elapsed helpers
// ---------------------------------------------------------------------------

/// `(time/since timestamp-ms)` → `Int` — milliseconds since the given Unix timestamp.
fn since(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Time)?;
            let ts = expect_int("since", v)?;
            let current = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            Ok(Value::Int(current - ts))
        }
        _ => Err(format!("`time/since` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/elapsed monotonic-ns)` → `Int` — nanoseconds since the given monotonic reading.
fn elapsed(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ts = expect_int("elapsed", v)?;
            // Get current monotonic
            let current = match monotonic(&[]).unwrap() {
                Value::Int(n) => n,
                _ => 0,
            };
            Ok(Value::Int(current - ts))
        }
        _ => Err(format!("`time/elapsed` requires 1 argument (Int), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// ISO 8601 conversion
// ---------------------------------------------------------------------------

/// `(time/to-iso unix-ms)` → `Str` — format Unix ms as ISO 8601 UTC string.
fn to_iso(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("to-iso", v)?;
            let dt = unix_ms_to_datetime(ms);
            Ok(Value::Str(Rc::from(dt.to_rfc3339().as_str())))
        }
        _ => Err(format!("`time/to-iso` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/from-iso str)` → `(Result Int Str)` — parse ISO 8601 string to Unix ms.
fn from_iso(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str("from-iso", v)?;
            match DateTime::parse_from_rfc3339(s) {
                Ok(dt) => {
                    let ms = dt.timestamp_millis();
                    Ok(ok(Value::Int(ms)))
                }
                Err(e) => Ok(err_val(&e.to_string())),
            }
        }
        _ => Err(format!("`time/from-iso` requires 1 argument (Str), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Date part extraction
// ---------------------------------------------------------------------------

/// `(time/year unix-ms)` → `Int` — extract year (UTC).
fn year(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("year", v)?;
            Ok(Value::Int(unix_ms_to_datetime(ms).year() as i64))
        }
        _ => Err(format!("`time/year` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/month unix-ms)` → `Int` — extract month 1-12 (UTC).
fn month(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("month", v)?;
            Ok(Value::Int(unix_ms_to_datetime(ms).month() as i64))
        }
        _ => Err(format!("`time/month` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/day unix-ms)` → `Int` — extract day of month 1-31 (UTC).
fn day(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("day", v)?;
            Ok(Value::Int(unix_ms_to_datetime(ms).day() as i64))
        }
        _ => Err(format!("`time/day` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/hour unix-ms)` → `Int` — extract hour 0-23 (UTC).
fn hour(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("hour", v)?;
            Ok(Value::Int(unix_ms_to_datetime(ms).hour() as i64))
        }
        _ => Err(format!("`time/hour` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/minute unix-ms)` → `Int` — extract minute 0-59 (UTC).
fn minute(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("minute", v)?;
            Ok(Value::Int(unix_ms_to_datetime(ms).minute() as i64))
        }
        _ => Err(format!("`time/minute` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/second unix-ms)` → `Int` — extract second 0-59 (UTC).
fn second(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("second", v)?;
            Ok(Value::Int(unix_ms_to_datetime(ms).second() as i64))
        }
        _ => Err(format!("`time/second` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/day-of-week unix-ms)` → `Int` — day of week 0=Sun, 6=Sat (UTC).
fn day_of_week(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let ms = expect_int("day-of-week", v)?;
            use chrono::Weekday;
            let dow = unix_ms_to_datetime(ms).weekday();
            let n = match dow {
                Weekday::Sun => 0,
                Weekday::Mon => 1,
                Weekday::Tue => 2,
                Weekday::Wed => 3,
                Weekday::Thu => 4,
                Weekday::Fri => 5,
                Weekday::Sat => 6,
            };
            Ok(Value::Int(n))
        }
        _ => Err(format!("`time/day-of-week` requires 1 argument (Int), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Format / parse with pattern
// ---------------------------------------------------------------------------

/// `(time/format pattern unix-ms)` → `Str` — format with a strftime pattern.
fn format_time(args: &[Value]) -> Result<Value, String> {
    match args {
        [pattern_val, ms_val] => {
            let pattern = expect_str("format", pattern_val)?;
            let ms = expect_int("format", ms_val)?;
            let dt = unix_ms_to_datetime(ms);
            Ok(Value::Str(Rc::from(dt.format(pattern).to_string().as_str())))
        }
        _ => Err(format!("`time/format` requires 2 arguments (Str Int), got {}", args.len())),
    }
}

/// `(time/parse pattern str)` → `(Result Int Str)` — parse with a strftime pattern.
fn parse_time(args: &[Value]) -> Result<Value, String> {
    match args {
        [pattern_val, str_val] => {
            let pattern = expect_str("parse", pattern_val)?;
            let s = expect_str("parse", str_val)?;
            match DateTime::parse_from_str(s, pattern) {
                Ok(dt) => Ok(ok(Value::Int(dt.timestamp_millis()))),
                Err(_) => {
                    // Try NaiveDateTime (no timezone) interpreted as UTC
                    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, pattern) {
                        return Ok(ok(Value::Int(ndt.and_utc().timestamp_millis())));
                    }
                    // Try NaiveDate (date only) → midnight UTC
                    match chrono::NaiveDate::parse_from_str(s, pattern) {
                        Ok(nd) => {
                            let ms = nd.and_hms_opt(0, 0, 0)
                                .map(|ndt| ndt.and_utc().timestamp_millis())
                                .unwrap_or(0);
                            Ok(ok(Value::Int(ms)))
                        }
                        Err(e) => Ok(err_val(&e.to_string())),
                    }
                }
            }
        }
        _ => Err(format!("`time/parse` requires 2 arguments (Str Str), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Duration multipliers
// ---------------------------------------------------------------------------

/// `(time/seconds n)` → `Int` — n seconds in milliseconds.
fn seconds(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => Ok(Value::Int(expect_int("seconds", v)? * 1_000)),
        _ => Err(format!("`time/seconds` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/minutes n)` → `Int` — n minutes in milliseconds.
fn minutes(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => Ok(Value::Int(expect_int("minutes", v)? * 60_000)),
        _ => Err(format!("`time/minutes` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/hours n)` → `Int` — n hours in milliseconds.
fn hours(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => Ok(Value::Int(expect_int("hours", v)? * 3_600_000)),
        _ => Err(format!("`time/hours` requires 1 argument (Int), got {}", args.len())),
    }
}

/// `(time/days n)` → `Int` — n days in milliseconds.
fn days(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => Ok(Value::Int(expect_int("days", v)? * 86_400_000)),
        _ => Err(format!("`time/days` requires 1 argument (Int), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monotonic_returns_positive_int() {
        let result = monotonic(&[]).unwrap();
        match result {
            Value::Int(ns) => assert!(ns > 0, "monotonic() should return positive nanoseconds"),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn test_monotonic_is_nondecreasing() {
        let t1 = match monotonic(&[]).unwrap() { Value::Int(n) => n, _ => panic!() };
        let t2 = match monotonic(&[]).unwrap() { Value::Int(n) => n, _ => panic!() };
        assert!(t2 >= t1);
    }

    #[test]
    fn test_now_returns_int() {
        let result = now(&[]).unwrap();
        match result { Value::Int(ms) => assert!(ms > 0), _ => panic!("expected Int") }
    }

    #[test]
    fn test_millis_identity() {
        assert_eq!(millis(&[Value::Int(1000)]).unwrap(), Value::Int(1000));
    }

    #[test]
    fn test_seconds_multiplier() {
        assert_eq!(seconds(&[Value::Int(5)]).unwrap(), Value::Int(5_000));
    }

    #[test]
    fn test_minutes_multiplier() {
        assert_eq!(minutes(&[Value::Int(2)]).unwrap(), Value::Int(120_000));
    }

    #[test]
    fn test_hours_multiplier() {
        assert_eq!(hours(&[Value::Int(1)]).unwrap(), Value::Int(3_600_000));
    }

    #[test]
    fn test_days_multiplier() {
        assert_eq!(days(&[Value::Int(1)]).unwrap(), Value::Int(86_400_000));
    }

    #[test]
    fn test_to_iso_epoch() {
        let result = to_iso(&[Value::Int(0)]).unwrap();
        if let Value::Str(s) = result {
            assert!(s.contains("1970"));
        }
    }

    #[test]
    fn test_from_iso_roundtrip() {
        let ms = 1_700_000_000_000i64; // some timestamp in 2023
        let iso = to_iso(&[Value::Int(ms)]).unwrap();
        let back = from_iso(&[iso]).unwrap();
        if let Value::Adt { ctor, fields, .. } = back {
            assert_eq!(ctor.as_ref(), "Ok");
            assert_eq!(fields[0], Value::Int(ms));
        }
    }

    #[test]
    fn test_year_month_day() {
        // 2024-01-15 12:00:00 UTC
        let ms = 1_705_320_000_000i64;
        assert_eq!(year(&[Value::Int(ms)]).unwrap(), Value::Int(2024));
        assert_eq!(month(&[Value::Int(ms)]).unwrap(), Value::Int(1));
        assert_eq!(day(&[Value::Int(ms)]).unwrap(), Value::Int(15));
    }

    #[test]
    fn test_hour_minute_second() {
        // 2024-01-15 12:34:56 UTC  → 1705320896000 ms
        let ms = 1_705_320_000_000i64 + 34 * 60_000 + 56_000;
        // 12:34:56
        let h = hour(&[Value::Int(ms)]).unwrap();
        let m = minute(&[Value::Int(ms)]).unwrap();
        let s = second(&[Value::Int(ms)]).unwrap();
        assert_eq!(m, Value::Int(34));
        assert_eq!(s, Value::Int(56));
        // h = 12 UTC at 2024-01-15T12:00:00 → already UTC
        assert_eq!(h, Value::Int(12));
    }

    #[test]
    fn test_format_time() {
        let ms = 0i64; // epoch = 1970-01-01 00:00:00
        let result = format_time(&[Value::Str(Rc::from("%Y-%m-%d")), Value::Int(ms)]).unwrap();
        assert_eq!(result, Value::Str(Rc::from("1970-01-01")));
    }

    #[test]
    fn test_parse_time() {
        let result = parse_time(&[
            Value::Str(Rc::from("%Y-%m-%d")),
            Value::Str(Rc::from("1970-01-01")),
        ]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
    }
}
