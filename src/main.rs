#![no_std]
#![no_main]
#![allow(linker_messages)]

//! 桌面天气站主程序 — Raspberry Pi Pico 2 W (RP2350)
//!
//! ## 程序结构
//! 1. 初始化硬件（I2C / SPI 屏 / WiFi / Flash）
//! 2. 尝试连接已记住的 WiFi；失败则进入 [`provisioning`] 配网（此阶段不启动 BLE）
//! 3. 进入主循环：读传感器 → 更新状态 → 按需联网 → 刷新屏幕
//!
//! ## Rust / Embassy 实战要点
//! - `#![no_std]`：不用标准库，适合 MCU；需要堆时用 `extern crate alloc`
//! - `#![no_main]`：入口由 `#[embassy_executor::main]` 宏生成，不是普通 `fn main()`
//! - `async/await`：等待 I2C、WiFi、定时器时不阻塞 CPU，Embassy 协作式调度
//! - `'static`：spawn 出去的任务、GPIO 引脚要求「整个程序生命周期有效」
//! - [`StaticCell`]：在运行时安全地给 `static` 变量做**一次性**初始化
//! - [`Mutex`] + `.lock().await`：BME280 与 DS3231 共用 I2C 时的异步互斥锁
//! - [`Option`] / [`match`]：Rust 用类型系统表达「可能有值」，避免空指针
//! - `-> !`（如 [`wifi_store::sys_reset`]）：表示函数永不返回

extern crate alloc;

mod ble;
mod buzzer;
mod config;
mod display;
mod i2c_bus;
mod network;
mod provisioning;
mod render;
mod sensors;
mod state;
mod wifi_store;

use ble::publish_snapshot;
use buzzer::{alert_beep, beep, hourly_chime, melody, melody_for_event, ALARM_MELODY};
use defmt::*;
use display::{draw_special_event, face_color, theme_for_hour, Face, FaceType, Ili9488Display};
use embassy_executor::Spawner;
use embassy_rp::flash::Flash;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::i2c::{self, Config as I2cConfig};
use embassy_rp::spi::{Config as SpiConfig, Spi};
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    prelude::*,
    text::Text,
};
use i2c_bus::{I2cBus, I2cIrqs};
use network::{connect_wifi, fetch_weather, init_wifi, sync_ntp};
use render::RenderCache;
use sensors::{Bme280, DateTime, Ds3231};
use state::SystemState;
use static_cell::StaticCell;
use trouble_host::prelude::ExternalController;
use panic_probe as _;

#[global_allocator]
// 堆分配器：供 alloc::format! 等少量动态内存使用（131072 = 128 KiB）
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

/// 长按触摸时播放爱心动画（局部重绘，不整屏刷新）。
async fn show_touch_heart(display: &mut Ili9488Display, hour: u8) {
    let bg = theme_for_hour(hour).bg;
    let mut heart = Face::new(FaceType::Heart);
    for _ in 0..4 {
        display.fill_rect(380, 215, 32, 32, bg.into_storage());
        heart.draw_scaled(display, 380, 215, 4, face_color(FaceType::Heart));
        Timer::after_millis(250).await;
        heart.next_frame();
    }
}

/// 纪念日/生日：蜂鸣旋律 + 全屏提示 + 爱心动画，每天只触发一次（由 state 去重）。
async fn handle_special_event(
    display: &mut Ili9488Display,
    buzzer: &mut Output<'static>,
    cache: &mut RenderCache,
    state: &mut SystemState,
) {
    let ev = match state.special_event {
        Some(ev) if state.should_handle_special_event() => ev,
        _ => return,
    };

    info!("特殊事件: {}", ev);
    state.mark_special_event_handled();

    melody(buzzer, melody_for_event(ev)).await;

    draw_special_event(display, ev);
    let mut heart = Face::new(FaceType::Heart);
    for _ in 0..5 {
        heart.draw_scaled(display, 220, 200, 6, face_color(FaceType::Heart));
        Timer::after_millis(500).await;
        heart.next_frame();
    }
    cache.invalidate();
}

