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

    /// Doc 10 section 15. The message is what the shell shows, so it says what
    /// happened and what is on offer rather than quoting SQLite at a person.
    #[error("this profile's database is damaged and cannot be opened: {detail}")]
    Corrupt { detail: String },

    /// A write that named a card the store does not hold. Its own variant
    /// because the alternative is an `UPDATE` that matches nothing and reports
    /// success, which is how a card silently fails to stay where it was put.
    #[error("card {0} is not in the store")]
    CardMissing(String),

    #[error("blob {0} is not in the store")]
    BlobMissing(String),

    #[error("blob {expected} does not hash to its content (got {actual})")]
    BlobCorrupt { expected: String, actual: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;
