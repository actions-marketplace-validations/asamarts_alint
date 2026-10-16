//! Deterministic reference time for time-dependent analysis.
//!
//! Explicit inputs are strict UTC dates. Repository fallback is the HEAD
//! committer timestamp. Rule implementations consume this value and never read
//! the host wall clock directly.

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Latest Unix timestamp representable by the public `YYYY-MM-DD` contract:
/// 9999-12-31T23:59:59Z.
const MAX_UNIX_SECONDS: u64 = 253_402_300_799;
const SECONDS_PER_DAY: u64 = 86_400;

/// One immutable UTC reference instant used throughout an analysis operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalysisTime {
    unix_seconds: u64,
}

impl AnalysisTime {
    /// Build from a nonnegative Unix timestamp when its UTC date is in
    /// `1970-01-01..=9999-12-31`.
    #[must_use]
    pub const fn from_unix_seconds(unix_seconds: u64) -> Option<Self> {
        if unix_seconds <= MAX_UNIX_SECONDS {
            Some(Self { unix_seconds })
        } else {
            None
        }
    }

    /// Seconds since 1970-01-01T00:00:00Z.
    #[must_use]
    pub const fn unix_seconds(self) -> u64 {
        self.unix_seconds
    }

    /// Elapsed time from a Git author timestamp. Future or pre-epoch author
    /// times have age zero, matching `git_blame_age`'s historical behavior.
    #[must_use]
    pub fn elapsed_since(self, earlier: SystemTime) -> Duration {
        let Ok(author) = earlier.duration_since(UNIX_EPOCH) else {
            return Duration::ZERO;
        };
        Duration::from_secs(self.unix_seconds.saturating_sub(author.as_secs()))
    }

    fn date_parts(self) -> (i32, u32, u32) {
        let days = i64::try_from(self.unix_seconds / SECONDS_PER_DAY)
            .expect("supported analysis dates fit i64 days");
        civil_from_days(days)
    }
}

impl fmt::Display for AnalysisTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.date_parts();
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// Strict `YYYY-MM-DD` parse failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseAnalysisTimeError {
    #[error("analysis date must be exactly YYYY-MM-DD with ASCII digits")]
    InvalidFormat,
    #[error("analysis date must be a valid UTC date from 1970-01-01 through 9999-12-31")]
    OutOfRange,
}

impl FromStr for AnalysisTime {
    type Err = ParseAnalysisTimeError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = input.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
        {
            return Err(ParseAnalysisTimeError::InvalidFormat);
        }
        let year = digits(bytes, 0, 4);
        let month = digits(bytes, 5, 7);
        let day = digits(bytes, 8, 10);
        if !(1970..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || !(1..=days_in_month(year, month)).contains(&day)
        {
            return Err(ParseAnalysisTimeError::OutOfRange);
        }
        let days = days_from_civil(
            i32::try_from(year).expect("four-digit year fits i32"),
            month,
            day,
        );
        let unix_seconds = u64::try_from(days)
            .ok()
            .and_then(|days| days.checked_mul(SECONDS_PER_DAY))
            .ok_or(ParseAnalysisTimeError::OutOfRange)?;
        Self::from_unix_seconds(unix_seconds).ok_or(ParseAnalysisTimeError::OutOfRange)
    }
}

const fn digits(bytes: &[u8], start: usize, end: usize) -> u32 {
    let mut value = 0;
    let mut i = start;
    while i < end {
        value = value * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    value
}

const fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

// Howard Hinnant's civil-calendar conversion, shifted to the Unix epoch.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month =
        i32::try_from(month).expect("validated month fits i32") + if month > 2 { -3 } else { 9 };
    let day_of_year =
        (153 * shifted_month + 2) / 5 + i32::try_from(day).expect("validated day fits i32") - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (
        i32::try_from(year).expect("supported analysis year fits i32"),
        u32::try_from(month).expect("computed civil month is positive and fits u32"),
        u32::try_from(day).expect("computed civil day is positive and fits u32"),
    )
}

/// Where a resolved reference instant came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisTimeSource {
    Explicit,
    Head,
}
impl fmt::Display for AnalysisTimeSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Explicit => "explicit",
            Self::Head => "head",
        })
    }
}

impl AnalysisTimeSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Head => "head",
        }
    }
}

