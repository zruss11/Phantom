use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::io::AsyncWriteExt;

pub fn docker_available() -> bool {
    if which::which("docker").is_err() {
        return false;
    }
    std::process::Command::new("docker")
        .arg("info")
        .arg("--format")
        .arg("{{.ServerVersion}}")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub async fn ensure_container_image_available<F>(image: &str, mut status: F) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    if image.trim().is_empty() {
        return Err("Container image is empty.".to_string());
    }
    let output = Command::new("docker")
        .arg("image")
        .arg("inspect")
        .arg(image)
        .output()
        .await
        .map_err(|err| format!("Failed to inspect container image: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    status("Downloading container...");
    let pull_output = Command::new("docker")
        .arg("pull")
        .arg(image)
        .output()
        .await
        .map_err(|err| format!("Failed to pull container image: {err}"))?;
    if pull_output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&pull_output.stderr).trim().to_string();
    let lower = stderr.to_lowercase();
    if lower.contains("denied") || lower.contains("unauthorized") {
        status("Signing into GitHub...");
        if try_login_ghcr_with_gh().await? {
            status("Downloading container...");
            let retry_output = Command::new("docker")
                .arg("pull")
                .arg(image)
                .output()
                .await
                .map_err(|err| format!("Failed to pull container image: {err}"))?;
            if retry_output.status.success() {
                return Ok(());
            }
        }
        return Err(format!(
            "Container image access denied for {}. Please sign in to GitHub to continue.",
            image
        ));
    }
    if lower.contains("not found") || lower.contains("manifest unknown") {
        return Err(format!(
            "Container image {} not found. Please check the image name or disable container isolation.",
            image
        ));
    }
    Err(format!(
        "Container image {} unavailable: {}",
        image,
        if stderr.is_empty() { "unknown error" } else { &stderr }
    ))
}

async fn try_login_ghcr_with_gh() -> Result<bool, String> {
    if which::which("gh").is_err() {
        return Ok(false);
    }
    let user_output = Command::new("gh")
        .args(["api", "user", "-q", ".login"])
        .output()
        .await
        .map_err(|err| format!("Failed to read GitHub user: {err}"))?;
    if !user_output.status.success() {
        return Ok(false);
    }
    let username = String::from_utf8_lossy(&user_output.stdout).trim().to_string();
    if username.is_empty() {
        return Ok(false);
    }

    let token_output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        .map_err(|err| format!("Failed to read GitHub token: {err}"))?;
    if !token_output.status.success() {
        return Ok(false);
    }
    let token = String::from_utf8_lossy(&token_output.stdout).trim().to_string();
    if token.is_empty() {
        return Ok(false);
    }

    let mut cmd = Command::new("docker");
    cmd.args(["login", "ghcr.io", "-u", &username, "--password-stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to start docker login: {err}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(token.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|err| format!("Failed to complete docker login: {err}"))?;
    Ok(output.status.success())
}

#[derive(Debug, Clone)]
pub struct ContainerMount {
    pub host_path: PathBuf,
    pub container_path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct ContainerRuntimeConfig {
    pub image: String,
    pub workdir: String,
    pub mounts: Vec<ContainerMount>,
    pub env: Vec<(String, String)>,
    pub runtime: Option<String>,
    pub network_mode: Option<String>,
    pub memory: Option<String>,
    pub cpus: Option<f64>,
    pub pids_limit: Option<i64>,
    pub user: Option<String>,
    pub read_only: bool,
    pub tmpfs: Vec<String>,
    pub extra_hosts: Vec<String>,
    pub name: Option<String>,
}

#[derive(Debug)]
pub struct ContainerRuntime {
    id: String,
    config: ContainerRuntimeConfig,
}

impl ContainerRuntime {
    pub async fn start(config: ContainerRuntimeConfig) -> Result<Self> {
        let mut cmd = Command::new("docker");
        cmd.arg("create");

        if let Some(name) = config.name.as_ref() {
            cmd.arg("--name").arg(name);
        }
        cmd.arg("--label").arg("phantom-harness=true");
        if let Some(runtime) = config.runtime.as_ref() {
            if !runtime.trim().is_empty() {
                cmd.arg("--runtime").arg(runtime);
            }
        }
        if let Some(network) = config.network_mode.as_ref() {
            if !network.trim().is_empty() {
                cmd.arg("--network").arg(network);
            }
        }
        if let Some(memory) = config.memory.as_ref() {
            if !memory.trim().is_empty() {
                cmd.arg("--memory").arg(memory);
            }
        }
        if let Some(cpus) = config.cpus {
            if cpus > 0.0 {
                cmd.arg("--cpus").arg(format!("{cpus}"));
            }
        }
        if let Some(pids) = config.pids_limit {
            if pids > 0 {
                cmd.arg("--pids-limit").arg(pids.to_string());
            }
        }
        if let Some(user) = config.user.as_ref() {
            if !user.trim().is_empty() {
                cmd.arg("--user").arg(user);
            }
        }
        if config.read_only {
            cmd.arg("--read-only");
        }
        if !config.workdir.trim().is_empty() {
            cmd.arg("--workdir").arg(&config.workdir);
        }
        for tmpfs in &config.tmpfs {
            if !tmpfs.trim().is_empty() {
                cmd.arg("--tmpfs").arg(tmpfs);
            }
        }
        for host in &config.extra_hosts {
            if !host.trim().is_empty() {
                cmd.arg("--add-host").arg(host);
            }
        }
        for mount in &config.mounts {
            let mut spec =
                format!("{}:{}", mount.host_path.to_string_lossy(), mount.container_path);
            if mount.read_only {
                spec.push_str(":ro");
            }
            cmd.arg("-v").arg(spec);
        }

        // Keep container alive; we'll exec agent commands for each prompt.
        cmd.arg(&config.image).arg("sleep").arg("infinity");

        let output = cmd
            .output()
            .await
            .context("failed to run docker create")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("docker create failed: {}", stderr.trim()));
        }
        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if id.is_empty() {
            return Err(anyhow::anyhow!("docker create returned empty container id"));
        }

        let status = Command::new("docker")
            .arg("start")
            .arg(&id)
            .status()
            .await
            .context("failed to run docker start")?;
        if !status.success() {
            return Err(anyhow::anyhow!("docker start failed"));
        }

        Ok(Self { id, config })
    }

    pub fn exec_command(&self, program: &str, args: &[String]) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("exec").arg("-i");
        if let Some(user) = self.config.user.as_ref() {
            if !user.trim().is_empty() {
                cmd.arg("--user").arg(user);
            }
        }
        if !self.config.workdir.trim().is_empty() {
            cmd.arg("--workdir").arg(&self.config.workdir);
        }
        for (key, value) in &self.config.env {
            cmd.arg("-e").arg(format!("{}={}", key, value));
        }
        cmd.arg(&self.id);
        cmd.arg(program);
        cmd.args(args);
        cmd
    }

    pub fn map_host_path(&self, host_path: &Path) -> Option<PathBuf> {
        for mount in &self.config.mounts {
            if host_path.starts_with(&mount.host_path) {
                let relative = host_path.strip_prefix(&mount.host_path).ok()?;
                let mut container_path = PathBuf::from(&mount.container_path);
                if !relative.as_os_str().is_empty() {
                    container_path.push(relative);
                }
                return Some(container_path);
            }
        }
        None
    }

    pub fn shared_temp_dir(&self) -> Option<PathBuf> {
        for mount in &self.config.mounts {
            if mount.container_path == "/workspace" {
                let temp = mount.host_path.join(".phantom").join("tmp");
                return Some(temp);
            }
        }
        None
    }

    pub async fn stop(&self, timeout_secs: u64) -> Result<()> {
        let status = Command::new("docker")
            .arg("stop")
            .arg("-t")
            .arg(timeout_secs.to_string())
            .arg(&self.id)
            .status()
            .await
            .context("failed to run docker stop")?;
        if !status.success() {
            return Err(anyhow::anyhow!("docker stop failed"));
        }
        Ok(())
    }

    pub async fn remove(&self, force: bool) -> Result<()> {
        let mut cmd = Command::new("docker");
        cmd.arg("rm");
        if force {
            cmd.arg("-f");
        }
        cmd.arg(&self.id);
        let status = cmd.status().await.context("failed to run docker rm")?;
        if !status.success() {
            return Err(anyhow::anyhow!("docker rm failed"));
        }
        Ok(())
    }
}
