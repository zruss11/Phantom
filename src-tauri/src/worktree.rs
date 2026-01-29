//! Git worktree operations for isolated branch-based development.
//!
//! This module provides utilities for creating and managing git worktrees,
//! enabling agents to work in isolated branches without affecting the main working tree.

use crate::utils::resolve_gh_binary;
use rand::seq::SliceRandom;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const GIT_COMMAND_TIMEOUT_SECS: u64 = 20;

/// Sanitize branch name for git and filesystem safety.
///
/// Normalizes the branch name to:
/// - Lowercase, kebab-case
/// - Only alphanumeric characters and dashes (plus one slash for prefix)
/// - Ensures valid prefix (feat/, fix/, chore/, etc.)
/// - Removes trailing dashes and slashes
/// - Strips URL patterns and other invalid sequences
pub fn sanitize_branch_name(branch: &str) -> String {
    const ALLOWED_PREFIXES: &[&str] = &[
        "feat/",
        "fix/",
        "chore/",
        "test/",
        "docs/",
        "refactor/",
        "perf/",
    ];

    // First, strip common URL patterns that would create invalid branch names
    let cleaned = branch
        .replace("https://", "")
        .replace("http://", "")
        .replace("www.", "")
        .replace(".com", "")
        .replace(".org", "")
        .replace(".io", "")
        .replace(".dev", "");

    let lower = cleaned.trim().to_lowercase();

    // Check if input starts with a valid prefix
    let (prefix, rest) = ALLOWED_PREFIXES
        .iter()
        .find_map(|p| lower.strip_prefix(p).map(|rest| (*p, rest)))
        .unwrap_or(("feat/", lower.as_str()));

    // Sanitize the rest: only alphanumeric and dashes, all slashes become dashes
    let mut result = String::from(prefix);
    let mut last_dash = false;

    for ch in rest.chars() {
        if ch.is_ascii_alphanumeric() {
            last_dash = false;
            result.push(ch);
        } else if ch == '-' || ch == '/' || ch.is_whitespace() || ch == '_' {
            // All separators (including slashes) become dashes
            if !last_dash && result.len() > prefix.len() {
                last_dash = true;
                result.push('-');
            }
        }
    }

    // Trim trailing dashes
    while result.ends_with('-') {
        result.pop();
    }

    // Truncate very long branch names (git has a 255 byte limit on refs)
    if result.len() > 60 {
        result.truncate(60);
        // Clean up any trailing dash from truncation
        while result.ends_with('-') {
            result.pop();
        }
    }

    // Handle edge case: if only prefix remains (no content), use fallback
    if result == prefix.trim_end_matches('/') || result == prefix {
        return "feat/task".to_string();
    }

    result
}

/// Run a git command and return its stdout output.
pub async fn run_git_command(repo_path: &PathBuf, args: &[&str]) -> Result<String, String> {
    let output = run_git_command_raw(repo_path, args).await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("Git command failed: {}", stderr))
    }
}

async fn run_git_command_bytes(repo_path: &PathBuf, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = run_git_command_raw(repo_path, args).await?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        if detail.is_empty() {
            Err("Git command failed.".to_string())
        } else {
            Err(detail.to_string())
        }
    }
}

