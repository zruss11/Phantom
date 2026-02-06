use super::types::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// Check if gh CLI is authenticated and get username
pub fn check_gh_cli_auth() -> GhCliAuthStatus {
    let output = Command::new("gh")
        .args(["auth", "status", "--hostname", "github.com"])
        .output();

    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{}{}", stdout, stderr);

            if out.status.success() || combined.contains("Logged in to") {
                let username = combined
                    .lines()
                    .find(|l| l.contains("Logged in to"))
                    .and_then(|l| l.split("account ").nth(1))
                    .map(|s| s.split_whitespace().next().unwrap_or("").to_string());

                GhCliAuthStatus {
                    available: true,
                    username,
                    error: None,
                }
            } else {
                GhCliAuthStatus {
                    available: false,
                    username: None,
                    error: Some("gh CLI not authenticated".to_string()),
                }
            }
        }
        Err(e) => GhCliAuthStatus {
            available: false,
            username: None,
            error: Some(format!("gh CLI not found: {}", e)),
        },
    }
}

/// Fetch issues for a repo using gh CLI
pub async fn fetch_issues_gh_cli(repo: &str) -> Result<Vec<GithubIssue>, String> {
    let output = tokio::process::Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--assignee",
            "@me",
            "--json",
            "id,number,title,state,url,labels,assignees,createdAt,updatedAt",
            "--limit",
            "50",
        ])
        .output()
        .await
        .map_err(|e| format!("gh CLI error: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh issue list failed: {}", stderr));
    }

    #[derive(Deserialize)]
    struct GhIssue {
        id: u64,
        number: u32,
        title: String,
        state: String,
        url: String,
        labels: Vec<GhLabel>,
        assignees: Vec<GhAssignee>,
        #[serde(rename = "createdAt")]
        created_at: String,
        #[serde(rename = "updatedAt")]
        updated_at: String,
    }

    #[derive(Deserialize)]
    struct GhLabel {
        name: String,
        #[serde(default)]
        color: String,
    }

    #[derive(Deserialize)]
    struct GhAssignee {
        login: String,
    }

    let issues: Vec<GhIssue> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse gh output: {}", e))?;

    Ok(issues
        .into_iter()
        .map(|i| GithubIssue {
            id: i.id,
            number: i.number,
            title: i.title,
            state: i.state,
            html_url: i.url,
            labels: i
                .labels
                .into_iter()
                .map(|l| GithubLabel {
                    name: l.name,
                    color: l.color,
                })
                .collect(),
            assignee: i.assignees.first().map(|a| a.login.clone()),
            created_at: i.created_at,
            updated_at: i.updated_at,
            repo: repo.to_string(),
        })
        .collect())
}

/// Fetch issues using GitHub REST API with token
pub async fn fetch_issues_rest(
    client: &Client,
    token: &str,
    repo: &str,
) -> Result<Vec<GithubIssue>, String> {
    #[derive(Deserialize)]
    struct ApiIssue {
        id: u64,
        number: u32,
        title: String,
        state: String,
        html_url: String,
        labels: Vec<ApiLabel>,
        assignee: Option<ApiUser>,
        created_at: String,
        updated_at: String,
    }

    #[derive(Deserialize)]
    struct ApiLabel {
        name: String,
        color: String,
    }

    #[derive(Deserialize)]
    struct ApiUser {
        login: String,
    }

    let url = format!(
        "https://api.github.com/repos/{}/issues?state=open&per_page=50",
        repo
    );
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "Phantom-Harness")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned {}", response.status()));
    }

    let issues: Vec<ApiIssue> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    Ok(issues
        .into_iter()
        .filter(|i| i.assignee.is_some())
        .map(|i| GithubIssue {
            id: i.id,
            number: i.number,
            title: i.title,
            state: i.state,
            html_url: i.html_url,
            labels: i
                .labels
                .into_iter()
                .map(|l| GithubLabel {
                    name: l.name,
                    color: l.color,
                })
                .collect(),
            assignee: i.assignee.map(|a| a.login),
            created_at: i.created_at,
            updated_at: i.updated_at,
            repo: repo.to_string(),
        })
        .collect())
}

