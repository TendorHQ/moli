use std::cmp::Ordering;

use crate::css_resource_urls::{parse_font_stretch_lower_bound, parse_font_weight_lower_bound};
use moli_css_parse::{CssNumericContext, CssNumericKind, UnitlessAngle};

fn components(mut value: &str) -> Vec<&str> {
    let mut values = Vec::new();
    while !value.trim_start().is_empty() {
        value = value.trim_start();
        let word_end = value.find(char::is_whitespace).unwrap_or(value.len());
        let len = if value.find('(').is_some_and(|index| index < word_end) {
            moli_css_parse::balanced_function_len(value).unwrap_or(value.len())
        } else {
            word_end
        };
        values.push(&value[..len]);
        value = &value[len..];
    }
    values
}

#[derive(Clone, Copy, Debug)]
struct Range {
    min: f32,
    max: f32,
}

impl Range {
    fn new(first: f32, second: f32) -> Self {
        Self {
            min: first.min(second),
            max: first.max(second),
        }
    }

    fn parse(value: &str, parse: impl Fn(&str) -> Option<f32>, initial: f32) -> Self {
        let mut values = components(value).into_iter();
        let min = values.next().and_then(&parse).unwrap_or(initial);
        let max = values.next().and_then(parse).unwrap_or(min);
        Self::new(min, max)
    }

    fn contains(self, value: f32) -> bool {
        self.min <= value && value <= self.max
    }

    fn width_rank(self, requested: f32) -> (u8, f32) {
        if self.contains(requested) {
            (0, 0.0)
        } else if requested <= 100.0 {
            if self.max < requested {
                (1, requested - self.max)
            } else {
                (2, self.min - requested)
            }
        } else if self.min > requested {
            (1, self.min - requested)
        } else {
            (2, requested - self.max)
        }
    }

    fn weight_rank(self, requested: f32) -> (u8, f32) {
        if self.contains(requested) {
            return (0, 0.0);
        }
        if (400.0..=500.0).contains(&requested) {
            if self.min > requested && self.min <= 500.0 {
                (1, self.min - requested)
            } else if self.max < requested {
                (2, requested - self.max)
            } else {
                (3, self.min - 500.0)
            }
        } else if requested < 400.0 {
            if self.max < requested {
                (1, requested - self.max)
            } else {
                (2, self.min - requested)
            }
        } else if self.min > requested {
            (1, self.min - requested)
        } else {
            (2, requested - self.max)
        }
    }

    fn style_rank(self, requested: f32) -> (u8, f32) {
        if self.contains(requested) {
            return (0, 0.0);
        }
        // Match Blink's slope search: italic and the default oblique slope are
        // 14deg, with asymmetric fallback on either side of zero and +/-14deg.
        if requested >= 14.0 {
            if self.min > requested {
                (1, self.min - requested)
            } else {
                (2, requested - self.max)
            }
        } else if requested >= 0.0 {
            if self.max >= 0.0 && self.max < requested {
                (1, requested - self.max)
            } else if self.min > requested {
                (2, self.min)
            } else {
                (3, -self.max)
            }
        } else if requested > -14.0 {
            if self.min > requested && self.min <= 0.0 {
                (1, self.min - requested)
            } else if self.max < requested {
                (2, -self.max)
            } else {
                (3, self.min)
            }
        } else if self.max < requested {
            (1, requested - self.max)
        } else {
            (2, self.min - requested)
        }
    }
}

/// Font capabilities are selected before testing unicode-range, and do not
/// depend on whether the source has been loaded or contains a particular glyph.
#[derive(Clone, Copy, Debug)]
pub(super) struct Selection {
    width: Range,
    slope: Range,
    weight: Range,
}

