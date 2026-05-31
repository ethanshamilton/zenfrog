pub mod error;
pub mod ingest;
pub mod lance;

pub use error::{DbError, DbResult};
pub use lance::{Db, DbConfig};
