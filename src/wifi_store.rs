//! WiFi 凭据 Flash 持久化（最后一扇区 4KB）。

use crate::config;
use defmt::*;
use embassy_rp::flash::{Blocking, Error, Flash, ERASE_SIZE, FLASH_BASE};
use heapless::String;

pub const FLASH_SIZE: usize = 4096 * 1024;
const STORE_OFFSET: u32 = (FLASH_SIZE - ERASE_SIZE) as u32;
const MAGIC: u32 = 0x5749_4649; // "WIFI"

#[repr(C)]
struct WifiStoreRaw {
    magic: u32,
    ssid_len: u8,
    pass_len: u8,
    _pad: [u8; 2],
    ssid: [u8; 32],
    password: [u8; 64],
}

pub struct WifiCredentials {
    pub ssid: String<32>,
    pub password: String<64>,
}

impl WifiCredentials {
    pub fn ssid_str(&self) -> &str {
        self.ssid.as_str()
    }

    pub fn password_str(&self) -> &str {
        self.password.as_str()
    }
}

fn read_raw() -> WifiStoreRaw {
    let ptr = (FLASH_BASE as u32 + STORE_OFFSET) as *const WifiStoreRaw;
    unsafe { core::ptr::read_volatile(ptr) }
}

fn raw_to_creds(raw: &WifiStoreRaw) -> Option<WifiCredentials> {
    if raw.magic != MAGIC {
        return None;
    }
    let ssid_len = raw.ssid_len as usize;
    let pass_len = raw.pass_len as usize;
    if ssid_len == 0 || ssid_len > 32 || pass_len > 64 {
        return None;
    }
    let ssid = core::str::from_utf8(&raw.ssid[..ssid_len]).ok()?;
    let password = core::str::from_utf8(&raw.password[..pass_len]).ok()?;
    let mut s = String::new();
    let mut p = String::new();
    s.push_str(ssid).ok()?;
    p.push_str(password).ok()?;
    Some(WifiCredentials { ssid: s, password: p })
}

pub fn load() -> Option<WifiCredentials> {
    raw_to_creds(&read_raw())
}

/// 开发用默认凭据（config 里填了真实 SSID 时生效）。
pub fn factory_credentials() -> Option<WifiCredentials> {
    if config::WIFI_SSID == "你的WiFi名称" {
        return None;
    }
    let mut ssid = String::new();
    let mut password = String::new();
    ssid.push_str(config::WIFI_SSID).ok()?;
    password.push_str(config::WIFI_PASSWORD).ok()?;
    Some(WifiCredentials { ssid, password })
}

pub fn credentials_to_use() -> Option<WifiCredentials> {
    load().or_else(factory_credentials)
}

pub fn save(
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
    creds: &WifiCredentials,
) -> Result<(), Error> {
    let ssid_bytes = creds.ssid.as_bytes();
    let pass_bytes = creds.password.as_bytes();
    if ssid_bytes.is_empty() || ssid_bytes.len() > 32 || pass_bytes.len() > 64 {
        return Err(Error::Other);
    }

    let mut raw = WifiStoreRaw {
        magic: MAGIC,
        ssid_len: ssid_bytes.len() as u8,
        pass_len: pass_bytes.len() as u8,
        _pad: [0xFF; 2],
        ssid: [0xFF; 32],
        password: [0xFF; 64],
    };
    raw.ssid[..ssid_bytes.len()].copy_from_slice(ssid_bytes);
    raw.password[..pass_bytes.len()].copy_from_slice(pass_bytes);

    flash.blocking_erase(STORE_OFFSET, STORE_OFFSET + ERASE_SIZE as u32)?;
    flash.blocking_write(STORE_OFFSET, unsafe {
        core::slice::from_raw_parts(
            &raw as *const WifiStoreRaw as *const u8,
            core::mem::size_of::<WifiStoreRaw>(),
        )
    })?;
    info!("WiFi 凭据已写入 Flash");
    Ok(())
}

pub fn clear(
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
) -> Result<(), Error> {
    flash.blocking_erase(STORE_OFFSET, STORE_OFFSET + ERASE_SIZE as u32)?;
    info!("WiFi 凭据已清除");
    Ok(())
}

pub fn sys_reset() -> ! {
    info!("设备重启...");
    cortex_m::peripheral::SCB::sys_reset()
}
