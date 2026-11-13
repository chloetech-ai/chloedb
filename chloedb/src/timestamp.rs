//! TIMESTAMP values: microseconds since 1970-01-01T00:00:00Z, written as
//! ISO 8601 text.
//!
//! [`parse`] reads a date (`2026-10-01`, midnight UTC) or a date and time
//! (`2026-10-01T12:30`, `2026-10-01 12:30:05.25`), with an optional offset
//! (`Z`, `+02:00`, `-0530`, `+02`); a time without one is UTC. Seconds take
//! up to nine decimals, of which the first six are kept. Years run from
//! 0000 to 9999, in the proleptic Gregorian calendar. [`format`] writes
//! UTC, `2026-10-01T12:30:05.250Z`, with the fraction only when it is not
//! zero: three digits when whole milliseconds, else six.

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Microseconds since the Unix epoch, or `None` for text that is not an
/// ISO 8601 date or date and time.
pub fn parse(text: &str) -> Option<i64> {
    let s = text.trim().as_bytes();
    let mut p = Cursor { s, at: 0 };
    let year = p.digits(4)?;
    p.byte(b'-')?;
    let month = p.digits(2)?;
    p.byte(b'-')?;
    let day = p.digits(2)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let mut micros = days_from_civil(year, month, day) * MICROS_PER_DAY;
    if p.done() {
        return Some(micros);
    }
    if !matches!(p.next()?, b'T' | b't' | b' ') {
        return None;
    }
    let hour = p.digits(2)?;
    p.byte(b':')?;
    let minute = p.digits(2)?;
    let mut second = 0;
    let mut fraction = 0;
    if p.peek() == Some(b':') {
        p.at += 1;
        second = p.digits(2)?;
        if matches!(p.peek(), Some(b'.' | b',')) {
            p.at += 1;
            let start = p.at;
            while p.peek().is_some_and(|c| c.is_ascii_digit()) {
                p.at += 1;
            }
            let digits = &s[start..p.at];
            if digits.is_empty() || digits.len() > 9 {
                return None;
            }
            for i in 0..6 {
                fraction = fraction * 10 + digits.get(i).map_or(0, |c| (c - b'0') as i64);
            }
        }
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    micros += ((hour * 60 + minute) * 60 + second) * 1_000_000 + fraction;
    let offset = match p.next() {
        None => 0,
        Some(b'Z' | b'z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            let h = p.digits(2)?;
            let m = match p.peek() {
                None => 0,
                Some(b':') => {
                    p.at += 1;
                    p.digits(2)?
                }
                Some(_) => p.digits(2)?,
            };
            if h > 23 || m > 59 {
                return None;
            }
            let o = (h * 60 + m) * 60_000_000;
            if sign == b'-' { -o } else { o }
        }
        Some(_) => return None,
    };
    if !p.done() {
        return None;
    }
    Some(micros - offset)
}

/// ISO 8601 text in UTC for microseconds since the Unix epoch. Years
/// outside 0000 to 9999 (which no parsed text reaches) are written with a
/// sign, as ISO 8601's expanded years are.
pub fn format(micros: i64) -> String {
    let days = micros.div_euclid(MICROS_PER_DAY);
    let mut rest = micros.rem_euclid(MICROS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let fraction = rest % 1_000_000;
    rest /= 1_000_000;
    let (h, min, s) = (rest / 3600, rest / 60 % 60, rest % 60);
    let year = if (0..=9999).contains(&y) { format!("{y:04}") } else { format!("{y:+05}") };
    let frac = match fraction {
        0 => String::new(),
        f if f % 1000 == 0 => format!(".{:03}", f / 1000),
        f => format!(".{f:06}"),
    };
    format!("{year}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}{frac}Z")
}

/// Microseconds since the Unix epoch now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as i64)
}

/// What `date_trunc` cuts a time to: the start of its second, minute,
/// hour, day, week (from Monday, as ISO 8601 and PostgreSQL start one),
/// month, quarter or year, in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl Unit {
    pub const ALL: [Unit; 8] = [Unit::Second, Unit::Minute, Unit::Hour, Unit::Day, Unit::Week, Unit::Month, Unit::Quarter, Unit::Year];

    /// The unit `name` names, in any case and singular or plural (`'day'`,
    /// `'DAYS'`).
    pub fn parse(name: &str) -> Option<Unit> {
        let n = name.trim().to_ascii_lowercase();
        let n = n.strip_suffix('s').unwrap_or(&n);
        Unit::ALL.into_iter().find(|u| u.name() == n)
    }

    pub fn name(self) -> &'static str {
        match self {
            Unit::Second => "second",
            Unit::Minute => "minute",
            Unit::Hour => "hour",
            Unit::Day => "day",
            Unit::Week => "week",
            Unit::Month => "month",
            Unit::Quarter => "quarter",
            Unit::Year => "year",
        }
    }
}

