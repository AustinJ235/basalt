use crate::image::convert;
use crate::interface::bin::lerp;
use crate::ulps_eq;

/// Representation of color in the linear color space
///
/// Component values are normalized from `0.0..=1.0`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl PartialEq for Color {
    fn eq(&self, other: &Self) -> bool {
        ulps_eq(self.r, other.r, 4)
            && ulps_eq(self.g, other.g, 4)
            && ulps_eq(self.b, other.b, 4)
            && ulps_eq(self.a, other.a, 4)
    }
}

impl Color {
    /// `Color` from a hexadecimal string interpreted as linear color.
    ///
    /// Length Of:
    /// - `2` is a luma color.
    /// - `4` is a luma color with alpha.
    /// - `6` is a RGB color.
    /// - `8` is a RGB color with alpha.
    ///
    /// ***Note:** Upon a parsing error, transparent black with be returned. If this isn't desired
    /// use the `checked` varient of this method.*
    pub fn hex<H: AsRef<str>>(hex: H) -> Self {
        Self::hex_checked(hex).unwrap_or_default()
    }

    /// `Color` from a hexadecimal string interpreted as standard color.
    ///
    /// *See `hex` for more information.*
    pub fn shex<H: AsRef<str>>(hex: H) -> Self {
        Self::shex_checked(hex).unwrap_or_default()
    }

    /// `Color` from a hexadecimal string interpreted as linear color.
    ///
    /// *See `hex` for more information.*
    pub fn hex_checked<H: AsRef<str>>(hex: H) -> Option<Self> {
        let hex = hex.as_ref();

        match hex.len() {
            2 => {
                let l = convert::u8f32(u8::from_str_radix(hex, 16).ok()?);
                Some(Self::rgb(l, l, l))
            },
            4 => {
                let l = convert::u8f32(u8::from_str_radix(&hex[0..2], 16).ok()?);
                let a = convert::u8f32(u8::from_str_radix(&hex[2..4], 16).ok()?);
                Some(Self::rgba(l, l, l, a))
            },
            6 => {
                let r = convert::u8f32(u8::from_str_radix(&hex[0..2], 16).ok()?);
                let g = convert::u8f32(u8::from_str_radix(&hex[2..4], 16).ok()?);
                let b = convert::u8f32(u8::from_str_radix(&hex[4..6], 16).ok()?);
                Some(Self::rgb(r, g, b))
            },
            8 => {
                let r = convert::u8f32(u8::from_str_radix(&hex[0..2], 16).ok()?);
                let g = convert::u8f32(u8::from_str_radix(&hex[2..4], 16).ok()?);
                let b = convert::u8f32(u8::from_str_radix(&hex[4..6], 16).ok()?);
                let a = convert::u8f32(u8::from_str_radix(&hex[6..8], 16).ok()?);
                Some(Self::rgba(r, g, b, a))
            },
            _ => None,
        }
    }

    /// `Color` from a hexadecimal string interpreted as standard color.
    ///
    /// *See `hex` for more information.*
    pub fn shex_checked<H: AsRef<str>>(hex: H) -> Option<Self> {
        let mut color = Self::hex_checked(hex)?;
        color.r = convert::stl(color.r);
        color.g = convert::stl(color.g);
        color.b = convert::stl(color.b);
        color.a = convert::stl(color.a);
        Some(color)
    }

