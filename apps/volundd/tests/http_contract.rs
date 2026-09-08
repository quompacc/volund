use std::collections::BTreeSet;
use volundd::api_auth::ROUTE_POLICIES;

const ROUTER_SOURCE: &str = include_str!("../src/api.rs");
const PUBLIC_ROUTER_SOURCE: &str = include_str!("../src/api_public.rs");
const OPERATIONS_ROUTER_SOURCE: &str = include_str!("../src/api_operations.rs");
const JOB_ROUTER_SOURCE: &str = include_str!("../src/api_jobs.rs");
const POLICY_ROUTER_SOURCE: &str = include_str!("../src/api_policy.rs");
const METADATA_ROUTER_SOURCE: &str = include_str!("../src/api_metadata.rs");
const HISTORY_ROUTER_SOURCE: &str = include_str!("../src/api_history.rs");
const THUMBNAIL_ROUTER_SOURCE: &str = include_str!("../src/api_thumbnail.rs");
const PREFERENCES_ROUTER_SOURCE: &str = include_str!("../src/api_preferences.rs");
const PROBLEMS_ROUTER_SOURCE: &str = include_str!("../src/api_problems.rs");
const PRIMARY_ROUTER_SOURCE: &str = include_str!("../src/api_primary.rs");
const IMPORT_ROUTER_SOURCE: &str = include_str!("../src/api_import.rs");
const OPENAPI: &str = include_str!("../../../contracts/http-api-v1.openapi.yaml");

fn normalized_path(path: &str) -> String {
    let mut result = String::new();
    let mut parameter = false;
    for character in path.chars() {
        match character {
            '{' => {
                parameter = true;
                result.push_str("{}");
            }
            '}' => parameter = false,
            _ if !parameter => result.push(character),
            _ => {}
        }
    }
    result
}

fn router_paths() -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let mut route_on_next_line = false;
    for line in ROUTER_SOURCE
        .lines()
        .chain(PUBLIC_ROUTER_SOURCE.lines())
        .chain(OPERATIONS_ROUTER_SOURCE.lines())
        .chain(JOB_ROUTER_SOURCE.lines())
        .chain(POLICY_ROUTER_SOURCE.lines())
        .chain(METADATA_ROUTER_SOURCE.lines())
        .chain(HISTORY_ROUTER_SOURCE.lines())
        .chain(THUMBNAIL_ROUTER_SOURCE.lines())
        .chain(PREFERENCES_ROUTER_SOURCE.lines())
        .chain(PROBLEMS_ROUTER_SOURCE.lines())
        .chain(PRIMARY_ROUTER_SOURCE.lines())
        .chain(IMPORT_ROUTER_SOURCE.lines())
        .map(str::trim)
    {
        let candidate = line
            .split_once(".route(\"")
            .map(|(_, value)| value)
            .or_else(|| route_on_next_line.then(|| line.strip_prefix('"')).flatten());
        if let Some(candidate) = candidate.and_then(|value| value.split('"').next()) {
            paths.insert(normalized_path(&format!("/api/v1{candidate}")));
        }
        route_on_next_line = line == ".route(";
    }
    paths
}

fn openapi_paths() -> BTreeSet<String> {
    OPENAPI
        .lines()
        .filter_map(|line| {
            line.strip_prefix("  /api/v1/")
                .and_then(|path| path.strip_suffix(':'))
                .map(|path| normalized_path(&format!("/api/v1/{path}")))
        })
        .collect()
}

fn openapi_operations() -> BTreeSet<(String, String)> {
    let mut current_path = None;
    let mut operations = BTreeSet::new();
    for line in OPENAPI.lines() {
        if let Some(path) = line
            .strip_prefix("  /api/v1/")
            .and_then(|path| path.strip_suffix(':'))
        {
            current_path = Some(normalized_path(&format!("/api/v1/{path}")));
        } else if let Some(method) = line
            .strip_prefix("    ")
            .and_then(|line| line.strip_suffix(':'))
            .filter(|method| matches!(*method, "get" | "post" | "put" | "patch" | "delete"))
        {
            operations.insert((
                method.to_ascii_uppercase(),
                current_path.clone().expect("operation path"),
            ));
        }
    }
    operations
}

fn policy_operations() -> BTreeSet<(String, String)> {
    ROUTE_POLICIES
        .iter()
        .map(|rule| (rule.method.to_owned(), rule.path.to_owned()))
        .collect()
}

