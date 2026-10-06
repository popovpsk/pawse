use gpui::SharedString;

use super::{Lang, active, fill};

pub struct PlaylistImportStrings {
    pub import: SharedString,
    pub import_tooltip: SharedString,
    pub ai: SharedString,
    pub ai_tooltip: SharedString,
    pub title: SharedString,
    pub description: SharedString,
    pub scope_mine: SharedString,
    pub scope_all: SharedString,
    pub scope_all_hint: SharedString,
    pub result_t: SharedString,
}

impl PlaylistImportStrings {
    pub fn result(&self, playlists: usize, added: usize, found: usize, total: usize) -> String {
        fill(
            &self.result_t,
            &[
                &playlists.to_string(),
                &added.to_string(),
                &found.to_string(),
                &total.to_string(),
            ],
        )
    }
}

pub fn playlist_import_strings() -> &'static PlaylistImportStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static PlaylistImportStrings {
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

static EN: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Import"),
    import_tooltip: SharedString::new_static("Import playlists from a server"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Make a playlist with an AI chat"),
    title: SharedString::new_static("Import playlists"),
    description: SharedString::new_static(
        "Playlists are copied from the server once, and nothing is sent back. Tracks you also have as files play from the files. If you already have a playlist with the same name, it only gets the tracks it's missing.",
    ),
    scope_mine: SharedString::new_static("Only my playlists"),
    scope_all: SharedString::new_static("All available"),
    scope_all_hint: SharedString::new_static(
        "Also other users' playlists and the ones the server makes from playlist files (.m3u and similar) in its library. If you're a server admin, this can include other users' private playlists.",
    ),
    result_t: SharedString::new_static("Playlists: {} · tracks added: {} · found: {} of {}"),
};

static ZH: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("导入"),
    import_tooltip: SharedString::new_static("从服务器导入播放列表"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("用 AI 对话生成播放列表"),
    title: SharedString::new_static("导入播放列表"),
    description: SharedString::new_static(
        "播放列表只会从服务器复制一次，不会向服务器回传任何内容。你也有文件的曲目会从文件播放。如果已有同名播放列表，只会补上其中缺少的曲目。",
    ),
    scope_mine: SharedString::new_static("仅我的播放列表"),
    scope_all: SharedString::new_static("所有可访问的"),
    scope_all_hint: SharedString::new_static(
        "还包括其他用户的播放列表，以及服务器根据其媒体库中的播放列表文件（.m3u 等）生成的列表。如果你是服务器管理员，其中可能还包括其他用户的私人播放列表。",
    ),
    result_t: SharedString::new_static("播放列表：{} · 新增曲目：{} · 找到：{}/{}"),
};

static PT: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importar"),
    import_tooltip: SharedString::new_static("Importar playlists de um servidor"),
    ai: SharedString::new_static("IA"),
    ai_tooltip: SharedString::new_static("Criar uma playlist com um chat de IA"),
    title: SharedString::new_static("Importar playlists"),
    description: SharedString::new_static(
        "As playlists são copiadas do servidor uma única vez, e nada é enviado de volta. As faixas que você também tem como arquivos tocam a partir dos arquivos. Se você já tem uma playlist com o mesmo nome, ela recebe só as faixas que faltam.",
    ),
    scope_mine: SharedString::new_static("Só as minhas playlists"),
    scope_all: SharedString::new_static("Todas as disponíveis"),
    scope_all_hint: SharedString::new_static(
        "Também as playlists de outros usuários e as que o servidor cria a partir de arquivos de playlist (.m3u e similares) na biblioteca dele. Se você for administrador do servidor, isso pode incluir playlists privadas de outros usuários.",
    ),
    result_t: SharedString::new_static(
        "Playlists: {} · faixas adicionadas: {} · encontradas: {} de {}",
    ),
};

