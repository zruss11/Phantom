//! Calendar integration (macOS).
//!
//! For now we mirror a basic "Coming up" experience by pulling upcoming Apple
//! Calendar events locally. This keeps everything on-device (no cloud sync) and
//! gives Notes a source of meeting titles.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CalendarEventDto {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(rename = "startMs")]
    pub(crate) start_ms: i64,
    #[serde(rename = "endMs")]
    pub(crate) end_ms: i64,
    pub(crate) all_day: bool,
    pub(crate) location: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) notes: Option<String>,
    pub(crate) calendar: Option<String>,
    pub(crate) meeting_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CalendarInfoDto {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) source: Option<String>,
    pub(crate) allows_modifications: bool,
}

fn find_meeting_url(s: &str) -> Option<String> {
    // Keep this intentionally simple and robust; we can expand later.
    // Zoom / Google Meet / Teams / generic https links.
    // Note: we avoid heavy regex dependencies; basic scanning is enough.
    fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
        let h = haystack.as_bytes();
        let n = needle.as_bytes();
        if n.is_empty() || n.len() > h.len() {
            return None;
        }

        for i in 0..=h.len() - n.len() {
            if !haystack.is_char_boundary(i) {
                continue;
            }
            if h[i..i + n.len()]
                .iter()
                .zip(n.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
            {
                return Some(i);
            }
        }
        None
    }
    let candidates = [
        "https://zoom.us/",
        "https://us02web.zoom.us/",
        "https://us04web.zoom.us/",
        "https://meet.google.com/",
        "https://teams.microsoft.com/",
        "https://teams.live.com/",
        "https://join.skype.com/",
        "https://webex.com/",
        "https://",
        "http://",
    ];

    for needle in candidates {
        if let Some(idx) = find_ascii_case_insensitive(s, needle) {
            // Slice from the original string to preserve case.
            let orig = &s[idx..];
            // Stop at whitespace.
            let end = orig
                .find(|c: char| {
                    c.is_whitespace() || c == ')' || c == ']' || c == '>' || c == '"' || c == '\''
                })
                .unwrap_or(orig.len());
            let url = orig[..end]
                .trim()
                .trim_end_matches(&['.', ',', ';', ':'][..]);
            if url.starts_with("http://") || url.starts_with("https://") {
                return Some(url.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn fetch_upcoming_events_macos(
    limit: u32,
    days: u32,
    calendar_ids: Option<Vec<String>>,
) -> Result<Vec<CalendarEventDto>, String> {
    use chrono::{Local, TimeDelta};
    use eventkit::EventsManager;
    use std::collections::{HashMap, HashSet};

    let mgr = EventsManager::new();
    mgr.ensure_authorized()
        .map_err(|e| format!("Calendar authorization failed: {}", e))?;

    let now = Local::now();
    let end = now + TimeDelta::try_days(days as i64).unwrap();

    let items = if let Some(ids) = calendar_ids.as_ref().filter(|v| !v.is_empty()) {
        // Map identifiers -> titles (eventkit-rs filters by titles).
        let calendars = mgr
            .list_calendars()
            .map_err(|e| format!("Calendar list failed: {}", e))?;

        let mut title_counts: HashMap<String, usize> = HashMap::new();
        for cal in &calendars {
            *title_counts.entry(cal.title.clone()).or_insert(0) += 1;
        }

        let mut titles: Vec<String> = Vec::new();
        let mut ambiguous: HashSet<String> = HashSet::new();
        for cal in calendars {
            if ids.iter().any(|id| id == &cal.identifier) {
                if title_counts.get(&cal.title).copied().unwrap_or(0) > 1 {
                    ambiguous.insert(cal.title.clone());
                }
                titles.push(cal.title);
            }
        }

        if !ambiguous.is_empty() {
            let mut names: Vec<String> = ambiguous.into_iter().collect();
            names.sort();
            return Err(format!(
                "Multiple calendars share the same title: {}. Phantom currently filters calendars by title due to an eventkit-rs limitation; please rename calendars to unique names.",
                names.join(", ")
            ));
        }

        if titles.is_empty() {
            // No matching calendars selected: return empty list quickly.
            Vec::new()
        } else {
            let refs: Vec<&str> = titles.iter().map(|s| s.as_str()).collect();
            mgr.fetch_events(now, end, Some(&refs))
                .map_err(|e| format!("Calendar fetch failed: {}", e))?
        }
    } else {
        mgr.fetch_events(now, end, None)
            .map_err(|e| format!("Calendar fetch failed: {}", e))?
    };

    let mut out = Vec::new();
    for it in items.into_iter().take(limit as usize) {
        let mut meeting_url = None;
        if meeting_url.is_none() {
            if let Some(loc) = it.location.as_deref() {
                meeting_url = find_meeting_url(loc);
            }
        }
        if meeting_url.is_none() {
            if let Some(n) = it.notes.as_deref() {
                meeting_url = find_meeting_url(n);
            }
        }

        out.push(CalendarEventDto {
            id: it.identifier,
            title: it.title,
            start_ms: it.start_date.timestamp_millis(),
            end_ms: it.end_date.timestamp_millis(),
            all_day: it.all_day,
            location: it.location,
            url: None,
            notes: it.notes,
            calendar: it.calendar_title,
            meeting_url,
        });
    }

    Ok(out)
}

#[cfg(not(target_os = "macos"))]
fn fetch_upcoming_events_macos(
    _limit: u32,
    _days: u32,
    _calendar_ids: Option<Vec<String>>,
) -> Result<Vec<CalendarEventDto>, String> {
    Ok(vec![])
}

#[cfg(target_os = "macos")]
fn list_calendars_macos() -> Result<Vec<CalendarInfoDto>, String> {
    use eventkit::EventsManager;

    let mgr = EventsManager::new();
    mgr.ensure_authorized()
        .map_err(|e| format!("Calendar authorization failed: {}", e))?;

    let calendars = mgr
        .list_calendars()
        .map_err(|e| format!("Calendar list failed: {}", e))?;

    Ok(calendars
        .into_iter()
        .map(|c| CalendarInfoDto {
            id: c.identifier,
            title: c.title,
            source: c.source,
            allows_modifications: c.allows_modifications,
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
fn list_calendars_macos() -> Result<Vec<CalendarInfoDto>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn calendar_list_calendars() -> Result<Vec<CalendarInfoDto>, String> {
    tauri::async_runtime::spawn_blocking(move || list_calendars_macos())
        .await
        .map_err(|e| format!("Calendar task join error: {}", e))?
}

#[tauri::command]
pub async fn calendar_get_upcoming_events(
    limit: Option<u32>,
    days: Option<u32>,
    calendar_ids: Option<Vec<String>>,
) -> Result<Vec<CalendarEventDto>, String> {
    let limit = limit.unwrap_or(10).clamp(1, 50);
    let days = days.unwrap_or(7).clamp(1, 31);

    tauri::async_runtime::spawn_blocking(move || {
        fetch_upcoming_events_macos(limit, days, calendar_ids)
    })
    .await
    .map_err(|e| format!("Calendar task join error: {}", e))?
}
