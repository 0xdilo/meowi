use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub name: Box<str>,
    pub content: Box<str>,
    pub active: bool,
}

impl Prompt {
    #[inline]
    pub fn new<N: Into<Box<str>>, C: Into<Box<str>>>(name: N, content: C, active: bool) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            active,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub enabled_models: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CustomModel {
    Derived {
        provider: String,
        model: String,
    },
    Standalone {
        name: String,
        endpoint: String,
        model: String,
        api_key: Option<String>,
        use_key_from: Option<String>,
    },
}

impl CustomModel {
    pub fn name(&self) -> &str {
        match self {
            CustomModel::Derived { model, .. } => model,
            CustomModel::Standalone { name, .. } => name,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeyBindings {
    pub new_chat: String,
    pub toggle_sidebar: String,
    pub switch_focus: String,
    pub lock_focus: String,
    pub delete_chat: String,
    pub copy_code: String,
    pub insert_mode: String,
    pub exit_insert_mode: String,
    pub command_mode: String,
    pub open_settings: String,
    pub copy_code_blocks: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub providers: Vec<ProviderConfig>,
    pub keybindings: KeyBindings,
    pub copy_code_blocks: Vec<String>,
    pub custom_models: Vec<CustomModel>,
    pub prompts: Vec<Prompt>,
}

const OPENAI_MODELS: &[&str] = &["gpt-4o", "gpt-4-turbo", "gpt-3.5-turbo"];
const ANTHROPIC_MODELS: &[&str] = &[
    "claude-3-7-sonnet-latest",
    "claude-3-5-haiku-latest",
    "claude-3-5-sonnet-latest",
    "claude-3-opus",
    "claude-3-sonnet",
];
const GROK_MODELS: &[&str] = &["grok-3-latest", "grok-3-mini-beta"];
const COPY_CODE_BLOCKS: &[&str] = &["c", "C", "x", "X"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            providers: vec![
                ProviderConfig {
                    name: "OpenAI".into(),
                    api_key: String::new(),
                    enabled_models: OPENAI_MODELS.iter().map(|&s| s.into()).collect(),
                },
                ProviderConfig {
                    name: "Anthropic".into(),
                    api_key: String::new(),
                    enabled_models: ANTHROPIC_MODELS.iter().map(|&s| s.into()).collect(),
                },
                ProviderConfig {
                    name: "Grok".into(),
                    api_key: String::new(),
                    enabled_models: GROK_MODELS.iter().map(|&s| s.into()).collect(),
                },
            ],
            keybindings: KeyBindings {
                new_chat: "n".into(),
                toggle_sidebar: "s".into(),
                switch_focus: "Tab".into(),
                lock_focus: "l".into(),
                delete_chat: "d".into(),
                copy_code: "y".into(),
                copy_code_blocks: COPY_CODE_BLOCKS.iter().map(|&s| s.into()).collect(),
                insert_mode: "i".into(),
                exit_insert_mode: "Esc".into(),
                command_mode: ":".into(),
                open_settings: "o".into(),
            },
            copy_code_blocks: COPY_CODE_BLOCKS.iter().map(|&s| s.into()).collect(),
            custom_models: Vec::new(),
            prompts: vec![Prompt::new("Default", "You are a helpful assistant.", true)],
        }
    }
}

pub fn get_config_path() -> PathBuf {
    let proj_dirs = ProjectDirs::from("com", "yourname", "meowi").unwrap();
    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir).unwrap();
    config_dir.join("config.toml")
}

pub fn load_or_create_config() -> Settings {
    let path = get_config_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap();
        toml::from_str(&content).unwrap_or_else(|_| {
            let default = Settings::default();
            save_config(&default);
            default
        })
    } else {
        let default = Settings::default();
        save_config(&default);
        default
    }
}

pub fn save_config(settings: &Settings) {
    let path = get_config_path();
    fs::write(&path, toml::to_string_pretty(settings).unwrap()).unwrap();
}

pub fn openai_models() -> Vec<String> {
    OPENAI_MODELS.iter().map(|&s| s.into()).collect()
}
pub fn anthropic_models() -> Vec<String> {
    ANTHROPIC_MODELS.iter().map(|&s| s.into()).collect()
}
pub fn grok_models() -> Vec<String> {
    GROK_MODELS.iter().map(|&s| s.into()).collect()
}