async fn run_git_diff_bytes(repo_path: &PathBuf, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = run_git_command_raw(repo_path, args).await?;
    if output.status.success() || output.status.code() == Some(1) {
        return Ok(output.stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if detail.is_empty() {
        Err("Git diff failed.".to_string())
    } else {
        Err(detail.to_string())
    }
}

pub async fn diff_stats(repo_path: &PathBuf) -> Result<(u64, u64, u64), String> {
    // Use a single numstat against the repo base to avoid double-counting partially staged files.
    // Note: git diff may return exit code 1 when there are differences.
    async fn numstat_against(repo_path: &PathBuf, base: &str) -> Result<String, String> {
        let args = ["diff", "--numstat", "--no-color", base];
        let out = run_git_diff_bytes(repo_path, &args).await?;
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    fn parse_numstat(text: &str) -> (u64, u64, u64) {
        let mut additions: u64 = 0;
        let mut deletions: u64 = 0;
        let mut files: u64 = 0;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            files += 1;
            let add = parts[0];
            let del = parts[1];
            // Binary changes show '-' in numstat.
            if add != "-" {
                if let Ok(n) = add.parse::<u64>() {
                    additions += n;
                }
            }
            if del != "-" {
                if let Ok(n) = del.parse::<u64>() {
                    deletions += n;
                }
            }
        }

        (additions, deletions, files)
    }

    let base = if run_git_command(repo_path, &["rev-parse", "--verify", "HEAD"]).await.is_ok() {
        "HEAD".to_string()
    } else {
        git_empty_tree_hash(repo_path).await.unwrap_or_default()
    };

    if base.is_empty() {
        return Ok((0, 0, 0));
    }

    let combined = numstat_against(repo_path, &base).await.unwrap_or_default();
    let (additions, deletions, files) = parse_numstat(&combined);

    Ok((additions, deletions, files))
}

async fn run_git_command_raw(
    repo_path: &PathBuf,
    args: &[&str],
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(repo_path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");

    tokio::time::timeout(Duration::from_secs(GIT_COMMAND_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| {
            format!(
                "Git command timed out after {}s: git {}",
                GIT_COMMAND_TIMEOUT_SECS,
                args.join(" ")
            )
        })?
        .map_err(|e| format!("Failed to execute git: {}", e))
}

async fn run_git_command_raw_with_input(
    repo_path: &PathBuf,
    args: &[&str],
    input: &[u8],
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(repo_path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to execute git: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input)
            .await
            .map_err(|e| format!("Failed to write to git stdin: {}", e))?;
        stdin
            .shutdown()
            .await
            .map_err(|e| format!("Failed to close git stdin: {}", e))?;
    }

    tokio::time::timeout(Duration::from_secs(GIT_COMMAND_TIMEOUT_SECS), child.wait_with_output())
        .await
        .map_err(|_| {
            format!(
                "Git command timed out after {}s: git {}",
                GIT_COMMAND_TIMEOUT_SECS,
                args.join(" ")
            )
        })?
        .map_err(|e| format!("Failed to execute git: {}", e))
}

async fn git_empty_tree_hash(repo_path: &PathBuf) -> Result<String, String> {
    let output = run_git_command_raw_with_input(
        repo_path,
        &["hash-object", "-t", "tree", "--stdin"],
        b"",
    )
    .await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        if detail.is_empty() {
            Err("Git command failed.".to_string())
        } else {
            Err(detail.to_string())
        }
    }
}

/// Check if a branch exists locally.
pub async fn branch_exists(repo_path: &PathBuf, branch: &str) -> Result<bool, String> {
    let result = run_git_command(
        repo_path,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ],
    )
    .await;
    match result {
        Ok(_) => Ok(true),
        Err(_) => Ok(false), // show-ref returns error if branch doesn't exist
    }
}

/// Check if a remote branch exists.
pub async fn remote_branch_exists(
    repo_path: &PathBuf,
    remote: &str,
    branch: &str,
) -> Result<bool, String> {
    let output = run_git_command(
        repo_path,
        &[
            "ls-remote",
            "--heads",
            remote,
            &format!("refs/heads/{}", branch),
        ],
    )
    .await?;
    Ok(!output.trim().is_empty())
}

/// Generate a unique branch name by appending -2, -3, etc. if the branch exists.
pub async fn unique_branch_name(repo_path: &PathBuf, desired: &str) -> Result<String, String> {
    let sanitized = sanitize_branch_name(desired);

    // Check if base name is available
    if !branch_exists(repo_path, &sanitized).await? {
        return Ok(sanitized);
    }

    // Try with numeric suffix
    for i in 2..100 {
        let candidate = format!("{}-{}", sanitized, i);
        if !branch_exists(repo_path, &candidate).await? {
            return Ok(candidate);
        }
    }

    Err("Could not find unique branch name after 100 attempts".to_string())
}

/// Generate a unique temporary branch name by appending -v2, -v3, etc. if the branch exists.
/// Unlike `unique_branch_name`, this does not sanitize or require branch prefixes,
/// making it suitable for animal-name branches used during worktree creation.
pub async fn unique_temp_branch_name(repo_path: &PathBuf, base: &str) -> Result<String, String> {
    // Check if base name is available
    if !branch_exists(repo_path, base).await? {
        return Ok(base.to_string());
    }

    // Try with -v2, -v3, etc. suffix (matches workspace naming convention)
    for i in 2..1000 {
        let candidate = format!("{}-v{}", base, i);
        if !branch_exists(repo_path, &candidate).await? {
            return Ok(candidate);
        }
    }

    Err("Could not find unique temporary branch name after 1000 attempts".to_string())
}

/// Get a list of local branch names.
pub async fn list_branches(repo_path: &PathBuf) -> Result<Vec<String>, String> {
    let output = run_git_command(repo_path, &["branch", "--format=%(refname:short)"]).await?;

    let branches: Vec<String> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();

    Ok(branches)
}

/// Get the current branch name.
pub async fn current_branch(repo_path: &PathBuf) -> Result<String, String> {
    run_git_command(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await
}

/// Sanitize a string into a workspace-friendly slug (kebab-case, no slashes).
pub fn sanitize_workspace_slug(input: &str) -> String {
    let mut result = String::new();
    let mut last_dash = false;

    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            last_dash = false;
            result.push(ch);
        } else if ch == '-' || ch.is_whitespace() || ch == '_' {
            if !last_dash && !result.is_empty() {
                last_dash = true;
                result.push('-');
            }
        }
    }

    while result.ends_with('-') {
        result.pop();
    }

    if result.is_empty() {
        "task".to_string()
    } else {
        result
    }
}

