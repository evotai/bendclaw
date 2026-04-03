use bendclaw::execution::memory::pressure::assess;
use bendclaw::execution::memory::pressure::PressureLevel;

#[test]
fn normal() {
    assert_eq!(assess(50_000, 100_000), PressureLevel::Normal);
    assert_eq!(assess(69_999, 100_000), PressureLevel::Normal);
}

#[test]
fn elevated() {
    assert_eq!(assess(70_000, 100_000), PressureLevel::Elevated);
    assert_eq!(assess(84_999, 100_000), PressureLevel::Elevated);
}

#[test]
fn high() {
    assert_eq!(assess(85_000, 100_000), PressureLevel::High);
    assert_eq!(assess(100_000, 100_000), PressureLevel::High);
}

#[test]
fn critical() {
    assert_eq!(assess(101_000, 100_000), PressureLevel::Critical);
    assert_eq!(assess(200_000, 100_000), PressureLevel::Critical);
}

#[test]
fn zero_max() {
    assert_eq!(assess(0, 0), PressureLevel::Critical);
    assert_eq!(assess(100, 0), PressureLevel::Critical);
}
