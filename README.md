# Device Detector

A Rust port of [Matomo's device-detector](https://github.com/matomo-org/device-detector) — parses User-Agent strings into bot/OS/client/device information using the Matomo YAML regex database.

## Features

- **Comprehensive Detection**: Identifies bots, operating systems, browsers/clients, and device types
- **High Performance**: Optimized with `fancy-regex`, `aho-corasick`, and parallel processing via `rayon`
- **Complete Compatibility**: Uses the same YAML regex database as the original Matomo device-detector
- **Detailed Device Information**: Supports detection of smartphones, tablets, TVs, consoles, cameras, wearables, and more
- **Semantic Versioning**: Properly handles OS and client version strings with comparison functions

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
device-detector = "0.1.0"
```

## Usage

```rust
use device_detector::DeviceDetector;

// Initialize the detector with Matomo's regex database
let detector = DeviceDetector::from_dir("path/to/matomo/regexes")?;

// Parse a User-Agent string
let user_agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 14_6 like Mac OS X) AppleWebKit/605.1.15";
let result = detector.parse(user_agent);

// Access detection results
if let Some(bot) = result.bot() {
    println!("Bot: {} ({})", bot.name, bot.category.unwrap_or("unknown"));
}

if let Some(os) = result.os() {
    println!("OS: {} {}", os.name, os.version);
}

if let Some(client) = result.client() {
    println!("Client: {} {}", client.name, client.version);
}

if let Some(device) = result.device() {
    if let Some(kind) = device.kind {
        println!("Device: {:?} {} {}", kind, device.brand, device.model);
    }
}
```

## Testing

The project uses Matomo's regex database which is expected to be located in a `regexes/` directory. You can clone the Matomo device-detector repository and point to its `regexes/` directory:

```bash
git clone https://github.com/matomo-org/device-detector.git vendor/device-detector
```

Run the test suite which uses Matomo's fixture files:

```bash
cargo test --release # otherwise, DeviceDetector loading is too slow
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Based on [Matomo's device-detector](https://github.com/matomo-org/device-detector)
- Uses regex patterns and test fixtures from the original project
