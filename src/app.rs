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
    ConfirmDisconnect,
    SubmitDisconnect,
    DisconnectSuccess,
    DisconnectFailure(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
    Connecting,
    Error,
    ConfirmDisconnect,
}

pub struct App {
    pub should_quit: bool,
    pub networks: Vec<WifiInfo>,
    pub selected_index: usize,
    pub list_state: ListState,
    pub is_scanning: bool,
    pub active_ssid: Option<String>,
    pub device_info: Option<WifiDeviceInfo>,
    pub input_mode: InputMode,
    pub password_input: Input,
    pub connecting_ssid: Option<String>,
    pub password_error: Option<String>,
    pub error_message: Option<String>,
    pub throbber_state: ThrobberState,
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
            input_mode: InputMode::Normal,
            password_input: Input::default(),
            connecting_ssid: None,
            password_error: None,
            error_message: None,
            throbber_state: ThrobberState::default(),
        }
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Tick => {
                if self.input_mode == InputMode::Connecting {
                    self.throbber_state.calc_next();
                }
            }
            Msg::Quit => self.should_quit = true,
            Msg::MoveUp => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.list_state.select(Some(self.selected_index));
                }
            }
            Msg::MoveDown => {
                if self.selected_index + 1 < self.networks.len() {
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
                        if self.input_mode == InputMode::Editing {
                            self.input_mode = InputMode::Error;
                            self.error_message =
                                Some(format!("Network \"{}\" is no longer available.", ssid));
                            self.password_input.reset();
                            self.password_error = None;
                        }
                        // Clamp selection to valid bounds
                        if !self.networks.is_empty() {
                            self.selected_index = self.selected_index.min(self.networks.len() - 1);
                            self.list_state.select(Some(self.selected_index));
                        } else {
                            self.selected_index = 0;
                            self.list_state.select(Some(0));
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
                self.error_message = Some(e);
                self.input_mode = InputMode::Error;
                self.is_scanning = false;
            }
            Msg::DismissError => {
                self.error_message = None;
                self.input_mode = InputMode::Normal;
            }
            Msg::EnterInput => {
                if let Some(net) = self.networks.get(self.selected_index) {
                    // If network is active (connected), show disconnect confirmation
                    if net.active {
                        self.input_mode = InputMode::ConfirmDisconnect;
                    } else if net.known {
                        // Known network - connect directly without password prompt
                        self.input_mode = InputMode::Connecting;
                        let ssid = net.ssid.clone();
                        self.connecting_ssid = Some(ssid);
                    } else if net.security == "Open" || net.security.contains("WEP/Open") {
                        self.input_mode = InputMode::Editing;
                        self.password_input.reset();
                        self.password_error = None;
                    } else {
                        self.input_mode = InputMode::Editing;
                        self.password_input.reset();
                        self.password_error = None;
                    }
                }
            }
            Msg::Input(c) => {
                self.password_input
                    .handle(tui_input::InputRequest::InsertChar(c));
            }
            Msg::Backspace => {
                self.password_input
                    .handle(tui_input::InputRequest::DeletePrevChar);
            }
            Msg::MoveCursorLeft => {
                self.password_input
                    .handle(tui_input::InputRequest::GoToPrevChar);
            }
            Msg::MoveCursorRight => {
                self.password_input
                    .handle(tui_input::InputRequest::GoToNextChar);
            }
            Msg::MoveCursorWordLeft => {
                self.password_input
                    .handle(tui_input::InputRequest::GoToPrevWord);
            }
            Msg::MoveCursorWordRight => {
                self.password_input
                    .handle(tui_input::InputRequest::GoToNextWord);
            }
            Msg::DeletePrevWord => {
                self.password_input
                    .handle(tui_input::InputRequest::DeletePrevWord);
            }
            Msg::SubmitConnection => {
                self.input_mode = InputMode::Connecting;
                let ssid = self
                    .networks
                    .get(self.selected_index)
                    .map(|n| n.ssid.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                self.connecting_ssid = Some(ssid);
            }
            Msg::CancelInput => {
                self.input_mode = InputMode::Normal;
                self.password_input.reset();
                self.password_error = None;
            }
            Msg::ConnectionSuccess => {
                self.input_mode = InputMode::Normal;
                self.connecting_ssid = None;
                self.password_error = None;
            }
            Msg::ConnectionFailure(error) => {
                // Special handling for password errors - return to password input
                // TODO: move this into the type system
                if error == "INCORRECT_PASSWORD" {
                    self.input_mode = InputMode::Editing;
                    self.connecting_ssid = None;
                    self.password_input.reset();
                    self.password_error = Some("Incorrect password. Try again.".to_string());
                } else {
                    self.input_mode = InputMode::Error;
                    self.connecting_ssid = None;
                    self.password_error = None;
                    self.error_message = Some(format!("Connection failed: {}", error));
                }
            }
            Msg::ConfirmDisconnect => {
                self.input_mode = InputMode::ConfirmDisconnect;
            }
            Msg::SubmitDisconnect => {
                self.input_mode = InputMode::Normal;
            }
            Msg::DisconnectSuccess => {
                self.input_mode = InputMode::Normal;
            }
            Msg::DisconnectFailure(error) => {
                self.input_mode = InputMode::Error;
                self.error_message = Some(format!("Disconnect failed: {}", error));
            }
        }
    }
}
