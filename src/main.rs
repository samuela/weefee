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
    let client = NetworkClient::new().unwrap();

    // Helpers to DRY up repeated sends
    let rescan = || {
      tx_net
        .blocking_send(Msg::DeviceInfoUpdate(client.get_device_info().unwrap()))
        .unwrap();
      tx_net
        .blocking_send(Msg::NetworksFound(client.get_wifi_networks().unwrap()))
        .unwrap();
    };

    // Initial fetch
    rescan();

    while let Some(cmd) = net_rx.blocking_recv() {
      match cmd {
        NetCmd::Scan => {
          // Update device info on each scan
          rescan();
        }
        NetCmd::Connect(ssid, password) => match client.connect(&ssid, &password) {
          Ok(_) => {
            tx_net.blocking_send(Msg::ConnectionSuccess).unwrap();
            // Trigger rescan to update network list with the new active connection
            rescan();
          }
          Err(e) => {
            tx_net.blocking_send(Msg::ConnectionFailure(e)).unwrap();
            // Trigger rescan to ensure UI reflects actual state
            rescan();
          }
        },
        NetCmd::Disconnect => match client.disconnect() {
          Ok(_) => {
            tx_net.blocking_send(Msg::DisconnectSuccess).unwrap();
            // Trigger rescan to update network list
            rescan();
          }
          Err(e) => {
            tx_net.blocking_send(Msg::DisconnectFailure(e)).unwrap();
            // Trigger rescan to ensure UI reflects actual state
            rescan();
          }
        },
        NetCmd::Forget(ssid) => match client.forget_network(&ssid) {
          Ok(_) => {
            tx_net.blocking_send(Msg::ForgetSuccess).unwrap();
            // Trigger rescan to update network list
            rescan();
          }
          Err(e) => {
            tx_net.blocking_send(Msg::ForgetFailure(e)).unwrap();
            // Trigger rescan to ensure UI reflects actual state
            rescan();
          }
        },
        NetCmd::ToggleAutoconnect(ssid) => {
          match client.toggle_autoconnect(&ssid) {
            Ok(_) => {
              tx_net.blocking_send(Msg::AutoconnectSuccess).unwrap();
              // Trigger rescan to update network list with new autoconnect status
              rescan();
            }
            Err(e) => {
              tx_net.blocking_send(Msg::AutoconnectFailure(e)).unwrap();
              // Trigger rescan to ensure UI reflects actual state
              rescan();
            }
          }
        }
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
                tx_input.blocking_send(Msg::DPressed).unwrap();
              }
              KeyCode::Char('q') => {
                tx_input.blocking_send(Msg::Quit).unwrap();
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::Quit).unwrap();
              }
              KeyCode::Char('j') | KeyCode::Down => {
                tx_input.blocking_send(Msg::MoveDown).unwrap();
              }
              KeyCode::Char('k') | KeyCode::Up => {
                tx_input.blocking_send(Msg::MoveUp).unwrap();
              }
              KeyCode::Enter => {
                tx_input.blocking_send(Msg::EnterInput).unwrap();
              }
              KeyCode::Char('f') => {
                tx_input.blocking_send(Msg::ConfirmForget).unwrap();
              }
              KeyCode::Char('a') | KeyCode::Char('A') => {
                tx_input.blocking_send(Msg::ToggleAutoconnect).unwrap();
              }
              _ => {}
            },
            AppStateKind::Editing => match key.code {
              KeyCode::Enter => {
                tx_input.blocking_send(Msg::SubmitConnection).unwrap();
              }
              KeyCode::Esc => {
                tx_input.blocking_send(Msg::CancelInput).unwrap();
              }
              KeyCode::Backspace if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::DeletePrevWord).unwrap();
              }
              KeyCode::Backspace if key.modifiers == KeyModifiers::ALT => {
                tx_input.blocking_send(Msg::DeletePrevWord).unwrap();
              }
              KeyCode::Backspace => {
                tx_input.blocking_send(Msg::Backspace).unwrap();
              }
              KeyCode::Left if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::MoveCursorWordLeft).unwrap();
              }
              KeyCode::Left if key.modifiers == KeyModifiers::ALT => {
                tx_input.blocking_send(Msg::MoveCursorWordLeft).unwrap();
              }
              KeyCode::Left => {
                tx_input.blocking_send(Msg::MoveCursorLeft).unwrap();
              }
              KeyCode::Right if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::MoveCursorWordRight).unwrap();
              }
              KeyCode::Right if key.modifiers == KeyModifiers::ALT => {
                tx_input.blocking_send(Msg::MoveCursorWordRight).unwrap();
              }
              KeyCode::Right => {
                tx_input.blocking_send(Msg::MoveCursorRight).unwrap();
              }
              KeyCode::Char('h') if key.modifiers == KeyModifiers::CONTROL => {
                // Ctrl+Backspace is often interpreted as Ctrl+H in terminals
                tx_input.blocking_send(Msg::DeletePrevWord).unwrap();
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::Quit).unwrap();
              }
              KeyCode::Char(c) => {
                tx_input.blocking_send(Msg::Input(c)).unwrap();
              }
              _ => {}
            },
            AppStateKind::Connecting => {
              // Ignore input while connecting
            }
            AppStateKind::Error => match key.code {
              KeyCode::Enter | KeyCode::Esc => {
                tx_input.blocking_send(Msg::DismissError).unwrap();
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::Quit).unwrap();
              }
              _ => {}
            },
            AppStateKind::ConfirmDisconnect => match key.code {
              KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                tx_input.blocking_send(Msg::SubmitDisconnect).unwrap();
              }
              KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                tx_input.blocking_send(Msg::CancelInput).unwrap();
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::Quit).unwrap();
              }
              _ => {}
            },
            AppStateKind::ConfirmForget => match key.code {
              KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                tx_input.blocking_send(Msg::SubmitForget).unwrap();
              }
              KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                tx_input.blocking_send(Msg::CancelInput).unwrap();
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::Quit).unwrap();
              }
              _ => {}
            },
            AppStateKind::ConfirmWeakSecurity => match key.code {
              KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                tx_input.blocking_send(Msg::SubmitConnection).unwrap();
              }
              KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                tx_input.blocking_send(Msg::CancelInput).unwrap();
              }
              KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                tx_input.blocking_send(Msg::Quit).unwrap();
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
              net_tx.send(NetCmd::Connect(net.ssid, password)).await.unwrap();
            } else if let App::Running {
              state: AppState::Connecting { network, .. },
              ..
            } = &app
            {
              net_tx.send(NetCmd::Connect(network.ssid.clone(), String::new())).await.unwrap();
            }
          }
        }
        Msg::SubmitDisconnect => {
          app.update(Msg::SubmitDisconnect);
          net_tx.send(NetCmd::Disconnect).await.unwrap();
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
            net_tx.send(NetCmd::Forget(net.ssid)).await.unwrap();
          }

          app.update(Msg::SubmitForget);
        }
        Msg::EnterInput => {
          app.update(Msg::EnterInput);
          // If we're now in Connecting mode, it means it's a known network
          // and we should connect without asking for password
          if let App::Running {
            state: AppState::Connecting { network, .. },
            ..
          } = &app
          {
            // Empty password for known networks (stored password will be used)
            net_tx.send(NetCmd::Connect(network.ssid.clone(), String::new())).await.unwrap();
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
              net_tx.send(NetCmd::ToggleAutoconnect(ssid)).await.unwrap();
            } else {
              // Show error if network is not known
              *state = AppState::ShowingError {
                error: anyhow::anyhow!("Cannot toggle auto-connect: network is not saved/known. Connect to it first."),
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