fn schema(name: &str) -> String {
    let marker = format!("    {name}:");
    let mut lines = OPENAPI.lines();
    lines.find(|line| *line == marker).expect("schema exists");
    lines
        .take_while(|line| line.is_empty() || line.starts_with("      "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn openapi_paths_and_recent_catalog_fields_match_the_router() {
    assert_eq!(openapi_paths(), router_paths());
    assert_eq!(openapi_operations(), policy_operations());

    let model = schema("Model");
    assert!(model.contains("tags:"));
    assert!(model.contains("collections:"));
    assert!(model.contains("viewerRotation:"));
    assert!(model.contains("licenseKind:"));
    assert!(model.contains("primaryFileId:"));
    assert!(model.contains("revision:"));
    assert!(model.contains("thumbnail:"));
    assert!(schema("ThumbnailCandidate").contains("mediaType:"));
    assert!(schema("ThumbnailUpdate").contains("expectedRevision:"));
    assert!(schema("ThumbnailRegenerate").contains("profile:"));
    assert!(schema("Artifact").contains("thumbnail-raster"));
    assert!(schema("ModelHistoryEvent").contains("change:"));
    assert!(schema("ModelHistoryPage").contains("total:"));
    assert!(schema("AddModelComponentRequest").contains("expectedRevision:"));
    assert!(schema("UpdateModelRequest").contains("collectionIds:"));
    assert!(schema("UpdateModelRequest").contains("viewerRotation:"));
    assert!(schema("UpdateModelRequest").contains("expectedRevision:"));
    assert!(schema("SetModelPrimaryRequest").contains("primaryFileId:"));
    assert!(schema("ModelComponent").contains("fileCount:"));
    assert!(schema("Author").contains("provenanceSource:"));
    assert!(schema("Author").contains("mergedIntoId:"));
    assert!(schema("AuthorUpdate").contains("expectedRevision:"));

    let configuration = schema("ConfigureImportRequest");
    assert!(configuration.contains("libraryRootId:"));
    assert!(configuration.contains("collectionIds:"));

    assert!(schema("Collection").contains("modelCount:"));
    assert!(schema("Collection").contains("revision:"));
    assert!(schema("CollectionDetail").contains("modelIds:"));
    assert!(schema("Tag").contains("mergedIntoId:"));
    assert!(schema("TagPage").contains("total:"));
    assert!(schema("CreateCollectionRequest").contains("description:"));
    assert!(schema("ManagedLibrary").contains("filesystemPath:"));
    assert!(schema("ManagedLibrary").contains("enabled:"));
    assert!(schema("ManagedLibrary").contains("storage:"));
    assert!(schema("ManagedLibrary").contains("revision:"));
    assert!(schema("UpdateLibraryRequest").contains("required: [confirmation, expectedRevision]"));
    assert!(schema("OperationsHealth").contains("backup:"));
    assert!(schema("ManagedJob").contains("canRetry:"));
    assert!(schema("ConversionProfile").contains("builtIn:"));
    assert!(schema("RevisionConfirmation").contains("required: [confirmation, expectedRevision]"));
    for operation in ["retireConversionProfile", "deleteScanSchedule"] {
        let request = OPENAPI
            .split(&format!("operationId: {operation}"))
            .nth(1)
            .unwrap()
            .split("responses:")
            .next()
            .unwrap();
        assert!(request.contains("#/components/schemas/RevisionConfirmation"));
    }
    assert!(schema("ScanSchedule").contains("nextRunAtUnixMs:"));
    assert!(schema("RetentionPreview").contains("artifactBytes:"));
    assert!(schema("LibraryPathValidation").contains("writablePermission:"));
    assert!(schema("ModelFile").contains("primary:"));
    assert!(schema("ModelFile").contains("rootName:"));
    assert!(schema("CurrentSession").contains("mustChangePassword:"));
    assert!(schema("ManagedUser").contains("lockedUntilUnixMs:"));
    assert!(schema("InvitationCreated").contains("activationToken:"));
    assert!(schema("UserPreferences").contains("previewAutoLoad:"));
    assert!(schema("UpdateUserPreferences").contains("expectedRevision:"));
    assert!(schema("ModelProblem").contains("remediation:"));
    assert!(schema("ProblemStatusUpdate").contains("expectedRevision:"));
}