/// Generate a readable slug for a repository path.
pub fn repo_slug(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
    sanitize_workspace_slug(name)
}

/// Resolve the Phantom Harness workspace root: ~/phantom-harness/workspaces
pub fn workspace_root_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    let root = home.join("phantom-harness").join("workspaces");
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("Failed to create workspace root: {}", e))?;
    Ok(root)
}

const ANIMAL_NAMES_RAW: &str = include_str!("../data/animals_az_15.txt");

/// Build a workspace path under ~/phantom-harness/workspaces/<repo>/<animal(-vN)>
pub fn build_workspace_path(repo_slug: &str) -> Result<PathBuf, String> {
    let root = workspace_root_dir()?;
    let repo_dir = root.join(repo_slug);
    std::fs::create_dir_all(&repo_dir)
        .map_err(|e| format!("Failed to create repo workspace dir: {}", e))?;

    let base_name = random_animal_name()?;
    let unique = unique_workspace_name(&repo_dir, base_name)?;
    Ok(repo_dir.join(unique))
}

fn random_animal_name() -> Result<&'static str, String> {
    let names: Vec<&'static str> = ANIMAL_NAMES_RAW
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    let mut rng = rand::thread_rng();
    names
        .choose(&mut rng)
        .copied()
        .ok_or_else(|| "No animal names available for workspace naming".to_string())
}

fn unique_workspace_name(repo_dir: &Path, base: &str) -> Result<String, String> {
    let base = sanitize_workspace_slug(base);
    if base.is_empty() {
        return Err("Workspace name is empty after sanitization".to_string());
    }

    let initial = repo_dir.join(&base);
    if !initial.exists() {
        return Ok(base);
    }

    for i in 1u64.. {
        let candidate = format!("{}-v{}", base, i);
        if !repo_dir.join(&candidate).exists() {
            return Ok(candidate);
        }
    }

    Err("Failed to find an available workspace name".to_string())
}

/// Remove a workspace directory safely (only under ~/phantom/workspaces).
pub fn remove_workspace_dir(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let root = workspace_root_dir()?;
    let canonical_root = std::fs::canonicalize(&root)
        .map_err(|e| format!("Failed to resolve workspace root: {}", e))?;
    let canonical_path = std::fs::canonicalize(path)
        .map_err(|e| format!("Failed to resolve workspace path: {}", e))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(format!(
            "Refusing to remove non-workspace path: {}",
            canonical_path.display()
        ));
    }
    if canonical_path == canonical_root {
        return Err("Refusing to remove workspace root".to_string());
    }
    std::fs::remove_dir_all(&canonical_path)
        .map_err(|e| format!("Failed to remove workspace: {}", e))?;
    Ok(())
}

/// Sync workspace contents from source directory to destination.
/// Attempts rsync first, then falls back to a filesystem-based sync.
pub async fn sync_workspace_from_source(src: &Path, dest: &Path) -> Result<(), String> {
    if let Err(err) = run_rsync(src, dest).await {
        eprintln!("[worktree] rsync failed, falling back: {}", err);
        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || sync_with_fs(&src, &dest))
            .await
            .map_err(|e| format!("Workspace sync task failed: {}", e))??;
    }
    Ok(())
}

