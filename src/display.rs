//! ILI9488 480×320 TFT 驱动与 UI 绘制。
//!
//! ## 模块结构
//! - [`Ili9488Display`]：SPI 命令/像素写入，实现 `DrawTarget` 供 embedded-graphics 使用
//! - [`Face`] / [`FaceType`]：8×8 位图表情，2 帧动画
//! - `draw_*`：时钟、天气面板、详情页、配网页等 UI 组件
//!
//! ## Rust 要点
//! - `impl DrawTarget for Ili9488Display`：接入 embedded-graphics 生态（Text、Line 等）
//! - `&'static [[u8;8];2]`：表情像素表放 flash，不占 RAM
//! - `blocking_write`：显示刷新在主循环同步进行，与 async 传感器读取分离

use crate::config;
use crate::sensors::DateTime;
use crate::state::{DisplayPage, NetworkWeather};
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Blocking as SpiBlocking, Spi};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, ascii::FONT_10X20, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::Text,
};

use crate::state::PressureTrend;

pub const BG_COLOR: Rgb565 = Rgb565::new(0, 0, 17);

pub struct Theme {
    pub bg: Rgb565,
    pub title: Rgb565,
    pub text: Rgb565,
    pub dim: Rgb565,
    pub accent: Rgb565,
    pub is_night: bool,
}

/// 根据当前小时返回日/夜配色主题。
pub fn theme_for_hour(hour: u8) -> Theme {
    if hour >= config::NIGHT_START_HOUR || hour < config::NIGHT_END_HOUR {
        Theme {
            bg: Rgb565::new(0, 0, 8),
            title: Rgb565::new(160, 160, 0),
            text: Rgb565::new(170, 170, 190),
            dim: Rgb565::new(90, 90, 110),
            accent: Rgb565::new(110, 110, 130),
            is_night: true,
        }
    } else {
        Theme {
            bg: BG_COLOR,
            title: Rgb565::YELLOW,
            text: Rgb565::WHITE,
            dim: Rgb565::new(200, 200, 200),
            accent: Rgb565::new(150, 150, 150),
            is_night: false,
        }
    }
}

/// ILI9488 显示驱动（SPI + DC/CS 引脚）。
pub struct Ili9488Display {
    spi: Spi<'static, SPI1, SpiBlocking>,
    dc: Output<'static>,
    cs: Output<'static>,
    width: u16,
    height: u16,
}

impl Ili9488Display {
    pub fn new(
        spi: Spi<'static, SPI1, SpiBlocking>,
        dc: Output<'static>,
        cs: Output<'static>,
    ) -> Self {
        Self {
            spi,
            dc,
            cs,
            width: 480,
            height: 320,
        }
    }

    fn write_cmd(&mut self, cmd: u8) {
        self.dc.set_low();
        self.cs.set_low();
        self.spi.blocking_write(&[cmd]).unwrap();
        self.cs.set_high();
    }

    fn write_data(&mut self, data: &[u8]) {
        self.dc.set_high();
        self.cs.set_low();
        self.spi.blocking_write(data).unwrap();
        self.cs.set_high();
    }

    fn write_u16(&mut self, data: &[u16]) {
        self.dc.set_high();
        self.cs.set_low();
        for &v in data {
            let b = v.to_be_bytes();
            self.spi.blocking_write(&b).unwrap();
        }
        self.cs.set_high();
    }

    pub fn init(&mut self) {
        self.write_cmd(0x11);
        self.delay_ms(120);
        self.write_cmd(0x3A);
        self.write_data(&[0x55]);
        self.write_cmd(0x36);
        self.write_data(&[0x28]);
        self.write_cmd(0xB6);
        self.write_data(&[0x02, 0x02, 0x3B]);
        self.write_cmd(0x29);
        self.delay_ms(50);
    }

    fn delay_ms(&self, ms: u32) {
        for _ in 0..ms * 1000 {
            cortex_m::asm::nop();
        }
    }