/// The start of the `unit` that `micros` is in (`date_trunc`), or `None`
/// for a time so near the ends of the integers that its start is not one.
pub fn trunc(micros: i64, unit: Unit) -> Option<i64> {
    let within = |n: i64| micros.checked_sub(micros.rem_euclid(n));
    let days = micros.div_euclid(MICROS_PER_DAY);
    let (y, m, _) = civil_from_days(days);
    let start = match unit {
        Unit::Second => return within(1_000_000),
        Unit::Minute => return within(60_000_000),
        Unit::Hour => return within(3_600_000_000),
        Unit::Day => days,
        Unit::Week => days - weekday(days),
        Unit::Month => days_from_civil(y, m, 1),
        Unit::Quarter => days_from_civil(y, (m - 1) / 3 * 3 + 1, 1),
        Unit::Year => days_from_civil(y, 1, 1),
    };
    start.checked_mul(MICROS_PER_DAY)
}

/// The units a [`bin`] width is written in, as [`parse_stride`] reads
/// them: microseconds each, largest first.
const STRIDE_UNITS: [(i64, &str); 7] = [
    (604_800_000_000, "weeks"),
    (86_400_000_000, "days"),
    (3_600_000_000, "hours"),
    (60_000_000, "minutes"),
    (1_000_000, "seconds"),
    (1_000, "milliseconds"),
    (1, "microseconds"),
];

/// The width of a `date_bin` bucket, in microseconds: a whole number and
/// a fixed unit, `'5 minutes'`, `'1 hour'`, `'30s'`, `'15m'`, `'250ms'`,
/// `'2 days'`, `'1 week'`; `None` for anything else, or a width that is
/// not positive. Months and years are not a fixed width: `date_trunc`
/// takes those.
pub fn parse_stride(text: &str) -> Option<i64> {
    let t = text.trim();
    let digits = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let n: i64 = t[..digits].parse().ok()?;
    let unit = match t[digits..].trim().to_ascii_lowercase().as_str() {
        "us" | "µs" | "usec" | "usecs" | "microsecond" | "microseconds" => 1,
        "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => 1_000,
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000_000,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000_000,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000_000,
        "d" | "day" | "days" => MICROS_PER_DAY,
        "w" | "week" | "weeks" => 7 * MICROS_PER_DAY,
        _ => return None,
    };
    n.checked_mul(unit).filter(|&w| w > 0)
}

/// A width in microseconds as [`parse_stride`] reads it back, in the
/// largest unit it is a whole number of: `'5 minutes'`, `'90 seconds'`.
pub fn stride_text(width: i64) -> String {
    let (u, name) = STRIDE_UNITS.into_iter().find(|(u, _)| width % u == 0).unwrap_or((1, "microseconds"));
    format!("{} {name}", width / u)
}

/// The start of the bucket of `width` microseconds that `micros` is in,
/// buckets counted from `origin` (`date_bin`, as PostgreSQL's): `origin +
/// ⌊(micros − origin) / width⌋ · width`. `None` when the start is outside
/// the integers.
pub fn bin(micros: i64, width: i64, origin: i64) -> Option<i64> {
    let d = (micros as i128) - (origin as i128);
    let start = (origin as i128) + d.div_euclid(width as i128) * width as i128;
    i64::try_from(start).ok()
}

