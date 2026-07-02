#![no_std]
#![no_main]
#![allow(linker_messages)]

//! 桌面天气站主程序 — Raspberry Pi Pico 2 W (RP2350)

extern crate alloc;

mod buzzer;
mod comfort;
mod config;
mod display;
mod i2c_bus;
mod network;
mod render;
mod sensors;
mod state;

use buzzer::{alert_beep, beep, hourly_chime, melody, melody_for_event, ALARM_MELODY};
use defmt::*;
use display::{draw_special_event, face_color, theme_for_hour, Face, FaceType, Ili9488Display};
use embassy_executor::Spawner;
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
use panic_probe as _;

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    {
        use core::mem::MaybeUninit;
        static mut HEAP_MEM: [MaybeUninit<u8>; 65536] = [MaybeUninit::uninit(); 65536];
        #[allow(static_mut_refs)]
        unsafe {
            HEAP.init(HEAP_MEM.as_mut_ptr() as usize, 65536);
        }
    }

    let p = embassy_rp::init(Default::default());
    info!("桌面天气站启动中...");

    let i2c = i2c::I2c::new_async(p.I2C0, p.PIN_1, p.PIN_0, I2cIrqs, I2cConfig::default());
    static I2C_BUS: StaticCell<I2cBus> = StaticCell::new();
    let bus = I2C_BUS.init(Mutex::new(i2c));

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
            Text::new("天气站启动中...", Point::new(140, 220), ts)
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

    let (mut wifi_control, stack, wifi_connected) = init_wifi(
        &spawner,
        p.PIO0,
        p.DMA_CH0,
        p.PIN_23,
        p.PIN_24,
        p.PIN_25,
        p.PIN_29,
    )
    .await;

    let mut state = SystemState::new();
    let mut render_cache = RenderCache::new();
    state.wifi_connected = wifi_connected;

    if wifi_connected && sync_ntp(stack, &rtc, bus).await {
        state.last_ntp_sync = 0;
    }

    if wifi_connected {
        match fetch_weather(stack).await {
            Some(w) => {
                info!(
                    "天气: {}°C {} [{}]",
                    w.temp,
                    w.text.as_str(),
                    w.weather_code.as_str()
                );
                state.set_weather_code(w.weather_code.as_str());
                state.network_weather = Some(w);
            }
            None => warn!("首次天气获取失败"),
        }
    }

    Timer::after_secs(1).await;
    info!("桌面天气站启动成功！");
    beep(&mut buzzer, 100).await;
    Timer::after_millis(50).await;
    beep(&mut buzzer, 100).await;

    let mut touch_pressed = false;
    let mut touch_down: Option<Instant> = None;
    let mut touch_long_done = false;
    let mut last_wifi_reconnect = 0u64;
    let long_press = Duration::from_millis(config::TOUCH_LONG_PRESS_MS);

    loop {
        let loop_start = embassy_time::Instant::now().as_secs();

        let link_up = stack.is_config_up();
        state.wifi_connected = link_up;
        if !link_up {
            if loop_start.saturating_sub(last_wifi_reconnect) >= config::WIFI_RECONNECT_INTERVAL {
                last_wifi_reconnect = loop_start;
                info!("WiFi 断开，尝试重连...");
                if connect_wifi(&mut wifi_control, stack).await {
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
            }
            Err(e) => warn!("BME280 读取失败: {:?}", e),
        }

        match rtc.get_time(bus).await {
            Ok(t) => state.update_time(t),
            Err(e) => warn!("DS3231 读取失败: {:?}", e),
        }

        state.sample_pressure_chart(loop_start);

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
            } else if !touch_long_done {
                if let Some(down) = touch_down {
                    if down.elapsed() >= long_press {
                        touch_long_done = true;
                        beep(&mut buzzer, 30).await;
                        show_touch_heart(&mut display, touch_hour).await;
                        render_cache.invalidate();
                    }
                }
            }
        } else if touch_pressed {
            if !touch_long_done {
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
            match fetch_weather(stack).await {
                Some(w) => {
                    info!(
                        "天气: {}°C {} [{}]",
                        w.temp,
                        w.text.as_str(),
                        w.weather_code.as_str()
                    );
                    state.set_weather_code(w.weather_code.as_str());
                    state.network_weather = Some(w);
                    state.last_weather_update = loop_start;
                }
                None => warn!("天气获取失败"),
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
