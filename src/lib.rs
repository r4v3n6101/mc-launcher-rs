use std::{io, result};

pub mod download;
pub mod file;
pub mod metadata;
pub mod process;
pub mod resources;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),
    #[error(transparent)]
    ZipError(#[from] zip::result::ZipError),
}

pub type Result<T> = result::Result<T, Error>;
