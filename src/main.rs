mod api;
use crate::config::CustomModel;
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
use url::Url;

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
            for m in &saved.enabled_models {
                if !p.models.contains(m) {
                    p.models.push(m.clone());
                }
            }
        }
    }

    app.custom_models = config.custom_models.clone();

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
        app.loading_frame = app.loading_frame.wrapping_add(1); // Add this line
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
                    if let Some((_, cb)) =
                        app.code_blocks.iter().find(|(m_idx, _)| m_idx == msg_idx)
                    {
                        clipboard::copy_to_clipboard(&cb.content).await?;
                        app.set_error("Code block copied to clipboard");
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

                    let api_key = if provider_name == "Custom" {
                        // Handle standalone custom models
                        let mut custom_model_data = None;
                        if let Some(cm) = app.custom_models.iter().find(|cm| {
                            if let CustomModel::Standalone { name, .. } = cm {
                                name == model_name
                            } else {
                                false
                            }
                        }) {
                            if let CustomModel::Standalone {
                                endpoint,
                                model,
                                api_key,
                                use_key_from,
                                ..
                            } = cm
                            {
                                let key = api_key.clone().or_else(|| {
                                    use_key_from.as_ref().and_then(|p_name| {
                                        app.providers.iter().find(|p| &p.name == p_name).and_then(
                                            |p| {
                                                if !p.api_key.is_empty() {
                                                    Some(p.api_key.clone())
                                                } else {
                                                    None
                                                }
                                            },
                                        )
                                    })
                                });
                                custom_model_data = Some((endpoint.clone(), model.clone(), key));
                            }
                        }

                        if let Some((endpoint, model_id, key)) = custom_model_data {
                            app.add_user_message(msg.clone());
                            let chat = app.chats.get_mut(app.current_chat).unwrap();
                            chat.streaming = true;
                            let tx = app.start_stream(chat_id.clone());
                            app.need_rebuild_cache = true;
                            app.jump_to_last_message();

                            // Spawn task for standalone custom model
                            task::spawn(async move {
                                if let Err(e) = api::stream_openai_compatible(
                                    &endpoint,
                                    key.as_deref(),
                                    &model_id,
                                    &messages,
                                    tx,
                                )
                                .await
                                {
                                    eprintln!("Stream error: {:?}", e);
                                }
                            });
                            app.mode = Mode::Normal;
                            return Ok(());
                        } else {
                            app.set_error("Custom model not found");
                            app.mode = Mode::Normal;
                            return Ok(());
                        }
                    } else {
                        // Handle built-in providers and derived custom models
                        let provider = app.providers.iter().find(|p| p.name == provider_name);
                        match provider {
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
                app.add_user_message(msg);
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
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.error_message = None;
                app.info_message = None;
            }
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
                    for p in app.providers.iter_mut() {
                        if app.selected_line == idx {
                            p.expanded = !p.expanded;
                            return Ok(());
                        }
                        idx += 1;
                        if p.expanded {
                            let mut all_models: Vec<String> = p.models.iter().cloned().collect();
                            for m in &p.enabled_models {
                                if !all_models.contains(m) {
                                    all_models.push(m.clone());
                                }
                            }
                            all_models.sort();
                            for m in &all_models {
                                if app.selected_line == idx {
                                    if p.enabled_models.contains(m) {
                                        p.enabled_models.retain(|x| x != m);
                                    } else {
                                        p.enabled_models.push(m.clone());
                                    }
                                    if let Some(saved) =
                                        config.providers.iter_mut().find(|c| c.name == p.name)
                                    {
                                        saved.enabled_models = p.enabled_models.clone();
                                    }
                                    save_config(config);
                                    return Ok(());
                                }
                                idx += 1;
                            }
                        }
                    }

                    idx += 1;
                    for _ in app.custom_models.iter() {
                        idx += 1;
                    }
                    if app.selected_line == idx {
                        app.mode = Mode::CustomModelInput;
                        app.custom_model_input_stage =
                            Some(crate::app::CustomModelStage::TypeChoice);
                        app.custom_model_name_input.clear();
                        app.custom_model_url_input.clear();
                        app.error_message = None;
                        app.info_message = Some("Choose model type".to_string());
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
            KeyCode::Char('d') if app.settings_tab == SettingsTab::Providers => {
                let mut idx = 0;
                for p in &app.providers {
                    idx += 1;
                    if p.expanded {
                        idx += p.models.len();
                    }
                }
                let custom_header = idx;
                let start = custom_header + 1;
                let end = start + app.custom_models.len();
                if (start..end).contains(&app.selected_line) {
                    let cm_idx = app.selected_line - start;
                    app.custom_models.remove(cm_idx);
                    config.custom_models.remove(cm_idx);
                    save_config(config);
                    app.info_message = Some("Deleted custom model".to_string());
                    app.error_message = None;
                    if app.selected_line >= end - 1 {
                        app.selected_line = end - 1;
                    }
                }
            }
            _ => {}
        },
        Mode::ApiKeyInput => match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Settings;
                app.api_key_input.clear();
                app.api_key_editing_started = false;
                app.info_message = Some("API key edit cancelled".to_string());
                app.error_message = None;
            }
            KeyCode::Char(c) => {
                app.error_message = None;
                app.info_message = None;
                if !app.api_key_editing_started {
                    app.api_key_input.clear();
                    app.api_key_editing_started = true;
                }
                if app.api_key_input.len() < 128 {
                    app.api_key_input.push(c);
                }
            }
            KeyCode::Backspace => {
                app.error_message = None;
                if app.api_key_editing_started {
                    app.api_key_input.pop();
                }
            }
            KeyCode::Enter => {
                let inp = app.api_key_input.trim();
                if inp.len() < 8 {
                    app.error_message = Some("API key too short (min 8 chars)".to_string());
                } else {
                    let p = &mut app.providers[app.selected_provider_idx];
                    p.api_key = inp.to_string();
                    if let Some(saved) = config.providers.iter_mut().find(|c| c.name == p.name) {
                        saved.api_key = p.api_key.clone();
                    }
                    save_config(config);
                    app.mode = Mode::Settings;
                    app.api_key_input.clear();
                    app.api_key_editing_started = false;
                    app.info_message = Some("API key updated".to_string());
                    app.error_message = None;
                }
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
        Mode::CustomModelInput => match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Settings;
                app.custom_model_name_input.clear();
                app.custom_model_url_input.clear();
                app.custom_model_model_input.clear();
                app.custom_model_input_stage = None;
                app.custom_model_api_key_choice = None;
                app.custom_model_api_key_input.clear();
                app.info_message = Some("Custom model addition cancelled".to_string());
                app.error_message = None;
            }
            KeyCode::Char(c) => {
                app.error_message = None;
                app.info_message = None;
                match app.custom_model_input_stage.unwrap() {
                    crate::app::CustomModelStage::DerivedModelName => {
                        app.custom_model_model_input.push(c)
                    }
                    crate::app::CustomModelStage::StandaloneName => {
                        app.custom_model_name_input.push(c)
                    }
                    crate::app::CustomModelStage::StandaloneUrl => {
                        app.custom_model_url_input.push(c)
                    }
                    crate::app::CustomModelStage::StandaloneModelId => {
                        app.custom_model_model_input.push(c)
                    }
                    crate::app::CustomModelStage::StandaloneApiKeyInput => {
                        app.custom_model_api_key_input.push(c)
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                app.error_message = None;
                match app.custom_model_input_stage.unwrap() {
                    crate::app::CustomModelStage::DerivedModelName => {
                        app.custom_model_model_input.pop();
                    }

                    crate::app::CustomModelStage::StandaloneName => {
                        app.custom_model_name_input.pop();
                    }
                    crate::app::CustomModelStage::StandaloneUrl => {
                        app.custom_model_url_input.pop();
                    }

                    crate::app::CustomModelStage::StandaloneModelId => {
                        app.custom_model_model_input.pop();
                    }
                    crate::app::CustomModelStage::StandaloneApiKeyInput => {
                        app.custom_model_api_key_input.pop();
                    }
                    _ => {}
                }
            }
            KeyCode::Down | KeyCode::Up => match app.custom_model_input_stage.unwrap() {
                crate::app::CustomModelStage::TypeChoice => {
                    let items = vec!["Derived", "Standalone"];
                    let cur = app
                        .custom_model_api_key_choice
                        .as_ref()
                        .and_then(|choice| items.iter().position(|&n| n == choice))
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Down {
                        (cur + 1) % items.len()
                    } else {
                        (cur + items.len() - 1) % items.len()
                    };
                    app.custom_model_api_key_choice = Some(items[next].to_string());
                }
                crate::app::CustomModelStage::ProviderChoice => {
                    let items = app
                        .providers
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>();
                    let cur = app
                        .custom_model_api_key_choice
                        .as_ref()
                        .and_then(|choice| items.iter().position(|n| n == choice))
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Down {
                        (cur + 1) % items.len()
                    } else {
                        (cur + items.len() - 1) % items.len()
                    };
                    app.custom_model_api_key_choice = Some(items[next].clone());
                }
                crate::app::CustomModelStage::StandaloneApiKeyChoice => {
                    let mut items = app
                        .providers
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>();
                    items.push("Custom".to_string());
                    let cur = app
                        .custom_model_api_key_choice
                        .as_ref()
                        .and_then(|choice| items.iter().position(|n| n == choice))
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Down {
                        (cur + 1) % items.len()
                    } else {
                        (cur + items.len() - 1) % items.len()
                    };
                    app.custom_model_api_key_choice = Some(items[next].clone());
                }
                _ => {}
            },
            KeyCode::Enter => match app.custom_model_input_stage.unwrap() {
                crate::app::CustomModelStage::TypeChoice => {
                    if let Some(choice) = &app.custom_model_api_key_choice {
                        app.custom_model_input_stage = Some(if choice == "Derived" {
                            crate::app::CustomModelStage::ProviderChoice
                        } else {
                            crate::app::CustomModelStage::StandaloneName
                        });
                        if choice == "Derived" {
                            app.custom_model_api_key_choice = Some(app.providers[0].name.clone());
                        }
                    }
                }
                crate::app::CustomModelStage::ProviderChoice => {
                    if app.custom_model_api_key_choice.is_some() {
                        app.custom_model_input_stage =
                            Some(crate::app::CustomModelStage::DerivedModelName);
                    }
                }
                crate::app::CustomModelStage::DerivedModelName => {
                    let model = app.custom_model_model_input.trim().to_string();
                    let provider = app.custom_model_api_key_choice.clone();
                    if model.is_empty() {
                        app.error_message = Some("Model name cannot be empty".to_string());
                    } else if model.len() > 50 {
                        app.error_message = Some("Model name too long".to_string());
                    } else if let Some(provider) = provider {
                        let new_cm = CustomModel::Derived {
                            provider: provider.clone(),
                            model: model.clone(),
                        };
                        app.custom_models.push(new_cm.clone());
                        config.custom_models.push(new_cm.clone());
                        save_config(config);
                        app.mode = Mode::Settings;
                        app.custom_model_input_stage = None;
                        app.custom_model_name_input.clear();
                        app.custom_model_url_input.clear();
                        app.custom_model_model_input.clear();
                        app.custom_model_api_key_choice = None;
                        app.custom_model_api_key_input.clear();
                        app.info_message =
                            Some(format!("Added derived model '{}:{}'", provider, model));
                        let mut idx = 0;
                        for p in &app.providers {
                            idx += 1;
                            if p.expanded {
                                idx += p.models.len();
                            }
                        }
                        let custom_header = idx;
                        app.selected_line = custom_header + 1 + (app.custom_models.len() - 1);
                    }
                }
                crate::app::CustomModelStage::StandaloneName => {
                    let nm = app.custom_model_name_input.trim();
                    if nm.is_empty() {
                        app.error_message = Some("Model name cannot be empty".to_string());
                    } else if nm.len() > 50 {
                        app.error_message = Some("Model name too long".to_string());
                    } else {
                        app.custom_model_input_stage =
                            Some(crate::app::CustomModelStage::StandaloneUrl);
                    }
                }
                crate::app::CustomModelStage::StandaloneUrl => {
                    let url_str = app.custom_model_url_input.trim();
                    match Url::parse(url_str) {
                        Ok(u)
                            if u.scheme().eq_ignore_ascii_case("http")
                                || u.scheme().eq_ignore_ascii_case("https") =>
                        {
                            app.custom_model_input_stage =
                                Some(crate::app::CustomModelStage::StandaloneModelId);
                        }
                        _ => {
                            app.error_message = Some("Invalid URL format".to_string());
                        }
                    }
                }
                crate::app::CustomModelStage::StandaloneModelId => {
                    let model_id = app.custom_model_model_input.trim();
                    if model_id.is_empty() {
                        app.error_message = Some("Model ID cannot be empty".to_string());
                    } else {
                        app.custom_model_input_stage =
                            Some(crate::app::CustomModelStage::StandaloneApiKeyChoice);
                        let mut items = app
                            .providers
                            .iter()
                            .map(|p| p.name.clone())
                            .collect::<Vec<_>>();
                        items.push("Custom".to_string());
                        app.custom_model_api_key_choice = Some(items[0].clone());
                    }
                }
                crate::app::CustomModelStage::StandaloneApiKeyChoice => {
                    if let Some(choice) = app.custom_model_api_key_choice.clone() {
                        if choice == "Custom" {
                            app.custom_model_input_stage =
                                Some(crate::app::CustomModelStage::StandaloneApiKeyInput);
                        } else {
                            let new_cm = CustomModel::Standalone {
                                name: app.custom_model_name_input.trim().to_string(),
                                endpoint: app.custom_model_url_input.trim().to_string(),
                                model: app.custom_model_model_input.trim().to_string(),
                                api_key: None,
                                use_key_from: Some(choice.clone()),
                            };
                            app.custom_models.push(new_cm.clone());
                            config.custom_models.push(new_cm.clone());
                            save_config(config);
                            app.mode = Mode::Settings;
                            app.custom_model_input_stage = None;
                            app.custom_model_name_input.clear();
                            app.custom_model_url_input.clear();
                            app.custom_model_model_input.clear();
                            app.custom_model_api_key_choice = None;
                            app.custom_model_api_key_input.clear();
                            app.info_message =
                                Some(format!("Added standalone model '{}'", new_cm.name()));
                            let mut idx = 0;
                            for p in &app.providers {
                                idx += 1;
                                if p.expanded {
                                    idx += p.models.len();
                                }
                            }
                            let custom_header = idx;
                            app.selected_line = custom_header + 1 + (app.custom_models.len() - 1);
                        }
                    }
                }
                crate::app::CustomModelStage::StandaloneApiKeyInput => {
                    let key = app.custom_model_api_key_input.trim();
                    if key.len() < 8 {
                        app.error_message = Some("API key too short (min 8 chars)".to_string());
                    } else {
                        let new_cm = CustomModel::Standalone {
                            name: app.custom_model_name_input.trim().to_string(),
                            endpoint: app.custom_model_url_input.trim().to_string(),
                            model: app.custom_model_model_input.trim().to_string(),
                            api_key: Some(key.to_string()),
                            use_key_from: None,
                        };
                        app.custom_models.push(new_cm.clone());
                        config.custom_models.push(new_cm.clone());
                        save_config(config);
                        app.mode = Mode::Settings;
                        app.custom_model_input_stage = None;
                        app.custom_model_name_input.clear();
                        app.custom_model_url_input.clear();
                        app.custom_model_model_input.clear();
                        app.custom_model_api_key_choice = None;
                        app.custom_model_api_key_input.clear();
                        app.info_message =
                            Some(format!("Added standalone model '{}'", new_cm.name()));
                        let mut idx = 0;
                        for p in &app.providers {
                            idx += 1;
                            if p.expanded {
                                idx += p.models.len();
                            }
                        }
                        let custom_header = idx;
                        app.selected_line = custom_header + 1 + (app.custom_models.len() - 1);
                    }
                }
            },
            _ => {}
        },
        Mode::Command => {}
    }
    Ok(())
}
