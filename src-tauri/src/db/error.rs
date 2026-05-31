use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("lancedb error: {0}")]
    Lance(#[from] lancedb::Error),

    #[error("arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing column: {0}")]
    MissingColumn(String),

    #[error("invalid data: {0}")]
    InvalidData(String),
}

pub type DbResult<T> = Result<T, DbError>;
