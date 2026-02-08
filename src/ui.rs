//! UI components using ravel-tui declarative builders.

use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{BorderType, Borders, ListItem};
use ravel::with;
use ravel_tui::{any, block, input, list, modal, text, throbber, vstack, View};

use crate::app::{App, AppState};
use crate::network::{WifiDeviceInfo, WifiInfo};

/// Main UI component - composes header, network list, footer, and any dialogs.
pub fn ui(app: &App) -> View!() {
    with(move |cx| {
        let App::Running {
            networks,
            list_state,
            device_info,
            state,
            show_detailed_view,
        } = app
        else {
            return cx.build(any(()));
        };

        let is_dialog_open = !matches!(state, AppState::Normal);

        cx.build(any((
            // Main layout: header, list, footer
            vstack((
                header(device_info, networks, is_dialog_open),
                network_list(networks, list_state, *show_detailed_view, is_dialog_open),
                footer(is_dialog_open),
            ))
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ]),
            // Dialog overlay
            dialog(state),
        )))
    })
}

/// Header showing WiFi status.
fn header(
    device_info: &Option<WifiDeviceInfo>,
    networks: &[WifiInfo],
    is_dimmed: bool,
) -> View!() {
    let wifi_disabled = device_info.as_ref().is_some_and(|info| !info.wifi_enabled);
    let is_connected = networks.iter().any(|n| n.active);

    let style = if wifi_disabled {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if !is_connected {
        Style::default()
            .fg(Color::Rgb(255, 165, 0))
            .add_modifier(Modifier::BOLD)
    } else if is_dimmed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };

    let block_style = if wifi_disabled {
        Style::default().fg(Color::Red)
    } else if !is_connected {
        Style::default().fg(Color::Rgb(255, 165, 0))
    } else if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };

    let header_text = if let Some(info) = device_info {
        let enabled_status = if info.wifi_enabled {
            "enabled"
        } else {
            "disabled"
        };
        let connected = networks.iter().any(|n| n.active);
        let connection_status = if connected {
            "connected"
        } else {
            "not connected"
        };
        format!("WeeFee | WiFi {}, {}", enabled_status, connection_status)
    } else {
        "WeeFee | Loading...".to_string()
    };

    block(text(header_text).style(style))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(block_style)
}

/// Network list with selection.
fn network_list(
    networks: &[WifiInfo],
    list_state: &ratatui::widgets::ListState,
    show_detailed_view: bool,
    is_dimmed: bool,
) -> View!() {
    let selected = list_state.selected();

    let block_style = if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };

    list(networks, move |net, i, _is_sel| {
        network_item(net, i, selected == Some(i), show_detailed_view, is_dimmed)
    })
    .title("Networks")
    .block_style(block_style)
}

/// Single network item in the list.
fn network_item(
    net: &WifiInfo,
    _index: usize,
    is_selected: bool,
    show_detailed_view: bool,
    is_dimmed: bool,
) -> ListItem<'static> {
    let main_style = if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else if is_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let prefix = if is_selected { "→ " } else { "  " };
    let active_marker = if net.active { "🔗 " } else { "   " };

    let signal_indicator = match net.strength {
        0..=25 => "▁    ",
        26..=50 => "▁▃   ",
        51..=75 => "▁▃▅  ",
        _ => "▁▃▅▇ ",
    };

    let signal_style = if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else if is_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let detail_style = Style::default().fg(Color::DarkGray);

    if show_detailed_view {
        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{}{}", prefix, active_marker), main_style),
            Span::styled(signal_indicator, signal_style),
            Span::styled(net.ssid.clone(), main_style),
        ])];

        let mut detail_parts = vec![];
        detail_parts.push(format!("signal: {}%", net.strength));

        if let Some(freq) = net.frequency {
            let band = if (2412..=2484).contains(&freq) {
                "2.4 GHz"
            } else if (5170..=5835).contains(&freq) {
                "5 GHz"
            } else if (5945..=7125).contains(&freq) {
                "6 GHz"
            } else {
                "unknown band"
            };
            detail_parts.push(format!("frequency: {} MHz ({})", freq, band));
        }

        let warning = if net.weak_security {
            " (⚠ insecure)"
        } else {
            ""
        };
        detail_parts.push(format!("security: {}{}", net.security, warning));

        if net.known {
            detail_parts.push("known network (F to forget)".to_string());
        }

        let detail_indent = Span::styled("          ", detail_style);
        lines.push(
            Line::from(vec![
                detail_indent.clone(),
                Span::styled(detail_parts.join(" | "), detail_style),
            ])
            .style(detail_style),
        );

        if net.known {
            let mut advanced_parts = vec![];

            if let Some(p) = net.priority {
                advanced_parts.push(format!("priority: {}", p));
            }

            match net.autoconnect {
                Some(true) => advanced_parts.push("auto-connect: on (A to toggle)".to_string()),
                Some(false) => advanced_parts.push("auto-connect: off (A to toggle)".to_string()),
                None => advanced_parts.push("auto-connect: default (A to toggle)".to_string()),
            }

            match net.autoconnect_retries {
                Some(r) => advanced_parts.push(format!("auto-connect retries: {}", r)),
                None => advanced_parts.push("auto-connect retries: default".to_string()),
            }

            if !advanced_parts.is_empty() {
                lines.push(
                    Line::from(vec![
                        detail_indent,
                        Span::styled(advanced_parts.join(" | "), detail_style),
                    ])
                    .style(detail_style),
                );
            }
        }

        ListItem::new(lines)
    } else {
        let content = Line::from(vec![
            Span::styled(format!("{}{}", prefix, active_marker), main_style),
            Span::styled(signal_indicator, signal_style),
            Span::styled(net.ssid.clone(), main_style),
        ]);
        ListItem::new(content)
    }
}

