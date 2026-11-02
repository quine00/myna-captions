//! What can be captioned: applications playing audio, output devices (their
//! monitor carries everything they play) and microphones.
//!
//! An application is followed across the nodes it creates: Firefox makes one
//! per media element and a player one per track, so a source is a key that
//! is resolved to a node again whenever the last one goes away.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An application's playback streams (`Stream/Output/Audio`).
    App,
    /// A device's mix, tapped at its monitor (`Audio/Sink`).
    Sink,
    /// A capture device (`Audio/Source`).
    Mic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Stable across the nodes an application creates.
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub running: bool,
}

/// One node in the graph, as a capture target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub key: String,
    pub serial: u64,
    pub kind: Kind,
    title: String,
    media: String,
    pub running: bool,
}

/// The nodes in a `pw-dump` document.
pub fn parse(dump: &str) -> Vec<Node> {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(dump) else {
        return Vec::new();
    };
    objects.iter().filter_map(node).collect()
}

fn node(object: &Value) -> Option<Node> {
    let info = object.get("info")?;
    let props = info.get("props")?;
    let prop = |k: &str| props.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let number = |k: &str| props.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
    let serial = number("object.serial")?;
    let running = info.get("state").and_then(Value::as_str) == Some("running");
    let (kind, key, title, media) = match prop("media.class")? {
        "Stream/Output/Audio" => {
            if prop("node.name") == Some("myna-captions") {
                return None;
            }
            let app = prop("application.name").or(prop("node.name")).unwrap_or("Unknown application");
            let process = number("application.process.id").map(|p| p.to_string()).unwrap_or_default();
            (Kind::App, format!("app:{app}:{process}"), app, prop("media.name").unwrap_or(""))
        }
        "Audio/Sink" => {
            let name = prop("node.name")?;
            (Kind::Sink, format!("sink:{name}"), prop("node.description").unwrap_or(name), "")
        }
        "Audio/Source" => {
            let name = prop("node.name")?;
            (Kind::Mic, format!("mic:{name}"), prop("node.description").unwrap_or(name), "")
        }
        _ => return None,
    };
    Some(Node { key, serial, kind, title: title.to_owned(), media: media.to_owned(), running })
}

/// Group nodes into sources: one per application, device or microphone.
pub fn sources(nodes: &[Node]) -> Vec<Source> {
    let mut out: Vec<Source> = Vec::new();
    for node in nodes {
        if let Some(source) = out.iter_mut().find(|s| s.key == node.key) {
            if node.running && !source.running {
                source.running = true;
                source.subtitle = node.media.clone();
            }
            continue;
        }
        let subtitle = match node.kind {
            Kind::App => node.media.clone(),
            Kind::Sink => "Everything playing on this output".into(),
            Kind::Mic => "Speech in the room".into(),
        };
        out.push(Source { key: node.key.clone(), kind: node.kind.clone(), title: node.title.clone(), subtitle, running: node.running });
    }
    let rank = |k: &Kind| match k {
        Kind::App => 0,
        Kind::Mic => 1,
        Kind::Sink => 2,
    };
    out.sort_by(|a, b| (rank(&a.kind), !a.running, &a.title).cmp(&(rank(&b.kind), !b.running, &b.title)));
    out
}

/// The node to tap for `key` now: a running one if there is one.
pub fn resolve(nodes: &[Node], key: &str) -> Option<Node> {
    let mut matching: Vec<&Node> = nodes.iter().filter(|n| n.key == key).collect();
    matching.sort_by_key(|n| (!n.running, std::cmp::Reverse(n.serial)));
    matching.first().map(|n| (*n).clone())
}

/// The current graph, through `pw-dump`.
pub fn list() -> Vec<Node> {
    std::process::Command::new("pw-dump")
        .output()
        .ok()
        .map(|out| parse(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id":1,"info":{"state":"suspended","props":{"media.class":"Audio/Sink","object.serial":10,"node.name":"spk","node.description":"Speakers"}}},
      {"id":2,"info":{"state":"idle","props":{"media.class":"Stream/Output/Audio","object.serial":11,"application.name":"Firefox","application.process.id":42,"media.name":"Old tab"}}},
      {"id":5,"info":{"state":"running","props":{"media.class":"Stream/Output/Audio","object.serial":15,"application.name":"Firefox","application.process.id":42,"media.name":"Meet"}}},
      {"id":3,"info":{"state":"running","props":{"media.class":"Audio/Source","object.serial":12,"node.name":"mic","node.description":"Headset"}}},
      {"id":6,"info":{"state":"running","props":{"media.class":"Stream/Output/Audio","object.serial":16,"node.name":"myna-captions"}}},
      {"id":4,"type":"PipeWire:Interface:Link"}
    ]"#;

    #[test]
    fn an_application_is_one_source_however_many_nodes() {
        let sources = sources(&parse(DUMP));
        let titles: Vec<_> = sources.iter().map(|s| (s.title.as_str(), s.subtitle.as_str())).collect();
        assert_eq!(titles, [("Firefox", "Meet"), ("Headset", "Speech in the room"), ("Speakers", "Everything playing on this output")]);
    }

    #[test]
    fn resolve_prefers_the_running_node() {
        let nodes = parse(DUMP);
        assert_eq!(resolve(&nodes, "app:Firefox:42").map(|n| n.serial), Some(15));
        assert_eq!(resolve(&nodes, "app:Gone:1"), None);
    }

    #[test]
    fn garbage_is_no_nodes() {
        assert!(parse("not json").is_empty());
    }
}