    fn set_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) {
        self.write_cmd(0x2A);
        self.write_data(&[
            (x0 >> 8) as u8,
            (x0 & 0xFF) as u8,
            (x1 >> 8) as u8,
            (x1 & 0xFF) as u8,
        ]);
        self.write_cmd(0x2B);
        self.write_data(&[
            (y0 >> 8) as u8,
            (y0 & 0xFF) as u8,
            (y1 >> 8) as u8,
            (y1 & 0xFF) as u8,
        ]);
        self.write_cmd(0x2C);
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        self.set_window(x, y, x + w - 1, y + h - 1);
        self.dc.set_high();
        self.cs.set_low();
        let buf = [color.to_be_bytes()[0], color.to_be_bytes()[1]];
        let total = w as u32 * h as u32;
        for _ in 0..total {
            self.spi.blocking_write(&buf).unwrap();
        }
        self.cs.set_high();
    }

    pub fn clear(&mut self, color: Rgb565) {
        let c = color.into_storage();
        self.fill_rect(0, 0, self.width, self.height, c);
    }
}

impl DrawTarget for Ili9488Display {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x >= 0
                && point.y >= 0
                && (point.x as u16) < self.width
                && (point.y as u16) < self.height
            {
                let c: u16 = color.into_storage();
                self.set_window(
                    point.x as u16,
                    point.y as u16,
                    point.x as u16,
                    point.y as u16,
                );
                self.write_u16(&[c]);
            }
        }
        Ok(())
    }
}

impl OriginDimensions for Ili9488Display {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

// --- 8×8 位图表情（每表情 2 帧） ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaceType {
    Happy,
    VeryHappy,
    Sad,
    Cold,
    Hot,
    Sleepy,
    Surprised,
    Angry,
    Sunny,
    Rainy,
    Snowy,
    Heart,
    Star,
    Loading,
}

pub struct Face {
    frames: &'static [[u8; 8]; 2],
    cur: usize,
}

impl Face {
    pub fn new(ft: FaceType) -> Self {
        let f: &'static [[u8; 8]; 2] = match ft {
            FaceType::Happy => &HAPPY,
            FaceType::VeryHappy => &VERY_HAPPY,
            FaceType::Sad => &SAD,
            FaceType::Cold => &COLD,
            FaceType::Hot => &HOT,
            FaceType::Sleepy => &SLEEPY,
            FaceType::Surprised => &SURPRISED,
            FaceType::Angry => &ANGRY,
            FaceType::Sunny => &SUNNY,
            FaceType::Rainy => &RAINY,
            FaceType::Snowy => &SNOWY,
            FaceType::Heart => &HEART,
            FaceType::Star => &STAR,
            FaceType::Loading => &LOADING,
        };
        Self { frames: f, cur: 0 }
    }

    pub fn next_frame(&mut self) {
        self.cur = (self.cur + 1) % 2;
    }

    pub fn draw_scaled(&self, d: &mut Ili9488Display, x: i32, y: i32, s: i32, color: Rgb565) {
        let frame = &self.frames[self.cur];
        for (ri, row) in frame.iter().enumerate() {
            for ci in 0..8u8 {
                if (row >> (7 - ci)) & 1 == 1 {
                    Rectangle::new(
                        Point::new(x + ci as i32 * s, y + ri as i32 * s),
                        Size::new(s as u32, s as u32),
                    )
                    .into_styled(PrimitiveStyle::with_fill(color))
                    .draw(d)
                    .ok();
                }
            }
        }
    }
}

