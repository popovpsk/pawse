use std::time::{Duration, Instant};

use crate::testing::FakeChromecast;
use crate::*;

fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn launch_load_and_control_playback() {
    let fake = FakeChromecast::start();
    let (client, events) = Client::connect(fake.address).unwrap();
    let app = client.launch(DEFAULT_MEDIA_RECEIVER).unwrap();
    assert_eq!(app.transport_id, "transport-1");
    assert!(fake.state().launched);

    let media = Media {
        url: "http://127.0.0.1:9/never-fetched".into(),
        content_type: "audio/flac".into(),
        title: Some("Song".into()),
        artist: Some("Band".into()),
        album: None,
        image: Some("http://127.0.0.1:9/cover.jpg".into()),
        duration: Some(30.0),
    };
    let status = client.load(&app, &media, false, 12.5).unwrap().unwrap();
    assert_eq!(status.state, PlayerState::Paused);
    let loaded = fake.state().loaded.unwrap();
    assert_eq!(loaded.content_type, "audio/flac");
    assert_eq!(loaded.title.as_deref(), Some("Song"));
    assert_eq!(
        loaded.image.as_deref(),
        Some("http://127.0.0.1:9/cover.jpg")
    );
    assert!(!loaded.autoplay);
    assert_eq!(loaded.start, 12.5);

    client.play(&app, status.media_session_id).unwrap();
    client.seek(&app, status.media_session_id, 20.0).unwrap();
    client.pause(&app, status.media_session_id).unwrap();
    let status = client.media_status(&app).unwrap().remove(0);
    assert_eq!(status.state, PlayerState::Paused);
    assert!(status.current_time.unwrap() >= 20.0);
    assert_eq!(status.duration, Some(30.0));

    client.set_volume(0.25).unwrap();
    assert_eq!(fake.state().volume, 0.25);

    fake.finish_track();
    let status = client.media_status(&app).unwrap().remove(0);
    assert_eq!(status.state, PlayerState::Idle);
    assert_eq!(status.idle_reason, Some(IdleReason::Finished));

    client.stop_app(&app).unwrap();
    assert!(!fake.state().launched);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Media(_)))
    );
}

#[test]
fn an_already_running_receiver_app_is_joined_not_relaunched() {
    let fake = FakeChromecast::start();
    let (first, _) = Client::connect(fake.address).unwrap();
    first.launch(DEFAULT_MEDIA_RECEIVER).unwrap();
    let (second, _) = Client::connect(fake.address).unwrap();
    second.launch(DEFAULT_MEDIA_RECEIVER).unwrap();
    let launches = fake
        .state()
        .commands
        .iter()
        .filter(|command| *command == "LAUNCH")
        .count();
    assert_eq!(launches, 1);
}

#[test]
fn a_dropped_connection_is_reported() {
    let fake = FakeChromecast::start();
    let (client, events) = Client::connect(fake.address).unwrap();
    client.receiver_status().unwrap();
    fake.disconnect_everyone();
    assert!(wait_for(|| client.is_closed()));
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Disconnected(_)))
    );
    assert!(client.receiver_status().is_err());
}

#[test]
fn a_device_that_does_not_listen_fails_to_connect() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    assert!(matches!(Client::connect(address), Err(Error::Io(_))));
}

#[test]
fn devices_come_from_the_mdns_record() {
    let txt = |key: &str| match key {
        "id" => Some("abc123"),
        "fn" => Some(" Living Room "),
        "md" => Some("Chromecast Audio"),
        "ca" => Some("2052"),
        _ => None,
    };
    let device = Device::from_service(["10.0.0.9".parse().unwrap()], 8009, txt).unwrap();
    assert_eq!(device.name, "Living Room");
    assert_eq!(device.address, "10.0.0.9:8009".parse().unwrap());
    let video_only = |key: &str| match key {
        "id" => Some("x"),
        "ca" => Some("1"),
        _ => None,
    };
    assert!(Device::from_service(["10.0.0.9".parse().unwrap()], 8009, video_only).is_none());
}
