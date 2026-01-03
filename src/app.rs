use crate::network::{WifiDeviceInfo, WifiInfo};
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
  ShowingError { message: String },
  /// Confirming disconnect from active network
  ConfirmDisconnect,
  /// Confirming forgetting a known network
  ConfirmForget,
  /// Confirming connection to a network with weak/no security
  ConfirmWeakSecurity { ssid: String, security_type: String },
}

// TODO: there are still some type-driven design style refactors due here
pub enum App {
  Running {
    networks: Vec<WifiInfo>,
    selected_index: usize,
    list_state: ListState,
    is_scanning: bool,
    active_ssid: Option<String>,
    device_info: Option<WifiDeviceInfo>,
    state: AppState,
    show_detailed_view: bool,
  },
  ShouldQuit,
}

impl App {
  pub fn new() -> Self {
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    Self::Running {
      networks: Vec::new(),
      selected_index: 0,
      list_state,
      is_scanning: false,
      active_ssid: None,
      device_info: None,
      state: AppState::Normal,
      show_detailed_view: false,
    }
  }

  pub fn update(&mut self, msg: Msg) {
    // Exit early if already quitting
    if matches!(self, App::ShouldQuit) {
      return;
    }

    // Extract fields from Running variant for processing
    let App::Running {
      networks,
      selected_index,
      list_state,
      is_scanning,
      active_ssid,
      device_info,
      state,
      show_detailed_view: d_pressed,
    } = self
    else {
      return;
    };

    match msg {
      Msg::Tick => {
        if let AppState::Connecting { throbber_state, .. } = state {
          throbber_state.calc_next();
        }
      }
      Msg::Quit => {
        *self = App::ShouldQuit;
        return;
      }
      Msg::MoveUp => {
        // If nothing selected, select first network
        if list_state.selected().is_none() {
          if !networks.is_empty() {
            *selected_index = 0;
            list_state.select(Some(0));
          }
        } else if *selected_index > 0 {
          *selected_index -= 1;
          list_state.select(Some(*selected_index));
        }
      }
      Msg::MoveDown => {
        // If nothing selected, select first network
        if list_state.selected().is_none() {
          if !networks.is_empty() {
            *selected_index = 0;
            list_state.select(Some(0));
          }
        } else if *selected_index + 1 < networks.len() {
          *selected_index += 1;
          list_state.select(Some(*selected_index));
        }
      }
      Msg::Scan => {
        *is_scanning = true;
      }
      Msg::DeviceInfoUpdate(info) => {
        *device_info = Some(info);
      }
      Msg::NetworksFound(new_networks) => {
        *active_ssid = new_networks.iter().find(|n| n.active).map(|n| n.ssid.clone());

        // Preserve selection by SSID across rescans
        let previously_selected_ssid = networks.get(*selected_index).map(|n| n.ssid.clone());

        *networks = new_networks;
        *is_scanning = false;

        // Try to find the previously selected network in the new list
        if let Some(ssid) = previously_selected_ssid {
          if let Some(new_index) = networks.iter().position(|n| n.ssid == ssid) {
            *selected_index = new_index;
            list_state.select(Some(new_index));
          } else {
            // Network disappeared - show error if password dialog was open
            if matches!(state, AppState::EditingPassword { .. }) {
              *state = AppState::ShowingError {
                message: format!("Network \"{}\" is no longer available.", ssid),
              };
            }

            // Network disappeared - deselect in all modes
            *selected_index = 0;
            list_state.select(None);
          }
        } else {
          *selected_index = 0;
          list_state.select(None);
        }
      }
      Msg::Error(e) => {
        *state = AppState::ShowingError { message: e };
        *is_scanning = false;
      }
      Msg::DismissError => {
        *state = AppState::Normal;
      }
      Msg::EnterInput => {
        if let Some(net) = networks.get(*selected_index) {
          // If network is active (connected), show disconnect confirmation
          if net.active {
            *state = AppState::ConfirmDisconnect;
          } else if net.weak_security {
            // Show warning for insecure networks before connecting (even if known)
            *state = AppState::ConfirmWeakSecurity {
              ssid: net.ssid.clone(),
              security_type: net.security.clone(),
            };
          } else if net.known {
            // Known secure network - connect directly without password prompt
            *state = AppState::Connecting {
              ssid: net.ssid.clone(),
              throbber_state: ThrobberState::default(),
            };
          } else {
            // Unknown secure network - proceed to password input
            *state = AppState::EditingPassword {
              password_input: Input::default(),
              error_message: None,
            };
          }
        }
      }
      Msg::Input(c) => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::InsertChar(c));
        }
      }
      Msg::Backspace => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::DeletePrevChar);
        }
      }
      Msg::MoveCursorLeft => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::GoToPrevChar);
        }
      }
      Msg::MoveCursorRight => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::GoToNextChar);
        }
      }
      Msg::MoveCursorWordLeft => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::GoToPrevWord);
        }
      }
      Msg::MoveCursorWordRight => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::GoToNextWord);
        }
      }
      Msg::DeletePrevWord => {
        if let AppState::EditingPassword { password_input, .. } = state {
          password_input.handle(tui_input::InputRequest::DeletePrevWord);
        }
      }
      Msg::SubmitConnection => {
        // If we're in ConfirmWeakSecurity mode, check if network is known
        if let AppState::ConfirmWeakSecurity { ssid: confirm_ssid, .. } = &*state {
          if let Some(net) = networks.get(*selected_index) {
            if net.known {
              // Known insecure network - connect directly
              *state = AppState::Connecting {
                ssid: confirm_ssid.clone(),
                throbber_state: ThrobberState::default(),
              };
            } else {
              // Unknown insecure network - go to password input
              *state = AppState::EditingPassword {
                password_input: Input::default(),
                error_message: None,
              };
            }
          }
        } else {
          // Otherwise, we're submitting from Editing mode, so connect
          let ssid = networks
            .get(*selected_index)
            .map(|n| n.ssid.clone())
            .unwrap_or_else(|| "Unknown".to_string());
          *state = AppState::Connecting {
            ssid,
            throbber_state: ThrobberState::default(),
          };
        }
      }
      Msg::CancelInput => {
        *state = AppState::Normal;
      }
      Msg::ConnectionSuccess => {
        *state = AppState::Normal;
      }
      Msg::ConnectionFailure(error) => {
        // Special handling for password errors - return to password input
        if error.contains("INCORRECT_PASSWORD") {
          *state = AppState::EditingPassword {
            password_input: Input::default(),
            error_message: Some("Incorrect password. Try again.".to_string()),
          };
        } else {
          *state = AppState::ShowingError {
            message: format!("Connection failed: {}", error),
          };
        }
      }
      Msg::SubmitDisconnect => {
        *state = AppState::Normal;
      }
      Msg::DisconnectSuccess => {
        *state = AppState::Normal;
      }
      Msg::DisconnectFailure(error) => {
        *state = AppState::ShowingError {
          message: format!("Disconnect failed: {}", error),
        };
      }
      Msg::ConfirmForget => {
        *state = AppState::ConfirmForget;
      }
      Msg::SubmitForget => {
        *state = AppState::Normal;
      }
      Msg::ForgetSuccess => {
        *state = AppState::Normal;
      }
      Msg::ForgetFailure(error) => {
        *state = AppState::ShowingError {
          message: format!("Failed to forget network: {}", error),
        };
      }
      Msg::DPressed => {
        *d_pressed = !*d_pressed;
      }
      Msg::ToggleAutoconnect => {
        // No-op in app state - handled by network layer
      }
      Msg::AutoconnectSuccess => {
        // Auto-connect setting changed successfully - rescan will update UI
      }
      Msg::AutoconnectFailure(error) => {
        *state = AppState::ShowingError {
          message: format!("Failed to toggle auto-connect: {}", error),
        };
      }
    }
  }
}