const HAPPY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0xA5, 0x99, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0xBD, 0x81, 0x42, 0x3C],
];
const VERY_HAPPY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0xA5, 0xBD, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0xBD, 0x81, 0x42, 0x3C],
];
const SAD: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0xA5, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0xBD, 0x81, 0x42, 0x3C],
];
const COLD: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0x81, 0x42, 0x3C],
    [0x3C, 0x42, 0xA5, 0x81, 0x99, 0x81, 0x42, 0x3C],
];
const HOT: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0xA5, 0x99, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0xBD, 0x81, 0x42, 0x3C],
];
const SLEEPY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0x81, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0xBD, 0xBD, 0x42, 0x3C],
];
const SURPRISED: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0xA5, 0x81, 0x81, 0x99, 0x42, 0x3C],
    [0x3C, 0x42, 0xA5, 0x81, 0x81, 0xBD, 0x42, 0x3C],
];
const ANGRY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0xA5, 0x81, 0x99, 0xA5, 0x42, 0x3C],
    [0x3C, 0x42, 0xA5, 0x81, 0xBD, 0x81, 0x42, 0x3C],
];
const SUNNY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0xA5, 0x99, 0x42, 0x3C],
    [0x3C, 0x42, 0xA5, 0x81, 0xA5, 0x99, 0x42, 0x3C],
];
const RAINY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0xA5, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0xA5, 0x54, 0x2A],
];
const SNOWY: [[u8; 8]; 2] = [
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0xA5, 0x42, 0x3C],
    [0x3C, 0x42, 0x91, 0x81, 0x99, 0xA5, 0x54, 0xAA],
];
const HEART: [[u8; 8]; 2] = [
    [0x00, 0x66, 0xFF, 0xFF, 0x7E, 0x3C, 0x18, 0x00],
    [0x00, 0x66, 0xFF, 0xFF, 0xFF, 0x7E, 0x3C, 0x18],
];
const STAR: [[u8; 8]; 2] = [
    [0x10, 0x38, 0x7C, 0xFE, 0x7C, 0x38, 0x10, 0x00],
    [0x10, 0x38, 0xFE, 0xFE, 0x38, 0x38, 0x10, 0x00],
];
const LOADING: [[u8; 8]; 2] = [
    [0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00],
    [0x04, 0x04, 0x04, 0x18, 0x18, 0x20, 0x20, 0x20],
];

/// 根据温湿度/天气码/气压趋势选择右下角表情。
pub fn select_face(
    temp: f32,
    code: &str,
    hour: u8,
    trend: PressureTrend,
    press: f32,
    filled: bool,
) -> FaceType {
    match code {
        "trend_rain" => return FaceType::Sad,
        "trend_sun" => return FaceType::Sunny,
        _ => {}
    }
    if filled {
        match trend {
            PressureTrend::Falling if press < 1000.0 => return FaceType::Sad,
            PressureTrend::Rising if press > 1013.0 => return FaceType::Sunny,
            _ => {}
        }
    }
    let wf = face_by_weather(code);
    match code {
        "d04" | "d05" | "d06" | "d07" | "d08" | "d09" | "d10" | "d11" | "d12" | "d13"
        | "d14" | "d15" | "d16" | "d17" | "d19" => wf,
        _ => {
            if temp < 10.0 || temp > 35.0 {
                face_by_temp(temp)
            } else {
                face_by_time(hour)
            }
        }
    }
}

fn face_by_temp(t: f32) -> FaceType {
    if t < 10.0 {
        FaceType::Cold
    } else if t < 18.0 {
        FaceType::Sad
    } else if t < 25.0 {
        FaceType::Happy
    } else if t < 30.0 {
        FaceType::VeryHappy
    } else if t < 35.0 {
        FaceType::Hot
    } else {
        FaceType::Angry
    }
}

fn face_by_weather(c: &str) -> FaceType {
    match c {
        "d01" | "n01" => FaceType::Sunny,
        "d02" | "n02" | "d03" => FaceType::Happy,
        "d04" | "d05" | "d07" | "d08" | "d09" | "d10" | "d11" | "d12" => FaceType::Rainy,
        "d13" | "d14" | "d15" | "d16" | "d17" => FaceType::Snowy,
        _ => FaceType::Happy,
    }
}

