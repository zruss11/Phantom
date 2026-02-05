use super::types::*;
use reqwest::Client;
use serde::Deserialize;

const SENTRY_API_URL: &str = "https://sentry.io/api/0";

/// Fetch organizations the user has access to
pub async fn fetch_organizations(client: &Client, token: &str) -> Result<Vec<SentryOrganization>, String> {
    #[derive(Deserialize)]
    struct ApiOrg {
        slug: String,
        name: String,
    }

    let url = format!("{}/organizations/", SENTRY_API_URL);
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("Sentry API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Sentry API returned {}", response.status()));
    }

    let orgs: Vec<ApiOrg> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Sentry response: {}", e))?;

    Ok(orgs
        .into_iter()
        .map(|o| SentryOrganization {
            slug: o.slug,
            name: o.name,
        })
        .collect())
}

/// Fetch projects in an organization
pub async fn fetch_projects(client: &Client, token: &str, org: &str) -> Result<Vec<SentryProject>, String> {
    #[derive(Deserialize)]
    struct ApiProject {
        slug: String,
        name: String,
        id: String,
    }

    let url = format!("{}/organizations/{}/projects/", SENTRY_API_URL, org);
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("Sentry API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Sentry API returned {}", response.status()));
    }

    let projects: Vec<ApiProject> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Sentry response: {}", e))?;

    Ok(projects
        .into_iter()
        .map(|p| SentryProject {
            slug: p.slug,
            name: p.name,
            id: p.id,
        })
        .collect())
}

/// Fetch unresolved issues for specified projects
pub async fn fetch_errors(
    client: &Client,
    token: &str,
    org: &str,
    project_slugs: &[String],
) -> Result<Vec<SentryError>, String> {
    #[derive(Deserialize)]
    struct ApiIssue {
        id: String,
        title: String,
        culprit: String,
        #[serde(rename = "shortId")]
        short_id: String,
        count: String, // Sentry returns as string
        #[serde(rename = "userCount")]
        user_count: u64,
        #[serde(rename = "firstSeen")]
        first_seen: String,
        #[serde(rename = "lastSeen")]
        last_seen: String,
        level: String,
        status: String,
        permalink: String,
        project: ApiProjectRef,
        metadata: ApiMetadata,
    }

    #[derive(Deserialize)]
    struct ApiProjectRef {
        slug: String,
    }

    #[derive(Deserialize)]
    struct ApiMetadata {
        filename: Option<String>,
        function: Option<String>,
        #[serde(rename = "type")]
        error_type: Option<String>,
        value: Option<String>,
    }

    let project_filter = if project_slugs.is_empty() {
        String::new()
    } else {
        project_slugs
            .iter()
            .map(|p| format!("project:{}", p))
            .collect::<Vec<_>>()
            .join(" OR ")
    };

    let query = if project_filter.is_empty() {
        "is:unresolved".to_string()
    } else {
        format!("is:unresolved ({})", project_filter)
    };

    let url = format!(
        "{}/organizations/{}/issues/?query={}&limit=50&sort=freq",
        SENTRY_API_URL,
        org,
        urlencoding::encode(&query)
    );

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("Sentry API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Sentry API returned {}", response.status()));
    }

    let issues: Vec<ApiIssue> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Sentry response: {}", e))?;

    Ok(issues
        .into_iter()
        .map(|i| SentryError {
            id: i.id,
            title: i.title,
            culprit: i.culprit,
            short_id: i.short_id,
            count: i.count.parse().unwrap_or(0),
            user_count: i.user_count,
            first_seen: i.first_seen,
            last_seen: i.last_seen,
            level: i.level,
            status: i.status,
            permalink: i.permalink,
            project: i.project.slug,
            metadata: SentryMetadata {
                filename: i.metadata.filename,
                function: i.metadata.function,
                error_type: i.metadata.error_type,
                value: i.metadata.value,
            },
        })
        .collect())
}

/// Mark an issue as resolved
pub async fn resolve_issue(client: &Client, token: &str, issue_id: &str) -> Result<(), String> {
    let url = format!("{}/issues/{}/", SENTRY_API_URL, issue_id);
    let response = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "status": "resolved" }))
        .send()
        .await
        .map_err(|e| format!("Sentry API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Sentry API returned {}", response.status()));
    }

    Ok(())
}
