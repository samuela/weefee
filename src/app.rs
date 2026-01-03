use crate::network::{WifiInfo, WifiDeviceInfo};
use ratatui::widgets::ListState;
use throbber_widgets_tui::ThrobberState;
use tui_input::Input;

// TODO: document what each of these are
pub enum Msg {
    Tick,
    Quit,
    MoveUp,
    MoveDown,
    Scan,
    NetworksFound(Vec<WifiInfo>),
    DeviceInfoUpdate(WifiDeviceInfo),
    Error(String),
    DismissError,
    EnterInput,
    Input(char),
    Backspace,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorWordLeft,
    MoveCursorWordRight,
    DeletePrevWord,
    SubmitConnection,
    CancelInput,
    ConnectionSuccess,
    ConnectionFailure(String),
    SubmitDisconnect,
    DisconnectSuccess,
    DisconnectFailure(String),
    ConfirmForget,
    SubmitForget,
    ForgetSuccess,
    ForgetFailure(String),
    DPressed,
    ToggleAutoconnect,
    AutoconnectSuccess,
    AutoconnectFailure(String),
}

/// Represents the different modal states of the application.
/// This enum makes illegal states unrepresentable by associating
/// state-specific data directly with each variant.
#[derive(Debug)]
pub enum AppState {
    /// Normal browsing mode - user can navigate the network list
    Normal,
    /// Editing password for a network connection
    EditingPassword {
        password_input: Input,
        /// Error message if password was incorrect
        error_message: Option<String>,
    },
    /// Currently connecting to a network
    Connecting {
        ssid: String,
        throbber_state: ThrobberState,
    },
    /// Displaying an error message
    ShowingError {
        message: String,
    },
    /// Confirming disconnect from active network
    ConfirmDisconnect,
    /// Confirming forgetting a known network
    ConfirmForget,
    /// Confirming connection to a network with weak/no security
    ConfirmWeakSecurity {
        ssid: String,
        security_type: String,
    },
}

pub struct App {
    pub should_quit: bool,
    pub networks: Vec<WifiInfo>,
    pub selected_index: usize,
    pub list_state: ListState,
    pub is_scanning: bool,
    pub active_ssid: Option<String>,
    pub device_info: Option<WifiDeviceInfo>,
    pub state: AppState,
    pub d_pressed: bool,
}

