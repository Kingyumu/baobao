//! I2C 传感器驱动 — BME280（温湿度气压）与 DS3231（RTC）。
//!
//! ## 通信模式
//! 所有方法通过 [`I2cBus`]（Mutex 包装的 async I2C）访问硬件：
//! `bus.lock().await` 获取独占访问权后再 `write` / `write_read`。
//!
//! ## Rust 要点
//! - `embedded_hal_async::i2c::I2c` trait：抽象 I2C 读写，与具体 HAL 解耦
//! - `Option<Calib>`：校准系数读一次后缓存，避免每次测量都读寄存器
//! - `Result<T, E>`：I2C 失败时返回 [`i2c::Error`]，调用方用 `match` 处理

use crate::i2c_bus::I2cBus;
use defmt::*;
use embassy_rp::i2c;
use embassy_time::{Duration, Timer};
use embedded_hal_async::i2c::I2c;

/// 统一的时间结构（与 DS3231 寄存器字段对应）。
#[derive(Debug, Clone, Copy)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8,
}

/// 单次 BME280 测量结果。
pub struct Measurements {
    pub temperature: f32,
    pub pressure: f32,
    pub humidity: f32,
}

/// Bosch 数据手册中的补偿参数（从芯片 OTP 读出）。
struct Calib {
    t1: u16,
    t2: i16,
    t3: i16,
    p1: u16,
    p2: i16,
    p3: i16,
    p4: i16,
    p5: i16,
    p6: i16,
    p7: i16,
    p8: i16,
    p9: i16,
    h1: u8,
    h2: i16,
    h3: u8,
    h4: i16,
    h5: i16,
    h6: i8,
    t_fine: i32, // 温度补偿中间值，湿度补偿会用到
}

pub struct Bme280 {
    addr: u8,
    cal: Option<Calib>,
}

impl Bme280 {
    pub fn new() -> Self {
        Self {
            addr: 0x76, // SDO 接 GND；接 VCC 时为 0x77
            cal: None,
        }
    }

    async fn read_regs(&self, bus: &I2cBus, reg: u8, buf: &mut [u8]) -> Result<(), i2c::Error> {
        bus.lock().await.write_read(self.addr, &[reg], buf).await
    }

    async fn write_reg(&self, bus: &I2cBus, reg: u8, val: u8) -> Result<(), i2c::Error> {
        bus.lock().await.write(self.addr, &[reg, val]).await
    }

    /// 软复位 → 读校准系数 → 配置 oversampling 与 standby。
    pub async fn init(&mut self, bus: &I2cBus) -> Result<(), i2c::Error> {
        let mut id = [0u8];
        self.read_regs(bus, 0xD0, &mut id).await?;
        info!("BME280 芯片ID: 0x{:02X}", id[0]);
        self.write_reg(bus, 0xE0, 0xB6).await?; // soft reset
        Timer::after(Duration::from_millis(10)).await;
        let mut pt = [0u8; 26];
        self.read_regs(bus, 0x88, &mut pt).await?;
        let mut h = [0u8; 7];
        self.read_regs(bus, 0xE1, &mut h).await?;
        // 小端序拼接 16 位校准寄存器
        self.cal = Some(Calib {
            t1: u16::from_le_bytes([pt[0], pt[1]]),
            t2: i16::from_le_bytes([pt[2], pt[3]]),
            t3: i16::from_le_bytes([pt[4], pt[5]]),
            p1: u16::from_le_bytes([pt[6], pt[7]]),
            p2: i16::from_le_bytes([pt[8], pt[9]]),
            p3: i16::from_le_bytes([pt[10], pt[11]]),
            p4: i16::from_le_bytes([pt[12], pt[13]]),
            p5: i16::from_le_bytes([pt[14], pt[15]]),
            p6: i16::from_le_bytes([pt[16], pt[17]]),
            p7: i16::from_le_bytes([pt[18], pt[19]]),
            p8: i16::from_le_bytes([pt[20], pt[21]]),
            p9: i16::from_le_bytes([pt[22], pt[23]]),
            h1: pt[25],
            h2: i16::from_le_bytes([h[0], h[1]]),
            h3: h[2],
            h4: ((h[3] as i16) << 4) | ((h[4] as i16) & 0x0F),
            h5: ((h[5] as i16) << 4) | ((h[4] as i16) >> 4),
            h6: h[6] as i8,
            t_fine: 0,
        });
        self.write_reg(bus, 0xF2, 0x01).await?; // humidity oversampling x1
        self.write_reg(bus, 0xF4, 0x27).await?; // temp x1, pressure x1, normal mode
        self.write_reg(bus, 0xF5, 0xA0).await?; // standby 1000ms, filter off
        Ok(())
    }

