//! The periods of a synthetic dataset: ISO weeks or months that end with
//! the last period of 2024, so the same spec gives the same periods on any
//! day.

use clap::ValueEnum;

/// The year the periods end in.
const LAST_YEAR: i32 = 2024;

/// Weekly or monthly periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PeriodType {
    Week,
    Month,
}

impl PeriodType {
    /// How many periods a season has.
    pub fn season(self) -> u32 {
        match self {
            PeriodType::Week => 52,
            PeriodType::Month => 12,
        }
    }

    /// `w` or `m`, for the dataset name.
    pub fn letter(self) -> char {
        match self {
            PeriodType::Week => 'w',
            PeriodType::Month => 'm',
        }
    }
}

/// One ISO week or one month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub kind: PeriodType,
    pub year: i32,
    /// The week (1 to 53) or the month (1 to 12).
    pub index: u32,
}

impl Period {
    /// The form DHIS2 and the Modeling App send to chap-core: `2024W7`,
    /// `202407`.
    pub fn api(&self) -> String {
        match self.kind {
            PeriodType::Week => format!("{}W{}", self.year, self.index),
            PeriodType::Month => format!("{}{:02}", self.year, self.index),
        }
    }

    /// The form of chap's CSV files: `2024W7`, `2024-07`.
    pub fn csv(&self) -> String {
        match self.kind {
            PeriodType::Week => format!("{}W{}", self.year, self.index),
            PeriodType::Month => format!("{}-{:02}", self.year, self.index),
        }
    }

    fn previous(&self) -> Period {
        match (self.kind, self.index) {
            (PeriodType::Week, 1) => Period {
                year: self.year - 1,
                index: iso_weeks(self.year - 1),
                ..*self
            },
            (PeriodType::Month, 1) => Period {
                year: self.year - 1,
                index: 12,
                ..*self
            },
            _ => Period {
                index: self.index - 1,
                ..*self
            },
        }
    }
}

/// `n` periods of `kind`, oldest first, the last one in 2024.
pub fn periods(kind: PeriodType, n: usize) -> Vec<Period> {
    let mut period = Period {
        kind,
        year: LAST_YEAR,
        index: match kind {
            PeriodType::Week => iso_weeks(LAST_YEAR),
            PeriodType::Month => 12,
        },
    };
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(period);
        period = period.previous();
    }
    out.reverse();
    out
}

/// How many ISO weeks `year` has: 53 when it starts on a Thursday, or on a
/// Wednesday in a leap year, else 52.
pub fn iso_weeks(year: i32) -> u32 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match weekday_of_january_first(year) {
        4 => 53,
        3 if leap => 53,
        _ => 52,
    }
}

/// The ISO weekday of 1 January of `year`: 1 for Monday, 7 for Sunday.
fn weekday_of_january_first(year: i32) -> u32 {
    // Days from 1970-01-01, a Thursday, by Howard Hinnant's civil algorithm.
    let y = i64::from(year) - 1;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    // 1 January is day 306 of the March-based year before it.
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + 306;
    let days = era * 146_097 + doe - 719_468;
    ((days.rem_euclid(7) + 3) % 7 + 1) as u32
}
