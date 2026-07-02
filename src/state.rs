use crate::config;
use crate::sensors::DateTime;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PressureTrend {
    Rising,
    Stable,
    Falling,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayPage {
    Main,
    Detail,
    Compare,
}

impl DisplayPage {
    pub fn next(self) -> Self {
        match self {
            Self::Main => Self::Detail,
            Self::Detail => Self::Compare,
            Self::Compare => Self::Main,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Main => "主页",
            Self::Detail => "详情",
            Self::Compare => "对比",
        }
    }
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
    pub display_page: DisplayPage,
    pressure_chart: [f32; config::PRESSURE_CHART_LEN],
    chart_idx: usize,
    chart_filled: bool,
    last_chart_sample: u64,
    pub weather_alert_active: bool,
    weather_alert_until: u64,
    last_weather_alert: u64,
    last_hourly_chime: Option<(u8, u8, u8)>,
    alarm_handled: Option<(u8, u8)>,
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
            display_page: DisplayPage::Main,
            pressure_chart: [1013.0; config::PRESSURE_CHART_LEN],
            chart_idx: 0,
            chart_filled: false,
            last_chart_sample: 0,
            weather_alert_active: false,
            weather_alert_until: 0,
            last_weather_alert: 0,
            last_hourly_chime: None,
            alarm_handled: None,
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

    pub fn next_page(&mut self) {
        self.display_page = self.display_page.next();
    }

    pub fn sample_pressure_chart(&mut self, now: u64) {
        if now.saturating_sub(self.last_chart_sample) < config::PRESSURE_CHART_INTERVAL {
            return;
        }
        self.last_chart_sample = now;
        self.pressure_chart[self.chart_idx] = self.pressure;
        self.chart_idx = (self.chart_idx + 1) % config::PRESSURE_CHART_LEN;
        if self.chart_idx == 0 {
            self.chart_filled = true;
        }
    }

    pub fn chart_count(&self) -> usize {
        if self.chart_filled {
            config::PRESSURE_CHART_LEN
        } else {
            self.chart_idx
        }
    }

    pub fn chart_value(&self, i: usize) -> f32 {
        let n = self.chart_count();
        if n == 0 {
            return self.pressure;
        }
        let start = if self.chart_filled {
            self.chart_idx
        } else {
            0
        };
        self.pressure_chart[(start + i) % config::PRESSURE_CHART_LEN]
    }

    pub fn check_weather_alert(&mut self, now: u64) -> bool {
        if self.weather_alert_active {
            return false;
        }
        if now.saturating_sub(self.last_weather_alert) < config::WEATHER_ALERT_COOLDOWN {
            return false;
        }
        if !self.pressure_filled || self.pressure_trend != PressureTrend::Falling {
            return false;
        }
        if self.pressure >= 1005.0 {
            return false;
        }
        self.last_weather_alert = now;
        true
    }

    pub fn activate_weather_alert(&mut self, now: u64) {
        self.weather_alert_active = true;
        self.weather_alert_until = now + config::WEATHER_ALERT_DURATION;
    }

    pub fn tick_weather_alert(&mut self, now: u64) {
        if self.weather_alert_active && now >= self.weather_alert_until {
            self.weather_alert_active = false;
        }
    }

    pub fn weather_alert_showing(&self) -> bool {
        self.weather_alert_active
    }

    pub fn check_hourly_chime(&mut self, t: &DateTime) -> bool {
        if !config::HOURLY_CHIME_ENABLED {
            return false;
        }
        if t.minute != 0 || t.second >= 2 {
            return false;
        }
        if t.hour < config::HOURLY_CHIME_START || t.hour > config::HOURLY_CHIME_END {
            return false;
        }
        let key = (t.hour, t.day, t.month);
        if self.last_hourly_chime == Some(key) {
            return false;
        }
        self.last_hourly_chime = Some(key);
        true
    }

    pub fn check_alarm(&mut self, t: &DateTime) -> bool {
        if !config::ALARM_ENABLED {
            return false;
        }
        if t.hour != config::ALARM_HOUR
            || t.minute != config::ALARM_MINUTE
            || t.second >= 2
        {
            return false;
        }
        let today = (t.month, t.day);
        if self.alarm_handled == Some(today) {
            return false;
        }
        self.alarm_handled = Some(today);
        true
    }
}
