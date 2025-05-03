use crate::app::{App, CustomModelStage, Mode, SettingsTab};
use crate::config::CustomModel;
use ratatui::widgets::ListState;
use ratatui::widgets::Padding;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Tabs,
    },
};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use textwrap::wrap;

pub fn draw(f: &mut Frame<'_>, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if app.sidebar_visible {
            [Constraint::Length(20), Constraint::Min(0)].as_ref()
        } else {
            [Constraint::Length(0), Constraint::Min(0)].as_ref()
        })
        .split(f.area());

    if app.sidebar_visible {
        draw_sidebar(f, app, chunks[0]);
    }

    match app.mode {
        Mode::Settings | Mode::ApiKeyInput | Mode::CustomModelInput => {
            draw_settings(f, app, chunks[1])
        }
        Mode::ModelSelect => draw_model_select(f, app, chunks[1]),
        _ => draw_chat(f, app, chunks[1]),
    }
}

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(|| SyntaxSet::load_defaults_newlines())
}
fn get_theme() -> &'static Theme {
    THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes["base16-ocean.dark"].clone()
    })
}

fn draw_sidebar(f: &mut Frame<'_>, app: &App, area: Rect) {
    let is_focused = app.focus == crate::app::Focus::Sidebar;
    let sidebar_block = Block::default()
        .title("🐱 Meowi")
        .borders(Borders::ALL)
        .style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(if is_focused {
            Color::Blue
        } else {
            Color::DarkGray
        }));

    let inner_area = sidebar_block.inner(area);
    f.render_widget(sidebar_block, area);

    let sidebar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner_area);

    let chat_items: Vec<ListItem> = app
        .chats
        .iter()
        .map(|chat| {
            let title = if chat.streaming {
                format!("{} ⏳", chat.title)
            } else {
                chat.title.clone()
            };
            ListItem::new(title)
        })
        .collect();

    let mut chat_state = ListState::default();
    if app.selected_sidebar_idx < app.chats.len() {
        chat_state.select(Some(app.selected_sidebar_idx));
    }

    let chat_list = List::new(chat_items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        )
        .style(Style::default().fg(Color::White));

    f.render_stateful_widget(chat_list, sidebar_chunks[0], &mut chat_state);

    let settings_item = ListItem::new("Settings");
    let mut settings_state = ListState::default();
    if app.selected_sidebar_idx == app.chats.len() {
        settings_state.select(Some(0));
    }

    let settings_list = List::new(vec![settings_item]).highlight_style(
        Style::default()
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(settings_list, sidebar_chunks[1], &mut settings_state);
}

#[derive(Debug)]
enum MessageSegment {
    Text(String),
    Code {
        language: Option<String>,
        content: String,
    },
}

fn parse_message_segments(content: &str) -> Vec<MessageSegment> {
    let mut segments = Vec::new();
    let mut lines = content.lines().peekable();
    let mut current_text = Vec::new();

    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix("```") {
            if !current_text.is_empty() {
                segments.push(MessageSegment::Text(current_text.join("\n")));
                current_text.clear();
            }
            let lang = if !rest.trim().is_empty() {
                Some(rest.trim().to_string())
            } else {
                None
            };
            let mut code_lines = Vec::new();
            while let Some(code_line) = lines.next() {
                if code_line.trim() == "```" {
                    break;
                }
                code_lines.push(code_line);
            }
            let code_content =
                if let (Some(ref l), Some(first)) = (lang.as_ref(), code_lines.first()) {
                    if first.trim().eq_ignore_ascii_case(l.trim()) {
                        code_lines[1..].join("\n")
                    } else {
                        code_lines.join("\n")
                    }
                } else {
                    code_lines.join("\n")
                };
            segments.push(MessageSegment::Code {
                language: lang,
                content: code_content,
            });
        } else {
            current_text.push(line);
        }
    }
    if !current_text.is_empty() {
        segments.push(MessageSegment::Text(current_text.join("\n")));
    }
    segments
}