/// Apply uncommitted changes from source repo to a newly created worktree.
/// This preserves tracked + untracked modifications without copying the whole tree.
pub async fn apply_uncommitted_changes(
    source_repo: &Path,
    worktree_path: &Path,
) -> Result<(), String> {
    let repo = source_repo.to_path_buf();
    let worktree = worktree_path.to_path_buf();

    let staged = run_git_diff_bytes(&repo, &["diff", "--binary", "--no-color", "--cached"]).await?;
    let unstaged = run_git_diff_bytes(&repo, &["diff", "--binary", "--no-color"]).await?;

    let mut patch: Vec<u8> = Vec::new();
    patch.extend_from_slice(&staged);
    patch.extend_from_slice(&unstaged);

    let untracked =
        run_git_command_bytes(&repo, &["ls-files", "--others", "--exclude-standard", "-z"]).await?;
    for raw_path in untracked.split(|byte| *byte == 0) {
        if raw_path.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(raw_path).to_string();
        let diff = run_git_diff_bytes(
            &repo,
            &[
                "diff",
                "--binary",
                "--no-color",
                "--no-index",
                "--",
                null_device_path(),
                &path,
            ],
        )
        .await?;
        patch.extend_from_slice(&diff);
    }

    if patch.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(());
    }

    let mut child = Command::new("git")
        .args(["apply", "--3way", "--whitespace=nowarn", "-"])
        .current_dir(&worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run git apply: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&patch)
            .await
            .map_err(|e| format!("Failed to write git apply input: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Failed to run git apply: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if detail.is_empty() {
        return Err("Git apply failed.".to_string());
    }

    if detail.contains("Applied patch to") {
        if detail.contains("with conflicts") {
            return Err(
                "Applied with conflicts. Resolve conflicts in the worktree before proceeding."
                    .to_string(),
            );
        }
        return Err(
            "Patch applied partially. Resolve changes in the worktree before proceeding."
                .to_string(),
        );
    }

    Err(detail.to_string())
}

fn null_device_path() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

async fn run_rsync(src: &Path, dest: &Path) -> Result<(), String> {
    let mut src_path = src.to_string_lossy().to_string();
    if !src_path.ends_with('/') {
        src_path.push('/');
    }
    let dest_path = dest.to_string_lossy().to_string();

    let output = Command::new("rsync")
        .args(["-a", "--delete", "--exclude=.git", &src_path, &dest_path])
        .output()
        .await
        .map_err(|e| format!("Failed to execute rsync: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("rsync failed: {}", stderr))
    }
}

fn sync_with_fs(src: &Path, dest: &Path) -> Result<(), String> {
    if !dest.exists() {
        std::fs::create_dir_all(dest)
            .map_err(|e| format!("Failed to create workspace directory: {}", e))?;
    }

    remove_extraneous(dest, src)?;
    copy_recursive(src, dest)?;
    Ok(())
}

fn remove_extraneous(dest: &Path, src: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(dest).map_err(|e| format!("Failed to read workspace: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read workspace entry: {}", e))?;
        let name = entry.file_name();
        if name.to_string_lossy() == ".git" {
            continue;
        }
        let dest_path = entry.path();
        let rel = dest_path
            .strip_prefix(dest)
            .map_err(|e| format!("Failed to compute workspace relative path: {}", e))?;
        let src_path = src.join(rel);

        let entry_type = entry
            .file_type()
            .map_err(|e| format!("Failed to read entry type: {}", e))?;

        if !src_path.exists() {
            if entry_type.is_dir() {
                std::fs::remove_dir_all(&dest_path)
                    .map_err(|e| format!("Failed to remove directory: {}", e))?;
            } else {
                std::fs::remove_file(&dest_path)
                    .map_err(|e| format!("Failed to remove file: {}", e))?;
            }
            continue;
        }

        if entry_type.is_dir() && !src_path.is_dir() {
            std::fs::remove_dir_all(&dest_path)
                .map_err(|e| format!("Failed to remove directory: {}", e))?;
        } else if entry_type.is_file() && src_path.is_dir() {
            std::fs::remove_file(&dest_path)
                .map_err(|e| format!("Failed to remove file: {}", e))?;
        } else if entry_type.is_dir() {
            remove_extraneous(&dest_path, &src_path)?;
        }
    }
    Ok(())
}