impl Selection {
    pub(super) fn new(weight: &str, stretch: &str, style: &str) -> Self {
        let angles = components(style);
        let angle = |value: &str| {
            moli_css_parse::resolve_css_numeric(
                value,
                CssNumericKind::Angle(UnitlessAngle::ZeroOnly),
                CssNumericContext::default(),
            )
            .and_then(|value| value.angle_degrees())
            .map(|value| (value as f32).clamp(-90.0, 90.0))
        };
        let min = match angles.first().copied() {
            Some("italic") => 14.0,
            Some("oblique") => angles.get(1).and_then(|value| angle(value)).unwrap_or(14.0),
            _ => 0.0,
        };
        let max = angles.get(2).and_then(|value| angle(value)).unwrap_or(min);
        let weight_value = |value: &str| match value {
            // FontFaceSet resolves relative weights against initial normal.
            "bolder" => Some(700.0),
            "lighter" => Some(100.0),
            _ => moli_css_parse::resolve_css_number(value, None)
                .map(|value| (value as f32).clamp(1.0, 1000.0))
                .or_else(|| parse_font_weight_lower_bound(value)),
        };
        let width_value = |value: &str| {
            parse_font_stretch_lower_bound(value).or_else(|| {
                moli_css_parse::resolve_css_numeric(
                    value,
                    CssNumericKind::Percentage,
                    CssNumericContext::default(),
                )
                .and_then(|value| value.percentage())
                .map(|value| value as f32)
            })
        };
        Self {
            width: Range::parse(stretch, width_value, 100.0),
            slope: Range::new(min, max),
            weight: Range::parse(weight, weight_value, 400.0),
        }
    }

    pub(super) fn compare(self, other: Self, requested: Self) -> Ordering {
        let first = [
            self.width.width_rank(requested.width.min),
            self.slope.style_rank(requested.slope.min),
            self.weight.weight_rank(requested.weight.min),
        ];
        let second = [
            other.width.width_rank(requested.width.min),
            other.slope.style_rank(requested.slope.min),
            other.weight.weight_rank(requested.weight.min),
        ];
        for (first, second) in first.into_iter().zip(second) {
            let ordering = first
                .0
                .cmp(&second.0)
                .then_with(|| first.1.total_cmp(&second.1));
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_search_prefers_500_then_lower_weights_before_600_for_normal() {
        let face = |weight| Selection::new(weight, "normal", "normal");
        assert_eq!(
            face("500").compare(face("300"), face("400")),
            Ordering::Less
        );
        assert_eq!(
            face("300").compare(face("600"), face("400")),
            Ordering::Less
        );
        assert_eq!(
            face("400").compare(face("500"), face("450")),
            Ordering::Greater
        );
        assert_eq!(
            face("600").compare(face("400"), face("550")),
            Ordering::Less
        );
    }

    #[test]
    fn descriptor_ranges_and_width_then_style_then_weight_control_matching() {
        let query = Selection::new("700", "normal", "oblique 20deg");
        assert_eq!(
            Selection::new("100 900", "normal", "oblique 10deg 30deg")
                .compare(Selection::new("700", "normal", "normal"), query),
            Ordering::Less
        );
        assert_eq!(
            Selection::new("400", "normal", "normal")
                .compare(Selection::new("700", "condensed", "oblique 20deg"), query),
            Ordering::Less
        );
        let narrow = Selection::new("400", "75%", "normal");
        let wide = Selection::new("400", "125%", "normal");
        assert_eq!(
            narrow.compare(wide, Selection::new("400", "normal", "normal")),
            Ordering::Less
        );
        assert_eq!(
            narrow.compare(wide, Selection::new("400", "110%", "normal")),
            Ordering::Greater
        );
    }

    #[test]
    fn calculation_components_keep_whitespace_and_descriptor_range_boundaries() {
        let ranged = Selection::new(
            "calc(100 + 200) calc(800 - 100)",
            "calc(50% + 25%) calc(100% + 25%)",
            "oblique calc(5deg + 5deg) calc(15deg + 15deg)",
        );
        let requested = Selection::new("calc(400 + 100)", "normal", "oblique calc(10deg + 10deg)");
        assert_eq!(
            ranged.compare(Selection::new("400", "normal", "normal"), requested),
            Ordering::Less,
            "ranged={ranged:?}, requested={requested:?}"
        );
    }
}
