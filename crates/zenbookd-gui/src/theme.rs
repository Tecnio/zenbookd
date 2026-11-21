use std::sync::Arc;

use gpui::{FontFeatures, Pixels, Rgba, px};

const RADIUS_CONTROL: f32 = 4.0;

const fn rgb8(red: u8, green: u8, blue: u8) -> Rgba {
    Rgba {
        r: red as f32 / 255.0,
        g: green as f32 / 255.0,
        b: blue as f32 / 255.0,
        a: 1.0,
    }
}

pub fn radius_control() -> Pixels {
    px(RADIUS_CONTROL)
}

pub fn canvas() -> Rgba {
    rgb8(0x14, 0x15, 0x16)
}

pub fn raised() -> Rgba {
    rgb8(0x24, 0x25, 0x27)
}

pub fn raised_hover() -> Rgba {
    rgb8(0x2e, 0x2f, 0x32)
}

pub fn inset() -> Rgba {
    rgb8(0x0e, 0x0e, 0x0f)
}

pub fn rule() -> Rgba {
    rgb8(0x26, 0x27, 0x29)
}

pub fn rule_strong() -> Rgba {
    rgb8(0x3a, 0x3b, 0x3e)
}

pub fn text() -> Rgba {
    rgb8(0xe4, 0xe4, 0xe2)
}

pub fn text_muted() -> Rgba {
    rgb8(0xa0, 0xa1, 0xa3)
}

pub fn text_faint() -> Rgba {
    rgb8(0x86, 0x87, 0x8a)
}

pub fn accent() -> Rgba {
    rgb8(0x56, 0xb6, 0xc2)
}

pub fn accent_hover() -> Rgba {
    rgb8(0x6f, 0xc4, 0xcf)
}

pub fn accent_ink() -> Rgba {
    rgb8(0x07, 0x1a, 0x1d)
}

pub fn good() -> Rgba {
    rgb8(0x7c, 0xbf, 0x86)
}

pub fn warning() -> Rgba {
    rgb8(0xd8, 0xa6, 0x48)
}

pub fn danger() -> Rgba {
    rgb8(0xe8, 0x6c, 0x64)
}

pub fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMUM_CONTRAST: f32 = 4.5;

    fn channel(value: f32) -> f32 {
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(color: Rgba) -> f32 {
        0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
    }

    fn contrast(foreground: Rgba, background: Rgba) -> f32 {
        let (first, second) = (luminance(foreground), luminance(background));

        let (light, dark) = if first > second {
            (first, second)
        } else {
            (second, first)
        };

        (light + 0.05) / (dark + 0.05)
    }

    fn assert_readable(name: &str, foreground: Rgba, backgrounds: &[(&str, Rgba)]) {
        for (background_name, background) in backgrounds {
            let ratio = contrast(foreground, *background);

            assert!(
                ratio >= MINIMUM_CONTRAST,
                "{name} on {background_name} is {ratio:.2}:1"
            );
        }
    }

    #[test]
    fn text_reads_at_four_and_a_half_to_one_where_it_is_used() {
        let surfaces = [
            ("canvas", canvas()),
            ("raised", raised()),
            ("raised_hover", raised_hover()),
        ];

        assert_readable("text", text(), &surfaces);
        assert_readable("text_muted", text_muted(), &surfaces);
        assert_readable("text_faint", text_faint(), &surfaces[..1]);
        assert_readable("accent", accent(), &surfaces[..1]);
        assert_readable("good", good(), &surfaces[..1]);
        assert_readable("warning", warning(), &surfaces[..1]);
        assert_readable("danger", danger(), &surfaces[..1]);
        assert_readable("accent_ink", accent_ink(), &[("accent", accent())]);
        assert_readable(
            "accent_ink",
            accent_ink(),
            &[("accent_hover", accent_hover())],
        );
    }
}