fn face_by_time(h: u8) -> FaceType {
    match h {
        6..=8 | 21..=23 | 0..=5 => FaceType::Sleepy,
        9..=11 | 14..=17 => FaceType::Happy,
        12..=13 => FaceType::VeryHappy,
        18..=20 => FaceType::Sunny,
        _ => FaceType::Happy,
    }
}

fn weather_emoji(c: &str) -> &'static str {
    match c {
        "d01" | "n01" | "trend_sun" => "☀",
        "d02" | "n02" => "⛅",
        "d03" => "☁",
        "d04" | "d05" => "⛈",
        "d06" | "d19" => "🌨",
        "d07" | "d08" | "d09" | "d10" | "d11" | "d12" | "trend_rain" => "🌧",
        "d13" | "d14" | "d15" | "d16" | "d17" => "❄",
        "d18" | "d20" => "🌫",
        _ => "☀",
    }
}

fn temp_icon(t: f32) -> &'static str {
    if t < 10.0 {
        "🥶"
    } else if t < 18.0 {
        "🌡"
    } else if t < 25.0 {
        "😊"
    } else if t < 30.0 {
        "😄"
    } else if t < 35.0 {
        "🥵"
    } else {
        "🔥"
    }
}

pub fn face_color(ft: FaceType) -> Rgb565 {
    match ft {
        FaceType::Happy | FaceType::VeryHappy | FaceType::Sunny => Rgb565::YELLOW,
        FaceType::Sad | FaceType::Rainy => Rgb565::CYAN,
        FaceType::Cold | FaceType::Snowy => Rgb565::WHITE,
        FaceType::Hot | FaceType::Angry | FaceType::Heart => Rgb565::RED,
        FaceType::Star => Rgb565::YELLOW,
        _ => Rgb565::GREEN,
    }
}

const SIN: [i32; 60] = [
    0, 105, 208, 309, 407, 500, 588, 669, 743, 809, 866, 914, 951, 978, 994, 1000, 994, 978,
    951, 914, 866, 809, 743, 669, 588, 500, 407, 309, 208, 105, 0, -105, -208, -309, -407,
    -500, -588, -669, -743, -809, -866, -914, -951, -978, -994, -1000, -994, -978, -951, -914,
    -866, -809, -743, -669, -588, -500, -407, -309, -208, -105,
];

fn sin_lut(i: usize) -> i32 {
    SIN[i % 60]
}

fn cos_lut(i: usize) -> i32 {
    sin_lut((i + 15) % 60)
}

fn hand_end(cx: i32, cy: i32, len: i32, idx: usize) -> Point {
    Point::new(
        cx + cos_lut(idx) * len / 1000,
        cy - sin_lut(idx) * len / 1000,
    )
}

const CX: i32 = 150;
const CY: i32 = 145;
const CR: i32 = 120;

// --- 主界面 UI 组件 ---

