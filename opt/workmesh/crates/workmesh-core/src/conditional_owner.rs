//! Cooperative compare-and-mutate ownership. Operator override APIs remain overrides.
use crate::{
    storage::{with_path_lock, write_string_atomic},
    task::{parse_task_file, split_front_matter, Task, TaskParseError},
    task_ops::{append_note, extract_section_content, update_front_matter_value, FieldValue},
};
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractPin {
    pub id: String,
    pub uid: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub definition_of_done: String,
}
impl ContractPin {
    pub fn from_task(task: &Task) -> Result<Self, TaskParseError> {
        let section = |name| {
            extract_section_content(&task.body, name)
                .map(|s| s.trim_end_matches('\n').to_string())
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("missing contract section"))
        };
        Ok(Self {
            id: task.id.clone(),
            uid: task
                .uid
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("missing UID"))?,
            description: section("Description")?,
            acceptance_criteria: section("Acceptance Criteria")?,
            definition_of_done: section("Definition of Done")?,
        })
    }
}
#[derive(Debug, Clone)]
pub enum OwnerAction {
    Claim,
    Check { minimum_seconds: i64 },
    Renew,
    Note(String),
    Complete,
}
fn invalid(message: &str) -> TaskParseError {
    TaskParseError::Invalid(message.into())
}
fn timestamp(value: Option<&str>) -> Option<NaiveDateTime> {
    value.and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").ok())
}

