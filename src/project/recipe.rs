use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Recipe {
    #[serde(skip)]
    pub name: String,

    #[serde(skip)]
    pub cookbook: String,

    #[serde(skip)]
    pub config_path: PathBuf,
    pub description: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub run: String,
}

impl Recipe {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.cookbook, self.name)
    }
}
