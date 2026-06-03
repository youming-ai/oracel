use ratatui::prelude::*;
use ratatui::widgets::*;

use super::state::TuiState;
use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;

pub fn render(frame: &mut Frame, state: Option<&TuiState>) {
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(frame.area());

    let (btc_str, bal_str, pnl_str, record, streak) = match state {
        Some(s) => (
            format!("${:.0}", s.btc_price.to_f64().unwrap_or(0.0)),
            format!("${:.2}", s.balance),
            format!("{:+.2}", s.pnl),
            format!("{}W/{}L", s.total_wins, s.total_losses),
            if s.consecutive_wins > 0 {
                format!("+{}", s.consecutive_wins)
            } else if s.consecutive_losses > 0 {
                format!("-{}", s.consecutive_losses)
            } else {
                "0".to_string()
            },
        ),
        None => ("—".into(), "—".into(), "—".into(), "—".into(), "—".into()),
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " POLYBOT v0.3.0 ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" | BTC {} | ", btc_str)),
        Span::styled(
            "LIVE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(header, chunks[0]);

    let (slug, ttl, pending) = match state {
        Some(s) => {
            let ttl = if s.settlement_ms > 0 {
                let remaining = (s.settlement_ms - Utc::now().timestamp_millis()).max(0) / 1000;
                if remaining > 0 {
                    format!("{}m{}s", remaining / 60, remaining % 60)
                } else {
                    "expired".to_string()
                }
            } else {
                "?".to_string()
            };
            (s.market_slug.clone(), ttl, s.pending_count)
        }
        None => ("—".into(), "—".into(), 0),
    };

    let market_info = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" Market: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}  ", slug)),
            Span::styled("TTL: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&ttl),
        ]),
        Line::from(vec![
            Span::styled(" Balance: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}  ", bal_str)),
            Span::styled("PnL: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}  ", pnl_str)),
            Span::raw(format!("{}  streak:{}  ", record, streak)),
            Span::styled("Pending: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}", pending)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Status "));
    frame.render_widget(market_info, chunks[1]);

    let rows: Vec<Row> = match state {
        Some(s) => s
            .recent_trades
            .iter()
            .rev()
            .skip(s.scroll_offset)
            .take(50)
            .map(|t| {
                let pnl_str = t
                    .pnl
                    .map(|p| format!("{:+.2}", p))
                    .unwrap_or_else(|| "—".into());
                let style = if t.result == "WIN" {
                    Style::default().fg(Color::Green)
                } else if t.result == "LOSS" {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(t.time.format("%H:%M:%S").to_string()),
                    Cell::from(t.direction.clone()),
                    Cell::from(format!("{:.3}", t.entry_price)),
                    Cell::from(format!("{:.2}", t.cost)),
                    Cell::from(format!("{}%", t.edge)),
                    Cell::from(pnl_str),
                ])
                .style(style)
            })
            .collect(),
        None => vec![],
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["Time", "Dir", "Price", "Cost", "Edge", "PnL"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .bottom_margin(1),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Recent Trades "),
    );
    frame.render_widget(table, chunks[2]);

    let decision = match state {
        Some(s) => s.last_decision.as_str(),
        None => "—",
    };
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" Decision: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(decision),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(status, chunks[3]);
}
