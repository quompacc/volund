use std::path::Path;

use axum::body::Body;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::api_models::ImportUploadSummary;
use crate::session::AuthenticatedSession;

struct UploadItem {
    internal_id: i64,
    draft_id: String,
    item_id: String,
    expected_bytes: i64,
    status: String,
    uploaded_bytes: i64,
    sha256: Option<String>,
    category: String,
}

/// Stream one reviewed draft item into the isolated incoming store.
///
/// # Errors
///
/// Refuses unknown, unconfigured, length-mismatched, concurrent, or unsafe
/// uploads and reports storage/database failures without publishing partial data.
pub async fn upload_item(
    pool: &PgPool,
    incoming_root: &Path,
    actor: &AuthenticatedSession,
    draft_id: &str,
    item_id: &str,
    content_length: Option<u64>,
    body: Body,
) -> Result<ImportUploadSummary, ImportUploadError> {
    let item = load_item(pool, actor.database_user_id(), draft_id, item_id).await?;
    if item.status == "uploaded" {
        if item.category == "archive" {
            crate::archive_intake::expand_zip(pool, incoming_root, draft_id, item_id)
                .await
                .map_err(ImportUploadError::BadRequest)?;
        }
        return completed_summary(&item, true);
    }
    let expected = u64::try_from(item.expected_bytes)
        .map_err(|_| ImportUploadError::Database("negative stored byte size".to_owned()))?;
    if content_length != Some(expected) {
        return Err(ImportUploadError::BadRequest(format!(
            "Content-Length must equal the reviewed file size of {expected} bytes"
        )));
    }
    // Keep reservation, cleanup and completion alive if the HTTP future is dropped.
    // Only the body stream is cancelled; a completed stream is published atomically.
    let (request_alive, cancelled) = tokio::sync::oneshot::channel::<()>();
    let archive = item.category == "archive";
    let worker_pool = pool.clone();
    let worker_root = incoming_root.to_path_buf();
    let upload = tokio::spawn(async move {
        finish_upload(&worker_pool, &worker_root, item, expected, body, cancelled).await
    });
    let result = upload
        .await
        .map_err(|error| ImportUploadError::Storage(format!("upload worker failed: {error}")));
    drop(request_alive);
    let result = result??;
    if archive {
        crate::archive_intake::expand_zip(pool, incoming_root, draft_id, item_id)
            .await
            .map_err(ImportUploadError::BadRequest)?;
    }
    Ok(result)
}

async fn finish_upload(
    pool: &PgPool,
    incoming_root: &Path,
    item: UploadItem,
    expected: u64,
    body: Body,
    mut cancelled: tokio::sync::oneshot::Receiver<()>,
) -> Result<ImportUploadSummary, ImportUploadError> {
    acquire_item(pool, item.internal_id).await?;
    let directory = incoming_root.join(&item.draft_id);
    let part_path = directory.join(format!("{}.part", item.item_id));
    let final_path = directory.join(format!("{}.bin", item.item_id));
    if let Err(error) = prepare_staging(&directory, &part_path, &final_path).await {
        reset_item(pool, item.internal_id).await;
        return Err(error);
    }
    let streamed = tokio::select! {
        biased;
        _ = &mut cancelled => Err(ImportUploadError::BadRequest("upload request was cancelled".to_owned())),
        result = stream_body(body, &part_path, expected) => result,
    };
    let (byte_size, sha256) = match streamed {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&part_path).await;
            reset_item(pool, item.internal_id).await;
            return Err(error);
        }
    };
    if let Err(error) = fs::rename(&part_path, &final_path).await {
        let _ = fs::remove_file(&part_path).await;
        reset_item(pool, item.internal_id).await;
        return Err(ImportUploadError::Storage(format!(
            "cannot publish staged upload: {error}"
        )));
    }
    if let Err(error) = persist_completion(pool, item.internal_id, byte_size, &sha256).await {
        let _ = fs::remove_file(&final_path).await;
        reset_item(pool, item.internal_id).await;
        return Err(error);
    }
    Ok(ImportUploadSummary {
        draft_id: item.draft_id,
        item_id: item.item_id,
        byte_size,
        sha256,
        status: "uploaded".to_owned(),
        already_uploaded: false,
    })
}

#[derive(Debug)]
pub enum ImportUploadError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Storage(String),
    Database(String),
}