fn copy_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    for entry in
        std::fs::read_dir(src).map_err(|e| format!("Failed to read source directory: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read source entry: {}", e))?;
        let name = entry.file_name();
        if name.to_string_lossy() == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dest_path = dest.join(&name);
        let entry_type = entry
            .file_type()
            .map_err(|e| format!("Failed to read entry type: {}", e))?;

        if entry_type.is_dir() {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
            copy_recursive(&src_path, &dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent directory: {}", e))?;
            }
            std::fs::copy(&src_path, &dest_path)
                .map_err(|e| format!("Failed to copy file: {}", e))?;
        }
    }
    Ok(())
}

/// Create a worktree with a new branch based on a specified base branch.
///
/// # Arguments
/// * `repo_path` - Path to the main repository
/// * `worktree_path` - Path where the worktree will be created
/// * `branch` - Name of the new branch to create
/// * `base_branch` - Branch to base the new branch on
pub async fn create_worktree(
    repo_path: &PathBuf,
    worktree_path: &PathBuf,
    branch: &str,
    base_branch: &str,
) -> Result<(), String> {
    // Ensure parent directory exists
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create worktree parent directory: {}", e))?;
    }

    // Create worktree with new branch based on base_branch
    // git worktree add -b <new-branch> <path> <base-branch>
    let result = run_git_command(
        repo_path,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            &worktree_path.to_string_lossy(),
            base_branch,
        ],
    )
    .await;

    if let Err(err) = result {
        let err_lower = err.to_lowercase();
        let looks_like_lfs = err_lower.contains("git-lfs")
            || err_lower.contains("git lfs")
            || err_lower.contains("git lfs");
        if looks_like_lfs {
            // Retry without hooks so missing git-lfs hooks don't block worktree creation.
            run_git_command(
                repo_path,
                &[
                    "-c",
                    "core.hooksPath=/dev/null",
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    &worktree_path.to_string_lossy(),
                    base_branch,
                ],
            )
            .await?;
        } else {
            return Err(err);
        }
    }

    Ok(())
}

/// Rename a branch in a worktree.
///
/// This is used to rename from the temporary animal name branch to the
/// LLM-generated branch name after worktree creation.
///
/// # Arguments
/// * `worktree_path` - Path to the worktree (where the rename will be executed)
/// * `old_branch` - Current branch name (e.g., "aardvark")
/// * `new_branch` - Desired branch name (e.g., "fix/login-bug")
pub async fn rename_worktree_branch(
    worktree_path: &PathBuf,
    old_branch: &str,
    new_branch: &str,
) -> Result<(), String> {
    // Verify the worktree still exists before attempting rename
    if !worktree_path.exists() {
        return Err("Worktree path no longer exists".to_string());
    }

    // Use checkout -B + branch -d instead of branch -m to avoid the temp file
    // race condition that occurs when multiple worktrees rename concurrently.
    // The issue: `git branch -m` uses a shared .tmp-renamed-log file in the
    // main repo's .git directory, which fails with concurrent operations.

    println!(
        "[worktree] Renaming branch {} -> {} in {:?}",
        old_branch, new_branch, worktree_path
    );

    // Step 1: Create the new branch at the same commit (checkout -B forces creation)
    run_git_command(worktree_path, &["checkout", "-B", new_branch]).await?;

    // Step 2: Delete the old branch (now safe since we're on the new one)
    // Use -D to force delete in case the branch isn't fully merged
    if let Err(e) = run_git_command(worktree_path, &["branch", "-D", old_branch]).await {
        // Non-fatal - the rename worked, old branch just wasn't cleaned up
        eprintln!(
            "[worktree] Warning: couldn't delete old branch {}: {}",
            old_branch, e
        );
    }

    println!(
        "[worktree] Successfully renamed branch {} -> {}",
        old_branch, new_branch
    );

    Ok(())
}

/// Check if a worktree has uncommitted changes (staged, unstaged, or untracked).
/// Returns Ok(true) if there are uncommitted changes, Ok(false) if clean.
pub async fn has_uncommitted_changes(worktree_path: &PathBuf) -> Result<bool, String> {
    let output = run_git_command(worktree_path, &["status", "--porcelain"]).await?;
    Ok(!output.trim().is_empty())
}

