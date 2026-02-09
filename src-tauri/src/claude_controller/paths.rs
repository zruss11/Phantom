use std::path::PathBuf;

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn claude_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude"))
}

pub fn teams_dir() -> Option<PathBuf> {
    claude_dir().map(|d| d.join("teams"))
}

pub fn team_dir(team_name: &str) -> Option<PathBuf> {
    if !safe_name(team_name) {
        return None;
    }
    teams_dir().map(|d| d.join(team_name))
}

pub fn team_config_path(team_name: &str) -> Option<PathBuf> {
    team_dir(team_name).map(|d| d.join("config.json"))
}

pub fn team_config_lock_path(team_name: &str) -> Option<PathBuf> {
    team_config_path(team_name).map(|p| {
        let mut lock = p;
        lock.set_extension("json.lock");
        lock
    })
}

pub fn inboxes_dir(team_name: &str) -> Option<PathBuf> {
    team_dir(team_name).map(|d| d.join("inboxes"))
}

pub fn inbox_path(team_name: &str, agent_name: &str) -> Option<PathBuf> {
    if !safe_name(agent_name) {
        return None;
    }
    inboxes_dir(team_name).map(|d| d.join(format!("{agent_name}.json")))
}

pub fn inbox_lock_path(team_name: &str, agent_name: &str) -> Option<PathBuf> {
    inbox_path(team_name, agent_name).map(|p| {
        let mut lock = p;
        lock.set_extension("json.lock");
        lock
    })
}
