//! The PipeWire nodes worth captioning: application output streams and the
//! sinks whose monitor carries a whole device's mix.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One application's playback stream (`Stream/Output/Audio`).
    Stream,
    /// A device's mix, tapped at its monitor (`Audio/Sink`).
    Sink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// `object.serial`, unique for the graph's lifetime, unlike the reused id.
    pub serial: u64,
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub running: bool,
}

/// The nodes in a `pw-dump` document, streams first.
pub fn parse(dump: &str) -> Vec<Node> {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(dump) else {
        return Vec::new();
    };
    let mut nodes: Vec<Node> = objects.iter().filter_map(node).collect();
    nodes.sort_by(|a, b| {
        (a.kind != Kind::Stream, !a.running, &a.title).cmp(&(b.kind != Kind::Stream, !b.running, &b.title))
    });
    nodes
}

fn node(object: &Value) -> Option<Node> {
    let info = object.get("info")?;
    let props = info.get("props")?;
    let prop = |k: &str| props.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let kind = match prop("media.class")? {
        "Stream/Output/Audio" => Kind::Stream,
        "Audio/Sink" => Kind::Sink,
        _ => return None,
    };
    let serial = props.get("object.serial")?.as_u64()?;
    let (title, subtitle) = match kind {
        Kind::Stream => (
            prop("application.name").or(prop("node.name")).unwrap_or("Unknown application"),
            prop("media.name").unwrap_or(""),
        ),
        Kind::Sink => (
            prop("node.description").or(prop("node.name")).unwrap_or("Unknown output"),
            "Everything playing on this output",
        ),
    };
    Some(Node {
        serial,
        kind,
        title: title.to_owned(),
        subtitle: subtitle.to_owned(),
        running: info.get("state").and_then(Value::as_str) == Some("running"),
    })
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

    #[test]
    fn keeps_streams_and_sinks_streams_first() {
        let dump = r#"[
          {"id":1,"info":{"state":"suspended","props":{"media.class":"Audio/Sink","object.serial":10,"node.description":"Speakers"}}},
          {"id":2,"info":{"state":"running","props":{"media.class":"Stream/Output/Audio","object.serial":11,"application.name":"Firefox","media.name":"Meet"}}},
          {"id":3,"info":{"state":"running","props":{"media.class":"Audio/Source","object.serial":12}}},
          {"id":4,"type":"PipeWire:Interface:Link"}
        ]"#;
        let nodes = parse(dump);
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].serial, nodes[0].title.as_str(), nodes[0].subtitle.as_str()), (11, "Firefox", "Meet"));
        assert_eq!(nodes[1].kind, Kind::Sink);
    }

    #[test]
    fn garbage_is_no_nodes() {
        assert!(parse("not json").is_empty());
    }
}
