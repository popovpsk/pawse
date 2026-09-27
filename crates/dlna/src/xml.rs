use roxmltree::{Document, Node, ParsingOptions};

pub(crate) fn parse(text: &str) -> Result<Document<'_>, String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: true,
            ..ParsingOptions::default()
        },
    )
    .map_err(|e| format!("malformed XML: {e}"))
}

pub(crate) trait LocalName {
    fn has_tag_name_local(&self, name: &str) -> bool;
}

impl LocalName for Node<'_, '_> {
    fn has_tag_name_local(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name().eq_ignore_ascii_case(name)
    }
}

pub(crate) fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|child| child.has_tag_name_local(name))
}

pub(crate) fn child_text<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|child| child.text())
}

pub(crate) fn text(node: Node<'_, '_>) -> Option<String> {
    let text: String = node
        .descendants()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}
