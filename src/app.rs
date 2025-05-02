use crate::config::CustomModel;
use once_cell::sync::Lazy;
use ratatui::text::Line;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::{self, Receiver, Sender};
use uuid::Uuid; // <-- Add this!

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    #[inline(always)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

impl From<&str> for Role {
    fn from(s: &str) -> Self {
        match s {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => Role::User,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub content: String,
    pub language: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomModelStage {
    TypeChoice,
    ProviderChoice,
    DerivedModelName,
    StandaloneName,
    StandaloneUrl,
    StandaloneModelId,
    StandaloneApiKeyChoice,
    StandaloneApiKeyInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
    Settings,
    ModelSelect,
    ApiKeyInput,
    RenameChat,
    CustomModelInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    #[inline(always)]
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role: role.as_str().to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub messages: Vec<Message>,
    pub model: String,
    pub streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Providers,
    Shortcuts,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub enabled_models: Vec<String>,
    pub expanded: bool,
}

pub struct StreamTask {
    pub chat_id: String,
    pub rx: Receiver<String>,
}

pub struct App<'a> {
    pub mode: Mode,
    pub chats: Vec<Chat>,
    pub current_chat: usize,
    pub sidebar_visible: bool,
    pub input: String,
    pub command: String,
    pub providers: Vec<Provider>,
    pub current_model: String,
    pub settings_tab: SettingsTab,
    pub selected_provider_idx: usize,
    pub selected_model_idx: usize,
    pub selected_line: usize,
    pub api_key_input: String,
    pub selected_sidebar_idx: usize,
    pub chat_scroll: u16,
    pub max_chat_scroll: u16,
    pub cursor_line: usize,
    pub show_full_message: Option<usize>,
    pub last_width: usize,
    pub line_cache: Vec<(Vec<Line<'a>>, bool)>,
    pub truncated_messages: HashSet<usize>,
    pub need_rebuild_cache: bool,
    pub line_to_message: Vec<(usize, bool)>,
    pub focus: Focus,
    pub stream_tasks: HashMap<String, StreamTask>,
    pub error_message: Option<String>,
    pub code_blocks: Vec<(usize, CodeBlock)>,
    pub api_key_old: String,
    pub api_key_editing_started: bool,
    pub info_message: Option<String>,
    pub custom_models: Vec<CustomModel>,
    pub custom_model_name_input: String,
    pub custom_model_url_input: String,
    pub custom_model_input_stage: Option<CustomModelStage>,
    pub custom_model_model_input: String,
    pub custom_model_api_key_choice: Option<String>,
    pub custom_model_api_key_input: String,
    pub loading_frame: usize,
}

impl<'a> App<'a> {
    pub fn new() -> Self {
        let providers = vec![
            Provider {
                name: "OpenAI".to_string(),
                api_key: String::new(),
                models: crate::config::openai_models(),
                enabled_models: crate::config::openai_models(),
                expanded: false,
            },
            Provider {
                name: "Anthropic".to_string(),
                api_key: String::new(),
                models: crate::config::anthropic_models(),
                enabled_models: crate::config::anthropic_models(),
                expanded: false,
            },
            Provider {
                name: "Grok".to_string(),
                api_key: String::new(),
                models: crate::config::grok_models(),
                enabled_models: crate::config::grok_models(),
                expanded: false,
            },
        ];

        let mut app = Self {
            mode: Mode::Normal,
            chats: Vec::new(),
            current_chat: 0,
            sidebar_visible: true,
            input: String::new(),
            command: String::new(),
            providers,
            current_model: "OpenAI:gpt-4o".to_string(),
            settings_tab: SettingsTab::Providers,
            selected_provider_idx: 0,
            selected_model_idx: 0,
            selected_line: 0,
            api_key_input: String::new(),
            selected_sidebar_idx: 0,
            chat_scroll: u16::MAX,
            max_chat_scroll: 0,
            cursor_line: 0,
            show_full_message: None,
            last_width: 0,
            line_cache: Vec::new(),
            truncated_messages: HashSet::new(),
            need_rebuild_cache: true,
            line_to_message: Vec::new(),
            focus: Focus::Chat,
            stream_tasks: HashMap::new(),
            error_message: None,
            code_blocks: Vec::new(),
            api_key_old: String::new(),
            api_key_editing_started: false,
            info_message: None,
            custom_models: Vec::new(),
            custom_model_name_input: String::new(),
            custom_model_url_input: String::new(),
            custom_model_input_stage: None,
            custom_model_model_input: String::new(),
            custom_model_api_key_choice: None,
            custom_model_api_key_input: String::new(),
            loading_frame: 0,
        };
        if app.chats.is_empty() {
            app.create_new_chat();
        }
        app
    }

    #[inline(always)]
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        self.focus = if self.sidebar_visible {
            Focus::Sidebar
        } else {
            Focus::Chat
        };
    }

    pub fn create_new_chat(&mut self) {
        let chat = Chat {
            id: Uuid::new_v4().to_string(),
            title: format!("Chat {}", self.chats.len() + 1),
            messages: Vec::new(),
            model: self.current_model.clone(),
            streaming: false,
        };
        self.chats.push(chat);
        self.current_chat = self.chats.len() - 1;
        self.selected_sidebar_idx = self.current_chat;
        self.chat_scroll = u16::MAX;
        self.cursor_line = 0;
        self.need_rebuild_cache = true;
        self.truncated_messages.clear();
    }

    #[inline(always)]
    pub fn current_model_name(&self) -> &str {
        &self.current_model
    }

    /// Returns a flat list of enabled models (provider, model).
    pub fn enabled_models_flat(&self) -> Vec<(Cow<'_, str>, Cow<'_, str>)> {
        let mut list = Vec::with_capacity(8);
        for p in &self.providers {
            for m in &p.enabled_models {
                list.push((Cow::Borrowed(p.name.as_str()), Cow::Borrowed(m.as_str())));
            }
        }
        for cm in &self.custom_models {
            match cm {
                CustomModel::Derived { provider, model } => {
                    list.push((
                        Cow::Borrowed(provider.as_str()),
                        Cow::Borrowed(model.as_str()),
                    ));
                }
                CustomModel::Standalone { name, .. } => {
                    list.push((Cow::Borrowed("Custom"), Cow::Borrowed(name.as_str())));
                }
            }
        }
        list
    }

    pub fn jump_to_last_message(&mut self) {
        let mut total_lines = 0;
        for (lines, is_truncated) in &self.line_cache {
            total_lines += lines.len();
            if *is_truncated {
                total_lines += 1;
            }
            total_lines += 1;
        }
        self.cursor_line = if total_lines > 0 { total_lines - 1 } else { 0 };
        self.chat_scroll = u16::MAX;
    }

    pub fn start_stream(&mut self, chat_id: String) -> Sender<String> {
        let (tx, rx) = mpsc::channel(100);
        self.stream_tasks
            .insert(chat_id.clone(), StreamTask { chat_id, rx });
        tx
    }

    pub fn process_stream(&mut self) {
        let mut to_remove = Vec::new();
        let mut content_updated = false;
        let mut new_code_blocks = Vec::new();
        let mut processed_chunks = Vec::new();

        for (chat_id, task) in self.stream_tasks.iter_mut() {
            while let Ok(chunk) = task.rx.try_recv() {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.id == *chat_id) {
                    chat.streaming = true;
                    let msg_idx = chat.messages.len();
                    if let Some(last_msg) = chat.messages.last_mut() {
                        if last_msg.role == "assistant" {
                            last_msg.content.push_str(&chunk);
                            processed_chunks.push((msg_idx - 1, last_msg.content.clone()));
                        } else {
                            chat.messages.push(Message::new(Role::Assistant, &chunk));
                            processed_chunks.push((msg_idx, chunk.clone()));
                        }
                    } else {
                        chat.messages.push(Message::new(Role::Assistant, &chunk));
                        processed_chunks.push((msg_idx, chunk.clone()));
                    }
                    self.need_rebuild_cache = true;
                    content_updated = true;
                    self.truncated_messages.remove(&msg_idx);
                }
            }
            if task.rx.is_closed() {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.id == *chat_id) {
                    chat.streaming = false;
                }
                to_remove.push(chat_id.clone());
            }
        }
        for chat_id in to_remove {
            self.stream_tasks.remove(&chat_id);
        }

        for (msg_idx, content) in processed_chunks {
            new_code_blocks.extend(self.parse_code_blocks_helper(msg_idx, &content));
        }

        self.code_blocks.extend(new_code_blocks);
        if content_updated {
            self.jump_to_last_message();
        }
    }

