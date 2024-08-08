use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

type TagID = String;

#[derive(Default, Eq, Debug, Deserialize, Clone, PartialEq)]
pub struct TagConf {
    pub uris: Vec<String>,
}

impl TagConf {
    pub fn is_empty(&self) -> bool {
        self.uris.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct TagMapper {
    file: String,
    conf: Arc<RwLock<TagMapperConfiguration>>,
}

#[derive(Debug, Clone)]
pub struct TagMapperHandle {
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
//     uris:
//       - foo.ogg
//       - bar.ogg
//

impl TagMapper {
    fn refresh(&mut self) -> Result<()> {
        let content = match fs::read_to_string(&self.file) {
            Ok(cnt) => cnt,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                debug!("No tag mapper configuration found");
                return Ok(());
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("Reading tag mapper configuration at '{}'", self.file)
                });
            }
        };

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

    pub fn handle(&self) -> TagMapperHandle {
        let conf = self.conf.clone();
        TagMapperHandle { conf }
    }

    pub fn new(filename: &str) -> Self {
        let empty_conf = Arc::new(RwLock::new(TagMapperConfiguration::new()));
        let tag_mapper = TagMapper {
            file: filename.to_string(),
            conf: empty_conf,
        };
        tag_mapper
    }

    pub fn new_initialized(filename: &str) -> Result<TagMapper> {
        let mut tag_mapper = Self::new(filename);
        tag_mapper.refresh()?;
        Ok(tag_mapper)
    }
}

impl TagMapperHandle {
    pub fn lookup(&self, tag_id: &TagID) -> Option<TagConf> {
        let r = self.conf.read().unwrap();
        return r.mappings.get(tag_id).cloned();
    }

    pub fn debug_dump(&self) {
        let r = self.conf.read().unwrap();
        r.debug_dump();
    }
}
