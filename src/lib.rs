use std::result;

pub mod io;
pub mod metadata;
pub mod resources;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),
    #[error(transparent)]
    ZipError(#[from] zip::result::ZipError),
}

pub type Result<T> = result::Result<T, Error>;
