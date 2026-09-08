use sqlx::{Postgres, Transaction};

// All capacity-increasing transactions must take this lock before draft locks,
// and retain the transaction through publication. Read in a separate statement
// after acquisition so READ COMMITTED sees the preceding reservation's commit.
pub(crate) async fn lock_and_used(tx: &mut Transaction<'_, Postgres>) -> Result<i64, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(860756368, 3)")
        .execute(&mut **tx)
        .await?;
    sqlx::query_scalar(
        "SELECT COALESCE(sum(CASE \
         WHEN status IN ('draft','uploading','uploaded','review_ready','reviewed','failed','committing') \
         THEN total_bytes \
         WHEN status IN ('cancelled','expired','committed') AND staging_cleaned_at IS NULL \
         THEN uploaded_bytes ELSE 0 END),0)::bigint FROM volund.import_drafts",
    )
    .fetch_one(&mut **tx)
    .await
}
