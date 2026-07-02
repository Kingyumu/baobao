//! 项目配置 — 烧录前在此修改 WiFi、城市、纪念日等参数。

pub const WIFI_SSID: &str = "你的WiFi名称";
pub const WIFI_PASSWORD: &str = "你的WiFi密码";
pub const CITY_CODE: &str = "101210106";
pub const NTP_SERVER: &str = "ntp.ntsc.ac.cn";
pub const DISPLAY_UPDATE_INTERVAL: u64 = 1;
pub const WEATHER_UPDATE_INTERVAL: u64 = 1800;
pub const NTP_SYNC_INTERVAL: u64 = 3600;
pub const BIRTHDAY_MONTH: u8 = 3;
pub const BIRTHDAY_DAY: u8 = 15;
pub const ANNIVERSARIES: &[(u8, u8, &str)] = &[
    (2, 14, "情人节快乐！"),
    (12, 25, "圣诞快乐！"),
    (1, 1, "新年快乐！"),
    (6, 15, "纪念日快乐！"),
];
