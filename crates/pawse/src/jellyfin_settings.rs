use gpui::{App, AppContext, Entity, Window};
use ui_components::settings::SettingGroup;

use crate::library_sources::{ConnectState, LibrarySources, describe_error};
use crate::localization::tr;
use crate::remote_settings::{ServerInputs, added, jellyfin_remote, server_group, set_connecting};
use crate::servers::{RemoteConfig, RemoteError, ServerKind};
use crate::settings_store::{JellyfinServer, SettingsStore, notify_save_error};

fn new_device_id(url: &str, username: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let seed = format!("{url}\n{username}\n{nanos}\n{}", std::process::id());
    format!(
        "pawse-{}",
        &music_library::sha256_hex(seed.as_bytes())[..32]
    )
}

fn login(url: &str, username: &str, password: &str) -> Result<JellyfinServer, RemoteError> {
    let device_id = new_device_id(url, username);
    let config = crate::servers::authenticate_jellyfin(url, username, password, &device_id)?;
    RemoteConfig::Jellyfin(config.clone()).client().ping()?;
    Ok(JellyfinServer {
        url: config.url,
        username: username.to_string(),
        user_id: config.user_id,
        token: config.token,
        device_id: config.device_id,
    })
}

fn connect(
    sources: Entity<LibrarySources>,
    inputs: ServerInputs,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((url, username, password)) = inputs.read(cx) else {
        set_connecting(
            &sources,
            ServerKind::Jellyfin,
            ConnectState {
                connecting: false,
                error: Some(tr().server_fill_fields.clone()),
            },
            cx,
        );
        return;
    };
    set_connecting(
        &sources,
        ServerKind::Jellyfin,
        ConnectState {
            connecting: true,
            error: None,
        },
        cx,
    );
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { login(&url, &username, &password) })
            .await;
        cx.update(|cx| {
            let server = match result {
                Ok(server) => server,
                Err(e) => {
                    set_connecting(
                        &sources,
                        ServerKind::Jellyfin,
                        ConnectState {
                            connecting: false,
                            error: Some(describe_error(&e)),
                        },
                        cx,
                    );
                    return;
                }
            };
            set_connecting(&sources, ServerKind::Jellyfin, ConnectState::default(), cx);
            if let Err(e) = cx
                .global_mut::<SettingsStore>()
                .add_jellyfin_server(server.clone())
            {
                notify_save_error(cx, e);
            }
            added(jellyfin_remote(&server), cx);
            let _ = handle.update(cx, |_, window, cx| inputs.clear(window, cx));
        });
    })
    .detach();
}

pub fn jellyfin_group(sources: Entity<LibrarySources>, inputs: ServerInputs) -> SettingGroup {
    server_group(
        ServerKind::Jellyfin,
        tr().jellyfin_servers_desc.clone(),
        sources,
        inputs,
        connect,
    )
}
