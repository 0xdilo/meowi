use crate::app::{App, CustomModelStage, Mode, SettingsTab};
use crate::config;
use crate::config::CustomModel;
use ratatui::prelude::Alignment;
use ratatui::prelude::Margin;
use ratatui::widgets::ListState;
use ratatui::widgets::Padding;
use ratatui::widgets::Wrap;
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
        Mode::Settings | Mode::ApiKeyInput | Mode::CustomModelInput | Mode::PromptInput => {
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

    let mut buffer_lines: Vec<Line> = Vec::new();
    let mut line_to_message_map: Vec<(usize, bool)> = Vec::new();

    let visual_selection_style = Style::default().bg(Color::Indexed(57));
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

            let current_chat_messages = app
                .chats
                .get(app.current_chat)
                .map_or_else(Vec::new, |chat| chat.messages.clone());

            for (original_msg_idx, message) in current_chat_messages.iter().enumerate() {
                if message.role == "system" {
                    continue;
                }

                let role = &message.role;
                let content = &message.content;
                let mut msg_lines_for_cache = Vec::new();
                let mut is_truncated_for_cache = false;

                let segments = parse_message_segments(content);
                let mut code_block_count_for_message = 0;

                for segment in segments {
                    match segment {
                        MessageSegment::Text(text_content) => {
                            let wrapped_lines = wrap(&text_content, text_width.max(1));
                            let is_trunc = app.truncated_messages.contains(&original_msg_idx)
                                && wrapped_lines.len() > MAX_VISIBLE_LINES_PER_MESSAGE;
                            let lines_to_render: Vec<Line> = wrapped_lines
                                .iter()
                                .take(if is_trunc {
                                    MAX_VISIBLE_LINES_PER_MESSAGE
                                } else {
                                    wrapped_lines.len()
                                })
                                .map(|line| {
                                    Line::from(line.to_string()).style(if *role == "user" {
                                        user_style
                                    } else {
                                        assistant_style
                                    })
                                })
                                .collect();
                            msg_lines_for_cache.extend(lines_to_render);
                            if is_trunc {
                                is_truncated_for_cache = true;
                            }
                        }
                        MessageSegment::Code {
                            language,
                            content: code_block_content,
                        } => {
                            app.code_blocks.push((
                                original_msg_idx,
                                crate::app::CodeBlock {
                                    content: code_block_content.clone(),
                                },
                            ));
                            msg_lines_for_cache.push(Line::raw(""));
                            let lang_display = language.as_deref().unwrap_or("code");

                            let block_width = chunks[0].width as usize;
                            let label = format!(" {} ", lang_display);
                            let border_len = block_width.saturating_sub(2 + label.len());
                            let right = if border_len > 0 {
                                border_len - border_len / 2
                            } else {
                                0
                            };
                            let top_border_str = format!("┌{}{}┐", label, "─".repeat(right));
                            msg_lines_for_cache
                                .push(Line::from(vec![Span::styled(top_border_str, border_style)]));

                            let syntax_set = get_syntax_set();
                            let theme = get_theme();
                            let syntax = syntax_set
                                .find_syntax_by_token(lang_display)
                                .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
                            let mut h = HighlightLines::new(syntax, theme);

                            for code_line_content in code_block_content.lines() {
                                let ranges = h
                                    .highlight_line(code_line_content, syntax_set)
                                    .unwrap_or_default();
                                let mut spans_for_line = vec![Span::styled("│ ", border_style)];
                                for (style, text_segment) in ranges {
                                    spans_for_line.push(Span::styled(
                                        text_segment.to_string(),
                                        Style::default()
                                            .fg(Color::Rgb(
                                                style.foreground.r,
                                                style.foreground.g,
                                                style.foreground.b,
                                            ))
                                            .bg(Color::Rgb(
                                                style.background.r,
                                                style.background.g,
                                                style.background.b,
                                            )),
                                    ));
                                }
                                msg_lines_for_cache.push(Line::from(spans_for_line));
                            }

                            let app_config = config::load_or_create_config();
                            let shortcuts = &app_config.keybindings.copy_code_blocks;
                            let hint_text = shortcuts
                                .get(code_block_count_for_message)
                                .map(|s| format!(" Copy [{}] ", s))
                                .unwrap_or_default();
                            let border_len = block_width.saturating_sub(2 + hint_text.len());
                            let right = if border_len > 0 {
                                border_len - border_len / 2
                            } else {
                                0
                            };
                            let bottom_border_str = format!("└{}{}┘", hint_text, "─".repeat(right));
                            msg_lines_for_cache.push(Line::from(vec![Span::styled(
                                bottom_border_str,
                                border_style,
                            )]));
                            msg_lines_for_cache.push(Line::raw(""));

                            code_block_count_for_message += 1;
                        }
                    }
                }
                app.line_cache
                    .push((msg_lines_for_cache, is_truncated_for_cache));
            }
            app.need_rebuild_cache = false;
        }

        let mut current_displayable_message_cache_idx = 0;

        let original_messages_count = app
            .chats
            .get(app.current_chat)
            .map_or(0, |c| c.messages.len());

        for original_msg_idx in 0..original_messages_count {
            if app.chats[app.current_chat].messages[original_msg_idx].role == "system" {
                continue;
            }

            if current_displayable_message_cache_idx < app.line_cache.len() {
                let (lines_from_cache, is_truncated_from_cache) =
                    &app.line_cache[current_displayable_message_cache_idx];

                for line_content in lines_from_cache.iter() {
                    buffer_lines.push(line_content.clone());
                    line_to_message_map.push((original_msg_idx, false));
                }
                if *is_truncated_from_cache {
                    buffer_lines.push(
                        Line::from("...".to_string()).style(Style::default().fg(Color::Gray)),
                    );
                    line_to_message_map.push((original_msg_idx, true));
                }
                buffer_lines.push(Line::raw(""));
                line_to_message_map.push((original_msg_idx, false));

                current_displayable_message_cache_idx += 1;
            }
        }
        if !is_streaming
            && !buffer_lines.is_empty()
            && buffer_lines.last().map_or(false, |l| l.spans.is_empty())
        {
            buffer_lines.pop();
            line_to_message_map.pop();
        }

        app.line_to_message = line_to_message_map.clone();

        app.display_buffer_text_content = buffer_lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        if is_streaming {
            if let Some(chat) = app.chats.get(app.current_chat) {
                let last_visible_msg = chat.messages.iter().rev().find(|m| m.role != "system");
                let show_loading = match last_visible_msg {
                    Some(msg) if msg.role == "assistant" && msg.content.trim().is_empty() => true,
                    None if !chat.messages.is_empty()
                        && chat.messages.iter().all(|m| m.role == "system") =>
                    {
                        true
                    }
                    None if chat.messages.is_empty() => true,
                    _ => false,
                };

                if show_loading {
                    let frames = ["🐱   ", "🐱.  ", "🐱.. ", "🐱...", "🐱 ..", "🐱  ."];
                    let frame_idx = (app.loading_frame / 6) % frames.len();
                    let frame_content = frames[frame_idx];
                    buffer_lines.push(Line::from(vec![
                        Span::styled(
                            frame_content,
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  Waiting for response..."),
                    ]));
                }
            }
        }

        let total_lines = buffer_lines.len();
        let viewport_height = chunks[0].height.saturating_sub(20).max(1) as usize;

        if total_lines > 0 && app.cursor_line >= total_lines {
            app.cursor_line = total_lines.saturating_sub(1);
        }

        let max_scroll = total_lines.saturating_sub(viewport_height);
        app.max_chat_scroll = max_scroll as u16;

        if app.chat_scroll == u16::MAX || (app.chat_scroll as usize) > max_scroll {
            app.chat_scroll = max_scroll as u16;
        }

        if app.cursor_line < app.chat_scroll as usize {
            app.chat_scroll = app.cursor_line as u16;
        } else if app.cursor_line >= (app.chat_scroll as usize + viewport_height) {
            app.chat_scroll = (app.cursor_line + 1).saturating_sub(viewport_height) as u16;
        }
        app.chat_scroll = app.chat_scroll.min(app.max_chat_scroll);

        let is_visual = app.mode == crate::app::Mode::Visual;
        let (vstart, vend) = match (app.visual_start, app.visual_end) {
            (Some(s), Some(e)) => (s.min(e), s.max(e)),
            _ => (usize::MAX, 0),
        };

        let is_focused = app.focus == crate::app::Focus::Chat;
        let title_text = if is_streaming {
            format!("{} ⏳", app.current_model_name())
        } else {
            app.current_model_name().to_string()
        };

        let mut display_lines_for_paragraph = Vec::with_capacity(buffer_lines.len());
        for (idx, line) in buffer_lines.iter().enumerate() {
            let mut styled_line = line.clone();
            if is_visual && idx >= vstart && idx <= vend {
                styled_line = styled_line.patch_style(visual_selection_style);
            }
            if idx == app.cursor_line {
                styled_line = styled_line.patch_style(cursor_style);
            }
            display_lines_for_paragraph.push(styled_line);
        }

        let paragraph = Paragraph::new(display_lines_for_paragraph)
            .block(
                Block::default()
                    .title(title_text)
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
            .wrap(Wrap { trim: false })
            .scroll((app.chat_scroll, 0));

        f.render_widget(paragraph, chunks[0]);

        let scrollbar_content_length = total_lines;
        if scrollbar_content_length > viewport_height {
            let mut scrollbar_state =
                ScrollbarState::new(scrollbar_content_length).position(app.chat_scroll as usize);
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(Color::DarkGray)),
                chunks[0].inner(Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                &mut scrollbar_state,
            );
        }
    } else {
        let paragraph = Paragraph::new("Start a new chat with 'n'")
            .block(
                Block::default()
                    .title(app.current_model_name())
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::Cyan)),
            )
            .alignment(Alignment::Center);
        f.render_widget(paragraph, chunks[0]);
        app.display_buffer_text_content.clear();
    }

    let mut current_status_text = String::new();
    if let Some(e) = &app.error_message {
        current_status_text = format!("Error: {}", e);
    } else if let Some(i) = &app.info_message {
        current_status_text = i.clone();
    }

    let input_text_display = match app.mode {
        Mode::Insert | Mode::RenameChat => format!("> {}", app.input),
        Mode::Command => format!(":{}", app.command),
        Mode::PromptInput => format!("Prompt: {}", app.input),
        Mode::Visual => {
            if !current_status_text.is_empty() {
                format!("-- VISUAL -- ({})", current_status_text)
            } else {
                "-- VISUAL --".to_string()
            }
        }
        Mode::Normal => current_status_text,
        _ => String::new(),
    };

    let input_block_style = Style::default().fg(if app.error_message.is_some() {
        Color::Red
    } else if app.info_message.is_some() && app.error_message.is_none() {
        Color::Green
    } else {
        Color::White
    });

    let input_block_title_str = match app.mode {
        Mode::Insert => "Insert",
        Mode::RenameChat => "Rename Chat",
        Mode::Command => "Command",
        Mode::Visual => "Visual",
        Mode::Normal => "Status",
        _ => "Input",
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(input_block_title_str)
        .style(input_block_style);

    let input_paragraph = Paragraph::new(input_text_display)
        .block(input_block)
        .wrap(Wrap { trim: true });
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

    let titles = ["Providers", "Shortcuts", "Prompts"]
        .iter()
        .cloned()
        .map(String::from)
        .collect::<Vec<_>>();

    let tabs = Tabs::new(titles)
        .select(match app.settings_tab {
            SettingsTab::Providers => 0,
            SettingsTab::Shortcuts => 1,
            SettingsTab::Prompts => 2,
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

    let main_settings_content_area = content_chunks[0];
    let settings_status_area = content_chunks[1];

    if app.mode == Mode::ApiKeyInput {
        let masked = mask_api_key(&app.api_key_old);
        let text = format!("Current: {}\nNew API Key: {}", masked, app.api_key_input);
        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Enter API Key"),
        );
        f.render_widget(paragraph, main_settings_content_area);
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
                f.render_stateful_widget(list, main_settings_content_area, &mut state);
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
                f.render_stateful_widget(list, main_settings_content_area, &mut state);
            }
            CustomModelStage::DerivedModelName => {
                let p = Paragraph::new(format!("Model Name: {}", app.custom_model_model_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Derived Model—Name"),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(p, main_settings_content_area);
            }
            CustomModelStage::StandaloneName => {
                let p = Paragraph::new(format!("Model Name: {}", app.custom_model_name_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Standalone Model—Name"),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(p, main_settings_content_area);
            }
            CustomModelStage::StandaloneUrl => {
                let p = Paragraph::new(format!("Endpoint URL: {}", app.custom_model_url_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Standalone Model—URL"),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(p, main_settings_content_area);
            }
            CustomModelStage::StandaloneModelId => {
                let p = Paragraph::new(format!("Model ID: {}", app.custom_model_model_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Add Standalone Model—Model ID"),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(p, main_settings_content_area);
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
                f.render_stateful_widget(list, main_settings_content_area, &mut state);
            }
            CustomModelStage::StandaloneApiKeyInput => {
                let p = Paragraph::new(format!("API Key: {}", app.custom_model_api_key_input))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Enter API Key"),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(p, main_settings_content_area);
            }
        }
    } else if app.mode == Mode::PromptInput {
        let title = if let Some(idx) = app.prompt_edit_idx {
            format!(
                "Edit Prompt: {}",
                app.prompts
                    .get(idx)
                    .map_or_else(|| "<Unknown>", |p| p.name.as_ref())
            )
        } else {
            "Add New Prompt".to_string()
        };
        let text_to_display = format!("Content: {}", app.input);
        let text_input_paragraph = Paragraph::new(text_to_display)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: true });

        f.render_widget(text_input_paragraph, main_settings_content_area);
    } else if app.settings_tab == SettingsTab::Prompts {
        let mut items = Vec::new();
        for prompt in &app.prompts {
            let status = if prompt.active { "[x]" } else { "[ ]" };
            items.push(ListItem::new(format!(
                "{} {}: {}",
                status, prompt.name, prompt.content
            )));
        }
        items.push(ListItem::new("  [Add New Prompt]"));

        let mut state = ListState::default();
        state.select(Some(app.selected_prompt_idx));
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Prompts"))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, main_settings_content_area, &mut state);
    } else if app.settings_tab == SettingsTab::Providers {
        let mut items = Vec::new();
        for p in &app.providers {
            let prefix = if p.expanded { "[-]" } else { "[+]" };
            items.push(ListItem::new(format!("{} {}", prefix, p.name)));

            if p.expanded {
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
        f.render_stateful_widget(list, main_settings_content_area, &mut state);
    } else {
        let paragraph = Paragraph::new("Shortcut customization coming soon!")
            .block(Block::default().borders(Borders::ALL).title("Shortcuts"));
        f.render_widget(paragraph, main_settings_content_area);
    }

    if let Some(err) = &app.error_message {
        let p = Paragraph::new(err.clone()).style(Style::default().fg(Color::Red));
        f.render_widget(p, settings_status_area);
    } else if let Some(info) = &app.info_message {
        let p = Paragraph::new(info.clone()).style(Style::default().fg(Color::Green));
        f.render_widget(p, settings_status_area);
    }
}
