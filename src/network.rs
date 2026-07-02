use crate::config;
use crate::i2c_bus::I2cBus;
use crate::wifi_store;
use crate::sensors::Ds3231;
use crate::state::NetworkWeather;
use cyw43::{Aligned, A4, Cyw43439};
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select;
use embassy_net::udp::{PacketMetadata, UdpMetadata, UdpSocket};
use embassy_rp::bind_interrupts;
use embassy_rp::dma::InterruptHandler as DmaIrq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, PIO0};
use embassy_rp::pio::InterruptHandler as PioIrq;
use embassy_rp::{dma::Channel, pio::Pio, Peri};
use embassy_time::{Duration, Timer};
use fixed::traits::ToFixed;
use static_cell::StaticCell;

bind_interrupts!(struct PioIrqs {
    PIO0_IRQ_0 => PioIrq<PIO0>;
});

bind_interrupts!(struct DmaIrqs {
    DMA_IRQ_0 => DmaIrq<DMA_CH0>, DmaIrq<DMA_CH1>;
});

static BTFW: Aligned<A4, [u8; 6164]> = Aligned({
    const RAW: [u8; 6164] = *cyw43_firmware::CYW43_43439A0_BTFW;
    RAW
});

pub async fn init_wifi(
    spawner: &Spawner,
    pio: Peri<'static, PIO0>,
    dma0: Peri<'static, DMA_CH0>,
    dma1: Peri<'static, DMA_CH1>,
    p23: Peri<'static, embassy_rp::peripherals::PIN_23>,
    p24: Peri<'static, embassy_rp::peripherals::PIN_24>,
    p25: Peri<'static, embassy_rp::peripherals::PIN_25>,
    p29: Peri<'static, embassy_rp::peripherals::PIN_29>,
) -> (
    cyw43::Control<'static>,
    embassy_net::Stack<'static>,
    Option<cyw43::bluetooth::BtDriver<'static>>,
) {
    let fw = &cyw43_setup::FW;
    let clm = cyw43_setup::CLM;
    let nvram = &cyw43_setup::NVRAM;

    let pwr = Output::new(p23, Level::Low);
    let cs = Output::new(p25, Level::High);
    let mut pio = Pio::new(pio, PioIrqs);
    let divider: u32 = (150_000_000 / 1_000_000) as u32;
    let dma_ch0 = Channel::new(dma0, DmaIrqs);
    let dma_ch1 = Channel::new(dma1, DmaIrqs);
    let spi = cyw43_pio::PioSpi::new(
        &mut pio.common,
        pio.sm0,
        divider.to_fixed(),
        pio.irq0,
        cs,
        p24,
        p29,
        dma_ch0,
        dma_ch1,
    );

    static CYW43_STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = CYW43_STATE.init(cyw43::State::new());

    let (net_device, bt_device, mut control, runner) = if config::BLE_ENABLED {
        let (net, bt, ctrl, run) =
            cyw43::new_with_bluetooth(state, pwr, spi, fw, &BTFW, nvram).await;
        (net, Some(bt), ctrl, run)
    } else {
        let (net, ctrl, run) = cyw43::new(state, pwr, spi, fw, nvram).await;
        (net, None, ctrl, run)
    };

    spawner.spawn(cyw43_task(runner).unwrap());

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = 0x12345678u64;

    static RESOURCES: StaticCell<embassy_net::StackResources<5>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(embassy_net::StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner).unwrap());

    (control, stack, bt_device)
}

#[derive(Debug, defmt::Format)]
pub enum WifiConnectError {
    JoinFailed,
    DhcpTimeout,
}

pub fn configure_ap_stack(stack: embassy_net::Stack<'static>) {
    let ip = embassy_net::Ipv4Address::new(
        config::PROVISION_IP.0,
        config::PROVISION_IP.1,
        config::PROVISION_IP.2,
        config::PROVISION_IP.3,
    );
    stack.set_config_v4(embassy_net::ConfigV4::Static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(ip, 24),
        gateway: Some(ip),
        dns_servers: heapless::Vec::new(),
    }));
}

pub fn configure_sta_stack(stack: embassy_net::Stack<'static>) {
    stack.set_config_v4(embassy_net::ConfigV4::Dhcp(Default::default()));
}

pub async fn start_ap(control: &mut cyw43::Control<'static>) {
    control
        .start_ap_open(config::PROVISION_AP_SSID, config::PROVISION_AP_CHANNEL)
        .await;
    info!("配网热点已开启: {}", config::PROVISION_AP_SSID);
}

pub async fn stop_ap(control: &mut cyw43::Control<'static>) {
    control.close_ap().await;
}

