use super::*;

fn spec() -> Spec {
    Spec {
        locations: 3,
        periods: 10,
        period_type: PeriodType::Month,
        cases: 50,
        covariates: 4,
        seed: 7,
    }
}

#[test]
fn the_same_spec_gives_the_same_dataset() {
    assert_eq!(generate(&spec()), generate(&spec()));
    let other = Spec { seed: 8, ..spec() };
    assert_ne!(generate(&spec()), generate(&other));
}

#[test]
fn every_column_has_one_value_for_each_location_and_period() {
    let data = generate(&spec());
    assert_eq!(data.rows(), 30);
    let names: Vec<&str> = data.columns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "disease_cases",
            "rainfall",
            "mean_temperature",
            "population",
            "covariate_4"
        ]
    );
    for (name, values) in &data.columns {
        assert_eq!(values.len(), 30, "{name}");
        assert!(values.iter().all(|v| v.is_finite()), "{name}");
    }
    assert_eq!(data.locations, ["loc01", "loc02", "loc03"]);
    assert_eq!(spec().values(), 150);
}

#[test]
fn cases_are_whole_and_not_negative_and_follow_the_typical_size() {
    let data = generate(&Spec {
        locations: 40,
        periods: 60,
        cases: 1000,
        ..spec()
    });
    let cases = &data.columns[0].1;
    assert!(cases.iter().all(|c| *c >= 0.0 && c.fract() == 0.0));
    let mean = cases.iter().sum::<f64>() / cases.len() as f64;
    assert!((300.0..3000.0).contains(&mean), "mean {mean}");

    let none = generate(&Spec { cases: 0, ..spec() });
    assert!(none.columns[0].1.iter().all(|c| *c == 0.0));
}

#[test]
fn no_covariates_leaves_the_target_only() {
    let data = generate(&Spec {
        covariates: 0,
        ..spec()
    });
    assert_eq!(data.columns.len(), 1);
    assert_eq!(data.columns[0].0, TARGET);
}

#[test]
fn the_periods_end_with_the_last_period_of_2024() {
    let months = periods(PeriodType::Month, 14);
    assert_eq!(months.first().unwrap().api(), "202311");
    assert_eq!(months.last().unwrap().api(), "202412");
    assert_eq!(months.last().unwrap().csv(), "2024-12");

    let weeks = periods(PeriodType::Week, 53);
    assert_eq!(weeks.last().unwrap().api(), "2024W52");
    // 2023 has 52 weeks, so 53 weeks back from 2024W52 is 2023W52.
    assert_eq!(weeks.first().unwrap().api(), "2023W52");
}

#[test]
fn a_year_that_starts_on_a_thursday_has_53_iso_weeks() {
    // 2020 starts on a Wednesday and is a leap year; 2015 and 2026 start on
    // a Thursday.
    for year in [2004, 2009, 2015, 2020, 2026] {
        assert_eq!(periods::iso_weeks(year), 53, "{year}");
    }
    for year in [2019, 2021, 2023, 2024, 2025] {
        assert_eq!(periods::iso_weeks(year), 52, "{year}");
    }
    // Back across 2020W53.
    let weeks = periods(PeriodType::Week, 52 * 4 + 2);
    let names: Vec<String> = weeks.iter().take(3).map(Period::api).collect();
    assert_eq!(names, ["2020W52", "2020W53", "2021W1"]);
}

#[test]
fn the_csv_has_a_header_and_a_row_for_each_location_and_period() {
    let data = generate(&spec());
    let text = csv(&data);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "time_period,location,disease_cases,rainfall,mean_temperature,population,covariate_4"
    );
    assert_eq!(lines.len(), 31);
    assert!(lines[1].starts_with("2024-03,loc01,"), "{}", lines[1]);
    assert!(lines[30].starts_with("2024-12,loc03,"), "{}", lines[30]);
}

#[test]
fn every_location_is_a_closed_square_with_its_id() {
    let data = generate(&spec());
    let geo = geojson(&data);
    let features = geo["features"].as_array().unwrap();
    assert_eq!(features.len(), 3);
    for (feature, id) in features.iter().zip(&data.locations) {
        assert_eq!(feature["id"], id.as_str());
        assert_eq!(feature["properties"]["id"], id.as_str());
        let ring = feature["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.len(), 5);
        assert_eq!(ring[0], ring[4]);
    }
}

#[test]
fn the_make_dataset_body_has_one_column_for_each_feature() {
    let data = generate(&spec());
    let body = make_dataset_body(&data, "x");
    let columns = &body["providedDataColumns"];
    assert_eq!(columns["orgUnit"].as_array().unwrap().len(), 30);
    assert_eq!(columns["period"][0], "202403");
    assert_eq!(columns["period"][10], "202403");
    assert_eq!(columns["orgUnit"][10], "loc02");
    assert_eq!(
        columns["values"]["disease_cases"].as_array().unwrap().len(),
        30
    );
    assert_eq!(body["name"], "x");
    assert!(body.get("providedData").is_none());
}

#[test]
fn the_name_says_what_the_dataset_is_made_of() {
    assert_eq!(spec().name(), "synthetic-3x10m-c50-k4-s7");
}