pub fn draw_clock(d: &mut Ili9488Display, t: &DateTime, theme: &Theme) {
    Circle::new(Point::new(CX - CR, CY - CR), (CR * 2) as u32)
        .into_styled(PrimitiveStyle::with_stroke(theme.text, 3))
        .draw(d)
        .ok();
    Circle::new(Point::new(CX - CR + 5, CY - CR + 5), ((CR - 5) * 2) as u32)
        .into_styled(PrimitiveStyle::with_stroke(theme.dim, 1))
        .draw(d)
        .ok();
    let ts = MonoTextStyle::new(&FONT_10X20, theme.text);
    let labels = ["12", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"];
    for i in 0..12usize {
        let outer = hand_end(CX, CY, CR - 8, i * 5);
        let inner = hand_end(CX, CY, CR - 22, i * 5);
        Line::new(inner, outer)
            .into_styled(PrimitiveStyle::with_stroke(theme.text, 3))
            .draw(d)
            .ok();
        let lp = hand_end(CX, CY, CR - 38, i * 5);
        let lx = if labels[i].len() == 1 { 4i32 } else { 9 };
        Text::new(labels[i], Point::new(lp.x - lx, lp.y + 6), ts)
            .draw(d)
            .ok();
    }
    for i in 0..60usize {
        if i % 5 != 0 {
            Line::new(hand_end(CX, CY, CR - 15, i), hand_end(CX, CY, CR - 8, i))
                .into_styled(PrimitiveStyle::with_stroke(theme.accent, 1))
                .draw(d)
                .ok();
        }
    }
    let sec = t.second as usize;
    let min = t.minute as usize;
    let hi = ((t.hour % 12) as usize) * 5 + min / 12;
    Line::new(Point::new(CX, CY), hand_end(CX, CY, 65, hi))
        .into_styled(PrimitiveStyle::with_stroke(theme.text, 4))
        .draw(d)
        .ok();
    Line::new(Point::new(CX, CY), hand_end(CX, CY, 90, min))
        .into_styled(PrimitiveStyle::with_stroke(theme.text, 2))
        .draw(d)
        .ok();
    Line::new(Point::new(CX, CY), hand_end(CX, CY, 100, sec))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::RED, 1))
        .draw(d)
        .ok();
    Circle::new(Point::new(CX - 4, CY - 4), 8)
        .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
        .draw(d)
        .ok();
    let ds = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(theme.text)
        .build();
    let s = alloc::format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
    Text::new(&s, Point::new(CX - 35, CY + 100), ds)
        .draw(d)
        .ok();
}

pub fn draw_local_panel(
    d: &mut Ili9488Display,
    temp: f32,
    hum: f32,
    press: f32,
    code: &str,
    trend: &str,
    net_weather: Option<&NetworkWeather>,
    wifi_connected: bool,
    theme: &Theme,
) {
    let x = 310i32;
    let mut y = 20i32;
    let title = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(theme.title)
        .build();
    let txt = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(theme.text)
        .build();
    let small = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(theme.dim)
        .build();
    let icon = weather_emoji(code);
    Text::new(
        &alloc::format!("{} {}", icon, config::LOCAL_CITY_NAME),
        Point::new(x, y),
        title,
    )
    .draw(d)
    .ok();
    y += 30;
    Line::new(Point::new(x, y), Point::new(x + 150, y))
        .into_styled(PrimitiveStyle::with_stroke(theme.accent, 1))
        .draw(d)
        .ok();
    y += 10;
    let outdoor = if wifi_connected {
        net_weather
    } else {
        None
    };
    if let Some(net) = outdoor {
        Text::new(
            &alloc::format!("室外 {:.1}°C", net.temp),
            Point::new(x, y),
            txt,
        )
        .draw(d)
        .ok();
        y += 25;
        Text::new(
            &alloc::format!("{} · 湿度 {:.0}%", net.text, net.humidity),
            Point::new(x, y),
            small,
        )
        .draw(d)
        .ok();
        y += 20;
        Text::new(&temp_diff_text(temp, net.temp), Point::new(x, y), small)
            .draw(d)
            .ok();
        y += 20;
    } else if !wifi_connected {
        Text::new("离线 · 仅室内数据", Point::new(x, y), small)
            .draw(d)
            .ok();
        y += 20;
    }
    Text::new("室内环境", Point::new(x, y), small).draw(d).ok();
    y += 18;
    Text::new(
        &alloc::format!("{} {:.1}°C", temp_icon(temp), temp),
        Point::new(x, y),
        txt,
    )
    .draw(d)
    .ok();
    y += 28;
    Text::new(&alloc::format!("💧 湿度 {:.0}%", hum), Point::new(x, y), txt)
        .draw(d)
        .ok();
    y += 28;
    Text::new(&alloc::format!("📊 {:.0} hPa", press), Point::new(x, y), txt)
        .draw(d)
        .ok();
    y += 28;
    Text::new(trend, Point::new(x + 20, y), small)
        .draw(d)
        .ok();
}