static RU: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Импорт"),
    import_tooltip: SharedString::new_static("Импортировать плейлисты с сервера"),
    ai: SharedString::new_static("ИИ"),
    ai_tooltip: SharedString::new_static("Собрать плейлист с помощью ИИ-чата"),
    title: SharedString::new_static("Импорт плейлистов"),
    description: SharedString::new_static(
        "Плейлисты копируются с сервера один раз, обратно ничего не отправляется. Треки, которые у вас есть и файлами, играют из файлов. Если плейлист с таким именем уже есть, в него добавятся только недостающие треки.",
    ),
    scope_mine: SharedString::new_static("Только мои плейлисты"),
    scope_all: SharedString::new_static("Все доступные"),
    scope_all_hint: SharedString::new_static(
        "Ещё и плейлисты других пользователей, и те, что сервер собирает из файлов плейлистов (.m3u и подобных) в своей медиатеке. Если вы администратор сервера, среди них могут оказаться и чужие личные плейлисты.",
    ),
    result_t: SharedString::new_static("Плейлистов: {} · добавлено треков: {} · найдено: {} из {}"),
};

static JA: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("インポート"),
    import_tooltip: SharedString::new_static("サーバーからプレイリストをインポート"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("AI チャットでプレイリストを作る"),
    title: SharedString::new_static("プレイリストのインポート"),
    description: SharedString::new_static(
        "プレイリストはサーバーから一度だけコピーされ、サーバーには何も送り返されません。ファイルとしても持っているトラックはファイルから再生されます。同じ名前のプレイリストがすでにある場合は、足りないトラックだけが追加されます。",
    ),
    scope_mine: SharedString::new_static("自分のプレイリストのみ"),
    scope_all: SharedString::new_static("利用できるすべて"),
    scope_all_hint: SharedString::new_static(
        "他のユーザーのプレイリストや、サーバーがライブラリ内のプレイリストファイル（.m3u など）から作ったものも含みます。サーバー管理者の場合は、他のユーザーの非公開プレイリストも含まれることがあります。",
    ),
    result_t: SharedString::new_static(
        "プレイリスト: {} · 追加したトラック: {} · 見つかった: {}/{}",
    ),
};

static DE: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importieren"),
    import_tooltip: SharedString::new_static("Playlists von einem Server importieren"),
    ai: SharedString::new_static("KI"),
    ai_tooltip: SharedString::new_static("Playlist mit einem KI-Chat erstellen"),
    title: SharedString::new_static("Playlists importieren"),
    description: SharedString::new_static(
        "Playlists werden einmalig vom Server kopiert, zurückgesendet wird nichts. Titel, die du auch als Dateien hast, werden von den Dateien abgespielt. Hast du schon eine Playlist mit demselben Namen, bekommt sie nur die fehlenden Titel.",
    ),
    scope_mine: SharedString::new_static("Nur meine Playlists"),
    scope_all: SharedString::new_static("Alle verfügbaren"),
    scope_all_hint: SharedString::new_static(
        "Auch Playlists anderer Benutzer und die, die der Server aus Playlist-Dateien (.m3u u. Ä.) in seiner Bibliothek erstellt. Bist du Server-Admin, können auch private Playlists anderer Benutzer dabei sein.",
    ),
    result_t: SharedString::new_static(
        "Playlists: {} · hinzugefügte Titel: {} · gefunden: {} von {}",
    ),
};

static FR: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importer"),
    import_tooltip: SharedString::new_static("Importer des playlists depuis un serveur"),
    ai: SharedString::new_static("IA"),
    ai_tooltip: SharedString::new_static("Créer une playlist avec un chat IA"),
    title: SharedString::new_static("Importer des playlists"),
    description: SharedString::new_static(
        "Les playlists sont copiées une seule fois depuis le serveur, et rien n'y est renvoyé. Les titres que vous avez aussi en fichiers sont lus depuis les fichiers. Si une playlist du même nom existe déjà, elle ne reçoit que les titres qui lui manquent.",
    ),
    scope_mine: SharedString::new_static("Seulement mes playlists"),
    scope_all: SharedString::new_static("Toutes celles disponibles"),
    scope_all_hint: SharedString::new_static(
        "Aussi les playlists des autres utilisateurs et celles que le serveur crée à partir de fichiers de playlist (.m3u, etc.) de sa bibliothèque. Si vous êtes administrateur du serveur, cela peut inclure les playlists privées des autres utilisateurs.",
    ),
    result_t: SharedString::new_static(
        "Playlists : {} · titres ajoutés : {} · trouvés : {} sur {}",
    ),
};

