use roxmltree::Node;

use crate::xml::LocalName;
use crate::{Device, address, xml};

const CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Description {
    pub device: Device,
    pub control: String,
    pub service_type: String,
}

pub(crate) fn parse(location: &str, text: &str) -> Result<Description, String> {
    let document = xml::parse(text)?;
    let root = document.root_element();
    let base = xml::child_text(root, "URLBase")
        .filter(|base| base.contains("://"))
        .unwrap_or(location)
        .to_string();
    for device in root
        .descendants()
        .filter(|node| node.has_tag_name_local("device"))
    {
        let Some(service) = content_directory(device) else {
            continue;
        };
        let service_type = xml::child_text(service, "serviceType")
            .unwrap_or_default()
            .to_string();
        let control = xml::child_text(service, "controlURL")
            .and_then(|control| address::join(&base, control))
            .ok_or("the ContentDirectory service has no controlURL")?;
        let udn = xml::child_text(device, "UDN")
            .map(str::trim)
            .filter(|udn| !udn.is_empty())
            .ok_or("the media server has no UDN")?
            .to_string();
        let name = xml::child_text(device, "friendlyName")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&udn)
            .to_string();
        let model = xml::child_text(device, "modelName")
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        return Ok(Description {
            device: Device {
                udn,
                name,
                model,
                location: location.to_string(),
            },
            control,
            service_type,
        });
    }
    Err("not a DLNA media server".into())
}

fn content_directory<'a, 'i>(device: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    device
        .children()
        .find(|node| node.has_tag_name_local("serviceList"))?
        .children()
        .filter(|node| node.has_tag_name_local("service"))
        .find(|service| {
            xml::child_text(*service, "serviceType")
                .is_some_and(|kind| kind.trim().starts_with(CONTENT_DIRECTORY))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_directory_of_an_embedded_device_is_found() {
        let text = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>
    <UDN>uuid:router</UDN>
    <friendlyName>Router</friendlyName>
    <deviceList>
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
        <friendlyName> Keenetic Media </friendlyName>
        <modelName>DLNA</modelName>
        <UDN>uuid:media</UDN>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
            <controlURL>/cm</controlURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>
            <controlURL>cds/control</controlURL>
          </service>
        </serviceList>
      </device>
    </deviceList>
  </device>
</root>"#;
        let description = parse("http://10.0.0.1:5000/dev/desc.xml", text).unwrap();
        assert_eq!(description.device.udn, "uuid:media");
        assert_eq!(description.device.name, "Keenetic Media");
        assert_eq!(description.device.model.as_deref(), Some("DLNA"));
        assert_eq!(description.control, "http://10.0.0.1:5000/dev/cds/control");
        assert_eq!(
            description.service_type,
            "urn:schemas-upnp-org:service:ContentDirectory:1"
        );
    }

    #[test]
    fn url_base_wins_over_the_description_address() {
        let text = r#"<root><URLBase>http://10.0.0.2:9000/</URLBase><device><UDN>uuid:a</UDN>
<serviceList><service><serviceType>urn:schemas-upnp-org:service:ContentDirectory:2</serviceType>
<controlURL>ctl</controlURL></service></serviceList></device></root>"#;
        let description = parse("http://10.0.0.1/desc.xml", text).unwrap();
        assert_eq!(description.control, "http://10.0.0.2:9000/ctl");
        assert_eq!(description.device.name, "uuid:a");
    }

    #[test]
    fn a_device_without_a_content_directory_is_not_a_media_server() {
        let text = r#"<root><device><UDN>uuid:tv</UDN><serviceList><service>
<serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
<controlURL>/av</controlURL></service></serviceList></device></root>"#;
        assert!(parse("http://tv/desc.xml", text).is_err());
    }
}