pub fn draw_partner_panel(
    d: &mut Ili9488Display,
    together_days: u32,
    partner_weather: Option<&NetworkWeather>,
    local_weather: Option<&NetworkWeather>,
    wifi_connected: bool,
    theme: &Theme,
) {
    let x = 310i32;
    let mut y = 20i32;
    let title = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(theme.title)
        .build();
    let txt = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(theme.text)
        .build();
    let small = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(theme.dim)
        .build();

    Text::new(
        &alloc::format!("Ta在·{}", config::PARTNER_CITY_NAME),
        Point::new(x, y),
        title,
    )
    .draw(d)
    .ok();
    y += 30;
    Line::new(Point::new(x, y), Point::new(x + 150, y))
        .into_styled(PrimitiveStyle::with_stroke(theme.accent, 1))
        .draw(d)
        .ok();
    y += 14;

    if !wifi_connected {
        Text::new("离线 · 暂无对方天气", Point::new(x, y), small)
            .draw(d)
            .ok();
    } else if let Some(net) = partner_weather {
        let icon = weather_emoji(net.weather_code.as_str());
        Text::new(
            &alloc::format!("{} {:.1}°C", icon, net.temp),
            Point::new(x, y),
            txt,
        )
        .draw(d)
        .ok();
        y += 30;
        Text::new(
            &alloc::format!("{} · 湿度 {:.0}%", net.text, net.humidity),
            Point::new(x, y),
            txt,
        )
        .draw(d)
        .ok();
        y += 28;
        if let Some(local) = local_weather {
            Text::new(
                &city_temp_diff_text(local.temp, net.temp),
                Point::new(x, y),
                small,
            )
            .draw(d)
            .ok();
        }
    } else {
        Text::new("对方天气加载中...", Point::new(x, y), small)
            .draw(d)
            .ok();
    }

    draw_together_days(d, x, 188, together_days, theme);
}

fn draw_together_days(d: &mut Ili9488Display, x: i32, y: i32, days: u32, theme: &Theme) {
    let small = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(theme.text)
        .build();
    let big = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(theme.title)
        .build();

    Text::new("我们在一起已经", Point::new(x, y), small)
        .draw(d)
        .ok();
    let days_s = alloc::format!("{}", days);
    let num_y = y + 18;
    Text::new(&days_s, Point::new(x + 4, num_y), big).draw(d).ok();
    let suffix_x = x + 4 + days_s.len() as i32 * 10 + 6;
    Text::new("天了", Point::new(suffix_x, num_y + 8), small)
        .draw(d)
        .ok();
}

fn city_temp_diff_text(local: f32, partner: f32) -> alloc::string::String {
    let diff = partner - local;
    if diff.abs() < 0.5 {
        alloc::format!("与这里气温相近")
    } else if diff > 0.0 {
        alloc::format!("比这里暖 {:.1}°C", diff)
    } else {
        alloc::format!("比这里凉 {:.1}°C", -diff)
    }
}

fn temp_diff_text(indoor: f32, outdoor: f32) -> alloc::string::String {
    let diff = indoor - outdoor;
    if diff.abs() < 0.5 {
        alloc::format!("室内外温差 持平")
    } else if diff > 0.0 {
        alloc::format!("室内比室外暖 {:.1}°C", diff)
    } else {
        alloc::format!("室内比室外凉 {:.1}°C", -diff)
    }
}

pub fn draw_wifi_icon(d: &mut Ili9488Display, connected: bool, theme: &Theme) {
    let (x, y) = (440i32, 10i32);
    let c = if connected {
        if theme.is_night {
            Rgb565::new(0, 120, 0)
        } else {
            Rgb565::GREEN
        }
    } else {
        theme.accent
    };
    Circle::new(Point::new(x + 6, y + 12), 4)
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(d)
        .ok();
    Circle::new(Point::new(x + 2, y + 8), 12)
        .into_styled(PrimitiveStyle::with_stroke(c, 2))
        .draw(d)
        .ok();
    Circle::new(Point::new(x - 2, y + 4), 20)
        .into_styled(PrimitiveStyle::with_stroke(c, 2))
        .draw(d)
        .ok();
    let s = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(c)
        .build();
    Text::new(
        if connected { "WiFi" } else { "断开" },
        Point::new(x - 5, y + 26),
        s,
    )
    .draw(d)
    .ok();
}

