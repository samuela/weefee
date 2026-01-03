use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
  event::{self, Event, KeyCode, KeyModifiers},
  execute,
  terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;

mod app;
mod network;
mod ui;

use app::{App, AppState, Msg};
use network::NetworkClient;

// TODO: can we get rid of this and use real app enums instead?
// Simplified enum for input handling - doesn't carry state data
#[derive(Debug, Clone, Copy, PartialEq)]
enum AppStateKind {
  Normal,
  Editing,
  Connecting,
  Error,
  ConfirmDisconnect,
  ConfirmForget,
  ConfirmWeakSecurity,
}

pub enum NetCmd {
  Scan,
  Connect(String, String), // SSID, Password
  Disconnect,
  Forget(String),            // SSID
  ToggleAutoconnect(String), // SSID
}

#[tokio::main]
async fn main() -> Result<()> {
  // Setup terminal
  enable_raw_mode()?;
  let mut stdout = io::stdout();
  execute!(stdout, EnterAlternateScreen)?;
  let backend = CrosstermBackend::new(stdout);
  let mut terminal = Terminal::new(backend)?;

  // Channels
  let (tx, mut rx) = mpsc::channel(100);
  let (net_tx, mut net_rx) = mpsc::channel(100);

  // Network Task
  let tx_net = tx.clone();
  std::thread::spawn(move || {
    // We use std::thread because nm might use thread-local storage or glib contexts
    // that are simpler to manage in a dedicated OS thread than tokio's thread pool.
    let client_res = NetworkClient::new();

    match client_res {
      Ok(client) => {
        // Initial fetch
        if let Ok(device_info) = client.get_device_info() {
          let _ = tx_net.blocking_send(Msg::DeviceInfoUpdate(device_info));
        }
        if let Ok(nets) = client.get_wifi_networks() {
          let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
        } else {
          let _ = tx_net.blocking_send(Msg::Error("Failed initial scan".into()));
        }

        while let Some(cmd) = net_rx.blocking_recv() {
          match cmd {
            NetCmd::Scan => {
              // Update device info on each scan
              if let Ok(device_info) = client.get_device_info() {
                let _ = tx_net.blocking_send(Msg::DeviceInfoUpdate(device_info));
              }
              match client.get_wifi_networks() {
                Ok(nets) => {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
                Err(e) => {
                  let _ = tx_net.blocking_send(Msg::Error(e.to_string()));
                }
              }
            }
            NetCmd::Connect(ssid, password) => match client.connect(&ssid, &password) {
              Ok(_) => {
                let _ = tx_net.blocking_send(Msg::ConnectionSuccess);
                // Trigger rescan to update network list with the new active connection
                if let Ok(nets) = client.get_wifi_networks() {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
              }
              Err(e) => {
                let _ = tx_net.blocking_send(Msg::ConnectionFailure(e));
                // Trigger rescan to ensure UI reflects actual state
                if let Ok(nets) = client.get_wifi_networks() {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
              }
            },
            NetCmd::Disconnect => match client.disconnect() {
              Ok(_) => {
                let _ = tx_net.blocking_send(Msg::DisconnectSuccess);
                // Trigger rescan to update network list
                if let Ok(nets) = client.get_wifi_networks() {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
              }
              Err(e) => {
                let _ = tx_net.blocking_send(Msg::DisconnectFailure(e.to_string()));
                // Trigger rescan to ensure UI reflects actual state
                if let Ok(nets) = client.get_wifi_networks() {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
              }
            },
            NetCmd::Forget(ssid) => match client.forget_network(&ssid) {
              Ok(_) => {
                let _ = tx_net.blocking_send(Msg::ForgetSuccess);
                // Trigger rescan to update network list
                if let Ok(nets) = client.get_wifi_networks() {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
              }
              Err(e) => {
                let _ = tx_net.blocking_send(Msg::ForgetFailure(e.to_string()));
                // Trigger rescan to ensure UI reflects actual state
                if let Ok(nets) = client.get_wifi_networks() {
                  let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                }
              }
            },
            NetCmd::ToggleAutoconnect(ssid) => {
              match client.toggle_autoconnect(&ssid) {
                Ok(_) => {
                  let _ = tx_net.blocking_send(Msg::AutoconnectSuccess);
                  // Trigger rescan to update network list with new autoconnect status
                  if let Ok(nets) = client.get_wifi_networks() {
                    let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                  }
                }
                Err(e) => {
                  let _ = tx_net.blocking_send(Msg::AutoconnectFailure(e.to_string()));
                  // Trigger rescan to ensure UI reflects actual state
                  if let Ok(nets) = client.get_wifi_networks() {
                    let _ = tx_net.blocking_send(Msg::NetworksFound(nets));
                  }
                }
              }
            }
          }
        }
      }
      Err(e) => {
        let _ = tx_net.blocking_send(Msg::Error(format!("Failed to init NM: {}", e)));
      }
    }
  });

  // Auto-refresh Task - refresh data every second
  let net_tx_refresh = net_tx.clone();
  tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
      interval.tick().await;
      if net_tx_refresh.send(NetCmd::Scan).await.is_err() {
        break;
      }
    }
  });

  // Input Task
  let tx_input = tx.clone();
  let app_input_state = std::sync::Arc::new(std::sync::Mutex::new(AppStateKind::Normal));
  let app_input_state_clone = app_input_state.clone();

  tokio::task::spawn_blocking(move || {
    loop {
      // Poll for events
      if event::poll(Duration::from_millis(200)).unwrap() {
        if let Event::Key(key) = event::read().unwrap() {
          let mode = *app_input_state_clone.lock().unwrap();
          match mode {
            AppStateKind::Normal => match key.code {
              KeyCode::Char('d') => {
                let _ = tx_input.blocking_send(Msg::DPressed);
              }
              KeyCode::Char('q') => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              KeyCode::Char('j') | KeyCode::Down => {
                let _ = tx_input.blocking_send(Msg::MoveDown);
              }
              KeyCode::Char('k') | KeyCode::Up => {
                let _ = tx_input.blocking_send(Msg::MoveUp);
              }
              KeyCode::Enter => {
                let _ = tx_input.blocking_send(Msg::EnterInput);
              }
              KeyCode::Char('f') => {
                let _ = tx_input.blocking_send(Msg::ConfirmForget);
              }
              KeyCode::Char('a') | KeyCode::Char('A') => {
                let _ = tx_input.blocking_send(Msg::ToggleAutoconnect);
              }
              _ => {}
            },
            AppStateKind::Editing => match key.code {
              KeyCode::Enter => {
                let _ = tx_input.blocking_send(Msg::SubmitConnection);
              }
              KeyCode::Esc => {
                let _ = tx_input.blocking_send(Msg::CancelInput);
              }
              KeyCode::Backspace if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::DeletePrevWord);
              }
              KeyCode::Backspace if key.modifiers == KeyModifiers::ALT => {
                let _ = tx_input.blocking_send(Msg::DeletePrevWord);
              }
              KeyCode::Backspace => {
                let _ = tx_input.blocking_send(Msg::Backspace);
              }
              KeyCode::Left if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::MoveCursorWordLeft);
              }
              KeyCode::Left if key.modifiers == KeyModifiers::ALT => {
                let _ = tx_input.blocking_send(Msg::MoveCursorWordLeft);
              }
              KeyCode::Left => {
                let _ = tx_input.blocking_send(Msg::MoveCursorLeft);
              }
              KeyCode::Right if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::MoveCursorWordRight);
              }
              KeyCode::Right if key.modifiers == KeyModifiers::ALT => {
                let _ = tx_input.blocking_send(Msg::MoveCursorWordRight);
              }
              KeyCode::Right => {
                let _ = tx_input.blocking_send(Msg::MoveCursorRight);
              }
              KeyCode::Char('h') if key.modifiers == KeyModifiers::CONTROL => {
                // Ctrl+Backspace is often interpreted as Ctrl+H in terminals
                let _ = tx_input.blocking_send(Msg::DeletePrevWord);
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              KeyCode::Char(c) => {
                let _ = tx_input.blocking_send(Msg::Input(c));
              }
              _ => {}
            },
            AppStateKind::Connecting => {
              // Ignore input while connecting
            }
            AppStateKind::Error => match key.code {
              KeyCode::Enter | KeyCode::Esc => {
                let _ = tx_input.blocking_send(Msg::DismissError);
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              _ => {}
            },
            AppStateKind::ConfirmDisconnect => match key.code {
              KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = tx_input.blocking_send(Msg::SubmitDisconnect);
              }
              KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                let _ = tx_input.blocking_send(Msg::CancelInput);
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              _ => {}
            },
            AppStateKind::ConfirmForget => match key.code {
              KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = tx_input.blocking_send(Msg::SubmitForget);
              }
              KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                let _ = tx_input.blocking_send(Msg::CancelInput);
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              _ => {}
            },
            AppStateKind::ConfirmWeakSecurity => match key.code {
              KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = tx_input.blocking_send(Msg::SubmitConnection);
              }
              KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                let _ = tx_input.blocking_send(Msg::CancelInput);
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                let _ = tx_input.blocking_send(Msg::Quit);
              }
              _ => {}
            },
          }
        }
      } else {
        if tx_input.blocking_send(Msg::Tick).is_err() {
          break;
        }
      }
    }
  });

  // Main Loop
  let mut app = App::new();

  loop {
    terminal.draw(|f| ui::draw(f, &mut app))?;

    // Sync input state for key handler
    if let Ok(mut mode) = app_input_state.lock() {
      *mode = match &app {
        App::Running { state, .. } => match state {
          AppState::Normal => AppStateKind::Normal,
          AppState::EditingPassword { .. } => AppStateKind::Editing,
          AppState::Connecting { .. } => AppStateKind::Connecting,
          AppState::ShowingError { .. } => AppStateKind::Error,
          AppState::ConfirmDisconnect { .. } => AppStateKind::ConfirmDisconnect,
          AppState::ConfirmForget { .. } => AppStateKind::ConfirmForget,
          AppState::ConfirmWeakSecurity { .. } => AppStateKind::ConfirmWeakSecurity,
        },
        App::ShouldQuit => AppStateKind::Normal, // Doesn't matter, we're quitting
      };
    }

    if let Some(msg) = rx.recv().await {
      match msg {
        Msg::Quit => {
          app = App::ShouldQuit;
        }
        Msg::SubmitConnection => {
          // This logic is cursed, and we should refactor the entire UI framework/setup to make this suck less

          // Capture password and whether we're coming from EditingPassword BEFORE updating state
          let (password, was_editing) = if let App::Running {
            state: AppState::EditingPassword { password_input, .. },
            ..
          } = &app
          {
            (password_input.value().to_string(), true)
          } else {
            (String::new(), false)
          };

          if let Some(net) = app.focused_network() {
            app.update(Msg::SubmitConnection);

            // If we were editing a password, use that password
            // Otherwise (known network or weak security confirmation), use empty password
            // (NetworkManager will use the stored credentials)
            if was_editing {
              let _ = net_tx.send(NetCmd::Connect(net.ssid, password)).await;
            } else if let App::Running {
              state: AppState::Connecting { ssid, .. },
              ..
            } = &app
            {
              let _ = net_tx.send(NetCmd::Connect(ssid.clone(), String::new())).await;
            }
          }
        }
        Msg::SubmitDisconnect => {
          app.update(Msg::SubmitDisconnect);
          let _ = net_tx.send(NetCmd::Disconnect).await;
        }
        Msg::ConfirmForget => {
          // Only show forget dialog if the network is known
          if let App::Running {
            networks, list_state, ..
          } = &app
          {
            if let Some(ix) = list_state.selected()
              && networks[ix].known
            {
              app.update(Msg::ConfirmForget);
            }
          }
        }
        Msg::SubmitForget => {
          // Capture network info before updating app state
          if let Some(net) = app.focused_network()
            && net.known
          {
            let _ = net_tx.send(NetCmd::Forget(net.ssid)).await;
          }

          app.update(Msg::SubmitForget);
        }
        Msg::EnterInput => {
          app.update(Msg::EnterInput);
          // If we're now in Connecting mode, it means it's a known network
          // and we should connect without asking for password
          if let App::Running {
            state: AppState::Connecting { ssid, .. },
            ..
          } = &app
          {
            // Empty password for known networks (stored password will be used)
            let _ = net_tx.send(NetCmd::Connect(ssid.clone(), String::new())).await;
          }
        }
        Msg::ToggleAutoconnect => {
          // Only toggle autoconnect when detail view is active
          if let Some(net) = app.focused_network()
            && let App::Running {
              show_detailed_view: true,
              state,
              ..
            } = &mut app
          {
            // Only toggle autoconnect for known networks
            if net.known {
              let ssid = net.ssid.clone();
              app.update(Msg::ToggleAutoconnect);
              let _ = net_tx.send(NetCmd::ToggleAutoconnect(ssid)).await;
            } else {
              // Show error if network is not known
              *state = AppState::ShowingError {
                message: "Cannot toggle auto-connect: network is not saved/known. Connect to it first.".to_string(),
              };
            }
          }
        }
        _ => {
          app.update(msg);
        }
      }

      if matches!(app, App::ShouldQuit) {
        break;
      }
    }
  }

  // Restore terminal
  disable_raw_mode()?;
  execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
  terminal.show_cursor()?;

  std::process::exit(0);
}
