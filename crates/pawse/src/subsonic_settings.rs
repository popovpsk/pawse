use gpui::{App, AppContext, Entity, Window};
use ui_components::settings::SettingGroup;

use crate::library_sources::{ConnectState, LibrarySources, describe_error};
use crate::localization::tr;
use crate::remote_settings::{ServerInputs, added, server_group, set_connecting, subsonic_remote};
use crate::servers::{RemoteConfig, ServerKind};
use crate::settings_store::{SettingsStore, SubsonicServer, notify_save_error};

fn connect(
    sources: Entity<LibrarySources>,
    inputs: ServerInputs,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((url, username, password)) = inputs.read(cx) else {
        set_connecting(
            &sources,
            ServerKind::Subsonic,
            ConnectState {
                connecting: false,
                error: Some(tr().server_fill_fields.clone()),
            },
            cx,
        );
        return;
    };
    let server = SubsonicServer {
        url,
        username,
        password,
    };
    set_connecting(
        &sources,
        ServerKind::Subsonic,
        ConnectState {
            connecting: true,
            error: None,
        },
        cx,
    );
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let config = server.config();
        let result = cx
            .background_spawn(async move { RemoteConfig::Subsonic(config).client().ping() })
            .await;
        cx.update(|cx| {
            let error = result.as_ref().err().map(describe_error);
            let failed = error.is_some();
            set_connecting(
                &sources,
                ServerKind::Subsonic,
                ConnectState {
                    connecting: false,
                    error,
                },
                cx,
            );
            if failed {
                return;
            }
            if let Err(e) = cx
                .global_mut::<SettingsStore>()
                .add_subsonic_server(server.clone())
            {
                notify_save_error(cx, e);
            }
            added(subsonic_remote(&server), cx);
            let _ = handle.update(cx, |_, window, cx| inputs.clear(window, cx));
        });
    })
    .detach();
}

pub fn subsonic_group(sources: Entity<LibrarySources>, inputs: ServerInputs) -> SettingGroup {
    server_group(
        ServerKind::Subsonic,
        Some(tr().subsonic_servers_desc.clone()),
        sources,
        inputs,
        connect,
    )
}
