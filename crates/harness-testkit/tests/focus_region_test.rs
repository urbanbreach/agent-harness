#[path = "support/focus_region.rs"]
mod focus_region;

use focus_region::anchored_region;

#[test]
fn anchored_region_applies_padding_before_anchor() {
    assert_eq!(
        anchored_region((5, 7), (20, 30), 10, 8, 2, 3),
        (3, 4, 8, 10)
    );
}

#[test]
fn anchored_region_clamps_to_bounds_with_minimum_size() {
    assert_eq!(anchored_region((1, 1), (3, 4), 10, 10, 5, 5), (0, 0, 3, 4));
    assert_eq!(anchored_region((3, 4), (3, 4), 10, 10, 0, 0), (3, 4, 1, 1));
}
