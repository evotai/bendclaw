use std::path::PathBuf;

use crate::error::BendclawError;
use crate::error::Result;

fn evotai_home() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| BendclawError::Env("HOME or USERPROFILE not set".into()))?;
    Ok(PathBuf::from(home).join(".evotai"))
}

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(evotai_home()?.join("sessions"))
}

pub fn runs_dir() -> Result<PathBuf> {
    Ok(evotai_home()?.join("runs"))
}

pub fn env_file_path() -> Result<PathBuf> {
    Ok(evotai_home()?.join("bendclaw.env"))
}
