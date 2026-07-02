//! SoftAP + DHCP + 简易 HTTP 配网页面。

use crate::config;
use crate::display::{draw_provisioning_screen, draw_provisioning_success};
use crate::display::Ili9488Display;
use crate::network::{configure_ap_stack, start_ap, WifiConnectError};
use crate::wifi_store::{self, WifiCredentials, FLASH_SIZE};
use cyw43::Control;
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::gpio::Output;
use embassy_time::{Duration, Timer};
use esp_hal_dhcp_server::simple_leaser::SimpleDhcpLeaser;
use esp_hal_dhcp_server::structs::DhcpServerConfig;
use esp_hal_dhcp_server::{run_dhcp_server, Ipv4Addr};
use heapless::String;

const HTTP_PORT: u16 = 80;

#[embassy_executor::task]
async fn dhcp_server_task(stack: Stack<'static>) {
    let gateway = Ipv4Addr::new(
        config::PROVISION_IP.0,
        config::PROVISION_IP.1,
        config::PROVISION_IP.2,
        config::PROVISION_IP.3,
    );
    let config = DhcpServerConfig {
        ip: gateway,
        lease_time: Duration::from_secs(3600),
        gateways: &[gateway],
        subnet: None,
        dns: &[],
        use_captive_portal: true,
    };
    let mut leaser = SimpleDhcpLeaser {
        start: Ipv4Addr::new(192, 168, 4, 10),
        end: Ipv4Addr::new(192, 168, 4, 50),
        leases: Default::default(),
    };
    info!("DHCP 服务已启动");
    let _ = run_dhcp_server(stack, config, &mut leaser).await;
}

pub async fn run(
    display: &mut Ili9488Display,
    control: &mut Control<'static>,
    stack: Stack<'static>,
    spawner: &Spawner,
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
    buzzer: &mut Output<'static>,
) -> ! {
    info!("进入 WiFi 配网模式");
    draw_provisioning_screen(display);

    configure_ap_stack(stack);
    start_ap(control).await;
    spawner.spawn(dhcp_server_task(stack).unwrap());

    let mut rx_buf = [0u8; 2048];
    let mut tx_buf = [0u8; 2048];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(15)));

        if socket.accept(HTTP_PORT).await.is_err() {
            continue;
        }

        let mut req = [0u8; 2048];
        let mut total = 0usize;
        while total < req.len() {
            match socket.read(&mut req[total..]).await {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(_) => break,
            }
            if total >= 4 && req[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                if !is_post(&req[..total]) || body_complete(&req[..total]) {
                    break;
                }
            }
        }

        if let Some(creds) = parse_save_post(&req[..total]) {
            match try_save_and_connect(control, stack, flash, &creds).await {
                Ok(()) => {
                    draw_provisioning_success(display);
                    crate::buzzer::beep(buzzer, 80).await;
                    Timer::after_millis(80).await;
                    crate::buzzer::beep(buzzer, 80).await;
                    send_html(&mut socket, SUCCESS_HTML).await;
                    Timer::after_secs(2).await;
                    wifi_store::sys_reset();
                }
                Err(e) => {
                    warn!("配网失败: {:?}", e);
                    start_ap(control).await;
                    configure_ap_stack(stack);
                    send_html(&mut socket, &fail_html(e)).await;
                }
            }
        } else if is_get(&req[..total]) {
            let path = request_path(&req[..total]);
            if is_captive_probe(path) {
                send_redirect(&mut socket).await;
            } else {
                send_html(&mut socket, FORM_HTML).await;
            }
        } else {
            send_html(&mut socket, FORM_HTML).await;
        }

        let _ = socket.close();
    }
}

async fn try_save_and_connect(
    control: &mut Control<'static>,
    stack: Stack<'static>,
    flash: &mut Flash<'_, embassy_rp::peripherals::FLASH, Blocking, FLASH_SIZE>,
    creds: &WifiCredentials,
) -> Result<(), WifiConnectError> {
    crate::network::stop_ap(control).await;
    crate::network::configure_sta_stack(stack);
    crate::network::connect_wifi_with(
        control,
        stack,
        creds.ssid_str(),
        creds.password_str(),
        config::PROVISION_CONNECT_ATTEMPTS,
    )
    .await?;
    wifi_store::remember(flash, creds).map_err(|_| WifiConnectError::JoinFailed)?;
    Ok(())
}

fn is_get(req: &[u8]) -> bool {
    req.starts_with(b"GET ")
}

fn is_post(req: &[u8]) -> bool {
    req.starts_with(b"POST ")
}

fn request_path(req: &[u8]) -> &str {
    let text = core::str::from_utf8(req).unwrap_or("");
    text.split_whitespace().nth(1).unwrap_or("/")
}

