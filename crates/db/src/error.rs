pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // sqlx errors
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

impl Error {
    /// Whether this error is a violated unique constraint, e.g. a name that is
    /// already taken.
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Self::Sqlx(sqlx::Error::Database(error)) if error.is_unique_violation())
    }

    /// The name of the violated constraint, if the database reported one.
    pub fn constraint(&self) -> Option<&str> {
        match self {
            Self::Sqlx(sqlx::Error::Database(error)) => error.constraint(),
            _ => None,
        }
    }
}
