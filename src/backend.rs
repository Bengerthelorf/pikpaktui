use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

pub trait Backend {
    fn name(&self) -> &'static str;

    fn ls(&self, path: &str) -> Result<Vec<Entry>>;
    fn mv(&self, current_path: &str, name: &str, target_path: &str) -> Result<String>;
    fn cp(&self, current_path: &str, name: &str, target_path: &str) -> Result<String>;
    fn rename(&self, current_path: &str, old_name: &str, new_name: &str) -> Result<String>;
    fn remove(&self, current_path: &str, name: &str) -> Result<String>;
}
