//! SQLite storage for certo.
#![forbid(unsafe_code)]

mod schema;
mod store;
mod zones;

pub use schema::DB_VERSION;
pub use store::{Db, DbError};
pub use zones::NewRecord;
