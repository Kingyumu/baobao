//! 脏矩形渲染缓存 — 只重绘变化的 UI 区域，降低 SPI 刷屏开销。
//!
//! ## 思路
//! 比较当前 [`SystemState`] 与上一帧缓存的字段；仅当时钟/天气/WiFi 等变化时
//! 调用对应 `draw_*`，否则跳过。换页或夜间主题切换时 `invalidate()` 全量重绘。
//!
//! ## Rust 要点
//! - `heapless::String<N>` 存上一帧字符串，用于 cheap 相等性比较
//! - `Option<f32>` 区分「无室外天气」与「有数据」两种 UI 状态

use crate::display::{
    draw_alert_banner, draw_clock, draw_date, draw_local_panel, draw_page_indicator,
    draw_partner_panel, draw_rain_overlay, draw_wifi_icon, face_color, select_face,
    theme_for_hour, Face, FaceType, Theme, Ili9488Display,
};
use crate::sensors::DateTime;
use crate::state::{DisplayPage, NetworkWeather, SystemState};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};

/// 屏幕各区域的像素范围 (x, y, w, h)，用于局部 `fill_rect` 清背景。
const CLOCK_REGION: (u16, u16, u16, u16) = (0, 0, 300, 265);
const INFO_REGION: (u16, u16, u16, u16) = (300, 0, 175, 235);
const FACE_REGION: (u16, u16, u16, u16) = (375, 210, 40, 50);
const WIFI_REGION: (u16, u16, u16, u16) = (425, 0, 55, 45);
const DATE_REGION: (u16, u16, u16, u16) = (0, 292, 480, 28);

fn fill_region(d: &mut Ili9488Display, (x, y, w, h): (u16, u16, u16, u16), bg: Rgb565) {
    d.fill_rect(x, y, w, h, bg.into_storage());
}

fn f32_changed(a: f32, b: f32) -> bool {
    (a - b).abs() > 0.05
}

