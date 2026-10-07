import type { AlbumSort, ArtistSort } from "./connection.svelte";

type Choice<T> = { sort: T; desc: boolean };

type SortPrefs = {
  albums: Choice<AlbumSort>;
  artists: Choice<ArtistSort>;
};

const KEY = "pawse.sort";

const ALBUM_SORTS: readonly AlbumSort[] = ["artist", "title", "year"];
const ARTIST_SORTS: readonly ArtistSort[] = ["name", "tracks"];

function pick<T extends string>(raw: unknown, allowed: readonly T[], fallback: Choice<T>): Choice<T> {
  if (typeof raw !== "object" || raw === null) return fallback;
  const { sort, desc } = raw as { sort?: unknown; desc?: unknown };
  if (!allowed.includes(sort as T) || typeof desc !== "boolean") return fallback;
  return { sort: sort as T, desc };
}

function load(): SortPrefs {
  let stored: { albums?: unknown; artists?: unknown } = {};
  try {
    stored = JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {};
  } catch {}
  return {
    albums: pick(stored.albums, ALBUM_SORTS, { sort: "artist", desc: false }),
    artists: pick(stored.artists, ARTIST_SORTS, { sort: "name", desc: false }),
  };
}

export const sortPrefs: SortPrefs = $state(load());

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(sortPrefs));
  } catch {}
}

export function setAlbumSort(sort: AlbumSort, desc: boolean) {
  sortPrefs.albums = { sort, desc };
  save();
}

export function setArtistSort(sort: ArtistSort, desc: boolean) {
  sortPrefs.artists = { sort, desc };
  save();
}
