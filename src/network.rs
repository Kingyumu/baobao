use crate::config;
use crate::i2c_bus::I2cBus;
use crate::sensors::Ds3231;
use crate::state::NetworkWeather;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select;
use embassy_net::udp::{PacketMetadata, UdpMetadata, UdpSocket};
use embassy_rp::bind_interrupts;
use embassy_rp::dma::InterruptHandler as DmaIrq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::InterruptHandler as PioIrq;
use embassy_rp::{dma::Channel, pio::Pio, Peri};
use embassy_time::{Duration, Timer};
use fixed::traits::ToFixed;
use static_cell::StaticCell;

bind_interrupts!(struct PioIrqs {
    PIO0_IRQ_0 => PioIrq<PIO0>;
});

bind_interrupts!(struct DmaIrqs {
    DMA_IRQ_0 => DmaIrq<DMA_CH0>;
});

pub async fn init_wifi(
    spawner: &Spawner,
    pio: Peri<'static, PIO0>,
    dma: Peri<'static, DMA_CH0>,
    p23: Peri<'static, embassy_rp::peripherals::PIN_23>,
    p24: Peri<'static, embassy_rp::peripherals::PIN_24>,
    p25: Peri<'static, embassy_rp::peripherals::PIN_25>,
    p29: Peri<'static, embassy_rp::peripherals::PIN_29>,
) -> (cyw43::Control<'static>, embassy_net::Stack<'static>, bool) {
    let fw = &cyw43_setup::FW;
    let clm = cyw43_setup::CLM;
    let nvram = &cyw43_setup::NVRAM;

    let pwr = Output::new(p23, Level::Low);
    let cs = Output::new(p25, Level::High);
    let mut pio = Pio::new(pio, PioIrqs);
    let divider: u32 = (150_000_000 / 1_000_000) as u32;
    let dma_ch = Channel::new(dma, DmaIrqs);
    let spi = cyw43_pio::PioSpi::new(
        &mut pio.common,
        pio.sm0,
        divider.to_fixed(),
        pio.irq0,
        cs,
        p24,
        p29,
        dma_ch,
    );

    static CYW43_STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = CYW43_STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw, nvram).await;
    spawner.spawn(cyw43_task(runner).unwrap());

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = 0x12345678u64;

    static RESOURCES: StaticCell<embassy_net::StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(embassy_net::StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner).unwrap());

    let mut wifi_ok = false;
    for _ in 0..3 {
        match control
            .join(
                config::WIFI_SSID,
                cyw43::JoinOptions::new(config::WIFI_PASSWORD.as_bytes()),
            )
            .await
        {
            Ok(_) => {
                wifi_ok = true;
                break;
            }
            Err(err) => {
                info!("WiFi 连接失败，重试中: {:?}", err);
                Timer::after_secs(5).await;
            }
        }
    }
    if wifi_ok {
        let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_secs(10);
        while !stack.is_config_up() {
            if embassy_time::Instant::now() >= deadline {
                warn!("DHCP 获取 IP 超时");
                wifi_ok = false;
                break;
            }
            Timer::after_millis(100).await;
        }
    }
    if wifi_ok {
        info!("WiFi 已连接");
    } else {
        warn!("WiFi 连接失败，基础功能不受影响");
    }
    (control, stack, wifi_ok)
}

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<
        'static,
        cyw43::SpiBus<Output<'static>, cyw43_pio::PioSpi<'static, PIO0, 0>>,
    >,
) {
    runner.run().await;
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
    let mut temp = 0.0f32;
    let mut humidity = 0.0f32;
    let mut ft = false;
    let mut fh = false;
    for line in json.split(',') {
        let line = line.trim();
        if line.starts_with("\"od22\"") {
            if let Some(v) = line.split(':').nth(1) {
                if let Ok(t) = v.trim_matches('"').trim().parse::<f32>() {
                    temp = t;
                    ft = true;
                }
            }
        } else if line.starts_with("\"od27\"") {
            if let Some(v) = line.split(':').nth(1) {
                if let Ok(h) = v.trim_matches('"').trim().parse::<f32>() {
                    humidity = h;
                    fh = true;
                }
            }
        }
    }
    if ft && fh {
        let mut text = heapless::String::<32>::new();
        let desc = if humidity >= 90.0 {
            "雨"
        } else if humidity >= 80.0 {
            "阴"
        } else if humidity >= 60.0 {
            "多云"
        } else if temp > 35.0 {
            "晴热"
        } else {
            "晴"
        };
        text.push_str(desc).ok();
        Some(NetworkWeather {
            temp,
            humidity,
            text,
        })
    } else {
        None
    }
}