/// A part of a time that `EXTRACT` and `date_part` read, in UTC, as
/// PostgreSQL names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Year,
    /// The ISO 8601 week-numbering year, which the week (`Week`) is of.
    IsoYear,
    /// 1 to 4.
    Quarter,
    /// 1 to 12.
    Month,
    /// The ISO 8601 week, 1 to 53: weeks start on Monday, and week 1 holds
    /// the year's first Thursday.
    Week,
    /// The day of the month, 1 to 31.
    Day,
    /// The day of the year, 1 to 366.
    Doy,
    /// The day of the week, Sunday 0 to Saturday 6.
    Dow,
    /// The day of the week, Monday 1 to Sunday 7.
    IsoDow,
    Hour,
    Minute,
    /// The seconds with their fraction (a FLOAT): 0 to 59.999999.
    Second,
    /// Seconds since 1970-01-01T00:00:00Z, with their fraction (a FLOAT).
    Epoch,
}

impl Part {
    pub const ALL: [Part; 13] = [
        Part::Year,
        Part::IsoYear,
        Part::Quarter,
        Part::Month,
        Part::Week,
        Part::Day,
        Part::Doy,
        Part::Dow,
        Part::IsoDow,
        Part::Hour,
        Part::Minute,
        Part::Second,
        Part::Epoch,
    ];

    /// The part `name` names, in any case and singular or plural
    /// (`'month'`, `'HOURS'`).
    pub fn parse(name: &str) -> Option<Part> {
        let n = name.trim().to_ascii_lowercase();
        let find = |n: &str| Part::ALL.into_iter().find(|p| p.name() == n);
        find(&n).or_else(|| find(n.strip_suffix('s')?))
    }

    pub fn name(self) -> &'static str {
        match self {
            Part::Year => "year",
            Part::IsoYear => "isoyear",
            Part::Quarter => "quarter",
            Part::Month => "month",
            Part::Week => "week",
            Part::Day => "day",
            Part::Doy => "doy",
            Part::Dow => "dow",
            Part::IsoDow => "isodow",
            Part::Hour => "hour",
            Part::Minute => "minute",
            Part::Second => "second",
            Part::Epoch => "epoch",
        }
    }

    /// Whether the part is a FLOAT (seconds with their fraction); the
    /// others are INT.
    pub fn is_float(self) -> bool {
        matches!(self, Part::Second | Part::Epoch)
    }
}

/// Part `part` of the time `micros`: an `i64`, or for [`Part::is_float`]
/// an `f64`'s bits as [`PartValue::Float`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartValue {
    Int(i64),
    Float(f64),
}

