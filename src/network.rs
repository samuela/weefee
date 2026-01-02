use anyhow::{Context, Result};
use dbus::arg::{PropMap, Variant};
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use nmdbus::{
    NetworkManager, accesspoint::AccessPoint, device::Device, device_wireless::DeviceWireless,
};
use std::collections::HashMap;
use std::time::Duration;

pub struct WifiInfo {
    pub ssid: String,
    pub strength: u8,
    pub security: String,
    pub active: bool,
    pub weak_security: bool,
}

#[derive(Debug, Clone)]
pub struct WifiDeviceInfo {
    pub device_count: usize,
    pub wifi_enabled: bool,
    pub device_names: Vec<String>,
}

pub struct NetworkClient {
    connection: Connection,
}

const NM_BUS_NAME: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE_TYPE_WIFI: u32 = 2;

impl NetworkClient {
    pub fn new() -> Result<Self> {
        let connection = Connection::new_system().context("Failed to connect to system bus")?;
        Ok(Self { connection })
    }

    pub fn get_device_info(&self) -> Result<WifiDeviceInfo> {
        let nm_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(5));

        // Check if WiFi is enabled globally
        let wifi_enabled = nm_proxy.wireless_enabled().unwrap_or(false);

        // Get all devices
        let device_paths = nm_proxy.get_devices().context("Failed to get devices")?;
        let mut device_names = Vec::new();

        for dev_path in device_paths {
            let dev_proxy =
                self.connection
                    .with_proxy(NM_BUS_NAME, &dev_path, Duration::from_secs(5));

            // TODO: collapse ifs
            if let Ok(dev_type) = dev_proxy.device_type() {
                if dev_type == DEVICE_TYPE_WIFI {
                    // Get device interface name
                    if let Ok(interface) = dev_proxy.interface() {
                        device_names.push(interface);
                    }
                }
            }
        }

