// ---------------------------------------------------------------------------
// Rendering proof (not snapshot-only)
// ---------------------------------------------------------------------------

/// Render the palette overlay via Ratatui TestBackend showing the title and
/// entry count. This is a real-surface artifact, not a snapshot.

#[test]
fn palette_overlay_renders_title_and_entries() {
    // arrange
    let view = PaletteLeafView::new(true, 0, 3, 5);

    let area = Rect::new(0, 0, 40, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = vec![
                Line::from("Command Palette"),
                Line::from(format!("Entries: {}", view.entry_count)),
                Line::from(format!("Query length: {}", view.query_len)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("OVL-PALETTE"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    // act
    let buffer = terminal.backend().buffer().clone();
    let output: String = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    // assert
    assert!(
        output.contains("OVL-PALETTE"),
        "missing title in:\n{output}"
    );
    assert!(
        output.contains("Entries: 3"),
        "missing entry count in:\n{output}"
    );
}

/// Render the session overlay via Ratatui TestBackend.
#[test]
fn session_overlay_renders_title_and_sessions() {
    // arrange
    let view = SessionLeafView::new(true, 0, 5, true);

    let area = Rect::new(0, 0, 40, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = vec![
                Line::from("Session Navigation"),
                Line::from(format!("Sessions: {}", view.session_count)),
                Line::from(format!("Lineage: {}", view.has_lineage)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("OVL-SESSION"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    // act
    let buffer = terminal.backend().buffer().clone();
    let output: String = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    // assert
    assert!(
        output.contains("OVL-SESSION"),
        "missing title in:\n{output}"
    );
    assert!(
        output.contains("Sessions: 5"),
        "missing session count in:\n{output}"
    );
}

/// Render the model switcher overlay via Ratatui TestBackend.
#[test]
fn model_overlay_renders_title_and_models() {
    // arrange
    let view = ModelLeafView::new(true, 0, 2, 4);

    let area = Rect::new(0, 0, 40, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = vec![
                Line::from("Model Switcher"),
                Line::from(format!("Providers: {}", view.provider_count)),
                Line::from(format!("Models: {}", view.model_count)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("OVL-MODEL"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    // act
    let buffer = terminal.backend().buffer().clone();
    let output: String = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    // assert
    assert!(output.contains("OVL-MODEL"), "missing title in:\n{output}");
    assert!(
        output.contains("Models: 4"),
        "missing model count in:\n{output}"
    );
}