/// 使用指定凭据连接 WiFi 并等待 DHCP。
pub async fn connect_wifi_with(
    control: &mut cyw43::Control<'static>,
    stack: embassy_net::Stack<'static>,
    ssid: &str,
    password: &str,
    attempts: u32,
) -> Result<(), WifiConnectError> {
    let mut joined = false;
    for _ in 0..attempts {
        match control
            .join(ssid, cyw43::JoinOptions::new(password.as_bytes()))
            .await
        {
            Ok(_) => {
                joined = true;
                break;
            }
            Err(err) => {
                info!("WiFi 连接失败，重试中: {:?}", err);
                Timer::after_secs(5).await;
            }
        }
    }
    if !joined {
        return Err(WifiConnectError::JoinFailed);
    }

    let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_secs(15);
    while !stack.is_config_up() {
        if embassy_time::Instant::now() >= deadline {
            return Err(WifiConnectError::DhcpTimeout);
        }
        Timer::after_millis(100).await;
    }
    info!("WiFi 已连接");
    Ok(())
}

/// 尝试连接 Flash 中保存的 WiFi（用于启动与断线重连）。
pub async fn connect_wifi(
    control: &mut cyw43::Control<'static>,
    stack: embassy_net::Stack<'static>,
) -> bool {
    let creds = match wifi_store::credentials_to_use() {
        Some(c) => c,
        None => return false,
    };
    configure_sta_stack(stack);
    connect_wifi_with(
        control,
        stack,
        creds.ssid_str(),
        creds.password_str(),
        config::PROVISION_CONNECT_ATTEMPTS,
    )
    .await
    .is_ok()
}

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<
        'static,
        cyw43::SpiBus<Output<'static>, cyw43_pio::PioSpi<'static, PIO0, 0>>,
        Cyw43439,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) {
    runner.run().await;
}

pub async fn sync_ntp(
    stack: embassy_net::Stack<'static>,
    rtc: &Ds3231,
    bus: &I2cBus,
) -> bool {
    info!("开始 NTP 时间同步...");

    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buf = [0u8; 512];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_buf = [0u8; 512];
    let sock = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    let addr = match stack
        .dns_query(config::NTP_SERVER, embassy_net::dns::DnsQueryType::A)
        .await
    {
        Ok(addrs) if !addrs.is_empty() => addrs[0],
        _ => {
            warn!("DNS 解析失败");
            return false;
        }
    };

    let ntp_addr = UdpMetadata::from(embassy_net::IpEndpoint::new(addr.into(), 123));
    let mut ntp_buf = [0u8; 48];
    ntp_buf[0] = 0x23;

    if sock.send_to(&ntp_buf, ntp_addr).await.is_err() {
        warn!("NTP 请求发送失败");
        return false;
    }

    let mut rx_buf = [0u8; 48];
    let timeout = Timer::after_secs(5);
    let result = select::select(
        async {
            let (len, _) = sock
                .recv_from(&mut rx_buf)
                .await
                .unwrap_or((0, ntp_addr));
            len
        },
        timeout,
    )
    .await;

    match result {
        select::Either::First(len) if len >= 48 => {
            let seconds = u32::from_be_bytes([
                rx_buf[40], rx_buf[41], rx_buf[42], rx_buf[43],
            ]);
            let unix_time = seconds.wrapping_sub(2208988800u32);
            let beijing_time = unix_time + 8 * 3600;
            let (year, month, day, hour, minute, second, weekday) =
                timestamp_to_datetime(beijing_time);
            let dt = crate::sensors::DateTime {
                year,
                month,
                day,
                hour,
                minute,
                second,
                weekday,
            };
            match rtc.set_time(bus, &dt).await {
                Ok(_) => {
                    info!(
                        "NTP 同步成功: {}-{}-{} {}:{}:{}",
                        year, month, day, hour, minute, second
                    );
                    return true;
                }
                Err(e) => warn!("DS3231 写入失败: {:?}", e),
            }
        }
        _ => warn!("NTP 响应超时"),
    }
    false
}

fn timestamp_to_datetime(ts: u32) -> (u16, u8, u8, u8, u8, u8, u8) {
    let seconds = ts % 60;
    let minutes = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    let days_total = ts / 86400;
    let mut year = 1970u16;
    let mut remaining = days_total;
    loop {
        let d = if is_leap_year(year) { 366 } else { 365 };
        if remaining < d {
            break;
        }
        remaining -= d;
        year += 1;
    }
    let dm = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u8;
    for &d in &dm {
        if remaining < d as u32 {
            break;
        }
        remaining -= d as u32;
        month += 1;
    }
    let day = (remaining + 1) as u8;
    let weekday = ((days_total + 4) % 7) as u8;
    (
        year,
        month,
        day,
        hours as u8,
        minutes as u8,
        seconds as u8,
        weekday,
    )
}

fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub async fn fetch_weather(stack: embassy_net::Stack<'static>) -> Option<NetworkWeather> {
    info!("获取网络天气...");

    let addr = match stack
        .dns_query("d1.weather.com.cn", embassy_net::dns::DnsQueryType::A)
        .await
    {
        Ok(addrs) if !addrs.is_empty() => addrs[0],
        _ => {
            warn!("DNS 解析失败");
            return None;
        }
    };

    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 4096];
    let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(Duration::from_secs(10)));
    if socket
        .connect(embassy_net::IpEndpoint::new(addr.into(), 80))
        .await
        .is_err()
    {
        warn!("TCP 连接失败");
        return None;
    }

    let request = alloc::format!(
        "GET /weather1d/{}.shtml HTTP/1.1\r\nHost: d1.weather.com.cn\r\nConnection: close\r\n\r\n",
        config::CITY_CODE
    );
    if socket.write(request.as_bytes()).await.is_err() {
        warn!("HTTP 请求发送失败");
        return None;
    }

    let mut buf = [0u8; 4096];
    let mut found = false;
    let mut total = 0;
    let mut json_data = heapless::String::<4096>::new();

    loop {
        match socket.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let chunk = core::str::from_utf8(&buf[..n]).unwrap_or("");
                if !found {
                    if let Some(pos) = chunk.find("observe24h_data = {") {
                        found = true;
                        for &b in chunk[pos + 19..].as_bytes() {
                            if json_data.push(b as char).is_err() {
                                break;
                            }
                        }
                    }
                } else {
                    for &b in chunk.as_bytes() {
                        if json_data.push(b as char).is_err() {
                            break;
                        }
                    }
                    if chunk.contains("};") {
                        break;
                    }
                }
                total += n;
                if total > 32768 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    socket.close();
    if !found || json_data.is_empty() {
        return None;
    }
    parse_weather_json(&json_data)
}

fn parse_weather_json(json: &str) -> Option<NetworkWeather> {
    let (hour, temp, humidity, precip) = parse_latest_observation(json)?;

    let mut text = heapless::String::<32>::new();
    let desc = weather_desc(temp, humidity, precip);
    text.push_str(desc).ok();

    let mut weather_code = heapless::String::<8>::new();
    weather_code
        .push_str(infer_weather_code(temp, humidity, precip, hour))
        .ok();

    Some(NetworkWeather {
        temp,
        humidity,
        text,
        weather_code,
    })
}

/// 取 observe24h_data 中最新一条小时观测（od2 数组最后一项）。
fn parse_latest_observation(json: &str) -> Option<(u8, f32, f32, f32)> {
    let mut latest = None;
    for segment in json.split("\"od21\"").skip(1) {
        let hour = parse_od21_value(segment)?;
        let temp = extract_f32_field(segment, "od22")?;
        let precip = extract_f32_field(segment, "od26").unwrap_or(0.0);
        let humidity = extract_f32_field(segment, "od27")?;
        latest = Some((hour, temp, humidity, precip));
    }
    latest
}

fn parse_od21_value(segment: &str) -> Option<u8> {
    let rest = segment.trim().trim_start_matches(':').trim();
    let raw = rest.trim_start_matches('"').split('"').next()?.trim();
    raw.parse().ok()
}

fn extract_f32_field(block: &str, key: &str) -> Option<f32> {
    let pattern = alloc::format!("\"{}\":", key);
    let rest = block.split(&pattern).nth(1)?;
    let raw = rest
        .trim()
        .trim_start_matches('"')
        .split(['"', ',', '}'])
        .next()?
        .trim();
    raw.parse().ok()
}

fn weather_desc(temp: f32, humidity: f32, precip: f32) -> &'static str {
    if precip >= 2.0 || (precip > 0.0 && humidity >= 90.0) {
        "中雨"
    } else if precip > 0.0 || humidity >= 92.0 {
        "小雨"
    } else if humidity >= 85.0 {
        "阴"
    } else if humidity >= 75.0 {
        "多云"
    } else if humidity >= 60.0 {
        "多云"
    } else if temp > 35.0 {
        "晴热"
    } else {
        "晴"
    }
}

/// 根据温湿度与降水量推断天气图标码（d01~d17 / n01）。
fn infer_weather_code(temp: f32, humidity: f32, precip: f32, hour: u8) -> &'static str {
    if precip >= 2.0 {
        "d08"
    } else if precip > 0.0 || humidity >= 92.0 {
        "d07"
    } else if humidity >= 85.0 {
        "d04"
    } else if humidity >= 75.0 {
        "d03"
    } else if humidity >= 60.0 {
        "d02"
    } else if temp <= 2.0 {
        "d13"
    } else if temp > 35.0 {
        "d01"
    } else if hour >= 18 || hour < 6 {
        "n01"
    } else {
        "d01"
    }
}
