fn render_help_tab(frame: &mut Frame, app: &AppState, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Help");
    let mut text = vec![
        Line::from("Keybindings:"),
        Line::from("  q     : Quit"),
        Line::from("  ?     : Help"),
        Line::from("  Tab   : Cycle Focus"),
        Line::from("  1/2/3 : Switch Tabs"),
        Line::from("  j/k   : Navigate Events"),
        Line::from("  Space : Toggle Follow Mode"),
    ];
    if app.replay_mode {
        text.push(Line::from("  r     : Reload from disk"));
    }
    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}
