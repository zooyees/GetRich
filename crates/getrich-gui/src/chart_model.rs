//! Shared viewport math for drawing, pointer hits and keyboard controls.
pub fn viewport(length: usize, span: usize, end: usize) -> (usize, usize, usize) {
    if length == 0 {
        return (0, 0, 0);
    }
    let span = if span == 0 {
        120.min(length)
    } else {
        span.clamp(1, length)
    };
    let end = if end == 0 {
        length
    } else {
        end.clamp(span, length)
    };
    (end - span, end, span)
}
pub fn hit(x: f32, left: f32, slot: f32, pad: f32, count: usize) -> Option<usize> {
    if !slot.is_finite() || slot <= 0.0 {
        return None;
    }
    let index = ((x - left) / slot - pad).floor();
    (index >= 0.0 && index < count as f32).then_some(index as usize)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_series_lengths_can_zoom_and_pan() {
        for n in [0, 1, 2, 7, 8, 80, 800] {
            for span in [0, 1, 7, 8, 120, 1000] {
                for end in [0, 1, 6, 120, 900] {
                    let (a, b, s) = viewport(n, span, end);
                    assert!(a <= b && b <= n);
                    assert_eq!(b - a, s);
                    assert!(n == 0 || s >= 1);
                }
            }
        }
    }
    #[test]
    fn padding_is_not_a_candle() {
        assert_eq!(hit(10.0, 0.0, 10.0, 70.0, 10), None);
        assert_eq!(hit(705.0, 0.0, 10.0, 70.0, 10), Some(0));
        assert_eq!(hit(795.0, 0.0, 10.0, 70.0, 10), Some(9));
    }
}
