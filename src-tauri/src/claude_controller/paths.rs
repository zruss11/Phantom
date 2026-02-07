use std::path::PathBuf;

pub fn claude_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude"))
}

pub fn teams_dir() -> Option<PathBuf> {
    claude_dir().map(|d| d.join("teams"))
}

pub fn team_dir(team_name: &str) -> Option<PathBuf> {
    teams_dir().map(|d| d.join(team_name))
}

pub fn team_config_path(team_name: &str) -> Option<PathBuf> {
    team_dir(team_name).map(|d| d.join("config.json"))
}

pub fn inboxes_dir(team_name: &str) -> Option<PathBuf> {
    team_dir(team_name).map(|d| d.join("inboxes"))
}

pub fn inbox_path(team_name: &str, agent_name: &str) -> Option<PathBuf> {
    inboxes_dir(team_name).map(|d| d.join(format!("{agent_name}.json")))
}

