use embassy_rp::bind_interrupts;
use embassy_rp::i2c::{self, Async as I2cAsync, InterruptHandler as I2cIrq};
use embassy_rp::peripherals::I2C0;
use embassy_sync::mutex::Mutex;

bind_interrupts!(pub struct I2cIrqs {
    I2C0_IRQ => I2cIrq<I2C0>;
});

pub type I2cBus = Mutex<embassy_sync::blocking_mutex::raw::NoopRawMutex, i2c::I2c<'static, I2C0, I2cAsync>>;
