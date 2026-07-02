use crate::display::{
    draw_alert_banner, draw_clock, draw_compare_page, draw_date, draw_detail_page,
    draw_page_indicator, draw_rain_overlay, draw_weather_panel, draw_wifi_icon, face_color,
    select_face, theme_for_hour, Face, FaceType, Theme, Ili9488Display,
};
use crate::sensors::DateTime;
use crate::state::{DisplayPage, NetworkWeather, SystemState};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};

const CLOCK_REGION: (u16, u16, u16, u16) = (0, 0, 300, 265);
const WEATHER_REGION: (u16, u16, u16, u16) = (300, 0, 175, 235);
const FACE_REGION: (u16, u16, u16, u16) = (375, 210, 40, 50);
const WIFI_REGION: (u16, u16, u16, u16) = (425, 0, 55, 45);
const DATE_REGION: (u16, u16, u16, u16) = (0, 292, 480, 28);

fn fill_region(d: &mut Ili9488Display, (x, y, w, h): (u16, u16, u16, u16), bg: Rgb565) {
    d.fill_rect(x, y, w, h, bg.into_storage());
}

fn f32_changed(a: f32, b: f32) -> bool {
    (a - b).abs() > 0.05
}

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
    wifi: bool,
    display_code: heapless::String<16>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            ready: false,
            page: DisplayPage::Main,
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
            wifi: false,
            display_code: heapless::String::new(),
        }
    }

    pub fn invalidate(&mut self) {
        self.ready = false;
    }

    pub fn update(
        &mut self,
        display: &mut Ili9488Display,
        state: &SystemState,
        time: &DateTime,
    ) {
        let theme = theme_for_hour(time.hour);
        let code = state.display_code();

        if state.display_page != DisplayPage::Main {
            display.clear(theme.bg);
            match state.display_page {
                DisplayPage::Detail => draw_detail_page(display, state, &theme),
                DisplayPage::Compare => draw_compare_page(display, state, &theme),
                DisplayPage::Main => {}
            }
            draw_wifi_icon(display, state.wifi_connected, &theme);
            draw_date(display, time, &theme);
            draw_page_indicator(display, state.display_page, &theme);
            self.sync(state, time, code, &theme);
            self.ready = true;
            return;
        }

        let ft = select_face(
            state.temperature,
            code,
            time.hour,
            state.pressure_trend,
            state.pressure,
            state.pressure_filled,
        );

        let layout_changed =
            !self.ready || self.page != state.display_page || self.night != theme.is_night;

        if layout_changed {
            display.clear(theme.bg);
            draw_clock(display, time, &theme);
            draw_weather_panel(
                display,
                state.temperature,
                state.humidity,
                state.pressure,
                code,
                state.trend_text(),
                state.network_weather.as_ref(),
                state.wifi_connected,
                &theme,
            );
            draw_wifi_icon(display, state.wifi_connected, &theme);
            self.draw_face(display, state, ft, code, &theme);
            draw_date(display, time, &theme);
            draw_page_indicator(display, state.display_page, &theme);
            if state.weather_alert_showing() {
                draw_rain_overlay(display, state.animation_counter, &theme);
                draw_alert_banner(display, &theme);
            }
            self.sync(state, time, code, &theme);
            self.ready = true;
            return;
        }

        if self.clock_changed(time) {
            fill_region(display, CLOCK_REGION, theme.bg);
            draw_clock(display, time, &theme);
        }

        if self.weather_changed(state, code) || self.wifi != state.wifi_connected {
            fill_region(display, WEATHER_REGION, theme.bg);
            draw_weather_panel(
                display,
                state.temperature,
                state.humidity,
                state.pressure,
                code,
                state.trend_text(),
                state.network_weather.as_ref(),
                state.wifi_connected,
                &theme,
            );
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
        self.draw_face(display, state, ft, code, &theme);

        if state.weather_alert_showing() {
            draw_rain_overlay(display, state.animation_counter, &theme);
            draw_alert_banner(display, &theme);
        }

        self.sync(state, time, code, &theme);
    }

    fn draw_face(
        &self,
        display: &mut Ili9488Display,
        state: &SystemState,
        ft: FaceType,
        code: &str,
        theme: &Theme,
    ) {
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

    fn weather_changed(&self, state: &SystemState, code: &str) -> bool {
        f32_changed(self.temp, state.temperature)
            || f32_changed(self.hum, state.humidity)
            || f32_changed(self.press, state.pressure)
            || self.weather_code.as_str() != state.weather_code.as_str()
            || self.trend.as_str() != state.trend_text()
            || self.display_code.as_str() != code
            || self.outdoor_changed(state.network_weather.as_ref())
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
    }
}
