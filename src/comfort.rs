//! 舒适度与露点计算（基于 BME280 温湿度）。

/// 露点温度（°C），Magnus 公式。
pub fn dew_point(temp_c: f32, rh_percent: f32) -> f32 {
    let rh = (rh_percent / 100.0).clamp(0.0001, 1.0);
    let gamma = (17.62 * temp_c) / (243.12 + temp_c) + libm::logf(rh);
    (243.12 * gamma) / (17.62 - gamma)
}

pub fn comfort_label(temp: f32, hum: f32) -> &'static str {
    if temp > 30.0 && hum > 70.0 {
        "闷热"
    } else if hum > 75.0 {
        "偏湿"
    } else if hum < 35.0 {
        "偏干"
    } else if temp < 16.0 {
        "偏冷"
    } else if temp > 28.0 {
        "偏热"
    } else {
        "舒适"
    }
}
