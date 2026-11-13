//! Reading chap-core's timestamps, and how long ago they were.

use crate::output;
use std::time::Duration;

/// How long ago a wire timestamp was, as a table cell.
pub(super) fn ago(now: u64, at: &str) -> Option<String> {
    let then = parse_rfc3339(at)?;
    Some(output::ago(Duration::from_secs(now.saturating_sub(then))))
}

/// Seconds since the Unix epoch for an RFC 3339 timestamp, as chap-core
/// writes `last_ping_at`.
///
/// Accepts `2026-09-22T09:04:30Z`, a fractional second, a numeric offset and
/// a space in place of the `T`. Anything else is `None`, which the table
/// shows as `-` rather than inventing an age.
pub fn parse_rfc3339(text: &str) -> Option<u64> {
    parse_rfc3339_precise(text).map(|s| s.floor() as u64)
}

/// [`parse_rfc3339`] with the fractional second, for job run times.
pub fn parse_rfc3339_precise(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', 't', ' '])?;
    let mut fields = date.split('-');
    let year: i64 = fields.next()?.parse().ok()?;
    let month: u32 = fields.next()?.parse().ok()?;
    let day: u32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (clock, offset) = split_offset(rest);
    let mut fields = clock.split(':');
    let hour: i64 = fields.next()?.trim().parse().ok()?;
    let minute: i64 = fields.next()?.parse().ok()?;
    let (second, fraction): (i64, f64) = match fields.next() {
        Some(text) => {
            let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
            let fraction = match fraction {
                "" => 0.0,
                digits => format!("0.{digits}").parse().ok()?,
            };
            (whole.parse().ok()?, fraction)
        }
        None => (0, 0.0),
    };
    if fields.next().is_some() {
        return None;
    }

    let seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    (seconds >= 0).then_some(seconds as f64 + fraction)
}

/// Split a time off its UTC offset, in seconds. A time with no offset at all
/// is read as UTC, which is what every chap-core timestamp is.
fn split_offset(rest: &str) -> (&str, i64) {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return (clock, 0);
    }
    // A clock holds no sign, so the last one can only start the offset.
    let Some(at) = rest.rfind(['+', '-']) else {
        return (rest, 0);
    };
    let (clock, offset) = rest.split_at(at);
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits: String = offset.chars().filter(char::is_ascii_digit).collect();
    let (hours, minutes) = match digits.len() {
        4 => (digits[..2].parse().unwrap_or(0), digits[2..].parse().ok()),
        2 => (digits.parse().unwrap_or(0), Some(0)),
        _ => (0, Some(0)),
    };
    (clock, sign * (hours * 3600 + minutes.unwrap_or(0) * 60))
}

/// Days since the Unix epoch for a civil date.
///
/// Howard Hinnant's `days_from_civil`, the inverse of the conversion
/// [`crate::backup::utc_parts`] uses, and exact for every date a deployment
/// will ever report.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since the Unix epoch, now.
pub(super) fn now() -> u64 {
    crate::backup::now()
}