/// Stable reasons why the repository fallback could not provide an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisTimeUnavailable {
    #[error("Git is unavailable; pass --as-of YYYY-MM-DD")]
    GitUnavailable,
    #[error(
        "HEAD commit time is unavailable (not a Git repository or no commits); pass --as-of YYYY-MM-DD"
    )]
    HeadUnavailable,
    #[error(
        "HEAD commit time is invalid or outside 1970-01-01 through 9999-12-31; pass --as-of YYYY-MM-DD"
    )]
    InvalidHeadTime,
    #[error(
        "analysis engine was reused with a different repository root; construct one engine per root"
    )]
    EngineRootChanged,
}

/// One successful or failed resolution, cached and shared by an engine run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisTimeResolution {
    Available {
        time: AnalysisTime,
        source: AnalysisTimeSource,
    },
    Unavailable(AnalysisTimeUnavailable),
}

impl AnalysisTimeResolution {
    #[must_use]
    pub const fn available(time: AnalysisTime, source: AnalysisTimeSource) -> Self {
        Self::Available { time, source }
    }

    #[must_use]
    pub const fn time(self) -> Option<AnalysisTime> {
        match self {
            Self::Available { time, .. } => Some(time),
            Self::Unavailable(_) => None,
        }
    }

    #[must_use]
    pub const fn source(self) -> Option<AnalysisTimeSource> {
        match self {
            Self::Available { source, .. } => Some(source),
            Self::Unavailable(_) => None,
        }
    }

    #[must_use]
    pub const fn error(self) -> Option<AnalysisTimeUnavailable> {
        match self {
            Self::Available { .. } => None,
            Self::Unavailable(error) => Some(error),
        }
    }
}

/// Resolve an explicit instant or the repository's HEAD committer timestamp.
#[must_use]
pub fn resolve_analysis_time(
    root: &Path,
    explicit: Option<AnalysisTime>,
) -> AnalysisTimeResolution {
    if let Some(time) = explicit {
        return AnalysisTimeResolution::available(time, AnalysisTimeSource::Explicit);
    }
    match crate::git::head_commit_time(root) {
        Ok(time) => AnalysisTimeResolution::available(time, AnalysisTimeSource::Head),
        Err(error) => AnalysisTimeResolution::Unavailable(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_date_parser_accepts_epoch_leaps_and_upper_bound() {
        for (input, seconds) in [
            ("1970-01-01", 0),
            ("2000-02-29", 951_782_400),
            ("2026-10-16", 1_792_108_800),
            ("9999-12-31", 253_402_214_400),
        ] {
            let parsed: AnalysisTime = input.parse().unwrap();
            assert_eq!(parsed.unix_seconds(), seconds, "{input}");
            assert_eq!(parsed.to_string(), input);
        }
    }

    #[test]
    fn strict_date_parser_rejects_noncanonical_and_invalid_dates() {
        for input in [
            "",
            "2026-1-01",
            "2026-01-1",
            " 2026-01-01",
            "2026-01-01 ",
            "2026/01/01",
            "２０２６-01-01",
            "1969-12-31",
            "0000-01-01",
            "2026-00-01",
            "2026-13-01",
            "2026-04-31",
            "2026-02-29",
            "2100-02-29",
            "2026-01-01T00:00:00Z",
        ] {
            assert!(input.parse::<AnalysisTime>().is_err(), "accepted {input:?}");
        }
        assert!("2000-02-29".parse::<AnalysisTime>().is_ok());
        assert!("2400-02-29".parse::<AnalysisTime>().is_ok());
    }

    #[test]
    fn unix_instants_render_their_utc_date() {
        assert_eq!(
            AnalysisTime::from_unix_seconds(86_399).unwrap().to_string(),
            "1970-01-01"
        );
        assert_eq!(
            AnalysisTime::from_unix_seconds(86_400).unwrap().to_string(),
            "1970-01-02"
        );
        assert!(AnalysisTime::from_unix_seconds(MAX_UNIX_SECONDS + 1).is_none());
    }

    #[test]
    fn elapsed_time_saturates_for_future_authors() {
        let reference = AnalysisTime::from_unix_seconds(100).unwrap();
        assert_eq!(
            reference.elapsed_since(UNIX_EPOCH + Duration::from_secs(40)),
            Duration::from_secs(60)
        );
        assert_eq!(
            reference.elapsed_since(UNIX_EPOCH + Duration::from_secs(101)),
            Duration::ZERO
        );
    }
}
