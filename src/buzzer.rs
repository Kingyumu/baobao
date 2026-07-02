use embassy_rp::gpio::Output;
use embassy_time::{Duration, Timer};

pub const BIRTHDAY_SONG: [(u32, u32); 25] = [
    (262, 400), (262, 400), (294, 800), (262, 800), (349, 800), (330, 1600),
    (262, 400), (262, 400), (294, 800), (262, 800), (392, 800), (349, 1600),
    (262, 400), (262, 400), (523, 800), (440, 800), (349, 800), (330, 800), (294, 1600),
    (466, 400), (466, 400), (440, 800), (349, 800), (392, 800), (349, 1600),
];

pub const VALENTINE_MELODY: [(u32, u32); 10] = [
    (523, 300), (659, 300), (784, 600), (659, 300), (784, 600),
    (880, 400), (784, 400), (659, 400), (523, 800), (0, 200),
];

pub const CHRISTMAS_MELODY: [(u32, u32); 12] = [
    (659, 200), (659, 200), (659, 400), (659, 200), (659, 200), (659, 400),
    (659, 200), (784, 200), (523, 300), (587, 100), (659, 800), (0, 200),
];

pub const NEW_YEAR_MELODY: [(u32, u32); 8] = [
    (523, 200), (587, 200), (659, 200), (784, 200),
    (880, 400), (988, 400), (1047, 800), (0, 200),
];

pub const ANNIVERSARY_MELODY: [(u32, u32); 8] = [
    (392, 400), (494, 400), (587, 400), (659, 800),
    (587, 400), (494, 400), (392, 800), (0, 200),
];

pub const ALARM_MELODY: [(u32, u32); 6] = [
    (880, 300), (0, 100), (880, 300), (0, 100), (880, 300), (0, 300),
];

pub fn melody_for_event(message: &str) -> &'static [(u32, u32)] {
    if message.contains("生日") {
        &BIRTHDAY_SONG
    } else if message.contains("情人节") {
        &VALENTINE_MELODY
    } else if message.contains("圣诞") {
        &CHRISTMAS_MELODY
    } else if message.contains("新年") {
        &NEW_YEAR_MELODY
    } else {
        &ANNIVERSARY_MELODY
    }
}

pub async fn beep(pin: &mut Output<'static>, ms: u64) {
    pin.set_high();
    Timer::after(Duration::from_millis(ms)).await;
    pin.set_low();
}

pub async fn melody(pin: &mut Output<'static>, notes: &[(u32, u32)]) {
    for &(freq, dur) in notes {
        if freq > 0 {
            let period: u64 = 1_000_000 / freq as u64;
            let cycles = (dur as u64 * 1000) / period;
            for _ in 0..cycles {
                pin.set_high();
                Timer::after(Duration::from_micros(period / 2)).await;
                pin.set_low();
                Timer::after(Duration::from_micros(period / 2)).await;
            }
        } else {
            Timer::after(Duration::from_millis(dur as u64)).await;
        }
    }
}

pub async fn hourly_chime(pin: &mut Output<'static>) {
    beep(pin, 40).await;
    Timer::after(Duration::from_millis(80)).await;
    beep(pin, 60).await;
}

pub async fn alert_beep(pin: &mut Output<'static>) {
    for _ in 0..3 {
        beep(pin, 80).await;
        Timer::after(Duration::from_millis(120)).await;
    }
}
