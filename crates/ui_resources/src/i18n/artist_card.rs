use gpui::SharedString;

use super::{Lang, fill};

pub struct ArtistCardStrings {
    pub since_t: SharedString,
    pub born_t: SharedString,
    pub members_t: SharedString,
    pub member_of_t: SharedString,
    pub setting: SharedString,
    pub setting_desc: SharedString,
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
    setting: SharedString::new_static("Fetch artist info from the internet"),
    setting_desc: SharedString::new_static(
        "Photo and facts on the artist page. Artist names from your library are sent to MusicBrainz, photos are downloaded from Deezer. Turn off for privacy.",
    ),
};

static ZH: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{}年至今"),
    born_t: SharedString::new_static("{}年出生"),
    members_t: SharedString::new_static("成员：{}"),
    member_of_t: SharedString::new_static("所属：{}"),
    setting: SharedString::new_static("从互联网获取艺人信息"),
    setting_desc: SharedString::new_static(
        "艺人页面上的照片和资料。媒体库中的艺人名称会发送到 MusicBrainz，照片从 Deezer 下载。关闭以保护隐私。",
    ),
};

static PT: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("desde {}"),
    born_t: SharedString::new_static("n. {}"),
    members_t: SharedString::new_static("Integrantes: {}"),
    member_of_t: SharedString::new_static("Integrante de: {}"),
    setting: SharedString::new_static("Buscar informações do artista na internet"),
    setting_desc: SharedString::new_static(
        "Foto e dados na página do artista. Os nomes dos artistas da biblioteca são enviados ao MusicBrainz e as fotos são baixadas do Deezer. Desative para privacidade.",
    ),
};

static RU: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("с {}"),
    born_t: SharedString::new_static("род. {}"),
    members_t: SharedString::new_static("Состав: {}"),
    member_of_t: SharedString::new_static("Группы: {}"),
    setting: SharedString::new_static("Подтягивать информацию об исполнителях"),
    setting_desc: SharedString::new_static(
        "Фото и факты на странице исполнителя. Имена исполнителей из медиатеки отправляются в MusicBrainz, фото загружаются с Deezer. Отключите для приватности.",
    ),
};

static JA: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{}年〜"),
    born_t: SharedString::new_static("{}年生まれ"),
    members_t: SharedString::new_static("メンバー：{}"),
    member_of_t: SharedString::new_static("所属：{}"),
    setting: SharedString::new_static("インターネットからアーティスト情報を取得"),
    setting_desc: SharedString::new_static(
        "アーティストページの写真とプロフィール。ライブラリのアーティスト名を MusicBrainz に送信し、写真を Deezer からダウンロードします。プライバシーのためにオフにすることができます。",
    ),
};

static DE: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("seit {}"),
    born_t: SharedString::new_static("geb. {}"),
    members_t: SharedString::new_static("Mitglieder: {}"),
    member_of_t: SharedString::new_static("Mitglied bei: {}"),
    setting: SharedString::new_static("Künstlerinfos aus dem Internet laden"),
    setting_desc: SharedString::new_static(
        "Foto und Fakten auf der Künstlerseite. Künstlernamen aus der Mediathek werden an MusicBrainz gesendet, Fotos von Deezer geladen. Für mehr Datenschutz deaktivieren.",
    ),
};

static FR: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("depuis {}"),
    born_t: SharedString::new_static("né(e) en {}"),
    members_t: SharedString::new_static("Membres : {}"),
    member_of_t: SharedString::new_static("Membre de : {}"),
    setting: SharedString::new_static("Récupérer les infos d'artiste depuis internet"),
    setting_desc: SharedString::new_static(
        "Photo et informations sur la page de l'artiste. Les noms d'artistes de la bibliothèque sont envoyés à MusicBrainz, les photos sont téléchargées depuis Deezer. Désactivez pour plus de confidentialité.",
    ),
};

static KO: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{}년~"),
    born_t: SharedString::new_static("{}년생"),
    members_t: SharedString::new_static("멤버: {}"),
    member_of_t: SharedString::new_static("소속: {}"),
    setting: SharedString::new_static("인터넷에서 아티스트 정보 가져오기"),
    setting_desc: SharedString::new_static(
        "아티스트 페이지의 사진과 정보. 라이브러리의 아티스트 이름을 MusicBrainz로 보내고 사진은 Deezer에서 내려받습니다. 개인정보 보호를 위해 끄세요.",
    ),
};

static IT: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("dal {}"),
    born_t: SharedString::new_static("n. {}"),
    members_t: SharedString::new_static("Membri: {}"),
    member_of_t: SharedString::new_static("Membro di: {}"),
    setting: SharedString::new_static("Cerca info sull'artista su internet"),
    setting_desc: SharedString::new_static(
        "Foto e informazioni nella pagina dell'artista. I nomi degli artisti della libreria vengono inviati a MusicBrainz, le foto scaricate da Deezer. Disattiva per la privacy.",
    ),
};

static TR: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{} yılından beri"),
    born_t: SharedString::new_static("d. {}"),
    members_t: SharedString::new_static("Üyeler: {}"),
    member_of_t: SharedString::new_static("Üyesi olduğu: {}"),
    setting: SharedString::new_static("Sanatçı bilgilerini internetten getir"),
    setting_desc: SharedString::new_static(
        "Sanatçı sayfasında fotoğraf ve bilgiler. Kitaplıktaki sanatçı adları MusicBrainz'e gönderilir, fotoğraflar Deezer'dan indirilir. Gizlilik için kapat.",
    ),
};

