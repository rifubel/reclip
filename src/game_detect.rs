use std::fs;
use std::path::PathBuf;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct GameInfo {
    pub name: String,
    pub pid: u32,
}

struct SteamGame {
    name: String,
    install_dir: String,
}

fn acf_value(contents: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    for line in contents.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&needle) {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix('"') {
                if let Some(end) = value.find('"') {
                    return Some(value[..end].to_string());
                }
            }
        }
    }
    None
}

fn steam_games() -> Vec<SteamGame> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = PathBuf::from(format!("{home}/.local/share/Steam/steamapps"));
    let mut games = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return games,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
            continue;
        }
        if let Ok(contents) = fs::read_to_string(entry.path()) {
            if let (Some(name), Some(install_dir)) =
                (acf_value(&contents, "name"), acf_value(&contents, "installdir"))
            {
                let lower = name.to_lowercase();
                let is_tool = lower.contains("steam linux runtime")
                    || lower.contains("proton")
                    || lower.contains("steamworks common redistributables")
                    || lower.contains("steamworks");
                if is_tool {
                    continue;
                }
                games.push(SteamGame { name, install_dir });
            }
        }
    }
    games
}

fn proc_identity(pid: u32) -> Option<String> {
    let exe = fs::read_link(format!("/proc/{pid}/exe"))
        .ok()?
        .to_string_lossy()
        .to_string();
    let cmdline = fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .replace('\0', " ");
    Some(format!("{exe} {cmdline}"))
}

pub fn detect(config: &Config) -> Option<GameInfo> {
    let games = steam_games();
    let mut candidates: Vec<(String, String)> = Vec::new();
    for game in &games {
        candidates.push((
            game.name.clone(),
            format!("/steamapps/common/{}/", game.install_dir),
        ));
    }
    for path in &config.manual_games {
        let name = PathBuf::from(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        candidates.push((name, path.clone()));
    }
    if candidates.is_empty() {
        return None;
    }

    let pid_entries = fs::read_dir("/proc").ok()?;
    for entry in pid_entries.flatten() {
        let pid = match entry.file_name().to_string_lossy().parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => continue,
        };
        if pid == std::process::id() {
            continue;
        }
        let identity = match proc_identity(pid) {
            Some(identity) => identity,
            None => continue,
        };
        for (name, marker) in &candidates {
            if identity.contains(marker.as_str()) {
                return Some(GameInfo { name: name.clone(), pid });
            }
        }
    }
    None
}
