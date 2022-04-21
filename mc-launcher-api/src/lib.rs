use std::{io, result};

pub mod game;
pub mod metadata;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error("unknown version {0}")]
    UnknownVersion(String),
}

pub type Result<T> = result::Result<T, Error>;
