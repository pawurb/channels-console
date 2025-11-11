use ratatui::{
    layout::Rect,
    style::Stylize,
    symbols::border,
    text::Line,
    widgets::{Block, Paragraph},
    Frame,
};
use std::time::Duration;

use crate::cmd::console::app::Focus;

/// Renders the bottom controls bar showing context-aware keybindings
pub fn render_bottom_bar(
    frame: &mut Frame,
    area: Rect,
    focus: Focus,
    _last_render_duration: Duration,
) {
    let controls_line = match focus {
        Focus::Channels => Line::from(vec![
            " Quit ".into(),
            "<q> ".blue().bold(),
            " | Navigate ".into(),
            "<←↑↓→/hjkl> ".blue().bold(),
            " | Toggle Logs ".into(),
            "<o> ".blue().bold(),
            " | Pause ".into(),
            "<p> ".blue().bold(),
        ]),
        Focus::Logs => Line::from(vec![
            " Quit ".into(),
            "<q> ".blue().bold(),
            " | Navigate ".into(),
            "<←↑↓→/hjkl> ".blue().bold(),
            " | Toggle Logs ".into(),
            "<o> ".blue().bold(),
            " | Pause ".into(),
            "<p> ".blue().bold(),
            " | Inspect ".into(),
            "<i> ".blue().bold(),
        ]),
        Focus::Inspect => Line::from(vec![
            " Quit ".into(),
            "<q> ".blue().bold(),
            " | Navigate ".into(),
            "<←↑↓→/hjkl> ".blue().bold(),
            " | Toggle Logs ".into(),
            "<o> ".blue().bold(),
            " | Pause ".into(),
            "<p> ".blue().bold(),
            " | Close ".into(),
            "<i/o/h> ".blue().bold(),
        ]),
    };

    #[cfg(feature = "dev")]
    let block = {
        use ratatui::text::Line;

        let render_time_ms = _last_render_duration.as_millis();
        let render_time_text = format!(" {}ms ", render_time_ms);

        Block::bordered()
            .title(" Controls ")
            .title_bottom(Line::from(render_time_text).right_aligned())
            .border_set(border::PLAIN)
    };

    #[cfg(not(feature = "dev"))]
    let block = Block::bordered()
        .title(" Controls ")
        .border_set(border::PLAIN);

    let paragraph = Paragraph::new(controls_line).block(block).left_aligned();

    frame.render_widget(paragraph, area);
}
