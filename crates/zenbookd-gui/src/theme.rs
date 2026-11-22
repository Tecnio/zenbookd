use std::sync::Arc;

use gpui::{FontFeatures, Pixels, Rgba, WindowAppearance, px};

const RADIUS_GROUP: f32 = 8.0;
const RADIUS_CONTROL: f32 = 5.0;
const RADIUS_SEGMENT: f32 = 3.5;

const fn rgb8(red: u8, green: u8, blue: u8) -> Rgba {
    Rgba {
        r: red as f32 / 255.0,
        g: green as f32 / 255.0,
        b: blue as f32 / 255.0,
        a: 1.0,
    }
}

pub fn radius_group() -> Pixels {
    px(RADIUS_GROUP)
}

pub fn radius_control() -> Pixels {
    px(RADIUS_CONTROL)
}

pub fn radius_segment() -> Pixels {
    px(RADIUS_SEGMENT)
}

pub fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub canvas: Rgba,
    pub surface: Rgba,
    pub control: Rgba,
    pub control_hover: Rgba,
    pub inset: Rgba,
    pub notice_bg: Rgba,
    pub border: Rgba,
    pub border_control: Rgba,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub text_faint: Rgba,
    pub accent: Rgba,
    pub accent_ink: Rgba,
    pub accent_text: Rgba,
    pub good: Rgba,
    pub warning: Rgba,
    pub danger: Rgba,
}

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }

    pub const fn dark() -> Self {
        Self {
            canvas: rgb8(0x1a, 0x1b, 0x1e),
            surface: rgb8(0x22, 0x24, 0x28),
            control: rgb8(0x2b, 0x2e, 0x33),
            control_hover: rgb8(0x34, 0x38, 0x3f),
            inset: rgb8(0x16, 0x17, 0x1a),
            notice_bg: rgb8(0x25, 0x27, 0x2c),
            border: rgb8(0x2f, 0x32, 0x39),
            border_control: rgb8(0x41, 0x45, 0x4e),
            text: rgb8(0xed, 0xef, 0xf2),
            text_muted: rgb8(0xa6, 0xab, 0xb5),
            text_faint: rgb8(0x93, 0x98, 0xa3),
            accent: rgb8(0x25, 0x63, 0xeb),
            accent_ink: rgb8(0xff, 0xff, 0xff),
            accent_text: rgb8(0x74, 0xab, 0xff),
            good: rgb8(0x5e, 0xbd, 0x76),
            warning: rgb8(0xdf, 0xa4, 0x3a),
            danger: rgb8(0xeb, 0x6b, 0x67),
        }
    }

    pub const fn light() -> Self {
        Self {
            canvas: rgb8(0xf3, 0xf4, 0xf6),
            surface: rgb8(0xff, 0xff, 0xff),
            control: rgb8(0xf7, 0xf8, 0xfa),
            control_hover: rgb8(0xec, 0xee, 0xf2),
            inset: rgb8(0xe8, 0xea, 0xef),
            notice_bg: rgb8(0xee, 0xf0, 0xf4),
            border: rgb8(0xdf, 0xe2, 0xe8),
            border_control: rgb8(0xc6, 0xcb, 0xd4),
            text: rgb8(0x18, 0x1a, 0x1f),
            text_muted: rgb8(0x4e, 0x54, 0x60),
            text_faint: rgb8(0x5c, 0x63, 0x70),
            accent: rgb8(0x25, 0x63, 0xeb),
            accent_ink: rgb8(0xff, 0xff, 0xff),
            accent_text: rgb8(0x1d, 0x4e, 0xd8),
            good: rgb8(0x1a, 0x7a, 0x40),
            warning: rgb8(0x94, 0x59, 0x00),
            danger: rgb8(0xc2, 0x27, 0x27),
        }
    }
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

    fn assert_readable(palette: &str, name: &str, foreground: Rgba, backgrounds: &[(&str, Rgba)]) {
        for (background_name, background) in backgrounds {
            let ratio = contrast(foreground, *background);

            assert!(
                ratio >= MINIMUM_CONTRAST,
                "{palette}: {name} on {background_name} is {ratio:.2}:1"
            );
        }
    }

    fn check_palette(palette: &str, theme: Theme) {
        let surfaces = [
            ("canvas", theme.canvas),
            ("surface", theme.surface),
            ("control", theme.control),
            ("control_hover", theme.control_hover),
            ("notice_bg", theme.notice_bg),
        ];

        let primary_surfaces = [
            ("canvas", theme.canvas),
            ("surface", theme.surface),
            ("notice_bg", theme.notice_bg),
        ];

        assert_readable(palette, "text", theme.text, &surfaces);
        assert_readable(palette, "text_muted", theme.text_muted, &surfaces);
        assert_readable(palette, "text_faint", theme.text_faint, &primary_surfaces);
        assert_readable(
            palette,
            "accent_text",
            theme.accent_text,
            &[("inset", theme.inset), ("surface", theme.surface)],
        );
        assert_readable(palette, "good", theme.good, &primary_surfaces);
        assert_readable(palette, "warning", theme.warning, &primary_surfaces);
        assert_readable(palette, "danger", theme.danger, &primary_surfaces);
        assert_readable(
            palette,
            "accent_ink",
            theme.accent_ink,
            &[("accent", theme.accent)],
        );
    }

    #[test]
    fn dark_and_light_palettes_meet_contrast_requirements() {
        check_palette("dark", Theme::dark());
        check_palette("light", Theme::light());
    }
}