async fn load_item(
    pool: &PgPool,
    owner_user_id: i64,
    draft_id: &str,
    item_id: &str,
) -> Result<UploadItem, ImportUploadError> {
    let row = sqlx::query(
        "SELECT item.id, draft.public_id::text, item.public_id::text, item.byte_size, \
         item.upload_status,item.uploaded_bytes,item.sha256,item.category \
         FROM volund.import_draft_items item \
         JOIN volund.import_drafts draft ON draft.id = item.import_draft_id \
         WHERE draft.public_id::text = $1 AND item.public_id::text = $2 \
         AND draft.owner_user_id=$3 AND draft.status IN \
         ('draft','uploading','uploaded','review_ready','reviewed','failed') \
         AND draft.configured_at IS NOT NULL AND draft.expires_at > now()",
    )
    .bind(draft_id)
    .bind(item_id)
    .bind(owner_user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| ImportUploadError::Database(format!("cannot load import item: {error}")))?
    .ok_or_else(|| ImportUploadError::NotFound("unknown reviewed import item".to_owned()))?;
    Ok(UploadItem {
        internal_id: row.get(0),
        draft_id: row.get(1),
        item_id: row.get(2),
        expected_bytes: row.get(3),
        status: row.get(4),
        uploaded_bytes: row.get(5),
        sha256: row.get(6),
        category: row.get(7),
    })
}

async fn acquire_item(pool: &PgPool, internal_id: i64) -> Result<(), ImportUploadError> {
    let policy = crate::import_policy::load(pool).await.map_err(|error| {
        ImportUploadError::Database(format!("cannot load import policy: {error:?}"))
    })?;
    let mut transaction = pool.begin().await.map_err(|error| {
        ImportUploadError::Database(format!("cannot begin upload reservation: {error}"))
    })?;
    let draft_id = sqlx::query_scalar::<_, i64>(
        "SELECT import_draft_id FROM volund.import_draft_items WHERE id=$1",
    )
    .bind(internal_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ImportUploadError::Database(format!("cannot locate import item: {error}")))?
    .ok_or_else(|| ImportUploadError::NotFound("unknown import item".to_owned()))?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(draft_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            ImportUploadError::Database(format!("cannot lock import draft: {error}"))
        })?;
    let active = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM volund.import_draft_items WHERE import_draft_id=$1 \
         AND upload_status='uploading' AND upload_started_at>=now()-interval '1 hour'",
    )
    .bind(draft_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| {
        ImportUploadError::Database(format!("cannot count active uploads: {error}"))
    })?;
    if active >= policy.max_concurrent_uploads {
        return Err(ImportUploadError::Conflict(
            "this draft reached its concurrent upload limit".to_owned(),
        ));
    }
    let acquired = sqlx::query_scalar::<_, i64>(
        "UPDATE volund.import_draft_items SET upload_status='uploading',upload_started_at=now(), \
         uploaded_bytes=0,sha256=NULL WHERE id=$1 AND (upload_status='pending' OR \
         (upload_status='uploading' AND upload_started_at<now()-interval '1 hour')) \
         RETURNING import_draft_id",
    )
    .bind(internal_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ImportUploadError::Database(format!("cannot reserve import item: {error}")))?;
    if acquired.is_none() {
        return Err(ImportUploadError::Conflict(
            "this import item is already being uploaded".to_owned(),
        ));
    }
    sqlx::query("UPDATE volund.import_drafts SET status='uploading',updated_at=now() WHERE id=$1")
        .bind(draft_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            ImportUploadError::Database(format!("cannot update import draft: {error}"))
        })?;
    transaction.commit().await.map_err(|error| {
        ImportUploadError::Database(format!("cannot commit upload reservation: {error}"))
    })?;
    Ok(())
}

async fn prepare_staging(
    directory: &Path,
    part_path: &Path,
    final_path: &Path,
) -> Result<(), ImportUploadError> {
    fs::create_dir_all(directory).await.map_err(|error| {
        ImportUploadError::Storage(format!("cannot create staging directory: {error}"))
    })?;
    if fs::try_exists(final_path).await.map_err(|error| {
        ImportUploadError::Storage(format!("cannot inspect staged upload: {error}"))
    })? {
        fs::remove_file(final_path).await.map_err(|error| {
            ImportUploadError::Storage(format!("cannot remove orphaned staged upload: {error}"))
        })?;
    }
    if fs::try_exists(part_path).await.map_err(|error| {
        ImportUploadError::Storage(format!("cannot inspect partial upload: {error}"))
    })? {
        fs::remove_file(part_path).await.map_err(|error| {
            ImportUploadError::Storage(format!("cannot remove stale partial upload: {error}"))
        })?;
    }
    Ok(())
}