static KO: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("가져오기"),
    import_tooltip: SharedString::new_static("서버에서 재생목록 가져오기"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("AI 채팅으로 재생목록 만들기"),
    title: SharedString::new_static("재생목록 가져오기"),
    description: SharedString::new_static(
        "재생목록은 서버에서 한 번만 복사되며, 서버로 다시 보내는 것은 없습니다. 파일로도 가지고 있는 트랙은 파일에서 재생됩니다. 같은 이름의 재생목록이 이미 있으면 빠진 트랙만 추가됩니다.",
    ),
    scope_mine: SharedString::new_static("내 재생목록만"),
    scope_all: SharedString::new_static("접근 가능한 전체"),
    scope_all_hint: SharedString::new_static(
        "다른 사용자의 재생목록과 서버가 라이브러리의 재생목록 파일(.m3u 등)로 만든 재생목록도 포함됩니다. 서버 관리자라면 다른 사용자의 비공개 재생목록도 포함될 수 있습니다.",
    ),
    result_t: SharedString::new_static("재생목록: {} · 추가된 트랙: {} · 찾음: {}/{}"),
};

static IT: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importa"),
    import_tooltip: SharedString::new_static("Importa playlist da un server"),
    ai: SharedString::new_static("IA"),
    ai_tooltip: SharedString::new_static("Crea una playlist con una chat IA"),
    title: SharedString::new_static("Importa playlist"),
    description: SharedString::new_static(
        "Le playlist vengono copiate dal server una sola volta e non viene rinviato nulla. I brani che hai anche come file vengono riprodotti dai file. Se hai già una playlist con lo stesso nome, riceve solo i brani che le mancano.",
    ),
    scope_mine: SharedString::new_static("Solo le mie playlist"),
    scope_all: SharedString::new_static("Tutte quelle disponibili"),
    scope_all_hint: SharedString::new_static(
        "Anche le playlist di altri utenti e quelle che il server crea dai file di playlist (.m3u e simili) nella sua libreria. Se sei amministratore del server, possono esserci anche le playlist private di altri utenti.",
    ),
    result_t: SharedString::new_static("Playlist: {} · brani aggiunti: {} · trovati: {} su {}"),
};

static TR: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("İçe aktar"),
    import_tooltip: SharedString::new_static("Sunucudan çalma listelerini içe aktar"),
    ai: SharedString::new_static("Yapay zekâ"),
    ai_tooltip: SharedString::new_static("Yapay zekâ sohbetiyle çalma listesi oluştur"),
    title: SharedString::new_static("Çalma listelerini içe aktar"),
    description: SharedString::new_static(
        "Çalma listeleri sunucudan bir kez kopyalanır, geri bir şey gönderilmez. Dosya olarak da sahip olduğunuz parçalar dosyalardan çalınır. Aynı adda bir çalma listeniz varsa ona yalnızca eksik parçalar eklenir.",
    ),
    scope_mine: SharedString::new_static("Yalnızca benim çalma listelerim"),
    scope_all: SharedString::new_static("Erişilebilen tümü"),
    scope_all_hint: SharedString::new_static(
        "Diğer kullanıcıların çalma listeleri ve sunucunun kitaplığındaki çalma listesi dosyalarından (.m3u ve benzerleri) oluşturdukları da dahil. Sunucu yöneticisiyseniz aralarında diğer kullanıcıların özel çalma listeleri de olabilir.",
    ),
    result_t: SharedString::new_static("Çalma listesi: {} · eklenen parça: {} · bulunan: {}/{}"),
};

