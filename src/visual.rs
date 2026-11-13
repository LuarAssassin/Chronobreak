//! 有限视觉原语；颜色为归一化 sRGB、straight alpha，长度均为逻辑像素。

use crate::ui::{Bounds, Color};

/// 从不透明 sRGB 字节构造颜色，不作预乘。
pub fn srgb8(red: u8, green: u8, blue: u8) -> Color {
    srgba8(red, green, blue, 255)
}

/// 从 sRGB 字节与线性 alpha 字节构造颜色，不作预乘。
pub fn srgba8(red: u8, green: u8, blue: u8, alpha: u8) -> Color {
    [red, green, blue, alpha].map(|channel| channel as f32 / 255.0)
}

/// sRGB 通道解码为线性光；输入必须有限，超出 [0, 1] 时钳位。
pub fn srgb_to_linear(channel: f32) -> f32 {
    let channel = unit(channel);
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// 线性光通道编码为 sRGB；输入必须有限，超出 [0, 1] 时钳位。
pub fn linear_to_srgb(channel: f32) -> f32 {
    let channel = unit(channel);
    if channel <= 0.0031308 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn unit(value: f32) -> f32 {
    assert!(value.is_finite(), "color channels must be finite");
    value.clamp(0.0, 1.0)
}

pub(crate) fn color(value: Color) -> Color {
    value.map(unit)
}

pub(crate) fn linear(value: Color) -> Color {
    let [red, green, blue, alpha] = color(value);
    [
        srgb_to_linear(red),
        srgb_to_linear(green),
        srgb_to_linear(blue),
        alpha,
    ]
}

fn finite(value: f32) -> f32 {
    assert!(value.is_finite(), "visual lengths must be finite");
    value
}

pub(crate) fn length(value: f32) -> f32 {
    finite(value).clamp(0.0, 4096.0)
}

/// 内描边，不参与布局/命中；宽度钳位到 [0, 4096]，绘制时再限制到短边一半。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub(crate) width: f32,
    pub(crate) color: Color,
}

impl Border {
    pub fn new(width: f32, value: Color) -> Self {
        Self {
            width: length(width),
            color: color(value),
        }
    }
}

/// 有限距离场阴影，不是 Gaussian/CSS blur。轮廓外 ±blur 内 smoothstep 衰减。
/// blur 钳位到 [0, 64]；offset 每轴 [-256, 256]；spread [-64, 64]。
/// 非有限输入拒绝；只扩大视觉范围，不改变布局、命中或子节点矩形裁剪。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub(crate) color: Color,
    pub(crate) blur: f32,
    pub(crate) offset: [f32; 2],
    pub(crate) spread: f32,
}

impl Shadow {
    pub fn new(value: Color, blur: f32) -> Self {
        Self {
            color: color(value),
            blur: finite(blur).clamp(0.0, 64.0),
            offset: [0.0; 2],
            spread: 0.0,
        }
    }

    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = [
            finite(x).clamp(-256.0, 256.0),
            finite(y).clamp(-256.0, 256.0),
        ];
        self
    }

    pub fn spread(mut self, spread: f32) -> Self {
        self.spread = finite(spread).clamp(-64.0, 64.0);
        self
    }

    pub(crate) fn shape(self, bounds: Bounds) -> Bounds {
        Bounds {
            x: bounds.x + self.offset[0] - self.spread,
            y: bounds.y + self.offset[1] - self.spread,
            width: (bounds.width + 2.0 * self.spread).max(0.0),
            height: (bounds.height + 2.0 * self.spread).max(0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Decoration {
    pub radius: f32,
    pub border: Option<Border>,
    pub shadow: Option<Shadow>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Segment {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub width: f32,
    pub color: Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trip_all_bytes_and_transfer_breakpoints() {
        for byte in 0..=255 {
            let channel = byte as f32 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(channel)) - channel).abs() < 0.000002);
        }
        assert!((srgb_to_linear(0.04045) - 0.003130805).abs() < 0.0000001);
        assert!((srgb_to_linear(0.5) - 0.21404114).abs() < 0.0000001);
        assert_eq!(linear([0.0, 1.0, 0.5, 0.5])[3], 0.5);
        assert_eq!(srgb8(0, 255, 0), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn finite_style_limits_and_color_clamps_are_explicit() {
        assert_eq!(color([-2.0, 2.0, 0.5, 1.5]), [0.0, 1.0, 0.5, 1.0]);
        assert_eq!(Border::new(-2.0, [1.0; 4]).width, 0.0);
        assert_eq!(Border::new(9000.0, [1.0; 4]).width, 4096.0);
        let shadow = Shadow::new([1.0; 4], 100.0)
            .offset(-900.0, 900.0)
            .spread(-90.0);
        assert_eq!(shadow.blur, 64.0);
        assert_eq!(shadow.offset, [-256.0, 256.0]);
        assert_eq!(shadow.spread, -64.0);
        let bounds = shadow.shape(Bounds {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 20.0,
        });
        assert_eq!((bounds.width, bounds.height), (0.0, 0.0));
    }

    #[test]
    fn nonfinite_styles_and_colors_are_rejected() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(std::panic::catch_unwind(|| Border::new(value, [0.0; 4])).is_err());
            assert!(std::panic::catch_unwind(|| Shadow::new([0.0; 4], value)).is_err());
            assert!(
                std::panic::catch_unwind(|| Shadow::new([0.0; 4], 1.0).offset(value, 0.0)).is_err()
            );
            assert!(std::panic::catch_unwind(|| Shadow::new([0.0; 4], 1.0).spread(value)).is_err());
            assert!(std::panic::catch_unwind(|| srgb_to_linear(value)).is_err());
            assert!(std::panic::catch_unwind(|| color([0.0, 0.0, 0.0, value])).is_err());
        }
    }

    #[test]
    fn alpha_overlap_is_linear_not_encoded_rgb() {
        let encoded = linear_to_srgb(srgb_to_linear(1.0) * 0.5);
        assert!((encoded - 0.735357).abs() < 0.000002);
        assert!((encoded - 0.5).abs() > 0.2);
    }
}
