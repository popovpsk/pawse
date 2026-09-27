use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    create_torrent,
};

use crate::{Config, Engine, Network, Upload};

pub struct Seeder {
    _session: Arc<Session>,
    _runtime: tokio::runtime::Runtime,
    pub addr: SocketAddr,
    pub torrent: Vec<u8>,
}

pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn local_options(port: u16) -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        listen: Some(ListenerOptions {
            listen_addr: ([127, 0, 0, 1], port).into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

impl Seeder {
    pub fn new(content: &Path, piece_length: u32) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let port = free_port();
        let name = content
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (session, torrent) = runtime.block_on(async {
            let created = create_torrent(
                content,
                CreateTorrentOptions {
                    name: Some(&name),
                    trackers: Vec::new(),
                    piece_length: Some(piece_length),
                },
                &BlockingSpawner::new(2),
            )
            .await
            .unwrap();
            let bytes = created.as_bytes().unwrap();
            let session = Session::new_with_opts(content.to_path_buf(), local_options(port))
                .await
                .unwrap();
            let handle = session
                .add_torrent(
                    AddTorrent::from_bytes(bytes.clone()),
                    Some(AddTorrentOptions {
                        output_folder: Some(content.to_string_lossy().into_owned()),
                        overwrite: true,
                        ..Default::default()
                    }),
                )
                .await
                .unwrap()
                .into_handle()
                .unwrap();
            handle.wait_until_initialized().await.unwrap();
            assert!(
                handle.stats().finished,
                "the seeder does not have the content"
            );
            (session, bytes.to_vec())
        });
        Self {
            _session: session,
            _runtime: runtime,
            addr: ([127, 0, 0, 1], port).into(),
            torrent,
        }
    }

    pub fn engine(&self, root: &Path) -> Engine {
        Engine::new(Config {
            work_dir: root.join("work"),
            state_dir: root.join("state"),
            upload: Upload::WhileActive,
            idle_unload: Duration::from_secs(600),
            work_limit_bytes: u64::MAX,
            network: Network::Local {
                listen_port: free_port(),
                peers: vec![self.addr],
            },
        })
        .unwrap()
    }
}