    /// 触发 forced 模式测量，等待后读原始 ADC 并补偿。
    pub async fn measure(&mut self, bus: &I2cBus) -> Result<Measurements, i2c::Error> {
        self.write_reg(bus, 0xF4, 0x25).await?; // forced mode
        Timer::after(Duration::from_millis(50)).await;
        let mut data = [0u8; 8];
        self.read_regs(bus, 0xF7, &mut data).await?;
        let cal = self.cal.as_mut().unwrap();
        // 20 位 ADC 原始值（数据手册位域拼接）
        let raw_p =
            ((data[0] as u32) << 12) | ((data[1] as u32) << 4) | ((data[2] as u32) >> 4);
        let raw_t =
            ((data[3] as u32) << 12) | ((data[4] as u32) << 4) | ((data[5] as u32) >> 4);
        let raw_h = ((data[6] as u32) << 8) | (data[7] as u32);
        let t = Self::comp_t(raw_t, cal);
        let p = Self::comp_p(raw_p, cal);
        let h = Self::comp_h(raw_h, cal);
        Ok(Measurements {
            temperature: t,
            pressure: p,
            humidity: h,
        })
    }

    /// 温度补偿（官方公式，会更新 cal.t_fine 供湿度用）。
    fn comp_t(raw: u32, c: &mut Calib) -> f32 {
        let v1 = (raw as f32 / 16384.0 - c.t1 as f32 / 1024.0) * c.t2 as f32;
        let v2 = raw as f32 / 131072.0 - c.t1 as f32 / 8192.0;
        let v2 = v2 * v2 * c.t3 as f32;
        c.t_fine = (v1 + v2) as i32;
        (v1 + v2) / 5120.0
    }

    fn comp_p(raw: u32, c: &mut Calib) -> f32 {
        let tf = c.t_fine as f32;
        let v1 = tf / 2.0 - 64000.0;
        let v2 = v1 * v1 * c.p6 as f32 / 32768.0 + v1 * c.p5 as f32 * 2.0;
        let v2 = v2 / 4.0 + c.p4 as f32 * 65536.0;
        let v3 = c.p3 as f32 * v1 * v1 / 524288.0;
        let v1 = (v3 + c.p2 as f32 * v1) / 524288.0;
        let v1 = (1.0 + v1 / 32768.0) * c.p1 as f32;
        if v1 > 0.0 {
            let p = 1048576.0 - raw as f32;
            let p = (p - v2 / 4096.0) * 6250.0 / v1;
            let v1 = c.p9 as f32 * p * p / 2147483648.0;
            let v2 = p * c.p8 as f32 / 32768.0;
            (p + (v1 + v2 + c.p7 as f32) / 16.0) / 100.0
        } else {
            0.0
        }
    }

    fn comp_h(raw: u32, c: &mut Calib) -> f32 {
        let tf = c.t_fine as f32;
        let v1 = tf - 76800.0;
        let v2 = c.h4 as f32 * 64.0 + (c.h5 as f32 / 16384.0) * v1;
        let v3 = raw as f32 - v2;
        let v4 = c.h2 as f32 / 65536.0;
        let v5 = 1.0 + (c.h3 as f32 / 67108864.0) * v1;
        let v6 = 1.0 + (c.h6 as f32 / 67108864.0) * v1 * v5;
        let h = v3 * v4 * (v5 * v6) * (1.0 - c.h1 as f32 * v3 * v4 * (v5 * v6) / 524288.0);
        h.clamp(0.0, 100.0)
    }
}

pub struct Ds3231 {
    addr: u8,
}

impl Ds3231 {
    pub fn new() -> Self {
        Self { addr: 0x68 }
    }

    /// 从 0x00 起连续读 7 字节时间寄存器，BCD → 十进制。
    pub async fn get_time(&self, bus: &I2cBus) -> Result<DateTime, i2c::Error> {
        let mut d = [0u8; 7];
        bus.lock().await.write_read(self.addr, &[0x00], &mut d).await?;
        Ok(DateTime {
            second: Self::bcd(d[0]),
            minute: Self::bcd(d[1]),
            hour: Self::bcd(d[2] & 0x3F), // 24h 模式
            weekday: d[3] & 0x07,
            day: Self::bcd(d[4]),
            month: Self::bcd(d[5] & 0x1F),
            year: 2000 + Self::bcd(d[6]) as u16,
        })
    }

    /// NTP 同步后写回 RTC（首字节 0x00 为起始寄存器地址）。
    pub async fn set_time(&self, bus: &I2cBus, dt: &DateTime) -> Result<(), i2c::Error> {
        bus.lock().await.write(
            self.addr,
            &[
                0x00,
                Self::dec(dt.second),
                Self::dec(dt.minute),
                Self::dec(dt.hour),
                dt.weekday & 0x07,
                Self::dec(dt.day),
                Self::dec(dt.month),
                Self::dec((dt.year % 100) as u8),
            ],
        ).await
    }

    fn bcd(v: u8) -> u8 {
        (v & 0x0F) + (v >> 4) * 10
    }

    fn dec(v: u8) -> u8 {
        (v % 10) | ((v / 10) << 4)
    }
}
