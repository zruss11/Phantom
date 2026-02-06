//! OpenClaw install + gateway management helpers.
//!
//! This is intentionally defensive: OpenClaw CLI output and JSON schemas may evolve.

use crate::utils::resolve_command_path;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandResult {
    pub(crate) ok: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolProbe {
    pub(crate) installed: bool,
    pub(crate) path: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NodeProbe {
    pub(crate) installed: bool,
    pub(crate) path: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) major: Option<u32>,
    pub(crate) ok: bool,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GatewayProbe {
    pub(crate) installed: Option<bool>,
    pub(crate) running: Option<bool>,
    pub(crate) port: Option<u16>,
    pub(crate) status_message: Option<String>,
    pub(crate) raw: Option<Value>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenClawProbe {
    pub(crate) os: String,
    pub(crate) brew: ToolProbe,
    pub(crate) node: NodeProbe,
    pub(crate) npm: ToolProbe,
    pub(crate) openclaw: ToolProbe,
    pub(crate) gateway: Option<GatewayProbe>,
}

fn shorten(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = s[..max].to_string();
    out.push_str("\n...(truncated)");
    out
}

async fn run_cmd_with_timeout(
    exe: &PathBuf,
    args: &[&str],
    timeout_secs: u64,
) -> Result<CommandResult, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", exe.display(), e))?;

    let output = timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| format!("Command timed out after {}s: {} {:?}", timeout_secs, exe.display(), args))?
        .map_err(|e| format!("Failed to wait for {}: {}", exe.display(), e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(CommandResult {
        ok: output.status.success(),
        exit_code: output.status.code(),
        stdout: shorten(stdout.trim_end(), 20_000),
        stderr: shorten(stderr.trim_end(), 20_000),
    })
}

async fn run_cmd_checked(
    exe: &PathBuf,
    args: &[&str],
    timeout_secs: u64,
    label: &str,
) -> Result<CommandResult, String> {
    let result = run_cmd_with_timeout(exe, args, timeout_secs).await?;
    if result.ok {
        Ok(result)
    } else {
        Err(format!(
            "{} failed (exit {:?}).\nstdout:\n{}\n\nstderr:\n{}",
            label, result.exit_code, result.stdout, result.stderr
        ))
    }
}

fn parse_node_major(version: &str) -> Option<u32> {
    // `node -v` usually returns `v22.2.0`
    let v = version.trim();
    let v = v.strip_prefix('v').unwrap_or(v);
    v.split('.').next()?.parse::<u32>().ok()
}

fn resolve_brew() -> Option<PathBuf> {
    // Prefer canonical macOS locations, then PATH/default search paths.
    for p in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    resolve_command_path("brew")
}

pub(crate) fn resolve_openclaw() -> Option<PathBuf> {
    resolve_command_path("openclaw")
}

fn tool_probe_from_version(exe_name: &str, exe: Option<PathBuf>, version: Result<String, String>) -> ToolProbe {
    match (exe, version) {
        (Some(path), Ok(v)) => ToolProbe {
            installed: true,
            path: Some(path.to_string_lossy().to_string()),
            version: Some(v.trim().to_string()),
            error: None,
        },
        (Some(path), Err(err)) => ToolProbe {
            installed: true,
            path: Some(path.to_string_lossy().to_string()),
            version: None,
            error: Some(format!("{} version check failed: {}", exe_name, err)),
        },
        (None, _) => ToolProbe {
            installed: false,
            path: None,
            version: None,
            error: None,
        },
    }
}

pub(crate) async fn probe() -> OpenClawProbe {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
    .to_string();

    let brew_path = if cfg!(target_os = "macos") {
        resolve_brew()
    } else {
        None
    };
    let brew = if cfg!(target_os = "macos") {
        let version = if let Some(path) = brew_path.as_ref() {
            match run_cmd_with_timeout(path, &["--version"], 10).await {
                Ok(r) if r.ok => Ok(r.stdout.lines().next().unwrap_or("").to_string()),
                Ok(r) => Err(r.stderr),
                Err(e) => Err(e),
            }
        } else {
            Err("brew not found".to_string())
        };
        tool_probe_from_version("brew", brew_path.clone(), version)
    } else {
        ToolProbe {
            installed: false,
            path: None,
            version: None,
            error: None,
        }
    };

    let node_path = resolve_command_path("node");
    let node_installed = node_path.is_some();
    let node_path_str = node_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let (node_version, node_major) = if let Some(path) = node_path.as_ref() {
        match run_cmd_with_timeout(path, &["-v"], 10).await {
            Ok(r) if r.ok => {
                let v = r.stdout.trim().to_string();
                let major = parse_node_major(&v);
                (Some(v), major)
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };
    let node_ok = node_major.map(|m| m >= 22).unwrap_or(false);
    let node = NodeProbe {
        installed: node_installed,
        path: node_path_str,
        version: node_version.clone(),
        major: node_major,
        ok: node_ok,
        error: if node_installed && node_version.is_none() {
            Some("node version check failed".to_string())
        } else {
            None
        },
    };

    let npm_path = resolve_command_path("npm");
    let npm = {
        let version = if let Some(path) = npm_path.as_ref() {
            match run_cmd_with_timeout(path, &["-v"], 10).await {
                Ok(r) if r.ok => Ok(r.stdout.trim().to_string()),
                Ok(r) => Err(r.stderr),
                Err(e) => Err(e),
            }
        } else {
            Err("npm not found".to_string())
        };
        tool_probe_from_version("npm", npm_path.clone(), version)
    };

    let openclaw_path = resolve_openclaw();
    let openclaw = {
        let version = if let Some(path) = openclaw_path.as_ref() {
            match run_cmd_with_timeout(path, &["--version"], 15).await {
                Ok(r) if r.ok => Ok(r.stdout.trim().to_string()),
                Ok(r) => Err(r.stderr),
                Err(e) => Err(e),
            }
        } else {
            Err("openclaw not found".to_string())
        };
        tool_probe_from_version("openclaw", openclaw_path.clone(), version)
    };

    let gateway = if let Some(path) = openclaw_path.as_ref() {
        match run_cmd_with_timeout(path, &["gateway", "status", "--json"], 20).await {
            Ok(r) if r.ok => {
                let json: Option<Value> = serde_json::from_str(r.stdout.trim()).ok();
                let parsed = json
                    .as_ref()
                    .map(parse_gateway_status)
                    .unwrap_or_else(|| GatewayProbe {
                        installed: None,
                        running: None,
                        port: None,
                        status_message: Some("Gateway status returned non-JSON output".to_string()),
                        raw: None,
                        error: None,
                    });
                Some(GatewayProbe {
                    raw: json,
                    ..parsed
                })
            }
            Ok(r) => Some(GatewayProbe {
                installed: Some(false),
                running: None,
                port: None,
                status_message: None,
                raw: None,
                error: Some(if !r.stderr.trim().is_empty() {
                    r.stderr
                } else {
                    r.stdout
                }),
            }),
            Err(e) => Some(GatewayProbe {
                installed: Some(false),
                running: None,
                port: None,
                status_message: None,
                raw: None,
                error: Some(e),
            }),
        }
    } else {
        None
    };

    OpenClawProbe {
        os,
        brew,
        node,
        npm,
        openclaw,
        gateway,
    }
}

pub(crate) fn extract_dashboard_url(stdout: &str) -> Option<String> {
    // Look for something like: http://127.0.0.1:18789/?token=...
    let re = regex::Regex::new(r"http://127\.0\.0\.1:\d+/\S*").ok()?;
    re.find(stdout).map(|m| m.as_str().to_string())
}

pub(crate) fn parse_gateway_status(json: &Value) -> GatewayProbe {
    let mut installed: Option<bool> = None;
    let mut running: Option<bool> = None;
    let mut port: Option<u16> = None;
    let mut status_message: Option<String> = None;

    if let Some(obj) = json.as_object() {
        // Common patterns: { running: true, port: 18789, installed: true }
        if let Some(v) = obj.get("installed").and_then(|v| v.as_bool()) {
            installed = Some(v);
        }
        if let Some(v) = obj.get("running").and_then(|v| v.as_bool()) {
            running = Some(v);
        }
        if let Some(v) = obj.get("port").and_then(|v| v.as_u64()) {
            port = u16::try_from(v).ok();
        }
        if let Some(v) = obj.get("status").and_then(|v| v.as_str()) {
            status_message = Some(v.to_string());
            if running.is_none() {
                running = Some(v.eq_ignore_ascii_case("running"));
            }
        }
        // Sometimes nested: { gateway: { port: 18789, ... } }
        if port.is_none() {
            if let Some(gw) = obj.get("gateway") {
                if let Some(v) = gw.get("port").and_then(|v| v.as_u64()) {
                    port = u16::try_from(v).ok();
                }
            }
        }
    }

    GatewayProbe {
        installed,
        running,
        port,
        status_message,
        raw: None,
        error: None,
    }
}

pub(crate) async fn install_brew() -> Result<CommandResult, String> {
    if !cfg!(target_os = "macos") {
        return Err("Homebrew install is only supported on macOS.".to_string());
    }

    // Official installer. NONINTERACTIVE prevents prompts, but this can still fail if sudo is needed.
    let bash = PathBuf::from("/bin/bash");
    if !bash.is_file() {
        return Err("/bin/bash not found".to_string());
    }

    let script =
        "NONINTERACTIVE=1 /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"";
    run_cmd_checked(&bash, &["-lc", script], 900, "Homebrew install").await
}

pub(crate) async fn install_node() -> Result<CommandResult, String> {
    if !cfg!(target_os = "macos") {
        return Err("Node install via Homebrew is only supported on macOS.".to_string());
    }

    let brew = resolve_brew().ok_or_else(|| "brew not found (install Homebrew first)".to_string())?;

    // If node is installed but too old, upgrade. Otherwise install.
    let node_probe = probe().await.node;
    let needs_upgrade = node_probe.installed && !node_probe.ok;

    let args = if needs_upgrade {
        vec!["upgrade", "node"]
    } else {
        vec!["install", "node"]
    };

    let result = run_cmd_checked(&brew, &args, 600, "brew node install/upgrade").await?;

    // Re-check node version after install/upgrade.
    let node_probe_after = probe().await.node;
    if !node_probe_after.ok {
        return Err(format!(
            "Node installed/updated but version is still not >= 22 (found {:?}).",
            node_probe_after.version
        ));
    }

    Ok(result)
}

pub(crate) async fn install_openclaw_cli() -> Result<CommandResult, String> {
    let npm = resolve_command_path("npm").ok_or_else(|| "npm not found (install Node first)".to_string())?;

    // Use user-writable prefix to avoid permissions issues.
    let home = dirs::home_dir().ok_or_else(|| "Could not resolve home directory".to_string())?;
    let prefix = home.join(".npm-global");
    let prefix_str = prefix.to_string_lossy().to_string();

    let _ = run_cmd_checked(
        &npm,
        &["config", "set", "prefix", &prefix_str],
        60,
        "npm config set prefix",
    )
    .await?;

    run_cmd_checked(
        &npm,
        &["install", "-g", "openclaw@latest", "--no-fund", "--no-audit"],
        900,
        "npm install -g openclaw@latest",
    )
    .await
}

pub(crate) async fn gateway_install() -> Result<CommandResult, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    run_cmd_checked(&openclaw, &["gateway", "install"], 180, "openclaw gateway install").await
}

pub(crate) async fn gateway_start() -> Result<CommandResult, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    run_cmd_checked(&openclaw, &["gateway", "start"], 180, "openclaw gateway start").await
}

pub(crate) async fn gateway_stop() -> Result<CommandResult, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    run_cmd_checked(&openclaw, &["gateway", "stop"], 180, "openclaw gateway stop").await
}

pub(crate) async fn gateway_restart() -> Result<CommandResult, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    run_cmd_checked(&openclaw, &["gateway", "restart"], 240, "openclaw gateway restart").await
}

pub(crate) async fn gateway_uninstall() -> Result<CommandResult, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    run_cmd_checked(&openclaw, &["gateway", "uninstall"], 240, "openclaw gateway uninstall").await
}

