const DAY_SECS: u64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    NewMusic,
    FromLibrary,
    Forgotten,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::NewMusic, Mode::FromLibrary, Mode::Forgotten];

    pub fn counts(self) -> &'static [u32] {
        match self {
            Mode::NewMusic => &ALBUM_COUNTS,
            Mode::FromLibrary | Mode::Forgotten => &PLAYLIST_COUNTS,
        }
    }

    pub fn builds_playlist(self) -> bool {
        self != Mode::NewMusic
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Week,
    Month,
    HalfYear,
    AllTime,
}

impl Period {
    pub const ALL: [Period; 4] = [
        Period::Week,
        Period::Month,
        Period::HalfYear,
        Period::AllTime,
    ];

    pub fn days(self) -> Option<u64> {
        match self {
            Period::Week => Some(7),
            Period::Month => Some(30),
            Period::HalfYear => Some(182),
            Period::AllTime => None,
        }
    }

    pub fn cutoff(self, now: u64) -> Option<u64> {
        self.days().map(|days| now.saturating_sub(days * DAY_SECS))
    }

    pub fn describe(self) -> &'static str {
        match self {
            Period::Week => "the last 7 days",
            Period::Month => "the last 30 days",
            Period::HalfYear => "the last 6 months",
            Period::AllTime => "all time",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Low,
    Medium,
    High,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Low, Detail::Medium, Detail::High];
}

pub const ALBUM_COUNTS: [u32; 3] = [5, 10, 20];
pub const PLAYLIST_COUNTS: [u32; 4] = [5, 10, 20, 50];
pub const DEFAULT_COUNT: u32 = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptOptions {
    pub mode: Mode,
    pub period: Period,
    pub count: u32,
    pub detail: Detail,
    pub wishes: String,
    pub answer_language: &'static str,
}

pub fn language_name(code: &str) -> &'static str {
    match code {
        "ru" => "Russian",
        "uk" => "Ukrainian",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "pt" => "Brazilian Portuguese",
        "it" => "Italian",
        "nl" => "Dutch",
        "sv" => "Swedish",
        "pl" => "Polish",
        "cs" => "Czech",
        "tr" => "Turkish",
        "vi" => "Vietnamese",
        "id" => "Indonesian",
        "th" => "Thai",
        "hi" => "Hindi",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Simplified Chinese",
        _ => "English",
    }
}
