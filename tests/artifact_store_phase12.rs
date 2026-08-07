//! Phase 12E: disk artifact store for truncated tool outputs.
//!
//! Covers `ArtifactStore` save/load/delete under a temp data root and the
//! `BoundedOutput::persist_if_truncated` helper that drops `full_output` after
//! a successful write.

use std::fs;

use luminus::{
    artifact_store::ArtifactStore,
    tool_output::{ArtifactId, BoundedOutput, Bounds, TruncationKind},
};

fn temp_root(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "luminus-artifact-phase12-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&root);
    root
}

#[test]
fn artifact_store_save_load_delete_round_trip() {
    let root = temp_root("roundtrip");
    let store = ArtifactStore::new(&root);

    let body = "full tool output\nline two\n";
    let id = store.save(body).expect("save");
    assert!(!id.as_str().is_empty());
    assert!(store.exists(&id));

    let path = store.path_for(&id);
    assert!(
        path.starts_with(store.artifacts_dir()),
        "artifact path must live under artifacts/: {path:?}"
    );
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("txt"));
    assert_eq!(fs::read_to_string(&path).unwrap(), body);
    assert_eq!(store.load(&id).unwrap(), body);

    assert!(store.delete(&id).unwrap());
    assert!(!store.exists(&id));
    assert_eq!(
        store.load(&id).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    assert!(!store.delete(&id).unwrap());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn artifact_store_uses_data_root_artifacts_layout() {
    let root = temp_root("layout");
    let store = ArtifactStore::new(&root);
    let id = store.save("x").unwrap();
    let expected_dir = root.join("artifacts");
    assert_eq!(store.artifacts_dir(), expected_dir);
    assert!(store.path_for(&id).is_file());
    assert!(store.path_for(&id).starts_with(&expected_dir));
    assert_eq!(
        store.path_for(&id).extension().and_then(|e| e.to_str()),
        Some("txt")
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn artifact_store_ids_are_unique() {
    let root = temp_root("unique");
    let store = ArtifactStore::new(&root);
    let a = store.save("one").unwrap();
    let b = store.save("two").unwrap();
    assert_ne!(a, b);
    assert_eq!(store.load(&a).unwrap(), "one");
    assert_eq!(store.load(&b).unwrap(), "two");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn persist_if_truncated_writes_artifact_and_drops_full_output() {
    let root = temp_root("persist");
    let store = ArtifactStore::new(&root);

    let input: String = (0..50).map(|i| format!("line-{i}\n")).collect();
    let bounds = Bounds {
        max_bytes: Some(64),
        max_lines: Some(5),
    };
    let mut bounded = BoundedOutput::truncate(&input, bounds);
    assert!(bounded.truncated);
    assert!(bounded.full_output.is_some());
    assert!(bounded.artifact_id.is_none());
    let full = bounded.full_output.clone().unwrap();

    bounded.persist_if_truncated(&store).expect("persist");

    let id = bounded
        .artifact_id
        .as_ref()
        .expect("artifact_id set after persist");
    assert!(
        bounded.full_output.is_none(),
        "full_output must be dropped after successful disk persist"
    );
    assert_eq!(store.load(id).unwrap(), full);
    assert_eq!(store.load(id).unwrap(), input);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn persist_full_alias_matches_persist_if_truncated() {
    let root = temp_root("alias");
    let store = ArtifactStore::new(&root);

    let mut bounded = BoundedOutput::truncate(
        "abcdefghij",
        Bounds {
            max_bytes: Some(3),
            max_lines: None,
        },
    );
    assert!(bounded.truncated);
    bounded.persist_full(&store).unwrap();
    assert!(bounded.artifact_id.is_some());
    assert!(bounded.full_output.is_none());
    assert_eq!(
        store.load(bounded.artifact_id.as_ref().unwrap()).unwrap(),
        "abcdefghij"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn persist_if_truncated_is_noop_when_not_truncated() {
    let root = temp_root("noop");
    let store = ArtifactStore::new(&root);

    let mut bounded = BoundedOutput::truncate("tiny", Bounds::default());
    assert!(!bounded.truncated);
    bounded.persist_if_truncated(&store).unwrap();
    assert!(bounded.artifact_id.is_none());
    assert!(bounded.full_output.is_none());
    // No artifacts directory required when nothing was written.
    assert!(
        !store.artifacts_dir().exists()
            || fs::read_dir(store.artifacts_dir())
                .map(|d| d.count() == 0)
                .unwrap_or(true)
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn persist_if_truncated_is_idempotent_when_already_persisted() {
    let root = temp_root("idempotent");
    let store = ArtifactStore::new(&root);

    let mut bounded = BoundedOutput::truncate(
        "0123456789abcdef",
        Bounds {
            max_bytes: Some(4),
            max_lines: None,
        },
    );
    bounded.persist_if_truncated(&store).unwrap();
    let first_id = bounded.artifact_id.clone().unwrap();

    // Even if full_output were reattached, an existing artifact_id short-circuits.
    bounded.full_output = Some("should-not-be-written".into());
    bounded.persist_if_truncated(&store).unwrap();
    assert_eq!(bounded.artifact_id.as_ref(), Some(&first_id));
    assert_eq!(store.load(&first_id).unwrap(), "0123456789abcdef");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn load_unknown_id_errors() {
    let root = temp_root("missing");
    let store = ArtifactStore::new(&root);
    let err = store.load(&ArtifactId::new("does-not-exist")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn truncated_metadata_survives_persist() {
    let root = temp_root("meta");
    let store = ArtifactStore::new(&root);

    let input = "aaaa\nbbbb\ncccc\ndddd";
    let mut bounded = BoundedOutput::truncate(
        input,
        Bounds {
            max_bytes: Some(6),
            max_lines: Some(2),
        },
    );
    assert_eq!(bounded.truncation, TruncationKind::Both);
    let preview = bounded.preview.clone();
    let total_bytes = bounded.total_bytes;
    let total_lines = bounded.total_lines;
    let bytes_omitted = bounded.bytes_omitted;
    let lines_omitted = bounded.lines_omitted;

    bounded.persist_if_truncated(&store).unwrap();

    assert_eq!(bounded.preview, preview);
    assert_eq!(bounded.total_bytes, total_bytes);
    assert_eq!(bounded.total_lines, total_lines);
    assert_eq!(bounded.bytes_omitted, bytes_omitted);
    assert_eq!(bounded.lines_omitted, lines_omitted);
    assert!(bounded.truncated);
    assert!(bounded.artifact_id.is_some());
    assert!(bounded.full_output.is_none());

    let _ = fs::remove_dir_all(&root);
}
