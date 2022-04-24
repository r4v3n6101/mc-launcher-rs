use std::{io, result};

pub mod file;
pub mod metadata;
pub mod resources;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error("unknown version {0}")]
    UnknownVersion(String),
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),
}

pub type Result<T> = result::Result<T, Error>;
