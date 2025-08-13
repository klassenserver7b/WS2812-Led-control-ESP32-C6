//! Datasheet (PDF) for a WS2812B, which explains how the pulses are to be sent:
//! https://cdn-shop.adafruit.com/datasheets/WS2812B.pdf

#![allow(unknown_lints)]
#![allow(unexpected_cfgs)]

use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Result};
use esp_idf_hal::rmt::VariableLengthSignal;
use esp_idf_hal::{
    delay::FreeRtos,
    prelude::Peripherals,
    rmt::{config::TransmitConfig, PinState, Pulse, TxRmtDriver},
};

use embedded_svc::wifi::AuthMethod;
use embedded_svc::wifi::{ClientConfiguration, Configuration};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};
use log::{info, warn};

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");

fn main() -> Result<(), anyhow::Error> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // `async-io` uses the ESP IDF `eventfd` syscall to implement async IO.
    // If you use `tokio`, you still have to do the same as it also uses the `eventfd` syscall
    let _mounted_eventfs = esp_idf_svc::io::vfs::MountedEventfs::mount(5)?;

    // This thread is necessary because the ESP IDF main task thread is running with a very low priority that cannot be raised
    // (lower than the hidden posix thread in `async-io`)
    // As a result, the main thread is constantly starving because of the higher prio `async-io` thread
    //
    // To use async networking IO, make your `main()` minimal by just spawning all work in a new thread
    thread::Builder::new()
        .stack_size(60000)
        .spawn(run_main)
        .unwrap()
        .join()
        .unwrap()
        .unwrap();

    Ok(())
}

pub fn run_main() -> Result<()> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let config = TransmitConfig::new().clock_divider(1);

    // Onboard RGB LED pin
    let mut tx_onboard =
        TxRmtDriver::new(peripherals.rmt.channel0, peripherals.pins.gpio8, &config)?;
    let timings_ws2812 = [350, 800, 700, 600];
    let onboard_led_state = Arc::new(RwLock::new(Vec::with_capacity(1)));
    onboard_led_state.write().unwrap().push(Rgb::new(8, 0, 0));

    // RGB Stripe pin
    let mut tx_stripe = TxRmtDriver::new(peripherals.rmt.channel1, peripherals.pins.gpio9, &config)?;

    let timings_ws2812b = [400, 800, 850, 450];
    let rgb_stripe_state = Arc::new(RwLock::new(Vec::with_capacity(50)));

    // cyan at 100% brightness
    for _ in 0..50 {
        rgb_stripe_state
            .write()
            .unwrap()
            .push(Rgb::from_hsv(150, 100, 13)?);
    }

    send_led_signal(
        &onboard_led_state.read().unwrap(),
        &mut tx_onboard,
        &timings_ws2812,
    )?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;
    connect_wifi(&mut wifi)?;

    onboard_led_state.write().unwrap()[0] = Rgb::new(8, 0, 4);
    send_led_signal(
        &onboard_led_state.read().unwrap(),
        &mut tx_onboard,
        &timings_ws2812,
    )?;

    core::mem::forget(wifi);

    send_led_signal(
        &rgb_stripe_state.read().unwrap(),
        &mut tx_stripe,
        &timings_ws2812b,
    )?;

    let onboard_led_clone = onboard_led_state.clone();
    let rgb_stripe_clone = rgb_stripe_state.clone();

    let _server = create_udp_server(
        onboard_led_clone,
        rgb_stripe_clone,
        tx_onboard,
        tx_stripe,
        timings_ws2812,
        timings_ws2812b,
    );

    loop {
        FreeRtos::delay_ms(50);
    }
}