        Ok(WifiDeviceInfo {
            device_count: device_names.len(),
            wifi_enabled,
            device_names,
        })
    }

    pub fn get_wifi_networks(&self) -> Result<Vec<WifiInfo>> {
        let nm_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(5));

        // Get all devices
        let device_paths = nm_proxy.get_devices().context("Failed to get devices")?;
        let mut networks = Vec::new();

        for dev_path in device_paths {
            let dev_proxy =
                self.connection
                    .with_proxy(NM_BUS_NAME, &dev_path, Duration::from_secs(5));

            // Check device type
            // TODO: collapse these ifs
            if let Ok(dev_type) = dev_proxy.device_type() {
                if dev_type == DEVICE_TYPE_WIFI {
                    // It's a WiFi device - request a scan to refresh the cache
                    let options: HashMap<String, Variant<Box<dyn dbus::arg::RefArg>>> =
                        HashMap::new();
                    let _ = dev_proxy.request_scan(options);

                    // Small delay to allow scan results to populate
                    std::thread::sleep(Duration::from_millis(100));

                    if let Ok(aps) = dev_proxy.get_access_points() {
                        // Get the active access point path - but verify it's actually connected
                        let active_ap_path = dev_proxy.active_access_point().unwrap_or_default();

                        // Verify the device state is actually connected (100 = ACTIVATED)
                        // Device states: 0=UNKNOWN, 10=UNMANAGED, 20=UNAVAILABLE, 30=DISCONNECTED,
                        // 40=PREPARE, 50=CONFIG, 60=NEED_AUTH, 70=IP_CONFIG, 80=IP_CHECK,
                        // 90=SECONDARIES, 100=ACTIVATED, 110=DEACTIVATING, 120=FAILED
                        let device_state = dev_proxy
                            .get::<u32>("org.freedesktop.NetworkManager.Device", "State")
                            .unwrap_or(0);

                        // Only consider the AP as truly active if device state is ACTIVATED (100)
                        let truly_active_ap = if device_state == 100 {
                            active_ap_path
                        } else {
                            dbus::Path::default()
                        };

                        for ap_path in aps {
                            let ap_proxy = self.connection.with_proxy(
                                NM_BUS_NAME,
                                &ap_path,
                                Duration::from_secs(5),
                            );

                            // Get SSID
                            let ssid_vec = ap_proxy.ssid().unwrap_or_default();
                            let ssid = String::from_utf8_lossy(&ssid_vec).to_string();

                            if ssid.is_empty() {
                                continue;
                            }

                            // Get Strength
                            let strength = ap_proxy.strength().unwrap_or(0);

                            // Security flags
                            let rsn = ap_proxy.rsn_flags().unwrap_or(0);
                            let wpa = ap_proxy.wpa_flags().unwrap_or(0);

                            let (security, weak_security) = decode_security(wpa, rsn);

                            // Only mark as active if device is truly activated and this is the active AP
                            let is_active = ap_path == truly_active_ap;

                            networks.push(WifiInfo {
                                ssid,
                                strength,
                                security,
                                active: is_active,
                                weak_security,
                            });
                        }
                    }
                }
            }
        }

        // Sort by SSID first to ensure duplicates are consecutive, but put active ones first
        networks.sort_by(|a, b| {
            match a.ssid.cmp(&b.ssid) {
                std::cmp::Ordering::Equal => {
                    // For same SSID, put active first so dedup keeps it
                    if a.active {
                        std::cmp::Ordering::Less
                    } else if b.active {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
                other => other,
            }
        });
        // Now deduplicate - keeps the first occurrence (which is active if any duplicate is active)
        networks.dedup_by(|a, b| a.ssid == b.ssid);

        // Final sort: active networks first, then by strength
        networks.sort_by(|a, b| {
            if a.active {
                std::cmp::Ordering::Less
            } else if b.active {
                std::cmp::Ordering::Greater
            } else {
                b.strength.cmp(&a.strength)
            }
        });

        Ok(networks)
    }

    pub fn connect(&self, ssid: &str, password: &str) -> Result<()> {
        let nm_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(30));

        // Find the device
        let device_paths = nm_proxy.get_devices().context("Failed to get devices")?;
        let mut wifi_device_path = None;

        for dev_path in device_paths {
            let dev_proxy =
                self.connection
                    .with_proxy(NM_BUS_NAME, &dev_path, Duration::from_secs(5));
            if let Ok(dev_type) = dev_proxy.device_type() {
                if dev_type == DEVICE_TYPE_WIFI {
                    wifi_device_path = Some(dev_path);
                    break;
                }
            }
        }

        let wifi_device_path = wifi_device_path.context("No WiFi device found")?;

        let mut final_map: HashMap<&str, PropMap> = HashMap::new();

        // Re-construct directly for dbus call
        let mut connection_settings: PropMap = HashMap::new();
        connection_settings.insert("id".to_string(), Variant(Box::new(ssid.to_string())));
        connection_settings.insert(
            "type".to_string(),
            Variant(Box::new("802-11-wireless".to_string())),
        );

        let mut wifi_settings: PropMap = HashMap::new();
        wifi_settings.insert(
            "ssid".to_string(),
            Variant(Box::new(ssid.as_bytes().to_vec())),
        );
        wifi_settings.insert(
            "mode".to_string(),
            Variant(Box::new("infrastructure".to_string())),
        );

        final_map.insert("connection", connection_settings);
        final_map.insert("802-11-wireless", wifi_settings);

        if !password.is_empty() {
            let mut security_settings: PropMap = HashMap::new();
            security_settings.insert(
                "key-mgmt".to_string(),
                Variant(Box::new("wpa-psk".to_string())),
            );
            security_settings.insert("psk".to_string(), Variant(Box::new(password.to_string())));
            final_map.insert("802-11-wireless-security", security_settings);
        }

        let specific_object = dbus::Path::new("/").unwrap();

        let (active_conn_path, _) = match nm_proxy.add_and_activate_connection(
            final_map,
            wifi_device_path,
            specific_object,
        ) {
            Ok(result) => result,
            Err(e) => {
                // If this fails immediately, it could be due to secrets issues or invalid params
                let err_str = e.to_string();
                if err_str.contains("secrets")
                    || err_str.contains("802-1x")
                    || err_str.contains("password")
                {
                    return Err(anyhow::anyhow!("INCORRECT_PASSWORD"));
                }
                return Err(anyhow::anyhow!("Failed to activate: {}", err_str));
            }
        };

        // Wait for the connection to reach a final state
        // Active connection states: 0=Unknown, 1=Activating, 2=Activated, 3=Deactivating, 4=Deactivated
        const NM_ACTIVE_CONNECTION_STATE_UNKNOWN: u32 = 0;
        const NM_ACTIVE_CONNECTION_STATE_ACTIVATING: u32 = 1;
        const NM_ACTIVE_CONNECTION_STATE_ACTIVATED: u32 = 2;
        const NM_ACTIVE_CONNECTION_STATE_DEACTIVATING: u32 = 3;
        const NM_ACTIVE_CONNECTION_STATE_DEACTIVATED: u32 = 4;

        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(30);

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow::anyhow!("Connection timeout"));
            }

            let ac_proxy = self.connection.with_proxy(
                NM_BUS_NAME,
                active_conn_path.clone(),
                Duration::from_secs(5),
            );

            // Try to get the state
            match ac_proxy.get::<u32>("org.freedesktop.NetworkManager.Connection.Active", "State") {
                Ok(state) => {
                    match state {
                        NM_ACTIVE_CONNECTION_STATE_ACTIVATED => {
                            // Successfully connected!
                            return Ok(());
                        }
                        NM_ACTIVE_CONNECTION_STATE_DEACTIVATED => {
                            // Connection failed - get reason if possible
                            if let Ok(reason) = ac_proxy.get::<u32>(
                                "org.freedesktop.NetworkManager.Connection.Active",
                                "StateReason",
                            ) {
                                // Common reasons:
                                // 7 = NO_SECRETS (missing password)
                                // 8 = SUPPLICANT_DISCONNECT (auth failure)
                                // 9 = CONFIG_FAILED
                                match reason {
                                    7 | 8 => return Err(anyhow::anyhow!("INCORRECT_PASSWORD")),
                                    _ => {
                                        return Err(anyhow::anyhow!(
                                            "Connection failed (reason: {})",
                                            reason
                                        ));
                                    }
                                }
                            }
                            return Err(anyhow::anyhow!("Connection failed"));
                        }
                        NM_ACTIVE_CONNECTION_STATE_ACTIVATING => {
                            // Still activating, continue waiting
                        }
                        NM_ACTIVE_CONNECTION_STATE_DEACTIVATING => {
                            // Deactivating, connection might have failed
                            // Wait for it to reach deactivated state
                        }
                        NM_ACTIVE_CONNECTION_STATE_UNKNOWN | _ => {
                            // Unknown state, continue waiting
                        }
                    }
                }
                Err(_) => {
                    // Object might not exist anymore - connection might have failed
                    // Check if we're past a reasonable time
                    if start.elapsed() > Duration::from_secs(2) {
                        return Err(anyhow::anyhow!("INCORRECT_PASSWORD"));
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn disconnect(&self) -> Result<()> {
        let nm_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(5));

        // Find the WiFi device
        let device_paths = nm_proxy.get_devices().context("Failed to get devices")?;
        let mut wifi_device_path = None;

        for dev_path in device_paths {
            let dev_proxy =
                self.connection
                    .with_proxy(NM_BUS_NAME, &dev_path, Duration::from_secs(5));
            if let Ok(dev_type) = dev_proxy.device_type() {
                if dev_type == DEVICE_TYPE_WIFI {
                    wifi_device_path = Some(dev_path);
                    break;
                }
            }
        }

        let wifi_device_path = wifi_device_path.context("No WiFi device found")?;
        let dev_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, &wifi_device_path, Duration::from_secs(5));

        // Disconnect the device by calling Disconnect() on the device interface
        dev_proxy
            .method_call::<(), _, _, _>("org.freedesktop.NetworkManager.Device", "Disconnect", ())
            .context("Failed to disconnect")?;

        Ok(())
    }
}

