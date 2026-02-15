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

    // Test all protocols
    let test_protocols = vec![
        ("Auto (with iTerm2 fix)", ProtocolType::Iterm2), // Simulating the fix
        ("Forced: Kitty", ProtocolType::Kitty),
        ("Forced: iTerm2", ProtocolType::Iterm2),
        ("Forced: Sixel", ProtocolType::Sixel),
        ("Forced: Halfblocks", ProtocolType::Halfblocks),
    ];

    let mut current_test = 0;

    let result = run_app(&mut terminal, img, test_protocols, &mut current_test);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

fn run_app(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    img: image::DynamicImage,
    test_protocols: Vec<(&str, ProtocolType)>,
    current_test: &mut usize,
) -> Result<()> {
    let mut picker = Picker::from_query_stdio()
        .map_err(|e| anyhow::anyhow!("Picker error: {:?}", e))?;

    let detected_protocol = picker.protocol_type();

    loop {
        let (test_name, protocol_type) = test_protocols[*current_test];
        picker.set_protocol_type(protocol_type);
        let mut protocol = picker.new_resize_protocol(img.clone());

        terminal.draw(|f| {
            let area = f.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(10)])
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
                Line::from(""),
                Line::from(format!("Detected protocol: {:?}", detected_protocol)),
                Line::from(format!("Current test: {}", test_name)),
                Line::from(format!("Protocol: {:?}", protocol_type)),
                Line::from(""),
                Line::from("Press 'n' for next test, 'q' to quit"),
            ];
            let info = Paragraph::new(info_lines)
                .block(Block::default().borders(Borders::ALL).title("Protocol Test"));
            f.render_widget(info, info_area);
        })?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('n') => {
                        *current_test = (*current_test + 1) % test_protocols.len();
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
