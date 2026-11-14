use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_FILE_BYTES: usize = 256 * 1024;
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_NODES: usize = 128;
pub const MAX_PARAGRAPHS: usize = 1024;
pub const MAX_DEPTH: usize = 32;
const MAX_REFS: usize = 16;
const MAX_TITLE_BYTES: usize = 256;
const MAX_ID_BYTES: usize = 64;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub source: String,
    pub paragraphs: Vec<ByteRange>,
    pub root: String,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub paragraph: usize,
    pub bytes: ByteRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    pub expanded: bool,
    pub source_refs: Vec<SourceRef>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

pub fn paragraphs(source: &str) -> Vec<ByteRange> {
    let mut offset = 0;
    source
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            ByteRange { start, end: offset }
        })
        .collect()
}

impl Document {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported schema version"));
        }
        if self.source.len() > MAX_SOURCE_BYTES
            || self.paragraphs.len() > MAX_PARAGRAPHS
            || self.nodes.is_empty()
            || self.nodes.len() > MAX_NODES
        {
            return Err(invalid("document size limit exceeded"));
        }
        if self.paragraphs != paragraphs(&self.source) {
            return Err(invalid(
                "paragraph ranges must exactly partition source lines",
            ));
        }
        let mut nodes = HashMap::new();
        for node in &self.nodes {
            if !valid_id(&node.id) || nodes.insert(node.id.as_str(), node).is_some() {
                return Err(invalid("invalid or duplicate node id"));
            }
            if node.title.is_empty()
                || node.title.len() > MAX_TITLE_BYTES
                || node.source_refs.len() > MAX_REFS
            {
                return Err(invalid("node text or reference limit exceeded"));
            }
            for reference in &node.source_refs {
                let Some(paragraph) = self.paragraphs.get(reference.paragraph) else {
                    return Err(invalid("dangling paragraph reference"));
                };
                let bytes = &reference.bytes;
                if bytes.start < paragraph.start
                    || bytes.end > paragraph.end
                    || bytes.start >= bytes.end
                    || self.source.get(bytes.start..bytes.end).is_none()
                {
                    return Err(invalid("invalid UTF-8 source byte range"));
                }
            }
        }
        let Some(root) = nodes.get(self.root.as_str()) else {
            return Err(invalid("missing root"));
        };
        if root.parent.is_some() {
            return Err(invalid("root must not have a parent"));
        }
        for node in &self.nodes {
            let mut current = node;
            let mut visited = HashSet::new();
            loop {
                if !visited.insert(current.id.as_str()) {
                    return Err(invalid("tree cycle"));
                }
                if visited.len() > MAX_DEPTH {
                    return Err(invalid("tree depth limit exceeded"));
                }
                match current.parent.as_deref() {
                    Some(parent) => {
                        current = nodes
                            .get(parent)
                            .ok_or_else(|| invalid("dangling parent"))?;
                    }
                    None if current.id == self.root => break,
                    None => return Err(invalid("multiple roots")),
                }
            }
        }
        Ok(())
    }

    pub fn serialize(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| invalid("JSON encoding failed"))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(invalid("encoded document exceeds file limit"));
        }
        Ok(bytes)
    }

    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(invalid("document exceeds file limit"));
        }
        let document: Self =
            serde_json::from_slice(bytes).map_err(|_| invalid("invalid document JSON"))?;
        document.validate()?;
        Ok(document)
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::parse(&bytes)
    }

    pub fn save_atomic(&self, path: &Path) -> io::Result<()> {
        self.save_with(path, |_| Ok(()))
    }

    fn save_with(
        &self,
        path: &Path,
        mut before: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()> {
        let bytes = self.serialize()?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = path
            .file_name()
            .ok_or_else(|| invalid("explicit file path required"))?;
        let mut temporary = None;
        for _ in 0..32 {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let mut temp_name = name.to_os_string();
            temp_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
            let temp_path = parent.join(temp_name);
            before("create")?;
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
            {
                Ok(file) => {
                    temporary = Some((TempFile(temp_path), file));
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        let (temporary, mut file) = temporary.ok_or_else(|| {
            io::Error::new(io::ErrorKind::AlreadyExists, "temporary name collisions")
        })?;
        before("write")?;
        let midpoint = bytes.len() / 2;
        file.write_all(&bytes[..midpoint])?;
        before("partial-write")?;
        file.write_all(&bytes[midpoint..])?;
        before("sync")?;
        file.sync_all()?;
        drop(file);
        before("rename")?;
        fs::rename(&temporary.0, path)?;
        Ok(())
    }
}

struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn fixture() -> Document {
    let source = "Synthetic notes — 离线示例\r\n\nPlants need light. 🌱\nWater supports growth.\n"
        .to_owned();
    let paragraphs = paragraphs(&source);
    Document {
        schema_version: 1,
        source,
        paragraphs: paragraphs.clone(),
        root: "notes".into(),
        nodes: vec![
            Node {
                id: "notes".into(),
                parent: None,
                title: "Offline synthetic notes (no LLM)".into(),
                expanded: true,
                source_refs: vec![],
            },
            Node {
                id: "light".into(),
                parent: Some("notes".into()),
                title: "Light".into(),
                expanded: false,
                source_refs: vec![SourceRef {
                    paragraph: 2,
                    bytes: paragraphs[2].clone(),
                }],
            },
            Node {
                id: "water".into(),
                parent: Some("notes".into()),
                title: "Water".into(),
                expanded: true,
                source_refs: vec![SourceRef {
                    paragraph: 3,
                    bytes: paragraphs[3].clone(),
                }],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "chronobreak-knowledge-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn unicode_roundtrip_and_disk_restore_preserve_every_field() {
        let original = fixture();
        assert_eq!(
            Document::parse(&original.serialize().unwrap()).unwrap(),
            original
        );
        let directory = TestDirectory::new();
        let path = directory.0.join("notes.json");
        original.save_atomic(&path).unwrap();
        assert_eq!(Document::load(&path).unwrap(), original);
        let mut changed = original.clone();
        changed.nodes[1].expanded = true;
        changed.save_atomic(&path).unwrap();
        assert_eq!(Document::load(&path).unwrap(), changed);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn exact_source_and_limit_boundaries() {
        for source in [
            "",
            "\n",
            "\r\n\n",
            "e\u{301} 中文 🌱\0\r\nlast",
            "no final newline",
        ] {
            let mut doc = fixture();
            doc.source = source.into();
            doc.paragraphs = paragraphs(source);
            doc.nodes.truncate(1);
            assert_eq!(Document::parse(&doc.serialize().unwrap()).unwrap(), doc);
        }
        let mut doc = fixture();
        doc.nodes.truncate(1);
        doc.source = "x".repeat(MAX_SOURCE_BYTES);
        doc.paragraphs = paragraphs(&doc.source);
        assert!(doc.serialize().is_ok());
        doc.source = "\0".repeat(MAX_SOURCE_BYTES);
        doc.paragraphs = paragraphs(&doc.source);
        assert!(doc.validate().is_ok());
        assert!(doc.serialize().is_err());
        doc.source = "\n".repeat(MAX_PARAGRAPHS);
        doc.paragraphs = paragraphs(&doc.source);
        assert!(doc.serialize().is_ok());
        for index in 1..MAX_NODES {
            doc.nodes.push(Node {
                id: format!("node-{index}"),
                parent: Some(doc.root.clone()),
                title: "Leaf".into(),
                expanded: false,
                source_refs: vec![],
            });
        }
        assert!(doc.serialize().is_ok());
        doc.nodes.truncate(MAX_DEPTH);
        for index in 1..MAX_DEPTH {
            doc.nodes[index].parent = Some(doc.nodes[index - 1].id.clone());
        }
        assert!(doc.serialize().is_ok());
    }

    #[test]
    fn rejects_invalid_models() {
        let mutations: Vec<fn(&mut Document)> = vec![
            |doc| doc.schema_version = 2,
            |doc| doc.nodes.push(doc.nodes[0].clone()),
            |doc| doc.root = "absent".into(),
            |doc| doc.nodes[0].parent = Some("light".into()),
            |doc| doc.nodes[1].parent = None,
            |doc| doc.nodes[1].parent = Some("absent".into()),
            |doc| {
                doc.nodes[1].parent = Some("water".into());
                doc.nodes[2].parent = Some("light".into());
            },
            |doc| doc.nodes[1].source_refs[0].paragraph = 99,
            |doc| doc.nodes[1].source_refs[0].bytes.start = 0,
            |doc| doc.nodes[1].source_refs[0].bytes.end = usize::MAX,
            |doc| doc.nodes[1].source_refs[0].bytes.end -= 2,
            |doc| doc.nodes[1].source_refs[0].bytes.start = doc.nodes[1].source_refs[0].bytes.end,
            |doc| doc.paragraphs[0].end -= 1,
            |doc| doc.nodes.clear(),
            |doc| doc.nodes[1].id = "bad id".into(),
            |doc| doc.nodes[1].id = "x".repeat(MAX_ID_BYTES + 1),
            |doc| doc.nodes[1].title = "x".repeat(MAX_TITLE_BYTES + 1),
            |doc| {
                doc.nodes[1].source_refs = vec![doc.nodes[1].source_refs[0].clone(); MAX_REFS + 1]
            },
            |doc| doc.source = "x".repeat(MAX_SOURCE_BYTES + 1),
            |doc| doc.paragraphs = vec![ByteRange { start: 0, end: 1 }; MAX_PARAGRAPHS + 1],
            |doc| doc.nodes = vec![doc.nodes[0].clone(); MAX_NODES + 1],
        ];
        for (index, mutate) in mutations.into_iter().enumerate() {
            let mut doc = fixture();
            mutate(&mut doc);
            assert!(doc.validate().is_err(), "mutation {index}");
            assert!(doc.serialize().is_err(), "serialize mutation {index}");
        }
        let mut doc = fixture();
        for index in 0..MAX_DEPTH {
            let parent = doc.nodes.last().unwrap().id.clone();
            doc.nodes.push(Node {
                id: format!("depth-{index}"),
                parent: Some(parent),
                title: "Depth".into(),
                expanded: true,
                source_refs: vec![],
            });
        }
        assert!(doc.validate().is_err());
    }

    #[test]
    fn rejects_unknown_fields_corruption_and_oversized_input() {
        let mut value = serde_json::to_value(fixture()).unwrap();
        for location in ["document", "node", "reference", "range", "paragraph"] {
            let mut changed = value.clone();
            let object = match location {
                "document" => &mut changed,
                "node" => &mut changed["nodes"][1],
                "reference" => &mut changed["nodes"][1]["source_refs"][0],
                "range" => &mut changed["nodes"][1]["source_refs"][0]["bytes"],
                _ => &mut changed["paragraphs"][0],
            };
            object["unexpected"] = true.into();
            assert!(Document::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        value["schema_version"] = 8.into();
        assert!(Document::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        for bytes in [b"{".as_slice(), b"\xff", b"{}", b"null"] {
            assert!(Document::parse(bytes).is_err());
        }
        assert!(Document::parse(&vec![b' '; MAX_FILE_BYTES + 1]).is_err());
        let directory = TestDirectory::new();
        let path = directory.0.join("bad.json");
        fs::write(&path, vec![b' '; MAX_FILE_BYTES + 1]).unwrap();
        assert!(Document::load(&path).is_err());
        fs::write(&path, b"{broken").unwrap();
        assert!(Document::load(&path).is_err());
    }

    #[test]
    fn every_pre_rename_failure_preserves_previous_file_and_cleans_temp() {
        let directory = TestDirectory::new();
        let path = directory.0.join("notes.json");
        let original = fixture();
        original.save_atomic(&path).unwrap();
        let previous = fs::read(&path).unwrap();
        let mut changed = original.clone();
        changed.nodes[1].expanded = true;
        for stage in ["create", "write", "partial-write", "sync", "rename"] {
            let result = changed.save_with(&path, |current| {
                if current == stage {
                    Err(io::Error::other("injected storage failure"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err(), "{stage}");
            assert_eq!(fs::read(&path).unwrap(), previous, "{stage}");
            assert_eq!(Document::load(&path).unwrap(), original);
            assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1, "{stage}");
        }
        changed.schema_version = 99;
        assert!(changed.save_atomic(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        assert!(
            original
                .save_atomic(&directory.0.join("missing/notes.json"))
                .is_err()
        );
        let destination_directory = directory.0.join("destination");
        fs::create_dir(&destination_directory).unwrap();
        assert!(original.save_atomic(&destination_directory).is_err());
        assert!(destination_directory.is_dir());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }
}
