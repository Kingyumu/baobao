//! BLE GATT 外设：广播并推送 BME280 温湿度与气压。

use crate::config;
use core::cell::Cell;
use cyw43::bluetooth::BtDriver;
use defmt::*;
use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::Timer;
use static_cell::StaticCell;
use trouble_host::prelude::*;

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;

/// 广播包中的 128-bit UUID（小端序）
const SERVICE_UUID_LE: [u8; 16] = [
    0xfb, 0x34, 0x9b, 0x5f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00, 0xb1,
    0xa7,
];

#[derive(Clone)]
struct SensorSnapshot {
    temperature: Cell<f32>,
    humidity: Cell<f32>,
    pressure: Cell<f32>,
}

static SNAPSHOT: Mutex<CriticalSectionRawMutex, SensorSnapshot> = Mutex::new(SensorSnapshot {
    temperature: Cell::new(0.0),
    humidity: Cell::new(0.0),
    pressure: Cell::new(0.0),
});

pub fn publish_snapshot(temp: f32, hum: f32, press: f32) {
    SNAPSHOT.lock(|s| {
        s.temperature.set(temp);
        s.humidity.set(hum);
        s.pressure.set(press);
    });
}

fn snapshot_bytes() -> [u8; 12] {
    SNAPSHOT.lock(|s| {
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&s.temperature.get().to_le_bytes());
        buf[4..8].copy_from_slice(&s.humidity.get().to_le_bytes());
        buf[8..12].copy_from_slice(&s.pressure.get().to_le_bytes());
        buf
    })
}

#[gatt_server]
struct Server {
    weather: WeatherService,
}

#[gatt_service(uuid = "a7b10001-0000-1000-8000-00805f9b34fb")]
struct WeatherService {
    /// 12 字节：温度(f32 LE, °C)、湿度(f32 LE, %)、气压(f32 LE, hPa)
    #[characteristic(uuid = "a7b10002-0000-1000-8000-00805f9b34fb", read, notify, value = [0u8; 12])]
    environment: [u8; 12],
}

type BleController = ExternalController<BtDriver<'static>, 10>;

pub async fn run(controller: BleController) {
    let address = Address::random([0xba, 0x0b, 0x00, 0x01, 0x00, 0x01]);
    info!("BLE 地址: {:?}", address);

    static RESOURCES: StaticCell<
        HostResources<BleController, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>,
    > = StaticCell::new();
    let resources = RESOURCES.init(HostResources::new());

    let stack = trouble_host::new(controller, resources)
        .set_random_address(address)
        .build();
    let runner = stack.runner();
    let mut peripheral = stack.peripheral();

    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: config::BLE_DEVICE_NAME,
        appearance: &appearance::sensor::GENERIC_SENSOR,
    }))
    .unwrap();

    let _ = join(ble_runner_task(runner), async {
        loop {
            match advertise(&mut peripheral, &server).await {
                Ok(conn) => {
                    info!("BLE 已连接");
                    select(notify_task(&server, &conn), gatt_events_task(&server, &conn)).await;
                    info!("BLE 连接断开");
                }
                Err(e) => {
                    warn!("BLE 广播失败: {:?}", defmt::Debug2Format(&e));
                    Timer::after_secs(5).await;
                }
            }
        }
    })
    .await;
}

async fn ble_runner_task(mut runner: Runner<'_, BleController, DefaultPacketPool>) {
    loop {
        if let Err(e) = runner.run().await {
            warn!("BLE runner 错误: {:?}", defmt::Debug2Format(&e));
            Timer::after_secs(1).await;
        }
    }
}

async fn advertise<'values, 'server>(
    peripheral: &mut Peripheral<'values, BleController, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<cyw43::bluetooth::Error>> {
    let mut adv_data = [0u8; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteServiceUuids128(&[SERVICE_UUID_LE]),
            AdStructure::CompleteLocalName(config::BLE_DEVICE_NAME.as_bytes()),
        ],
        &mut adv_data[..],
    )?;

    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..len],
                scan_data: &[],
            },
        )
        .await?;

    info!("BLE 广播中: {}", config::BLE_DEVICE_NAME);
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    Ok(conn)
}

async fn gatt_events_task<'values, 'server>(
    server: &Server<'values>,
    conn: &GattConnection<'values, 'server, DefaultPacketPool>,
) -> Result<(), Error> {
    let env = server.weather.environment;
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                info!("BLE 断开: {:?}", reason);
                break;
            }
            GattConnectionEvent::Gatt { event } => {
                let reply = match event {
                    GattEvent::Read(read) if read.handle() == env.handle => {
                        let data = snapshot_bytes();
                        read.accept_unprocessed(&data)
                    }
                    GattEvent::Write(_) | GattEvent::Read(_) => event.accept(),
                    _ => event.accept(),
                };
                if let Ok(reply) = reply {
                    let _ = reply.send().await;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn notify_task<'values, 'server>(
    server: &Server<'values>,
    conn: &GattConnection<'values, 'server, DefaultPacketPool>,
) {
    let env = server.weather.environment;
    loop {
        let data = snapshot_bytes();
        if env.notify(conn, &data, false).await.is_err() {
            break;
        }
        Timer::after_secs(config::BLE_NOTIFY_INTERVAL_SECS).await;
    }
}
