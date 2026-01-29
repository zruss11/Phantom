use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn ensure_local_image<F>(
    agent_id: &str,
    container_dir: &Path,
    mut status: F,
) -> Result<String, String>
where
    F: FnMut(&str) + Send,
{
    if !container_dir.exists() {
        return Err(
            "Container build assets are missing. Please reinstall Phantom or disable container isolation."
                .to_string(),
        );
    }

    let base_tag = "phantom-harness/agent-base:local".to_string();
    if !docker_image_exists(&base_tag).await? {
        status("Building base container (one-time setup)...");
        let base_file = container_dir.join("Dockerfile.base");
        build_image(&base_file, &base_tag, container_dir, &[]).await?;
    }

    let image_id = agent_image_id(agent_id);
    let agent_tag = format!("phantom-harness/agent-{}:local", image_id);
    let legacy_tag = format!("phantom-harness/agent-{}:local", sanitize_agent_id(agent_id));
    if !docker_image_exists(&agent_tag).await? {
        if agent_tag != legacy_tag && docker_image_exists(&legacy_tag).await? {
            return Ok(legacy_tag);
        }
        let label = agent_display_name(agent_id);
        status(&format!("Building container for {label}..."));
        let dockerfile = dockerfile_for_agent(agent_id, container_dir)?;
        let build_args = [format!("BASE_IMAGE={}", base_tag)];
        build_image(&dockerfile, &agent_tag, container_dir, &build_args).await?;
    }

    Ok(agent_tag)
}

fn sanitize_agent_id(agent_id: &str) -> String {
    agent_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn agent_image_id(agent_id: &str) -> String {
    match agent_id {
        "claude-code" => "claude".to_string(),
        other => sanitize_agent_id(other),
    }
}

fn agent_display_name(agent_id: &str) -> String {
    match agent_id {
        "codex" => "Codex".to_string(),
        "claude-code" => "Claude Code".to_string(),
        _ => agent_id.to_string(),
    }
}

async fn docker_image_exists(tag: &str) -> Result<bool, String> {
    let status = Command::new("docker")
        .arg("image")
        .arg("inspect")
        .arg(tag)
        .status()
        .await
        .map_err(|err| format!("Failed to inspect container image: {err}"))?;
    Ok(status.success())
}

fn dockerfile_for_agent(agent_id: &str, container_dir: &Path) -> Result<PathBuf, String> {
    let filename = match agent_id {
        "codex" => "Dockerfile.codex",
        "claude-code" => "Dockerfile.claude",
        other => {
            return Err(format!(
                "Container isolation is not yet available for agent: {}. Disable container isolation or use a supported agent (Codex or Claude Code).",
                other
            ))
        }
    };
    Ok(container_dir.join(filename))
}

async fn build_image(
    dockerfile: &Path,
    tag: &str,
    context_dir: &Path,
    build_args: &[String],
) -> Result<(), String> {
    if !dockerfile.exists() {
        return Err("Container Dockerfile missing.".to_string());
    }
    let mut cmd = Command::new("docker");
    cmd.arg("build")
        .arg("-f")
        .arg(dockerfile)
        .arg("-t")
        .arg(tag);
    for build_arg in build_args {
        cmd.arg("--build-arg").arg(build_arg);
    }
    cmd.arg(context_dir);
    let output = cmd
        .output()
        .await
        .map_err(|err| format!("Failed to build container: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = stderr
        .lines()
        .last()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .or_else(|| stdout.lines().last().map(|line| line.trim().to_string()))
        .unwrap_or_else(|| "unknown error".to_string());
    Err(format!("Container build failed: {message}"))
}
