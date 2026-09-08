use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::api_models::{ErrorBody, ErrorEnvelope};
use crate::author_catalog::AuthorError;
use crate::collection_catalog::CollectionError;
use crate::identity::IdentityError;
use crate::import_commit::ImportCommitError;
use crate::import_lifecycle::ImportLifecycleError;
use crate::import_planner::ImportPlanError;
use crate::import_review::ImportReviewError;
use crate::import_upload::ImportUploadError;
use crate::library_admin::LibraryAdminError;
use crate::lifecycle::LifecycleError;
use crate::model_maintenance::ModelError;
use crate::session::SessionError;
use crate::settings::SettingsError;
use crate::source_move::MoveError;
use crate::tag_catalog::TagError;
use crate::user_admin::UserAdminError;

pub(crate) enum ApiError {
    BadRequest(String),
    Conflict(String),
    Forbidden,
    NotFound(String),
    Database(String),
    Identity(IdentityError),
    Library(LibraryAdminError),
    SetupUnavailable,
    Session(SessionError),
    Settings(SettingsError),
    User(UserAdminError),
    Move(MoveError),
    Model(ModelError),
    Collection(CollectionError),
    Author(AuthorError),
    Tag(TagError),
    Import(ImportPlanError),
    Upload(ImportUploadError),
    Review(ImportReviewError),
    Commit(ImportCommitError),
    ImportLifecycle(ImportLifecycleError),
    Lifecycle(LifecycleError),
}

impl IntoResponse for ApiError {
    #[allow(clippy::too_many_lines)] // Central exhaustive mapping keeps public errors consistent.
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "the current account is not allowed to perform this operation".to_owned(),
            ),
            Self::Lifecycle(LifecycleError::Forbidden) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "the current account is not allowed to perform this lifecycle operation".to_owned(),
            ),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, "not_found", message),
            Self::SetupUnavailable => setup_unavailable_response(),
            Self::Identity(error) => identity_error_response(error),
            Self::Library(LibraryAdminError::NotFound) => (
                StatusCode::NOT_FOUND,
                "not_found",
                "library not found".to_owned(),
            ),
            Self::Library(LibraryAdminError::Storage(diagnostic)) => {
                internal_error("library storage", &diagnostic, "library scan failed")
            }
            Self::Library(LibraryAdminError::Database(diagnostic)) => internal_error(
                "library administration",
                &diagnostic,
                "library operation failed",
            ),
            Self::Session(error) => session_error_response(error),
            Self::Settings(error) => settings_error_response(error),
            Self::User(error) => user_error_response(error),
            Self::Author(error) => author_error_response(error),
            Self::Collection(error) => collection_error_response(error),
            Self::Tag(error) => tag_error_response(error),
            Self::Library(LibraryAdminError::BadRequest(message))
            | Self::Move(MoveError::BadRequest(message))
            | Self::Model(ModelError::BadRequest(message))
            | Self::Import(ImportPlanError::BadRequest(message))
            | Self::Review(ImportReviewError::BadRequest(message))
            | Self::Commit(ImportCommitError::BadRequest(message))
            | Self::Upload(ImportUploadError::BadRequest(message))
            | Self::ImportLifecycle(ImportLifecycleError::BadRequest(message))
            | Self::Lifecycle(LifecycleError::BadRequest(message)) => {
                (StatusCode::BAD_REQUEST, "bad_request", message)
            }
            Self::Lifecycle(LifecycleError::NotFound) => (
                StatusCode::NOT_FOUND,
                "not_found",
                "lifecycle resource or plan not found".to_owned(),
            ),
            Self::ImportLifecycle(ImportLifecycleError::NotFound) => (
                StatusCode::NOT_FOUND,
                "not_found",
                "import draft not found".to_owned(),
            ),
            Self::Move(MoveError::NotFound(message))
            | Self::Model(ModelError::NotFound(message))
            | Self::Import(ImportPlanError::NotFound(message))
            | Self::Review(ImportReviewError::NotFound(message))
            | Self::Commit(ImportCommitError::NotFound(message))
            | Self::Upload(ImportUploadError::NotFound(message)) => {
                (StatusCode::NOT_FOUND, "not_found", message)
            }
            Self::Conflict(message)
            | Self::Library(LibraryAdminError::Conflict(message))
            | Self::Move(MoveError::Conflict(message) | MoveError::Busy(message))
            | Self::Model(ModelError::Conflict(message))
            | Self::Upload(ImportUploadError::Conflict(message))
            | Self::ImportLifecycle(ImportLifecycleError::Conflict(message))
            | Self::Lifecycle(LifecycleError::Conflict(message)) => {
                (StatusCode::CONFLICT, "conflict", message)
            }
            Self::Commit(
                ImportCommitError::Conflict(message) | ImportCommitError::Busy(message),
            ) => (StatusCode::CONFLICT, "conflict", message),
            Self::Move(MoveError::Storage(diagnostic) | MoveError::Database(diagnostic)) => {
                internal_error("managed-move", &diagnostic, "managed move failed")
            }
            Self::Model(ModelError::RevisionConflict) => (
                StatusCode::CONFLICT,
                "revision_conflict",
                "the model changed; reload it before saving".to_owned(),
            ),
            Self::Model(ModelError::Database(diagnostic)) => {
                internal_error("catalog", &diagnostic, "catalog operation failed")
            }
            Self::Import(ImportPlanError::Database(diagnostic)) => {
                internal_error("import planning", &diagnostic, "import preview failed")
            }
            Self::Review(ImportReviewError::Database(diagnostic)) => {
                internal_error("import review", &diagnostic, "import review failed")
            }
            Self::Commit(
                ImportCommitError::Storage(diagnostic) | ImportCommitError::Database(diagnostic),
            ) => internal_error(
                "import commit",
                &diagnostic,
                "import confirmation failed and was rolled back",
            ),
            Self::Upload(
                ImportUploadError::Storage(diagnostic) | ImportUploadError::Database(diagnostic),
            ) => internal_error("import upload", &diagnostic, "import upload failed"),
            Self::ImportLifecycle(
                ImportLifecycleError::Storage(diagnostic)
                | ImportLifecycleError::Database(diagnostic),
            ) => internal_error(
                "import lifecycle",
                &diagnostic,
                "import lifecycle operation failed",
            ),
            Self::Database(diagnostic) => {
                internal_error("database", &diagnostic, "database request failed")
            }
            Self::Lifecycle(LifecycleError::Database(diagnostic)) => internal_error(
                "catalog lifecycle",
                &diagnostic,
                "catalog lifecycle operation failed",
            ),
        };
        (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody { code, message },
            }),
        )
            .into_response()
    }
}

