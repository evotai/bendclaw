pub use chrono::DateTime;
pub use chrono::Utc;

/// Current UTC timestamp.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}
