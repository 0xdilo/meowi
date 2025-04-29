use crate::app::Chat;
use directories::ProjectDirs;
use serde_json;
use std::{fs, path::PathBuf};

pub fn get_history_path() -> PathBuf {
    let proj_dirs = ProjectDirs::from("com", "yourname", "meowi").unwrap();
    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir).unwrap();
    data_dir.join("history.json")
}

pub fn load_history() -> Vec<Chat> {
    let path = get_history_path();
    if path.exists() {
        let content = fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    }
}

pub fn save_history(chats: &[Chat]) {
    let path = get_history_path();
    let content = serde_json::to_string_pretty(chats).unwrap();
    fs::write(path, content).unwrap();
}