static PL: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importuj"),
    import_tooltip: SharedString::new_static("Importuj playlisty z serwera"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Utwórz playlistę z pomocą czatu AI"),
    title: SharedString::new_static("Import playlist"),
    description: SharedString::new_static(
        "Playlisty są kopiowane z serwera jednorazowo, nic nie jest odsyłane z powrotem. Utwory, które masz też jako pliki, są odtwarzane z plików. Jeśli masz już playlistę o tej samej nazwie, trafią do niej tylko brakujące utwory.",
    ),
    scope_mine: SharedString::new_static("Tylko moje playlisty"),
    scope_all: SharedString::new_static("Wszystkie dostępne"),
    scope_all_hint: SharedString::new_static(
        "Także playlisty innych użytkowników i te, które serwer tworzy z plików playlist (.m3u i podobnych) w swojej bibliotece. Jeśli jesteś administratorem serwera, mogą się wśród nich znaleźć także prywatne playlisty innych użytkowników.",
    ),
    result_t: SharedString::new_static("Playlisty: {} · dodane utwory: {} · znalezione: {} z {}"),
};

static NL: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importeren"),
    import_tooltip: SharedString::new_static("Afspeellijsten van een server importeren"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Maak een afspeellijst met een AI-chat"),
    title: SharedString::new_static("Afspeellijsten importeren"),
    description: SharedString::new_static(
        "Afspeellijsten worden één keer van de server gekopieerd; er wordt niets teruggestuurd. Nummers die je ook als bestand hebt, worden vanaf de bestanden afgespeeld. Heb je al een afspeellijst met dezelfde naam, dan krijgt die alleen de nummers die ontbreken.",
    ),
    scope_mine: SharedString::new_static("Alleen mijn afspeellijsten"),
    scope_all: SharedString::new_static("Alle beschikbare"),
    scope_all_hint: SharedString::new_static(
        "Ook afspeellijsten van andere gebruikers en de lijsten die de server maakt van afspeellijstbestanden (.m3u en dergelijke) in zijn bibliotheek. Ben je serverbeheerder, dan kunnen daar ook privé-afspeellijsten van andere gebruikers tussen zitten.",
    ),
    result_t: SharedString::new_static(
        "Afspeellijsten: {} · nummers toegevoegd: {} · gevonden: {} van {}",
    ),
};

static UK: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Імпорт"),
    import_tooltip: SharedString::new_static("Імпортувати плейлисти із сервера"),
    ai: SharedString::new_static("ШІ"),
    ai_tooltip: SharedString::new_static("Скласти плейлист за допомогою ШІ-чату"),
    title: SharedString::new_static("Імпорт плейлистів"),
    description: SharedString::new_static(
        "Плейлисти копіюються із сервера один раз, назад нічого не надсилається. Треки, які у вас є й файлами, грають із файлів. Якщо плейлист із такою назвою вже є, до нього додадуться лише відсутні треки.",
    ),
    scope_mine: SharedString::new_static("Лише мої плейлисти"),
    scope_all: SharedString::new_static("Усі доступні"),
    scope_all_hint: SharedString::new_static(
        "А також плейлисти інших користувачів і ті, що сервер складає з файлів плейлистів (.m3u тощо) у своїй медіатеці. Якщо ви адміністратор сервера, серед них можуть опинитися й чужі приватні плейлисти.",
    ),
    result_t: SharedString::new_static("Плейлистів: {} · додано треків: {} · знайдено: {} з {}"),
};

static VI: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Nhập"),
    import_tooltip: SharedString::new_static("Nhập danh sách phát từ máy chủ"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Tạo danh sách phát bằng trò chuyện AI"),
    title: SharedString::new_static("Nhập danh sách phát"),
    description: SharedString::new_static(
        "Danh sách phát được sao chép từ máy chủ một lần và không gửi lại gì. Bài hát bạn cũng có dưới dạng tệp sẽ phát từ tệp. Nếu đã có danh sách phát cùng tên, chỉ những bài còn thiếu được thêm vào.",
    ),
    scope_mine: SharedString::new_static("Chỉ danh sách phát của tôi"),
    scope_all: SharedString::new_static("Tất cả danh sách truy cập được"),
    scope_all_hint: SharedString::new_static(
        "Gồm cả danh sách phát của người dùng khác và danh sách máy chủ tạo từ tệp danh sách phát (.m3u và tương tự) trong thư viện của nó. Nếu bạn là quản trị viên máy chủ, có thể gồm cả danh sách phát riêng tư của người dùng khác.",
    ),
    result_t: SharedString::new_static(
        "Danh sách phát: {} · đã thêm bài hát: {} · tìm thấy: {}/{}",
    ),
};