/// Read-only ownership exclusion for cooperative repository-wide admission.
/// Active status is case-insensitive, as in WorkMesh selection. A declared lease
/// permits another worker only when its representation and timestamps are valid
/// and the owning WorkMesh lease predicate establishes that it has expired.
pub fn ownership_blocks_other_task(path: &Path) -> Result<bool, TaskParseError> {
    with_path_lock(path, || {
        let task = parse_task_file(path)?;
        if task.status.trim().eq_ignore_ascii_case("in progress") {
            return Ok(true);
        }
        let (front, _) = split_front_matter(&fs::read_to_string(path)?)?;
        let value: serde_yaml::Value = serde_yaml::from_str(&front)
            .map_err(|_| invalid("malformed/duplicate task front matter"))?;
        let mapping = value
            .as_mapping()
            .ok_or_else(|| invalid("task front matter must be a mapping"))?;
        let flat_keys = ["lease_owner", "lease_acquired_at", "lease_expires_at"];
        let flat = flat_keys
            .iter()
            .any(|key| mapping.contains_key(serde_yaml::Value::String((*key).into())));
        let nested = mapping.get(serde_yaml::Value::String("lease".into()));
        if !flat && nested.is_none() {
            return Ok(false);
        }
        let (fields, keys) = if flat {
            if nested.is_some() {
                return Ok(true);
            }
            (mapping, flat_keys)
        } else {
            let Some(fields) = nested.and_then(serde_yaml::Value::as_mapping) else {
                return Ok(true);
            };
            (fields, ["owner", "acquired_at", "expires_at"])
        };
        let scalar = |key: &str| {
            fields
                .get(serde_yaml::Value::String(key.into()))
                .and_then(serde_yaml::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        let (Some(_owner), Some(acquired), Some(expiry)) = (
            scalar(keys[0]),
            timestamp(scalar(keys[1])),
            timestamp(scalar(keys[2])),
        ) else {
            return Ok(true);
        };
        if acquired > expiry || task.lease.is_none() {
            return Ok(true);
        }
        Ok(crate::task_ops::is_lease_active(&task))
    })
}

/// Returns the exact resulting file text. All comparisons and the one write hold the same path lock.
pub fn conditional_owner(
    path: &Path,
    pin: &ContractPin,
    owner: &str,
    now: NaiveDateTime,
    action: OwnerAction,
) -> Result<String, TaskParseError> {
    with_path_lock(path, || {
        if owner.is_empty()
            || owner.len() > 128
            || !owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
        {
            return Err(invalid("empty owner"));
        }
        let task = parse_task_file(path)?;
        if ContractPin::from_task(&task)? != *pin {
            return Err(invalid("task contract changed"));
        }
        let text = fs::read_to_string(path)?;
        // Operator reading remains permissive, but conditional mutation never guesses at malformed YAML.
        let (front, _) = split_front_matter(&text)?;
        let strict: serde_yaml::Value = serde_yaml::from_str(&front)
            .map_err(|_| invalid("malformed/duplicate task front matter"))?;
        let mapping = strict
            .as_mapping()
            .ok_or_else(|| invalid("task front matter must be a mapping"))?;
        for key in ["lease_owner", "lease_acquired_at", "lease_expires_at"] {
            if mapping
                .get(serde_yaml::Value::String(key.into()))
                .is_some_and(|v| !v.is_string())
            {
                return Err(invalid("malformed lease scalar"));
            }
        }
        if task.lease.is_none() {
            let (front, _) = split_front_matter(&text)?;
            if front.lines().any(|line| {
                ["lease_owner:", "lease_acquired_at:", "lease_expires_at:"]
                    .iter()
                    .any(|key| line.trim_start().starts_with(key))
            }) {
                return Err(invalid("malformed ownerless lease"));
            }
        }
        let mut updated = text.clone();
        if matches!(action, OwnerAction::Claim) {
            if !task.status.eq_ignore_ascii_case("To Do") {
                return Err(invalid("claim requires To Do"));
            }
            // Malformed, indefinite and live leases are never stolen, even from the same owner.
            if let Some(lease) = &task.lease {
                let expiry = timestamp(lease.expires_at.as_deref())
                    .ok_or_else(|| invalid("malformed/indefinite lease"))?;
                if timestamp(lease.acquired_at.as_deref()).is_none() {
                    return Err(invalid("malformed acquired lease"));
                }
                if expiry > now {
                    return Err(invalid("live lease exists"));
                }
            }
            updated = set(&updated, "status", Some("In Progress".into()))?;
            updated = set(&updated, "lease_owner", Some(owner.into()))?;
            updated = set(
                &updated,
                "lease_acquired_at",
                Some(now.format("%Y-%m-%d %H:%M").to_string()),
            )?;
        } else {
            let lease = task
                .lease
                .as_ref()
                .ok_or_else(|| invalid("lease missing"))?;
            let expiry =
                timestamp(lease.expires_at.as_deref()).ok_or_else(|| invalid("malformed lease"))?;
            if task.status != "In Progress"
                || lease.owner != owner
                || expiry <= now
                || timestamp(lease.acquired_at.as_deref()).is_none()
            {
                return Err(invalid("ownership lost/expired"));
            }
            if let OwnerAction::Check { minimum_seconds } = action {
                if expiry - now < Duration::seconds(minimum_seconds) {
                    return Err(invalid("insufficient live lease"));
                }
                return Ok(text);
            }
        }
        match action {
            OwnerAction::Claim | OwnerAction::Renew => {
                updated = set(
                    &updated,
                    "lease_expires_at",
                    Some(
                        (now + Duration::minutes(10))
                            .format("%Y-%m-%d %H:%M")
                            .to_string(),
                    ),
                )?
            }
            OwnerAction::Note(note) => {
                let (front, body) = split_front_matter(&updated)?;
                updated = format!("---\n{front}\n---\n{}", append_note(&body, &note, "notes"));
            }
            OwnerAction::Complete => {
                updated = set(&updated, "status", Some("Done".into()))?;
                for field in ["lease_owner", "lease_acquired_at", "lease_expires_at"] {
                    updated = set(&updated, field, None)?;
                }
            }
            OwnerAction::Check { .. } => unreachable!(),
        }
        updated = set(
            &updated,
            "updated_date",
            Some(now.format("%Y-%m-%d %H:%M").to_string()),
        )?;
        write_string_atomic(path, &updated)?;
        Ok(updated)
    })
}
fn set(text: &str, key: &str, value: Option<String>) -> Result<String, TaskParseError> {
    update_front_matter_value(text, key, value.map(FieldValue::Scalar))
}

/// Only renewal's two timestamp fields; owner, acquisition identity and all other content remain bound.
pub fn normalize_owner_timestamps(text: &str) -> Result<String, TaskParseError> {
    set(&set(text, "lease_expires_at", None)?, "updated_date", None)
}

/// Exact note rendering for a caller's reviewed snapshot; not a mutation or ownership check.
/// Allows a supervisor to reject unrelated concurrent changes, rather than masking the whole task.
pub fn preview_owner_note(
    text: &str,
    note: &str,
    now: NaiveDateTime,
) -> Result<String, TaskParseError> {
    let (front, body) = split_front_matter(text)?;
    set(
        &format!("---\n{front}\n---\n{}", append_note(&body, note, "notes")),
        "updated_date",
        Some(now.format("%Y-%m-%d %H:%M").to_string()),
    )
}

/// Exact terminal metadata delta, for validating a caller's immutable implementation checkpoint.
pub fn preview_owner_completion(text: &str, now: NaiveDateTime) -> Result<String, TaskParseError> {
    let mut rendered = set(text, "status", Some("Done".into()))?;
    for field in ["lease_owner", "lease_acquired_at", "lease_expires_at"] {
        rendered = set(&rendered, field, None)?;
    }
    set(
        &rendered,
        "updated_date",
        Some(now.format("%Y-%m-%d %H:%M").to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_ownership_observation_preserves_status_lease_and_uncertainty_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.md");
        let now = chrono::Local::now().naive_local();
        let acquired = (now - Duration::hours(2))
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let past = (now - Duration::hours(1))
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let future = (now + Duration::hours(1))
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let flat = |expiry: &str| {
            format!(
                "lease_owner: foreign\nlease_acquired_at: {acquired}\nlease_expires_at: {expiry}\n"
            )
        };
        let nested = |expiry: &str| {
            format!("lease:\n  owner: foreign\n  acquired_at: {acquired}\n  expires_at: {expiry}\n")
        };
        let cases = [
            ("To Do", String::new(), false),
            ("In Progress", String::new(), true),
            ("in progress", String::new(), true),
            (" iN pRoGrEsS ", String::new(), true),
            ("To Do", flat(&future), true),
            ("Done", flat(&future), true),
            ("To Do", flat(&past), false),
            (
                "To Do",
                format!("lease_owner: foreign\nlease_acquired_at: {acquired}\n"),
                true,
            ),
            ("To Do", flat("not-a-date"), true),
            (
                "To Do",
                flat(&past).replace(&acquired, "bad acquisition"),
                true,
            ),
            ("To Do", format!("lease_expires_at: {future}\n"), true),
            (
                "To Do",
                flat(&past).replace("lease_owner: foreign", "lease_owner: []"),
                true,
            ),
            ("To Do", nested(&future), true),
            ("To Do", nested(&past), false),
            ("To Do", flat(&past) + &nested(&past), true),
        ];
        for (status, fields, expected) in cases {
            let text = format!("---\nid: task-001\nstatus: '{status}'\n{fields}---\n");
            fs::write(&path, &text).unwrap();
            assert_eq!(
                ownership_blocks_other_task(&path).unwrap(),
                expected,
                "status={status}; {fields}"
            );
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                text,
                "observation must not change task bytes"
            );
        }
    }
    #[test]
    fn preview_deltas_are_exact_and_normalization_preserves_contract_and_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.md");
        let text="---\nid: task-001\nuid: stable\nstatus: To Do\n---\nDescription:\nfix\nAcceptance Criteria:\nworks\nDefinition of Done:\nverified\n";
        fs::write(&path, text).unwrap();
        let pin = ContractPin::from_task(&parse_task_file(&path).unwrap()).unwrap();
        let now = chrono::Local::now().naive_local();
        let claimed = conditional_owner(&path, &pin, "owner", now, OwnerAction::Claim).unwrap();
        let noted = conditional_owner(
            &path,
            &pin,
            "owner",
            now,
            OwnerAction::Note("exact evidence".into()),
        )
        .unwrap();
        assert_eq!(
            preview_owner_note(&claimed, "exact evidence", now).unwrap(),
            noted
        );
        let completed =
            conditional_owner(&path, &pin, "owner", now, OwnerAction::Complete).unwrap();
        assert_eq!(preview_owner_completion(&noted, now).unwrap(), completed);
        let normalized = normalize_owner_timestamps(&noted).unwrap();
        assert!(normalized.contains("lease_owner: owner"));
        assert!(normalized.contains("lease_acquired_at:"));
        assert!(normalized.contains("exact evidence"));
        assert!(!normalized.contains("updated_date:"));
        assert!(!normalized.contains("lease_expires_at:"));
        assert_ne!(
            normalized,
            normalize_owner_timestamps(&noted.replace("works", "changed scope")).unwrap()
        );
    }
}