fn is_captive_probe(path: &str) -> bool {
    matches!(
        path,
        "/generate_204"
            | "/gen_204"
            | "/hotspot-detect.html"
            | "/library/test/success.html"
            | "/connecttest.txt"
            | "/ncsi.txt"
    )
}

fn body_complete(req: &[u8]) -> bool {
    let Some(header_end) = find_header_end(req) else {
        return false;
    };
    let body = &req[header_end..];
    let content_len = parse_content_length(req).unwrap_or(0);
    body.len() >= content_len
}

fn find_header_end(req: &[u8]) -> Option<usize> {
    req.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

fn parse_content_length(req: &[u8]) -> Option<usize> {
    let text = core::str::from_utf8(req).ok()?;
    for line in text.lines() {
        if line.len() >= 15 && line[..15].eq_ignore_ascii_case("content-length:") {
            let v = line.split(':').nth(1)?.trim();
            return v.parse().ok();
        }
    }
    None
}

fn parse_form(body: &str) -> Option<(String<32>, String<64>)> {
    let mut ssid = String::<32>::new();
    let mut pass = String::<64>::new();
    for pair in body.split('&') {
        if let Some(v) = pair.strip_prefix("ssid=") {
            decode_form_value(v, &mut ssid)?;
        } else if let Some(v) = pair.strip_prefix("pass=") {
            decode_form_value(v, &mut pass)?;
        }
    }
    if ssid.is_empty() {
        return None;
    }
    Some((ssid, pass))
}

fn decode_form_value<const N: usize>(src: &str, out: &mut String<N>) -> Option<()> {
    for ch in src.chars() {
        let c = if ch == '+' { ' ' } else { ch };
        out.push(c).ok()?;
    }
    Some(())
}

fn parse_save_post(req: &[u8]) -> Option<WifiCredentials> {
    if !is_post(req) {
        return None;
    }
    let path = request_path(req);
    if !path.starts_with("/save") {
        return None;
    }
    let header_end = find_header_end(req)?;
    let body = core::str::from_utf8(&req[header_end..]).ok()?;
    let (ssid, pass) = parse_form(body)?;
    Some(WifiCredentials { ssid, password: pass })
}

async fn send_html(socket: &mut TcpSocket<'_>, html: &str) {
    send_bytes(socket, html.as_bytes()).await;
}

async fn send_redirect(socket: &mut TcpSocket<'_>) {
    let resp = "HTTP/1.0 302 Found\r\nLocation: http://192.168.4.1/\r\nConnection: close\r\n\r\n";
    send_bytes(socket, resp.as_bytes()).await;
}

async fn send_bytes(socket: &mut TcpSocket<'_>, data: &[u8]) {
    let mut off = 0;
    while off < data.len() {
        match socket.write(&data[off..]).await {
            Ok(0) => break,
            Ok(n) => off += n,
            Err(_) => break,
        }
    }
}

fn fail_html(err: WifiConnectError) -> String<512> {
    let msg = match err {
        WifiConnectError::JoinFailed => "无法连接该 WiFi，请检查名称和密码",
        WifiConnectError::DhcpTimeout => "已连接但获取 IP 失败，请重试",
    };
    let mut html = String::new();
    let _ = core::fmt::write(
        &mut html,
        format_args!(
            "HTTP/1.0 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
            <!DOCTYPE html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">\
            <title>配网失败</title></head><body>\
            <h2>连接失败</h2><p>{}</p><p><a href=/ >返回重试</a></p></body></html>",
            msg
        ),
    );
    html
}

const FORM_HTML: &str = concat!(
    "HTTP/1.0 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n",
    "<!DOCTYPE html><html><head><meta charset=utf-8>",
    "<meta name=viewport content=\"width=device-width,initial-scale=1\">",
    "<title>天气站配网</title>",
    "<style>body{font-family:sans-serif;margin:24px;max-width:420px}",
    "input{width:100%;padding:10px;margin:8px 0;box-sizing:border-box;font-size:16px}",
    "button{width:100%;padding:12px;font-size:16px;background:#2563eb;color:#fff;border:0;border-radius:8px}</style>",
    "</head><body>",
    "<h2>桌面天气站 WiFi 配网</h2>",
    "<p>请填写家里 WiFi 的名称和密码。</p>",
    "<form method=POST action=/save>",
    "<label>WiFi 名称</label><input name=ssid maxlength=32 required autocomplete=off>",
    "<label>WiFi 密码</label><input name=pass type=password maxlength=64 autocomplete=off>",
    "<button type=submit>保存并连接</button>",
    "</form></body></html>"
);

const SUCCESS_HTML: &str = concat!(
    "HTTP/1.0 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n",
    "<!DOCTYPE html><html><head><meta charset=utf-8>",
    "<meta name=viewport content=\"width=device-width,initial-scale=1\">",
    "<title>配网成功</title></head><body>",
    "<h2>配网成功</h2><p>设备即将重启，请稍候...</p></body></html>"
);