/// Fetch workflow runs using gh CLI
pub async fn fetch_workflows_gh_cli(repo: &str) -> Result<Vec<GithubWorkflow>, String> {
    let output = tokio::process::Command::new("gh")
        .args([
            "run",
            "list",
            "--repo",
            repo,
            "--json",
            "databaseId,displayTitle,status,conclusion,headBranch,number,url,createdAt",
            "--limit",
            "20",
        ])
        .output()
        .await
        .map_err(|e| format!("gh CLI error: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh run list failed: {}", stderr));
    }

    #[derive(Deserialize)]
    struct GhRun {
        #[serde(rename = "databaseId")]
        database_id: u64,
        #[serde(rename = "displayTitle")]
        display_title: String,
        status: String,
        conclusion: Option<String>,
        #[serde(rename = "headBranch")]
        head_branch: String,
        number: u32,
        url: String,
        #[serde(rename = "createdAt")]
        created_at: String,
    }

    let runs: Vec<GhRun> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse gh output: {}", e))?;

    Ok(runs
        .into_iter()
        .map(|r| GithubWorkflow {
            id: r.database_id,
            name: r.display_title,
            status: r.status,
            conclusion: r.conclusion,
            branch: r.head_branch,
            run_number: r.number,
            html_url: r.url,
            created_at: r.created_at,
            duration_seconds: None,
            repo: repo.to_string(),
        })
        .collect())
}

/// Fetch workflow runs using GitHub REST API
pub async fn fetch_workflows_rest(
    client: &Client,
    token: &str,
    repo: &str,
) -> Result<Vec<GithubWorkflow>, String> {
    #[derive(Deserialize)]
    struct ApiResponse {
        workflow_runs: Vec<ApiRun>,
    }

    #[derive(Deserialize)]
    struct ApiRun {
        id: u64,
        name: String,
        status: String,
        conclusion: Option<String>,
        head_branch: String,
        run_number: u32,
        html_url: String,
        created_at: String,
    }

    let url = format!(
        "https://api.github.com/repos/{}/actions/runs?per_page=20",
        repo
    );
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "Phantom-Harness")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned {}", response.status()));
    }

    let api_response: ApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    Ok(api_response
        .workflow_runs
        .into_iter()
        .map(|r| GithubWorkflow {
            id: r.id,
            name: r.name,
            status: r.status,
            conclusion: r.conclusion,
            branch: r.head_branch,
            run_number: r.run_number,
            html_url: r.html_url,
            created_at: r.created_at,
            duration_seconds: None,
            repo: repo.to_string(),
        })
        .collect())
}

/// Re-run a failed workflow
pub async fn rerun_workflow_gh_cli(repo: &str, run_id: u64) -> Result<(), String> {
    let output = tokio::process::Command::new("gh")
        .args(["run", "rerun", "--repo", repo, &run_id.to_string()])
        .output()
        .await
        .map_err(|e| format!("gh CLI error: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh run rerun failed: {}", stderr));
    }

    Ok(())
}

/// Result of extracting GitHub repo info from a folder
#[derive(Serialize, Clone)]
pub struct GitRepoInfo {
    pub owner: String,
    pub repo: String,
    pub full_name: String,
    pub path: String,
}

/// Parse a GitHub remote URL to extract owner and repo
fn parse_github_remote(remote_url: &str) -> Option<(String, String)> {
    let trimmed = remote_url.trim().trim_end_matches(".git");

    let rest = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("git://github.com/") {
        rest
    } else {
        return None;
    };

    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();

    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some((owner.to_string(), repo.to_string()))
}

/// Get GitHub repo info from a local folder path
pub fn get_github_repo_from_path(folder_path: &str) -> Result<GitRepoInfo, String> {
    let path = Path::new(folder_path);

    if !path.exists() {
        return Err("Path does not exist".to_string());
    }

    if !path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    // Check if it's a git repository
    let git_dir = path.join(".git");
    if !git_dir.exists() {
        return Err("Not a git repository (no .git directory)".to_string());
    }

    // Get the remote origin URL
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run git command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("No origin remote found: {}", stderr));
    }

    let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Parse the remote URL
    let (owner, repo) = parse_github_remote(&remote_url)
        .ok_or_else(|| format!("Not a GitHub repository. Remote URL: {}", remote_url))?;

    Ok(GitRepoInfo {
        full_name: format!("{}/{}", owner, repo),
        owner,
        repo,
        path: folder_path.to_string(),
    })
}
