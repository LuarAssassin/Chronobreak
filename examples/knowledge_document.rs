#[path = "support/knowledge_document.rs"]
mod knowledge_document;

use knowledge_document::{Document, fixture};
use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments.next();
    let path = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Err(io::Error::other(
            "usage: knowledge_document <create|load> <explicit-file-path>",
        ));
    }
    let (Some(command), Some(path)) = (command, path) else {
        return Err(io::Error::other(
            "usage: knowledge_document <create|load> <explicit-file-path>",
        ));
    };
    let document = if command == "create" {
        let document = fixture();
        let replay = Document::parse(&document.serialize()?)?;
        replay.save_atomic(&path)?;
        let loaded = Document::load(&path)?;
        if loaded != document {
            return Err(io::Error::other("saved document differs from fixture"));
        }
        loaded
    } else if command == "load" {
        Document::load(&path)?
    } else {
        return Err(io::Error::other("expected create or load"));
    };
    println!(
        "Offline document, no LLM/network: schema {}, {} nodes, {} source bytes; validated {}",
        document.schema_version,
        document.nodes.len(),
        document.source.len(),
        path.display()
    );
    Ok(())
}
