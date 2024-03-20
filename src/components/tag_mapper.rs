use anyhow::{Context, Result};
use serde::Deserialize;
use slog_scope::info;
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, RwLock};

type TagID = String;
type FilePath = String;

#[derive(Debug, Deserialize, Clone)]
pub struct TagConf {
    files: Vec<FilePath>,
}

pub struct TagMapper {
    file: String,
    conf: Arc<RwLock<TagMapperConfiguration>>,
}

#[derive(Debug, Deserialize)]
pub struct TagMapperConfiguration {
    mappings: HashMap<TagID, TagConf>,
}

impl TagMapperConfiguration {
    fn new() -> Self {
        let mappings = HashMap::new();
        TagMapperConfiguration { mappings }
    }

    fn debug_dump(&self) {
        for (key, value) in &self.mappings {
            info!("{} / {:?}", key, value);
        }
    }
}

// mappings:
//   12345:
//     files:
//       - foo.ogg
//       - bar.ogg
//

impl TagMapper {
    fn refresh(&mut self) -> Result<()> {
        let content = fs::read_to_string(&self.file)
            .with_context(|| format!("Reading tag_mapper configuration at {}", self.file))?;
        let conf: TagMapperConfiguration = serde_yaml::from_str(&content).with_context(|| {
            format!(
                "YAML unmarshalling tag_mapper configuration at {}",
                self.file
            )
        })?;
        let mut w = self.conf.write().unwrap();
        *w = conf;
        Ok(())
    }

    pub fn new(filename: &str) -> Self {
        let empty_conf = Arc::new(RwLock::new(TagMapperConfiguration::new()));
        let tag_mapper = TagMapper {
            file: filename.to_string(),
            conf: empty_conf,
        };
        tag_mapper
    }

    pub fn new_initialized(filename: &str) -> Result<Self> {
        let mut tag_mapper = Self::new(filename);
        tag_mapper.refresh()?;
        Ok(tag_mapper)
    }

    pub fn lookup(&self, tag_id: &TagID) -> Option<TagConf> {
        let r = self.conf.read().unwrap();
        return r.mappings.get(tag_id).cloned();
    }

    pub fn debug_dump(&self) {
        let r = self.conf.read().unwrap();
        r.debug_dump();
    }
}