fn create_udp_server(
    onboard_led_state_lock: Arc<RwLock<Vec<Rgb>>>,
    rgb_stripe_state_lock: Arc<RwLock<Vec<Rgb>>>,
    mut tx_onboard: TxRmtDriver,
    mut tx_stripe: TxRmtDriver,
    timings_ws2812: [u64; 4],
    timings_ws2812b: [u64; 4],
) -> Result<(), anyhow::Error> {
    let addr = "0.0.0.0:5568".to_socket_addrs()?.next().unwrap();
    let udp_socket = UdpSocket::bind(addr)?;

    info!("Created UDP server on {}", addr);

    onboard_led_state_lock.write().unwrap()[0] = Rgb::new(0, 0, 8);
    send_led_signal(
        &onboard_led_state_lock.read().unwrap(),
        &mut tx_onboard,
        &timings_ws2812,
    )?;

    loop {
        let mut buf = [0u8; 638];
        let (size, addr) = udp_socket.recv_from(&mut buf)?;
        info!("Received {} bytes from {}", size, addr);

        if !(125..=638).contains(&size) {
            warn!("Received invalid packet size: {}", size);
            continue;
        }

        let universe = u16::from_be_bytes(buf[113..=114].try_into().unwrap());

        let property_value_count = u16::from_be_bytes(buf[123..=124].try_into().unwrap());

        if size < 125 + property_value_count as usize {
            warn!(
                "Received packet with insufficient size for property values: {}",
                size
            );
            continue;
        }
        let property_values = &buf[125..(125 + property_value_count as usize)];

        {
            let mut rgb_stripe_state = rgb_stripe_state_lock.write().unwrap();
            info!(
                "updating rgb leds based on universe {} from {}",
                universe, addr
            );

            for (i, chunk) in property_values.chunks(3).enumerate() {
                if i >= rgb_stripe_state.len() {
                    info!(
                        "got data for more than {} leds ({} values)",
                        i,
                        property_value_count - 1
                    );
                    break;
                }
                rgb_stripe_state[i] =
                    Rgb::from_slice(chunk.try_into().expect("slice with incorrect length"));
            }
        }
        info!("updating rgb stripe color");

        send_led_signal(
            &rgb_stripe_state_lock.read().unwrap(),
            &mut tx_stripe,
            &timings_ws2812b,
        )?;

        info!("updated rgb stripe color");
    }
}

fn send_led_signal(rgb: &[Rgb], tx: &mut TxRmtDriver, timings: &[u64; 4]) -> Result<()> {
    let ticks_hz = tx.counter_clock()?;
    let (t0h, t0l, t1h, t1l) = (
        Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(timings[0]))?,
        Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(timings[1]))?,
        Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(timings[2]))?,
        Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(timings[3]))?,
    );
    let mut signal = VariableLengthSignal::new();
    for color in rgb {
        // Convert RGB to u32 color value
        let color: u32 = color.into();
        // Each color is 24 bits, so we need 24 pulses
        for i in (0..24).rev() {
            let p = 2_u32.pow(i);
            let bit: bool = p & color != 0;
            let (high_pulse, low_pulse) = if bit { (t1h, t1l) } else { (t0h, t0l) };
            signal.push(&[high_pulse, low_pulse])?;
        }
    }

    tx.start_blocking(&signal)?;
    Ok(())
}
#[derive(Copy, Clone)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn from_slice(rgb: &[u8; 3]) -> Self {
        Self {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
        }
    }

    /// Converts hue, saturation, value to RGB
    #[allow(dead_code)]
    pub fn from_hsv(h: u32, s: u32, v: u32) -> Result<Self> {
        if h > 360 || s > 100 || v > 100 {
            bail!("The given HSV values are not in valid range");
        }
        let s = s as f64 / 100.0;
        let v = v as f64 / 100.0;
        let c = s * v;
        let x = c * (1.0 - (((h as f64 / 60.0) % 2.0) - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match h {
            0..=59 => (c, x, 0.0),
            60..=119 => (x, c, 0.0),
            120..=179 => (0.0, c, x),
            180..=239 => (0.0, x, c),
            240..=299 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Ok(Self {
            r: ((r + m) * 255.0) as u8,
            g: ((g + m) * 255.0) as u8,
            b: ((b + m) * 255.0) as u8,
        })
    }
}

impl From<&Rgb> for u32 {
    /// Convert RGB to u32 color value
    ///
    /// e.g. rgb: (1,2,4)
    /// G        R        B
    /// 7      0 7      0 7      0
    /// 00000010 00000001 00000100
    fn from(rgb: &Rgb) -> Self {
        (rgb.g as u32) | ((rgb.r as u32) << 8) | ((rgb.b as u32) << 16)
    }
}

fn connect_wifi(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: PASSWORD.try_into().unwrap(),
        channel: None,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)?;

    wifi.start()?;
    info!("Wifi started");

    wifi.connect()?;
    info!("Wifi connected");

    wifi.wait_netif_up()?;
    info!("Wifi netif up");

    Ok(())
}
