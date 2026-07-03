//! 项目配置 — 城市、纪念日等；WiFi 可在烧录后通过手机配网，也可在此填开发默认凭据。
//!
//! ## 设计说明
//! 全部用 `pub const`：编译期常量，不占 RAM，改配置后重新 `cargo build` 即可。
//! 布尔开关（如 `BLE_ENABLED`）便于开发时关闭整块功能，减少固件体积与调试面。

// --- WiFi（开发兜底；正式送礼可保持占位符，靠网页配网） ---
pub const WIFI_SSID: &str = "你的WiFi名称";
pub const WIFI_PASSWORD: &str = "你的WiFi密码";

// --- 网络服务（中国天气网城市码，见制作指南查询方法） ---
pub const CITY_CODE: &str = "101210106"; // 本地 / 设备所在城市
pub const LOCAL_CITY_NAME: &str = "杭州-余杭";
pub const PARTNER_CITY_CODE: &str = "101280801003"; // 对方所在城市
pub const PARTNER_CITY_NAME: &str = "佛山-顺德-大良街道";
pub const NTP_SERVER: &str = "ntp.ntsc.ac.cn";
pub const DISPLAY_UPDATE_INTERVAL: u64 = 1; // 主循环周期（秒）
pub const WEATHER_UPDATE_INTERVAL: u64 = 1800; // 30 分钟
pub const NTP_SYNC_INTERVAL: u64 = 3600;
pub const WIFI_RECONNECT_INTERVAL: u64 = 60;

// --- SoftAP 网页配网 ---
pub const PROVISION_AP_SSID: &str = "Baobao";
pub const PROVISION_AP_CHANNEL: u8 = 6;
pub const PROVISION_IP: (u8, u8, u8, u8) = (192, 168, 4, 1);
pub const PROVISION_CONNECT_ATTEMPTS: u32 = 3; // 配网时单网络重试次数
pub const TOUCH_CLEAR_WIFI_MS: u64 = 5000; // 长按清除全部 WiFi 记忆

// --- 显示与交互 ---
pub const NIGHT_START_HOUR: u8 = 22;
pub const NIGHT_END_HOUR: u8 = 7;
pub const TOUCH_LONG_PRESS_MS: u64 = 2000; // 长按出爱心

// --- 闹钟（软件轮询 DS3231，到点蜂鸣） ---
pub const ALARM_ENABLED: bool = false;
pub const ALARM_HOUR: u8 = 7;
pub const ALARM_MINUTE: u8 = 30;

// --- 整点报时 ---
pub const HOURLY_CHIME_ENABLED: bool = false;
pub const HOURLY_CHIME_START: u8 = 8;
pub const HOURLY_CHIME_END: u8 = 21;

// --- 室温曲线（环形缓冲区，本地页时钟下方折线图） ---
pub const TEMP_CHART_INTERVAL: u64 = 60; // 每分钟采样一次
pub const TEMP_CHART_LEN: usize = 120; // 约 2 小时历史

// --- 变天预警 ---
pub const WEATHER_ALERT_COOLDOWN: u64 = 3600;
pub const WEATHER_ALERT_DURATION: u64 = 30;

// --- BLE 广播（手机 nRF Connect 等可连接） ---
pub const BLE_ENABLED: bool = false;
pub const BLE_DEVICE_NAME: &str = "Baobao";
pub const BLE_NOTIFY_INTERVAL_SECS: u64 = 2;

// --- 恋爱纪念日（对方页显示「在一起 n 天」，含起始日当天） ---
pub const LOVE_START_YEAR: u16 = 2026;
pub const LOVE_START_MONTH: u8 = 6;
pub const LOVE_START_DAY: u8 = 20;

// --- 纪念日（月, 日, 显示文案） ---
pub const BIRTHDAY_MONTH: u8 = 3;
pub const BIRTHDAY_DAY: u8 = 15;
pub const ANNIVERSARIES: &[(u8, u8, &str)] = &[
    (2, 14, "情人节快乐！"),
    (12, 25, "圣诞快乐！"),
    (1, 1, "新年快乐！"),
    (6, 20, "宝宝~到了我们的恋爱纪念日哦❤"),
];
