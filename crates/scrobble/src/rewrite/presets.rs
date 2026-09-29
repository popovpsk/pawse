use super::Field;

pub struct PresetRule {
    pub pattern: &'static str,
    pub replacement: &'static str,
}

pub struct Preset {
    pub id: &'static str,
    pub fields: &'static [Field],
    pub rules: &'static [PresetRule],
    pub example: (&'static str, &'static str),
}

const fn first(pattern: &'static str, replacement: &'static str) -> PresetRule {
    PresetRule {
        pattern,
        replacement,
    }
}

const TITLE_ALBUM: &[Field] = &[Field::Title, Field::Album];
const TITLE: &[Field] = &[Field::Title];
const ALBUM: &[Field] = &[Field::Album];

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "remastered",
        fields: TITLE_ALBUM,
        rules: &[
            first(r"Live\s/\sRemastered", "Live"),
            first(r"\s[(\[].*Re-?[Mm]aster(ed)?.*[)\]]$", ""),
            first(r"\s-\s\d{4}(\s-)?\s.*Re-?[Mm]aster(ed)?.*$", ""),
            first(r"\s-\sRe-?[Mm]aster(ed)?.*$", ""),
        ],
        example: ("Let It Be (Remastered 2009)", "Let It Be"),
    },
    Preset {
        id: "explicit",
        fields: TITLE_ALBUM,
        rules: &[
            first(r"(?i)\s[(\[]Explicit( Version)?[)\]]", ""),
            first(r"(?i)\s[(\[]Clean( Version)?[)\]]", ""),
        ],
        example: ("HUMBLE. (Explicit)", "HUMBLE."),
    },
    Preset {
        id: "single_ep",
        fields: ALBUM,
        rules: &[first(r"\s-\s(Single|EP)$", "")],
        example: ("Blinding Lights - Single", "Blinding Lights"),
    },
    Preset {
        id: "version",
        fields: TITLE,
        rules: &[
            first(r"\s[(\[]Album Version[)\]]$", ""),
            first(r"\s[(\[]Re-?[Rr]ecorded[)\]]$", ""),
            first(r"\s[(\[]Single Version[)\]]$", ""),
            first(r"\s[(\[]Edit[)\]]$", ""),
            first(r"\s-\sMono Version$", ""),
            first(r"\s-\sStereo Version$", ""),
            first(r"(?i)\s-\sOriginal$", ""),
            first(r"(?i)\s-\sOriginal.*Version(\s\d{4})?$", ""),
        ],
        example: (
            "Your Cheatin' Heart (Single Version)",
            "Your Cheatin' Heart",
        ),
    },
    Preset {
        id: "edition",
        fields: ALBUM,
        rules: &[
            first(r"(?i)\s[(\[](Super )?Deluxe( Edition| Version)?[)\]]$", ""),
            first(r"\s[(\[]Expanded.*[)\]]$", ""),
            first(r"\s-\sExpanded Edition$", ""),
            first(r"(?i)\s[(\[]Bonus Track Edition[)\]]", ""),
            first(r"(?i)\s[(\[]\d+th\sAnniversary.*[)\]]", ""),
            first(r"(?i)\sRe-?issue$", ""),
            first(r"(?i)\s\[.*?Re-?issue.*?\]", ""),
            first(r"(?i)\s\(.*?Re-?issue.*?\)", ""),
        ],
        example: ("Ace of Spades (Expanded Edition)", "Ace of Spades"),
    },
    Preset {
        id: "remix_suffix",
        fields: TITLE,
        rules: &[
            first(
                r"(?i)-\s(.+?)\s((Re)?mix|edit|dub|mix|vip|version)$",
                "(${1} ${2})",
            ),
            first(r"(?i)-\s(Remix|VIP|Instrumental)$", "(${1})"),
        ],
        example: ("Strobe - Club Edit", "Strobe (Club Edit)"),
    },
    Preset {
        id: "feat",
        fields: TITLE,
        rules: &[
            first(r"(?i)\s[(\[](feat\.?|ft\.|featuring)\s.+?[)\]]", ""),
            first(r"(?i)\s(feat\.|ft\.|featuring)\s.+$", ""),
        ],
        example: ("Get Lucky (feat. Pharrell Williams)", "Get Lucky"),
    },
    Preset {
        id: "live",
        fields: TITLE,
        rules: &[
            first(r"\s-\sLive(\s.+)?$", ""),
            first(r"\s[(\[]Live(\s[^)\]]*)?[)\]]$", ""),
        ],
        example: (
            "Ticket To Ride - Live At The Hollywood Bowl",
            "Ticket To Ride",
        ),
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}