    fn parse_code_blocks_helper(&self, msg_idx: usize, content: &str) -> Vec<(usize, CodeBlock)> {
        static OPENING_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^```(\w+)?\s*$").unwrap());
        static CLOSING_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^```\s*$").unwrap());

        let mut blocks = Vec::new();
        let mut idx = 0;
        let lines: Vec<&str> = content.lines().collect();
        while idx < lines.len() {
            if let Some(caps) = OPENING_RE.captures(lines[idx]) {
                let lang = caps.get(1).and_then(|m| {
                    let s = m.as_str();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                });
                let start_fence = idx;
                idx += 1;
                let mut code_lines = Vec::new();
                while idx < lines.len() && !CLOSING_RE.is_match(lines[idx]) {
                    code_lines.push(lines[idx]);
                    idx += 1;
                }
                let end_fence = idx;
                let content_str = code_lines.join("\n");
                blocks.push((
                    msg_idx,
                    CodeBlock {
                        content: content_str,
                        language: lang,
                        start_line: start_fence + 1,
                        end_line: end_fence.saturating_sub(1),
                    },
                ));
            }
            idx += 1;
        }
        blocks
    }

    #[inline(always)]
    pub fn set_error(&mut self, message: &str) {
        self.error_message = Some(message.to_string());
    }

    #[inline(always)]
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    #[inline(always)]
    pub fn has_valid_chat(&self) -> bool {
        !self.chats.is_empty() && self.current_chat < self.chats.len()
    }

    #[inline(always)]
    pub fn toggle_message_truncation(&mut self, msg_idx: usize) {
        if !self.truncated_messages.insert(msg_idx) {
            self.truncated_messages.remove(&msg_idx);
        }
        self.need_rebuild_cache = true;
    }

    pub fn add_user_message(&mut self, content: String) {
        if let Some(chat) = self.chats.get_mut(self.current_chat) {
            let msg_idx = chat.messages.len();
            chat.messages.push(Message::new(Role::User, &content));
            self.truncated_messages.insert(msg_idx);
            self.code_blocks
                .extend(self.parse_code_blocks_helper(msg_idx, &content));
            self.need_rebuild_cache = true;
        }
    }
}