    /// `Color` from linear RGBF components.
    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: 1.0,
        }
    }

    /// `Color` from linear RGBAF components.
    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// `Color` from linear RGB8 components.
    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: convert::u8f32(r),
            g: convert::u8f32(g),
            b: convert::u8f32(b),
            a: 1.0,
        }
    }

    /// `Color` from linear RGBA8 components.
    pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: convert::u8f32(r),
            g: convert::u8f32(g),
            b: convert::u8f32(b),
            a: convert::u8f32(a),
        }
    }

    /// `Color` from linear RGB16 components.
    pub fn rgb16(r: u16, g: u16, b: u16) -> Self {
        Self {
            r: convert::u16f32(r),
            g: convert::u16f32(g),
            b: convert::u16f32(b),
            a: 1.0,
        }
    }

    /// `Color` from linear RGBA16 components.
    pub fn rgba16(r: u16, g: u16, b: u16, a: u16) -> Self {
        Self {
            r: convert::u16f32(r),
            g: convert::u16f32(g),
            b: convert::u16f32(b),
            a: convert::u16f32(a),
        }
    }

    /// `Color` from standard RGBF components.
    pub fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            r: convert::stl(r.clamp(0.0, 1.0)),
            g: convert::stl(g.clamp(0.0, 1.0)),
            b: convert::stl(b.clamp(0.0, 1.0)),
            a: 1.0,
        }
    }

    /// `Color` from standard RGBAF components.
    pub fn srgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: convert::stl(r.clamp(0.0, 1.0)),
            g: convert::stl(g.clamp(0.0, 1.0)),
            b: convert::stl(b.clamp(0.0, 1.0)),
            a: convert::stl(a.clamp(0.0, 1.0)),
        }
    }

    /// `Color` from standard RGB8 components.
    pub fn srgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: convert::stl(convert::u8f32(r)),
            g: convert::stl(convert::u8f32(g)),
            b: convert::stl(convert::u8f32(b)),
            a: 1.0,
        }
    }

    /// `Color` from standard RGBA8 components.
    pub fn srgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: convert::stl(convert::u8f32(r)),
            g: convert::stl(convert::u8f32(g)),
            b: convert::stl(convert::u8f32(b)),
            a: convert::stl(convert::u8f32(a)),
        }
    }

    /// `Color` from standard RGB16 components.
    pub fn srgb16(r: u16, g: u16, b: u16) -> Self {
        Self {
            r: convert::stl(convert::u16f32(r)),
            g: convert::stl(convert::u16f32(g)),
            b: convert::stl(convert::u16f32(b)),
            a: 1.0,
        }
    }

    /// `Color` from standard RGBA16 components.
    pub fn srgba16(r: u16, g: u16, b: u16, a: u16) -> Self {
        Self {
            r: convert::stl(convert::u16f32(r)),
            g: convert::stl(convert::u16f32(g)),
            b: convert::stl(convert::u16f32(b)),
            a: convert::stl(convert::u16f32(a)),
        }
    }

    /// `Color` from HSL values.
    ///
    /// - `h` is the hue of the color and ranges from `0.0` to `360.0`.
    /// - `s` is the saturation of the color and ranges from `0.0` to `100.0`.
    /// - `l` is the lightness of the color and ranges from `0.0` to `100.0`.
    ///
    /// ***Note:** Values outside of the their range will be clamped.*
    pub fn hsl(h: f32, s: f32, l: f32) -> Self {
        let [r, g, b] = Self::hsl_to_srgb(h, s, l);
        Self::srgb(r, g, b)
    }

    /// `Color` from HSL values with alpha.
    ///
    /// `a` is the alpha of the color and ranges from `0.0` to `1.0`.
    ///
    /// *See `Color::hsl` for more information.*
    pub fn hsla(h: f32, s: f32, l: f32, a: f32) -> Self {
        let [r, g, b] = Self::hsl_to_srgb(h, s, l);
        Self::srgba(r, g, b, a)
    }

    /// Blend this `Color` with another `Color`.
    ///
    /// `t` is a value in the range of `0.0..=1.0`.
    ///
    /// If `t` is `0.0` the output will be the same as `self`.
    /// If `t` is `1.0` the output will be the same as `other`.
    pub fn blend(&self, other: Self, mut t: f32) -> Self {
        t = t.clamp(0.0, 1.0);

        Self {
            r: lerp(t, self.r, other.r),
            g: lerp(t, self.g, other.g),
            b: lerp(t, self.b, other.b),
            a: lerp(t, self.a, other.a),
        }
    }

    fn hsl_to_srgb(mut h: f32, mut s: f32, mut l: f32) -> [f32; 3] {
        h = (h / 360.0).clamp(0.0, 1.0);
        s = (s / 100.0).clamp(0.0, 1.0);
        l = (l / 100.0).clamp(0.0, 1.0);

        if ulps_eq(s, 0.0, 4) {
            return [l; 3];
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            (l + s) - (l * s)
        };

        let p = (2.0 * l) - q;

        [
            Self::hue_to_rgb(p, q, h + (1.0 / 3.0)),
            Self::hue_to_rgb(p, q, h),
            Self::hue_to_rgb(p, q, h - (1.0 / 3.0)),
        ]
    }

    fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
        if t < 0.0 {
            t += 1.0;
        } else if t > 1.0 {
            t -= 1.0;
        }

        if t < 1.0 / 6.0 {
            p + ((q - p) * 6.0 * t)
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            (p + (q - p)) * ((2.0 / 3.0) - t) * 6.0
        } else {
            p
        }
    }

    /// Convert into an RGBF array.
    pub fn rgbf_array(self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }

    /// Convert into an RGBAF array.
    pub fn rgbaf_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Convert into an RGB8 array.
    pub fn rgb8_array(self) -> [u8; 3] {
        [
            convert::f32u8(self.r),
            convert::f32u8(self.g),
            convert::f32u8(self.b),
        ]
    }

    /// Convert into an RGBA8 array.
    pub fn rgba8_array(self) -> [u8; 4] {
        [
            convert::f32u8(self.r),
            convert::f32u8(self.g),
            convert::f32u8(self.b),
            convert::f32u8(self.a),
        ]
    }

    /// Convert into an RGB16 array.
    pub fn rgb16_array(self) -> [u16; 3] {
        [
            convert::f32u16(self.r),
            convert::f32u16(self.g),
            convert::f32u16(self.b),
        ]
    }

    /// Convert into an RGBA16 array.
    pub fn rgba16_array(self) -> [u16; 4] {
        [
            convert::f32u16(self.r),
            convert::f32u16(self.g),
            convert::f32u16(self.b),
            convert::f32u16(self.a),
        ]
    }

    /// Convert into a standard RGBF array.
    pub fn srgbf_array(self) -> [f32; 3] {
        [
            convert::lts(self.r),
            convert::lts(self.g),
            convert::lts(self.b),
        ]
    }

    /// Convert into a standard RGBAF array.
    pub fn srgbaf_array(self) -> [f32; 4] {
        [
            convert::lts(self.r),
            convert::lts(self.g),
            convert::lts(self.b),
            convert::lts(self.a),
        ]
    }

    /// Convert into a standard RGB8 array.
    pub fn srgb8_array(self) -> [u8; 3] {
        [
            convert::f32u8(convert::lts(self.r)),
            convert::f32u8(convert::lts(self.g)),
            convert::f32u8(convert::lts(self.b)),
        ]
    }

    /// Convert into a standard RGBA8 array.
    pub fn srgba8_array(self) -> [u8; 4] {
        [
            convert::f32u8(convert::lts(self.r)),
            convert::f32u8(convert::lts(self.g)),
            convert::f32u8(convert::lts(self.b)),
            convert::f32u8(convert::lts(self.a)),
        ]
    }

    /// Convert into a standard RGB16 array.
    pub fn srgb16_array(self) -> [u16; 3] {
        [
            convert::f32u16(convert::lts(self.r)),
            convert::f32u16(convert::lts(self.g)),
            convert::f32u16(convert::lts(self.b)),
        ]
    }

    /// Convert into a standard RGBA16 array.
    pub fn srgba16_array(self) -> [u16; 4] {
        [
            convert::f32u16(convert::lts(self.r)),
            convert::f32u16(convert::lts(self.g)),
            convert::f32u16(convert::lts(self.b)),
            convert::f32u16(convert::lts(self.a)),
        ]
    }
}

