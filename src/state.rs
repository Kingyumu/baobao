use crate::config;
use crate::sensors::DateTime;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PressureTrend {
    Rising,
    Stable,
    Falling,
}

#[derive(Debug, Clone)]
pub struct NetworkWeather {
    pub temp: f32,
    pub humidity: f32,
    pub text: heapless::String<32>,
    pub weather_code: heapless::String<8>,
}

pub struct SystemState {
    pub temperature: f32,
    pub humidity: f32,
    pub pressure: f32,
    pressure_history: [f32; 10],
    pressure_idx: usize,
    pub pressure_filled: bool,
    pub pressure_trend: PressureTrend,
    pub current_time: Option<DateTime>,
    pub weather_code: heapless::String<8>,
    pub wifi_connected: bool,
    pub network_weather: Option<NetworkWeather>,
    pub animation_counter: u32,
    pub special_event: Option<&'static str>,
    pub special_event_handled: Option<(u8, u8)>,
    pub last_weather_update: u64,
    pub last_ntp_sync: u64,
}

impl SystemState {
    pub fn new() -> Self {
        let mut wc = heapless::String::new();
        wc.push_str("d01").unwrap();
        Self {
            temperature: 0.0,
            humidity: 0.0,
            pressure: 1013.0,
            pressure_history: [1013.0; 10],
            pressure_idx: 0,
            pressure_filled: false,
            pressure_trend: PressureTrend::Stable,
            current_time: None,
            weather_code: wc,
            wifi_connected: false,
            network_weather: None,
            animation_counter: 0,
            special_event: None,
            special_event_handled: None,
            last_weather_update: 0,
            last_ntp_sync: 0,
        }
    }

    pub fn update_pressure(&mut self, p: f32) {
        self.pressure = p;
        self.pressure_history[self.pressure_idx] = p;
        self.pressure_idx = (self.pressure_idx + 1) % 10;
        if self.pressure_idx == 0 {
            self.pressure_filled = true;
        }
        if !self.pressure_filled {
            return;
        }
        let (old, new) = self.pressure_history[..10].iter().enumerate().fold(
            (0.0f32, 0.0f32),
            |(o, n), (i, &v)| {
                if i < 5 {
                    (o + v, n)
                } else {
                    (o, n + v)
                }
            },
        );
        let diff = new / 5.0 - old / 5.0;
        self.pressure_trend = if diff > 0.5 {
            PressureTrend::Rising
        } else if diff < -0.5 {
            PressureTrend::Falling
        } else {
            PressureTrend::Stable
        };
    }

    pub fn trend_text(&self) -> &'static str {
        match self.pressure_trend {
            PressureTrend::Rising => "↑ 转晴",
            PressureTrend::Stable => "→ 平稳",
            PressureTrend::Falling => "↓ 转阴",
        }
    }

    pub fn update_time(&mut self, t: DateTime) {
        let today = (t.month, t.day);
        if self.special_event_handled.map(|d| d != today).unwrap_or(false) {
            self.special_event_handled = None;
        }
        self.check_special(&t);
        self.current_time = Some(t);
    }

    fn check_special(&mut self, t: &DateTime) {
        if t.month == config::BIRTHDAY_MONTH && t.day == config::BIRTHDAY_DAY {
            self.special_event = Some("生日快乐！");
            return;
        }
        for &(m, d, msg) in config::ANNIVERSARIES {
            if t.month == m && t.day == d {
                self.special_event = Some(msg);
                return;
            }
        }
        self.special_event = None;
    }

    pub fn should_handle_special_event(&self) -> bool {
        let Some(ev) = self.special_event else {
            return false;
        };
        let Some(t) = self.current_time else {
            return false;
        };
        let today = (t.month, t.day);
        self.special_event_handled != Some(today) && !ev.is_empty()
    }

    pub fn mark_special_event_handled(&mut self) {
        if let Some(t) = self.current_time {
            self.special_event_handled = Some((t.month, t.day));
        }
    }

    pub fn display_code(&self) -> &str {
        match self.pressure_trend {
            PressureTrend::Falling if self.pressure < 1000.0 => "trend_rain",
            PressureTrend::Rising if self.pressure > 1013.0 => "trend_sun",
            _ => self.weather_code.as_str(),
        }
    }

    pub fn set_weather_code(&mut self, code: &str) {
        self.weather_code.clear();
        let _ = self.weather_code.push_str(code);
    }
}
