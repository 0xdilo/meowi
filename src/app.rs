use ratatui::text::Line;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::{self, Receiver, Sender};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub content: String,
    pub language: Option<String>, // e.g., "rust", "python"
    pub start_line: usize,        // Line index of the opening fence
    pub end_line: usize,          // Line index of the closing fence
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Chat,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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
    pub truncated_messages: HashSet<usize>, // Messages that are truncated (user messages by default)
    pub need_rebuild_cache: bool,
    pub line_to_message: Vec<(usize, bool)>,
    pub focus: Focus,
    pub stream_tasks: HashMap<String, StreamTask>,
    pub error_message: Option<String>,
    pub code_blocks: Vec<(usize, CodeBlock)>, // (message_idx, CodeBlock)
}

impl<'a> App<'a> {
    pub fn new() -> Self {
        let providers = vec![
            Provider {
                name: "OpenAI".to_string(),
                api_key: "".to_string(),
                models: vec!["gpt-4o".to_string(), "gpt-3.5-turbo".to_string()],
                enabled_models: vec!["gpt-4o".to_string()],
                expanded: false,
            },
            Provider {
                name: "Anthropic".to_string(),
                api_key: "".to_string(),
                models: vec!["claude-3-opus".to_string()],
                enabled_models: vec![],
                expanded: false,
            },
            Provider {
                name: "Grok".to_string(),
                api_key: "".to_string(),
                models: vec!["grok-3".to_string()],
                enabled_models: vec!["grok-3".to_string()],
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
        };
        if app.chats.is_empty() {
            app.create_new_chat();
        }
        app
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        if !self.sidebar_visible {
            self.focus = Focus::Chat;
        } else {
            self.focus = Focus::Sidebar;
        }
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
        self.truncated_messages.clear(); // Reset for new chat
    }

    pub fn current_model_name(&self) -> &str {
        &self.current_model
    }

    pub fn enabled_models_flat(&self) -> Vec<(String, String)> {
        let mut list = vec![];
        for p in &self.providers {
            for m in &p.enabled_models {
                list.push((p.name.clone(), m.clone()));
            }
        }
        list
    }

    pub fn jump_to_last_message(&mut self) {
        let mut total_lines = 0;
        for (lines, is_truncated) in &self.line_cache {
            total_lines += lines.len();
            if *is_truncated {
                total_lines += 1; // For ellipsis line
            }
            total_lines += 1; // For separator
        }
        if total_lines > 0 {
            self.cursor_line = total_lines - 1;
        } else {
            self.cursor_line = 0;
        }
        self.chat_scroll = u16::MAX; // Trigger scroll to bottom
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

        // First, process all stream tasks and collect chunks with their chat IDs and message indices
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
                            chat.messages.push(Message {
                                role: "assistant".to_string(),
                                content: chunk.clone(),
                            });
                            processed_chunks.push((msg_idx, chunk.clone()));
                        }
                    } else {
                        chat.messages.push(Message {
                            role: "assistant".to_string(),
                            content: chunk.clone(),
                        });
                        processed_chunks.push((msg_idx, chunk.clone()));
                    }
                    self.need_rebuild_cache = true;
                    content_updated = true;
                    // Ensure new assistant message is not truncated
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

        // Now that mutable borrow is dropped, parse code blocks
        for (msg_idx, content) in processed_chunks {
            new_code_blocks.extend(self.parse_code_blocks_helper(msg_idx, &content));
        }

        // Finally, update code_blocks
        self.code_blocks.extend(new_code_blocks);
        if content_updated {
            self.jump_to_last_message(); // Autoscroll to bottom
        }
    }

    fn parse_code_blocks_helper(&self, msg_idx: usize, content: &str) -> Vec<(usize, CodeBlock)> {
        let opening_re = Regex::new(r"^```(\w+)?\s*$").unwrap();
        let closing_re = Regex::new(r"^```\s*$").unwrap();
        let mut blocks = Vec::new();
        let mut idx = 0;
        let lines: Vec<&str> = content.lines().collect();
        while idx < lines.len() {
            if let Some(caps) = opening_re.captures(lines[idx]) {
                // start of a code block
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
                while idx < lines.len() && !closing_re.is_match(lines[idx]) {
                    code_lines.push(lines[idx]);
                    idx += 1;
                }
                // idx now at closing fence or at end
                let end_fence = idx;
                let content_str = code_lines.join("\n");
                blocks.push((
                    msg_idx,
                    CodeBlock {
                        content: content_str,
                        language: lang,
                        // for UI we want start_line = first code‐line index
                        start_line: start_fence + 1,
                        end_line: end_fence - 1,
                    },
                ));
            }
            idx += 1;
        }
        blocks
    }

    pub fn set_error(&mut self, message: &str) {
        self.error_message = Some(message.to_string());
    }

    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    pub fn has_valid_chat(&self) -> bool {
        !self.chats.is_empty() && self.current_chat < self.chats.len()
    }

    pub fn toggle_message_truncation(&mut self, msg_idx: usize) {
        if self.truncated_messages.contains(&msg_idx) {
            self.truncated_messages.remove(&msg_idx);
        } else {
            self.truncated_messages.insert(msg_idx);
        }
        self.need_rebuild_cache = true;
    }

    pub fn add_user_message(&mut self, content: String) {
        if let Some(chat) = self.chats.get_mut(self.current_chat) {
            let msg_idx = chat.messages.len();
            chat.messages.push(Message {
                role: "user".to_string(),
                content: content.clone(),
            });
            self.truncated_messages.insert(msg_idx); // Truncate user message by default
            self.code_blocks
                .extend(self.parse_code_blocks_helper(msg_idx, &content));
            self.need_rebuild_cache = true;
        }
    }
}