pub fn part(micros: i64, part: Part) -> PartValue {
    let days = micros.div_euclid(MICROS_PER_DAY);
    let rest = micros.rem_euclid(MICROS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    PartValue::Int(match part {
        Part::Year => y,
        Part::IsoYear => iso_week(days).0,
        Part::Quarter => (m - 1) / 3 + 1,
        Part::Month => m,
        Part::Week => iso_week(days).1,
        Part::Day => d,
        Part::Doy => days - days_from_civil(y, 1, 1) + 1,
        Part::Dow => (weekday(days) + 1) % 7,
        Part::IsoDow => weekday(days) + 1,
        Part::Hour => rest / 3_600_000_000,
        Part::Minute => rest / 60_000_000 % 60,
        Part::Second => return PartValue::Float((rest % 60_000_000) as f64 / 1e6),
        Part::Epoch => return PartValue::Float(micros as f64 / 1e6),
    })
}

/// The day of the week of `days` after 1970-01-01, a Thursday: Monday 0
/// to Sunday 6.
fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

/// The ISO 8601 year and week of the day `days` after 1970-01-01: its
/// week's Thursday's year, and that Thursday's week of it.
fn iso_week(days: i64) -> (i64, i64) {
    let thursday = days - weekday(days) + 3;
    let (y, _, _) = civil_from_days(thursday);
    (y, (thursday - days_from_civil(y, 1, 1)) / 7 + 1)
}

/// `strftime`'s directives, as SQLite's: what [`strftime`] writes.
pub const DIRECTIVES: &str = "%Y year, %m month 01-12, %d day 01-31, %H hour 00-23, %M minute, %S second, %f seconds with milliseconds (SS.SSS), %j day of the year 001-366, %w day of the week 0-6 from Sunday, %u day of the week 1-7 from Monday, %W week of the year 00-53 from the first Monday, %V ISO week 01-53, %G its year, %s seconds since 1970, %F %Y-%m-%d, %T %H:%M:%S, %% a %";

/// The time `micros` written by `format`'s `%` directives (SQLite's
/// `strftime`, in UTC), or what is wrong with `format`.
pub fn strftime(format: &str, micros: i64) -> Result<String, String> {
    let days = micros.div_euclid(MICROS_PER_DAY);
    let rest = micros.rem_euclid(MICROS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let (h, min, s) = (rest / 3_600_000_000, rest / 60_000_000 % 60, rest / 1_000_000 % 60);
    let year = |y: i64| if (0..=9999).contains(&y) { format!("{y:04}") } else { format!("{y:+05}") };
    let doy = days - days_from_civil(y, 1, 1);
    let mut out = String::with_capacity(format.len() + 8);
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        use std::fmt::Write;
        let _ = match chars.next() {
            Some('Y') => write!(out, "{}", year(y)),
            Some('m') => write!(out, "{m:02}"),
            Some('d') => write!(out, "{d:02}"),
            Some('H') => write!(out, "{h:02}"),
            Some('M') => write!(out, "{min:02}"),
            Some('S') => write!(out, "{s:02}"),
            Some('f') => write!(out, "{s:02}.{:03}", rest % 1_000_000 / 1000),
            Some('j') => write!(out, "{:03}", doy + 1),
            Some('w') => write!(out, "{}", (weekday(days) + 1) % 7),
            Some('u') => write!(out, "{}", weekday(days) + 1),
            Some('W') => write!(out, "{:02}", (doy + 7 - weekday(days)) / 7),
            Some('V') => write!(out, "{:02}", iso_week(days).1),
            Some('G') => write!(out, "{}", year(iso_week(days).0)),
            Some('s') => write!(out, "{}", micros.div_euclid(1_000_000)),
            Some('F') => write!(out, "{}-{m:02}-{d:02}", year(y)),
            Some('T') => write!(out, "{h:02}:{min:02}:{s:02}"),
            Some('%') => write!(out, "%"),
            Some(x) => return Err(format!("strftime has no %{x}: it writes {DIRECTIVES}")),
            None => return Err(format!("strftime's format ends in a lone %: write %% for a %; it writes {DIRECTIVES}")),
        };
    }
    Ok(out)
}

struct Cursor<'a> {
    s: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.at += 1;
        Some(c)
    }

    fn byte(&mut self, b: u8) -> Option<()> {
        (self.next()? == b).then_some(())
    }

    /// Exactly `n` ASCII digits.
    fn digits(&mut self, n: usize) -> Option<i64> {
        let ds = self.s.get(self.at..self.at + n)?;
        if !ds.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at += n;
        Some(ds.iter().fold(0, |v, c| v * 10 + (c - b'0') as i64))
    }

    fn done(&self) -> bool {
        self.at == self.s.len()
    }
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01 (`civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strides_read_and_bins_count_from_the_origin() {
        assert_eq!(parse_stride("5 minutes"), Some(300_000_000));
        assert_eq!(parse_stride(" 15m "), Some(900_000_000));
        assert_eq!(parse_stride("250ms"), Some(250_000));
        assert_eq!(parse_stride("1 HOUR"), Some(3_600_000_000));
        assert_eq!(parse_stride("2d"), Some(2 * MICROS_PER_DAY));
        assert_eq!(parse_stride("1 week"), Some(7 * MICROS_PER_DAY));
        for bad in ["0 minutes", "-5 minutes", "1 month", "5", "minutes", "1.5 hours", "99999999999999999 weeks"] {
            assert_eq!(parse_stride(bad), None, "{bad}");
        }
        for w in [1, 1_000, 300_000_000, 90_000_000, MICROS_PER_DAY, 7 * MICROS_PER_DAY, 123_456_789] {
            assert_eq!(parse_stride(&stride_text(w)), Some(w), "{}", stride_text(w));
        }
        assert_eq!(stride_text(300_000_000), "5 minutes");
        let t = parse("2026-11-13T02:07:31.5Z").unwrap();
        assert_eq!(bin(t, 300_000_000, 0), parse("2026-11-13T02:05:00Z"));
        // From an origin of :02:30, five-minute buckets start at :02:30 and :07:30.
        assert_eq!(bin(t, 300_000_000, parse("2000-01-01T00:02:30Z").unwrap()), parse("2026-11-13T02:07:30Z"));
        // Before 1970 and before the origin, the bucket below.
        assert_eq!(bin(-1, 60_000_000, 0), Some(-60_000_000));
        assert_eq!(bin(5, 10, 100), Some(0));
        assert_eq!(bin(i64::MIN, 60_000_000, 0), None);
        // A daily bin is date_trunc('day').
        for t in [0, -1, 1_234_567_890_123_456, -987_654_321_000] {
            assert_eq!(bin(t, MICROS_PER_DAY, 0), trunc(t, Unit::Day));
            assert_eq!(bin(t, 60_000_000, 0), trunc(t, Unit::Minute));
        }
    }

    const DAY: i64 = MICROS_PER_DAY;

    #[test]
    fn dates_and_times() {
        assert_eq!(parse("1970-01-01"), Some(0));
        assert_eq!(parse("1970-01-02"), Some(DAY));
        assert_eq!(parse("1969-12-31"), Some(-DAY));
        assert_eq!(parse("2000-03-01"), Some(11_017 * DAY));
        assert_eq!(parse("2026-10-01T00:00:00Z"), Some(20_727 * DAY));
        assert_eq!(parse(" 2026-10-01 12:30 "), Some(20_727 * DAY + 45_000_000_000));
        assert_eq!(parse("2026-10-01t12:30:05.25z"), Some(20_727 * DAY + 45_005_250_000));
        // Seven to nine decimals: the microseconds are kept.
        assert_eq!(parse("1970-01-01T00:00:00.123456789"), Some(123_456));
        assert_eq!(parse("1970-01-01T00:00:00,5"), Some(500_000));
        assert_eq!(parse("0000-01-01"), Some(-719_528 * DAY));
        assert_eq!(parse("9999-12-31T23:59:59.999999Z"), Some(253_402_300_799_999_999));
    }

    #[test]
    fn offsets() {
        let utc = parse("2026-10-01T12:00:00Z").unwrap();
        for (text, minutes) in [("+02:00", 120), ("-05:30", -330), ("+0200", 120), ("+02", 120), ("-00:00", 0)] {
            assert_eq!(parse(&format!("2026-10-01T12:00:00{text}")), Some(utc - minutes * 60_000_000), "{text}");
        }
    }

    #[test]
    fn refuses_what_is_not_a_date() {
        for bad in [
            "", "2026", "2026-10", "2026-1-01", "26-10-01", "2026-13-01", "2026-00-10", "2026-02-29", "1900-02-29", "2026-04-31",
            "2026-10-01T", "2026-10-01T24:00", "2026-10-01T12:60", "2026-10-01T12:00:60", "2026-10-01T12", "2026-10-01T12:00:00.",
            "2026-10-01T12:00:00.1234567890", "2026-10-01T12:00:00+24:00", "2026-10-01T12:00:00 Z", "2026-10-01T12:00:00Zx", "now",
            "+2026-10-01", "2026-10-01T12:00:00+2",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        assert_eq!(parse("2000-02-29"), Some(11_016 * DAY));
        assert_eq!(parse("2024-02-29").map(format).as_deref(), Some("2024-02-29T00:00:00Z"));
    }

    #[test]
    fn truncates_to_units() {
        let t = parse("2026-11-05T07:31:52.123456Z").unwrap();
        let cut = |u: &str| format(trunc(t, Unit::parse(u).unwrap()).unwrap());
        assert_eq!(cut("second"), "2026-11-05T07:31:52Z");
        assert_eq!(cut("MINUTES"), "2026-11-05T07:31:00Z");
        assert_eq!(cut("hour"), "2026-11-05T07:00:00Z");
        assert_eq!(cut("day"), "2026-11-05T00:00:00Z");
        // 2026-11-05 is a Thursday: its week starts on Monday the 2nd.
        assert_eq!(cut("week"), "2026-11-02T00:00:00Z");
        assert_eq!(cut("month"), "2026-11-01T00:00:00Z");
        assert_eq!(cut("quarter"), "2026-10-01T00:00:00Z");
        assert_eq!(cut("years"), "2026-01-01T00:00:00Z");
        // Before 1970, and a Monday is its own week's start.
        assert_eq!(format(trunc(parse("1969-12-31T23:59:59.5Z").unwrap(), Unit::Day).unwrap()), "1969-12-31T00:00:00Z");
        assert_eq!(format(trunc(parse("1969-12-29T12:00Z").unwrap(), Unit::Week).unwrap()), "1969-12-29T00:00:00Z");
        assert_eq!(format(trunc(parse("2024-02-29T12:00Z").unwrap(), Unit::Quarter).unwrap()), "2024-01-01T00:00:00Z");
        assert_eq!(Unit::parse("fortnight"), None);
        assert_eq!(trunc(i64::MIN, Unit::Day), None);
        assert_eq!(trunc(i64::MIN, Unit::Second), None);
        assert!(trunc(i64::MAX, Unit::Second).is_some());
    }

    #[test]
    fn parts_of_times() {
        let t = parse("2026-11-05T07:31:52.25Z").unwrap();
        let p = |name: &str| part(t, Part::parse(name).unwrap());
        for (name, want) in [("year", 2026), ("isoyear", 2026), ("quarter", 4), ("month", 11), ("week", 45), ("day", 5), ("doy", 309), ("dow", 4), ("isodow", 4), ("hour", 7), ("minute", 31)] {
            assert_eq!(p(name), PartValue::Int(want), "{name}");
        }
        assert_eq!(p("SECONDS"), PartValue::Float(52.25));
        assert_eq!(p("epoch"), PartValue::Float(t as f64 / 1e6));
        assert_eq!(Part::parse("days"), Some(Part::Day));
        assert_eq!(Part::parse("century"), None);
        // ISO weeks at the year's ends: 2021-01-03 (a Sunday) is in 2020's
        // week 53, and 2024-12-30 (a Monday) in 2025's week 1.
        let at = |d: &str| parse(d).unwrap();
        assert_eq!((part(at("2021-01-03"), Part::IsoYear), part(at("2021-01-03"), Part::Week)), (PartValue::Int(2020), PartValue::Int(53)));
        assert_eq!((part(at("2024-12-30"), Part::IsoYear), part(at("2024-12-30"), Part::Week)), (PartValue::Int(2025), PartValue::Int(1)));
        assert_eq!(part(at("2026-11-08"), Part::Dow), PartValue::Int(0));
        assert_eq!(part(at("2026-11-08"), Part::IsoDow), PartValue::Int(7));
        assert_eq!(part(at("1969-12-31T23:00Z"), Part::Hour), PartValue::Int(23));
    }

    #[test]
    fn strftime_as_sqlite() {
        let t = parse("2026-11-05T07:31:52.25Z").unwrap();
        assert_eq!(strftime("%Y-%m-%d %H:%M:%S", t).unwrap(), "2026-11-05 07:31:52");
        assert_eq!(strftime("%F %T, %f", t).unwrap(), "2026-11-05 07:31:52, 52.250");
        assert_eq!(strftime("%j %w %u %W %V %G %s 100%%", t).unwrap(), "309 4 4 44 45 2026 1793863912 100%");
        assert_eq!(strftime("%Y-%m", parse("1969-12-31T23:59Z").unwrap()).unwrap(), "1969-12");
        // Week of the year from the first Monday: 2026-01-01 is a Thursday.
        assert_eq!(strftime("%W", parse("2026-01-04").unwrap()).unwrap(), "00");
        assert_eq!(strftime("%W", parse("2026-01-05").unwrap()).unwrap(), "01");
        assert!(strftime("%Q", t).unwrap_err().contains("no %Q"));
        assert!(strftime("50%", t).unwrap_err().contains("lone %"));
    }

    #[test]
    fn formats_utc_and_round_trips() {
        assert_eq!(format(0), "1970-01-01T00:00:00Z");
        assert_eq!(format(-1), "1969-12-31T23:59:59.999999Z");
        assert_eq!(format(1_500), "1970-01-01T00:00:00.001500Z");
        assert_eq!(format(250_000), "1970-01-01T00:00:00.250Z");
        assert_eq!(format(i64::MAX), "+294247-01-10T04:00:54.775807Z");
        assert_eq!(format(i64::MIN), "-290308-12-21T19:59:05.224192Z");
        // Days from 0000-01-01 to 9999-12-31, at odd times.
        for day in (-719_528..2_932_897).step_by(97) {
            let t = day * DAY + (day.rem_euclid(86_400)) * 1_000_003 % DAY;
            assert_eq!(parse(&format(t)), Some(t), "{}", format(t));
        }
    }
}
