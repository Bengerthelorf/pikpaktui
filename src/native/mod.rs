pub mod auth;

use crate::backend::{Backend, Entry};
use anyhow::{Result, anyhow};

pub struct NativeBackend {
    auth: auth::NativeAuth,
}

impl NativeBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            auth: auth::NativeAuth::new()?,
        })
    }

    pub fn auth(&self) -> &auth::NativeAuth {
        &self.auth
    }
}

impl Backend for NativeBackend {
    fn name(&self) -> &'static str {
        "rust-native"
    }

    fn ls(&self, _path: &str) -> Result<Vec<Entry>> {
        Err(anyhow!("rust-native ls not implemented yet"))
    }

    fn mv(&self, _current_path: &str, _name: &str, _target_path: &str) -> Result<String> {
        Err(anyhow!("rust-native move not implemented yet"))
    }

    fn cp(&self, _current_path: &str, _name: &str, _target_path: &str) -> Result<String> {
        Err(anyhow!("rust-native copy not implemented yet"))
    }

    fn rename(&self, _current_path: &str, _old_name: &str, _new_name: &str) -> Result<String> {
        Err(anyhow!("rust-native rename not implemented yet"))
    }

    fn remove(&self, _current_path: &str, _name: &str) -> Result<String> {
        Err(anyhow!("rust-native remove not implemented yet"))
    }
}
