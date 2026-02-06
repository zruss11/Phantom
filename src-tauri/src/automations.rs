use chrono::{DateTime, Local};
use croner::Cron;
use std::str::FromStr;

pub fn compute_next_run_at(cron_expr: &str, from: DateTime<Local>) -> Result<i64, String> {
    let expr = cron_expr.trim();
    if expr.is_empty() {
        return Err("Cron expression is required.".to_string());
    }
    let cron = Cron::from_str(expr).map_err(|e| format!("Invalid cron expression: {}", e))?;
    let next = cron
        .find_next_occurrence(&from, false)
        .map_err(|e| format!("Unable to compute next run time: {}", e))?;
    Ok(next.timestamp())
}
