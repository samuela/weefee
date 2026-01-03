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
    pub known: bool,
    pub priority: Option<i32>,
    pub autoconnect: Option<bool>,
    pub autoconnect_retries: Option<i32>,
    pub frequency: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct WifiDeviceInfo {
    pub wifi_enabled: bool,
}

pub struct NetworkClient {
    connection: Connection,
}

#[derive(Debug, Clone)]
struct SavedConnection {
    path: dbus::Path<'static>,
    ssid: String,
    priority: Option<i32>,
    autoconnect: Option<bool>,
    autoconnect_retries: Option<i32>,
}

const NM_BUS_NAME: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const DEVICE_TYPE_WIFI: u32 = 2;

impl NetworkClient {
    pub fn new() -> Result<Self> {
        let connection = Connection::new_system().context("Failed to connect to system bus")?;
        Ok(Self { connection })
    }

    fn get_connection_autoconnect(&self, ssid: &str) -> Option<bool> {
        // TODO: using nmcli is gross but alas could not get the dbus implementation to work correctly
        // Use nmcli to read autoconnect reliably
        let output = std::process::Command::new("nmcli")
            .args(&["-g", "connection.autoconnect", "connection", "show", ssid])
            .output()
            .ok()?;

        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
            match value.as_str() {
                "yes" | "true" | "1" => Some(true),
                "no" | "false" | "0" => Some(false),
                _ => Some(true), // default
            }
        } else {
            Some(true) // default on error
        }
    }

    fn get_saved_connections(&self) -> Result<Vec<SavedConnection>> {
        let settings_proxy = self.connection.with_proxy(
            NM_BUS_NAME,
            NM_SETTINGS_PATH,
            Duration::from_secs(5),
        );

        let (connection_paths,): (Vec<dbus::Path>,) = settings_proxy
            .method_call(
                "org.freedesktop.NetworkManager.Settings",
                "ListConnections",
                (),
            )
            .context("Failed to list connections")?;

        let mut saved_connections = Vec::new();

        for conn_path in connection_paths {
            let conn_proxy = self.connection.with_proxy(
                NM_BUS_NAME,
                &conn_path,
                Duration::from_secs(5),
            );

            // Get connection settings
            let (settings,): (HashMap<String, PropMap>,) = match conn_proxy
                .method_call(
                    "org.freedesktop.NetworkManager.Settings.Connection",
                    "GetSettings",
                    (),
                ) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Check if it's a WiFi connection
            if let Some(conn_settings) = settings.get("connection") {
                if let Some(conn_type) = conn_settings.get("type") {
                    if let Some(type_str) = conn_type.0.as_str() {
                        if type_str == "802-11-wireless" {
                            // Get SSID
                            if let Some(wifi_settings) = settings.get("802-11-wireless") {
                                if let Some(ssid_variant) = wifi_settings.get("ssid") {
                                    if let Some(ssid_bytes) = ssid_variant.0.as_iter() {
                                        let ssid_vec: Vec<u8> = ssid_bytes
                                            .filter_map(|v| v.as_u64().map(|b| b as u8))
                                            .collect();
                                        let ssid = String::from_utf8_lossy(&ssid_vec).to_string();

                                        // Get autoconnect-priority
                                        let priority = conn_settings
                                            .get("autoconnect-priority")
                                            .and_then(|v| v.0.as_i64())
                                            .map(|p| p as i32);

                                        // Get autoconnect using nmcli
                                        let autoconnect = self.get_connection_autoconnect(&ssid);

                                        // Get autoconnect-retries
                                        let autoconnect_retries = conn_settings
                                            .get("autoconnect-retries")
                                            .and_then(|v| v.0.as_i64())
                                            .map(|r| r as i32);

                                        saved_connections.push(SavedConnection {
                                            path: conn_path.clone(),
                                            ssid,
                                            priority,
                                            autoconnect,
                                            autoconnect_retries,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(saved_connections)
    }

    pub fn get_device_info(&self) -> Result<WifiDeviceInfo> {
        let nm_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(5));

        // Check if WiFi is enabled globally
        let wifi_enabled = nm_proxy.wireless_enabled().unwrap_or(false);

        Ok(WifiDeviceInfo {
            wifi_enabled,
        })
    }

    pub fn get_wifi_networks(&self) -> Result<Vec<WifiInfo>> {
        let nm_proxy = self
            .connection
            .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(5));

        // Get saved connections
        let saved_connections = self.get_saved_connections().unwrap_or_default();

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

                            // Get Frequency
                            let frequency = ap_proxy.frequency().ok();

                            // Security flags
                            let rsn = ap_proxy.rsn_flags().unwrap_or(0);
                            let wpa = ap_proxy.wpa_flags().unwrap_or(0);

                            let (security, weak_security) = decode_security(wpa, rsn);

                            // Only mark as active if device is truly activated and this is the active AP
                            let is_active = ap_path == truly_active_ap;

                            // Check if this network is known
                            let saved_conn = saved_connections.iter().find(|c| c.ssid == ssid);
                            let known = saved_conn.is_some();
                            let priority = saved_conn.and_then(|c| c.priority);
                            let autoconnect = saved_conn.and_then(|c| c.autoconnect);
                            let autoconnect_retries = saved_conn.and_then(|c| c.autoconnect_retries);

                            networks.push(WifiInfo {
                                ssid,
                                strength,
                                security,
                                active: is_active,
                                weak_security,
                                known,
                                priority,
                                autoconnect,
                                autoconnect_retries,
                                frequency,
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

        // Check if this is a known network
        let saved_connections = self.get_saved_connections().unwrap_or_default();
        let saved_conn = saved_connections.iter().find(|c| c.ssid == ssid);

        let active_conn_path = if let Some(conn) = saved_conn {
            // Network is known - use ActivateConnection (no password needed)
            // Find the specific access point for this SSID
            let dev_proxy = self
                .connection
                .with_proxy(NM_BUS_NAME, &wifi_device_path, Duration::from_secs(5));

            let mut specific_ap = dbus::Path::new("/").unwrap();
            if let Ok(aps) = dev_proxy.get_access_points() {
                for ap_path in aps {
                    let ap_proxy = self.connection.with_proxy(
                        NM_BUS_NAME,
                        &ap_path,
                        Duration::from_secs(5),
                    );
                    if let Ok(ap_ssid) = ap_proxy.ssid() {
                        let ap_ssid_str = String::from_utf8_lossy(&ap_ssid).to_string();
                        if ap_ssid_str == ssid {
                            specific_ap = ap_path;
                            break;
                        }
                    }
                }
            }

            // Create a proxy with a longer timeout for this operation
            let nm_proxy_long = self
                .connection
                .with_proxy(NM_BUS_NAME, NM_PATH, Duration::from_secs(60));

            match nm_proxy_long.method_call::<(dbus::Path,), _, _, _>(
                "org.freedesktop.NetworkManager",
                "ActivateConnection",
                (conn.path.clone(), wifi_device_path.clone(), specific_ap),
            ) {
                Ok((active_path,)) => active_path,
                Err(e) => {
                    let err_str = e.to_string();
                    // If the connection is already active or activating, that's not really an error
                    if err_str.contains("AlreadyActive") || err_str.contains("already active") {
                        // Connection is already active/activating - wait for it to complete
                        // We need to find the active connection path
                        // For now, let's just wait and check the device state
                        std::thread::sleep(Duration::from_millis(500));
                        return Ok(());
                    }
                    return Err(anyhow::anyhow!("Failed to activate: {}", err_str));
                }
            }
        } else {
            // Network is not known - create new connection with password
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

            let (connection_path, active_path) = match nm_proxy.add_and_activate_connection(
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

            // Store the connection path so we can delete it if connection fails
            let new_connection_path = Some(connection_path);

            // Wait for connection and delete profile if it fails
            let wait_result = self.wait_for_connection_state(
                &active_path,
                new_connection_path.as_ref(),
            );

            return wait_result;
        };

        // Wait for known network connection to complete (no cleanup needed)
        self.wait_for_connection_state(&active_conn_path, None)
    }

    fn wait_for_connection_state(
        &self,
        active_conn_path: &dbus::Path,
        new_connection_path: Option<&dbus::Path>,
    ) -> Result<()> {
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
                // We timed out waiting for connection. If it was a new connection, delete the NetworkManager connection profile so it's not listed as a known network in the future.
                // Delete the connection profile if this was a new connection that timed out
                if let Some(conn_path) = new_connection_path {
                    // TODO: log the error if there's an error here
                    let _ = self.delete_connection(conn_path);
                }
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
                            // Connection failed - delete the profile if this was a new connection
                            if let Some(conn_path) = new_connection_path {
                                // TODO: log the error if there's an error here
                                let _ = self.delete_connection(conn_path);
                            }

                            // Get reason if possible
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
                    // Object might not exist yet or might not be accessible
                    // For known networks, this is common during initial activation
                    // Continue waiting as long as we haven't timed out
                }
            }

            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn delete_connection(&self, connection_path: &dbus::Path) -> Result<()> {
        let conn_proxy = self.connection.with_proxy(
            NM_BUS_NAME,
            connection_path,
            Duration::from_secs(5),
        );

        conn_proxy
            .method_call::<(), _, _, _>(
                "org.freedesktop.NetworkManager.Settings.Connection",
                "Delete",
                (),
            )
            .context("Failed to delete connection")?;

        Ok(())
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

    pub fn forget_network(&self, ssid: &str) -> Result<()> {
        // Get saved connections
        let saved_connections = self.get_saved_connections()?;

        // Find ALL connections for this SSID (there might be multiple)
        let connections_to_delete: Vec<_> = saved_connections
            .iter()
            .filter(|c| c.ssid == ssid)
            .collect();

        if connections_to_delete.is_empty() {
            return Err(anyhow::anyhow!("Network '{}' not found in saved connections", ssid));
        }

        // Delete all matching connections
        let mut deleted_count = 0;
        let mut last_error = None;

        for connection in connections_to_delete {
            match self.delete_connection(&connection.path) {
                Ok(_) => deleted_count += 1,
                Err(e) => last_error = Some(e),
            }
        }

        if deleted_count == 0 {
            if let Some(err) = last_error {
                return Err(err);
            } else {
                return Err(anyhow::anyhow!("Failed to delete any connections for '{}'", ssid));
            }
        }

        Ok(())
    }

    pub fn toggle_autoconnect(&self, ssid: &str) -> Result<()> {
        // TODO: using nmcli is gross but alas could not get the dbus implementation to work correctly

        // Use nmcli exclusively for toggling autoconnect
        // Get current value first
        let current = self.get_connection_autoconnect(ssid).unwrap_or(true);

        // Toggle to opposite value
        let new_value = if current { "no" } else { "yes" };

        // Use nmcli to modify the connection
        let output = std::process::Command::new("nmcli")
            .args(&["connection", "modify", ssid, "connection.autoconnect", new_value])
            .output()
            .context("Failed to execute nmcli")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Failed to toggle autoconnect: {}", error));
        }

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