/// 上一帧快照；`ready=false` 表示需要全量布局重绘。
pub struct RenderCache {
    ready: bool,
    page: DisplayPage,
    night: bool,
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u16,
    temp: f32,
    hum: f32,
    press: f32,
    weather_code: heapless::String<8>,
    trend: heapless::String<16>,
    outdoor_temp: Option<f32>,
    outdoor_text: heapless::String<32>,
    outdoor_code: heapless::String<8>,
    partner_temp: Option<f32>,
    partner_text: heapless::String<32>,
    partner_code: heapless::String<8>,
    together_days: u32,
    wifi: bool,
    display_code: heapless::String<16>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            ready: false,
            page: DisplayPage::Local,
            night: false,
            second: 255,
            minute: 255,
            hour: 255,
            day: 0,
            month: 0,
            year: 0,
            temp: 0.0,
            hum: 0.0,
            press: 0.0,
            weather_code: heapless::String::new(),
            trend: heapless::String::new(),
            outdoor_temp: None,
            outdoor_text: heapless::String::new(),
            outdoor_code: heapless::String::new(),
            partner_temp: None,
            partner_text: heapless::String::new(),
            partner_code: heapless::String::new(),
            together_days: 0,
            wifi: false,
            display_code: heapless::String::new(),
        }
    }

    /// 强制下一帧全量重绘（换页、WiFi 重连、预警等事件后调用）。
    pub fn invalidate(&mut self) {
        self.ready = false;
    }

    /// 主渲染入口：本地页与对方页均保留左侧时钟圆盘。
    pub fn update(
        &mut self,
        display: &mut Ili9488Display,
        state: &SystemState,
        time: &DateTime,
    ) {
        let theme = theme_for_hour(time.hour);
        let local_code = state.display_code();
        let partner_code = state
            .partner_weather
            .as_ref()
            .map(|w| w.weather_code.as_str())
            .unwrap_or("d01");

        let layout_changed =
            !self.ready || self.page != state.display_page || self.night != theme.is_night;

        if layout_changed {
            display.clear(theme.bg);
            draw_clock(display, time, &theme);
            self.draw_info_panel(display, state, local_code, &theme);
            draw_wifi_icon(display, state.wifi_connected, &theme);
            self.draw_face(display, state, local_code, partner_code, &theme);
            draw_date(display, time, &theme);
            draw_page_indicator(display, state.display_page, &theme);
            if state.display_page == DisplayPage::Local && state.weather_alert_showing() {
                draw_rain_overlay(display, state.animation_counter, &theme);
                draw_alert_banner(display, &theme);
            }
            self.sync(state, time, local_code, &theme);
            self.ready = true;
            return;
        }

        if self.clock_changed(time) {
            fill_region(display, CLOCK_REGION, theme.bg);
            draw_clock(display, time, &theme);
        }

        if self.info_changed(state, local_code) || self.wifi != state.wifi_connected {
            fill_region(display, INFO_REGION, theme.bg);
            self.draw_info_panel(display, state, local_code, &theme);
        }

        if self.wifi != state.wifi_connected {
            fill_region(display, WIFI_REGION, theme.bg);
            draw_wifi_icon(display, state.wifi_connected, &theme);
        }

        if self.date_changed(time) {
            fill_region(display, DATE_REGION, theme.bg);
            draw_date(display, time, &theme);
        }

        fill_region(display, FACE_REGION, theme.bg);
        self.draw_face(display, state, local_code, partner_code, &theme);

        if state.display_page == DisplayPage::Local && state.weather_alert_showing() {
            draw_rain_overlay(display, state.animation_counter, &theme);
            draw_alert_banner(display, &theme);
        }

        self.sync(state, time, local_code, &theme);
    }

    fn draw_info_panel(
        &self,
        display: &mut Ili9488Display,
        state: &SystemState,
        local_code: &str,
        theme: &Theme,
    ) {
        match state.display_page {
            DisplayPage::Local => draw_local_panel(
                display,
                state.temperature,
                state.humidity,
                state.pressure,
                local_code,
                state.trend_text(),
                state.network_weather.as_ref(),
                state.wifi_connected,
                theme,
            ),
            DisplayPage::Partner => draw_partner_panel(
                display,
                state.together_days,
                state.partner_weather.as_ref(),
                state.network_weather.as_ref(),
                state.wifi_connected,
                theme,
            ),
        }
    }

    fn draw_face(
        &self,
        display: &mut Ili9488Display,
        state: &SystemState,
        local_code: &str,
        partner_code: &str,
        theme: &Theme,
    ) {
        let (ft, code) = match state.display_page {
            DisplayPage::Local => (
                select_face(
                    state.temperature,
                    local_code,
                    state
                        .current_time
                        .map(|t| t.hour)
                        .unwrap_or(12),
                    state.pressure_trend,
                    state.pressure,
                    state.pressure_filled,
                ),
                local_code,
            ),
            DisplayPage::Partner => {
                let temp = state
                    .partner_weather
                    .as_ref()
                    .map(|w| w.temp)
                    .unwrap_or(20.0);
                (
                    select_face(
                        temp,
                        partner_code,
                        state
                            .current_time
                            .map(|t| t.hour)
                            .unwrap_or(12),
                        state.pressure_trend,
                        state.pressure,
                        state.pressure_filled,
                    ),
                    partner_code,
                )
            }
        };

        let mut face = Face::new(ft);
        if state.animation_counter % 2 == 0 {
            face.next_frame();
        }
        face.draw_scaled(display, 380, 215, 4, face_color(ft));

        if state.animation_counter % 15 == 14 {
            fill_region(display, (380, 215, 32, 32), theme.bg);
            Face::new(FaceType::Sleepy).draw_scaled(
                display,
                380,
                215,
                4,
                face_color(FaceType::Sleepy),
            );
        }

        if code == "trend_rain" || code == "trend_sun" {
            let tip = MonoTextStyleBuilder::new()
                .font(&FONT_6X10)
                .text_color(theme.dim)
                .build();
            Text::new(
                if code == "trend_rain" {
                    "气压下降"
                } else {
                    "气压上升"
                },
                Point::new(378, 251),
                tip,
            )
            .draw(display)
            .ok();
        }
    }

    fn clock_changed(&self, t: &DateTime) -> bool {
        self.second != t.second || self.minute != t.minute || self.hour != t.hour
    }

    fn date_changed(&self, t: &DateTime) -> bool {
        self.day != t.day || self.month != t.month || self.year != t.year
    }

    fn info_changed(&self, state: &SystemState, local_code: &str) -> bool {
        match state.display_page {
            DisplayPage::Local => self.local_info_changed(state, local_code),
            DisplayPage::Partner => self.partner_info_changed(state),
        }
    }

    fn local_info_changed(&self, state: &SystemState, code: &str) -> bool {
        f32_changed(self.temp, state.temperature)
            || f32_changed(self.hum, state.humidity)
            || f32_changed(self.press, state.pressure)
            || self.weather_code.as_str() != state.weather_code.as_str()
            || self.trend.as_str() != state.trend_text()
            || self.display_code.as_str() != code
            || self.outdoor_changed(state.network_weather.as_ref())
    }

    fn partner_info_changed(&self, state: &SystemState) -> bool {
        self.partner_changed(state.partner_weather.as_ref())
            || self.outdoor_changed(state.network_weather.as_ref())
            || self.together_days != state.together_days
    }

    fn outdoor_changed(&self, net: Option<&NetworkWeather>) -> bool {
        match (self.outdoor_temp, net) {
            (None, None) => false,
            (Some(a), Some(b)) => {
                f32_changed(a, b.temp)
                    || self.outdoor_text.as_str() != b.text.as_str()
                    || self.outdoor_code.as_str() != b.weather_code.as_str()
            }
            _ => true,
        }
    }

    fn partner_changed(&self, net: Option<&NetworkWeather>) -> bool {
        match (self.partner_temp, net) {
            (None, None) => false,
            (Some(a), Some(b)) => {
                f32_changed(a, b.temp)
                    || self.partner_text.as_str() != b.text.as_str()
                    || self.partner_code.as_str() != b.weather_code.as_str()
            }
            _ => true,
        }
    }

    fn sync(&mut self, state: &SystemState, time: &DateTime, code: &str, theme: &Theme) {
        self.page = state.display_page;
        self.night = theme.is_night;
        self.second = time.second;
        self.minute = time.minute;
        self.hour = time.hour;
        self.day = time.day;
        self.month = time.month;
        self.year = time.year;
        self.temp = state.temperature;
        self.hum = state.humidity;
        self.press = state.pressure;
        self.weather_code.clear();
        let _ = self.weather_code.push_str(state.weather_code.as_str());
        self.trend.clear();
        let _ = self.trend.push_str(state.trend_text());
        self.display_code.clear();
        let _ = self.display_code.push_str(code);
        self.wifi = state.wifi_connected;
        self.outdoor_temp = state.network_weather.as_ref().map(|w| w.temp);
        self.outdoor_text.clear();
        if let Some(w) = state.network_weather.as_ref() {
            let _ = self.outdoor_text.push_str(w.text.as_str());
            self.outdoor_code.clear();
            let _ = self.outdoor_code.push_str(w.weather_code.as_str());
        } else {
            self.outdoor_code.clear();
        }
        self.partner_temp = state.partner_weather.as_ref().map(|w| w.temp);
        self.partner_text.clear();
        if let Some(w) = state.partner_weather.as_ref() {
            let _ = self.partner_text.push_str(w.text.as_str());
            self.partner_code.clear();
            let _ = self.partner_code.push_str(w.weather_code.as_str());
        } else {
            self.partner_code.clear();
        }
        self.together_days = state.together_days;
    }
}
