//! A synthetic dataset as chap's CSV, as GeoJSON, and as the body of
//! `POST /v1/analytics/make-dataset`.

use super::Dataset;
use serde_json::{Value, json};

/// The size of one location's square, in degrees.
const CELL: f64 = 0.1;

/// Where the grid of squares starts: open sea, so the squares are not
/// mistaken for a real place on a map.
const ORIGIN: (f64, f64) = (-30.0, -20.0);

/// The dataset as the CSV `chap eval` reads: `time_period`, `location`, the
/// target, then the covariates, one row for each location and period.
pub fn csv(data: &Dataset) -> String {
    let mut out = String::from("time_period,location");
    for (name, _) in &data.columns {
        out.push(',');
        out.push_str(name);
    }
    out.push('\n');
    let mut row = 0;
    for location in &data.locations {
        for period in &data.periods {
            out.push_str(&period.csv());
            out.push(',');
            out.push_str(location);
            for (_, values) in &data.columns {
                out.push(',');
                out.push_str(&number(values[row]));
            }
            out.push('\n');
            row += 1;
        }
    }
    out
}

/// One square for each location, on a grid, with the org unit as the
/// feature's `id`: chap-core drops a location whose feature has no `id`.
pub fn geojson(data: &Dataset) -> Value {
    let columns = (data.locations.len() as f64).sqrt().ceil().max(1.0) as usize;
    let features: Vec<Value> = data
        .locations
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let x = ORIGIN.0 + (i % columns) as f64 * CELL;
            let y = ORIGIN.1 + (i / columns) as f64 * CELL;
            let ring = [
                [x, y],
                [x + CELL, y],
                [x + CELL, y + CELL],
                [x, y + CELL],
                [x, y],
            ]
            .map(|[a, b]| json!([round6(a), round6(b)]));
            json!({
                "type": "Feature",
                "id": id,
                "properties": {"id": id, "name": format!("Location {}", i + 1)},
                "geometry": {"type": "Polygon", "coordinates": [ring]},
            })
        })
        .collect();
    json!({"type": "FeatureCollection", "features": features})
}

/// How the observations go to chap-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    // One object for each value (`providedData`), as the Modeling App sends
    // them. Not doc comments: clap would make them a long help.
    Rows,
    // One list for each column (`providedDataColumns`, chap-core v2.7.0 and
    // later): much smaller.
    Columns,
}

/// The body of `POST /v1/analytics/make-dataset`, as JSON text: the
/// observations of a large dataset are too many for a [`Value`].
pub fn make_dataset_body(data: &Dataset, name: &str, format: Format) -> String {
    let observations = match format {
        Format::Rows => format!(r#""providedData":{}"#, rows_text(data)),
        Format::Columns => format!(r#""providedDataColumns":{}"#, columns_text(data)),
    };
    format!(
        r#"{{"name":{},"geojson":{},{observations},"dataToBeFetched":[]}}"#,
        Value::String(name.to_string()),
        geojson(data)
    )
}

/// The body of `POST /v1/crud/prediction-setups/{id}/run`, as JSON text:
/// the observations one object each, as the Modeling App sends them.
pub fn prediction_run_body(data: &Dataset, name: &str) -> String {
    format!(
        r#"{{"name":{},"geojson":{},"providedData":{}}}"#,
        Value::String(name.to_string()),
        geojson(data),
        rows_text(data)
    )
}

/// The observations one object each: org unit, DHIS2 period id, feature
/// name and value, row by row.
pub fn rows_text(data: &Dataset) -> String {
    use std::fmt::Write;
    let names: Vec<String> = data
        .columns
        .iter()
        .map(|(name, _)| Value::String(name.clone()).to_string())
        .collect();
    let mut out = String::with_capacity(data.rows() * data.columns.len() * 72);
    out.push('[');
    let mut row = 0;
    let mut first = true;
    for location in &data.locations {
        let location = Value::String(location.clone()).to_string();
        for period in &data.periods {
            let period = period.api();
            for ((_, values), name) in data.columns.iter().zip(&names) {
                if !first {
                    out.push(',');
                }
                first = false;
                let _ = write!(
                    out,
                    r#"{{"orgUnit":{location},"period":"{period}","featureName":{name},"value":{}}}"#,
                    number(values[row])
                );
            }
            row += 1;
        }
    }
    out.push(']');
    out
}

fn columns_text(data: &Dataset) -> String {
    let mut org_units = Vec::with_capacity(data.rows());
    let mut periods = Vec::with_capacity(data.rows());
    for location in &data.locations {
        for period in &data.periods {
            org_units.push(location.clone());
            periods.push(period.api());
        }
    }
    let values: serde_json::Map<String, Value> = data
        .columns
        .iter()
        .map(|(name, values)| (name.clone(), json!(values)))
        .collect();
    json!({"orgUnit": org_units, "period": periods, "values": values}).to_string()
}

/// `12` for a whole number, else the shortest form that reads back the same.
fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn round6(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}
