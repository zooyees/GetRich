//! Shared viewport math for drawing, pointer hits and keyboard controls.
pub fn slot_width(width: f32, count: usize) -> f32 {
    width / count.max(1) as f32
}
pub fn candle_width(slot: f32) -> f32 {
    (slot * 0.62).max(1.0)
}
pub fn zoom_span(span: usize, ratio: f32, length: usize) -> usize {
    if length == 0 {
        return 0;
    }
    let current = span.clamp(1, length);
    if !ratio.is_finite() || ratio <= 0.0 {
        return current;
    }
    let scaled = (current as f32 * ratio).round() as usize;
    let next = if ratio < 1.0 {
        scaled.min(current.saturating_sub(1))
    } else if ratio > 1.0 {
        scaled.max(current.saturating_add(1))
    } else {
        current
    };
    next.clamp(1, length)
}
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
    fn zoom_is_monotonic_and_can_zoom_out_after_reaching_one_bar() {
        for n in [1, 2, 8, 120, 6000] {
            for span in 1..=n {
                let less = zoom_span(span, 0.8, n);
                let more = zoom_span(span, 1.25, n);
                assert!(less <= span && more >= span);
                if span > 1 {
                    assert!(less < span);
                }
                if span < n {
                    assert!(more > span);
                }
            }
        }
        assert_eq!(zoom_span(1, 1.01, 120), 2);
        assert_eq!(zoom_span(0, 1.25, 0), 0);
    }
    #[test]
    fn zoom_below_eighty_bars_expands_candles_across_the_plot() {
        let mut previous = 0.0;
        for count in [120, 80, 40, 20, 10, 5, 2, 1] {
            let slot = slot_width(960.0, count);
            let width = candle_width(slot);
            assert!(width > previous);
            previous = width;
            assert_eq!(hit(slot * 0.5, 0.0, slot, 0.0, count), Some(0));
            assert_eq!(
                hit(960.0 - slot * 0.5, 0.0, slot, 0.0, count),
                Some(count - 1)
            );
        }
    }
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