/// Footer with keyboard shortcuts.
fn footer(_is_dimmed: bool) -> View!() {
    text("↑/↓: Navigate | Enter to dis/connect | D: Details | Q: Quit")
        .style(Style::default().fg(Color::DarkGray))
}

/// Dialog overlay based on current app state.
fn dialog(state: &AppState) -> View!() {
    // Clone data out of state to avoid borrowing issues with closures
    let dialog_data = match state {
        AppState::Normal => DialogData::None,
        AppState::EditingPassword { network } => DialogData::Password(network.clone()),
        AppState::Connecting { .. } => DialogData::Connecting,
        AppState::ShowingError { error } => DialogData::Error(format!("{:#}", error)),
        AppState::ConfirmDisconnect { network } => DialogData::Disconnect(network.clone()),
        AppState::ConfirmForget { network } => DialogData::Forget(network.clone()),
        AppState::ConfirmWeakSecurity { network } => DialogData::WeakSecurity(network.clone()),
    };

    with(move |cx| match &dialog_data {
        DialogData::None => cx.build(any(())),
        DialogData::Password(network) => cx.build(any(password_dialog(network))),
        DialogData::Connecting => cx.build(any(connecting_dialog())),
        DialogData::Error(msg) => cx.build(any(error_dialog_msg(msg))),
        DialogData::Disconnect(network) => cx.build(any(disconnect_dialog(network))),
        DialogData::Forget(network) => cx.build(any(forget_dialog(network))),
        DialogData::WeakSecurity(network) => cx.build(any(weak_security_dialog(network))),
    })
}

/// Helper enum for dialog data to avoid lifetime issues.
enum DialogData {
    None,
    Password(WifiInfo),
    Connecting,
    Error(String),
    Disconnect(WifiInfo),
    Forget(WifiInfo),
    WeakSecurity(WifiInfo),
}

/// Password input dialog.
fn password_dialog(network: &WifiInfo) -> View!() {
    modal(
        vstack((
            // SSID info
            block(text(format!("Connecting to {}...", network.ssid)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
            // Password input
            block(input().style(Style::default().fg(Color::Yellow)))
                .title("Password")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        ))
        .constraints([Constraint::Length(3), Constraint::Length(3)]),
    )
    .width(50)
    .height(6)
}

/// Connecting throbber dialog.
fn connecting_dialog() -> View!() {
    modal(
        block(
            throbber("Connecting...")
                .style(Style::default().fg(Color::Yellow))
                .throbber_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Cyan)),
    )
    .width(30)
    .height(3)
}

/// Error dialog with a string message.
fn error_dialog_msg(error_msg: &str) -> View!() {
    modal(
        block(
            vstack((
                text(error_msg.to_string())
                    .style(Style::default().fg(Color::White))
                    .wrap(),
                text("Enter or Esc to dismiss")
                    .style(Style::default().fg(Color::DarkGray)),
            ))
            .constraints([Constraint::Min(0), Constraint::Length(2)]),
        )
        .title("Error")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Red)),
    )
    .percent(60, 25)
}

/// Disconnect confirmation dialog.
fn disconnect_dialog(network: &WifiInfo) -> View!() {
    modal(
        block(
            vstack((
                text(format!("Disconnect from {}?", network.ssid))
                    .style(Style::default().fg(Color::White)),
                yes_no_prompt(),
            ))
            .constraints([Constraint::Min(0), Constraint::Length(2)]),
        )
        .title("Disconnect")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Yellow)),
    )
    .percent(60, 25)
}

/// Forget network confirmation dialog.
fn forget_dialog(network: &WifiInfo) -> View!() {
    let message = if network.active {
        "This will disconnect and delete the saved password and settings."
    } else {
        "This will delete the saved password and settings."
    };

    modal(
        block(
            vstack((
                text(format!("Forget network {}?\n\n{}", network.ssid, message))
                    .style(Style::default().fg(Color::White))
                    .wrap(),
                yes_no_prompt(),
            ))
            .constraints([Constraint::Min(0), Constraint::Length(2)]),
        )
        .title("Forget Network")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Red)),
    )
    .percent(60, 25)
}

/// Weak security warning dialog.
fn weak_security_dialog(network: &WifiInfo) -> View!() {
    let message = if network.security == "Open" {
        format!(
            "Network {} has no security. Anyone can intercept your data.",
            network.ssid
        )
    } else if network.security.contains("WEP") {
        format!(
            "Network {} uses {}.\nWEP is outdated and can be cracked in minutes. Your data can be easily intercepted by attackers.",
            network.ssid, network.security
        )
    } else {
        format!(
            "Network {} uses {}.\nThis encryption method is outdated and insecure. Your data may be vulnerable to interception.",
            network.ssid, network.security
        )
    };

    modal(
        block(
            vstack((
                text(message)
                    .style(Style::default().fg(Color::White))
                    .wrap(),
                text("Continue anyway? Y/N")
                    .style(Style::default().fg(Color::White)),
            ))
            .constraints([Constraint::Min(0), Constraint::Length(2)]),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Red)),
    )
    .percent(70, 30)
}

/// Yes/No prompt text.
fn yes_no_prompt() -> View!() {
    text("Yes / No").style(Style::default().fg(Color::DarkGray))
}