pub(crate) async fn run_doctor_fix() -> Result<CommandResult, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    run_cmd_checked(&openclaw, &["doctor", "--fix"], 600, "openclaw doctor --fix").await
}

pub(crate) async fn dashboard_url() -> Result<String, String> {
    let openclaw = resolve_openclaw().ok_or_else(|| "openclaw not found (install OpenClaw first)".to_string())?;
    let result = run_cmd_checked(&openclaw, &["dashboard", "--no-open"], 60, "openclaw dashboard").await?;
    extract_dashboard_url(&result.stdout)
        .or_else(|| extract_dashboard_url(&result.stderr))
        .ok_or_else(|| format!("Could not find dashboard URL in output.\nstdout:\n{}\n\nstderr:\n{}", result.stdout, result.stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dashboard_url() {
        let stdout = "Starting...\nOpen: http://127.0.0.1:18789/?token=abc123\nDone\n";
        let url = extract_dashboard_url(stdout).unwrap();
        assert_eq!(url, "http://127.0.0.1:18789/?token=abc123");
    }

    #[test]
    fn test_parse_gateway_status_minimal() {
        let json = serde_json::json!({
            "installed": true,
            "running": false,
            "port": 18789,
            "status": "stopped"
        });
        let p = parse_gateway_status(&json);
        assert_eq!(p.installed, Some(true));
        assert_eq!(p.running, Some(false));
        assert_eq!(p.port, Some(18789));
        assert_eq!(p.status_message, Some("stopped".to_string()));
    }
}
