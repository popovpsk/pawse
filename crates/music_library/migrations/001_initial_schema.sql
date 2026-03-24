-- ============================================================================
-- MIGRATION 001: Initial Schema
-- Description: Core tables for music library with many-to-many relationships
-- ============================================================================

-- ----------------------------------------------------------------------------
-- VERSION TABLE
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY CHECK (version = 1),
    applied_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);

-- ----------------------------------------------------------------------------
-- CORE ENTITIES
-- ----------------------------------------------------------------------------

-- Artists table (normalized)
CREATE TABLE IF NOT EXISTS artists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL COLLATE NOCASE,
    disambiguation TEXT,
    type TEXT NOT NULL DEFAULT 'unknown' CHECK (type IN ('person', 'group', 'unknown')),
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
    
    UNIQUE(name, disambiguation)
);

-- Albums table (normalized)
CREATE TABLE IF NOT EXISTS albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL COLLATE NOCASE,
    year INTEGER CHECK (year >= 1900 AND year <= 2100),
    genre TEXT,
    cover_art_path TEXT,
    total_discs INTEGER DEFAULT 1,
    type TEXT NOT NULL DEFAULT 'unknown' CHECK (type IN ('album', 'single', 'ep', 'compilation', 'unknown')),
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
    
    UNIQUE(title, year)
);

-- Tracks table (core entity)
CREATE TABLE IF NOT EXISTS tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- File info (required, unique)
    file_path TEXT NOT NULL UNIQUE,
    file_size INTEGER CHECK (file_size > 0),
    file_modified_at DATETIME,
    
    -- Audio metadata (from tags)
    title TEXT NOT NULL COLLATE NOCASE,
    duration_ms INTEGER NOT NULL CHECK (duration_ms > 0),
    
    -- Track positioning
    track_number INTEGER CHECK (track_number > 0),
    total_tracks INTEGER CHECK (total_tracks > 0),
    disc_number INTEGER DEFAULT 1 CHECK (disc_number > 0),
    
    -- Technical audio info
    sample_rate INTEGER CHECK (sample_rate > 0),
    bit_depth INTEGER CHECK (bit_depth > 0),
    channels INTEGER CHECK (channels > 0),
    codec TEXT,
    
    -- Playback stats
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played_at DATETIME,
    rating INTEGER CHECK (rating >= 0 AND rating <= 5),
    
    -- Library state
    is_available INTEGER NOT NULL DEFAULT 1,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    
    -- Timestamps
    added_at DATETIME NOT NULL DEFAULT (datetime('now')),
    updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
    
    -- Foreign keys (nullable for many-to-many)
    album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL
);

-- ----------------------------------------------------------------------------
-- MANY-TO-MANY RELATIONSHIPS
-- ----------------------------------------------------------------------------

-- Track ↔ Artist relationship (a track can have multiple artists)
CREATE TABLE IF NOT EXISTS track_artists (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'primary' CHECK (role IN (
        'primary', 'featured', 'remixer', 'producer', 'engineer', 'mixer', 'mastering'
    )),
    display_order INTEGER NOT NULL DEFAULT 1,
    
    PRIMARY KEY (track_id, artist_id, role)
);

-- Album ↔ Artist relationship (compilations, various artists, etc.)
CREATE TABLE IF NOT EXISTS album_artists (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'primary' CHECK (role IN ('primary', 'featured', 'various')),
    display_order INTEGER NOT NULL DEFAULT 1,
    
    PRIMARY KEY (album_id, artist_id, role)
);

-- ----------------------------------------------------------------------------
-- FULL-TEXT SEARCH (FTS5)
-- ----------------------------------------------------------------------------

-- FTS5 virtual table for tracks
CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
    title,
    artist_names,
    album_title,
    genre,
    content='tracks',
    content_rowid='id'
);

-- Triggers to keep FTS in sync with track changes
CREATE TRIGGER IF NOT EXISTS tracks_fts_insert AFTER INSERT ON tracks BEGIN
    INSERT INTO tracks_fts(rowid, title, artist_names, album_title, genre)
    SELECT 
        NEW.id,
        NEW.title,
        GROUP_CONCAT(a.name, ' '),
        al.title,
        COALESCE(NEW.genre, al.genre)
    FROM NEW
    LEFT JOIN track_artists ta ON ta.track_id = NEW.id
    LEFT JOIN artists a ON a.id = ta.artist_id
    LEFT JOIN albums al ON al.id = NEW.album_id
    GROUP BY NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS tracks_fts_delete AFTER DELETE ON tracks BEGIN
    INSERT INTO tracks_fts(tracks_fts, rowid, title, artist_names, album_title, genre) 
    VALUES('delete', OLD.id, OLD.title, '', '', '');
END;

CREATE TRIGGER IF NOT EXISTS tracks_fts_update AFTER UPDATE ON tracks BEGIN
    INSERT INTO tracks_fts(tracks_fts, rowid, title, artist_names, album_title, genre) 
    VALUES('delete', OLD.id, OLD.title, '', '', '');
    INSERT INTO tracks_fts(rowid, title, artist_names, album_title, genre)
    SELECT 
        NEW.id,
        NEW.title,
        GROUP_CONCAT(a.name, ' '),
        al.title,
        COALESCE(NEW.genre, al.genre)
    FROM NEW
    LEFT JOIN track_artists ta ON ta.track_id = NEW.id
    LEFT JOIN artists a ON a.id = ta.artist_id
    LEFT JOIN albums al ON al.id = NEW.album_id
    GROUP BY NEW.id;
END;

-- ----------------------------------------------------------------------------
-- TRIGGERS FOR UPDATED_AT
-- ----------------------------------------------------------------------------

CREATE TRIGGER IF NOT EXISTS update_artists_timestamp AFTER UPDATE ON artists BEGIN
    UPDATE artists SET updated_at = datetime('now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS update_albums_timestamp AFTER UPDATE ON albums BEGIN
    UPDATE albums SET updated_at = datetime('now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS update_tracks_timestamp AFTER UPDATE ON tracks BEGIN
    UPDATE tracks SET updated_at = datetime('now') WHERE id = NEW.id;
END;
