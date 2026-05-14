pub const MIGRATIONS: &[(i32, &str)] = &[
    (
        1,
        r#"
        CREATE TABLE artists (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort_name TEXT NOT NULL
        );

        CREATE UNIQUE INDEX idx_artists_name ON artists(name);
        CREATE INDEX idx_artists_sort_name ON artists(sort_name);

        CREATE TABLE albums (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            year INTEGER,
            cover_art_path TEXT
        );

        CREATE INDEX idx_albums_title ON albums(title);
        CREATE INDEX idx_albums_year ON albums(year);

        CREATE TABLE album_artists (
            album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
            artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (album_id, artist_id)
        );

        CREATE TABLE tracks (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            title TEXT,
            album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL,
            track_number INTEGER,
            disc_number INTEGER NOT NULL DEFAULT 1,
            duration_ms INTEGER,
            year INTEGER,
            cover_art_path TEXT,
            start_offset_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE UNIQUE INDEX idx_tracks_path_offset ON tracks(path, start_offset_ms);

        CREATE INDEX idx_tracks_album_id ON tracks(album_id);
        CREATE INDEX idx_tracks_track_number ON tracks(track_number);

        CREATE TABLE track_artists (
            track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
            role TEXT NOT NULL DEFAULT 'main',
            credited_as TEXT,
            position INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (track_id, artist_id, role, position)
        );

        CREATE INDEX idx_track_artists_artist_id ON track_artists(artist_id);
        "#,
    ),
];