static ID: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Impor"),
    import_tooltip: SharedString::new_static("Impor playlist dari server"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Buat playlist dengan chat AI"),
    title: SharedString::new_static("Impor playlist"),
    description: SharedString::new_static(
        "Playlist disalin dari server satu kali, dan tidak ada yang dikirim balik. Lagu yang juga ada sebagai file diputar dari file. Jika sudah ada playlist dengan nama yang sama, hanya lagu yang belum ada yang ditambahkan.",
    ),
    scope_mine: SharedString::new_static("Hanya playlist saya"),
    scope_all: SharedString::new_static("Semua yang tersedia"),
    scope_all_hint: SharedString::new_static(
        "Termasuk playlist milik pengguna lain dan playlist yang dibuat server dari file playlist (.m3u dan sejenisnya) di pustakanya. Bagi admin server, ini bisa mencakup playlist pribadi milik pengguna lain.",
    ),
    result_t: SharedString::new_static("Playlist: {} · lagu ditambahkan: {} · ditemukan: {}/{}"),
};

static TH: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("นำเข้า"),
    import_tooltip: SharedString::new_static("นำเข้าเพลย์ลิสต์จากเซิร์ฟเวอร์"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("สร้างเพลย์ลิสต์ด้วยแชต AI"),
    title: SharedString::new_static("นำเข้าเพลย์ลิสต์"),
    description: SharedString::new_static(
        "เพลย์ลิสต์จะถูกคัดลอกจากเซิร์ฟเวอร์เพียงครั้งเดียว และไม่มีการส่งอะไรกลับไป เพลงที่คุณมีเป็นไฟล์ด้วยจะเล่นจากไฟล์ หากมีเพลย์ลิสต์ชื่อเดียวกันอยู่แล้ว จะเพิ่มเฉพาะเพลงที่ยังขาดอยู่",
    ),
    scope_mine: SharedString::new_static("เฉพาะเพลย์ลิสต์ของฉัน"),
    scope_all: SharedString::new_static("ทั้งหมดที่เข้าถึงได้"),
    scope_all_hint: SharedString::new_static(
        "รวมถึงเพลย์ลิสต์ของผู้ใช้อื่น และเพลย์ลิสต์ที่เซิร์ฟเวอร์สร้างจากไฟล์เพลย์ลิสต์ (.m3u และอื่น ๆ) ในคลังของเซิร์ฟเวอร์ หากคุณเป็นผู้ดูแลเซิร์ฟเวอร์ อาจรวมถึงเพลย์ลิสต์ส่วนตัวของผู้ใช้อื่นด้วย",
    ),
    result_t: SharedString::new_static("เพลย์ลิสต์: {} · เพิ่มเพลง: {} · พบ: {}/{}"),
};

static CS: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importovat"),
    import_tooltip: SharedString::new_static("Importovat playlisty ze serveru"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Sestavit playlist pomocí AI chatu"),
    title: SharedString::new_static("Import playlistů"),
    description: SharedString::new_static(
        "Playlisty se ze serveru zkopírují jednou a nic se neposílá zpět. Skladby, které máte i jako soubory, se přehrávají ze souborů. Pokud už máte playlist se stejným názvem, přibudou do něj jen chybějící skladby.",
    ),
    scope_mine: SharedString::new_static("Jen moje playlisty"),
    scope_all: SharedString::new_static("Všechny dostupné"),
    scope_all_hint: SharedString::new_static(
        "Také playlisty jiných uživatelů a ty, které server vytváří ze souborů playlistů (.m3u a podobných) ve své knihovně. Pokud jste správce serveru, mohou mezi nimi být i soukromé playlisty jiných uživatelů.",
    ),
    result_t: SharedString::new_static("Playlisty: {} · přidané skladby: {} · nalezeno: {} z {}"),
};

