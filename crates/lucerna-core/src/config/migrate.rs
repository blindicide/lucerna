//! Schema migration framework (directive §53).
//!
//! v1 ships with an empty migration table. The framework is exercised by tests with a synthetic
//! v0 → v1 migration.

use toml_edit::DocumentMut;

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("no migration path from schema version {from} to {to}")]
    NoPath { from: u32, to: u32 },
    #[error("migration from schema version {from} failed: {reason}")]
    Failed { from: u32, reason: String },
}

/// A pure step that upgrades a document from schema `N` to `N + 1`.
pub type Migration = fn(&mut DocumentMut) -> Result<(), MigrationError>;

/// Apply `migrations[from]`, `migrations[from + 1]`, ... until `target` is reached, then stamp
/// `schema_version = target`. `migrations[n]` upgrades version `n` to `n + 1`.
pub fn migrate_with(
    doc: &mut DocumentMut,
    from: u32,
    target: u32,
    migrations: &[Migration],
) -> Result<(), MigrationError> {
    if from > target || usize::try_from(target).map_or(true, |t| t > migrations.len()) {
        return Err(MigrationError::NoPath { from, to: target });
    }
    for version in from..target {
        let step = migrations
            .get(usize::try_from(version).unwrap_or(usize::MAX))
            .ok_or(MigrationError::NoPath {
                from: version,
                to: target,
            })?;
        step(doc).map_err(|e| match e {
            MigrationError::Failed { reason, .. } => MigrationError::Failed {
                from: version,
                reason,
            },
            other => other,
        })?;
    }
    doc["schema_version"] = toml_edit::value(i64::from(target));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v0_to_v1(doc: &mut DocumentMut) -> Result<(), MigrationError> {
        // A synthetic change: v0 called the setting `mute`, v1 calls it `audio` (inverted).
        if let Some(mute) = doc
            .get("general")
            .and_then(|g| g.get("mute"))
            .and_then(|m| m.as_bool())
        {
            doc["general"]["audio"] = toml_edit::value(!mute);
            if let Some(general) = doc["general"].as_table_mut() {
                general.remove("mute");
            }
        }
        Ok(())
    }

    fn failing(_: &mut DocumentMut) -> Result<(), MigrationError> {
        Err(MigrationError::Failed {
            from: 0,
            reason: "boom".into(),
        })
    }

    #[test]
    fn steps_run_in_order_and_stamp_the_version() {
        let mut doc: DocumentMut = "[general]\nmute = true\nkeep = 1\n".parse().unwrap();
        migrate_with(&mut doc, 0, 1, &[v0_to_v1]).unwrap();
        assert_eq!(doc["schema_version"].as_integer(), Some(1));
        assert_eq!(doc["general"]["audio"].as_bool(), Some(false));
        assert!(doc["general"].get("mute").is_none());
        assert_eq!(
            doc["general"]["keep"].as_integer(),
            Some(1),
            "unrelated keys survive"
        );
    }

    #[test]
    fn multi_step_chains_apply_every_step() {
        fn bump(doc: &mut DocumentMut) -> Result<(), MigrationError> {
            let n = doc.get("counter").and_then(|c| c.as_integer()).unwrap_or(0);
            doc["counter"] = toml_edit::value(n + 1);
            Ok(())
        }
        let mut doc: DocumentMut = "".parse().unwrap();
        migrate_with(&mut doc, 0, 3, &[bump, bump, bump]).unwrap();
        assert_eq!(doc["counter"].as_integer(), Some(3));
        let mut doc: DocumentMut = "counter = 10".parse().unwrap();
        migrate_with(&mut doc, 2, 3, &[bump, bump, bump]).unwrap();
        assert_eq!(
            doc["counter"].as_integer(),
            Some(11),
            "starts from the given version"
        );
    }

    #[test]
    fn missing_paths_and_failures_are_errors() {
        let mut doc: DocumentMut = "".parse().unwrap();
        assert!(matches!(
            migrate_with(&mut doc, 0, 2, &[v0_to_v1]),
            Err(MigrationError::NoPath { .. })
        ));
        assert!(matches!(
            migrate_with(&mut doc, 3, 1, &[v0_to_v1]),
            Err(MigrationError::NoPath { .. })
        ));
        assert!(matches!(
            migrate_with(&mut doc, 0, 1, &[failing]),
            Err(MigrationError::Failed { from: 0, .. })
        ));
    }

    #[test]
    fn already_current_is_a_no_op_that_still_stamps() {
        let mut doc: DocumentMut = "schema_version = 1".parse().unwrap();
        migrate_with(&mut doc, 1, 1, &[v0_to_v1]).unwrap();
        assert_eq!(doc["schema_version"].as_integer(), Some(1));
    }
}
