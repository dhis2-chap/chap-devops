use super::*;
use crate::top::data::{Kind, Node, Service};

fn service(name: &str, cpu: Option<&str>, memory: Option<&str>) -> Service {
    Service {
        service: name.to_string(),
        role: "chap-core".to_string(),
        model: None,
        state: "running".to_string(),
        health: Some("healthy".to_string()),
        cpu: cpu.map(str::to_string),
        memory: memory.map(str::to_string),
        url: None,
    }
}

#[test]
fn sizes_are_read_in_the_units_docker_prints() {
    assert_eq!(parse_size("512B"), Some(512));
    assert_eq!(parse_size("1.5KiB"), Some(1536));
    assert_eq!(parse_size("181.2MiB"), Some(190_001_971));
    assert_eq!(parse_size("2GiB"), Some(2 * 1024 * 1024 * 1024));
    assert_eq!(parse_size("2GB"), Some(2_000_000_000));
    assert_eq!(parse_size(" 3kB "), Some(3000));
    assert_eq!(parse_size("--"), None);
    assert_eq!(parse_size("3 parsecs"), None);
}

#[test]
fn memory_gives_the_use_and_the_limit() {
    assert_eq!(
        parse_memory("1MiB / 2GiB"),
        (Some(1024 * 1024), Some(2 * 1024 * 1024 * 1024))
    );
    assert_eq!(parse_memory("0B / 0B"), (Some(0), Some(0)));
    assert_eq!(parse_memory(""), (None, None));
}

#[test]
fn percent_drops_the_sign() {
    assert_eq!(parse_percent("12.34%"), Some(12.34));
    assert_eq!(parse_percent("250.00%"), Some(250.0));
    assert_eq!(parse_percent("--"), None);
}

#[test]
fn one_sample_per_service_and_a_line_each() {
    let nodes = vec![Node {
        project: "demo-ab12cd".to_string(),
        name: "demo".to_string(),
        kind: Kind::Init,
        group: None,
        dir: None,
        services: vec![
            service("chap", Some("2.00%"), Some("300MiB / 8GiB")),
            service("dhis2", None, None),
        ],
    }];
    let tick = samples(&nodes, 1_700_000_000);
    assert_eq!(tick.len(), 2);
    assert_eq!(tick[0].deployment, "demo");
    assert_eq!(tick[0].cpu_percent, Some(2.0));
    assert_eq!(tick[0].memory_bytes, Some(300 * 1024 * 1024));
    assert_eq!(tick[1].cpu_percent, None);

    let mut out = Vec::new();
    append(&mut out, &tick).expect("written");
    let text = String::from_utf8(out).expect("text");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("json");
    assert_eq!(first["service"], "chap");
    assert_eq!(first["unix"], 1_700_000_000u64);
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("json");
    assert!(second.get("cpu_percent").is_none(), "{second}");
}
