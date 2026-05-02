#![doc = "Small color model used by the presentation layer."]

/// RGBA color represented as normalized floating-point channels.
///
/// Each channel is expected to be in the `0.0..=1.0` range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red channel.
    pub r: f32,

    /// Green channel.
    pub g: f32,

    /// Blue channel.
    pub b: f32,

    /// Alpha channel.
    pub a: f32,
}

impl Color {
    /// Creates a new normalized RGBA color.
    #[must_use]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Returns opaque white.
    #[must_use]
    pub const fn white() -> Self {
        Self::rgba(1.0, 1.0, 1.0, 1.0)
    }

    /// Returns opaque black.
    #[must_use]
    pub const fn black() -> Self {
        Self::rgba(0.0, 0.0, 0.0, 1.0)
    }

    /// Parses a minimal CSS hex color.
    ///
    /// Supported formats:
    ///
    /// - `#rgb`
    /// - `#rrggbb`
    ///
    /// Unsupported formats deliberately return `None`.
    #[must_use]
    pub fn from_css_hex(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        let hex = trimmed.strip_prefix('#')?;
        let bytes = hex.as_bytes();

        match bytes.len() {
            3 => {
                let red = hex_nibble(bytes[0])?;
                let green = hex_nibble(bytes[1])?;
                let blue = hex_nibble(bytes[2])?;

                Some(Self::from_rgb_u8(
                    red.saturating_mul(17),
                    green.saturating_mul(17),
                    blue.saturating_mul(17),
                ))
            }
            6 => {
                let red = hex_pair(bytes[0], bytes[1])?;
                let green = hex_pair(bytes[2], bytes[3])?;
                let blue = hex_pair(bytes[4], bytes[5])?;

                Some(Self::from_rgb_u8(red, green, blue))
            }
            _ => None,
        }
    }

    /// Converts the color to the `[f32; 4]` format consumed by the app renderer.
    #[must_use]
    pub const fn to_wgpu_clear(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Returns a simple perceived luminance value.
    #[must_use]
    pub fn luminance(self) -> f32 {
        fn linear(channel: f32) -> f32 {
            if channel <= 0.039_28 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        }

        0.0722_f32.mul_add(
            linear(self.b),
            0.2126_f32.mul_add(linear(self.r), 0.7152 * linear(self.g)),
        )
    }

    /// Returns a high-contrast text color for this background.
    #[must_use]
    pub fn readable_foreground(self) -> Self {
        if self.luminance() > 0.45 {
            Self::rgba(0.08, 0.09, 0.11, 1.0)
        } else {
            Self::rgba(0.92, 0.94, 0.98, 1.0)
        }
    }

    #[must_use]
    fn from_rgb_u8(red: u8, green: u8, blue: u8) -> Self {
        Self {
            r: f32::from(red) / 255.0,
            g: f32::from(green) / 255.0,
            b: f32::from(blue) / 255.0,
            a: 1.0,
        }
    }
}

#[must_use]
const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[must_use]
fn hex_pair(high: u8, low: u8) -> Option<u8> {
    let high_value = hex_nibble(high)?;
    let low_value = hex_nibble(low)?;
    Some(high_value.saturating_mul(16).saturating_add(low_value))
}
