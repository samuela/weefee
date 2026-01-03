use crate::network::{WifiDeviceInfo, WifiInfo};
use ratatui::widgets::ListState;
use throbber_widgets_tui::ThrobberState;
use tui_input::Input;

// TODO: split this up/come up with a better design
pub enum Msg {
  Tick,
  Quit,
  MoveUp,
  MoveDown,
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
    network: WifiInfo,
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
  ConfirmDisconnect { network: WifiInfo },
  /// Confirming forgetting a known network
  ConfirmForget { network: WifiInfo },
  /// Confirming connection to a network with weak/no security
  ConfirmWeakSecurity { network: WifiInfo },
}

// TODO: there are still some type-driven design style refactors due here
pub enum App {
  Running {
    networks: Vec<WifiInfo>,
    list_state: ListState,
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
      list_state,
      device_info: None,
      state: AppState::Normal,
      show_detailed_view: false,
    }
  }

  pub fn focused_network(&self) -> Option<WifiInfo> {
    match self {
      Self::ShouldQuit => None,
      Self::Running {
        networks, list_state, ..
      } => list_state.selected().map(|ix| networks[ix].clone()),
    }
  }

  pub fn update(&mut self, msg: Msg) {
    // Exit early if already quitting
    if matches!(self, App::ShouldQuit) {
      return;
    }

    // Extract fields from Running variant for processing
    let focused_network = self.focused_network().clone();
    let App::Running {
      networks,
      list_state,
      device_info,
      state,
      show_detailed_view,
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
        list_state.select_previous();
      }
      Msg::MoveDown => {
        match list_state.selected() {
          Some(ix) if ix == networks.len() - 1 => {
            // If we're focused on the last element, do nothing. Without this special case pressing down on the last element de-focuses it briefly.
          }
          _ => list_state.select_next(),
        }
      }
      Msg::DeviceInfoUpdate(info) => {
        *device_info = Some(info);
      }
      Msg::NetworksFound(new_networks) => {
        // Preserve selection by SSID across rescans
        // TODO: should we use some other kind of network ID?
        // TODO: handle the case where there's not a previously selected network
        if let Some(ix) = list_state.selected() {
          // Try to find the previously selected network in the new list
          // TODO: should compare on id here?
          list_state.select(networks.iter().position(|n| n.ssid == networks[ix].ssid));
        } else {
          list_state.select_first();
        }

        *networks = new_networks;
      }
      Msg::Error(e) => {
        *state = AppState::ShowingError { message: e };
      }
      Msg::DismissError => {
        *state = AppState::Normal;
      }
      Msg::EnterInput => {
        if let Some(net) = focused_network {
          // If network is active (connected), show disconnect confirmation
          if net.active {
            *state = AppState::ConfirmDisconnect { network: net };
          } else if net.weak_security {
            // Show warning for insecure networks before connecting (even if known)
            *state = AppState::ConfirmWeakSecurity { network: net };
          } else if net.known {
            // Known secure network - connect directly without password prompt
            *state = AppState::Connecting {
              ssid: net.ssid.clone(),
              throbber_state: ThrobberState::default(),
            };
          } else {
            // Unknown secure network - proceed to password input
            *state = AppState::EditingPassword {
              network: net.clone(),
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
        if let AppState::ConfirmWeakSecurity { network } = &*state {
          if network.known {
            // Known insecure network - connect directly
            *state = AppState::Connecting {
              ssid: network.ssid.clone(),
              throbber_state: ThrobberState::default(),
            };
          } else {
            // Unknown insecure network - go to password input
            *state = AppState::EditingPassword {
              network: network.clone(),
              password_input: Input::default(),
              error_message: None,
            };
          }
        } else if let AppState::EditingPassword { network, .. } = &*state {
          // Otherwise, we're submitting from Editing mode, so connect
          *state = AppState::Connecting {
            ssid: network.ssid.clone(),
            throbber_state: ThrobberState::default(),
          };
        } else {
          panic!("this should never happen");
        }
      }
      Msg::CancelInput => {
        *state = AppState::Normal;
      }
      Msg::ConnectionSuccess => {
        *state = AppState::Normal;
      }
      Msg::ConnectionFailure(error) => {
        *state = AppState::ShowingError {
          message: format!("Connection failed: {}", error),
        };
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
        if let Some(net) = focused_network {
          *state = AppState::ConfirmForget { network: net };
        }
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
        *show_detailed_view = !*show_detailed_view;
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
