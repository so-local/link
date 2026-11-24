link_test_kit::program!(system);

mod conformance;
mod credentials;
mod stopped;

/// The Node release Link pins for `dsh`, from the adapter's pins.
fn node_version() -> String {
    let pins: serde_json::Value =
        serde_json::from_str(include_str!("../../externals.json")).unwrap();
    pins["node"]["version"].as_str().unwrap().to_owned()
}
