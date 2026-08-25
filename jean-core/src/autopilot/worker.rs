//! Provider-native Autopilot Worker execution.
//!
//! A native Worker is a bounded, non-interactive invocation of the selected
//! provider CLI. It is deliberately separate from Jean Chat's persisted
//! message queue: the mission controller owns the lifecycle, while the
//! provider owns the actual terminal/CLI execution protocol.

use std::path::Path;

use serde::Deserialize;
use tauri::AppHandle;

use super::types::AutopilotMission;

pub const AUTOPILOT_WORKER_RESPONSE_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "summary": { "type": "string", "maxLength": 20000 },
    "checks": {
      "type": "array",
      "items": { "type": "string", "maxLength": 2000 },
      "maxItems": 50
    },
    "changed_paths": {
      "type": "array",
      "items": { "type": "string", "maxLength": 1000 },
      "maxItems": 100
    },
    "risks": {
      "type": "array",
      "items": { "type": "string", "maxLength": 2000 },
      "maxItems": 50
    },
    "succeeded": { "type": "boolean" }
  },
  "required": ["summary", "checks", "changed_paths", "risks", "succeeded"],
  "additionalProperties": false
}"#;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerOutput {
    summary: String,
    #[serde(default)]
    checks: Vec<String>,
    #[serde(default)]
    changed_paths: Vec<String>,
    #[serde(default)]
    risks: Vec<String>,
    succeeded: bool,
}

#[derive(Debug)]
pub struct NativeWorkerResult {
    pub succeeded: bool,
    pub summary: String,
    pub evidence: Vec<String>,
}

fn effective_execution_mode(mission: &AutopilotMission) -> &str {
    if mission.worker.execution_mode == "yolo" && !mission.policy.auto_approve_safe_actions {
        "build"
    } else {
        mission.worker.execution_mode.as_str()
    }
}

fn worker_prompt(prompt: &str) -> String {
    format!(
        "{prompt}\n\nYou are running as the provider-native Autopilot Worker. This is one bounded task, not an open-ended conversation. Work only in the selected worktree. Use the provider's normal file and terminal tools, run the smallest useful verification, and stop when this task is complete. Do not widen scope or make product decisions that are not supported by the mission. If the task cannot be completed safely, set succeeded to false and explain the blocker. Return exactly one JSON object matching the required output schema; do not return markdown or a conversational preamble."
    )
}

fn validate_output(output: &str) -> Result<NativeWorkerResult, String> {
    let value: serde_json::Value = serde_json::from_str(output.trim())
        .map_err(|error| format!("Native Worker returned invalid JSON: {error}"))?;
    let parsed: WorkerOutput = serde_json::from_value(value)
        .map_err(|error| format!("Native Worker response did not match its schema: {error}"))?;
    if parsed.summary.trim().is_empty() {
        return Err("Native Worker returned an empty summary".to_string());
    }
    if parsed.summary.len() > 20_000 {
        return Err("Native Worker summary is too long".to_string());
    }

    let mut evidence = Vec::new();
    evidence.extend(
        parsed
            .checks
            .into_iter()
            .map(|check| format!("check: {check}")),
    );
    evidence.extend(
        parsed
            .changed_paths
            .into_iter()
            .map(|path| format!("changed: {path}")),
    );
    evidence.extend(parsed.risks.into_iter().map(|risk| format!("risk: {risk}")));
    if evidence.is_empty() {
        evidence.push("Native Worker returned no additional evidence.".to_string());
    }

    Ok(NativeWorkerResult {
        succeeded: parsed.succeeded,
        summary: parsed.summary,
        evidence,
    })
}

/// Run a provider-native Worker synchronously. Callers must run this on a
/// blocking executor because several provider adapters wait on child
/// processes or a local provider server.
pub fn run_native_worker_blocking(
    app: &AppHandle,
    mission: &AutopilotMission,
    prompt: &str,
    model: &str,
) -> Result<NativeWorkerResult, String> {
    let worktree = Path::new(&mission.worktree_path);
    if !worktree.is_dir() {
        return Err(format!(
            "Autopilot Worker worktree does not exist: {}",
            worktree.display()
        ));
    }

    let prompt = worker_prompt(prompt);
    let execution_mode = effective_execution_mode(mission);
    let raw_output = match mission.worker.backend.as_str() {
        "claude" => crate::chat::claude::execute_one_shot_claude_worker(
            app,
            &prompt,
            model,
            AUTOPILOT_WORKER_RESPONSE_SCHEMA,
            worktree,
            execution_mode,
        )?,
        "codex" => crate::chat::codex::execute_one_shot_codex(
            app,
            &prompt,
            model,
            AUTOPILOT_WORKER_RESPONSE_SCHEMA,
            Some(worktree),
            None,
        )?,
        "opencode" => crate::chat::opencode::execute_one_shot_opencode(
            app,
            &prompt,
            model,
            Some(AUTOPILOT_WORKER_RESPONSE_SCHEMA),
            Some(worktree),
            None,
        )?,
        "kimi" => crate::chat::kimi::execute_one_shot_kimi_with_mode(
            app,
            &prompt,
            model,
            Some(AUTOPILOT_WORKER_RESPONSE_SCHEMA),
            Some(worktree),
            execution_mode,
        )?,
        backend => {
            return Err(format!(
                "Native Autopilot Worker is not supported for backend {backend} yet"
            ));
        }
    };

    validate_output(&raw_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_structured_worker_result_and_builds_evidence() {
        let result = validate_output(
            r#"{
                "summary":"Implemented the fix",
                "checks":["bun run typecheck passed"],
                "changed_paths":["src/app.ts"],
                "risks":["Integration test still needs a real backend"],
                "succeeded":true
            }"#,
        )
        .unwrap();

        assert!(result.succeeded);
        assert_eq!(result.summary, "Implemented the fix");
        assert!(result
            .evidence
            .iter()
            .any(|item| item.starts_with("check:")));
        assert!(result
            .evidence
            .iter()
            .any(|item| item.starts_with("changed:")));
    }

    #[test]
    fn rejects_non_object_or_unknown_worker_output() {
        assert!(validate_output(r#"{"summary":"done"}"#).is_err());
        assert!(validate_output(r#"{"summary":"done","unknown":true}"#).is_err());
    }
}