/// BLE 协议栈独立任务：与主循环并行，通过 [`ble::publish_snapshot`] 读传感器快照。
#[embassy_executor::task]
async fn ble_host_task(bt_device: cyw43::bluetooth::BtDriver<'static>) {
    let controller: ExternalController<_, 10> = ExternalController::new(bt_device);
    ble::run(controller).await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 初始化堆：MaybeUninit 避免在 const 上下文要求未初始化数组为 0
    {
        use core::mem::MaybeUninit;
        static mut HEAP_MEM: [MaybeUninit<u8>; 131072] = [MaybeUninit::uninit(); 131072];
        #[allow(static_mut_refs)]
        unsafe {
            HEAP.init(HEAP_MEM.as_mut_ptr() as usize, 131072);
        }
    }

    let p = embassy_rp::init(Default::default());
    info!("桌面天气站启动中...");

    // I2C 总线用 Mutex 包装：多个 async 调用方串行访问，避免并发读写冲突
    let i2c = i2c::I2c::new_async(p.I2C0, p.PIN_1, p.PIN_0, I2cIrqs, I2cConfig::default());
    static I2C_BUS: StaticCell<I2cBus> = StaticCell::new();
    let bus = I2C_BUS.init(Mutex::new(i2c));

    // SPI 屏用 blocking 模式：draw 在主循环里同步刷像素，实现简单
    let spi = Spi::new_blocking(p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, SpiConfig::default());
    let dc = Output::new(p.PIN_8, Level::Low);
    let cs = Output::new(p.PIN_9, Level::Low);

    let mut display = Ili9488Display::new(spi, dc, cs);
    display.init();
    let boot_theme = theme_for_hour(12);
    display.clear(boot_theme.bg);
    {
        let mut loading = Face::new(FaceType::Loading);
        for _ in 0..6 {
            display.clear(boot_theme.bg);
            loading.draw_scaled(&mut display, 216, 120, 6, face_color(FaceType::Loading));
            let ts = MonoTextStyleBuilder::new()
                .font(&FONT_10X20)
                .text_color(boot_theme.text)
                .build();
            Text::new("天气站启动中...宝宝我爱你❤", Point::new(140, 220), ts)
                .draw(&mut display)
                .ok();
            loading.next_frame();
            Timer::after_millis(300).await;
        }
    }

    let mut bme = Bme280::new();
    match bme.init(bus).await {
        Ok(()) => info!("BME280 初始化成功"),
        Err(e) => warn!("BME280 初始化失败: {:?}", e),
    }

    let rtc = Ds3231::new();
    let mut buzzer = Output::new(p.PIN_6, Level::Low);
    let touch = Input::new(p.PIN_7, Pull::Down);
    let mut flash = Flash::new_blocking(p.FLASH);

    let (mut wifi_control, stack, bt_device) = init_wifi(
        &spawner,
        p.PIO0,
        p.DMA_CH0,
        p.DMA_CH1,
        p.PIN_23,
        p.PIN_24,
        p.PIN_25,
        p.PIN_29,
    )
    .await;

    let wifi_connected = connect_wifi(&mut wifi_control, stack, &mut flash).await;

    // 全部记住的网络都连不上 → 开热点配网；`run` 成功后会 reboot，正常不会返回
    if !wifi_connected {
        provisioning::run(
            &mut display,
            &mut wifi_control,
            stack,
            &spawner,
            &mut flash,
            &mut buzzer,
        )
        .await;
    }

    // 仅联网成功后才启动 BLE，配网阶段关闭蓝牙以降低复杂度
    if config::BLE_ENABLED {
        if let Some(bt) = bt_device {
            spawner.spawn(ble_host_task(bt).unwrap());
            info!("BLE 传感器广播已启动");
        }
    }

    let mut state = SystemState::new();
    let mut render_cache = RenderCache::new();
    state.wifi_connected = wifi_connected;

    if wifi_connected && sync_ntp(stack, &rtc, bus).await {
        state.last_ntp_sync = 0;
    }

    if wifi_connected {
        match fetch_weather(stack, config::CITY_CODE).await {
            Some(w) => {
                info!(
                    "本地天气: {}°C {} [{}]",
                    w.temp,
                    w.text.as_str(),
                    w.weather_code.as_str()
                );
                state.set_weather_code(w.weather_code.as_str());
                state.network_weather = Some(w);
            }
            None => warn!("首次本地天气获取失败"),
        }
        match fetch_weather(stack, config::PARTNER_CITY_CODE).await {
            Some(w) => {
                info!(
                    "对方天气: {}°C {} [{}]",
                    w.temp,
                    w.text.as_str(),
                    w.weather_code.as_str()
                );
                state.partner_weather = Some(w);
            }
            None => warn!("首次对方天气获取失败"),
        }
    }

    Timer::after_secs(1).await;
    info!("桌面天气站启动成功！");
    beep(&mut buzzer, 100).await;
    Timer::after_millis(50).await;
    beep(&mut buzzer, 100).await;

    // 触摸状态机：短按切页 / 2s 长按爱心 / 5s 长按清除 WiFi 并重启
    let mut touch_pressed = false;
    let mut touch_down: Option<Instant> = None;
    let mut touch_long_done = false;
    let mut touch_clear_done = false;
    let mut last_wifi_reconnect = 0u64;
    let long_press = Duration::from_millis(config::TOUCH_LONG_PRESS_MS);
    let clear_wifi_press = Duration::from_millis(config::TOUCH_CLEAR_WIFI_MS);

    loop {
        let loop_start = embassy_time::Instant::now().as_secs();

        // 链路断开时按间隔重连，会依次尝试 Flash 里记住的全部 WiFi
        let link_up = stack.is_config_up();
        state.wifi_connected = link_up;
        if !link_up {
            if loop_start.saturating_sub(last_wifi_reconnect) >= config::WIFI_RECONNECT_INTERVAL {
                last_wifi_reconnect = loop_start;
                info!("WiFi 断开，尝试重连...");
                if connect_wifi(&mut wifi_control, stack, &mut flash).await {
                    state.wifi_connected = true;
                    state.last_ntp_sync = 0;
                    state.last_weather_update = 0;
                    render_cache.invalidate();
                }
            }
        }

        match bme.measure(bus).await {
            Ok(m) => {
                state.temperature = m.temperature;
                state.humidity = m.humidity;
                state.update_pressure(m.pressure);
                if config::BLE_ENABLED {
                    publish_snapshot(m.temperature, m.humidity, m.pressure);
                }
            }
            Err(e) => warn!("BME280 读取失败: {:?}", e),
        }

        match rtc.get_time(bus).await {
            Ok(t) => state.update_time(t),
            Err(e) => warn!("DS3231 读取失败: {:?}", e),
        }

        state.sample_temp_chart(loop_start);

        if let Some(t) = state.current_time {
            if state.check_hourly_chime(&t) {
                hourly_chime(&mut buzzer).await;
            }
            if state.check_alarm(&t) {
                melody(&mut buzzer, &ALARM_MELODY).await;
            }
        }

        if state.check_weather_alert(loop_start) {
            state.activate_weather_alert(loop_start);
            alert_beep(&mut buzzer).await;
            render_cache.invalidate();
        }
        state.tick_weather_alert(loop_start);

        let touch_hour = state
            .current_time
            .map(|t| t.hour)
            .unwrap_or(12);

        if touch.is_high() {
            if !touch_pressed {
                touch_pressed = true;
                touch_down = Some(Instant::now());
                touch_long_done = false;
                touch_clear_done = false;
            } else if !touch_clear_done && !touch_long_done {
                if let Some(down) = touch_down {
                    let held = down.elapsed();
                    if held >= clear_wifi_press {
                        info!("清除 WiFi 配置，进入配网模式...");
                        beep(&mut buzzer, 100).await;
                        Timer::after_millis(80).await;
                        beep(&mut buzzer, 100).await;
                        let _ = wifi_store::clear(&mut flash);
                        wifi_store::sys_reset();
                    } else if held >= long_press {
                        touch_long_done = true;
                        beep(&mut buzzer, 30).await;
                        show_touch_heart(&mut display, touch_hour).await;
                        render_cache.invalidate();
                    }
                }
            }
        } else if touch_pressed {
            if !touch_long_done && !touch_clear_done {
                state.next_page();
                info!("切换页面: {}", state.display_page.label());
                beep(&mut buzzer, 50).await;
                render_cache.invalidate();
            }
            touch_pressed = false;
            touch_down = None;
        }

        handle_special_event(&mut display, &mut buzzer, &mut render_cache, &mut state).await;

        if state.wifi_connected
            && loop_start - state.last_weather_update >= config::WEATHER_UPDATE_INTERVAL
        {
            let mut updated = false;
            if let Some(w) = fetch_weather(stack, config::CITY_CODE).await {
                info!(
                    "本地天气: {}°C {} [{}]",
                    w.temp,
                    w.text.as_str(),
                    w.weather_code.as_str()
                );
                state.set_weather_code(w.weather_code.as_str());
                state.network_weather = Some(w);
                updated = true;
            } else {
                warn!("本地天气获取失败");
            }
            if let Some(w) = fetch_weather(stack, config::PARTNER_CITY_CODE).await {
                info!(
                    "对方天气: {}°C {} [{}]",
                    w.temp,
                    w.text.as_str(),
                    w.weather_code.as_str()
                );
                state.partner_weather = Some(w);
                updated = true;
            } else {
                warn!("对方天气获取失败");
            }
            if updated {
                state.last_weather_update = loop_start;
            }
        }

        if state.wifi_connected && loop_start - state.last_ntp_sync >= config::NTP_SYNC_INTERVAL {
            if sync_ntp(stack, &rtc, bus).await {
                state.last_ntp_sync = loop_start;
            }
        }

        let time = state.current_time.unwrap_or(DateTime {
            year: 2026,
            month: 1,
            day: 1,
            weekday: 3,
            hour: 0,
            minute: 0,
            second: 0,
        });

        render_cache.update(&mut display, &state, &time);

        state.animation_counter += 1;
        Timer::after_secs(config::DISPLAY_UPDATE_INTERVAL).await;
    }
}
