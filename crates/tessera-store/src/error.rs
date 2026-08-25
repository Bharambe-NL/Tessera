use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Doc 01 section 6.3 fixes the vocabulary. An event type outside it means
    /// an agent emitted something nothing can read back, so it is rejected at
    /// the boundary rather than stored.
    #[error("unknown event type `{0}`; add it to EVENT_VOCABULARY before emitting it")]
    UnknownEventType(String),

    #[error("the projection for `{event_type}` needs `{field}` in its payload")]
    ProjectionFieldMissing { event_type: String, field: &'static str },

    #[error("no migration path from schema version {found} to {expected}")]
    SchemaVersion { found: i32, expected: i32 },

    /// Migrations run with foreign keys off, because a table rebuild has to
    /// drop a parent table. This is the check that they went back on clean.
    #[error("migration to schema version {version} left {rows} broken foreign key references")]
    MigrationBrokeReferences { version: i32, rows: i64 },

    #[error("blob {0} is not in the store")]
    BlobMissing(String),

    #[error("blob {expected} does not hash to its content (got {actual})")]
    BlobCorrupt { expected: String, actual: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;
