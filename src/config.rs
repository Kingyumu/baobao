//! 项目配置 — 烧录前在此修改 WiFi、城市、纪念日等参数。

pub const WIFI_SSID: &str = "你的WiFi名称";
pub const WIFI_PASSWORD: &str = "你的WiFi密码";
pub const CITY_CODE: &str = "101210106";
pub const NTP_SERVER: &str = "ntp.ntsc.ac.cn";
pub const DISPLAY_UPDATE_INTERVAL: u64 = 1;
pub const WEATHER_UPDATE_INTERVAL: u64 = 1800;
pub const NTP_SYNC_INTERVAL: u64 = 3600;
pub const WIFI_RECONNECT_INTERVAL: u64 = 60;
pub const NIGHT_START_HOUR: u8 = 22;
pub const NIGHT_END_HOUR: u8 = 7;
pub const TOUCH_LONG_PRESS_MS: u64 = 2000;

// 闹钟（软件检测，到点蜂鸣）
pub const ALARM_ENABLED: bool = true;
pub const ALARM_HOUR: u8 = 7;
pub const ALARM_MINUTE: u8 = 30;

// 整点报时（8:00–21:00 每小时短响一声）
pub const HOURLY_CHIME_ENABLED: bool = true;
pub const HOURLY_CHIME_START: u8 = 8;
pub const HOURLY_CHIME_END: u8 = 21;

// 气压曲线（每 60 秒采样，保留 48 点 ≈ 48 分钟）
pub const PRESSURE_CHART_INTERVAL: u64 = 60;
pub const PRESSURE_CHART_LEN: usize = 48;

// 变天预警
pub const WEATHER_ALERT_COOLDOWN: u64 = 3600;
pub const WEATHER_ALERT_DURATION: u64 = 30;

// BLE 广播室内传感器（手机 nRF Connect 等可连接读取/订阅）
pub const BLE_ENABLED: bool = true;
pub const BLE_DEVICE_NAME: &str = "BaobaoWeather";
pub const BLE_NOTIFY_INTERVAL_SECS: u64 = 2;

pub const BIRTHDAY_MONTH: u8 = 3;
pub const BIRTHDAY_DAY: u8 = 15;
pub const ANNIVERSARIES: &[(u8, u8, &str)] = &[
    (2, 14, "情人节快乐！"),
    (12, 25, "圣诞快乐！"),
    (1, 1, "新年快乐！"),
    (6, 15, "纪念日快乐！"),
];
