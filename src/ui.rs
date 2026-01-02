use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
};
use throbber_widgets_tui::{CANADIAN, Throbber, WhichUse};

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // List
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_network_list(f, app, chunks[1]);

    match app.input_mode {
        InputMode::Editing => {
            // Show error message if present in a separate block above
            if let Some(error) = &app.password_error {
                let error_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(Color::Red));
                let error_area = centered_rect_fixed(50, 3, f.area());
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
            }

            // Password input block below (or centered if no error)
            let password_block = Block::default()
                .title("Password")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            let password_area = if app.password_error.is_some() {
                // Position below the error block
                let base_area = centered_rect_fixed(50, 3, f.area());
                Rect {
                    x: base_area.x,
                    y: base_area.y + 3, // 3 for error block height, no spacing
                    width: base_area.width,
                    height: base_area.height,
                }
            } else {
                centered_rect_fixed(50, 3, f.area())
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

            let scroll = app.password_input.visual_scroll(inner_area.width as usize);
            let input_widget = Paragraph::new(app.password_input.value())
                .style(Style::default().fg(Color::Yellow))
                .scroll((0, scroll as u16));
            f.render_widget(input_widget, inner_area);

            // Set cursor position
            f.set_cursor_position((
                inner_area.x + ((app.password_input.visual_cursor()).max(scroll) - scroll) as u16,
                inner_area.y,
            ));
        }
        InputMode::Connecting => {
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
            f.render_stateful_widget(throbber, throbber_area, &mut app.throbber_state);
        }
        InputMode::Normal => {}
        InputMode::Error => {
            let error_msg = app.error_message.as_deref().unwrap_or("Unknown error");
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

            let message =
                Paragraph::new(format!("{}\n\nPress Enter or Esc to dismiss.", error_msg))
                    .style(Style::default().fg(Color::White));
            f.render_widget(message, inner_area);
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

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let active = app.active_ssid.as_deref().unwrap_or("None");
    let text = Paragraph::new(format!("WeeFee - WiFi Manager | Connected: {}", active))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded));
    f.render_widget(text, area);
}

fn draw_network_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .networks
        .iter()
        .enumerate()
        .map(|(i, net)| {
            let style = if i == app.selected_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if net.weak_security {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };

            let prefix = if i == app.selected_index { "> " } else { "  " };
            let active_marker = if net.active { "*" } else { " " };
            let warning = if net.weak_security { " (!)" } else { "" };
            let content = format!(
                "{}{}{} ({}%) [{}{}]",
                prefix, active_marker, net.ssid, net.strength, net.security, warning
            );
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title("Networks"));

    // We handle selection manually via style in the list item for simplicity
    // or we could use ListState.
    f.render_widget(list, area);
}
