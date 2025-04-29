mod api;
mod app;
mod clipboard;
mod config;
mod storage;
mod ui;

use crate::app::Focus;
use crate::app::{App, Mode, SettingsTab};
use crate::config::{load_or_create_config, save_config};
use crate::storage::{load_history, save_history};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::env;
use std::{io, time::Duration};
use tokio::task;

#[tokio::main]

async fn main() -> Result<()> {
    let mut app = App::new();
    app.chats = load_history();

    if !app.chats.is_empty() {
        app.current_chat = 0;
        app.selected_sidebar_idx = 0;
        app.focus = Focus::Chat;
        app.need_rebuild_cache = true;
        if let Some(chat) = app.chats.get(0) {
            app.current_model = chat.model.clone();
        }
    }
    let mut config = load_or_create_config();

    for saved in &config.providers {
        if let Some(p) = app.providers.iter_mut().find(|p| p.name == saved.name) {
            p.api_key = saved.api_key.clone();
            p.enabled_models = saved.enabled_models.clone();
        }
    }

    let enabled = app.enabled_models_flat();
    if let Some((provider, model)) = enabled.get(0) {
        if app.chats.is_empty() {
            app.current_model = format!("{}:{}", provider, model);
        }
    } else if app.chats.is_empty() {
        app.current_model = "No model selected".to_string();
    }
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &mut app, &mut config).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    save_history(&app.chats);

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App<'_>,
    config: &mut config::Settings,
) -> Result<()> {
    loop {
        app.process_stream();
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key, config).await?;
            }
        }
    }
}

