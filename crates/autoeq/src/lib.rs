use std::time::Duration;

use ureq::Agent;

const RESULTS_URL: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/";
const INDEX_FILE: &str = "INDEX.md";
const PROFILE_SUFFIX: &str = "%20ParametricEQ.txt";
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CACHE_NAME_BYTES: usize = 200;
const CACHE_NAME_CHARS: &str = "%-_.()&,+'!$";
const USER_AGENT: &str = concat!(
    "Pawse/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/popovpsk/pawse )"
);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request failed: {0}")]
    Transport(String),
    #[error("HTTP {0}")]
    Status(u16),
    #[error("the AutoEq list is empty")]
    EmptyIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub source: String,
    pub rig: Option<String>,
    path: String,
}

impl Entry {
    pub fn cache_name(&self) -> Option<String> {
        let name = format!("{}.txt", self.path.replace('/', "%2F"));
        let safe = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || CACHE_NAME_CHARS.contains(c));
        (safe && name.len() <= MAX_CACHE_NAME_BYTES).then_some(name)
    }

    fn url(&self) -> String {
        let folder = self.path.rsplit('/').next().unwrap_or_default();
        format!("{RESULTS_URL}{}/{folder}{PROFILE_SUFFIX}", self.path)
    }
}

pub fn parse_index(text: &str) -> Vec<Entry> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Entry> {
    let rest = line.trim().strip_prefix("- [")?;
    let (name, rest) = rest.split_once("](")?;
    let (path, by) = rest.rsplit_once(") by ")?;
    let path = path.strip_prefix("./")?.trim_end_matches('/');
    if name.is_empty() || path.is_empty() || path.contains("..") {
        return None;
    }
    let by = by.trim();
    let (source, rig) = match by.split_once(" on ") {
        Some((source, rig)) => (source, Some(rig.to_string())),
        None => (by, None),
    };
    Some(Entry {
        name: name.to_string(),
        source: source.to_string(),
        rig,
        path: path.to_string(),
    })
}

#[derive(Clone)]
pub struct Client {
    agent: Agent,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let agent = Agent::new_with_config(
            Agent::config_builder()
                .http_status_as_error(false)
                .user_agent(USER_AGENT)
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(15)))
                .timeout_recv_body(Some(Duration::from_secs(30)))
                .build(),
        );
        Self { agent }
    }

    pub fn index(&self) -> Result<Vec<Entry>, Error> {
        let entries = parse_index(&self.get(&format!("{RESULTS_URL}{INDEX_FILE}"))?);
        if entries.is_empty() {
            return Err(Error::EmptyIndex);
        }
        Ok(entries)
    }

    pub fn parametric_eq(&self, entry: &Entry) -> Result<String, Error> {
        self.get(&entry.url())
    }

    fn get(&self, url: &str) -> Result<String, Error> {
        let mut response = self
            .agent
            .get(url)
            .call()
            .map_err(|e| Error::Transport(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(Error::Status(status));
        }
        response
            .body_mut()
            .with_config()
            .limit(MAX_INDEX_BYTES)
            .read_to_string()
            .map_err(|e| Error::Transport(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "# Index
This is a list of all equalization profiles.

- [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990
- [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045
- [1MORE Quad Driver](./HypetheSonics/Bruel%20&%20Kjaer%205128%20in-ear/1MORE%20Quad%20Driver) by HypetheSonics on Bruel & Kjaer 5128
- [Broken](no-dot-slash) by nobody
- [Escape](./../etc) by nobody
";

    #[test]
    fn reads_index_lines() {
        let entries = parse_index(INDEX);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "Sennheiser HD 600");
        assert_eq!(entries[0].source, "oratory1990");
        assert_eq!(entries[0].rig, None);
        assert_eq!(entries[1].name, "1MORE Aero (ANC Off)");
        assert_eq!(entries[2].rig.as_deref(), Some("Bruel & Kjaer 5128"));
    }

    #[test]
    fn cache_names_are_flat_and_safe() {
        let entries = parse_index(INDEX);
        assert_eq!(
            entries[0].cache_name().as_deref(),
            Some("oratory1990%2Fover-ear%2FSennheiser%20HD%20600.txt")
        );
        assert!(entries[2].cache_name().is_some());
        let hostile = parse_index("- [X](./a/b:c) by y\n- [Y](./a/C%3A\\\\x) by y\n");
        assert_eq!(hostile.len(), 2);
        assert!(hostile.iter().all(|entry| entry.cache_name().is_none()));
    }

    #[test]
    fn profile_url_repeats_the_folder_name() {
        let entries = parse_index(INDEX);
        assert_eq!(
            entries[0].url(),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/oratory1990/over-ear/Sennheiser%20HD%20600/Sennheiser%20HD%20600%20ParametricEQ.txt"
        );
        assert_eq!(
            entries[1].url(),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)/1MORE%20Aero%20(ANC%20Off)%20ParametricEQ.txt"
        );
    }
}
