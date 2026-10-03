use roxmltree::Node;

use crate::xml::LocalName;
use crate::{Device, address, xml};

const CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:";
const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:";
const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:";
const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Description {
    pub device: Device,
    pub control: String,
    pub service_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Service {
    pub control: String,
    pub service_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RendererDescription {
    pub device: Device,
    pub av_transport: Service,
    pub rendering_control: Option<Service>,
    pub connection_manager: Option<Service>,
}

pub(crate) fn parse(location: &str, text: &str) -> Result<Description, String> {
    let document = xml::parse(text)?;
    let root = document.root_element();
    let base = url_base(root, location);
    for device in root
        .descendants()
        .filter(|node| node.has_tag_name_local("device"))
    {
        let Some(service) = service(device, &base, CONTENT_DIRECTORY)? else {
            continue;
        };
        return Ok(Description {
            device: identity(device, location, "the media server has no UDN")?,
            control: service.control,
            service_type: service.service_type,
        });
    }
    Err("not a DLNA media server".into())
}

pub(crate) fn parse_renderer(location: &str, text: &str) -> Result<RendererDescription, String> {
    let document = xml::parse(text)?;
    let root = document.root_element();
    let base = url_base(root, location);
    for device in root
        .descendants()
        .filter(|node| node.has_tag_name_local("device"))
    {
        let Some(av_transport) = service(device, &base, AV_TRANSPORT)? else {
            continue;
        };
        return Ok(RendererDescription {
            device: identity(device, location, "the renderer has no UDN")?,
            av_transport,
            rendering_control: service(device, &base, RENDERING_CONTROL)?,
            connection_manager: service(device, &base, CONNECTION_MANAGER)?,
        });
    }
    Err("not a DLNA renderer".into())
}

fn url_base(root: Node<'_, '_>, location: &str) -> String {
    xml::child_text(root, "URLBase")
        .filter(|base| base.contains("://"))
        .unwrap_or(location)
        .to_string()
}

fn identity(device: Node<'_, '_>, location: &str, missing_udn: &str) -> Result<Device, String> {
    let udn = xml::child_text(device, "UDN")
        .map(str::trim)
        .filter(|udn| !udn.is_empty())
        .ok_or(missing_udn)?
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
    Ok(Device {
        udn,
        name,
        model,
        location: location.to_string(),
    })
}

fn service(device: Node<'_, '_>, base: &str, prefix: &str) -> Result<Option<Service>, String> {
    let Some(node) = device
        .children()
        .find(|node| node.has_tag_name_local("serviceList"))
        .and_then(|list| {
            list.children()
                .filter(|node| node.has_tag_name_local("service"))
                .find(|service| {
                    xml::child_text(*service, "serviceType")
                        .is_some_and(|kind| kind.trim().starts_with(prefix))
                })
        })
    else {
        return Ok(None);
    };
    let service_type = xml::child_text(node, "serviceType")
        .unwrap_or_default()
        .trim()
        .to_string();
    let control = xml::child_text(node, "controlURL")
        .and_then(|control| address::join(base, control))
        .ok_or_else(|| format!("the {service_type} service has no controlURL"))?;
    Ok(Some(Service {
        control,
        service_type,
    }))
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