impl App {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            should_quit: false,
            networks: Vec::new(),
            selected_index: 0,
            list_state,
            is_scanning: false,
            active_ssid: None,
            device_info: None,
            state: AppState::Normal,
            d_pressed: false,
        }
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Tick => {
                if let AppState::Connecting { throbber_state, .. } = &mut self.state {
                    throbber_state.calc_next();
                }
            }
            Msg::Quit => self.should_quit = true,
            Msg::MoveUp => {
                // If nothing selected, select first network
                if self.list_state.selected().is_none() {
                    if !self.networks.is_empty() {
                        self.selected_index = 0;
                        self.list_state.select(Some(0));
                    }
                } else if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.list_state.select(Some(self.selected_index));
                }
            }
            Msg::MoveDown => {
                // If nothing selected, select first network
                if self.list_state.selected().is_none() {
                    if !self.networks.is_empty() {
                        self.selected_index = 0;
                        self.list_state.select(Some(0));
                    }
                } else if self.selected_index + 1 < self.networks.len() {
                    self.selected_index += 1;
                    self.list_state.select(Some(self.selected_index));
                }
            }
            Msg::Scan => {
                self.is_scanning = true;
            }
            Msg::DeviceInfoUpdate(info) => {
                self.device_info = Some(info);
            }
            Msg::NetworksFound(networks) => {
                self.active_ssid = networks.iter().find(|n| n.active).map(|n| n.ssid.clone());

                // Preserve selection by SSID across rescans
                let previously_selected_ssid = self
                    .networks
                    .get(self.selected_index)
                    .map(|n| n.ssid.clone());

                self.networks = networks;
                self.is_scanning = false;

                // Try to find the previously selected network in the new list
                if let Some(ssid) = previously_selected_ssid {
                    if let Some(new_index) = self.networks.iter().position(|n| n.ssid == ssid) {
                        self.selected_index = new_index;
                        self.list_state.select(Some(new_index));
                    } else {
                        // Network disappeared - show error if password dialog was open
                        if matches!(self.state, AppState::EditingPassword { .. }) {
                            self.state = AppState::ShowingError {
                                message: format!("Network \"{}\" is no longer available.", ssid),
                            };
                        }

                        // If a dialog is open, deselect everything (user will need to reselect)
                        if !matches!(self.state, AppState::Normal) {
                            self.selected_index = 0;
                            self.list_state.select(None);
                        } else {
                            // In normal mode, clamp selection to valid bounds
                            if !self.networks.is_empty() {
                                self.selected_index = self.selected_index.min(self.networks.len() - 1);
                                self.list_state.select(Some(self.selected_index));
                            } else {
                                self.selected_index = 0;
                                self.list_state.select(Some(0));
                            }
                        }
                    }
                } else if !self.networks.is_empty() {
                    self.selected_index = self.selected_index.min(self.networks.len() - 1);
                    self.list_state.select(Some(self.selected_index));
                } else {
                    self.selected_index = 0;
                    self.list_state.select(Some(0));
                }
            }
            Msg::Error(e) => {
                self.state = AppState::ShowingError { message: e };
                self.is_scanning = false;
            }
            Msg::DismissError => {
                self.state = AppState::Normal;
            }
            Msg::EnterInput => {
                if let Some(net) = self.networks.get(self.selected_index) {
                    // If network is active (connected), show disconnect confirmation
                    if net.active {
                        self.state = AppState::ConfirmDisconnect;
                    } else if net.weak_security {
                        // Show warning for insecure networks before connecting (even if known)
                        self.state = AppState::ConfirmWeakSecurity {
                            ssid: net.ssid.clone(),
                            security_type: net.security.clone(),
                        };
                    } else if net.known {
                        // Known secure network - connect directly without password prompt
                        self.state = AppState::Connecting {
                            ssid: net.ssid.clone(),
                            throbber_state: ThrobberState::default(),
                        };
                    } else {
                        // Unknown secure network - proceed to password input
                        self.state = AppState::EditingPassword {
                            password_input: Input::default(),
                            error_message: None,
                        };
                    }
                }
            }
            Msg::Input(c) => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::InsertChar(c));
                }
            }
            Msg::Backspace => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::DeletePrevChar);
                }
            }
            Msg::MoveCursorLeft => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::GoToPrevChar);
                }
            }
            Msg::MoveCursorRight => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::GoToNextChar);
                }
            }
            Msg::MoveCursorWordLeft => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::GoToPrevWord);
                }
            }
            Msg::MoveCursorWordRight => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::GoToNextWord);
                }
            }
            Msg::DeletePrevWord => {
                if let AppState::EditingPassword { password_input, .. } = &mut self.state {
                    password_input.handle(tui_input::InputRequest::DeletePrevWord);
                }
            }
            Msg::SubmitConnection => {
                // If we're in ConfirmWeakSecurity mode, check if network is known
                if let AppState::ConfirmWeakSecurity { ssid, .. } = &self.state {
                    if let Some(net) = self.networks.get(self.selected_index) {
                        if net.known {
                            // Known insecure network - connect directly
                            self.state = AppState::Connecting {
                                ssid: ssid.clone(),
                                throbber_state: ThrobberState::default(),
                            };
                        } else {
                            // Unknown insecure network - go to password input
                            self.state = AppState::EditingPassword {
                                password_input: Input::default(),
                                error_message: None,
                            };
                        }
                    }
                } else {
                    // Otherwise, we're submitting from Editing mode, so connect
                    let ssid = self
                        .networks
                        .get(self.selected_index)
                        .map(|n| n.ssid.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    self.state = AppState::Connecting {
                        ssid,
                        throbber_state: ThrobberState::default(),
                    };
                }
            }
            Msg::CancelInput => {
                self.state = AppState::Normal;
            }
            Msg::ConnectionSuccess => {
                self.state = AppState::Normal;
            }
            Msg::ConnectionFailure(error) => {
                // Special handling for password errors - return to password input
                if error.contains("INCORRECT_PASSWORD") {
                    self.state = AppState::EditingPassword {
                        password_input: Input::default(),
                        error_message: Some("Incorrect password. Try again.".to_string()),
                    };
                } else {
                    self.state = AppState::ShowingError {
                        message: format!("Connection failed: {}", error),
                    };
                }
            }
            Msg::SubmitDisconnect => {
                self.state = AppState::Normal;
            }
            Msg::DisconnectSuccess => {
                self.state = AppState::Normal;
            }
            Msg::DisconnectFailure(error) => {
                self.state = AppState::ShowingError {
                    message: format!("Disconnect failed: {}", error),
                };
            }
            Msg::ConfirmForget => {
                self.state = AppState::ConfirmForget;
            }
            Msg::SubmitForget => {
                self.state = AppState::Normal;
            }
            Msg::ForgetSuccess => {
                self.state = AppState::Normal;
            }
            Msg::ForgetFailure(error) => {
                self.state = AppState::ShowingError {
                    message: format!("Failed to forget network: {}", error),
                };
            }
            Msg::DPressed => {
                self.d_pressed = !self.d_pressed;
            }
            Msg::ToggleAutoconnect => {
                // No-op in app state - handled by network layer
            }
            Msg::AutoconnectSuccess => {
                // Auto-connect setting changed successfully - rescan will update UI
            }
            Msg::AutoconnectFailure(error) => {
                self.state = AppState::ShowingError {
                    message: format!("Failed to toggle auto-connect: {}", error),
                };
            }
        }
    }
}
