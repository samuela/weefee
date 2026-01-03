use anyhow::{Context, Result};
use dbus::blocking::Connection;
use networkmanager::NetworkManager;
use networkmanager::devices::{Any, Device, Wireless};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
struct ConnectionInfo {
  priority: Option<i32>,
  autoconnect: Option<bool>,
  autoconnect_retries: Option<i32>,
}

pub struct NetworkClient {
  connection: Connection,
}

impl NetworkClient {
  pub fn new() -> Result<Self> {
    let connection = Connection::new_system().context("Failed to connect to system bus")?;
    Ok(Self { connection })
  }

  pub fn get_device_info(&self) -> Result<WifiDeviceInfo> {
    let nm = NetworkManager::new(&self.connection);
    let wifi_enabled = nm.wireless_enabled().context("Failed to get WiFi state")?;
    Ok(WifiDeviceInfo { wifi_enabled })
  }

  pub fn get_wifi_networks(&self) -> Result<Vec<WifiInfo>> {
    let nm = NetworkManager::new(&self.connection);
    let devices = nm.get_devices().context("Failed to get devices")?;

    // Batch load all connection info upfront to avoid repeated nmcli calls
    let connection_info_map = self.get_all_connection_info()?;

    let mut networks = Vec::new();

    for device in devices {
      if let Device::WiFi(wifi_device) = device {
        // Request a scan to refresh the cache
        let _ = wifi_device.request_scan(HashMap::new());

        // Small delay to allow scan results to populate
        std::thread::sleep(Duration::from_millis(100));

        // Get all access points
        let access_points = wifi_device
          .get_all_access_points()
          .context("Failed to get access points")?;

        // Check if device is active
        let is_device_active = wifi_device.state().unwrap_or(0) == 100; // 100 = ACTIVATED

        // Get active access point if connected
        let active_ap = if is_device_active {
          wifi_device.active_access_point().ok()
        } else {
          None
        };

        for ap in access_points {
          let ssid = ap.ssid().unwrap_or_default();

          if ssid.is_empty() {
            continue;
          }

          let strength = ap.strength().unwrap_or(0);
          let frequency = ap.frequency().ok();

          // Determine security
          let wpa_flags = ap.wpa_flags().unwrap_or(0);
          let rsn_flags = ap.rsn_flags().unwrap_or(0);
          let (security, weak_security) = decode_security(wpa_flags, rsn_flags);

          // Check if this AP is the active one - compare SSIDs since we don't have path method
          let is_active = if let Some(ref active) = active_ap {
            let active_ssid = active.ssid().unwrap_or_default();
            ssid == active_ssid
          } else {
            false
          };

          // Look up connection info from the cache
          let (known, priority, autoconnect, autoconnect_retries) = connection_info_map
            .get(&ssid)
            .map(|info| (true, info.priority, info.autoconnect, info.autoconnect_retries))
            .unwrap_or((false, None, None, None));

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

    // Sort by SSID first to ensure duplicates are consecutive, but put active ones first
    networks.sort_by(|a, b| match a.ssid.cmp(&b.ssid) {
      std::cmp::Ordering::Equal => {
        if a.active {
          std::cmp::Ordering::Less
        } else if b.active {
          std::cmp::Ordering::Greater
        } else {
          std::cmp::Ordering::Equal
        }
      }
      other => other,
    });

    // Deduplicate - keeps the first occurrence (which is active if any duplicate is active)
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

  fn get_all_connection_info(&self) -> Result<HashMap<String, ConnectionInfo>> {
    // Batch load all connection info with minimal nmcli calls
    let mut result = HashMap::new();

    // Get all connection names in one call
    let output = std::process::Command::new("nmcli")
      .args(&["--terse", "--fields", "NAME,TYPE", "connection", "show"])
      .output()
      .context("Failed to execute nmcli")?;

    if !output.status.success() {
      return Ok(result);
    }

    let connections = String::from_utf8_lossy(&output.stdout);
    let wifi_connections: Vec<String> = connections
      .lines()
      .filter_map(|line| {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 2 && parts[1] == "802-11-wireless" {
          Some(parts[0].to_string())
        } else {
          None
        }
      })
      .collect();

    if wifi_connections.is_empty() {
      return Ok(result);
    }

    // Batch get all properties for each connection in one call per connection
    for ssid in &wifi_connections {
      let mut autoconnect = Some(true);
      let mut priority = None;
      let mut autoconnect_retries = None;

      // Get all fields for this connection in one call
      let output = std::process::Command::new("nmcli")
        .args(&[
          "--terse",
          "--fields",
          "connection.autoconnect,connection.autoconnect-priority,connection.autoconnect-retries",
          "connection",
          "show",
          ssid,
        ])
        .output()
        .ok();

      if let Some(output) = output {
        if output.status.success() {
          let values = String::from_utf8_lossy(&output.stdout);
          let lines: Vec<&str> = values.lines().collect();

          // Parse autoconnect
          if let Some(line) = lines.get(0) {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
              let value = parts[1].trim().to_lowercase();
              autoconnect = match value.as_str() {
                "yes" | "true" | "1" => Some(true),
                "no" | "false" | "0" => Some(false),
                "" => Some(true),
                _ => Some(true),
              };
            }
          }

          // Parse priority
          if let Some(line) = lines.get(1) {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
              let value = parts[1].trim();
              if !value.is_empty() {
                priority = value.parse::<i32>().ok();
              }
            }
          }

          // Parse autoconnect-retries
          if let Some(line) = lines.get(2) {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
              let value = parts[1].trim();
              if !value.is_empty() {
                autoconnect_retries = value.parse::<i32>().ok();
              }
            }
          }
        }
      }

      result.insert(
        ssid.clone(),
        ConnectionInfo {
          priority,
          autoconnect,
          autoconnect_retries,
        },
      );
    }

    Ok(result)
  }

  fn get_connection_info(&self, ssid: &str) -> Result<(bool, Option<i32>, Option<bool>, Option<i32>)> {
    // Simplified version for single lookups (used in connect/toggle_autoconnect)
    let all_info = self.get_all_connection_info()?;
    if let Some(info) = all_info.get(ssid) {
      Ok((true, info.priority, info.autoconnect, info.autoconnect_retries))
    } else {
      Ok((false, None, None, None))
    }
  }

  pub fn connect(&self, ssid: &str, password: &str) -> Result<()> {
    let nm = NetworkManager::new(&self.connection);
    let devices = nm.get_devices().context("Failed to get devices")?;

    // Find the WiFi device to ensure it exists
    let _wifi_device = devices
      .into_iter()
      .find_map(|d| if let Device::WiFi(w) = d { Some(w) } else { None })
      .context("No WiFi device found")?;

    // Check if this is a known network
    let (known, _, _, _) = self.get_connection_info(ssid)?;

    if known {
      // Known network - use nmcli to activate (networkmanager-rs doesn't expose easy activation API)
      let output = std::process::Command::new("nmcli")
        .args(&["connection", "up", ssid])
        .output()
        .context("Failed to execute nmcli")?;

      if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        if error.contains("AlreadyActive") || error.contains("already active") {
          std::thread::sleep(Duration::from_millis(500));
          return Ok(());
        }
        // For known networks, keep the profile even if connection fails
        return Err(anyhow::anyhow!("Failed to activate: {}", error));
      }
      Ok(())
    } else {
      // New network - use nmcli to create and connect
      let mut args = vec!["device", "wifi", "connect", ssid];
      if !password.is_empty() {
        args.push("password");
        args.push(password);
      }

      let output = std::process::Command::new("nmcli")
        .args(&args)
        .output()
        .context("Failed to execute nmcli")?;

      if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);

        // For unknown networks that fail to connect, delete the connection profile
        // that was created by nmcli. This prevents the network from being marked
        // as "known" after a failed connection attempt.
        self.forget_network(ssid).context("failed to forget network")?;

        Err(anyhow::anyhow!("Failed to connect: {}", error))
      } else {
        // Wait a bit to let connection establish
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
      }
    }
  }

  pub fn disconnect(&self) -> Result<()> {
    let nm = NetworkManager::new(&self.connection);
    let devices = nm.get_devices().context("Failed to get devices")?;

    // Find the WiFi device
    for device in devices {
      if let Device::WiFi(wifi_device) = device {
        wifi_device.disconnect().context("Failed to disconnect")?;
      }
    }

    Ok(())
  }

  pub fn forget_network(&self, ssid: &str) -> Result<()> {
    // Use nmcli to delete the connection
    let output = std::process::Command::new("nmcli")
      .args(&["connection", "delete", ssid])
      .output()
      .context("Failed to execute nmcli")?;

    // In some cases, eg RSN networks, nmcli does not create a network profile after a failed connection attempt. We
    // consider a forgetting successful as long as no network profile exists afterwards.
    if output.status.success() || String::from_utf8_lossy(&output.stderr).contains("cannot delete unknown connection") {
      Ok(())
    } else {
      Err(anyhow::anyhow!("Failed to forget network: {:?}", output))
    }
  }

  pub fn toggle_autoconnect(&self, ssid: &str) -> Result<()> {
    // Get current value
    let (known, _, autoconnect, _) = self.get_connection_info(ssid)?;

    if !known {
      return Err(anyhow::anyhow!("Network not found in saved connections"));
    }

    let current = autoconnect.unwrap_or(true);
    let new_value = if current { "no" } else { "yes" };

    // Use nmcli to modify the connection
    let output = std::process::Command::new("nmcli")
      .args(&["connection", "modify", ssid, "connection.autoconnect", new_value])
      .output()
      .context("Failed to execute nmcli")?;

    if output.status.success() {
      Ok(())
    } else {
      return Err(anyhow::anyhow!("Failed to toggle autoconnect: {:?}", output));
    }
  }
}

fn decode_security(wpa_flags: u32, rsn_flags: u32) -> (String, bool) {
  if wpa_flags == 0 && rsn_flags == 0 {
    return ("Open".to_string(), true);
  }

  let mut modes = Vec::new();
  let mut weak = false;

  // Check for WPA (Legacy)
  if wpa_flags != 0 {
    modes.push("WPA");
  }

  // Check for RSN (WPA2/WPA3)
  if rsn_flags != 0 {
    // bit 0x100 is Key Mgmt PSK (Personal)
    // bit 0x200 is Key Mgmt 802.1X (Enterprise)
    // bit 0x1000 is SAE (WPA3 Personal)

    if (rsn_flags & 0x1000) != 0 {
      modes.push("WPA3");
    } else if (rsn_flags & 0x100) != 0 {
      modes.push("WPA2");
    } else if (rsn_flags & 0x200) != 0 {
      modes.push("WPA2-Ent");
    } else {
      modes.push("RSN");
    }
  }

  let mode_str = if modes.is_empty() {
    weak = true;
    "WEP/Open".to_string()
  } else {
    modes.join("/")
  };

  (mode_str, weak)
}
