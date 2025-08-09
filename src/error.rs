use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("project root error: {0}")]
    Root(String),

    #[error("template parse error: {0}")]
    Template(String),
}
