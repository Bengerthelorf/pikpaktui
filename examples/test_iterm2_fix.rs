use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{picker::{Picker, ProtocolType}, StatefulImage};
use std::io;

fn main() -> Result<()> {
    // Load test image
    let url = "https://picsum.photos/400/300";
    eprintln!("Downloading test image from {}...", url);

    let response = reqwest::blocking::get(url)?;
    let bytes = response.bytes()?;
    let img = image::load_from_memory(&bytes)?;

    eprintln!("Image loaded: {}x{}", img.width(), img.height());

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // Detect protocol
    let mut picker = Picker::from_query_stdio()
        .map_err(|e| anyhow::anyhow!("Picker error: {:?}", e))?;

    let detected_protocol = picker.protocol_type();
    eprintln!("Detected protocol: {:?}", detected_protocol);

    // Force iTerm2 if detected as Kitty
    if matches!(detected_protocol, ProtocolType::Kitty) {
        eprintln!("Detected as Kitty, forcing iTerm2 protocol...");
        picker.set_protocol_type(ProtocolType::Iterm2);
    }

    let result = run_app(&mut terminal, img, picker);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

fn run_app(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    img: image::DynamicImage,
    picker: Picker,
) -> Result<()> {
    let protocol_type = picker.protocol_type();
    let mut protocol = picker.new_resize_protocol(img.clone());

    loop {
        terminal.draw(|f| {
            let area = f.area();

            // Split into main area and info area
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(7)])
                .split(area);

            let main_area = chunks[0];
            let info_area = chunks[1];

            // Render image
            let img_widget = StatefulImage::default();
            f.render_stateful_widget(img_widget, main_area, &mut protocol);

            // Render info
            let info_lines = vec![
                Line::from(format!("Terminal: {}", std::env::var("TERM").unwrap_or_default())),
                Line::from(format!("TERM_PROGRAM: {}", std::env::var("TERM_PROGRAM").unwrap_or_default())),
                Line::from(format!("COLORTERM: {}", std::env::var("COLORTERM").unwrap_or_default())),
                Line::from(format!("Protocol: {:?}", protocol_type)),
                Line::from(format!("Forced: iTerm2 (was Kitty)")),
                Line::from(format!("Area: {}x{}", main_area.width, main_area.height)),
                Line::from("Press 'q' to quit"),
            ];
            let info = Paragraph::new(info_lines)
                .block(Block::default().borders(Borders::ALL).title("Info"));
            f.render_widget(info, info_area);
        })?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    Ok(())
}
