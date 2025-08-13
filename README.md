# WS2812 LED Control for ESP32-C6

A Rust-based project for controlling WS2812(B) LED strips via the E1.31 protocol on ESP32-C6 microcontrollers. This project enables wireless LED control through applications like OpenRGB, xLights, or any other E1.31-compatible software.

## Features

- **E1.31 Protocol Support**: Receive lighting data via industry-standard E1.31 (sACN) protocol
- **Dual LED Control**: Supports both onboard RGB LED and external LED strips
- **Wi-Fi Connectivity**: Connect to your wireless network for remote control
- **WS2812/WS2812B Compatible**: Works with popular addressable LED strips
- **Real-time Updates**: Low-latency LED updates via UDP communication

## Hardware Requirements

- **ESP32-C6** development board
- **WS2812B LED strip** (up to 50 LEDs supported by default)
- **Power supply** appropriate for your LED count
- **Jumper wires** for connections

## Pin Configuration

| Component | GPIO Pin |
|-----------|----------|
| Onboard RGB LED | GPIO8 |
| External LED Strip | GPIO9 |

## Installation

### Prerequisites

Follow the setup instructions from the [esp-idf-template prerequisites](https://github.com/esp-rs/esp-idf-template#prerequisites) to install Rust, ESP-IDF toolchain, and required tools.

### Environment Setup

Create a `.env` file or set environment variables:
```bash
export WIFI_SSID="YourWiFiNetwork"
export WIFI_PASS="YourWiFiPassword"
```

### Building and Flashing

1. Clone the repository:
   ```bash
   git clone https://github.com/klassenserver7b/WS2812-Led-control-ESP32-C6.git
   cd WS2812-Led-control-ESP32-C6
   ```

2. Build and flash to your ESP32-C6:
   ```bash
   cargo espflash flash --target riscv32imac-esp-espidf --release --partition-table partitions.csv --monitor
   ```

## Configuration

### LED Count

To change the number of LEDs in your strip, modify the capacity in `src/main.rs`:

```rust
// Change from 50 to your desired LED count
let rgb_stripe_state = Arc::new(RwLock::new(Vec::with_capacity(50)));

// Update the initialization loop accordingly
for _ in 0..50 {  // Change to your LED count
    rgb_stripe_state
        .write()
        .unwrap()
        .push(Rgb::from_hsv(150, 100, 13)?);
}
```

### Different ESP32 Variants

For other ESP32 variants, refer to the [esp-idf-template prerequisites](https://github.com/esp-rs/esp-idf-template#prerequisites) for target configuration. 
You'll also need to check the [hardware notes](https://github.com/esp-rs/esp-idf-hal?tab=readme-ov-file#hardware-notes) to identify which GPIOs to avoid, and adapt the GPIO pin assignments in the code based on your specific board's onboard LED pin and available pins for external connections.

### WS2812 vs WS2812B Timing

The project includes timing configurations for both LED protocols:

**WS2812B (default for external strip):**
```rust
let timings_ws2812b = [400, 800, 850, 450];  // T0H, T0L, T1H, T1L in nanoseconds
```

**WS2812:**
```rust
let timings_ws2812 = [350, 800, 700, 600];   // T0H, T0L, T1H, T1L in nanoseconds
```

Check your LED strip and onboard LED datasheets to determine which timing protocol they use. To use WS2812 timing for your external strip, change the corresponding line in `src/main.rs`:
```rust
// From:
send_led_signal(&rgb_stripe_state.read().unwrap(), &mut tx_stripe, &timings_ws2812b)?;

// To:
send_led_signal(&rgb_stripe_state.read().unwrap(), &mut tx_stripe, &timings_ws2812)?;
```

see: [WS2812B Datasheet](https://cdn-shop.adafruit.com/datasheets/WS2812B.pdf)
and  [WS2812 Datasheet](https://cdn-shop.adafruit.com/datasheets/WS2812.pdf)

## Usage

### Network Configuration

1. The device will connect to your Wi-Fi network using the credentials provided
2. The onboard LED will change color to indicate connection status:
   - **Red**: Starting up
   - **Yellow**: Wi-Fi connected
   - **Blue**: UDP server ready

*Note: Actual colors may vary depending on your specific board's LED implementation. Test on your hardware to confirm the status colors.*
*WS2812 and WS2812B should both expect GRB but my onboard WS2812 LED for example expects BRG*

### E1.31 Protocol

- **Protocol**: E1.31 (sACN)
- **Port**: 5568 (UDP)
- **Universe**: Any (configurable in your lighting software)
- **Data Format**: RGB (3 bytes per LED)

### Compatible Software

- **OpenRGB**: Popular RGB lighting control software
- **xLights**: Professional Christmas lighting sequencer
- **QLC+**: Open source lighting control suite
- **Any E1.31 compatible software**

### OpenRGB Setup

1. Go to Settings in OpenRGB
2. Navigate to the E1.31 Devices tab
3. Add a new E1.31 device with the following configuration:
   - **Name**: ESP32-C6 Main (or your preferred name)
   - **IP Address**: Your ESP32's IP address
   - **Start Universe**: 0
   - **Start Channel**: 0
   - **Number of LEDs**: 50 (or your LED count)
   - **Type**: Linear (as it is a linear stripe)
   - **RGB Order**: RGB
   - **Universe Size**: 512

![OpenRGB E1.31 Device Configuration](https://github.com/user-attachments/assets/7e4a1722-f17d-4971-bd17-de16c4feb7bd)

## Development

### Project Structure

```
├── .cargo/config.toml      # Rust/Cargo configuration
├── .github/workflows/      # CI/CD workflows
├── src/main.rs            # Main application code
├── Cargo.toml             # Dependencies and metadata
├── partitions.csv         # ESP32 partition table
├── sdkconfig.defaults     # ESP-IDF configuration
└── README.md             # This file
```

### Key Components

- **LED Signal Generation**: Uses ESP32's RMT peripheral for precise timing
- **Wi-Fi Management**: Handles connection and reconnection
- **UDP Server**: Processes incoming E1.31 packets
- **Color Management**: RGB and HSV color space support

### Building for Development

For development builds with debugging:
*Note: on some ESPs with only 4MB flash you can't run debug builds as those are too big in filesize*
```bash
cargo build --target riscv32imac-esp-espidf
cargo espflash flash --target riscv32imac-esp-espidf --partition-table partitions.csv --monitor
```

## Troubleshooting

### Common Issues

1. **Wi-Fi Connection Failed**
   - Verify SSID and password in environment variables
   - Check Wi-Fi network compatibility (2.4GHz required)

2. **LEDs Not Responding**
   - Verify pin connections (GPIO9 (or what you set) for external strip)
   - Check LED strip power supply
   - Confirm LED count configuration matches physical strip

3. **Build Errors**
   - Ensure you followed the [prerequisites](https://github.com/esp-rs/esp-idf-template#prerequisites)
   - Verify Rust toolchain includes required components (nightly toolchain with riscv32 support for newer esps)
   - Check target architecture matches your ESP32 variant

### Debugging

Monitor serial output during operation (requires [monitor setup](https://github.com/esp-rs/esp-idf-template?tab=readme-ov-file#install-esp-idf-monitor)):
```bash
cargo espflash monitor
```

The application provides detailed logging for:
- Wi-Fi connection status
- UDP packet reception
- LED update operations

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
5. Submit a pull request

## License

This project is licensed under the Apache 2.0 License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- ESP-RS community for excellent Rust support on ESP32
- Espressif for those cheap and handy chips and the ESP-IDF framework
