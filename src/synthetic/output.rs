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

/// The body of `POST /v1/analytics/make-dataset`, with the observations in
/// columns (`providedDataColumns`, chap-core v2.7.0 and later): much
/// smaller than one object for each value.
pub fn make_dataset_body(data: &Dataset, name: &str) -> Value {
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
    json!({
        "name": name,
        "geojson": geojson(data),
        "providedDataColumns": {
            "orgUnit": org_units,
            "period": periods,
            "values": values,
        },
        "dataToBeFetched": [],
    })
}

/// The observations one object each (`providedData`), as the Modeling App
/// sends them: org unit, DHIS2 period id, feature name and value.
pub fn observation_rows(data: &Dataset) -> Vec<Value> {
    let mut rows = Vec::with_capacity(data.rows() * data.columns.len());
    let mut row = 0;
    for location in &data.locations {
        for period in &data.periods {
            let period = period.api();
            for (name, values) in &data.columns {
                rows.push(json!({
                    "orgUnit": location,
                    "period": period,
                    "featureName": name,
                    "value": values[row],
                }));
            }
            row += 1;
        }
    }
    rows
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
