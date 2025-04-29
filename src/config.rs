use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub enabled_models: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub providers: Vec<ProviderConfig>,
    pub keybindings: KeyBindings,
    pub copy_code_blocks: Vec<String>,
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
pub copy_code_blocks: Vec<String>, // e.g., ["c", "C", "x", "X"]
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            providers: vec![
                ProviderConfig {
                    name: "OpenAI".to_string(),
                    api_key: "".to_string(),
                    enabled_models: vec!["gpt-4o".to_string()],
                },
                ProviderConfig {
                    name: "Anthropic".to_string(),
                    api_key: "".to_string(),
                    enabled_models: vec!["claude-3-opus".to_string()],
                },
                ProviderConfig {
                    name: "Grok".to_string(),
                    api_key: "".to_string(),
                    enabled_models: vec!["grok-3".to_string()],
                },
            ],
            keybindings: KeyBindings {
                new_chat: "n".to_string(),
                toggle_sidebar: "s".to_string(),
                switch_focus: "Tab".to_string(),
                lock_focus: "l".to_string(),
                delete_chat: "d".to_string(),
                copy_code: "y".to_string(),
                copy_code_blocks: vec!["c".to_string(), "C".to_string(), "x".to_string(), "X".to_string()],
                insert_mode: "i".to_string(),
                exit_insert_mode: "Esc".to_string(),
                command_mode: ":".to_string(),
                open_settings: "o".to_string(),
            },
            copy_code_blocks: vec!["c".to_string(), "C".to_string(), "x".to_string(), "X".to_string()],
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
        let content = fs::read_to_string(path).unwrap();
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
