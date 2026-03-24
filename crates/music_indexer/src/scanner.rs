use std::path::PathBuf;
use std::thread;
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender};

pub struct DirEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub is_dir: bool,
}

pub struct FileRecord {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub modified: SystemTime,
}

pub struct Scanner {
    root_path: PathBuf,
    num_threads: usize,
    batch_size: usize,
}

impl Scanner {
    pub fn new(root_path: PathBuf, num_threads: usize, batch_size: usize) -> Self {
        Self {
            root_path,
            num_threads,
            batch_size,
        }
    }

    pub fn scan(self) {
        let (entry_tx, entry_rx) = crossbeam_channel::unbounded::<Vec<DirEntry>>();
        let (record_tx, record_rx) = crossbeam_channel::unbounded::<Vec<FileRecord>>();

        let scanner_handle = self.spawn_scanner_thread(entry_tx);
        let metadata_handles = self.spawn_metadata_threads(entry_rx, record_tx);
        let db_handle = self.spawn_db_thread(record_rx);

        let _ = scanner_handle.join();
        for handle in metadata_handles {
            let _ = handle.join();
        }
        let _ = db_handle.join();
    }

    fn spawn_scanner_thread(
        &self,
        entry_tx: Sender<Vec<DirEntry>>,
    ) -> thread::JoinHandle<()> {
        let root = self.root_path.clone();

        thread::spawn(move || {
            scan_directory_recursive(root, &entry_tx);
            drop(entry_tx);
        })
    }

    fn spawn_metadata_threads(
        &self,
        entry_rx: Receiver<Vec<DirEntry>>,
        record_tx: Sender<Vec<FileRecord>>,
    ) -> Vec<thread::JoinHandle<()>> {
        let mut handles = Vec::new();

        for _ in 0..self.num_threads {
            let entry_rx = entry_rx.clone();
            let record_tx = record_tx.clone();
            let batch_size = self.batch_size;

            let handle = thread::spawn(move || {
                metadata_worker_loop(entry_rx, record_tx, batch_size);
            });

            handles.push(handle);
        }

        handles
    }

    fn spawn_db_thread(
        &self,
        record_rx: Receiver<Vec<FileRecord>>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            db_worker_loop(record_rx);
        })
    }
}

fn scan_directory_recursive(root: PathBuf, tx: &Sender<Vec<DirEntry>>) {
    let entries = match std::fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    let mut dir_entries = Vec::new();
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let is_dir = entry.path().is_dir();
        let dir_entry = DirEntry {
            file_name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            is_dir,
        };

        if is_dir {
            subdirs.push(entry.path());
        }
        dir_entries.push(dir_entry);
    }

    if !dir_entries.is_empty() {
        let _ = tx.send(dir_entries);
    }

    for subdir in subdirs {
        scan_directory_recursive(subdir, tx);
    }
}

fn metadata_worker_loop(
    entry_rx: Receiver<Vec<DirEntry>>,
    record_tx: Sender<Vec<FileRecord>>,
    batch_size: usize,
) {
    let mut batch = Vec::with_capacity(batch_size);

    while let Ok(entries) = entry_rx.recv() {
        for entry in entries {
            if !entry.is_dir
                && let Ok(meta) = std::fs::metadata(&entry.path)
            {
                let record = FileRecord {
                    path: entry.path,
                    file_name: entry.file_name,
                    size: meta.len(),
                    modified: meta.modified().unwrap(),
                };

                batch.push(record);

                if batch.len() >= batch_size {
                    let _ = record_tx.send(
                        std::mem::replace(&mut batch, Vec::with_capacity(batch_size)),
                    );
                }
            }
        }
    }

    if !batch.is_empty() {
        let _ = record_tx.send(batch);
    }
}

fn db_worker_loop(record_rx: Receiver<Vec<FileRecord>>) {
    while let Ok(batch) = record_rx.recv() {
        process_batch(batch);
    }
}

fn process_batch(batch: Vec<FileRecord>) {
    for record in batch {
        println!("Got record: {:?}", record.path);
    }
}
