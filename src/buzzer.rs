use embassy_rp::gpio::Output;
use embassy_time::{Duration, Timer};

pub const BIRTHDAY_SONG: [(u32, u32); 25] = [
    (262, 400),
    (262, 400),
    (294, 800),
    (262, 800),
    (349, 800),
    (330, 1600),
    (262, 400),
    (262, 400),
    (294, 800),
    (262, 800),
    (392, 800),
    (349, 1600),
    (262, 400),
    (262, 400),
    (523, 800),
    (440, 800),
    (349, 800),
    (330, 800),
    (294, 1600),
    (466, 400),
    (466, 400),
    (440, 800),
    (349, 800),
    (392, 800),
    (349, 1600),
];

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