static SV: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importera"),
    import_tooltip: SharedString::new_static("Importera spellistor från en server"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("Skapa en spellista med en AI-chatt"),
    title: SharedString::new_static("Importera spellistor"),
    description: SharedString::new_static(
        "Spellistorna kopieras från servern en gång, och inget skickas tillbaka. Låtar som du också har som filer spelas från filerna. Om du redan har en spellista med samma namn får den bara de låtar som saknas.",
    ),
    scope_mine: SharedString::new_static("Bara mina spellistor"),
    scope_all: SharedString::new_static("Alla tillgängliga"),
    scope_all_hint: SharedString::new_static(
        "Även andra användares spellistor och de som servern skapar av spellistfiler (.m3u och liknande) i sitt bibliotek. Om du är serveradministratör kan även andra användares privata spellistor komma med.",
    ),
    result_t: SharedString::new_static("Spellistor: {} · tillagda låtar: {} · hittade: {} av {}"),
};

static HI: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("इम्पोर्ट"),
    import_tooltip: SharedString::new_static("सर्वर से प्लेलिस्ट इम्पोर्ट करें"),
    ai: SharedString::new_static("AI"),
    ai_tooltip: SharedString::new_static("AI चैट से प्लेलिस्ट बनाएँ"),
    title: SharedString::new_static("प्लेलिस्ट इम्पोर्ट करें"),
    description: SharedString::new_static(
        "प्लेलिस्ट सर्वर से एक बार कॉपी होती हैं, वापस कुछ नहीं भेजा जाता। जो ट्रैक आपके पास फ़ाइलों के रूप में भी हैं, वे फ़ाइलों से चलते हैं। अगर उसी नाम की प्लेलिस्ट पहले से है, तो उसमें सिर्फ़ छूटे हुए ट्रैक जुड़ते हैं।",
    ),
    scope_mine: SharedString::new_static("सिर्फ़ मेरी प्लेलिस्ट"),
    scope_all: SharedString::new_static("सभी उपलब्ध"),
    scope_all_hint: SharedString::new_static(
        "दूसरे उपयोगकर्ताओं की प्लेलिस्ट और वे भी, जिन्हें सर्वर अपनी लाइब्रेरी की प्लेलिस्ट फ़ाइलों (.m3u आदि) से बनाता है। अगर आप सर्वर एडमिन हैं, तो इसमें दूसरे उपयोगकर्ताओं की निजी प्लेलिस्ट भी आ सकती हैं।",
    ),
    result_t: SharedString::new_static("प्लेलिस्ट: {} · जोड़े गए ट्रैक: {} · मिले: {}/{}"),
};

static ES: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importar"),
    import_tooltip: SharedString::new_static("Importar listas desde un servidor"),
    ai: SharedString::new_static("IA"),
    ai_tooltip: SharedString::new_static("Crear una lista con un chat de IA"),
    title: SharedString::new_static("Importar listas"),
    description: SharedString::new_static(
        "Las listas se copian del servidor una sola vez y no se envía nada de vuelta. Las canciones que también tienes como archivos se reproducen desde los archivos. Si ya tienes una lista con el mismo nombre, solo recibe las canciones que le faltan.",
    ),
    scope_mine: SharedString::new_static("Solo mis listas"),
    scope_all: SharedString::new_static("Todas las disponibles"),
    scope_all_hint: SharedString::new_static(
        "También las listas de otros usuarios y las que el servidor crea a partir de archivos de lista (.m3u y similares) de su biblioteca. Si eres administrador del servidor, puede incluir listas privadas de otros usuarios.",
    ),
    result_t: SharedString::new_static(
        "Listas: {} · canciones añadidas: {} · encontradas: {} de {}",
    ),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_keeps_its_placeholders() {
        for lang in Lang::all() {
            let s = for_lang(*lang);
            assert_eq!(s.result_t.matches("{}").count(), 4, "{lang:?}");
        }
    }
}
