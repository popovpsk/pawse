use gpui::SharedString;

use super::{Lang, fill};

pub struct ArtistCardStrings {
    pub since_t: SharedString,
    pub born_t: SharedString,
    pub members_t: SharedString,
    pub member_of_t: SharedString,
}

impl ArtistCardStrings {
    pub fn since(&self, year: i32) -> String {
        fill(&self.since_t, &[&year.to_string()])
    }

    pub fn born(&self, year: i32) -> String {
        fill(&self.born_t, &[&year.to_string()])
    }

    pub fn members(&self, names: &str) -> String {
        fill(&self.members_t, &[names])
    }

    pub fn member_of(&self, names: &str) -> String {
        fill(&self.member_of_t, &[names])
    }
}

pub fn artist_card_strings_for(lang: Lang) -> &'static ArtistCardStrings {
    for_lang(lang)
}

fn for_lang(lang: Lang) -> &'static ArtistCardStrings {
    match lang {
        Lang::En => &EN,
        Lang::Zh => &ZH,
        Lang::Pt => &PT,
        Lang::Ru => &RU,
        Lang::Ja => &JA,
        Lang::De => &DE,
        Lang::Fr => &FR,
        Lang::Ko => &KO,
        Lang::It => &IT,
        Lang::Tr => &TR,
        Lang::Pl => &PL,
        Lang::Nl => &NL,
        Lang::Uk => &UK,
        Lang::Vi => &VI,
        Lang::Id => &ID,
        Lang::Th => &TH,
        Lang::Cs => &CS,
        Lang::Sv => &SV,
        Lang::Hi => &HI,
        Lang::Es => &ES,
    }
}

static EN: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("since {}"),
    born_t: SharedString::new_static("born {}"),
    members_t: SharedString::new_static("Members: {}"),
    member_of_t: SharedString::new_static("Member of: {}"),
};

static ZH: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{}年至今"),
    born_t: SharedString::new_static("{}年出生"),
    members_t: SharedString::new_static("成员：{}"),
    member_of_t: SharedString::new_static("所属：{}"),
};

static PT: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("desde {}"),
    born_t: SharedString::new_static("n. {}"),
    members_t: SharedString::new_static("Integrantes: {}"),
    member_of_t: SharedString::new_static("Integrante de: {}"),
};

static RU: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("с {}"),
    born_t: SharedString::new_static("род. {}"),
    members_t: SharedString::new_static("Состав: {}"),
    member_of_t: SharedString::new_static("Группы: {}"),
};

static JA: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{}年〜"),
    born_t: SharedString::new_static("{}年生まれ"),
    members_t: SharedString::new_static("メンバー：{}"),
    member_of_t: SharedString::new_static("所属：{}"),
};

static DE: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("seit {}"),
    born_t: SharedString::new_static("geb. {}"),
    members_t: SharedString::new_static("Mitglieder: {}"),
    member_of_t: SharedString::new_static("Mitglied bei: {}"),
};

static FR: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("depuis {}"),
    born_t: SharedString::new_static("né(e) en {}"),
    members_t: SharedString::new_static("Membres : {}"),
    member_of_t: SharedString::new_static("Membre de : {}"),
};

static KO: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{}년~"),
    born_t: SharedString::new_static("{}년생"),
    members_t: SharedString::new_static("멤버: {}"),
    member_of_t: SharedString::new_static("소속: {}"),
};

static IT: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("dal {}"),
    born_t: SharedString::new_static("n. {}"),
    members_t: SharedString::new_static("Membri: {}"),
    member_of_t: SharedString::new_static("Membro di: {}"),
};

static TR: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{} yılından beri"),
    born_t: SharedString::new_static("d. {}"),
    members_t: SharedString::new_static("Üyeler: {}"),
    member_of_t: SharedString::new_static("Üyesi olduğu: {}"),
};

static PL: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("od {}"),
    born_t: SharedString::new_static("ur. {}"),
    members_t: SharedString::new_static("Skład: {}"),
    member_of_t: SharedString::new_static("Zespoły: {}"),
};

static NL: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("sinds {}"),
    born_t: SharedString::new_static("geb. {}"),
    members_t: SharedString::new_static("Leden: {}"),
    member_of_t: SharedString::new_static("Lid van: {}"),
};

static UK: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("з {}"),
    born_t: SharedString::new_static("нар. {}"),
    members_t: SharedString::new_static("Склад: {}"),
    member_of_t: SharedString::new_static("Гурти: {}"),
};

static VI: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("từ {}"),
    born_t: SharedString::new_static("sinh {}"),
    members_t: SharedString::new_static("Thành viên: {}"),
    member_of_t: SharedString::new_static("Thành viên của: {}"),
};

static ID: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("sejak {}"),
    born_t: SharedString::new_static("lahir {}"),
    members_t: SharedString::new_static("Anggota: {}"),
    member_of_t: SharedString::new_static("Anggota dari: {}"),
};

static TH: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("ตั้งแต่ {}"),
    born_t: SharedString::new_static("เกิด {}"),
    members_t: SharedString::new_static("สมาชิก: {}"),
    member_of_t: SharedString::new_static("สังกัด: {}"),
};

static CS: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("od {}"),
    born_t: SharedString::new_static("nar. {}"),
    members_t: SharedString::new_static("Členové: {}"),
    member_of_t: SharedString::new_static("Člen: {}"),
};

static SV: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("sedan {}"),
    born_t: SharedString::new_static("f. {}"),
    members_t: SharedString::new_static("Medlemmar: {}"),
    member_of_t: SharedString::new_static("Medlem i: {}"),
};

static HI: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{} से"),
    born_t: SharedString::new_static("जन्म {}"),
    members_t: SharedString::new_static("सदस्य: {}"),
    member_of_t: SharedString::new_static("समूह: {}"),
};

static ES: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("desde {}"),
    born_t: SharedString::new_static("n. {}"),
    members_t: SharedString::new_static("Miembros: {}"),
    member_of_t: SharedString::new_static("Miembro de: {}"),
};