fn decode_security(wpa: u32, rsn: u32) -> (String, bool) {
    if wpa == 0 && rsn == 0 {
        return ("Open".to_string(), true);
    }

    let mut modes = Vec::new();
    let mut weak = false;

    // Check for WPA (Legacy)
    if wpa != 0 {
        modes.push("WPA");
        // WPA is generally considered older/weaker than WPA2, but not "Open" weak.
        // However, if it uses TKIP exclusively, it's weak.
        // For simplicity, we flag WPA (without WPA2/RSN) as potentially legacy.
    }

    // Check for RSN (WPA2/WPA3)
    if rsn != 0 {
        // bit 0x100 is Key Mgmt PSK (Personal)
        // bit 0x200 is Key Mgmt 802.1X (Enterprise)
        // bit 0x1000 is SAE (WPA3 Personal) - guessed value, need to confirm for NM

        if (rsn & 0x1000) != 0 {
            modes.push("WPA3");
        } else if (rsn & 0x100) != 0 {
            modes.push("WPA2");
        } else if (rsn & 0x200) != 0 {
            modes.push("WPA2-Ent");
        } else {
            modes.push("RSN");
        }
    }

    // Check weak ciphers (WEP/TKIP) in pairwise or group
    // 0x1 = WEP40, 0x2 = WEP104, 0x4 = TKIP
    let weak_mask = 0x1 | 0x2 | 0x4;

    // WPA flags usually mirror RSN structure for ciphers
    if (wpa & weak_mask) != 0 || (rsn & weak_mask) != 0 {
        // If it explicitly supports weak ciphers, flag it?
        // Many WPA2 mixed modes support TKIP.
        // Real "Weak" is WEP or Open.
        // Let's check for pure WEP.
        // If neither WPA nor RSN flags indicate PSK/802.1x, it might be WEP?
        // Actually, NM sets flags for WEP.
    }

    // Simplified logic:
    // Open = Weak
    // WEP = Weak (WEP is usually inferred if Privacy capability is set but no WPA/RSN, but let's stick to flags)
    // Actually, NM might not set WPA/RSN flags for WEP, but `flags` property might have privacy bit.
    // For now, if wpa==0 and rsn==0, it's Open (Weak).

    // 0x100 in RSN is usually safe.

    let mode_str = if modes.is_empty() {
        weak = true; // Assume weak if unknown (likely WEP or Open)
        "WEP/Open".to_string()
    } else {
        modes.join("/")
    };

    (mode_str, weak)
}