pub fn draw_date(d: &mut Ili9488Display, t: &DateTime, theme: &Theme) {
    let s = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(theme.dim)
        .build();
    let wd = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    let ds = alloc::format!(
        "{}年{}月{}日 {}",
        t.year,
        t.month,
        t.day,
        wd[t.weekday as usize % 7]
    );
    let x = (480 - ds.len() as i32 * 6) / 2;
    Text::new(&ds, Point::new(x, 305), s).draw(d).ok();
}

pub fn draw_special_event(d: &mut Ili9488Display, message: &str) {
    d.clear(Rgb565::new(0, 0, 50));
    let big = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::YELLOW)
        .build();
    Text::new("特别的日子！", Point::new(160, 120), big)
        .draw(d)
        .ok();
    Text::new(message, Point::new(160, 160), big).draw(d).ok();
}

pub fn draw_page_indicator(d: &mut Ili9488Display, page: DisplayPage, theme: &Theme) {
    let s = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(theme.dim)
        .build();
    let hint = alloc::format!("[{}] 轻触切换 长按爱心", page.label());
    Text::new(&hint, Point::new(8, 310), s).draw(d).ok();
}

pub fn draw_rain_overlay(d: &mut Ili9488Display, frame: u32, _theme: &Theme) {
    let rain = Rgb565::new(80, 160, 255);
    let style = PrimitiveStyle::with_stroke(rain, 1);
    for i in 0..12 {
        let x = ((frame.wrapping_mul(17).wrapping_add(i as u32 * 53)) % 460) as i32 + 10;
        let y = ((frame.wrapping_mul(11).wrapping_add(i as u32 * 37)) % 250) as i32 + 20;
        Line::new(Point::new(x, y), Point::new(x - 2, y + 8))
            .into_styled(style)
            .draw(d)
            .ok();
    }
}

pub fn draw_alert_banner(d: &mut Ili9488Display, _theme: &Theme) {
    let s = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(Rgb565::new(255, 100, 100))
        .build();
    Text::new("变天预警 · 气压持续下降", Point::new(8, 12), s)
        .draw(d)
        .ok();
}

// --- 配网专用全屏页 ---

pub fn draw_provisioning_screen(d: &mut Ili9488Display) {
    d.clear(Rgb565::new(0, 0, 40));
    let title = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::YELLOW)
        .build();
    let txt = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .build();
    let hint = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(Rgb565::new(160, 160, 180))
        .build();
    Text::new("WiFi 配网", Point::new(170, 50), title)
        .draw(d)
        .ok();
    Text::new("1. 手机连接热点", Point::new(24, 110), txt).draw(d).ok();
    let ssid = alloc::format!("   {}", config::PROVISION_AP_SSID);
    Text::new(&ssid, Point::new(24, 140), txt).draw(d).ok();
    Text::new("2. 浏览器打开", Point::new(24, 180), txt).draw(d).ok();
    Text::new("   192.168.4.1", Point::new(24, 210), txt).draw(d).ok();
    Text::new("3. 填写家里 WiFi", Point::new(24, 250), txt).draw(d).ok();
    Text::new("长按屏幕 5 秒可重新配网", Point::new(90, 300), hint)
        .draw(d)
        .ok();
}

pub fn draw_provisioning_success(d: &mut Ili9488Display) {
    d.clear(Rgb565::new(0, 24, 0));
    let title = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::YELLOW)
        .build();
    let txt = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .build();
    Text::new("配网成功", Point::new(170, 130), title).draw(d).ok();
    Text::new("设备即将重启...", Point::new(120, 180), txt)
        .draw(d)
        .ok();
}