fn setup_unavailable_response() -> (StatusCode, &'static str, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "setup_unavailable",
        "first-owner setup is not configured".to_owned(),
    )
}

fn identity_error_response(error: IdentityError) -> (StatusCode, &'static str, String) {
    match error {
        IdentityError::InvalidBootstrapToken => (
            StatusCode::UNAUTHORIZED,
            "invalid_setup_credentials",
            "setup credentials are invalid".to_owned(),
        ),
        IdentityError::AlreadyInitialized => (
            StatusCode::CONFLICT,
            "already_initialized",
            "instance setup is already complete".to_owned(),
        ),
        IdentityError::InvalidInput(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
        IdentityError::Unavailable(diagnostic) => {
            internal_error("identity", &diagnostic, "identity operation failed")
        }
    }
}

fn session_error_response(error: SessionError) -> (StatusCode, &'static str, String) {
    match error {
        SessionError::InvalidCredentials => (
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "email or password is invalid".to_owned(),
        ),
        SessionError::InvalidSession => (
            StatusCode::UNAUTHORIZED,
            "invalid_session",
            "authentication is required".to_owned(),
        ),
        SessionError::InvalidCsrf => (
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "request verification failed".to_owned(),
        ),
        SessionError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "session not found".to_owned(),
        ),
        SessionError::Unavailable(diagnostic) => {
            internal_error("session", &diagnostic, "session operation failed")
        }
    }
}

fn user_error_response(error: UserAdminError) -> (StatusCode, &'static str, String) {
    match error {
        UserAdminError::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
        UserAdminError::Forbidden => (
            StatusCode::FORBIDDEN,
            "forbidden",
            "the current account cannot modify this user".to_owned(),
        ),
        UserAdminError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "user not found".to_owned(),
        ),
        UserAdminError::Conflict(message) => (StatusCode::CONFLICT, "conflict", message),
        UserAdminError::Database(diagnostic) => {
            internal_error("user administration", &diagnostic, "user operation failed")
        }
    }
}

fn author_error_response(error: AuthorError) -> (StatusCode, &'static str, String) {
    match error {
        AuthorError::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
        AuthorError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "author not found".to_owned(),
        ),
        AuthorError::Conflict(message) => (StatusCode::CONFLICT, "conflict", message),
        AuthorError::RevisionConflict => (
            StatusCode::CONFLICT,
            "revision_conflict",
            "the author changed; reload it before saving".to_owned(),
        ),
        AuthorError::Database(diagnostic) => {
            internal_error("author catalog", &diagnostic, "author operation failed")
        }
    }
}

fn collection_error_response(error: CollectionError) -> (StatusCode, &'static str, String) {
    match error {
        CollectionError::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
        CollectionError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "collection or model not found".to_owned(),
        ),
        CollectionError::Conflict(message) => (StatusCode::CONFLICT, "conflict", message),
        CollectionError::RevisionConflict => (
            StatusCode::CONFLICT,
            "revision_conflict",
            "the collection changed; reload it before saving".to_owned(),
        ),
        CollectionError::Database(diagnostic) => internal_error(
            "collection catalog",
            &diagnostic,
            "collection operation failed",
        ),
    }
}

fn tag_error_response(error: TagError) -> (StatusCode, &'static str, String) {
    match error {
        TagError::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
        TagError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "tag not found".to_owned(),
        ),
        TagError::Conflict(message) => (StatusCode::CONFLICT, "conflict", message),
        TagError::RevisionConflict => (
            StatusCode::CONFLICT,
            "revision_conflict",
            "the tag changed; reload it before saving".to_owned(),
        ),
        TagError::Database(diagnostic) => {
            internal_error("tag catalog", &diagnostic, "tag operation failed")
        }
    }
}

fn settings_error_response(error: SettingsError) -> (StatusCode, &'static str, String) {
    match error {
        SettingsError::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message),
        SettingsError::Unknown => (
            StatusCode::NOT_FOUND,
            "not_found",
            "setting is not registered".to_owned(),
        ),
        SettingsError::Conflict => (
            StatusCode::CONFLICT,
            "revision_conflict",
            "the setting changed; reload it before saving".to_owned(),
        ),
        SettingsError::Database(diagnostic) => {
            internal_error("settings", &diagnostic, "settings operation failed")
        }
    }
}

fn internal_error(
    context: &str,
    diagnostic: &str,
    public_message: &str,
) -> (StatusCode, &'static str, String) {
    crate::runtime_log::failure(
        "api",
        "api.operation_failed",
        "api_internal_error",
        &format!(
            "{context}: {}",
            crate::operational_log::sanitize(diagnostic)
        ),
    );
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        public_message.to_owned(),
    )
}