/// [Colors from SVG keywords](https://www.w3.org/TR/SVG11/types.html#ColorKeywords)
#[rustfmt::skip]
impl Color {
    pub fn alice_blue() -> Self { Self::srgb8(240, 248, 255) }
    pub fn antique_white() -> Self { Self::srgb8(250, 235, 215) }
    pub fn aqua() -> Self { Self::srgb8(0, 255, 255) }
    pub fn aquamarine() -> Self { Self::srgb8(127, 255, 212) }
    pub fn azure() -> Self { Self::srgb8(240, 255, 255) }
    pub fn beige() -> Self { Self::srgb8(245, 245, 220) }
    pub fn bisque() -> Self { Self::srgb8(255, 228, 196) }
    pub fn black() -> Self { Self::srgb8(0, 0, 0) }
    pub fn blanched_almond() -> Self { Self::srgb8(255, 235, 205) }
    pub fn blue() -> Self { Self::srgb8(0, 0, 255) }
    pub fn blue_violet() -> Self { Self::srgb8(138, 43, 226) }
    pub fn brown() -> Self { Self::srgb8(165, 42, 42) }
    pub fn burlywood() -> Self { Self::srgb8(222, 184, 135) }
    pub fn cadet_blue() -> Self { Self::srgb8(95, 158, 160) }
    pub fn chartreuse() -> Self { Self::srgb8(127, 255, 0) }
    pub fn chocolate() -> Self { Self::srgb8(210, 105, 30) }
    pub fn coral() -> Self { Self::srgb8(255, 127, 80) }
    pub fn cornflower_blue() -> Self { Self::srgb8(100, 149, 237) }
    pub fn cornsilk() -> Self { Self::srgb8(255, 248, 220) }
    pub fn crimson() -> Self { Self::srgb8(220, 20, 60) }
    pub fn cyan() -> Self { Self::srgb8(0, 255, 255) }
    pub fn dark_blue() -> Self { Self::srgb8(0, 0, 139) }
    pub fn dark_cyan() -> Self { Self::srgb8(0, 139, 139) }
    pub fn dark_goldenrod() -> Self { Self::srgb8(184, 134, 11) }
    pub fn dark_gray() -> Self { Self::srgb8(169, 169, 169) }
    pub fn dark_green() -> Self { Self::srgb8(0, 100, 0) }
    pub fn dark_grey() -> Self { Self::srgb8(169, 169, 169) }
    pub fn dark_khaki() -> Self { Self::srgb8(189, 183, 107) }
    pub fn dark_magenta() -> Self { Self::srgb8(139, 0, 139) }
    pub fn dark_olive_green() -> Self { Self::srgb8(85, 107, 47) }
    pub fn dark_orange() -> Self { Self::srgb8(255, 140, 0) }
    pub fn dark_orchid() -> Self { Self::srgb8(153, 50, 204) }
    pub fn dark_red() -> Self { Self::srgb8(139, 0, 0) }
    pub fn dark_salmon() -> Self { Self::srgb8(233, 150, 122) }
    pub fn dark_seagreen() -> Self { Self::srgb8(143, 188, 143) }
    pub fn dark_slate_blue() -> Self { Self::srgb8(72, 61, 139) }
    pub fn dark_slate_gray() -> Self { Self::srgb8(47, 79, 79) }
    pub fn dark_slate_grey() -> Self { Self::srgb8(47, 79, 79) }
    pub fn dark_turquoise() -> Self { Self::srgb8(0, 206, 209) }
    pub fn dark_violet() -> Self { Self::srgb8(148, 0, 211) }
    pub fn deep_pink() -> Self { Self::srgb8(255, 20, 147) }
    pub fn deep_sky_blue() -> Self { Self::srgb8(0, 191, 255) }
    pub fn dim_gray() -> Self { Self::srgb8(105, 105, 105) }
    pub fn dim_grey() -> Self { Self::srgb8(105, 105, 105) }
    pub fn dodger_blue() -> Self { Self::srgb8(30, 144, 255) }
    pub fn fire_brick() -> Self { Self::srgb8(178, 34, 34) }
    pub fn floral_white() -> Self { Self::srgb8(255, 250, 240) }
    pub fn forest_green() -> Self { Self::srgb8(34, 139, 34) }
    pub fn fuchsia() -> Self { Self::srgb8(255, 0, 255) }
    pub fn gainsboro() -> Self { Self::srgb8(220, 220, 220) }
    pub fn ghost_white() -> Self { Self::srgb8(248, 248, 255) }
    pub fn gold() -> Self { Self::srgb8(255, 215, 0) }
    pub fn goldenrod() -> Self { Self::srgb8(218, 165, 32) }
    pub fn gray() -> Self { Self::srgb8(128, 128, 128) }
    pub fn grey() -> Self { Self::srgb8(128, 128, 128) }
    pub fn green() -> Self { Self::srgb8(0, 128, 0) }
    pub fn green_yellow() -> Self { Self::srgb8(173, 255, 47) }
    pub fn honeydew() -> Self { Self::srgb8(240, 255, 240) }
    pub fn hot_pink() -> Self { Self::srgb8(255, 105, 180) }
    pub fn indian_red() -> Self { Self::srgb8(205, 92, 92) }
    pub fn indigo() -> Self { Self::srgb8(75, 0, 130) }
    pub fn ivory() -> Self { Self::srgb8(255, 255, 240) }
    pub fn khaki() -> Self { Self::srgb8(240, 230, 140) }
    pub fn lavender() -> Self { Self::srgb8(230, 230, 250) }
    pub fn lavender_blush() -> Self { Self::srgb8(255, 240, 245) }
    pub fn lawn_green() -> Self { Self::srgb8(124, 252, 0) }
    pub fn lemon_chiffon() -> Self { Self::srgb8(255, 250, 205) }
    pub fn light_blue() -> Self { Self::srgb8(173, 216, 230) }
    pub fn light_coral() -> Self { Self::srgb8(240, 128, 128) }
    pub fn light_cyan() -> Self { Self::srgb8(224, 255, 255) }
    pub fn light_goldenrod_yellow() -> Self { Self::srgb8(250, 250, 210) }
    pub fn light_gray() -> Self { Self::srgb8(211, 211, 211) }
    pub fn light_green() -> Self { Self::srgb8(144, 238, 144) }
    pub fn light_grey() -> Self { Self::srgb8(211, 211, 211) }
    pub fn light_pink() -> Self { Self::srgb8(255, 182, 193) }
    pub fn light_salmon() -> Self { Self::srgb8(255, 160, 122) }
    pub fn light_seagreen() -> Self { Self::srgb8(32, 178, 170) }
    pub fn light_skyblue() -> Self { Self::srgb8(135, 206, 250) }
    pub fn light_slate_gray() -> Self { Self::srgb8(119, 136, 153) }
    pub fn light_slate_grey() -> Self { Self::srgb8(119, 136, 153) }
    pub fn light_steel_blue() -> Self { Self::srgb8(176, 196, 222) }
    pub fn light_yellow() -> Self { Self::srgb8(255, 255, 224) }
    pub fn lime() -> Self { Self::srgb8(0, 255, 0) }
    pub fn lime_green() -> Self { Self::srgb8(50, 205, 50) }
    pub fn linen() -> Self { Self::srgb8(250, 240, 230) }
    pub fn magenta() -> Self { Self::srgb8(255, 0, 255) }
    pub fn maroon() -> Self { Self::srgb8(128, 0, 0) }
    pub fn medium_aquamarine() -> Self { Self::srgb8(102, 205, 170) }
    pub fn medium_blue() -> Self { Self::srgb8(0, 0, 205) }
    pub fn medium_orchid() -> Self { Self::srgb8(186, 85, 211) }
    pub fn medium_purple() -> Self { Self::srgb8(147, 112, 219) }
    pub fn medium_sea_green() -> Self { Self::srgb8(60, 179, 113) }
    pub fn mediums_late_blue() -> Self { Self::srgb8(123, 104, 238) }
    pub fn medium_spring_green() -> Self { Self::srgb8(0, 250, 154) }
    pub fn medium_turquoise() -> Self { Self::srgb8(72, 209, 204) }
    pub fn medium_violet_red() -> Self { Self::srgb8(199, 21, 133) }
    pub fn midnight_blue() -> Self { Self::srgb8(25, 25, 112) }
    pub fn mint_cream() -> Self { Self::srgb8(245, 255, 250) }
    pub fn misty_rose() -> Self { Self::srgb8(255, 228, 225) }
    pub fn moccasin() -> Self { Self::srgb8(255, 228, 181) }
    pub fn navajo_white() -> Self { Self::srgb8(255, 222, 173) }
    pub fn navy() -> Self { Self::srgb8(0, 0, 128) }
    pub fn old_lace() -> Self { Self::srgb8(253, 245, 230) }
    pub fn olive() -> Self { Self::srgb8(128, 128, 0) }
    pub fn olive_drab() -> Self { Self::srgb8(107, 142, 35) }
    pub fn orange() -> Self { Self::srgb8(255, 165, 0) }
    pub fn orange_red() -> Self { Self::srgb8(255, 69, 0) }
    pub fn orchid() -> Self { Self::srgb8(218, 112, 214) }
    pub fn pale_goldenrod() -> Self { Self::srgb8(238, 232, 170) }
    pub fn pale_green() -> Self { Self::srgb8(152, 251, 152) }
    pub fn pale_turquoise() -> Self { Self::srgb8(175, 238, 238) }
    pub fn pale_violet_red() -> Self { Self::srgb8(219, 112, 147) }
    pub fn papaya_whip() -> Self { Self::srgb8(255, 239, 213) }
    pub fn peach_puff() -> Self { Self::srgb8(255, 218, 185) }
    pub fn peru() -> Self { Self::srgb8(205, 133, 63) }
    pub fn pink() -> Self { Self::srgb8(255, 192, 203) }
    pub fn plum() -> Self { Self::srgb8(221, 160, 221) }
    pub fn powder_blue() -> Self { Self::srgb8(176, 224, 230) }
    pub fn purple() -> Self { Self::srgb8(128, 0, 128) }
    pub fn red() -> Self { Self::srgb8(255, 0, 0) }
    pub fn rosy_brown() -> Self { Self::srgb8(188, 143, 143) }
    pub fn royal_blue() -> Self { Self::srgb8(65, 105, 225) }
    pub fn saddle_brown() -> Self { Self::srgb8(139, 69, 19) }
    pub fn salmon() -> Self { Self::srgb8(250, 128, 114) }
    pub fn sandy_brown() -> Self { Self::srgb8(244, 164, 96) }
    pub fn sea_green() -> Self { Self::srgb8(46, 139, 87) }
    pub fn sea_shell() -> Self { Self::srgb8(255, 245, 238) }
    pub fn sienna() -> Self { Self::srgb8(160, 82, 45) }
    pub fn silver() -> Self { Self::srgb8(192, 192, 192) }
    pub fn sky_blue() -> Self { Self::srgb8(135, 206, 235) }
    pub fn slate_blue() -> Self { Self::srgb8(106, 90, 205) }
    pub fn slate_gray() -> Self { Self::srgb8(112, 128, 144) }
    pub fn slate_grey() -> Self { Self::srgb8(112, 128, 144) }
    pub fn snow() -> Self { Self::srgb8(255, 250, 250) }
    pub fn spring_green() -> Self { Self::srgb8(0, 255, 127) }
    pub fn steel_blue() -> Self { Self::srgb8(70, 130, 180) }
    pub fn tan() -> Self { Self::srgb8(210, 180, 140) }
    pub fn teal() -> Self { Self::srgb8(0, 128, 128) }
    pub fn thistle() -> Self { Self::srgb8(216, 191, 216) }
    pub fn tomato() -> Self { Self::srgb8(255, 99, 71) }
    pub fn turquoise() -> Self { Self::srgb8(64, 224, 208) }
    pub fn violet() -> Self { Self::srgb8(238, 130, 238) }
    pub fn wheat() -> Self { Self::srgb8(245, 222, 179) }
    pub fn white() -> Self { Self::srgb8(255, 255, 255) }
    pub fn white_smoke() -> Self { Self::srgb8(245, 245, 245) }
    pub fn yellow() -> Self { Self::srgb8(255, 255, 0) }
    pub fn yellow_green() -> Self { Self::srgb8(154, 205, 50) }
}
