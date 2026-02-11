#![allow(dead_code)]

use device_detector::{ClientHints, DeviceDetector};
use fixtures::fixtures;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Matomo fixtures use `[]` or `null` to represent "no value" for struct fields.
/// This deserializer accepts either a struct `T`, an empty sequence, or null → None.
fn empty_array_as_none<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: serde::de::DeserializeOwned,
    D: serde::Deserializer<'de>,
{
    let value: serde_yaml::Value = serde_yaml::Value::deserialize(deserializer)?;
    match &value {
        serde_yaml::Value::Null => Ok(None),
        serde_yaml::Value::Sequence(seq) if seq.is_empty() => Ok(None),
        _ => serde_yaml::from_value(value)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

// Global DeviceDetector instance that is initialized once
static DETECTOR_INSTANCE: OnceLock<Arc<DeviceDetector>> = OnceLock::new();

fn get_shared_detector() -> Arc<DeviceDetector> {
    DETECTOR_INSTANCE
        .get_or_init(|| {
            let t = std::time::Instant::now();
            let path = Path::new("vendor/device-detector/regexes");
            assert!(path.exists(), "regexes dir not found at {:?}", path);
            let dd = DeviceDetector::from_dir(path).expect("failed to build DeviceDetector");
            eprintln!("detector loaded in {:?}", t.elapsed());
            Arc::new(dd)
        })
        .clone()
}

fn make_detector() -> Arc<DeviceDetector> {
    get_shared_detector()
}

// ---------------------------------------------------------------------------
// Bot fixtures
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BotFixture {
    user_agent: String,
    bot: BotFixtureData,
}

#[derive(Debug, Deserialize)]
struct BotFixtureData {
    name: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    producer: Option<BotFixtureProducer>,
}

#[derive(Debug, Deserialize)]
struct BotFixtureProducer {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[fixtures(["vendor/device-detector/Tests/fixtures/bots.yml"])]
#[test]
fn test_bot_fixtures(path: &std::path::Path) {
    let dd = make_detector();
    let content = std::fs::read_to_string(path).unwrap();
    let fixtures: Vec<BotFixture> = serde_yaml::from_str(&content).unwrap();

    for f in &fixtures {
        let result = dd.parse(&f.user_agent);
        assert!(result.is_bot(), "expected bot for UA: {}", f.user_agent);

        let bot = result.bot().unwrap();
        assert_eq!(
            bot.name, f.bot.name,
            "bot name mismatch for UA: {}",
            f.user_agent
        );
    }
}

// ---------------------------------------------------------------------------
// Device fixtures
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeviceFixture {
    user_agent: String,
    #[serde(default, deserialize_with = "empty_array_as_none")]
    os: Option<OsFixture>,
    #[serde(default, deserialize_with = "empty_array_as_none")]
    client: Option<ClientFixture>,
    #[serde(default, deserialize_with = "empty_array_as_none")]
    device: Option<DeviceFixtureData>,
    /// Client hints headers — when present, the expected result depends on
    /// client-hints processing which we have not implemented yet.
    #[serde(default)]
    headers: Option<serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
struct OsFixture {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClientFixture {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceFixtureData {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    brand: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[fixtures([
    "vendor/device-detector/Tests/fixtures/camera*.yml",
    "vendor/device-detector/Tests/fixtures/car_browser*.yml",
    "vendor/device-detector/Tests/fixtures/console*.yml",
    "vendor/device-detector/Tests/fixtures/desktop*.yml",
    "vendor/device-detector/Tests/fixtures/feature_phone*.yml",
    "vendor/device-detector/Tests/fixtures/feed_reader*.yml",
    "vendor/device-detector/Tests/fixtures/mediaplayer*.yml",
    "vendor/device-detector/Tests/fixtures/mobile_apps*.yml",
    "vendor/device-detector/Tests/fixtures/peripheral*.yml",
    "vendor/device-detector/Tests/fixtures/phablet*.yml",
    "vendor/device-detector/Tests/fixtures/podcasting*.yml",
    "vendor/device-detector/Tests/fixtures/portable_media_player*.yml",
    "vendor/device-detector/Tests/fixtures/smart_display*.yml",
    "vendor/device-detector/Tests/fixtures/smart_speaker*.yml",
    "vendor/device-detector/Tests/fixtures/smartphone*.yml",
    "vendor/device-detector/Tests/fixtures/tablet*.yml",
    "vendor/device-detector/Tests/fixtures/tv*.yml",
    "vendor/device-detector/Tests/fixtures/unknown*.yml",
    "vendor/device-detector/Tests/fixtures/wearable*.yml",
])]
#[test]
fn test_device_fixtures(path: &std::path::Path) {
    let dd = make_detector();
    let content = std::fs::read_to_string(path).unwrap();
    let fixtures: Vec<DeviceFixture> = serde_yaml::from_str(&content).unwrap();

    for f in &fixtures {
        // Skip entries that require client-hints processing (not yet implemented).
        if f.headers.is_some() {
            continue;
        }

        let result = dd.parse(&f.user_agent);

        if let Some(expected_device) = &f.device {
            if let Some(expected_brand) = &expected_device.brand {
                // When expected type and brand are both empty, a None device
                // is acceptable (Matomo returns empty fields, we return None).
                let expected_type = expected_device.kind.as_deref().unwrap_or("");
                if expected_type.is_empty()
                    && expected_brand.is_empty()
                    && result.device().is_none()
                {
                    continue;
                }

                let device = result.device().unwrap_or_else(|| {
                    panic!("expected device detection for UA: {}", f.user_agent)
                });
                assert!(
                    device.brand.eq_ignore_ascii_case(expected_brand),
                    "device brand: expected {:?}, got {:?} for UA: {}",
                    expected_brand,
                    device.brand,
                    f.user_agent,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Client-hints app fixtures
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClientHintsAppFixture {
    user_agent: String,
    #[serde(default)]
    headers: Option<HashMap<String, serde_yaml::Value>>,
    #[serde(default, deserialize_with = "empty_array_as_none")]
    os: Option<OsFixture>,
    #[serde(default, deserialize_with = "empty_array_as_none")]
    client: Option<ClientFixture>,
    #[serde(default, deserialize_with = "empty_array_as_none")]
    device: Option<DeviceFixtureData>,
}

/// Build a `ClientHints` from the fixture's headers map.
fn build_hints(headers: &Option<HashMap<String, serde_yaml::Value>>) -> ClientHints {
    let mut hints = ClientHints::default();
    let headers = match headers {
        Some(h) => h,
        None => return hints,
    };

    // X-Requested-With may appear as various case/prefix forms in fixtures.
    for key in [
        "X-Requested-With",
        "x-requested-with",
        "http-x-requested-with",
    ] {
        if let Some(val) = headers.get(key) {
            if let Some(s) = val.as_str() {
                hints.x_requested_with = Some(s.to_string());
                break;
            }
        }
    }

    // Sec-CH-UA-Mobile: "?1" → true, "?0" → false
    if let Some(val) = headers.get("Sec-CH-UA-Mobile") {
        if let Some(s) = val.as_str() {
            if s.contains("?1") {
                hints.mobile = Some(true);
            } else if s.contains("?0") {
                hints.mobile = Some(false);
            }
        }
    }

    // Sec-CH-UA-Model
    if let Some(val) = headers.get("Sec-CH-UA-Model") {
        if let Some(s) = val.as_str() {
            let trimmed = s.trim_matches('"');
            if !trimmed.is_empty() {
                hints.model = Some(trimmed.to_string());
            }
        }
    }

    hints
}

#[fixtures(["vendor/device-detector/Tests/fixtures/clienthints-app.yml"])]
#[test]
fn test_clienthints_app_fixtures(path: &std::path::Path) {
    let dd = make_detector();
    let content = std::fs::read_to_string(path).unwrap();
    let fixtures: Vec<ClientHintsAppFixture> = serde_yaml::from_str(&content).unwrap();

    for f in &fixtures {
        let hints = build_hints(&f.headers);
        let result = dd.parse_with_hints(&f.user_agent, Some(&hints));

        // Assert client name and type.
        if let Some(expected_client) = &f.client {
            let expected_name = expected_client.name.as_deref().unwrap_or("");
            let expected_type = expected_client.kind.as_deref().unwrap_or("");

            if !expected_name.is_empty() {
                let client = result.client().unwrap_or_else(|| {
                    panic!(
                        "expected client {:?} for UA: {}",
                        expected_name, f.user_agent
                    )
                });
                assert_eq!(
                    client.name, expected_name,
                    "client name mismatch for UA: {}",
                    f.user_agent,
                );
                if !expected_type.is_empty() {
                    assert_eq!(
                        client.kind.as_str(),
                        expected_type,
                        "client type mismatch for UA: {}",
                        f.user_agent,
                    );
                }
            }
        }

        // Assert device brand.
        if let Some(expected_device) = &f.device {
            if let Some(expected_brand) = &expected_device.brand {
                let expected_type = expected_device.kind.as_deref().unwrap_or("");
                if expected_type.is_empty()
                    && expected_brand.is_empty()
                    && result.device().is_none()
                {
                    continue;
                }

                let device = result.device().unwrap_or_else(|| {
                    panic!("expected device detection for UA: {}", f.user_agent)
                });
                assert!(
                    device.brand.eq_ignore_ascii_case(expected_brand),
                    "device brand: expected {:?}, got {:?} for UA: {}",
                    expected_brand,
                    device.brand,
                    f.user_agent,
                );
            }
        }
    }
}
