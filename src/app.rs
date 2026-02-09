use crate::network::{WifiDeviceInfo, WifiInfo};
use ratatui::widgets::ListState;

// TODO: split this up/come up with a better design
pub enum Msg {
  Tick,
  Quit,
  MoveUp,
  MoveDown,
  NetworksFound(Vec<WifiInfo>),
  DeviceInfoUpdate(WifiDeviceInfo),
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
  ConnectionFailure(anyhow::Error),
  SubmitDisconnect,
  DisconnectSuccess,
  DisconnectFailure(anyhow::Error),
  ConfirmForget,
  SubmitForget,
  ForgetSuccess,
  ForgetFailure(anyhow::Error),
  DPressed,
  ToggleAutoconnect,
  AutoconnectSuccess,
  AutoconnectFailure(anyhow::Error),
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
    /// The password being entered
    password: String,
    /// Cursor position in the password string
    cursor: usize,
  },
  /// Currently connecting to a network
  /// The ThrobberState is managed by ravel-tui's throbber builder
  Connecting { network: WifiInfo },
  /// Displaying an error message
  ShowingError { error: anyhow::Error },
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
      } => list_state.selected().and_then(|ix| networks.get(ix).cloned()),
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
        // Throbber animation is now handled by ravel-tui's throbber builder
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
          Some(ix) if networks.len() > 0 && ix == networks.len() - 1 => {
            // If we're focused on the last element, do nothing. Without this special case pressing down on the last element de-focuses it briefly.
          }
          _ if networks.is_empty() => {
            // No networks, nothing to select
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
        if let Some(net) = focused_network {
          // Try to find the previously selected network in the new list
          list_state.select(new_networks.iter().position(|n| n.ssid == net.ssid));
        } else {
          list_state.select_first();
        }

        *networks = new_networks;
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
            *state = AppState::Connecting { network: net.clone() };
          } else {
            // Unknown secure network - proceed to password input
            *state = AppState::EditingPassword {
              network: net.clone(),
              password: String::new(),
              cursor: 0,
            };
          }
        }
      }
      Msg::Input(c) => {
        if let AppState::EditingPassword {
          password, cursor, ..
        } = state
        {
          password.insert(*cursor, c);
          *cursor += 1;
        }
      }
      Msg::Backspace => {
        if let AppState::EditingPassword {
          password, cursor, ..
        } = state
        {
          if *cursor > 0 {
            *cursor -= 1;
            password.remove(*cursor);
          }
        }
      }
      Msg::MoveCursorLeft => {
        if let AppState::EditingPassword { cursor, .. } = state {
          if *cursor > 0 {
            *cursor -= 1;
          }
        }
      }
      Msg::MoveCursorRight => {
        if let AppState::EditingPassword {
          password, cursor, ..
        } = state
        {
          if *cursor < password.len() {
            *cursor += 1;
          }
        }
      }
      Msg::MoveCursorWordLeft => {
        if let AppState::EditingPassword {
          password, cursor, ..
        } = state
        {
          // Move to start of previous word
          while *cursor > 0 && password.chars().nth(*cursor - 1) == Some(' ') {
            *cursor -= 1;
          }
          while *cursor > 0 && password.chars().nth(*cursor - 1) != Some(' ') {
            *cursor -= 1;
          }
        }
      }
      Msg::MoveCursorWordRight => {
        if let AppState::EditingPassword {
          password, cursor, ..
        } = state
        {
          let len = password.len();
          // Move to end of current word
          while *cursor < len && password.chars().nth(*cursor) != Some(' ') {
            *cursor += 1;
          }
          // Skip spaces
          while *cursor < len && password.chars().nth(*cursor) == Some(' ') {
            *cursor += 1;
          }
        }
      }
      Msg::DeletePrevWord => {
        if let AppState::EditingPassword {
          password, cursor, ..
        } = state
        {
          let start = *cursor;
          // Skip spaces
          while *cursor > 0 && password.chars().nth(*cursor - 1) == Some(' ') {
            *cursor -= 1;
          }
          // Delete word
          while *cursor > 0 && password.chars().nth(*cursor - 1) != Some(' ') {
            *cursor -= 1;
          }
          password.drain(*cursor..start);
        }
      }
      Msg::SubmitConnection => {
        // If we're in ConfirmWeakSecurity mode, check if network is known
        if let AppState::ConfirmWeakSecurity { network } = &*state {
          if network.known {
            // Known insecure network - connect directly
            *state = AppState::Connecting {
              network: network.clone(),
            };
          } else {
            // Unknown insecure network - go to password input
            *state = AppState::EditingPassword {
              network: network.clone(),
              password: String::new(),
              cursor: 0,
            };
          }
        } else if let AppState::EditingPassword { network, .. } = &*state {
          // Otherwise, we're submitting from Editing mode, so connect
          *state = AppState::Connecting {
            network: network.clone(),
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
        *state = AppState::ShowingError { error };
      }
      Msg::SubmitDisconnect => {
        *state = AppState::Normal;
      }
      Msg::DisconnectSuccess => {
        *state = AppState::Normal;
      }
      Msg::DisconnectFailure(error) => {
        *state = AppState::ShowingError { error };
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
        *state = AppState::ShowingError { error };
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
        *state = AppState::ShowingError { error };
      }
    }
  }
}
