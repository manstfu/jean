use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use tauri::AppHandle;
use uuid::Uuid;

use super::types::{AutopilotMission, MissionEvent};

static MISSION_LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn mission_lock(mission_id: &str) -> Arc<Mutex<()>> {
    let mut locks = MISSION_LOCKS.lock().expect("autopilot mission lock map");
    locks
        .entry(mission_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn safe_mission_id(mission_id: &str) -> Result<(), String> {
    if mission_id.is_empty() || mission_id.len() > 100 {
        return Err("Invalid Autopilot mission id".to_string());
    }
    if !mission_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("Invalid Autopilot mission id".to_string());
    }
    Ok(())
}

pub fn get_missions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to get app data directory: {error}"))?;
    let missions_dir = app_data_dir.join("autopilot").join("missions");
    fs::create_dir_all(&missions_dir)
        .map_err(|error| format!("Failed to create Autopilot missions directory: {error}"))?;
    Ok(missions_dir)
}

fn mission_path(app: &AppHandle, mission_id: &str) -> Result<PathBuf, String> {
    safe_mission_id(mission_id)?;
    Ok(get_missions_dir(app)?.join(format!("{mission_id}.json")))
}

fn event_path(app: &AppHandle, mission_id: &str) -> Result<PathBuf, String> {
    safe_mission_id(mission_id)?;
    Ok(get_missions_dir(app)?.join(format!("{mission_id}.events.jsonl")))
}

fn save_mission_unlocked(app: &AppHandle, mission: &AutopilotMission) -> Result<(), String> {
    let path = mission_path(app, &mission.id)?;
    let temp_path = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let contents = serde_json::to_string_pretty(mission)
        .map_err(|error| format!("Failed to serialize Autopilot mission: {error}"))?;

    fs::write(&temp_path, contents)
        .map_err(|error| format!("Failed to write Autopilot mission: {error}"))?;
    fs::rename(&temp_path, &path)
        .map_err(|error| format!("Failed to finalize Autopilot mission: {error}"))?;
    Ok(())
}

pub fn save_mission(app: &AppHandle, mission: &AutopilotMission) -> Result<(), String> {
    let lock = mission_lock(&mission.id);
    let _guard = lock.lock().expect("Autopilot mission lock");
    save_mission_unlocked(app, mission)
}

pub fn create_mission(app: &AppHandle, mission: &AutopilotMission) -> Result<(), String> {
    let lock = mission_lock(&mission.id);
    let _guard = lock.lock().expect("Autopilot mission lock");
    let path = mission_path(app, &mission.id)?;
    if path.exists() {
        return Err(format!("Autopilot mission already exists: {}", mission.id));
    }
    save_mission_unlocked(app, mission)
}

pub fn load_mission(app: &AppHandle, mission_id: &str) -> Result<AutopilotMission, String> {
    let lock = mission_lock(mission_id);
    let _guard = lock.lock().expect("Autopilot mission lock");
    let path = mission_path(app, mission_id)?;
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read Autopilot mission {mission_id}: {error}"))?;
    let mut mission: AutopilotMission = serde_json::from_str(&contents)
        .map_err(|error| format!("Failed to parse Autopilot mission {mission_id}: {error}"))?;
    if mission.schema_version == 0 {
        mission.schema_version = super::types::AUTOPILOT_SCHEMA_VERSION;
    }
    Ok(mission)
}

pub fn with_mission_mut<F, T>(app: &AppHandle, mission_id: &str, update: F) -> Result<T, String>
where
    F: FnOnce(&mut AutopilotMission) -> Result<T, String>,
{
    let lock = mission_lock(mission_id);
    let _guard = lock.lock().expect("Autopilot mission lock");
    let path = mission_path(app, mission_id)?;
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read Autopilot mission {mission_id}: {error}"))?;
    let mut mission: AutopilotMission = serde_json::from_str(&contents)
        .map_err(|error| format!("Failed to parse Autopilot mission {mission_id}: {error}"))?;
    let result = update(&mut mission)?;
    mission.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    save_mission_unlocked(app, &mission)?;
    Ok(result)
}

pub fn append_event(app: &AppHandle, mission_id: &str, event: &MissionEvent) -> Result<(), String> {
    let lock = mission_lock(mission_id);
    let _guard = lock.lock().expect("Autopilot mission lock");
    let path = event_path(app, mission_id)?;
    let line = serde_json::to_string(event)
        .map_err(|error| format!("Failed to serialize Autopilot event: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Failed to open Autopilot event log: {error}"))?;
    writeln!(file, "{line}")
        .map_err(|error| format!("Failed to append Autopilot event: {error}"))?;
    file.flush()
        .map_err(|error| format!("Failed to flush Autopilot event log: {error}"))?;
    Ok(())
}

pub fn load_events(app: &AppHandle, mission_id: &str) -> Result<Vec<MissionEvent>, String> {
    let lock = mission_lock(mission_id);
    let _guard = lock.lock().expect("Autopilot mission lock");
    let path = event_path(app, mission_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read Autopilot event log: {error}"))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|error| format!("Failed to parse Autopilot event: {error}"))
        })
        .collect()
}

pub fn list_missions(app: &AppHandle) -> Result<Vec<AutopilotMission>, String> {
    let missions_dir = get_missions_dir(app)?;
    let mut missions = Vec::new();
    for entry in fs::read_dir(&missions_dir)
        .map_err(|error| format!("Failed to list Autopilot missions: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to read Autopilot entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".events.json"))
        {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read Autopilot mission file: {error}"))?;
        let mission: AutopilotMission = serde_json::from_str(&contents)
            .map_err(|error| format!("Failed to parse Autopilot mission file: {error}"))?;
        missions.push(mission);
    }
    missions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(missions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autopilot::types::{
        AutopilotMission, DirectorConfig, MissionPolicy, StartAutopilotMissionRequest, WorkerConfig,
    };

    fn test_mission() -> AutopilotMission {
        AutopilotMission::new(StartAutopilotMissionRequest {
            project_id: "project".to_string(),
            worktree_id: "worktree".to_string(),
            worktree_path: "C:/worktree".to_string(),
            goal: "Ship the feature".to_string(),
            acceptance_criteria: vec![],
            constraints: vec![],
            non_goals: vec![],
            worker: WorkerConfig::default(),
            director: DirectorConfig::default(),
            policy: MissionPolicy::default(),
            worker_session_id: None,
            start_worker: false,
        })
    }

    #[test]
    fn mission_id_cannot_escape_storage_directory() {
        assert!(safe_mission_id("../../outside").is_err());
        assert!(safe_mission_id("mission-1").is_ok());
    }

    #[test]
    fn storage_round_trip_keeps_event_log_separate() {
        let temp = tempfile::tempdir().unwrap();
        let app = AppHandle::new(temp.path().into(), temp.path().into()).unwrap();
        let mission = test_mission();
        create_mission(&app, &mission).unwrap();
        let event = MissionEvent::new(
            "mission_created",
            "Mission created",
            serde_json::json!({ "goal": mission.goal }),
        );
        append_event(&app, &mission.id, &event).unwrap();

        let loaded = load_mission(&app, &mission.id).unwrap();
        let events = load_events(&app, &mission.id).unwrap();
        assert_eq!(loaded, mission);
        assert_eq!(events, vec![event]);
        assert!(get_missions_dir(&app)
            .unwrap()
            .join(format!("{}.events.jsonl", mission.id))
            .exists());
    }
}
