/// Snapshot helpers with standard redactions for non-deterministic fields.
///
/// Usage in tests:
/// ```rust
/// use bendclaw_test_harness::snapshot;
/// snapshot::assert_json("my_snapshot", &value);
/// ```

/// Assert a JSON snapshot, redacting ULIDs and timestamps.
#[macro_export]
macro_rules! assert_json_snapshot {
    ($name:expr, $value:expr) => {
        insta::assert_json_snapshot!($name, $value, {
            ".**id" => "[id]",
            ".**_at" => "[timestamp]",
            ".**_time" => "[timestamp]",
        });
    };
}

/// Assert a YAML snapshot, redacting ULIDs and timestamps.
#[macro_export]
macro_rules! assert_yaml_snapshot {
    ($name:expr, $value:expr) => {
        insta::assert_yaml_snapshot!($name, $value, {
            ".**id" => "[id]",
            ".**_at" => "[timestamp]",
        });
    };
}
