//! Synthetic Chap datasets for tests and benchmarks.
//!
//! The same [`Spec`] always gives the same dataset: every value comes from
//! one random number generator that starts from the seed. The values have
//! a season, a trend in the population, and cases that follow the rainfall
//! of the period before, so a model has something to learn. They are not
//! realistic data. See `docs/bench.md`.

mod output;
mod periods;

pub use output::{Format, csv, geojson, make_dataset_body, prediction_run_body};
pub use periods::{Period, PeriodType, periods};

/// The column a model predicts.
pub const TARGET: &str = "disease_cases";

/// The covariates, in the order `--covariates N` takes them. Past the end
/// of this list they are `covariate_<n>`.
pub const NAMED_COVARIATES: &[&str] = &["rainfall", "mean_temperature", "population"];

/// What to generate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub locations: usize,
    pub periods: usize,
    pub period_type: PeriodType,
    /// The typical number of cases in one location and period.
    pub cases: u64,
    pub covariates: usize,
    pub seed: u64,
}

impl Spec {
    /// The default name of the dataset: what it is made of, so two
    /// different datasets do not get the same name.
    pub fn name(&self) -> String {
        format!(
            "synthetic-{}x{}{}-c{}-k{}-s{}",
            self.locations,
            self.periods,
            self.period_type.letter(),
            self.cases,
            self.covariates,
            self.seed
        )
    }

    /// How many values the dataset has: one for each row and column.
    pub fn values(&self) -> usize {
        self.locations * self.periods * (self.covariates + 1)
    }
}

/// A generated dataset, one row for each location and period, location by
/// location.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    pub locations: Vec<String>,
    pub periods: Vec<Period>,
    /// Each column with its values in row order: the target first, then the
    /// covariates.
    pub columns: Vec<(String, Vec<f64>)>,
}

impl Dataset {
    pub fn rows(&self) -> usize {
        self.locations.len() * self.periods.len()
    }
}

/// The covariate names for `n` covariates.
pub fn covariate_names(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| match NAMED_COVARIATES.get(i) {
            Some(name) => name.to_string(),
            None => format!("covariate_{}", i + 1),
        })
        .collect()
}

/// The org unit id of location `i` (from 0) of `n`: `loc01` to `loc18`.
pub fn location_id(i: usize, n: usize) -> String {
    let width = n.to_string().len().max(2);
    format!("loc{:0width$}", i + 1)
}

/// Generate the dataset of `spec`.
pub fn generate(spec: &Spec) -> Dataset {
    let mut rng = Rng::new(spec.seed);
    let periods = periods(spec.period_type, spec.periods);
    let season = spec.period_type.season() as f64;
    let names = covariate_names(spec.covariates);
    let rows = spec.locations * spec.periods;
    let mut columns: Vec<(String, Vec<f64>)> = std::iter::once(TARGET.to_string())
        .chain(names.iter().cloned())
        .map(|name| (name, Vec::with_capacity(rows)))
        .collect();

    for _ in 0..spec.locations {
        let scale = spec.cases as f64 * (0.5 * rng.normal()).exp();
        let phase = rng.uniform() * std::f64::consts::TAU;
        let rain_scale = 50.0 + 200.0 * rng.uniform();
        let temperature = 22.0 + 6.0 * rng.uniform();
        let population = 10f64.powf(4.0 + 2.0 * rng.uniform());
        let mut rain_before = rain_scale;
        for t in 0..spec.periods {
            let angle = std::f64::consts::TAU * t as f64 / season + phase;
            let wave = angle.sin();
            let rain = (rain_scale * (1.0 + 0.8 * wave) * (0.5 + rng.uniform())).max(0.0);
            // Cases follow the season a little later, and the rain of the
            // period before.
            let lagged = (angle - 0.5).sin();
            let mean = scale
                * (1.0 + 0.6 * lagged)
                * (1.0 + 0.3 * (rain_before / rain_scale - 1.0)).max(0.05);
            let cases = (mean + mean.sqrt() * rng.normal()).max(0.0).round();
            columns[0].1.push(cases);
            for (k, column) in columns.iter_mut().skip(1).enumerate() {
                let value = match k {
                    0 => round(rain, 1),
                    1 => round(temperature + 4.0 * wave + 0.8 * rng.normal(), 2),
                    2 => (population * (1.0 + 0.02 * t as f64 / season)).round(),
                    _ => round(
                        10.0 * (1.0 + 0.5 * (angle + k as f64).sin()) + rng.normal(),
                        3,
                    ),
                };
                column.1.push(value);
            }
            rain_before = rain;
        }
    }
    Dataset {
        locations: (0..spec.locations)
            .map(|i| location_id(i, spec.locations))
            .collect(),
        periods,
        columns,
    }
}

fn round(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

/// SplitMix64: small, fast, and the same on every machine, which is all a
/// test dataset needs. Not for anything secret.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)`.
    pub(crate) fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A standard normal number (Box-Muller).
    pub(crate) fn normal(&mut self) -> f64 {
        let u = 1.0 - self.uniform();
        let v = self.uniform();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

#[cfg(test)]
mod tests;
