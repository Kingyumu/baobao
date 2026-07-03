//! I2C 总线类型别名 — BME280 与 DS3231 共用 I2C0。
//!
//! ## Rust 要点
//! - [`bind_interrupts!`]：把硬件 IRQ 绑定到 Embassy 中断处理函数
//! - [`Mutex<NoopRawMutex, _>`]：单核 MCU 上用「空操作」锁即可，比 CriticalSection 更轻
//! - `'static`：I2C 驱动被 [`StaticCell`] 初始化后，整个程序期间有效

use embassy_rp::bind_interrupts;
use embassy_rp::i2c::{self, Async as I2cAsync, InterruptHandler as I2cIrq};
use embassy_rp::peripherals::I2C0;
use embassy_sync::mutex::Mutex;

bind_interrupts!(pub struct I2cIrqs {
    I2C0_IRQ => I2cIrq<I2C0>;
});

/// 全局 I2C 总线：外层 Mutex 保证同一时刻只有一个任务在读写。
pub type I2cBus = Mutex<embassy_sync::blocking_mutex::raw::NoopRawMutex, i2c::I2c<'static, I2C0, I2cAsync>>;
