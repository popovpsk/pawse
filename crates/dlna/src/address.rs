use url::{Position, Url};

pub(crate) fn join(base: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    Url::parse(base)
        .and_then(|base| base.join(reference))
        .ok()
        .map(String::from)
}

pub(crate) fn key(location: &str, target: &str) -> String {
    let (Ok(base), Ok(url)) = (Url::parse(location), Url::parse(target)) else {
        return target.to_string();
    };
    if url.scheme() != base.scheme() || url.host_str() != base.host_str() {
        return target.to_string();
    }
    let tail = &url[Position::BeforePath..];
    match url.port_or_known_default() {
        port if port == base.port_or_known_default() => tail.to_string(),
        Some(port) => format!(":{port}{tail}"),
        None => target.to_string(),
    }
}

pub(crate) fn identity(path: &str, size: Option<u64>) -> String {
    match size {
        Some(size) => format!("{path}#{size}"),
        None => path.to_string(),
    }
}

pub(crate) fn path(key: &str) -> &str {
    match key.rsplit_once('#') {
        Some((path, size)) if !size.is_empty() && size.bytes().all(|b| b.is_ascii_digit()) => path,
        _ => key,
    }
}

pub(crate) fn url(location: &str, key: &str) -> Option<String> {
    let key = path(key);
    let base = Url::parse(location).ok()?;
    if key.starts_with('/') {
        return base.join(key).ok().map(String::from);
    }
    if let Some(rest) = key.strip_prefix(':') {
        let split = rest.find('/')?;
        let port: u16 = rest[..split].parse().ok()?;
        let mut base = base;
        base.set_port(Some(port)).ok()?;
        return base.join(&rest[split..]).ok().map(String::from);
    }
    Url::parse(key).ok().map(String::from)
}

pub(crate) fn candidates(input: &str) -> Vec<String> {
    let input = input.trim();
    let input = if input.contains("://") {
        input.to_string()
    } else {
        format!("http://{input}")
    };
    let Ok(url) = Url::parse(&input) else {
        return vec![input];
    };
    if url.path() != "/" || url.query().is_some() {
        return vec![input];
    }
    ["rootDesc.xml", "description.xml", "DeviceDescription.xml"]
        .iter()
        .filter_map(|path| url.join(path).ok().map(String::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCATION: &str = "http://192.168.1.5:8200/rootDesc.xml";

    #[test]
    fn a_media_url_on_the_description_origin_keeps_only_its_path_and_query() {
        let key = key(LOCATION, "http://192.168.1.5:8200/MediaItems/12.flac?x=1");
        assert_eq!(key, "/MediaItems/12.flac?x=1");
        assert_eq!(
            url("http://10.0.0.9:8200/rootDesc.xml", &key).as_deref(),
            Some("http://10.0.0.9:8200/MediaItems/12.flac?x=1")
        );
    }

    #[test]
    fn a_media_url_on_another_port_of_the_same_host_keeps_the_port() {
        let key = key(
            "http://192.168.1.5:2869/upnphost/udhisapi.dll?content=uuid:1",
            "http://192.168.1.5:10243/WMPNSSv4/1/a.flac",
        );
        assert_eq!(key, ":10243/WMPNSSv4/1/a.flac");
        assert_eq!(
            url("http://10.0.0.9:2869/desc", &key).as_deref(),
            Some("http://10.0.0.9:10243/WMPNSSv4/1/a.flac")
        );
    }

    #[test]
    fn a_media_url_on_another_host_is_kept_whole() {
        let target = "http://cdn.example/a.mp3";
        assert_eq!(key(LOCATION, target), target);
        assert_eq!(url(LOCATION, target).as_deref(), Some(target));
    }

    #[test]
    fn relative_references_resolve_against_the_description() {
        assert_eq!(
            join(LOCATION, "/ctl/ContentDir").as_deref(),
            Some("http://192.168.1.5:8200/ctl/ContentDir")
        );
        assert_eq!(
            join("http://h:1/dev/desc.xml", "cds/control").as_deref(),
            Some("http://h:1/dev/cds/control")
        );
        assert_eq!(join(LOCATION, " "), None);
    }

    #[test]
    fn the_size_mark_tells_reused_addresses_apart_and_is_never_requested() {
        let key = identity("/MediaItems/455.flac", Some(315_151));
        assert_eq!(key, "/MediaItems/455.flac#315151");
        assert_ne!(key, identity("/MediaItems/455.flac", Some(312_140)));
        assert_eq!(
            url(LOCATION, &key).as_deref(),
            Some("http://192.168.1.5:8200/MediaItems/455.flac")
        );
        assert_eq!(identity("/a.mp3", None), "/a.mp3");
        assert_eq!(path("/a.mp3#frag"), "/a.mp3#frag");
        assert_eq!(path("/a.mp3#"), "/a.mp3#");
    }

    #[test]
    fn a_bare_address_is_tried_with_the_usual_description_paths() {
        assert_eq!(
            candidates("nas:8200"),
            vec![
                "http://nas:8200/rootDesc.xml".to_string(),
                "http://nas:8200/description.xml".to_string(),
                "http://nas:8200/DeviceDescription.xml".to_string(),
            ]
        );
        assert_eq!(
            candidates("http://nas:49152/description.xml"),
            vec!["http://nas:49152/description.xml".to_string()]
        );
    }
}
