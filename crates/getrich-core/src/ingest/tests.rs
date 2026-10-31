use super::*;
#[test]
fn fetch_duration_is_included_without_overlapping_or_catching_up() {
    assert_eq!(
        poll_delay(5000, 200, Duration::from_millis(1800)),
        Duration::from_millis(3400)
    );
    assert_eq!(
        poll_delay(5000, 0, Duration::from_millis(8000)),
        Duration::ZERO
    );
    assert_eq!(
        poll_delay(4500, 0, Duration::from_millis(1)),
        Duration::from_millis(4499)
    );
}
