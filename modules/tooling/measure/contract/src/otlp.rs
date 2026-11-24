//! Reports as OTLP JSON, written ([`otlp`]) and read ([`read_otlp`]).

use super::*;

/// `reports` as OTLP JSON (`ExportMetricsServiceRequest`), from
/// `service` (`lnk-measure`, `lnk-harness`): a resource per unit,
/// its points grouped by measure. Reports of the same unit are one
/// resource, so a batch of many (a request each) names the unit, and
/// describes each measure, once.
pub fn otlp(reports: &[Report], service: &str) -> Value {
    let mut units: Vec<(&Unit, Vec<&Point>)> = Vec::new();
    let mut index: std::collections::HashMap<&Unit, usize> = std::collections::HashMap::new();
    // A parent with no points still places its children in the unit tree.
    for report in reports {
        let at = *index.entry(&report.unit).or_insert_with(|| {
            units.push((&report.unit, Vec::new()));
            units.len() - 1
        });
        units[at].1.extend(&report.points);
    }
    let resources: Vec<Value> = units
        .into_iter()
        .map(|(unit, points)| {
            let metrics: Vec<Value> = MEASURES
                .iter()
                .filter_map(|m| {
                    let points: Vec<Value> = points
                        .iter()
                        .filter(|p| p.measure == m)
                        .map(|p| data_point(p))
                        .collect();
                    (!points.is_empty()).then(|| metric(m, points))
                })
                .collect();
            json!({
                "resource": {"attributes": unit.attributes(service)},
                "scopeMetrics": [{
                    "scope": {"name": "link", "version": env!("CARGO_PKG_VERSION")},
                    "metrics": metrics,
                }],
            })
        })
        .collect();
    json!({ "resourceMetrics": resources })
}

fn data_point(point: &Point) -> Value {
    let mut json = json!({"timeUnixNano": point.at.to_string()});
    if let Some(start) = point.start {
        json["startTimeUnixNano"] = start.to_string().into();
    }
    match whole(point.measure) {
        true => json["asInt"] = (point.value.round() as i64).to_string().into(),
        false => json["asDouble"] = point.value.into(),
    }
    json
}

/// Whether the measure's values are whole numbers (bytes, counts), sent
/// as OTLP's `asInt`; seconds are `asDouble`.
fn whole(measure: &Measure) -> bool {
    measure.unit != "s"
}

fn metric(measure: &Measure, points: Vec<Value>) -> Value {
    let mut json = json!({
        "name": measure.name,
        "unit": measure.unit,
        "description": measure.about,
    });
    match measure.kind {
        Kind::Gauge | Kind::Each => json["gauge"] = json!({"dataPoints": points}),
        // 1: delta, 2: cumulative.
        Kind::Total | Kind::Delta => {
            json["sum"] = json!({
                "dataPoints": points,
                "aggregationTemporality": if measure.kind == Kind::Total { 2 } else { 1 },
                "isMonotonic": true,
            });
        }
    }
    json
}

/// Where an OTLP endpoint at `url` takes metrics: `<url>/v1/metrics`,
/// unless it names that path already.
pub fn metrics_url(url: &str) -> String {
    let url = url.trim_end_matches('/');
    match url.ends_with("/v1/metrics") {
        true => url.to_string(),
        false => format!("{url}/v1/metrics"),
    }
}

/// The reports in OTLP JSON `request`, keeping only what the schema
/// has: measures Link knows, of the kind it says, finite and not
/// negative; units [`Unit::valid`]. Everything else is dropped: a name,
/// a description, an attribute Link doesn't have, a unit with text
/// where an ID goes.
pub fn read_otlp(request: &Value) -> Vec<Report> {
    let mut reports = Vec::new();
    for resource in request["resourceMetrics"].as_array().into_iter().flatten() {
        let value = |key: &str| -> Option<&Value> {
            resource["resource"]["attributes"]
                .as_array()?
                .iter()
                .find(|a| a["key"] == key)
                .map(|a| &a["value"])
        };
        let attribute = |key: &str| -> Option<String> {
            value(key)?["stringValue"].as_str().map(str::to_string)
        };
        let teams = value(TEAMS).and_then(|v| v["arrayValue"]["values"].as_array());
        let unit = Unit {
            id: attribute(ID).unwrap_or_default(),
            kind: attribute(KIND).unwrap_or_default(),
            parent: attribute(PARENT),
            name: attribute(NAME),
            machine: attribute(MACHINE),
            owner: attribute(OWNER),
            teams: teams
                .into_iter()
                .flatten()
                .filter_map(|t| t["stringValue"].as_str().map(str::to_string))
                .collect(),
        };
        let mut points = Vec::new();
        for scope in resource["scopeMetrics"].as_array().into_iter().flatten() {
            for metric in scope["metrics"].as_array().into_iter().flatten() {
                let Some(measure) = metric["name"].as_str().and_then(find) else {
                    continue;
                };
                let data = match measure.kind {
                    Kind::Gauge | Kind::Each => &metric["gauge"],
                    Kind::Total | Kind::Delta => &metric["sum"],
                };
                for point in data["dataPoints"].as_array().into_iter().flatten() {
                    let number = |key: &str| -> Option<u64> {
                        match &point[key] {
                            Value::String(s) => s.parse().ok(),
                            other => other.as_u64(),
                        }
                    };
                    let value = match (&point["asInt"], &point["asDouble"]) {
                        (Value::String(s), _) => s.parse::<i64>().ok().map(|v| v as f64),
                        (Value::Number(n), _) => n.as_f64(),
                        (_, Value::Number(n)) => n.as_f64(),
                        _ => None,
                    };
                    let (Some(value), Some(at)) = (value, number("timeUnixNano")) else {
                        continue;
                    };
                    if !value.is_finite() || value < 0.0 {
                        continue;
                    }
                    points.push(Point {
                        measure,
                        value,
                        at,
                        start: number("startTimeUnixNano"),
                    });
                }
            }
        }
        reports.push(Report { unit, points });
    }
    reports
}
