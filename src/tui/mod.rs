pub mod theme;

mod logo;
mod render;

use ratatui::{Terminal, backend::TestBackend};

pub use theme::Theme;

use crate::app::App;

/// Draw the interactive application view.
pub fn draw(frame: &mut ratatui::Frame<'_>, app: &App, theme: Theme) {
    render::draw(frame, app, theme)
}

/// Draw the application view with the current composer text.
pub fn draw_with_composer(frame: &mut ratatui::Frame<'_>, app: &App, theme: Theme, composer: &str) {
    render::draw_with_composer(frame, app, theme, composer)
}

/// Render the current application state into deterministic plain terminal text.
pub fn render_to_string(app: &App, width: u16, height: u16, theme: Theme) -> String {
    render_to_string_with_composer(app, width, height, theme, "")
}

/// Render the application with composer text shown in the bottom status area.
pub fn render_to_string_with_composer(
    app: &App,
    width: u16,
    height: u16,
    theme: Theme,
    composer: &str,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test backend should initialize");
    terminal
        .draw(|frame| render::draw_with_composer(frame, app, theme, composer))
        .expect("test backend should render");

    let buffer = terminal.backend().buffer();
    let mut output = String::with_capacity(width as usize * height as usize);
    for y in 0..height {
        if y > 0 {
            output.push('\n');
        }
        for x in 0..width {
            output.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
    }
    output
}
