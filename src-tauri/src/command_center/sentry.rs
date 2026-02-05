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

    // Fetch issues from each project using the per-project endpoint
    // API: GET /api/0/projects/{org}/{project}/issues/
    let mut all_issues = Vec::new();

    for project_slug in project_slugs {
        let url = format!(
            "{}/projects/{}/{}/issues/?query={}&statsPeriod=14d",
            SENTRY_API_URL,
            org,
            project_slug,
            urlencoding::encode("is:unresolved")
        );

        eprintln!("[Sentry] Requesting URL: {}", url);

        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| format!("Sentry API error: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            eprintln!("[Sentry] API error response for project {}: {}", project_slug, body);
            // Continue to next project instead of failing entirely
            continue;
        }

        let issues: Vec<ApiIssue> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Sentry response: {}", e))?;

        eprintln!("[Sentry] Project {} returned {} issues", project_slug, issues.len());

        all_issues.extend(issues.into_iter().map(|i| SentryError {
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
        }));
    }

    // Sort by event count (most frequent first)
    all_issues.sort_by(|a, b| b.count.cmp(&a.count));

    // Limit to top 50 across all projects
    all_issues.truncate(50);

    Ok(all_issues)
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
