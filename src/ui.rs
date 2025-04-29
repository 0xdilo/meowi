use crate::app::{App, Mode, SettingsTab};
use ratatui::widgets::ListState;
use ratatui::widgets::Padding;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Tabs,
    },
};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
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
        Mode::Settings | Mode::ApiKeyInput => draw_settings(f, app, chunks[1]),
        Mode::ModelSelect => draw_model_select(f, app, chunks[1]),
        _ => draw_chat(f, app, chunks[1]),
    }
}

// one-time loaders
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

use ratatui::text::Span;

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
    let code_style = Style::default()
        .fg(Color::White)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let border_style = Style::default().fg(Color::DarkGray);
    let shortcut_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

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

            // Collect message contents first to avoid holding borrow
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

            // Now parse code blocks and build line cache
            for (msg_idx, role, content) in messages {
                app.parse_code_blocks(msg_idx, &content);

                let mut wrapped_lines = Vec::new();
                let mut line_count = 0;

                let filtered: String = content
                    .lines()
                    .filter(|l| !l.trim().starts_with("```"))
                    .collect::<Vec<_>>()
                    .join("\n");

                let non_code_lines = wrap(&filtered, text_width);
                for line in non_code_lines {
                    wrapped_lines.push((line.to_string(), false, None::<String>));
                    line_count += 1;
                }

                let is_truncated = app.truncated_messages.contains(&msg_idx)
                    && wrapped_lines.len() > MAX_VISIBLE_LINES_PER_MESSAGE;

                let styled_lines: Vec<Line> = wrapped_lines
                    .iter()
                    .take(if is_truncated {
                        MAX_VISIBLE_LINES_PER_MESSAGE
                    } else {
                        wrapped_lines.len()
                    })
                    .enumerate()
                    .map(|(line_idx, (line, is_code, _lang))| {
                        let code_block = app.code_blocks.iter().find(|(m_idx, cb)| {
                            *m_idx == msg_idx
                                && line_idx >= cb.start_line
                                && line_idx < cb.start_line + cb.content.lines().count()
                        });

                        if let Some((_, cb)) = code_block {
                            Line::from(vec![Span::styled(line.clone(), code_style)])
                        } else {
                            Line::from(line.clone()).style(if role == "user" {
                                user_style
                            } else {
                                assistant_style
                            })
                        }
                    })
                    .collect();

                app.line_cache.push((styled_lines, is_truncated));
            }
            app.need_rebuild_cache = false;
        }

        // --- enhanced code blocks with syntax highlighting and proper fences ---
        let syntax_set = get_syntax_set();
        let theme = get_theme();
        let shortcuts = vec![
            "c".to_string(),
            "C".to_string(),
            "x".to_string(),
            "X".to_string(),
        ];

        let mut global_line_idx = 0;
        for (msg_idx, (lines, is_truncated)) in app.line_cache.iter().enumerate() {
            let cbs: Vec<_> = app
                .code_blocks
                .iter()
                .enumerate()
                .filter(|(_, (m, _))| *m == msg_idx)
                .collect();
            let mut next_cb = cbs.iter().peekable();
            let mut line_idx = 0;

            while line_idx < lines.len() {
                if let Some((cb_i, (_, cb))) = next_cb.peek().cloned() {
                    if line_idx == cb.start_line {
                        // blank line before
                        let mut newline = Line::raw("");
                        if global_line_idx == app.cursor_line {
                            newline = newline.patch_style(cursor_style);
                        }
                        buffer_lines.push(newline);
                        line_to_message.push((msg_idx, false));
                        global_line_idx += 1;

                        // top border with lang
                        let lang = cb.language.as_deref().unwrap_or("code");
                        let label = format!(" {} ", lang);
                        let width = text_width;
                        let dashes = width.saturating_sub(2 + label.len());
                        let top = format!("┌─{}{}┐", label, "─".repeat(dashes));
                        let mut top_line = Line::from(vec![Span::styled(top, border_style)]);
                        if global_line_idx == app.cursor_line {
                            top_line = top_line.patch_style(cursor_style);
                        }
                        buffer_lines.push(top_line);
                        line_to_message.push((msg_idx, false));
                        global_line_idx += 1;

                        // syntax highlight each code line
                        let syntax = syntax_set
                            .find_syntax_by_token(lang)
                            .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
                        let mut h = HighlightLines::new(syntax, theme);
                        for code in cb.content.lines() {
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
                            buffer_lines.push(Line::from(spans));
                            line_to_message.push((msg_idx, false));
                            global_line_idx += 1;
                        }

                        // bottom border with copy hint, right-aligned
                        let hint = shortcuts
                            .get(*cb_i)
                            .map(|s| format!(" Copy [{}] ", s))
                            .unwrap_or_default();
                        let dash_count = width.saturating_sub(2 + hint.len());
                        let bottom = format!("└{}{}┘", "─".repeat(dash_count), hint);
                        let mut bottom_line = Line::from(vec![Span::styled(bottom, border_style)]);
                        if global_line_idx == app.cursor_line {
                            bottom_line = bottom_line.patch_style(cursor_style);
                        }
                        buffer_lines.push(bottom_line);
                        line_to_message.push((msg_idx, false));
                        global_line_idx += 1;

                        // skip all code lines in wrapped 'lines'
                        line_idx = cb.start_line + cb.content.lines().count();
                        next_cb.next();
                        continue;
                    }
                }
                // non-code line
                let mut styled_line = lines[line_idx].clone();
                if global_line_idx == app.cursor_line {
                    styled_line = styled_line.patch_style(cursor_style);
                }
                buffer_lines.push(styled_line);
                line_to_message.push((msg_idx, false));
                global_line_idx += 1;
                line_idx += 1;
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

        let total_lines = buffer_lines.len();
        let viewport_height = chunks[0].height.saturating_sub(2) as usize;

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
            app.chat_scroll = (app.cursor_line - viewport_height + 1) as u16;
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

    // Input area
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

fn draw_settings(f: &mut Frame<'_>, app: &App, area: Rect) {
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

    match app.mode {
        Mode::ApiKeyInput => {
            let paragraph = Paragraph::new(format!("API Key: {}", app.api_key_input)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Enter API Key"),
            );
            f.render_widget(paragraph, chunks[1]);
        }
        _ => match app.settings_tab {
            SettingsTab::Providers => {
                let mut items = Vec::new();
                for p in &app.providers {
                    let prefix = if p.expanded { "[-]" } else { "[+]" };
                    items.push(ListItem::new(format!("{} {}", prefix, p.name)));
                    if p.expanded {
                        for m in &p.models {
                            let checked = if p.enabled_models.contains(m) {
                                "[x]"
                            } else {
                                "[ ]"
                            };
                            items.push(ListItem::new(format!("    {} {}", checked, m)));
                        }
                    }
                }

                let mut state = ListState::default();
                state.select(Some(app.selected_line));

                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Providers"))
                    .highlight_style(
                        Style::default()
                            .bg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    );

                f.render_stateful_widget(list, chunks[1], &mut state);
            }
            SettingsTab::Shortcuts => {
                let paragraph = Paragraph::new("Shortcut customization coming soon!")
                    .block(Block::default().borders(Borders::ALL).title("Shortcuts"));
                f.render_widget(paragraph, chunks[1]);
            }
        },
    }
}