async fn stream_body(
    body: Body,
    part_path: &Path,
    expected: u64,
) -> Result<(i64, String), ImportUploadError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(part_path)
        .await
        .map_err(|error| {
            ImportUploadError::Storage(format!("cannot create partial upload: {error}"))
        })?;
    let mut stream = body.into_data_stream();
    let mut digest = Sha256::new();
    let mut received = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            ImportUploadError::BadRequest(format!("upload stream was interrupted: {error}"))
        })?;
        received = received.checked_add(chunk.len() as u64).ok_or_else(|| {
            ImportUploadError::BadRequest("uploaded byte count overflowed".to_owned())
        })?;
        if received > expected {
            return Err(ImportUploadError::BadRequest(
                "upload exceeded the reviewed file size".to_owned(),
            ));
        }
        file.write_all(&chunk).await.map_err(|error| {
            ImportUploadError::Storage(format!("cannot write staged upload: {error}"))
        })?;
        digest.update(&chunk);
    }
    if received != expected {
        return Err(ImportUploadError::BadRequest(format!(
            "upload ended after {received} of {expected} bytes"
        )));
    }
    // Tokio can defer the final write error; sync_data alone does not report it.
    file.flush().await.map_err(|error| {
        ImportUploadError::Storage(format!("cannot flush staged upload: {error}"))
    })?;
    file.sync_data().await.map_err(|error| {
        ImportUploadError::Storage(format!("cannot sync staged upload: {error}"))
    })?;
    let byte_size = i64::try_from(received)
        .map_err(|_| ImportUploadError::BadRequest("uploaded file is too large".to_owned()))?;
    Ok((byte_size, format!("{:x}", digest.finalize())))
}

async fn persist_completion(
    pool: &PgPool,
    internal_id: i64,
    byte_size: i64,
    sha256: &str,
) -> Result<(), ImportUploadError> {
    let mut transaction = pool.begin().await.map_err(|error| {
        ImportUploadError::Database(format!("cannot begin upload completion: {error}"))
    })?;
    // Serialize completions before changing items or taking aggregate snapshots.
    sqlx::query(
        "SELECT d.id FROM volund.import_drafts d WHERE d.id= \
        (SELECT import_draft_id FROM volund.import_draft_items WHERE id=$1) FOR UPDATE",
    )
    .bind(internal_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| {
        ImportUploadError::Database(format!("cannot lock upload completion: {error}"))
    })?;
    let result = sqlx::query(
        "UPDATE volund.import_draft_items i SET upload_status='uploaded',uploaded_bytes=$2, \
         sha256=$3,upload_completed_at=now() WHERE i.id=$1 AND i.upload_status='uploading' \
         AND EXISTS(SELECT 1 FROM volund.import_drafts d WHERE d.id=i.import_draft_id \
         AND d.status NOT IN ('cancelled','expired','committing','committed'))",
    )
    .bind(internal_id)
    .bind(byte_size)
    .bind(sha256)
    .execute(&mut *transaction)
    .await
    .map_err(|error| {
        ImportUploadError::Database(format!("cannot complete import upload: {error}"))
    })?;
    if result.rows_affected() != 1 {
        return Err(ImportUploadError::Conflict(
            "import item upload ownership was lost".to_owned(),
        ));
    }
    sqlx::query(
        "UPDATE volund.import_drafts d SET uploaded_bytes=items.bytes, \
         status=CASE WHEN items.pending=0 THEN 'uploaded' ELSE 'uploading' END,updated_at=now() \
         FROM (SELECT import_draft_id,coalesce(sum(uploaded_bytes),0)::bigint bytes, \
         count(*) FILTER (WHERE upload_status<>'uploaded') pending FROM volund.import_draft_items \
         WHERE import_draft_id=(SELECT import_draft_id FROM volund.import_draft_items WHERE id=$1) \
         GROUP BY import_draft_id) items WHERE d.id=items.import_draft_id",
    )
    .bind(internal_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| {
        ImportUploadError::Database(format!("cannot update draft progress: {error}"))
    })?;
    transaction.commit().await.map_err(|error| {
        ImportUploadError::Database(format!("cannot commit upload completion: {error}"))
    })?;
    Ok(())
}

async fn reset_item(pool: &PgPool, internal_id: i64) {
    let _ = sqlx::query(
        "UPDATE volund.import_draft_items SET upload_status = 'pending', upload_started_at = NULL \
         WHERE id = $1 AND upload_status = 'uploading'",
    )
    .bind(internal_id)
    .execute(pool)
    .await;
}

fn completed_summary(
    item: &UploadItem,
    already_uploaded: bool,
) -> Result<ImportUploadSummary, ImportUploadError> {
    let sha256 = item.sha256.clone().ok_or_else(|| {
        ImportUploadError::Database("uploaded item has no stored SHA-256".to_owned())
    })?;
    Ok(ImportUploadSummary {
        draft_id: item.draft_id.clone(),
        item_id: item.item_id.clone(),
        byte_size: item.uploaded_bytes,
        sha256,
        status: "uploaded".to_owned(),
        already_uploaded,
    })
}