/// Remove a worktree.
///
/// This removes both the worktree directory and its git metadata.
pub async fn remove_worktree(repo_path: &PathBuf, worktree_path: &PathBuf) -> Result<(), String> {
    // Remove the worktree using git
    run_git_command(
        repo_path,
        &[
            "worktree",
            "remove",
            &worktree_path.to_string_lossy(),
            "--force",
        ],
    )
    .await?;

    Ok(())
}

/// List all worktrees for a repository.
pub async fn list_worktrees(repo_path: &PathBuf) -> Result<Vec<String>, String> {
    let output = run_git_command(repo_path, &["worktree", "list", "--porcelain"]).await?;

    let worktrees: Vec<String> = output
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .map(|line| line.strip_prefix("worktree ").unwrap_or("").to_string())
        .collect();

    Ok(worktrees)
}

/// Push a branch to the remote and create a PR using gh CLI.
///
/// Returns the PR URL on success.
pub async fn create_pull_request(
    worktree_path: &PathBuf,
    branch: &str,
    _base_branch: &str,
) -> Result<String, String> {
    // Push branch to origin
    run_git_command(worktree_path, &["push", "-u", "origin", branch]).await?;

    // Create PR using gh CLI with --fill to auto-generate title/body from commits
    let gh_path = resolve_gh_binary()?;
    let output = Command::new(&gh_path)
        .args(["pr", "create", "--fill", "--head", branch])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(url)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("gh pr create failed: {}", stderr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_branch_name_basic() {
        assert_eq!(
            sanitize_branch_name("Add user notifications"),
            "feat/add-user-notifications"
        );
        // "fix login bug" without slash becomes "feat/fix-login-bug" (no prefix match)
        assert_eq!(sanitize_branch_name("fix login bug"), "feat/fix-login-bug");
        assert_eq!(
            sanitize_branch_name("feat/some-feature"),
            "feat/some-feature"
        );
    }

    #[test]
    fn test_sanitize_branch_name_preserves_prefix() {
        assert_eq!(sanitize_branch_name("fix/auth-issue"), "fix/auth-issue");
        assert_eq!(
            sanitize_branch_name("chore/update-deps"),
            "chore/update-deps"
        );
        assert_eq!(
            sanitize_branch_name("docs/readme-update"),
            "docs/readme-update"
        );
    }

    #[test]
    fn test_sanitize_branch_name_handles_special_chars() {
        assert_eq!(
            sanitize_branch_name("Add user's notifications!"),
            "feat/add-users-notifications"
        );
        assert_eq!(sanitize_branch_name("fix_login_bug"), "feat/fix-login-bug");
        assert_eq!(
            sanitize_branch_name("  spaces around  "),
            "feat/spaces-around"
        );
    }

    #[test]
    fn test_sanitize_branch_name_removes_trailing() {
        assert_eq!(sanitize_branch_name("some-branch-"), "feat/some-branch");
        assert_eq!(sanitize_branch_name("feat/feature/"), "feat/feature");
    }

    #[test]
    fn test_sanitize_branch_name_handles_urls() {
        // URLs should be stripped and converted to valid branch names
        assert_eq!(
            sanitize_branch_name("https://github.com/anthropics/skills how can we add"),
            "feat/github-anthropics-skills-how-can-we-add"
        );
        assert_eq!(
            sanitize_branch_name("fix http://example.com/issue"),
            "feat/fix-example-issue"
        );
        // Multiple slashes should be collapsed
        assert_eq!(sanitize_branch_name("feat/foo/bar/baz"), "feat/foo-bar-baz");
    }

    #[test]
    fn test_sanitize_branch_name_truncates_long_names() {
        let long_input = "this is a very long branch name that should be truncated to a reasonable length for git compatibility";
        let result = sanitize_branch_name(long_input);
        assert!(result.len() <= 65); // feat/ (5) + 60 = 65
        assert!(result.starts_with("feat/"));
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn test_sanitize_workspace_slug() {
        assert_eq!(
            sanitize_workspace_slug("Add Workspace Home View"),
            "add-workspace-home-view"
        );
        assert_eq!(sanitize_workspace_slug("  spaces  "), "spaces");
        assert_eq!(sanitize_workspace_slug(""), "task");
    }
}
