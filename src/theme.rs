//! 有限语义色板；颜色均为 encoded sRGB / straight alpha，不含字体或度量变量。
use crate::{Signal, srgb8, ui::Color};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

/// UI 线程控制器；在事件/消息回调中 set，框架自动提交并重绘，不重建节点。
#[derive(Clone)]
pub struct ThemeController(pub(crate) Signal<ThemeMode>);

impl Default for ThemeController {
    fn default() -> Self {
        Self::new(ThemeMode::Light)
    }
}

impl ThemeController {
    pub fn signal(&self) -> &Signal<ThemeMode> {
        &self.0
    }
    pub fn new(mode: ThemeMode) -> Self {
        Self(Signal::new(mode))
    }

    pub fn mode(&self) -> ThemeMode {
        self.0.get()
    }

    pub fn set(&self, mode: ThemeMode) {
        self.0.update(|current| *current = mode);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeColor {
    Background,
    Surface,
    SurfaceRaised,
    Text,
    Muted,
    Border,
    Primary,
    OnPrimary,
    Accent,
    OnAccent,
    Focus,
    Error,
    Success,
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SemanticTheme {
    pub mode: ThemeMode,
}

impl SemanticTheme {
    pub fn new(mode: ThemeMode) -> Self {
        Self { mode }
    }

    pub fn color(self, role: ThemeColor) -> Color {
        use ThemeColor::*;
        let rgb = match (self.mode, role) {
            (ThemeMode::Light, Background) => (247, 250, 252),
            (ThemeMode::Light, Surface) => (255, 255, 255),
            (ThemeMode::Light, SurfaceRaised) => (237, 245, 248),
            (ThemeMode::Light, Text) => (24, 43, 54),
            (ThemeMode::Light, Muted) => (76, 98, 109),
            (ThemeMode::Light, Border) => (203, 216, 222),
            (ThemeMode::Light, Primary) => (185, 227, 240),
            (ThemeMode::Light, OnPrimary) => (23, 53, 65),
            (ThemeMode::Light, Accent) => (244, 211, 94),
            (ThemeMode::Light, OnAccent) => (52, 43, 13),
            (ThemeMode::Light, Focus) => (0, 115, 138),
            (ThemeMode::Light, Error) => (180, 35, 24),
            (ThemeMode::Light, Success) => (36, 115, 66),
            (ThemeMode::Light, Selection) => (217, 239, 247),
            (ThemeMode::Dark, Background) => (17, 19, 24),
            (ThemeMode::Dark, Surface) => (25, 29, 35),
            (ThemeMode::Dark, SurfaceRaised) => (36, 42, 50),
            (ThemeMode::Dark, Text) => (242, 244, 247),
            (ThemeMode::Dark, Muted) => (173, 183, 196),
            (ThemeMode::Dark, Border) => (58, 68, 79),
            (ThemeMode::Dark, Primary) => (205, 65, 69),
            (ThemeMode::Dark, OnPrimary) => (255, 255, 255),
            (ThemeMode::Dark, Accent) => (232, 198, 83),
            (ThemeMode::Dark, OnAccent) => (33, 27, 9),
            (ThemeMode::Dark, Focus) => (240, 205, 90),
            (ThemeMode::Dark, Error) => (255, 156, 152),
            (ThemeMode::Dark, Success) => (138, 210, 160),
            (ThemeMode::Dark, Selection) => (74, 40, 46),
        };
        srgb8(rgb.0, rgb.1, rgb.2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlStyle {
    Primary,
    Secondary,
    Checkbox,
    Input,
    Tree,
}
