//! WiFi 凭据 Flash 持久化（最后一扇区 4KB，最多记住 5 个网络，最近使用的优先）。

use crate::config;
use defmt::*;
use embassy_rp::flash::{Blocking, Error, Flash, ERASE_SIZE, FLASH_BASE};
use heapless::String;
use heapless::Vec;

pub const FLASH_SIZE: usize = 4096 * 1024;
const STORE_OFFSET: u32 = (FLASH_SIZE - ERASE_SIZE) as u32;
const MAGIC: u32 = 0x5746_4D32; // "WFM2"
pub const MAX_NETWORKS: usize = 5;

#[repr(C)]
#[derive(Copy, Clone)]
struct WifiEntryRaw {
    ssid_len: u8,
    pass_len: u8,
    _pad: [u8; 2],
    ssid: [u8; 32],
    password: [u8; 64],
}

#[repr(C)]
struct WifiStoreRaw {
    magic: u32,
    count: u8,
    _pad: [u8; 3],
    entries: [WifiEntryRaw; MAX_NETWORKS],
}

#[derive(Clone)]
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

fn entry_raw_to_creds(raw: &WifiEntryRaw) -> Option<WifiCredentials> {
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

fn creds_to_entry_raw(creds: &WifiCredentials) -> Result<WifiEntryRaw, Error> {
    let ssid_bytes = creds.ssid.as_bytes();
    let pass_bytes = creds.password.as_bytes();
    if ssid_bytes.is_empty() || ssid_bytes.len() > 32 || pass_bytes.len() > 64 {
        return Err(Error::Other);
    }
    let mut raw = WifiEntryRaw {
        ssid_len: ssid_bytes.len() as u8,
        pass_len: pass_bytes.len() as u8,
        _pad: [0xFF; 2],
        ssid: [0xFF; 32],
        password: [0xFF; 64],
    };
    raw.ssid[..ssid_bytes.len()].copy_from_slice(ssid_bytes);
    raw.password[..pass_bytes.len()].copy_from_slice(pass_bytes);
    Ok(raw)
}

fn load_from_flash() -> Vec<WifiCredentials, MAX_NETWORKS> {
    let mut list = Vec::new();
    let raw = read_raw();
    if raw.magic != MAGIC {
        return list;
    }
    let count = (raw.count as usize).min(MAX_NETWORKS);
    for entry in raw.entries.iter().take(count) {
        if let Some(creds) = entry_raw_to_creds(entry) {
            let _ = list.push(creds);
        }
    }
    list
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

/// 返回已保存的全部 WiFi（最近使用的在前）。
pub fn all_credentials() -> Vec<WifiCredentials, MAX_NETWORKS> {
    let list = load_from_flash();
    if list.is_empty() {
        if let Some(c) = factory_credentials() {
            let mut out = Vec::new();
            let _ = out.push(c);
            return out;
        }
    }
    list
}

fn write_store(
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
    entries: &Vec<WifiCredentials, MAX_NETWORKS>,
) -> Result<(), Error> {
    let mut raw = WifiStoreRaw {
        magic: MAGIC,
        count: entries.len() as u8,
        _pad: [0xFF; 3],
        entries: [WifiEntryRaw {
            ssid_len: 0,
            pass_len: 0,
            _pad: [0xFF; 2],
            ssid: [0xFF; 32],
            password: [0xFF; 64],
        }; MAX_NETWORKS],
    };
    for (i, creds) in entries.iter().enumerate() {
        raw.entries[i] = creds_to_entry_raw(creds)?;
    }

    flash.blocking_erase(STORE_OFFSET, STORE_OFFSET + ERASE_SIZE as u32)?;
    flash.blocking_write(STORE_OFFSET, unsafe {
        core::slice::from_raw_parts(
            &raw as *const WifiStoreRaw as *const u8,
            core::mem::size_of::<WifiStoreRaw>(),
        )
    })?;
    Ok(())
}

/// 记住一条 WiFi：同 SSID 更新密码并置顶，新 SSID 插入队首，超出容量丢弃最旧的。
pub fn remember(
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
    creds: &WifiCredentials,
) -> Result<(), Error> {
    let mut list = load_from_flash();
    list.retain(|e| e.ssid != creds.ssid);
    list.insert(0, creds.clone()).map_err(|_| Error::Other)?;
    while list.len() > MAX_NETWORKS {
        list.pop();
    }
    write_store(flash, &list)?;
    info!("WiFi 已记住: {} (共 {} 条)", creds.ssid_str(), list.len());
    Ok(())
}

pub fn clear(
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
) -> Result<(), Error> {
    flash.blocking_erase(STORE_OFFSET, STORE_OFFSET + ERASE_SIZE as u32)?;
    info!("WiFi 记忆已清除");
    Ok(())
}

pub fn sys_reset() -> ! {
    info!("设备重启...");
    cortex_m::peripheral::SCB::sys_reset()
}
