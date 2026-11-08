//! Small bounded XML/OPC helpers shared by native visual readers.
use anyhow::{Context, Result, bail, ensure};
use quick_xml::{Reader, events::Event};
use std::{collections::HashMap, fs::File, io::Read};
use zip::ZipArchive;

#[derive(Default, Clone, Debug)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Node>,
    pub text: String,
}
impl Node {
    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|n| n.name == name)
    }
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Self> {
        self.children.iter().filter(move |n| n.name == name)
    }
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    pub fn local_attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.rsplit(':').next() == Some(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn descendant(&self, name: &str) -> Option<&Self> {
        self.children.iter().find_map(|n| {
            if n.name == name {
                Some(n)
            } else {
                n.descendant(name)
            }
        })
    }
}
pub(crate) fn parse(xml: &str) -> Result<Node> {
    ensure!(
        xml.len() <= 64 * 1024 * 1024,
        "Visual XML exceeds 64 MiB limit"
    );
    let mut reader = Reader::from_str(xml);
    let mut stack = vec![Node::default()];
    let mut nodes = 0;
    loop {
        let event = reader.read_event()?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                nodes += 1;
                ensure!(
                    nodes <= 1_000_000 && stack.len() < 128,
                    "Visual XML complexity limit exceeded"
                );
                let mut node = Node {
                    name: String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
                    ..Default::default()
                };
                for attr in e.attributes() {
                    let attr = attr?;
                    node.attrs.insert(
                        std::str::from_utf8(attr.key.as_ref())?.to_owned(),
                        quick_xml::escape::unescape(std::str::from_utf8(&attr.value)?)?
                            .into_owned(),
                    );
                }
                if empty {
                    stack.last_mut().context("XML parent")?.children.push(node)
                } else {
                    stack.push(node)
                }
            }
            Event::Text(t) => stack
                .last_mut()
                .context("XML text parent")?
                .text
                .push_str(&quick_xml::escape::unescape(&t.decode()?)?),
            Event::CData(t) => stack
                .last_mut()
                .context("XML text parent")?
                .text
                .push_str(&t.decode()?),
            Event::GeneralRef(t) => {
                let escaped = format!("&{};", t.decode()?);
                stack
                    .last_mut()
                    .context("XML text parent")?
                    .text
                    .push_str(&quick_xml::escape::unescape(&escaped)?);
            }
            Event::End(_) => {
                ensure!(stack.len() > 1, "Unbalanced visual XML");
                let node = stack.pop().context("XML element")?;
                stack.last_mut().context("XML parent")?.children.push(node);
            }
            Event::DocType(_) => bail!("Document types are not supported in workbook visual XML"),
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.len() == 1, "Unclosed visual XML");
    stack
        .pop()
        .context("XML document")?
        .children
        .into_iter()
        .next()
        .context("Empty XML")
}

pub(crate) fn part_bytes(zip: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("Missing visual part {name}"))?;
    ensure!(
        entry.size() <= limit,
        "Visual part {name} exceeds resource limit"
    );
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}
pub(crate) fn part_xml(zip: &mut ZipArchive<File>, name: &str) -> Result<String> {
    String::from_utf8(part_bytes(zip, name, 64 * 1024 * 1024)?)
        .context("Visual part is not UTF-8 XML")
}
pub(crate) fn relationship_part(owner: &str) -> String {
    let (folder, file) = owner.rsplit_once('/').unwrap_or(("", owner));
    if folder.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{folder}/_rels/{file}.rels")
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Relationship {
    pub target: String,
    pub kind: String,
    pub external: bool,
}
pub(crate) fn resolve_target(owner: &str, target: &str) -> Result<String> {
    ensure!(
        !target.contains('\\')
            && !target.contains(':')
            && !target.contains('?')
            && !target.contains('#'),
        "Invalid package relationship target"
    );
    let raw = if target.starts_with('/') {
        target.trim_start_matches('/').to_owned()
    } else {
        match owner.rsplit_once('/') {
            Some((folder, _)) => format!("{folder}/{target}"),
            None => target.to_owned(),
        }
    };
    let mut parts = vec![];
    for p in raw.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                ensure!(parts.pop().is_some(), "Package relationship escapes root");
            }
            _ => parts.push(p),
        }
    }
    ensure!(!parts.is_empty(), "Empty package relationship target");
    Ok(parts.join("/"))
}
pub(crate) fn resolve_relationships(
    owner: &str,
    xml: &str,
) -> Result<HashMap<String, Relationship>> {
    let root = parse(xml)?;
    let mut out = HashMap::new();
    for rel in root.children_named("Relationship") {
        let external = rel.attr("TargetMode") == Some("External");
        if let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) {
            out.insert(
                id.to_owned(),
                Relationship {
                    target: if external {
                        target.to_owned()
                    } else {
                        resolve_target(owner, target)?
                    },
                    kind: rel.attr("Type").unwrap_or("").to_owned(),
                    external,
                },
            );
        }
    }
    Ok(out)
}
pub(crate) fn read_relationships(
    zip: &mut ZipArchive<File>,
    owner: &str,
) -> Result<HashMap<String, Relationship>> {
    let path = relationship_part(owner);
    if zip.index_for_name(&path).is_none() {
        return Ok(HashMap::new());
    }
    resolve_relationships(owner, &part_xml(zip, &path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_paths_stay_inside_archive() {
        assert_eq!(
            resolve_target("xl/drawings/drawing1.xml", "../charts/chart1.xml").unwrap(),
            "xl/charts/chart1.xml"
        );
        assert!(resolve_target("xl/drawings/drawing1.xml", "../../../private.xml").is_err());
        assert!(resolve_target("xl/drawings/drawing1.xml", "file:///tmp/secrets").is_err());
    }
}