fn draw_chat(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    const MAX_VISIBLE_LINES_PER_MESSAGE: usize = 10;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    if app.cursor_line == usize::MAX {
        app.jump_to_last_message();
    }

    let mut buffer_lines = Vec::new();
    let mut line_to_message = Vec::new();

    let cursor_style = Style::default().bg(Color::Blue);
    let user_style = Style::default().fg(Color::Yellow);
    let assistant_style = Style::default().fg(Color::Green);
    let border_style = Style::default().fg(Color::LightGreen);

    if app.has_valid_chat() {
        let text_width = (chunks[0].width as usize).saturating_sub(4);
        let is_streaming = app
            .chats
            .get(app.current_chat)
            .map(|c| c.streaming)
            .unwrap_or(false);

        if app.need_rebuild_cache || text_width != app.last_width {
            app.last_width = text_width;
            app.line_cache.clear();
            app.code_blocks.clear();

            let messages: Vec<(usize, String, String)> = app
                .chats
                .get(app.current_chat)
                .map(|chat| {
                    chat.messages
                        .iter()
                        .enumerate()
                        .map(|(idx, msg)| (idx, msg.role.clone(), msg.content.clone()))
                        .collect()
                })
                .unwrap_or_default();

            for (msg_idx, role, content) in messages {
                let mut msg_lines = Vec::new();
                let mut is_truncated = false;

                let segments = parse_message_segments(&content);
                let mut code_block_count = 0;

                for segment in segments {
                    match segment {
                        MessageSegment::Text(text) => {
                            let wrapped_lines = wrap(&text, text_width);
                            let is_trunc = app.truncated_messages.contains(&msg_idx)
                                && wrapped_lines.len() > MAX_VISIBLE_LINES_PER_MESSAGE;
                            let lines: Vec<Line> = wrapped_lines
                                .iter()
                                .take(if is_trunc {
                                    MAX_VISIBLE_LINES_PER_MESSAGE
                                } else {
                                    wrapped_lines.len()
                                })
                                .map(|line| {
                                    Line::from(line.to_string()).style(if role == "user" {
                                        user_style
                                    } else {
                                        assistant_style
                                    })
                                })
                                .collect();
                            msg_lines.extend(lines);
                            if is_trunc {
                                is_truncated = true;
                            }
                        }
                        MessageSegment::Code { language, content } => {
                            app.code_blocks.push((
                                msg_idx,
                                crate::app::CodeBlock {
                                    content: content.clone(),
                                },
                            ));

                            msg_lines.push(Line::raw(""));
                            let lang = language.as_deref().unwrap_or("code");

                            let area_width = chunks[0].width.saturating_sub(2) as usize;
                            let label = format!(" {} ", lang);
                            let border_len = area_width.saturating_sub(20 + label.len());
                            let top = format!("┌─{}{}┐", label, "─".repeat(border_len));
                            msg_lines.push(Line::from(vec![Span::styled(top, border_style)]));

                            let syntax_set = get_syntax_set();
                            let theme = get_theme();
                            let syntax = syntax_set
                                .find_syntax_by_token(lang)
                                .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
                            let mut h = HighlightLines::new(syntax, theme);
                            for code in content.lines() {
                                let ranges = h.highlight_line(code, syntax_set).unwrap();
                                let mut spans = vec![Span::styled("│ ", border_style)];
                                for (style, text) in ranges {
                                    spans.push(Span::styled(
                                        text.to_string(),
                                        Style::default()
                                            .fg(Color::Rgb(
                                                style.foreground.r,
                                                style.foreground.g,
                                                style.foreground.b,
                                            ))
                                            .bg(Color::Black),
                                    ));
                                }
                                msg_lines.push(Line::from(spans));
                            }

                            let shortcuts = vec!["c", "C", "x", "X"];
                            let hint = shortcuts
                                .get(code_block_count)
                                .map(|s| format!(" Copy [{}] ", s))
                                .unwrap_or_default();
                            let border_len = area_width.saturating_sub(20 + hint.len());
                            let bottom = format!("└{}{}┘", "─".repeat(border_len), hint);
                            msg_lines.push(Line::from(vec![Span::styled(bottom, border_style)]));

                            code_block_count += 1;
                        }
                    }
                }

                app.line_cache.push((msg_lines, is_truncated));
            }
            app.need_rebuild_cache = false;
        }

        let mut global_line_idx = 0;
        for (msg_idx, (lines, is_truncated)) in app.line_cache.iter().enumerate() {
            for line in lines.iter() {
                let mut styled_line = line.clone();
                if global_line_idx == app.cursor_line {
                    styled_line = styled_line.patch_style(cursor_style);
                }
                buffer_lines.push(styled_line);
                line_to_message.push((msg_idx, false));
                global_line_idx += 1;
            }
            if *is_truncated {
                let mut ellipsis_line =
                    Line::from("...".to_string()).style(Style::default().fg(Color::Gray));
                if global_line_idx == app.cursor_line {
                    ellipsis_line = ellipsis_line.patch_style(cursor_style);
                }
                buffer_lines.push(ellipsis_line);
                line_to_message.push((msg_idx, true));
                global_line_idx += 1;
            }
            let mut sep = Line::raw("");
            if global_line_idx == app.cursor_line {
                sep = sep.patch_style(cursor_style);
            }
            buffer_lines.push(sep);
            line_to_message.push((msg_idx, false));
            global_line_idx += 1;
        }

        app.line_to_message = line_to_message.clone();

        // --- LOADING CAT ANIMATION ---
        if is_streaming {
            if let Some(chat) = app.chats.get(app.current_chat) {
                let last_msg = chat.messages.last();
                let show_loading = match last_msg {
                    Some(msg) if msg.role == "assistant" && msg.content.trim().is_empty() => true,
                    None => true,
                    _ => false,
                };
                if show_loading {
                    // Cat loading animation frames
                    let frames = ["🐱   ", "🐱.  ", "🐱.. ", "🐱...", "🐱 ..", "🐱  ."];
                    let frame = frames[(app.loading_frame / 6) % frames.len()];
                    buffer_lines.push(Line::from(vec![
                        Span::styled(
                            frame,
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  Waiting for response..."),
                    ]));
                }
            }
        }
        // --- END LOADING CAT ANIMATION ---

        let total_lines = buffer_lines.len();
        let viewport_height = chunks[0].height.saturating_sub(20) as usize;

        if total_lines > 0 && app.cursor_line >= total_lines {
            app.cursor_line = total_lines - 1;
        }

        let max_scroll = total_lines.saturating_sub(viewport_height);
        app.max_chat_scroll = max_scroll as u16;
        if app.chat_scroll == u16::MAX {
            app.chat_scroll = max_scroll as u16;
        } else if app.chat_scroll as usize > max_scroll {
            app.chat_scroll = max_scroll as u16;
        }

        if app.cursor_line < app.chat_scroll as usize {
            app.chat_scroll = app.cursor_line as u16;
        } else if app.cursor_line >= app.chat_scroll as usize + viewport_height {
            app.chat_scroll = (app.cursor_line + 1).saturating_sub(viewport_height) as u16;
        }

        let is_focused = app.focus == crate::app::Focus::Chat;
        let title = if is_streaming {
            format!("{} ⏳", app.current_model_name())
        } else {
            app.current_model_name().to_string()
        };
        let paragraph = Paragraph::new(buffer_lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .padding(Padding {
                        left: 1,
                        right: 1,
                        top: 0,
                        bottom: 0,
                    })
                    .style(Style::default().fg(Color::LightBlue))
                    .border_style(Style::default().fg(if is_focused {
                        Color::Blue
                    } else {
                        Color::DarkGray
                    })),
            )
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((app.chat_scroll, 0));
        f.render_widget(paragraph, chunks[0]);
        let scrollbar_content_length = total_lines.max(viewport_height);
        let scrollbar_position = (app.chat_scroll as usize)
            .min(max_scroll)
            .min(scrollbar_content_length.saturating_sub(viewport_height));
        let mut scrollbar_state =
            ScrollbarState::new(scrollbar_content_length).position(scrollbar_position);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            chunks[0],
            &mut scrollbar_state,
        );
    } else {
        let paragraph = Paragraph::new("Start a new chat with 'n'").block(
            Block::default()
                .title(app.current_model_name())
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(paragraph, chunks[0]);
    }

    let input_text = match app.mode {
        Mode::Insert | Mode::RenameChat => format!("> {}", app.input),
        Mode::Command => format!(":{}", app.command),
        _ => app
            .error_message
            .as_ref()
            .map_or(String::new(), |e| format!("Error: {}", e)),
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(match app.mode {
            Mode::Insert => "Insert",
            Mode::RenameChat => "Rename Chat",
            Mode::Command => "Command",
            _ => "Input",
        })
        .style(Style::default().fg(
            if app.error_message.is_some() && matches!(app.mode, Mode::Normal) {
                Color::Red
            } else {
                Color::White
            },
        ));

    let input_paragraph = Paragraph::new(input_text).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);
}

fn draw_model_select(f: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .title("Select Model")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Green));

    let models = app.enabled_models_flat();

    let items: Vec<ListItem> = models
        .iter()
        .map(|(provider, model)| ListItem::new(format!("{}:{}", provider, model)))
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected_model_idx));

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, area, &mut state);
}

