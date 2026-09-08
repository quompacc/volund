use super::*;
use crate::import_configuration::{
    normalize_collection_ids, normalize_tags, optional_text, required_text, validate_kind,
};

#[test]
fn names_are_humanized_and_slugged_deterministically() {
    assert_eq!(model_name("VÖLUND Projekt.zip"), "VÖLUND Projekt");
    assert_eq!(slugify("VÖLUND Projekt").unwrap(), "voelund-projekt");
    assert!(slugify("---").is_err());
}

#[test]
fn project_files_are_classified_into_a_clean_common_root() {
    let entries = vec![
        ImportManifestEntry {
            path: "Voron/CAD/main.step".into(),
            byte_size: 50,
        },
        ImportManifestEntry {
            path: "Voron/docs/manual.pdf".into(),
            byte_size: 10,
        },
        ImportManifestEntry {
            path: "Voron/STLs/panel.stl".into(),
            byte_size: 20,
        },
        ImportManifestEntry {
            path: "Voron/CAD/source.f3d".into(),
            byte_size: 30,
        },
    ];
    let mut planned = plan_items(entries, "voron");
    mark_primary_candidate(&mut planned);
    assert_eq!(planned[0].suggested_relative_path, "voron/CAD/main.step");
    assert!(planned[0].is_primary_candidate);
    assert_eq!(planned[1].category, "document");
    assert_eq!(planned[2].suggested_relative_path, "voron/Meshes/panel.stl");
    assert_eq!(planned[3].suggested_relative_path, "voron/CAD/source.f3d");
}

#[test]
fn supported_step_assembly_wins_over_larger_proprietary_cad() {
    let entries = vec![
        ImportManifestEntry {
            path: "CAD/Switchwire_Assembly.step".into(),
            byte_size: 50,
        },
        ImportManifestEntry {
            path: "CAD/Switchwire_Assembly.f3d".into(),
            byte_size: 5_000,
        },
    ];
    let mut planned = plan_items(entries, "switchwire");
    mark_primary_candidate(&mut planned);
    assert!(planned[0].is_primary_candidate);
    assert!(!planned[1].is_primary_candidate);
}

#[test]
fn unsafe_duplicate_and_negative_manifest_entries_are_rejected() {
    assert!(
        normalize_entries(vec![ImportManifestEntry {
            path: "../escape.step".into(),
            byte_size: 1,
        }])
        .is_err()
    );
    assert!(
        normalize_entries(vec![
            ImportManifestEntry {
                path: "part.step".into(),
                byte_size: 1,
            },
            ImportManifestEntry {
                path: "part.step".into(),
                byte_size: 1,
            },
        ])
        .is_err()
    );
    assert!(
        normalize_entries(vec![ImportManifestEntry {
            path: "part.step".into(),
            byte_size: -1,
        }])
        .is_err()
    );
}

#[test]
fn reviewed_metadata_is_trimmed_validated_and_deduplicated() {
    assert_eq!(
        required_text(" Voron 2.4 ", "name", 20).unwrap(),
        "Voron 2.4"
    );
    assert_eq!(
        optional_text(Some("  Steve Builds  "), "author", 30).unwrap(),
        Some("Steve Builds".into())
    );
    assert_eq!(validate_kind("assembly").unwrap(), "assembly");
    assert!(validate_kind("printer").is_err());
    assert_eq!(
        normalize_tags(vec!["Voron".into(), "voron".into(), "CoreXY".into()]).unwrap(),
        vec!["Voron", "CoreXY"]
    );
    assert_eq!(
        normalize_collection_ids(vec!["collection-1".into(), "collection-1".into()]).unwrap(),
        vec!["collection-1"]
    );
}
