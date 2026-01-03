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

use app::{App, InputMode, Msg};
use network::NetworkClient;

pub enum NetCmd {
    Scan,
    Connect(String, String), // SSID, Password
    Disconnect,
    Forget(String), // SSID
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
                                let _ = tx_net.blocking_send(Msg::ConnectionFailure(e.to_string()));
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
                        },
                    }
                }
            }
            Err(e) => {
                let _ = tx_net.blocking_send(Msg::Error(format!("Failed to init NM: {}", e)));
            }
        }
    });

    // Auto-refresh Task - refresh data every second
    let tx_refresh = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tx_refresh.send(Msg::Scan).await.is_err() {
                break;
            }
        }
    });

    // Input Task
    let tx_input = tx.clone();
    let app_input_state = std::sync::Arc::new(std::sync::Mutex::new(InputMode::Normal));
    let app_input_state_clone = app_input_state.clone();

    tokio::task::spawn_blocking(move || {
        loop {
            // Poll for events
            if event::poll(Duration::from_millis(200)).unwrap() {
                if let Event::Key(key) = event::read().unwrap() {
                    let mode = *app_input_state_clone.lock().unwrap();
                    match mode {
                        InputMode::Normal => match key.code {
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
                            KeyCode::Char('r') | KeyCode::Char('s') => {
                                let _ = tx_input.blocking_send(Msg::Scan);
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
                        InputMode::Editing => match key.code {
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
                        InputMode::Connecting => {
                            // Ignore input while connecting
                        }
                        InputMode::Error => match key.code {
                            KeyCode::Enter | KeyCode::Esc => {
                                let _ = tx_input.blocking_send(Msg::DismissError);
                            }
                            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                                let _ = tx_input.blocking_send(Msg::Quit);
                            }
                            _ => {}
                        },
                        InputMode::ConfirmDisconnect => match key.code {
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
                        InputMode::ConfirmForget => match key.code {
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
                        InputMode::ConfirmWeakSecurity => match key.code {
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
            *mode = app.input_mode;
        }

        if let Some(msg) = rx.recv().await {
            match msg {
                Msg::Quit => {
                    app.should_quit = true;
                }
                Msg::Scan => {
                    app.update(Msg::Scan); // Update UI state
                    let _ = net_tx.send(NetCmd::Scan).await;
                }
                Msg::SubmitConnection => {
                    if let Some(net) = app.networks.get(app.selected_index) {
                        let ssid = net.ssid.clone();
                        let password = app.password_input.value().to_string();
                        app.update(Msg::SubmitConnection);

                        // If we're now in Connecting mode, it means it's a known insecure network
                        // and we should connect with empty password (stored password will be used)
                        if app.input_mode == InputMode::Connecting {
                            if let Some(connecting_ssid) = app.connecting_ssid.clone() {
                                let _ = net_tx.send(NetCmd::Connect(connecting_ssid, String::new())).await;
                            }
                        } else {
                            // Otherwise, we're connecting with the entered password
                            let _ = net_tx.send(NetCmd::Connect(ssid, password)).await;
                        }
                    }
                }
                Msg::SubmitDisconnect => {
                    app.update(Msg::SubmitDisconnect);
                    let _ = net_tx.send(NetCmd::Disconnect).await;
                }
                Msg::ConfirmForget => {
                    // Only show forget dialog if the network is known
                    if let Some(net) = app.networks.get(app.selected_index) {
                        if net.known {
                            app.update(Msg::ConfirmForget);
                        }
                    }
                }
                Msg::SubmitForget => {
                    // Capture network info before updating app state
                    let network_to_forget = app.networks.get(app.selected_index).map(|n| (n.ssid.clone(), n.known));

                    app.update(Msg::SubmitForget);

                    if let Some((ssid, is_known)) = network_to_forget {
                        if is_known {
                            let _ = net_tx.send(NetCmd::Forget(ssid)).await;
                        }
                    }
                }
                Msg::EnterInput => {
                    app.update(Msg::EnterInput);
                    // If we're now in Connecting mode, it means it's a known network
                    // and we should connect without asking for password
                    if app.input_mode == InputMode::Connecting {
                        if let Some(ssid) = app.connecting_ssid.clone() {
                            // Empty password for known networks (stored password will be used)
                            let _ = net_tx.send(NetCmd::Connect(ssid, String::new())).await;
                        }
                    }
                }
                Msg::ToggleAutoconnect => {
                    // Only toggle autoconnect when detail view is active (d_pressed)
                    if app.d_pressed {
                        // Only toggle autoconnect for known networks
                        if let Some(net) = app.networks.get(app.selected_index) {
                            if net.known {
                                let ssid = net.ssid.clone();
                                app.update(Msg::ToggleAutoconnect);
                                let _ = net_tx.send(NetCmd::ToggleAutoconnect(ssid)).await;
                            } else {
                                // Show error if network is not known
                                app.input_mode = InputMode::Error;
                                app.error_message = Some("Cannot toggle auto-connect: network is not saved/known. Connect to it first.".to_string());
                            }
                        }
                    }
                    // If detail view is not active, ignore the key press silently
                }
                _ => {
                    app.update(msg);
                }
            }

            if app.should_quit {
                break;
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    std::process::exit(0);
}