fn mask_api_key(k: &str) -> String {
    if k.len() <= 4 {
        "".repeat(k.len())
    } else {
        let (head, tail) = k.split_at(k.len() - 4);
        format!("{}{}", "".repeat(head.len()), tail)
    }
}

pub fn draw_settings(f: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let titles = ["Providers", "Shortcuts"]
        .iter()
        .cloned()
        .map(String::from)
        .collect::<Vec<_>>();

    let tabs = Tabs::new(titles)
        .select(match app.settings_tab {
            SettingsTab::Providers => 0,
            SettingsTab::Shortcuts => 1,
        })
        .block(Block::default().borders(Borders::ALL).title("Settings"))
        .highlight_style(
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().fg(Color::White));

    f.render_widget(tabs, chunks[0]);

    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(chunks[1]);

    if app.mode == Mode::ApiKeyInput {
        let masked = mask_api_key(&app.api_key_old);
        let text = format!("Current: {}\nNew API Key: {}", masked, app.api_key_input);
        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Enter API Key"),
        );
        f.render_widget(paragraph, content_chunks[0]);
    } else if app.mode == Mode::CustomModelInput {
        match app.custom_model_input_stage.unwrap() {
            CustomModelStage::TypeChoice => {
                let items = vec![
                    ListItem::new("Derived from existing provider"),
                    ListItem::new("Standalone custom model"),
                ];
                let selected = app
                    .custom_model_api_key_choice
                    .as_ref()
                    .map(|choice| if choice == "Derived" { 0 } else { 1 })
                    .unwrap_or(0);
                let mut state = ListState::default();
                state.select(Some(selected));
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Model Type"))
                    .highlight_style(
                        Style::default()
                            .bg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_stateful_widget(list, content_chunks[0], &mut state);
            }
            CustomModelStage::ProviderChoice => {
                let items = app
                    .providers
                    .iter()
                    .map(|p| ListItem::new(p.name.clone()))
                    .collect::<Vec<_>>();
                let selected = app
                    .custom_model_api_key_choice
                    .as_ref()
                    .and_then(|choice| app.providers.iter().position(|p| p.name == *choice))
                    .unwrap_or(0);
                let mut state = ListState::default();
                state.select(Some(selected));
                let list = List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Select Provider"),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_stateful_widget(list, content_chunks[0], &mut state);
            }
            CustomModelStage::DerivedModelName => {
                let p = Paragraph::new(format!("Model Name: {}", app.custom_model_model_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Derived Model—Name"),
                    );
                f.render_widget(p, content_chunks[0]);
            }
            CustomModelStage::StandaloneName => {
                let p = Paragraph::new(format!("Model Name: {}", app.custom_model_name_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Standalone Model—Name"),
                    );
                f.render_widget(p, content_chunks[0]);
            }
            CustomModelStage::StandaloneUrl => {
                let p = Paragraph::new(format!("Endpoint URL: {}", app.custom_model_url_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Standalone Model—URL"),
                    );
                f.render_widget(p, content_chunks[0]);
            }
            CustomModelStage::StandaloneModelId => {
                let p = Paragraph::new(format!("Model ID: {}", app.custom_model_model_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Standalone Model—Model ID"),
                    );
                f.render_widget(p, content_chunks[0]);
            }
            CustomModelStage::StandaloneApiKeyChoice => {
                let mut items = app
                    .providers
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>();
                items.push("Custom".to_string());
                let selected = app
                    .custom_model_api_key_choice
                    .as_ref()
                    .and_then(|choice| items.iter().position(|n| n == choice))
                    .unwrap_or(0);
                let list_items = items
                    .iter()
                    .map(|n| ListItem::new(n.clone()))
                    .collect::<Vec<_>>();
                let mut state = ListState::default();
                state.select(Some(selected));
                let list = List::new(list_items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("API Key Source"),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_stateful_widget(list, content_chunks[0], &mut state);
            }
            CustomModelStage::StandaloneApiKeyInput => {
                let p = Paragraph::new(format!("API Key: {}", app.custom_model_api_key_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Enter API Key"),
                    );
                f.render_widget(p, content_chunks[0]);
            }
        }
    } else if app.settings_tab == SettingsTab::Providers {
        let mut items = Vec::new();
        for p in &app.providers {
            let prefix = if p.expanded { "[-]" } else { "[+]" };
            items.push(ListItem::new(format!("{} {}", prefix, p.name)));

            if p.expanded {
                // Show the union of p.models and p.enabled_models, sorted and deduped
                let mut all_models: Vec<String> = p.models.iter().cloned().collect();
                for m in &p.enabled_models {
                    if !all_models.contains(m) {
                        all_models.push(m.clone());
                    }
                }
                all_models.sort();
                for m in &all_models {
                    let checked = if p.enabled_models.contains(m) {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    items.push(ListItem::new(format!("    {} {}", checked, m)));
                }
            }
        }
        items.push(ListItem::new("Custom Models:"));
        for cm in &app.custom_models {
            let display = match cm {
                CustomModel::Derived { provider, model } => {
                    format!("  {}:{} (Derived)", provider, model)
                }
                CustomModel::Standalone { name, endpoint, .. } => {
                    format!("  {} → {}", name, endpoint)
                }
            };
            items.push(ListItem::new(display));
        }
        items.push(ListItem::new("  [Add Custom Model]"));

        let mut state = ListState::default();
        state.select(Some(app.selected_line));
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Providers"))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, content_chunks[0], &mut state);
    } else {
        let paragraph = Paragraph::new("Shortcut customization coming soon!")
            .block(Block::default().borders(Borders::ALL).title("Shortcuts"));
        f.render_widget(paragraph, content_chunks[0]);
    }

    if let Some(err) = &app.error_message {
        let p = Paragraph::new(err.clone()).style(Style::default().fg(Color::Red));
        f.render_widget(p, content_chunks[1]);
    } else if let Some(info) = &app.info_message {
        let p = Paragraph::new(info.clone()).style(Style::default().fg(Color::Green));
        f.render_widget(p, content_chunks[1]);
    }
}
