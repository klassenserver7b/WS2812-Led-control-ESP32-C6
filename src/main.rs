//! Datasheet (PDF) for a WS2812B, which explains how the pulses are to be sent:
//! https://cdn-shop.adafruit.com/datasheets/WS2812B.pdf

#![allow(unknown_lints)]
#![allow(unexpected_cfgs)]

use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{bail, Result};
use esp_idf_hal::rmt::VariableLengthSignal;
use esp_idf_hal::{
    delay::FreeRtos,
    prelude::Peripherals,
    rmt::{config::TransmitConfig, PinState, Pulse, TxRmtDriver},
};

use embedded_svc::wifi::{ClientConfiguration, Configuration};
use embedded_svc::{
    http::{Headers, Method},
    io::{Read, Write},
    wifi::AuthMethod,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::server::EspHttpServer,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};

use log::info;
use serde::Deserialize;

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");

// Max payload length
const MAX_LEN: usize = 768;

// Need lots of stack to parse JSON
const STACK_SIZE: usize = 10240;

pub fn main() -> Result<()> {
    esp_idf_hal::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    connect_wifi(&mut wifi)?;

    let mut server = create_server()?;

    let led_state = Arc::new(RwLock::new(Vec::new()));

    // 3 seconds white at 100% brightness
    for _ in 0..50 {
        led_state.write().unwrap().push(Rgb::new(255, 255, 255));
    }

    let led_clone = led_state.clone();
    server.fn_handler::<anyhow::Error, _>("/post", Method::Post, move |mut req| {
        let len = req.content_len().unwrap_or(0) as usize;

        if len > MAX_LEN {
            req.into_status_response(413)?
                .write_all("Request too big".as_bytes())?;
            return Ok(());
        }

        let mut buf = vec![0; len];
        req.read_exact(&mut buf)?;
        let mut resp = req.into_ok_response()?;

        if let Ok(form) = serde_json::from_slice::<FormData>(&buf) {
            let mut led_state = led_clone.write().unwrap();
            led_state.clear();

            if form.rainbow {
                // Generate a rainbow effect
                for i in 0..360 {
                    let rgb = Rgb::from_hsv(i, 100, 100)?;
                    led_state.push(rgb);
                }
            } else {
                for led in form.ledstates {
                    led_state.push(Rgb::new(led[0], led[1], led[2]));
                }
            }
        } else {
            resp.write_all("JSON error".as_bytes())?;
        }

        Ok(())
    })?;

    // Keep wifi and the server running beyond when main() returns (forever)
    // Do not call this if you ever want to stop or access them later.
    // Otherwise you can either add an infinite loop so the main task
    // never returns, or you can move them to another thread.
    // https://doc.rust-lang.org/stable/core/mem/fn.forget.html
    core::mem::forget(wifi);
    core::mem::forget(server);

    // Onboard RGB LED pin
    // ESP32-C3-DevKitC-02 gpio8, ESP32-C3-DevKit-RUST-1 gpio2
    let led = peripherals.pins.gpio9;
    let channel = peripherals.rmt.channel0;
    let config = TransmitConfig::new().clock_divider(1);
    let mut tx = TxRmtDriver::new(channel, led, &config)?;

    loop {
        FreeRtos::delay_ms(100);
        neopixel(&led_state.read().unwrap(), &mut tx)?
    }
}

fn neopixel(rgb: &Vec<Rgb>, tx: &mut TxRmtDriver) -> Result<()> {
    let ticks_hz = tx.counter_clock()?;
    let (t0h, t0l, t1h, t1l) = (
        Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(400))?,
        Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(800))?,
        Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(850))?,
        Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(450))?,
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
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
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

#[derive(Deserialize)]
struct FormData {
    rainbow: bool,
    ledstates: Vec<[u8; 3]>,
}

fn create_server() -> Result<EspHttpServer<'static>> {
    let server_configuration = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    Ok(EspHttpServer::new(&server_configuration)?)
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
