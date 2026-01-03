use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use throbber_widgets_tui::{CANADIAN, Throbber, WhichUse};

use crate::app::{App, AppState};
use crate::network::WifiDeviceInfo;
use crate::network::WifiInfo;

pub fn draw(f: &mut Frame, app: &mut App) {
    // Early return if app is quitting
    let App::Running {
        networks,
        selected_index,
        list_state,
        is_scanning: _,
        active_ssid: _,
        device_info,
        state,
        d_pressed,
    } = app else {
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // List
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    let is_dialog_open = !matches!(state, AppState::Normal);
    draw_header(f, device_info, networks, chunks[0], is_dialog_open);
    draw_network_list(f, networks, *selected_index, list_state, *d_pressed, chunks[1], is_dialog_open);
    draw_footer(f, chunks[2], is_dialog_open);

    match state {
        AppState::EditingPassword { password_input, error_message } => {
            // Get the SSID we're connecting to
            let ssid = networks
                .get(*selected_index)
                .map(|n| n.ssid.as_str())
                .unwrap_or("Unknown");

            // Calculate base position for all blocks
            let base_area = centered_rect_fixed(50, 3, f.area());
            let mut current_y = base_area.y;

            // SSID info block at the top
            let ssid_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            let ssid_area = Rect {
                x: base_area.x,
                y: current_y,
                width: base_area.width,
                height: 3,
            };
            f.render_widget(Clear, ssid_area);
            f.render_widget(ssid_block, ssid_area);

            let ssid_inner = Rect {
                x: ssid_area.x + 1,
                y: ssid_area.y + 1,
                width: ssid_area.width.saturating_sub(2),
                height: 1,
            };

            use ratatui::text::{Line, Span};
            let ssid_text = Line::from(vec![
                Span::raw("Connecting to "),
                Span::styled(ssid, Style::default().fg(Color::Yellow)),
                Span::raw("..."),
            ]);
            let ssid_widget = Paragraph::new(ssid_text);
            f.render_widget(ssid_widget, ssid_inner);

            current_y += 3;

            // Show error message if present in a separate block
            if let Some(error) = error_message {
                let error_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(Color::Red));
                let error_area = Rect {
                    x: base_area.x,
                    y: current_y,
                    width: base_area.width,
                    height: 3,
                };
                f.render_widget(Clear, error_area);
                f.render_widget(error_block, error_area);

                let error_inner = Rect {
                    x: error_area.x + 1,
                    y: error_area.y + 1,
                    width: error_area.width.saturating_sub(2),
                    height: 1,
                };
                let error_widget =
                    Paragraph::new(error.as_str()).style(Style::default().fg(Color::Red));
                f.render_widget(error_widget, error_inner);

                current_y += 3;
            }

            // Password input block
            let password_block = Block::default()
                .title("Password")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            let password_area = Rect {
                x: base_area.x,
                y: current_y,
                width: base_area.width,
                height: 3,
            };
            f.render_widget(Clear, password_area);
            f.render_widget(password_block, password_area);

            // Calculate inner area for the text input
            let inner_area = Rect {
                x: password_area.x + 1,
                y: password_area.y + 1,
                width: password_area.width.saturating_sub(2),
                height: 1,
            };

            let scroll = password_input.visual_scroll(inner_area.width as usize);
            let input_widget = Paragraph::new(password_input.value())
                .style(Style::default().fg(Color::Yellow))
                .scroll((0, scroll as u16));
            f.render_widget(input_widget, inner_area);

            // Set cursor position
            f.set_cursor_position((
                inner_area.x + ((password_input.visual_cursor()).max(scroll) - scroll) as u16,
                inner_area.y,
            ));
        }
        AppState::Connecting { throbber_state, .. } => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(Color::Cyan));
            let area = centered_rect_fixed(30, 3, f.area());
            f.render_widget(Clear, area); // Clear background
            f.render_widget(block, area);

            // Calculate inner area with margin for the content
            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            // Throbber on first line
            let throbber_area = Rect {
                x: inner_area.x,
                y: inner_area.y,
                width: inner_area.width,
                height: 1,
            };
            let throbber = Throbber::default()
                .label("Connecting...")
                .style(Style::default().fg(Color::Yellow))
                .throbber_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .throbber_set(CANADIAN)
                .use_type(WhichUse::Spin);
            f.render_stateful_widget(throbber, throbber_area, throbber_state);
        }
        AppState::Normal => {}
        AppState::ConfirmDisconnect => {
            let ssid = networks
                .get(*selected_index)
                .map(|n| n.ssid.as_str())
                .unwrap_or("Unknown");

            let block = Block::default()
                .title("Disconnect")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(Color::Yellow));
            let area = centered_rect(60, 25, f.area());
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            use ratatui::text::{Line, Span};

            // Split inner area: message area (flexible) and prompt at bottom (1 line)
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),      // Message area
                    Constraint::Length(2),   // Blank line + prompt
                ])
                .split(inner_area);

            let message_lines = vec![
                Line::from(vec![
                    Span::raw("Disconnect from "),
                    Span::styled(ssid, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw("?"),
                ]),
            ];

            let message = Paragraph::new(message_lines)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: true });
            f.render_widget(message, layout[0]);

            // Render prompt at bottom, centered
            let prompt_line = Line::from(vec![
                Span::styled("Y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("es / "),
                Span::styled("N", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("o"),
            ]);
            let prompt_widget = Paragraph::new(vec![Line::from(""), prompt_line])
                .style(Style::default().fg(Color::White))
                .alignment(ratatui::layout::Alignment::Center);
            f.render_widget(prompt_widget, layout[1]);
        }
        AppState::ConfirmForget => {
            let network = networks.get(*selected_index);
            let ssid = network.map(|n| n.ssid.as_str()).unwrap_or("Unknown");
            let is_active = network.map(|n| n.active).unwrap_or(false);

            let block = Block::default()
                .title("Forget Network")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(Color::Red));
            let area = centered_rect(60, 25, f.area());
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            use ratatui::text::{Line, Span};

            // Split inner area: message area (flexible) and prompt at bottom (1 line)
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),      // Message area
                    Constraint::Length(2),   // Blank line + prompt
                ])
                .split(inner_area);

            let mut message_lines = vec![
                Line::from(vec![
                    Span::raw("Forget network "),
                    Span::styled(ssid, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw("?"),
                ]),
                Line::from(""),
            ];

            if is_active {
                message_lines.push(Line::from("This will disconnect and delete the saved password and settings."));
            } else {
                message_lines.push(Line::from("This will delete the saved password and settings."));
            }

            let message = Paragraph::new(message_lines)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: true });
            f.render_widget(message, layout[0]);

            // Render prompt at bottom, centered
            let prompt_line = Line::from(vec![
                Span::styled("Y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("es / "),
                Span::styled("N", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("o"),
            ]);
            let prompt_widget = Paragraph::new(vec![Line::from(""), prompt_line])
                .style(Style::default().fg(Color::White))
                .alignment(ratatui::layout::Alignment::Center);
            f.render_widget(prompt_widget, layout[1]);
        }
        AppState::ConfirmWeakSecurity { ssid, security_type } => {

            use ratatui::text::{Line, Span};
            let mut message_lines = vec![];

            // Distinguish between no security and weak security
            if security_type == "Open" {
                message_lines.push(Line::from(vec![
                    Span::raw("Network "),
                    Span::styled(ssid.as_str(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(" has "),
                    Span::styled("no security", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::raw(". Anyone can intercept your data."),
                ]));
            } else {
                // Weak security (WEP or similar)
                message_lines.push(Line::from(vec![
                    Span::raw("Network "),
                    Span::styled(ssid.as_str(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(" uses "),
                    Span::styled(security_type.as_str(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::raw("."),
                ]));

                if security_type.contains("WEP") {
                    message_lines.push(Line::from("WEP is outdated and can be cracked in minutes. Your data can be easily intercepted by attackers."));
                } else {
                    message_lines.push(Line::from("This encryption method is outdated and insecure. Your data may be vulnerable to interception."));
                }
            }

            message_lines.push(Line::from(""));
            message_lines.push(Line::from(vec![
                Span::styled("Continue anyway? ", Style::default().fg(Color::White)),
                Span::styled("Y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("es / "),
                Span::styled("N", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("o"),
            ]));

            let block = Block::default()
                // .title("⚠ Security Warning")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(Color::Red));

            let area = centered_rect(70, 30, f.area());
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            // Split inner area: message area (flexible) and prompt at bottom (1 line)
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),      // Message area
                    Constraint::Length(2),   // Blank line + prompt
                ])
                .split(inner_area);

            // Remove the last two lines from message_lines (blank line + prompt)
            let prompt_line = message_lines.pop();
            let _blank_line = message_lines.pop();

            let message = Paragraph::new(message_lines)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: true });
            f.render_widget(message, layout[0]);

            // Render prompt at bottom, centered
            if let Some(prompt) = prompt_line {
                let prompt_widget = Paragraph::new(vec![Line::from(""), prompt])
                    .style(Style::default().fg(Color::White))
                    .alignment(ratatui::layout::Alignment::Center);
                f.render_widget(prompt_widget, layout[1]);
            }
        }
        AppState::ShowingError { message } => {
            let block = Block::default()
                .title("Error")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(Color::Red));
            let area = centered_rect(60, 25, f.area());
            f.render_widget(Clear, area); // Clear background
            f.render_widget(block, area);

            // Calculate inner area with margin for the message
            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            let error_display =
                Paragraph::new(format!("{}\n\nPress Enter or Esc to dismiss.", message))
                    .style(Style::default().fg(Color::White))
                    .wrap(Wrap { trim: true });
            f.render_widget(error_display, inner_area);
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn centered_rect_fixed(width: u16, height: u16, r: Rect) -> Rect {
    let vertical_margin = r.height.saturating_sub(height) / 2;
    let horizontal_margin = r.width.saturating_sub(width) / 2;

    Rect {
        x: r.x + horizontal_margin,
        y: r.y + vertical_margin,
        width: width.min(r.width),
        height: height.min(r.height),
    }
}

fn draw_header(f: &mut Frame, device_info: &Option<WifiDeviceInfo>, networks: &[WifiInfo], area: Rect, is_dimmed: bool) {
    // Check if WiFi is disabled
    let wifi_disabled = device_info.as_ref().map_or(false, |info| !info.wifi_enabled);
    // Check if we're connected to any network
    let is_connected = networks.iter().any(|n| n.active);

    let style = if wifi_disabled {
        // WiFi is disabled - use red color
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else if !is_connected {
        // WiFi is enabled but not connected - use orange color
        Style::default()
            .fg(Color::Rgb(255, 165, 0))
            .add_modifier(Modifier::BOLD)
    } else if is_dimmed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };
    let block_style = if wifi_disabled {
        // WiFi is disabled - use red border
        Style::default().fg(Color::Red)
    } else if !is_connected {
        // WiFi is enabled but not connected - use orange border
        Style::default().fg(Color::Rgb(255, 165, 0))
    } else if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };

    let header_text = if let Some(info) = device_info {
        // TODO: make the disabled thing a bit louder, eg with emojis or color change
        let enabled_status = if info.wifi_enabled { "enabled" } else { "disabled" };
        let connected = networks.iter().any(|n| n.active);
        let connection_status = if connected { "connected" } else { "not connected" };
        format!("WeeFee | WiFi {}, {}", enabled_status, connection_status)
    } else {
        "WeeFee | Loading...".to_string()
    };

    let text = Paragraph::new(header_text)
        .style(style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(block_style),
        );
    f.render_widget(text, area);
}

fn draw_network_list(f: &mut Frame, networks: &[WifiInfo], selected_index: usize, list_state: &mut ListState, d_pressed: bool, area: Rect, is_dimmed: bool) {
    use ratatui::text::{Line, Span};

    let items: Vec<ListItem> = networks
        .iter()
        .enumerate()
        .map(|(i, net)| {
            let main_style = if is_dimmed {
                Style::default().fg(Color::DarkGray)
            } else if i == selected_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = if i == selected_index { "→ " } else { "  " };
            // let active_marker = if net.active { "🛜 " } else { "   " };
            // let active_marker = if net.active { "● " } else { "  " };
            // let active_marker = if net.active { "🌐 " } else { "   " };
            let active_marker = if net.active { "🔗 " } else { "   " };

            // Signal strength indicator (always shown)
            let signal_indicator = match net.strength {
                0..=25 => "▁    ",
                26..=50 => "▁▃   ",
                51..=75 => "▁▃▅  ",
                _ => "▁▃▅▇ ",
            };

            // Signal style: yellow when focused, gray otherwise
            let signal_style = if is_dimmed {
                Style::default().fg(Color::DarkGray)
            } else if i == selected_index {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let detail_style = if is_dimmed {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            if d_pressed {
                // Multi-line format: network name on first line, details on subsequent lines
                let mut lines = vec![
                    // First line: prefix, active marker, signal, and SSID
                    Line::from(vec![
                        Span::styled(format!("{}{}", prefix, active_marker), main_style),
                        Span::styled(signal_indicator, signal_style),
                        Span::styled(net.ssid.clone(), main_style),
                    ])
                ];

                // Build details for second line
                let mut detail_parts = vec![];

                // Signal strength percentage
                detail_parts.push(format!("signal: {}%", net.strength));

                // Frequency and band information
                if let Some(freq) = net.frequency {
                    let band = if freq >= 2412 && freq <= 2484 {
                        "2.4 GHz"
                    } else if freq >= 5170 && freq <= 5835 {
                        "5 GHz"
                    } else if freq >= 5945 && freq <= 7125 {
                        "6 GHz"
                    } else {
                        "unknown band"
                    };
                    detail_parts.push(format!("frequency: {} MHz ({})", freq, band));
                }

                // Security with warning if weak
                let warning = if net.weak_security { " (⚠ insecure)" } else { "" };
                detail_parts.push(format!("security: {}{}", net.security, warning));

                // Known status
                if net.known {
                    detail_parts.push("known network (F to forget)".to_string());
                }

                // Second line: basic details (always gray, no highlight)
                let detail_indent = Span::styled("          ", detail_style);
                lines.push(Line::from(vec![
                    detail_indent.clone(),
                    Span::styled(detail_parts.join(" | "), detail_style),
                ]).style(detail_style)); // Apply style to entire line to prevent highlighting

                // Third line: advanced details (only for known networks)
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
                        lines.push(Line::from(vec![
                            detail_indent,
                            Span::styled(advanced_parts.join(" | "), detail_style),
                        ]).style(detail_style)); // Apply style to entire line to prevent highlighting
                    }
                }

                ListItem::new(lines)
            } else {
                // Single line format: just show the network name
                let content = Line::from(vec![
                    Span::styled(format!("{}{}", prefix, active_marker), main_style),
                    Span::styled(signal_indicator, signal_style),
                    Span::styled(net.ssid.clone(), main_style),
                ]);
                ListItem::new(content)
            }
        })
        .collect();

    let block_style = if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title("Networks")
                .style(block_style),
        );

    f.render_stateful_widget(list, area, list_state);
}

fn draw_footer(f: &mut Frame, area: Rect, is_dimmed: bool) {
    use ratatui::text::Span;

    let style = if is_dimmed {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let shortcuts = Span::styled(
        "↑/↓: Navigate | Enter to dis/connect | D: Details | Q: Quit",
        style
    );

    let footer = Paragraph::new(shortcuts);
    f.render_widget(footer, area);
}
