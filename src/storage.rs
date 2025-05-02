use crate::app::Chat;
use directories::ProjectDirs;
use serde_json;
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::PathBuf,
};

pub fn get_history_path() -> Result<PathBuf, std::io::Error> {
    let proj_dirs = ProjectDirs::from("com", "yourname", "meowi").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "ProjectDirs not found")
    })?;
    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;
    Ok(data_dir.join("history.json"))
}

pub fn load_history() -> Vec<Chat> {
    match get_history_path()
        .and_then(|path| File::open(path).map(BufReader::new))
        .and_then(|reader| {
            serde_json::from_reader(reader)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }) {
        Ok(chats) => chats,
        Err(_) => Vec::new(),
    }
}

pub fn save_history(chats: &[Chat]) {
    if let Ok(path) = get_history_path() {
        let tmp_path = path.with_extension("json.tmp");
        if let Ok(file) = File::create(&tmp_path) {
            let writer = BufWriter::new(file);
            if serde_json::to_writer_pretty(writer, chats).is_ok() {
                let _ = fs::rename(&tmp_path, &path);
            } else {
                let _ = fs::remove_file(&tmp_path);
            }
        }
    }
}
