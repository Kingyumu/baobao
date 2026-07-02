use crate::display::{
    draw_clock, draw_date, draw_weather_panel, draw_wifi_icon, face_color, select_face, Face,
    FaceType, BG_COLOR, Ili9488Display,
};
use crate::sensors::DateTime;
use crate::state::{NetworkWeather, SystemState};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};

const CLOCK_REGION: (u16, u16, u16, u16) = (0, 0, 300, 265);
const WEATHER_REGION: (u16, u16, u16, u16) = (300, 0, 175, 220);
const FACE_REGION: (u16, u16, u16, u16) = (375, 210, 40, 50);
const WIFI_REGION: (u16, u16, u16, u16) = (425, 0, 55, 45);
const DATE_REGION: (u16, u16, u16, u16) = (0, 292, 480, 28);

fn fill_region(d: &mut Ili9488Display, (x, y, w, h): (u16, u16, u16, u16)) {
    d.fill_rect(x, y, w, h, BG_COLOR.into_storage());
}

fn f32_changed(a: f32, b: f32) -> bool {
    (a - b).abs() > 0.05
}

pub struct RenderCache {
    ready: bool,
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
    wifi: bool,
    display_code: heapless::String<16>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            ready: false,
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
        let code = state.display_code();
        let ft = select_face(
            state.temperature,
            code,
            time.hour,
            state.pressure_trend,
            state.pressure,
            state.pressure_filled,
        );

        if !self.ready {
            display.clear(BG_COLOR);
            draw_clock(display, time);
            draw_weather_panel(
                display,
                state.temperature,
                state.humidity,
                state.pressure,
                code,
                state.trend_text(),
                state.network_weather.as_ref(),
            );
            draw_wifi_icon(display, state.wifi_connected);
            self.draw_face(display, state, ft, code);
            draw_date(display, time);
            self.sync(state, time, code);
            self.ready = true;
            return;
        }

        if self.clock_changed(time) {
            fill_region(display, CLOCK_REGION);
            draw_clock(display, time);
        }

        if self.weather_changed(state, code) {
            fill_region(display, WEATHER_REGION);
            draw_weather_panel(
                display,
                state.temperature,
                state.humidity,
                state.pressure,
                code,
                state.trend_text(),
                state.network_weather.as_ref(),
            );
        }

        if self.wifi != state.wifi_connected {
            fill_region(display, WIFI_REGION);
            draw_wifi_icon(display, state.wifi_connected);
        }

        if self.date_changed(time) {
            fill_region(display, DATE_REGION);
            draw_date(display, time);
        }

        fill_region(display, FACE_REGION);
        self.draw_face(display, state, ft, code);

        self.sync(state, time, code);
    }

    fn draw_face(
        &self,
        display: &mut Ili9488Display,
        state: &SystemState,
        ft: FaceType,
        code: &str,
    ) {
        let mut face = Face::new(ft);
        if state.animation_counter % 2 == 0 {
            face.next_frame();
        }
        face.draw_scaled(display, 380, 215, 4, face_color(ft));

        if state.animation_counter % 15 == 14 {
            fill_region(display, (380, 215, 32, 32));
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
                .text_color(Rgb565::new(200, 200, 200))
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
                f32_changed(a, b.temp) || self.outdoor_text.as_str() != b.text.as_str()
            }
            _ => true,
        }
    }

    fn sync(&mut self, state: &SystemState, time: &DateTime, code: &str) {
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
        }
    }
}
