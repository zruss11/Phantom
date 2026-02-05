use super::types::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

/// Execute a GraphQL query against Linear API
async fn graphql_query<T: for<'de> Deserialize<'de>>(
    client: &Client,
    token: &str,
    query: &str,
    variables: Option<serde_json::Value>,
) -> Result<T, String> {
    #[derive(Serialize)]
    struct GraphQLRequest<'a> {
        query: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        variables: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct GraphQLResponse<T> {
        data: Option<T>,
        errors: Option<Vec<GraphQLError>>,
    }

    #[derive(Deserialize)]
    struct GraphQLError {
        message: String,
    }

    let response = client
        .post(LINEAR_API_URL)
        .header("Authorization", token)
        .header("Content-Type", "application/json")
        .json(&GraphQLRequest { query, variables })
        .send()
        .await
        .map_err(|e| format!("Linear API error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Linear API returned {}", response.status()));
    }

    let gql_response: GraphQLResponse<T> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Linear response: {}", e))?;

    if let Some(errors) = gql_response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.clone()).collect();
        return Err(format!("Linear GraphQL errors: {}", messages.join(", ")));
    }

    gql_response.data.ok_or_else(|| "No data in Linear response".to_string())
}

/// Fetch all teams the user has access to
/// Note: We fetch Teams (not Projects) because Teams are Linear's primary organizational unit.
/// Projects in Linear are optional higher-level groupings that many users don't use.
pub async fn fetch_projects(client: &Client, token: &str) -> Result<Vec<LinearProject>, String> {
    #[derive(Deserialize)]
    struct Response {
        teams: TeamsResponse,
    }

    #[derive(Deserialize)]
    struct TeamsResponse {
        nodes: Vec<TeamNode>,
    }

    #[derive(Deserialize)]
    struct TeamNode {
        id: String,
        name: String,
        key: String,
    }

    let query = r#"
        query {
            teams(first: 50) {
                nodes {
                    id
                    name
                    key
                }
            }
        }
    "#;

    let response: Response = graphql_query(client, token, query, None).await?;

    Ok(response
        .teams
        .nodes
        .into_iter()
        .map(|t| LinearProject {
            id: t.id,
            name: format!("{} ({})", t.name, t.key),
            state: "active".to_string(), // Teams don't have state like projects
        })
        .collect())
}

/// Fetch active cycles
pub async fn fetch_cycles(client: &Client, token: &str) -> Result<Vec<LinearCycle>, String> {
    #[derive(Deserialize)]
    struct Response {
        cycles: CyclesResponse,
    }

    #[derive(Deserialize)]
    struct CyclesResponse {
        nodes: Vec<CycleNode>,
    }

    #[derive(Deserialize)]
    struct CycleNode {
        id: String,
        name: Option<String>,
        number: u32,
        #[serde(rename = "startsAt")]
        starts_at: String,
        #[serde(rename = "endsAt")]
        ends_at: String,
    }

    let query = r#"
        query {
            cycles(first: 20, filter: { isActive: { eq: true } }) {
                nodes {
                    id
                    name
                    number
                    startsAt
                    endsAt
                }
            }
        }
    "#;

    let response: Response = graphql_query(client, token, query, None).await?;

    Ok(response
        .cycles
        .nodes
        .into_iter()
        .map(|c| LinearCycle {
            id: c.id,
            name: c.name,
            number: c.number,
            starts_at: c.starts_at,
            ends_at: c.ends_at,
        })
        .collect())
}

/// Fetch issues for specified projects/cycles
pub async fn fetch_issues(
    client: &Client,
    token: &str,
    project_ids: &[String],
    cycle_ids: &[String],
) -> Result<Vec<LinearIssue>, String> {
    #[derive(Deserialize)]
    struct Response {
        issues: IssuesResponse,
    }

    #[derive(Deserialize)]
    struct IssuesResponse {
        nodes: Vec<IssueNode>,
    }

    #[derive(Deserialize)]
    struct IssueNode {
        id: String,
        identifier: String,
        title: String,
        priority: u8,
        state: StateNode,
        labels: LabelsResponse,
        assignee: Option<UserNode>,
        #[serde(rename = "createdAt")]
        created_at: String,
        #[serde(rename = "updatedAt")]
        updated_at: String,
        url: String,
        team: TeamRef,
        cycle: Option<CycleRef>,
    }

    #[derive(Deserialize)]
    struct StateNode {
        name: String,
        color: String,
        #[serde(rename = "type")]
        state_type: String,
    }

    #[derive(Deserialize)]
    struct LabelsResponse {
        nodes: Vec<LabelNode>,
    }

    #[derive(Deserialize)]
    struct LabelNode {
        name: String,
        color: String,
    }

    #[derive(Deserialize)]
    struct UserNode {
        name: String,
    }

    #[derive(Deserialize)]
    struct TeamRef {
        name: String,
    }

    #[derive(Deserialize)]
    struct CycleRef {
        name: Option<String>,
    }

    // Build filter based on team/cycle IDs
    // Note: project_ids actually contains team IDs (we renamed the concept but kept the variable name)
    let mut filters = vec!["state: { type: { nin: [\"completed\", \"canceled\"] } }".to_string()];

    if !project_ids.is_empty() {
        let ids: Vec<_> = project_ids.iter().map(|id| format!("\"{}\"", id)).collect();
        filters.push(format!("team: {{ id: {{ in: [{}] }} }}", ids.join(", ")));
    }

    if !cycle_ids.is_empty() {
        let ids: Vec<_> = cycle_ids.iter().map(|id| format!("\"{}\"", id)).collect();
        filters.push(format!("cycle: {{ id: {{ in: [{}] }} }}", ids.join(", ")));
    }

    let filter_str = if filters.is_empty() {
        String::new()
    } else {
        format!("filter: {{ {} }}", filters.join(", "))
    };

    let query = format!(r#"
        query {{
            issues(first: 50, {}) {{
                nodes {{
                    id
                    identifier
                    title
                    priority
                    state {{
                        name
                        color
                        type
                    }}
                    labels {{
                        nodes {{
                            name
                            color
                        }}
                    }}
                    assignee {{
                        name
                    }}
                    createdAt
                    updatedAt
                    url
                    team {{
                        name
                    }}
                    cycle {{
                        name
                    }}
                }}
            }}
        }}
    "#, filter_str);

    let response: Response = graphql_query(client, token, &query, None).await?;

    Ok(response
        .issues
        .nodes
        .into_iter()
        .map(|i| LinearIssue {
            id: i.id,
            identifier: i.identifier,
            title: i.title,
            priority: i.priority,
            state: LinearState {
                name: i.state.name,
                color: i.state.color,
                state_type: i.state.state_type,
            },
            labels: i.labels.nodes.into_iter().map(|l| LinearLabel { name: l.name, color: l.color }).collect(),
            assignee: i.assignee.map(|a| a.name),
            created_at: i.created_at,
            updated_at: i.updated_at,
            url: i.url,
            project: Some(i.team.name), // Using 'project' field to store team name for backwards compat
            cycle: i.cycle.and_then(|c| c.name),
        })
        .collect())
}
