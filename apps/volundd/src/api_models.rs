use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatusResponse {
    pub initialized: bool,
    pub bootstrap_available: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFirstOwnerRequest {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSessionResponse {
    pub session_id: String,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub must_change_password: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeOwnPasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteUserRequest {
    pub email: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetUserPasswordRequest {
    pub password: String,
    #[serde(default = "default_must_change")]
    pub must_change: bool,
}

fn default_must_change() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingRequest {
    pub value: Value,
    pub expected_revision: i64,
    pub confirmation: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootSummary {
    pub id: String,
    pub key: String,
    pub name: String,
    pub read_only: bool,
    pub file_count: i64,
    pub missing_file_count: i64,
    pub latest_scan_status: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub id: String,
    pub path: String,
    pub sha256: String,
    pub byte_size: i64,
    pub format: Option<String>,
    pub modified_at_unix_ms: i64,
    pub missing: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // Independent persisted inspection facts, not one state machine.
pub struct ModelFileSummary {
    pub id: String,
    pub path: String,
    pub root_key: String,
    pub root_name: String,
    pub role: String,
    pub primary: bool,
    pub format: Option<String>,
    pub byte_size: i64,
    pub modified_at_unix_ms: i64,
    pub missing: bool,
    pub revision: i64,
    pub caption: String,
    pub description: String,
    pub notes: String,
    pub printable: bool,
    pub printed: bool,
    pub pre_supported: bool,
    pub up_axis: Option<String>,
    pub support_hint: String,
    pub orientation: [f64; 3],
    pub lifecycle_state: String,
    pub lifecycle_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelFileRequest {
    pub expected_revision: i64,
    pub caption: String,
    pub description: String,
    pub notes: String,
    pub printable: bool,
    pub printed: bool,
    pub pre_supported: bool,
    pub up_axis: Option<String>,
    pub support_hint: String,
    pub orientation: [f64; 3],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSlicerHandoffRequest {
    pub target_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlicerTargetSummary {
    pub id: String,
    pub name: String,
    pub scheme: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlicerHandoffResponse {
    pub target_id: String,
    pub launch_url: String,
    pub download_url: String,
    pub expires_at_unix_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSummary {
    pub name: String,
    pub path: String,
    pub file_count: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFileRequest {
    pub destination_directory: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFileResponse {
    pub id: String,
    pub previous_path: String,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct EnqueuePreviewRequest {
    #[serde(default = "default_preview_profile")]
    pub profile: String,
}

fn default_preview_profile() -> String {
    "web".to_owned()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewRequest {
    pub source_name: String,
    pub entries: Vec<ImportManifestEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportManifestEntry {
    pub path: String,
    pub byte_size: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDraftSummary {
    pub id: String,
    pub source_name: String,
    pub suggested_model_name: String,
    pub suggested_slug: String,
    pub total_files: i64,
    pub total_bytes: i64,
    pub items: Vec<ImportDraftItemSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDraftPage {
    pub items: Vec<ImportDraftLifecycleSummary>,
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDraftLifecycleSummary {
    pub id: String,
    pub display_name: String,
    pub status: String,
    pub actor_id: Option<String>,
    pub actor_name: Option<String>,
    pub total_files: i64,
    pub uploaded_files: i64,
    pub total_bytes: i64,
    pub uploaded_bytes: i64,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub expires_at_unix_ms: i64,
    pub target_action: Option<String>,
    pub target_model_id: Option<String>,
    pub last_error_code: Option<String>,
    pub result_model_id: Option<String>,
    pub can_retry: bool,
    pub can_cancel: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)] // Wire names explicitly distinguish byte categories.
pub struct ImportStorageSummary {
    pub reserved_bytes: i64,
    pub uploaded_bytes: i64,
    pub reclaimable_bytes: i64,
    pub capacity_bytes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameImportRequest {
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct CancelImportRequest {
    pub confirmation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportListQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCancelSummary {
    pub id: String,
    pub status: String,
    pub staging_cleaned: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveImportItemRequest {
    pub action: String,
    pub target_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDraftItemSummary {
    pub id: String,
    pub original_path: String,
    pub byte_size: i64,
    pub category: String,
    pub suggested_relative_path: String,
    pub is_primary_candidate: bool,
    pub upload_status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportUploadSummary {
    pub draft_id: String,
    pub item_id: String,
    pub byte_size: i64,
    pub sha256: String,
    pub status: String,
    pub already_uploaded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReviewSummary {
    pub draft_id: String,
    pub model_name: String,
    pub model_action: String,
    pub existing_model_id: Option<String>,
    pub root_key: String,
    pub root_name: String,
    pub base_directory: String,
    pub create_files: i64,
    pub reuse_files: i64,
    pub relocate_files: i64,
    pub conflicts: i64,
    pub skip_files: i64,
    pub new_bytes: i64,
    pub saved_bytes: i64,
    pub items: Vec<ImportReviewItemSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReviewItemSummary {
    pub id: String,
    pub original_path: String,
    pub category: String,
    pub byte_size: i64,
    pub sha256: String,
    pub action: String,
    pub target_path: String,
    pub existing_file_id: Option<String>,
    pub existing_path: Option<String>,
    pub is_primary: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCommitSummary {
    pub draft_id: String,
    pub model_id: String,
    pub model_name: String,
    pub total_files: i64,
    pub created_files: i64,
    pub reused_files: i64,
    pub relocated_files: i64,
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureImportRequest {
    pub model_name: String,
    pub kind: String,
    pub library_root_id: String,
    pub description: String,
    pub author_name: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
    #[serde(default)]
    pub target_action: Option<String>,
    #[serde(default)]
    pub target_model_id: Option<String>,
    #[serde(default)]
    pub expected_model_revision: Option<i64>,
    #[serde(default)]
    pub license_kind: Option<String>,
    #[serde(default)]
    pub license_value: Option<String>,
    #[serde(default)]
    pub primary_item_id: Option<String>,
    #[serde(default)]
    pub thumbnail_item_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportConfigurationSummary {
    pub id: String,
    pub model_name: String,
    pub slug: String,
    pub kind: String,
    pub library_root_id: String,
    pub description: String,
    pub author_name: Option<String>,
    pub tags: Vec<String>,
    pub collection_ids: Vec<String>,
    pub ready_for_upload: bool,
    pub target_action: String,
    pub target_model_id: Option<String>,
    pub expected_model_revision: Option<i64>,
    pub license_kind: String,
    pub license_value: Option<String>,
    pub primary_item_id: Option<String>,
    pub thumbnail_item_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelRequest {
    pub name: String,
    pub slug: String,
    pub kind: String,
    pub primary_file_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelRequest {
    pub expected_revision: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: String,
    pub license_kind: String,
    pub license_value: Option<String>,
    pub author_name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tag_ids: Option<Vec<String>>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
    pub primary_file_id: Option<String>,
    pub viewer_rotation: [f64; 3],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetModelPrimaryRequest {
    pub expected_revision: i64,
    pub primary_file_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddModelComponentRequest {
    pub expected_revision: i64,
    pub child_model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveModelComponentRequest {
    pub expected_revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub license_kind: String,
    pub license_value: Option<String>,
    pub author_name: Option<String>,
    pub primary_file_id: Option<String>,
    pub file_count: i64,
    pub formats: Vec<String>,
    pub updated_at_unix_ms: i64,
    pub tags: Vec<String>,
    pub tag_ids: Vec<String>,
    pub collections: Vec<String>,
    pub viewer_rotation: [f64; 3],
    pub revision: i64,
    pub thumbnail: ThumbnailState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailState {
    pub kind: String,
    pub candidate_id: Option<String>,
    pub url: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelComponentSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSummary {
    pub sha256: String,
    pub byte_size: i64,
    pub format: Option<String>,
    pub source_count: i64,
    pub available_source_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub id: String,
    pub status: String,
    pub started_at_unix_ms: i64,
    pub finished_at_unix_ms: Option<i64>,
    pub discovered_files: i64,
    pub hashed_files: i64,
    pub missing_files: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSummary {
    pub id: String,
    pub profile: String,
    pub status: String,
    pub converter_version: String,
    pub requested_at_unix_ms: i64,
    pub finished_at_unix_ms: Option<i64>,
    pub artifacts: Vec<ArtifactSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub kind: String,
    pub url: String,
    pub sha256: String,
    pub byte_size: i64,
    pub media_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}