async fn handle_key(app: &mut App<'_>, key: KeyEvent, config: &mut config::Settings) -> Result<()> {
    match app.mode {
        Mode::Normal => match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if app.focus == crate::app::Focus::Sidebar {
                    if app.selected_sidebar_idx < app.chats.len() {
                        app.selected_sidebar_idx += 1;
                    }
                } else {
                    app.cursor_line = app.cursor_line.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.focus == crate::app::Focus::Sidebar {
                    if app.selected_sidebar_idx > 0 {
                        app.selected_sidebar_idx -= 1;
                    }
                } else {
                    app.cursor_line = app.cursor_line.saturating_sub(1);
                }
            }
            KeyCode::Char('g') => {
                if app.focus == crate::app::Focus::Sidebar {
                    app.selected_sidebar_idx = 0;
                } else {
                    app.cursor_line = 0;
                }
            }
            KeyCode::Char('G') => {
                if app.focus == crate::app::Focus::Sidebar {
                    app.selected_sidebar_idx = app.chats.len();
                } else {
                    app.jump_to_last_message();
                }
            }
            KeyCode::Tab => {
                if app.sidebar_visible {
                    app.focus = match app.focus {
                        crate::app::Focus::Sidebar => crate::app::Focus::Chat,
                        crate::app::Focus::Chat => crate::app::Focus::Sidebar,
                    };
                }
            }
            KeyCode::PageUp => {
                if !app.sidebar_visible {
                    let viewport_height = 10;
                    app.cursor_line = app.cursor_line.saturating_sub(viewport_height);
                }
            }
            KeyCode::PageDown => {
                if !app.sidebar_visible {
                    let viewport_height = 10;
                    app.cursor_line = app.cursor_line.saturating_add(viewport_height);
                }
            }
            KeyCode::Char('o') => {
                app.mode = Mode::Settings;
            }
            KeyCode::Char('e') => {
                if let Some((msg_idx, _)) = app.line_to_message.get(app.cursor_line) {
                    app.toggle_message_truncation(*msg_idx);
                }
            }
            KeyCode::Esc => {
                if app.show_full_message.is_some() {
                    app.show_full_message = None;
                } else {
                    return Err(anyhow::anyhow!("Quit"));
                }
            }
            KeyCode::Char('i') => {
                app.mode = Mode::Insert;
                app.clear_error();
            }
            KeyCode::Char('n') => app.create_new_chat(),
            KeyCode::Char('s') => app.toggle_sidebar(),
            KeyCode::Char('m') => {
                app.mode = Mode::ModelSelect;
                app.selected_model_idx = 0;
            }
            KeyCode::Char('r') => {
                if app.sidebar_visible && app.selected_sidebar_idx < app.chats.len() {
                    app.input = app.chats[app.selected_sidebar_idx].title.clone();
                    app.mode = Mode::RenameChat;
                }
            }
            KeyCode::Char('d') => {
                if app.sidebar_visible && app.selected_sidebar_idx < app.chats.len() {
                    app.chats.remove(app.selected_sidebar_idx);
                    if app.chats.is_empty() {
                        app.current_chat = 0;
                        app.selected_sidebar_idx = 0;
                        app.cursor_line = 0;
                        app.line_cache.clear();
                        app.line_to_message.clear();
                        app.need_rebuild_cache = true;
                    } else {
                        if app.selected_sidebar_idx >= app.chats.len() {
                            app.selected_sidebar_idx = app.chats.len() - 1;
                        }
                        app.current_chat = app.selected_sidebar_idx;
                        app.cursor_line = 0;
                        app.need_rebuild_cache = true;
                    }
                }
            }

KeyCode::Char('c') => {
    if let Some((msg_idx, _)) = app.line_to_message.get(app.cursor_line) {
        if let Some((_, cb)) = app.code_blocks.iter().find(|(m_idx, _)| m_idx == msg_idx) {
            clipboard::copy_to_clipboard(&cb.content).await?;
            // app.set_error("Code block copied to clipboard");
        }
    }
}
KeyCode::Char('C') => {
    if let Some((msg_idx, _)) = app.line_to_message.get(app.cursor_line) {
        if let Some((_, cb)) = app
            .code_blocks
            .iter()
            .filter(|(m_idx, _)| m_idx == msg_idx)
            .nth(1)
        {
            clipboard::copy_to_clipboard(&cb.content).await?;
            app.set_error("Code block copied to clipboard");
        }
    }
}
KeyCode::Char('x') => {
    if let Some((msg_idx, _)) = app.line_to_message.get(app.cursor_line) {
        if let Some((_, cb)) = app
            .code_blocks
            .iter()
            .filter(|(m_idx, _)| m_idx == msg_idx)
            .nth(2)
        {
            clipboard::copy_to_clipboard(&cb.content).await?;
            app.set_error("Code block copied to clipboard");
        }
    }
}
KeyCode::Char('X') => {
    if let Some((msg_idx, _)) = app.line_to_message.get(app.cursor_line) {
        if let Some((_, cb)) = app
            .code_blocks
            .iter()
            .filter(|(m_idx, _)| m_idx == msg_idx)
            .nth(3)
        {
            clipboard::copy_to_clipboard(&cb.content).await?;
            app.set_error("Code block copied to clipboard");
        }
    }
}

            KeyCode::Enter => {
                if app.focus == crate::app::Focus::Sidebar {
                    if app.selected_sidebar_idx < app.chats.len() {
                        app.current_chat = app.selected_sidebar_idx;
                        if let Some(chat) = app.chats.get(app.current_chat) {
                            app.current_model = chat.model.clone();
                            app.jump_to_last_message();
                            app.chat_scroll = u16::MAX;
                            app.need_rebuild_cache = true;
                        }
                    } else if app.selected_sidebar_idx == app.chats.len() {
                        app.mode = Mode::Settings;
                    }
                }
            }
            _ => {}
        },
        Mode::Insert => match key.code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Enter => {
                if !app.has_valid_chat() {
                    app.set_error("No chat selected. Press 'n' to create a new chat.");
                    app.mode = Mode::Normal;
                    return Ok(());
                }
                let msg = app.input.clone();
                app.input.clear();

                // Step 1: Gather data immutably
                let (chat_id, messages, provider_name, model_name, api_key) = {
                    let chat = app
                        .chats
                        .get(app.current_chat)
                        .ok_or_else(|| anyhow::anyhow!("No chat selected"))?;
                    if chat.streaming {
                        app.mode = Mode::Normal;
                        return Ok(());
                    }
                    let chat_id = chat.id.clone();
                    let mut messages = chat.messages.clone();
                    messages.push(crate::app::Message {
                        role: "user".to_string(),
                        content: msg.clone(),
                    });

                    let model_parts: Vec<&str> = chat.model.split(':').collect();
                    if model_parts.len() != 2 {
                        app.set_error("Invalid model format");
                        app.mode = Mode::Normal;
                        return Ok(());
                    }
                    let provider_name = model_parts[0];
                    let model_name = model_parts[1];

                    let provider = app.providers.iter().find(|p| p.name == provider_name);
                    let api_key = match provider {
                        Some(p) if !p.api_key.is_empty() => p.api_key.clone(),
                        _ => {
                            let env_key = match provider_name {
                                "OpenAI" => "OPENAI_API_KEY",
                                "Grok" => "GROK_API_KEY",
                                "Anthropic" => "ANTHROPIC_API_KEY",
                                _ => {
                                    app.set_error(&format!(
                                        "No API key set for provider {}",
                                        provider_name
                                    ));
                                    app.mode = Mode::Normal;
                                    return Ok(());
                                }
                            };
                            match env::var(env_key) {
                                Ok(key) if !key.is_empty() => key,
                                _ => {
                                    app.set_error(&format!(
                                        "No API key set for provider {}. Set {} or configure in settings.",
                                        provider_name, env_key
                                    ));
                                    app.mode = Mode::Normal;
                                    return Ok(());
                                }
                            }
                        }
                    };

                    (
                        chat_id,
                        messages,
                        provider_name.to_string(),
                        model_name.to_string(),
                        api_key,
                    )
                };

                // Step 2: Perform mutable operations
                app.add_user_message(msg); // Use new method to ensure truncation
                let chat = app.chats.get_mut(app.current_chat).unwrap();
                chat.streaming = true;
                let tx = app.start_stream(chat_id.clone());
                app.need_rebuild_cache = true;
                app.jump_to_last_message();

                // Step 3: Spawn async task
                task::spawn(async move {
                    if let Err(e) =
                        api::stream_message(&api_key, &provider_name, &model_name, &messages, tx)
                            .await
                    {
                        eprintln!("Stream error: {:?}", e);
                    }
                });

                app.mode = Mode::Normal;
            }
            KeyCode::Char(c) => app.input.push(c),
            KeyCode::Backspace => {
                app.input.pop();
            }
            _ => {}
        },
        Mode::ModelSelect => match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let models = app.enabled_models_flat();
                if app.selected_model_idx + 1 < models.len() {
                    app.selected_model_idx += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.selected_model_idx > 0 {
                    app.selected_model_idx -= 1;
                }
            }
            KeyCode::Enter => {
                let models = app.enabled_models_flat();
                if let Some((provider, model)) = models.get(app.selected_model_idx) {
                    let new_model = format!("{}:{}", provider, model);
                    app.current_model = new_model.clone();
                    if let Some(chat) = app.chats.get_mut(app.current_chat) {
                        chat.model = new_model;
                    }
                }
                app.mode = Mode::Normal;
            }
            KeyCode::Esc => app.mode = Mode::Normal,
            _ => {}
        },
        Mode::Settings => match key.code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Char('s') => app.toggle_sidebar(),
            KeyCode::Char('h') | KeyCode::Left => {
                app.settings_tab = SettingsTab::Providers;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                app.settings_tab = SettingsTab::Shortcuts;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                app.selected_line += 1;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.selected_line > 0 {
                    app.selected_line -= 1;
                }
            }
            KeyCode::Enter => {
                if app.settings_tab == SettingsTab::Providers {
                    let mut idx = 0;
                    for (_p_idx, p) in app.providers.iter_mut().enumerate() {
                        if app.selected_line == idx {
                            p.expanded = !p.expanded;
                            break;
                        }
                        idx += 1;
                        if p.expanded {
                            for (_m_idx, m) in p.models.iter().enumerate() {
                                if app.selected_line == idx {
                                    if p.enabled_models.contains(m) {
                                        p.enabled_models.retain(|x| x != m);
                                    } else {
                                        p.enabled_models.push(m.clone());
                                    }
                                    for saved in &mut config.providers {
                                        if saved.name == p.name {
                                            saved.enabled_models = p.enabled_models.clone();
                                        }
                                    }
                                    save_config(config);
                                    break;
                                }
                                idx += 1;
                            }
                        }
                    }
                }
            }
            KeyCode::Char('e') => {
                let mut idx = 0;
                for (_p_idx, p) in app.providers.iter().enumerate() {
                    if app.selected_line == idx {
                        app.api_key_input = p.api_key.clone();
                        app.selected_provider_idx = _p_idx;
                        app.mode = Mode::ApiKeyInput;
                        break;
                    }
                    idx += 1;
                    if p.expanded {
                        idx += p.models.len();
                    }
                }
            }
            _ => {}
        },
        Mode::ApiKeyInput => match key.code {
            KeyCode::Esc => app.mode = Mode::Settings,
            KeyCode::Enter => {
                let p = &mut app.providers[app.selected_provider_idx];
                p.api_key = app.api_key_input.clone();
                for saved in &mut config.providers {
                    if saved.name == p.name {
                        saved.api_key = p.api_key.clone();
                    }
                }
                save_config(config);
                app.mode = Mode::Settings;
            }
            KeyCode::Backspace => {
                app.api_key_input.pop();
            }
            KeyCode::Char(c) => {
                app.api_key_input.push(c);
            }
            _ => {}
        },
        Mode::RenameChat => match key.code {
            KeyCode::Esc => {
                app.input.clear();
                app.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                if app.selected_sidebar_idx < app.chats.len() {
                    if !app.input.trim().is_empty() {
                        app.chats[app.selected_sidebar_idx].title = app.input.clone();
                    }
                }
                app.input.clear();
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Char(c) => {
                app.input.push(c);
            }
            _ => {}
        },
        Mode::Command => {}
    }
    Ok(())
}
