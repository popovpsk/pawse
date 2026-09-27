use std::collections::HashMap;

use server_http::Status;

use crate::xml::{self, LocalName};
use crate::{Error, read_text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    Upnp(u32, String),
    Other(Error),
}

impl Failure {
    pub(crate) fn into_error(self) -> Error {
        match self {
            Failure::Upnp(code, message) => Error::Server(format!("UPnP error {code}: {message}")),
            Failure::Other(error) => error,
        }
    }
}

pub(crate) type Args = HashMap<String, String>;

pub(crate) fn call(
    agent: &ureq::Agent,
    control: &str,
    service_type: &str,
    action: &str,
    args: &[(&str, &str)],
) -> Result<Args, Failure> {
    let body = envelope(service_type, action, args);
    let response = agent
        .post(control)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPACTION", format!("\"{service_type}#{action}\""))
        .send(body.as_str())
        .map_err(|e| Failure::Other(Error::Transient(e.to_string())))?;
    let status = response.status().as_u16();
    let text = read_text(response.into_body()).map_err(Failure::Other)?;
    if let Some(fault) = fault(&text) {
        return Err(fault);
    }
    match server_http::classify(status) {
        Status::Success => out_args(&text).map_err(|e| Failure::Other(Error::Server(e))),
        Status::Auth => Err(Failure::Other(Error::Auth)),
        Status::Transient => Err(Failure::Other(Error::Transient(format!("HTTP {status}")))),
        Status::Failed => Err(Failure::Other(Error::Server(format!("HTTP {status}")))),
    }
}

pub(crate) fn envelope(service_type: &str, action: &str, args: &[(&str, &str)]) -> String {
    let args: String = args
        .iter()
        .map(|(name, value)| format!("<{name}>{}</{name}>", xml::escape(value)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
<s:Body><u:{action} xmlns:u=\"{service_type}\">{args}</u:{action}></s:Body></s:Envelope>"
    )
}

fn body<'a, 'i>(document: &'a roxmltree::Document<'i>) -> Option<roxmltree::Node<'a, 'i>> {
    document
        .root_element()
        .children()
        .find(|node| node.has_tag_name_local("Body"))
}

pub(crate) fn fault(text: &str) -> Option<Failure> {
    let document = xml::parse(text).ok()?;
    let fault = body(&document)?
        .children()
        .find(|node| node.has_tag_name_local("Fault"))?;
    let upnp = fault
        .descendants()
        .find(|node| node.has_tag_name_local("UPnPError"));
    let code = upnp
        .and_then(|error| xml::child_text(error, "errorCode"))
        .and_then(|code| code.trim().parse().ok())
        .unwrap_or(0);
    let message = upnp
        .and_then(|error| xml::child_text(error, "errorDescription"))
        .or_else(|| xml::child_text(fault, "faultstring"))
        .unwrap_or("SOAP fault")
        .trim()
        .to_string();
    Some(Failure::Upnp(code, message))
}

pub(crate) fn out_args(text: &str) -> Result<Args, String> {
    let document = xml::parse(text)?;
    let response = body(&document)
        .and_then(|body| body.children().find(|node| node.is_element()))
        .ok_or("the SOAP response has no body")?;
    Ok(response
        .children()
        .filter(|node| node.is_element())
        .map(|node| {
            (
                node.tag_name().name().to_string(),
                xml::text(node).unwrap_or_default(),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

    #[test]
    fn arguments_are_escaped_into_the_envelope() {
        let envelope = envelope(SERVICE, "Search", &[("SearchCriteria", "a < \"b\" & c")]);
        assert!(envelope.contains(
            "<u:Search xmlns:u=\"urn:schemas-upnp-org:service:ContentDirectory:1\">\
<SearchCriteria>a &lt; &quot;b&quot; &amp; c</SearchCriteria></u:Search>"
        ));
    }

    #[test]
    fn the_escaped_result_comes_back_as_text() {
        let text = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body>
<u:BrowseResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
<Result>&lt;DIDL-Lite&gt;&lt;/DIDL-Lite&gt;</Result><NumberReturned>0</NumberReturned>
<TotalMatches>12</TotalMatches></u:BrowseResponse></s:Body></s:Envelope>"#;
        let args = out_args(text).unwrap();
        assert_eq!(args["Result"], "<DIDL-Lite></DIDL-Lite>");
        assert_eq!(args["TotalMatches"], "12");
        assert_eq!(fault(text), None);
    }

    #[test]
    fn a_upnp_error_is_read_from_the_fault() {
        let text = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault>
<faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail>
<UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>708</errorCode>
<errorDescription>Unsupported or invalid search criteria</errorDescription></UPnPError>
</detail></s:Fault></s:Body></s:Envelope>"#;
        assert_eq!(
            fault(text),
            Some(Failure::Upnp(
                708,
                "Unsupported or invalid search criteria".into()
            ))
        );
    }
}