static PL: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("od {}"),
    born_t: SharedString::new_static("ur. {}"),
    members_t: SharedString::new_static("Skład: {}"),
    member_of_t: SharedString::new_static("Zespoły: {}"),
    setting: SharedString::new_static("Pobieraj informacje o wykonawcach z internetu"),
    setting_desc: SharedString::new_static(
        "Zdjęcie i informacje na stronie wykonawcy. Nazwy wykonawców z biblioteki są wysyłane do MusicBrainz, zdjęcia pobierane z Deezera. Wyłącz dla prywatności.",
    ),
};

static NL: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("sinds {}"),
    born_t: SharedString::new_static("geb. {}"),
    members_t: SharedString::new_static("Leden: {}"),
    member_of_t: SharedString::new_static("Lid van: {}"),
    setting: SharedString::new_static("Artiestinfo van internet ophalen"),
    setting_desc: SharedString::new_static(
        "Foto en feiten op de artiestpagina. Artiestnamen uit je bibliotheek worden naar MusicBrainz gestuurd, foto's worden van Deezer gedownload. Zet uit voor privacy.",
    ),
};

static UK: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("з {}"),
    born_t: SharedString::new_static("нар. {}"),
    members_t: SharedString::new_static("Склад: {}"),
    member_of_t: SharedString::new_static("Гурти: {}"),
    setting: SharedString::new_static("Підтягувати інформацію про виконавців"),
    setting_desc: SharedString::new_static(
        "Фото й факти на сторінці виконавця. Імена виконавців із медіатеки надсилаються до MusicBrainz, фото завантажуються з Deezer. Вимкніть для приватності.",
    ),
};

static VI: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("từ {}"),
    born_t: SharedString::new_static("sinh {}"),
    members_t: SharedString::new_static("Thành viên: {}"),
    member_of_t: SharedString::new_static("Thành viên của: {}"),
    setting: SharedString::new_static("Lấy thông tin nghệ sĩ từ internet"),
    setting_desc: SharedString::new_static(
        "Ảnh và thông tin trên trang nghệ sĩ. Tên nghệ sĩ trong thư viện được gửi tới MusicBrainz, ảnh được tải từ Deezer. Tắt để bảo vệ quyền riêng tư.",
    ),
};

static ID: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("sejak {}"),
    born_t: SharedString::new_static("lahir {}"),
    members_t: SharedString::new_static("Anggota: {}"),
    member_of_t: SharedString::new_static("Anggota dari: {}"),
    setting: SharedString::new_static("Ambil info artis dari internet"),
    setting_desc: SharedString::new_static(
        "Foto dan info di halaman artis. Nama artis dari pustaka dikirim ke MusicBrainz, foto diunduh dari Deezer. Matikan untuk privasi.",
    ),
};

static TH: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("ตั้งแต่ {}"),
    born_t: SharedString::new_static("เกิด {}"),
    members_t: SharedString::new_static("สมาชิก: {}"),
    member_of_t: SharedString::new_static("สังกัด: {}"),
    setting: SharedString::new_static("ดึงข้อมูลศิลปินจากอินเทอร์เน็ต"),
    setting_desc: SharedString::new_static(
        "รูปภาพและข้อมูลในหน้าศิลปิน ชื่อศิลปินในคลังเพลงจะถูกส่งไปยัง MusicBrainz และดาวน์โหลดรูปภาพจาก Deezer ปิดเพื่อความเป็นส่วนตัว",
    ),
};

static CS: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("od {}"),
    born_t: SharedString::new_static("nar. {}"),
    members_t: SharedString::new_static("Členové: {}"),
    member_of_t: SharedString::new_static("Člen: {}"),
    setting: SharedString::new_static("Stahovat informace o interpretech z internetu"),
    setting_desc: SharedString::new_static(
        "Fotka a údaje na stránce interpreta. Jména interpretů z knihovny se odesílají do MusicBrainz, fotky se stahují z Deezeru. Vypněte pro ochranu soukromí.",
    ),
};

static SV: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("sedan {}"),
    born_t: SharedString::new_static("f. {}"),
    members_t: SharedString::new_static("Medlemmar: {}"),
    member_of_t: SharedString::new_static("Medlem i: {}"),
    setting: SharedString::new_static("Hämta artistinfo från internet"),
    setting_desc: SharedString::new_static(
        "Foto och fakta på artistsidan. Artistnamn från biblioteket skickas till MusicBrainz, foton hämtas från Deezer. Stäng av för integritet.",
    ),
};

static HI: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("{} से"),
    born_t: SharedString::new_static("जन्म {}"),
    members_t: SharedString::new_static("सदस्य: {}"),
    member_of_t: SharedString::new_static("समूह: {}"),
    setting: SharedString::new_static("इंटरनेट से कलाकार की जानकारी लाएं"),
    setting_desc: SharedString::new_static(
        "कलाकार पेज पर फ़ोटो और जानकारी। लाइब्रेरी के कलाकारों के नाम MusicBrainz को भेजे जाते हैं, फ़ोटो Deezer से डाउनलोड होती हैं। गोपनीयता के लिए बंद करें।",
    ),
};

static ES: ArtistCardStrings = ArtistCardStrings {
    since_t: SharedString::new_static("desde {}"),
    born_t: SharedString::new_static("n. {}"),
    members_t: SharedString::new_static("Miembros: {}"),
    member_of_t: SharedString::new_static("Miembro de: {}"),
    setting: SharedString::new_static("Buscar información del artista en internet"),
    setting_desc: SharedString::new_static(
        "Foto y datos en la página del artista. Los nombres de artistas de la biblioteca se envían a MusicBrainz y las fotos se descargan de Deezer. Desactiva para mayor privacidad.",
    ),
};
