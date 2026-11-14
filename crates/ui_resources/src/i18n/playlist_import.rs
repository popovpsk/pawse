use gpui::SharedString;

use super::{Lang, active, fill};

pub struct PlaylistImportStrings {
    pub import: SharedString,
    pub import_tooltip: SharedString,
    pub ai_tooltip: SharedString,
    pub title: SharedString,
    pub description: SharedString,
    pub scope_mine: SharedString,
    pub scope_all: SharedString,
    pub scope_all_hint: SharedString,
    pub result_t: SharedString,
    pub files_title: SharedString,
    pub files_description: SharedString,
    pub choose_files: SharedString,
    pub choose_folder: SharedString,
    pub no_files: SharedString,
    pub files_failed: SharedString,
    pub servers_title: SharedString,
    pub folder_show: SharedString,
    pub folder_hide: SharedString,
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
    import_tooltip: SharedString::new_static("Import playlists"),
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
    files_title: SharedString::new_static("Playlist files"),
    files_description: SharedString::new_static(
        "Playlists from .m3u, .m3u8 and .pls files are copied once. Tracks that aren't in your library are skipped. If you already have a playlist with the same name, it only gets the tracks it's missing.",
    ),
    choose_files: SharedString::new_static("Choose files…"),
    choose_folder: SharedString::new_static("Choose folder…"),
    files_failed: SharedString::new_static("Couldn't read the playlist files."),
    no_files: SharedString::new_static("No playlist files found."),
    servers_title: SharedString::new_static("Servers"),
    folder_show: SharedString::new_static("Show playlist files from your music folders"),
    folder_hide: SharedString::new_static("Hide playlist files from your music folders"),
};

static ZH: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("导入"),
    import_tooltip: SharedString::new_static("导入播放列表"),
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
    files_title: SharedString::new_static("播放列表文件"),
    files_description: SharedString::new_static(
        "来自 .m3u、.m3u8 和 .pls 文件的播放列表只会复制一次。不在你音乐库中的曲目会被跳过。如果已有同名播放列表，只会补上其中缺少的曲目。",
    ),
    choose_files: SharedString::new_static("选择文件…"),
    choose_folder: SharedString::new_static("选择文件夹…"),
    files_failed: SharedString::new_static("无法读取播放列表文件。"),
    no_files: SharedString::new_static("未找到播放列表文件。"),
    servers_title: SharedString::new_static("服务器"),
    folder_show: SharedString::new_static("显示音乐文件夹中的播放列表文件"),
    folder_hide: SharedString::new_static("隐藏音乐文件夹中的播放列表文件"),
};

static PT: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importar"),
    import_tooltip: SharedString::new_static("Importar playlists"),
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
    files_title: SharedString::new_static("Arquivos de playlist"),
    files_description: SharedString::new_static(
        "As playlists de arquivos .m3u, .m3u8 e .pls são copiadas uma única vez. As faixas que não estão na sua biblioteca são ignoradas. Se você já tem uma playlist com o mesmo nome, ela recebe só as faixas que faltam.",
    ),
    choose_files: SharedString::new_static("Escolher arquivos…"),
    choose_folder: SharedString::new_static("Escolher pasta…"),
    files_failed: SharedString::new_static("Não foi possível ler os arquivos de playlist."),
    no_files: SharedString::new_static("Nenhum arquivo de playlist encontrado."),
    servers_title: SharedString::new_static("Servidores"),
    folder_show: SharedString::new_static("Mostrar arquivos de playlist das suas pastas de música"),
    folder_hide: SharedString::new_static("Ocultar arquivos de playlist das suas pastas de música"),
};

static RU: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Импорт"),
    import_tooltip: SharedString::new_static("Импортировать плейлисты"),
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
    files_title: SharedString::new_static("Файлы плейлистов"),
    files_description: SharedString::new_static(
        "Плейлисты из файлов .m3u, .m3u8 и .pls копируются один раз. Треки, которых нет в вашей медиатеке, пропускаются. Если плейлист с таким именем уже есть, в него добавятся только недостающие треки.",
    ),
    choose_files: SharedString::new_static("Выбрать файлы…"),
    choose_folder: SharedString::new_static("Выбрать папку…"),
    files_failed: SharedString::new_static("Не удалось прочитать файлы плейлистов."),
    no_files: SharedString::new_static("Файлы плейлистов не найдены."),
    servers_title: SharedString::new_static("Серверы"),
    folder_show: SharedString::new_static("Показать файлы плейлистов из папок с музыкой"),
    folder_hide: SharedString::new_static("Скрыть файлы плейлистов из папок с музыкой"),
};

static JA: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("インポート"),
    import_tooltip: SharedString::new_static("プレイリストをインポート"),
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
    files_title: SharedString::new_static("プレイリストファイル"),
    files_description: SharedString::new_static(
        ".m3u、.m3u8、.pls ファイルのプレイリストは一度だけコピーされます。ライブラリにないトラックはスキップされます。同じ名前のプレイリストがすでにある場合は、足りないトラックだけが追加されます。",
    ),
    choose_files: SharedString::new_static("ファイルを選択…"),
    choose_folder: SharedString::new_static("フォルダを選択…"),
    files_failed: SharedString::new_static("プレイリストファイルを読み込めませんでした。"),
    no_files: SharedString::new_static("プレイリストファイルが見つかりませんでした。"),
    servers_title: SharedString::new_static("サーバー"),
    folder_show: SharedString::new_static("音楽フォルダ内のプレイリストファイルを表示"),
    folder_hide: SharedString::new_static("音楽フォルダ内のプレイリストファイルを非表示"),
};

static DE: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importieren"),
    import_tooltip: SharedString::new_static("Playlists importieren"),
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
    files_title: SharedString::new_static("Playlist-Dateien"),
    files_description: SharedString::new_static(
        "Playlists aus Dateien im Format .m3u, .m3u8 und .pls werden einmalig kopiert. Titel, die nicht in deiner Bibliothek sind, werden übersprungen. Hast du schon eine Playlist mit demselben Namen, bekommt sie nur die fehlenden Titel.",
    ),
    choose_files: SharedString::new_static("Dateien wählen…"),
    choose_folder: SharedString::new_static("Ordner wählen…"),
    files_failed: SharedString::new_static("Die Playlist-Dateien konnten nicht gelesen werden."),
    no_files: SharedString::new_static("Keine Playlist-Dateien gefunden."),
    servers_title: SharedString::new_static("Server"),
    folder_show: SharedString::new_static("Playlist-Dateien aus deinen Musikordnern anzeigen"),
    folder_hide: SharedString::new_static("Playlist-Dateien aus deinen Musikordnern ausblenden"),
};

static FR: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importer"),
    import_tooltip: SharedString::new_static("Importer des playlists"),
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
    files_title: SharedString::new_static("Fichiers de playlist"),
    files_description: SharedString::new_static(
        "Les playlists des fichiers .m3u, .m3u8 et .pls sont copiées une seule fois. Les titres absents de votre bibliothèque sont ignorés. Si une playlist du même nom existe déjà, elle ne reçoit que les titres qui lui manquent.",
    ),
    choose_files: SharedString::new_static("Choisir des fichiers…"),
    choose_folder: SharedString::new_static("Choisir un dossier…"),
    files_failed: SharedString::new_static("Impossible de lire les fichiers de playlist."),
    no_files: SharedString::new_static("Aucun fichier de playlist trouvé."),
    servers_title: SharedString::new_static("Serveurs"),
    folder_show: SharedString::new_static(
        "Afficher les fichiers de playlist de vos dossiers de musique",
    ),
    folder_hide: SharedString::new_static(
        "Masquer les fichiers de playlist de vos dossiers de musique",
    ),
};

static KO: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("가져오기"),
    import_tooltip: SharedString::new_static("재생목록 가져오기"),
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
    files_title: SharedString::new_static("재생목록 파일"),
    files_description: SharedString::new_static(
        ".m3u, .m3u8, .pls 파일의 재생목록은 한 번만 복사됩니다. 라이브러리에 없는 트랙은 건너뜁니다. 같은 이름의 재생목록이 이미 있으면 빠진 트랙만 추가됩니다.",
    ),
    choose_files: SharedString::new_static("파일 선택…"),
    choose_folder: SharedString::new_static("폴더 선택…"),
    files_failed: SharedString::new_static("재생목록 파일을 읽을 수 없습니다."),
    no_files: SharedString::new_static("재생목록 파일을 찾지 못했습니다."),
    servers_title: SharedString::new_static("서버"),
    folder_show: SharedString::new_static("음악 폴더의 재생목록 파일 표시"),
    folder_hide: SharedString::new_static("음악 폴더의 재생목록 파일 숨기기"),
};

static IT: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importa"),
    import_tooltip: SharedString::new_static("Importa playlist"),
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
    files_title: SharedString::new_static("File di playlist"),
    files_description: SharedString::new_static(
        "Le playlist dai file .m3u, .m3u8 e .pls vengono copiate una sola volta. I brani che non sono nella tua libreria vengono saltati. Se hai già una playlist con lo stesso nome, riceve solo i brani che le mancano.",
    ),
    choose_files: SharedString::new_static("Scegli file…"),
    choose_folder: SharedString::new_static("Scegli cartella…"),
    files_failed: SharedString::new_static("Impossibile leggere i file di playlist."),
    no_files: SharedString::new_static("Nessun file di playlist trovato."),
    servers_title: SharedString::new_static("Server"),
    folder_show: SharedString::new_static("Mostra i file di playlist delle tue cartelle musicali"),
    folder_hide: SharedString::new_static(
        "Nascondi i file di playlist delle tue cartelle musicali",
    ),
};

static TR: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("İçe aktar"),
    import_tooltip: SharedString::new_static("Çalma listelerini içe aktar"),
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
    files_title: SharedString::new_static("Çalma listesi dosyaları"),
    files_description: SharedString::new_static(
        ".m3u, .m3u8 ve .pls dosyalarındaki çalma listeleri bir kez kopyalanır. Kitaplığınızda olmayan parçalar atlanır. Aynı adda bir çalma listeniz varsa ona yalnızca eksik parçalar eklenir.",
    ),
    choose_files: SharedString::new_static("Dosya seç…"),
    choose_folder: SharedString::new_static("Klasör seç…"),
    files_failed: SharedString::new_static("Çalma listesi dosyaları okunamadı."),
    no_files: SharedString::new_static("Çalma listesi dosyası bulunamadı."),
    servers_title: SharedString::new_static("Sunucular"),
    folder_show: SharedString::new_static(
        "Müzik klasörlerinizdeki çalma listesi dosyalarını göster",
    ),
    folder_hide: SharedString::new_static(
        "Müzik klasörlerinizdeki çalma listesi dosyalarını gizle",
    ),
};

static PL: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importuj"),
    import_tooltip: SharedString::new_static("Importuj playlisty"),
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
    files_title: SharedString::new_static("Pliki playlist"),
    files_description: SharedString::new_static(
        "Playlisty z plików .m3u, .m3u8 i .pls są kopiowane jednorazowo. Utwory, których nie ma w bibliotece, są pomijane. Jeśli masz już playlistę o tej samej nazwie, trafią do niej tylko brakujące utwory.",
    ),
    choose_files: SharedString::new_static("Wybierz pliki…"),
    choose_folder: SharedString::new_static("Wybierz folder…"),
    files_failed: SharedString::new_static("Nie udało się odczytać plików playlist."),
    no_files: SharedString::new_static("Nie znaleziono plików playlist."),
    servers_title: SharedString::new_static("Serwery"),
    folder_show: SharedString::new_static("Pokaż pliki playlist z folderów z muzyką"),
    folder_hide: SharedString::new_static("Ukryj pliki playlist z folderów z muzyką"),
};

static NL: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importeren"),
    import_tooltip: SharedString::new_static("Afspeellijsten importeren"),
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
    files_title: SharedString::new_static("Afspeellijstbestanden"),
    files_description: SharedString::new_static(
        "Afspeellijsten uit bestanden (.m3u, .m3u8 en .pls) worden één keer gekopieerd. Nummers die niet in je bibliotheek staan, worden overgeslagen. Heb je al een afspeellijst met dezelfde naam, dan krijgt die alleen de nummers die ontbreken.",
    ),
    choose_files: SharedString::new_static("Bestanden kiezen…"),
    choose_folder: SharedString::new_static("Map kiezen…"),
    files_failed: SharedString::new_static("Kan de afspeellijstbestanden niet lezen."),
    no_files: SharedString::new_static("Geen afspeellijstbestanden gevonden."),
    servers_title: SharedString::new_static("Servers"),
    folder_show: SharedString::new_static("Afspeellijstbestanden uit je muziekmappen tonen"),
    folder_hide: SharedString::new_static("Afspeellijstbestanden uit je muziekmappen verbergen"),
};

static UK: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Імпорт"),
    import_tooltip: SharedString::new_static("Імпортувати плейлисти"),
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
    files_title: SharedString::new_static("Файли плейлистів"),
    files_description: SharedString::new_static(
        "Плейлисти з файлів .m3u, .m3u8 і .pls копіюються один раз. Треки, яких немає у вашій медіатеці, пропускаються. Якщо плейлист із такою назвою вже є, до нього додадуться лише відсутні треки.",
    ),
    choose_files: SharedString::new_static("Вибрати файли…"),
    choose_folder: SharedString::new_static("Вибрати папку…"),
    files_failed: SharedString::new_static("Не вдалося прочитати файли плейлистів."),
    no_files: SharedString::new_static("Файли плейлистів не знайдено."),
    servers_title: SharedString::new_static("Сервери"),
    folder_show: SharedString::new_static("Показати файли плейлистів із папок з музикою"),
    folder_hide: SharedString::new_static("Сховати файли плейлистів із папок з музикою"),
};

static VI: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Nhập"),
    import_tooltip: SharedString::new_static("Nhập danh sách phát"),
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
    files_title: SharedString::new_static("Tệp danh sách phát"),
    files_description: SharedString::new_static(
        "Danh sách phát từ tệp .m3u, .m3u8 và .pls được sao chép một lần. Bài hát không có trong thư viện của bạn sẽ bị bỏ qua. Nếu đã có danh sách phát cùng tên, chỉ những bài còn thiếu được thêm vào.",
    ),
    choose_files: SharedString::new_static("Chọn tệp…"),
    choose_folder: SharedString::new_static("Chọn thư mục…"),
    files_failed: SharedString::new_static("Không đọc được tệp danh sách phát."),
    no_files: SharedString::new_static("Không tìm thấy tệp danh sách phát."),
    servers_title: SharedString::new_static("Máy chủ"),
    folder_show: SharedString::new_static("Hiện tệp danh sách phát trong thư mục nhạc của bạn"),
    folder_hide: SharedString::new_static("Ẩn tệp danh sách phát trong thư mục nhạc của bạn"),
};

static ID: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Impor"),
    import_tooltip: SharedString::new_static("Impor playlist"),
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
    files_title: SharedString::new_static("File playlist"),
    files_description: SharedString::new_static(
        "Playlist dari file .m3u, .m3u8, dan .pls disalin satu kali. Lagu yang tidak ada di pustaka Anda dilewati. Jika sudah ada playlist dengan nama yang sama, hanya lagu yang belum ada yang ditambahkan.",
    ),
    choose_files: SharedString::new_static("Pilih file…"),
    choose_folder: SharedString::new_static("Pilih folder…"),
    files_failed: SharedString::new_static("Tidak dapat membaca file playlist."),
    no_files: SharedString::new_static("Tidak ada file playlist yang ditemukan."),
    servers_title: SharedString::new_static("Server"),
    folder_show: SharedString::new_static("Tampilkan file playlist dari folder musik Anda"),
    folder_hide: SharedString::new_static("Sembunyikan file playlist dari folder musik Anda"),
};

static TH: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("นำเข้า"),
    import_tooltip: SharedString::new_static("นำเข้าเพลย์ลิสต์"),
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
    files_title: SharedString::new_static("ไฟล์เพลย์ลิสต์"),
    files_description: SharedString::new_static(
        "เพลย์ลิสต์จากไฟล์ .m3u, .m3u8 และ .pls จะถูกคัดลอกเพียงครั้งเดียว เพลงที่ไม่มีในคลังเพลงของคุณจะถูกข้าม หากมีเพลย์ลิสต์ชื่อเดียวกันอยู่แล้ว จะเพิ่มเฉพาะเพลงที่ยังขาดอยู่",
    ),
    choose_files: SharedString::new_static("เลือกไฟล์…"),
    choose_folder: SharedString::new_static("เลือกโฟลเดอร์…"),
    files_failed: SharedString::new_static("อ่านไฟล์เพลย์ลิสต์ไม่ได้"),
    no_files: SharedString::new_static("ไม่พบไฟล์เพลย์ลิสต์"),
    servers_title: SharedString::new_static("เซิร์ฟเวอร์"),
    folder_show: SharedString::new_static("แสดงไฟล์เพลย์ลิสต์จากโฟลเดอร์เพลงของคุณ"),
    folder_hide: SharedString::new_static("ซ่อนไฟล์เพลย์ลิสต์จากโฟลเดอร์เพลงของคุณ"),
};

static CS: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importovat"),
    import_tooltip: SharedString::new_static("Importovat playlisty"),
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
    files_title: SharedString::new_static("Soubory playlistů"),
    files_description: SharedString::new_static(
        "Playlisty ze souborů .m3u, .m3u8 a .pls se zkopírují jednou. Skladby, které nejsou ve vaší knihovně, se přeskočí. Pokud už máte playlist se stejným názvem, přibudou do něj jen chybějící skladby.",
    ),
    choose_files: SharedString::new_static("Vybrat soubory…"),
    choose_folder: SharedString::new_static("Vybrat složku…"),
    files_failed: SharedString::new_static("Soubory playlistů se nepodařilo přečíst."),
    no_files: SharedString::new_static("Nebyly nalezeny žádné soubory playlistů."),
    servers_title: SharedString::new_static("Servery"),
    folder_show: SharedString::new_static("Zobrazit soubory playlistů z vašich hudebních složek"),
    folder_hide: SharedString::new_static("Skrýt soubory playlistů z vašich hudebních složek"),
};

static SV: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importera"),
    import_tooltip: SharedString::new_static("Importera spellistor"),
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
    files_title: SharedString::new_static("Spellistfiler"),
    files_description: SharedString::new_static(
        "Spellistor från filer i formaten .m3u, .m3u8 och .pls kopieras en gång. Låtar som inte finns i ditt bibliotek hoppas över. Om du redan har en spellista med samma namn får den bara de låtar som saknas.",
    ),
    choose_files: SharedString::new_static("Välj filer…"),
    choose_folder: SharedString::new_static("Välj mapp…"),
    files_failed: SharedString::new_static("Det gick inte att läsa spellistfilerna."),
    no_files: SharedString::new_static("Inga spellistfiler hittades."),
    servers_title: SharedString::new_static("Servrar"),
    folder_show: SharedString::new_static("Visa spellistfiler från dina musikmappar"),
    folder_hide: SharedString::new_static("Dölj spellistfiler från dina musikmappar"),
};

static HI: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("इम्पोर्ट"),
    import_tooltip: SharedString::new_static("प्लेलिस्ट इम्पोर्ट करें"),
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
    files_title: SharedString::new_static("प्लेलिस्ट फ़ाइलें"),
    files_description: SharedString::new_static(
        ".m3u, .m3u8 और .pls फ़ाइलों की प्लेलिस्ट एक बार कॉपी होती हैं। जो ट्रैक आपकी लाइब्रेरी में नहीं हैं, वे छोड़ दिए जाते हैं। अगर उसी नाम की प्लेलिस्ट पहले से है, तो उसमें सिर्फ़ छूटे हुए ट्रैक जुड़ते हैं।",
    ),
    choose_files: SharedString::new_static("फ़ाइलें चुनें…"),
    choose_folder: SharedString::new_static("फ़ोल्डर चुनें…"),
    files_failed: SharedString::new_static("प्लेलिस्ट फ़ाइलें पढ़ी नहीं जा सकीं।"),
    no_files: SharedString::new_static("कोई प्लेलिस्ट फ़ाइल नहीं मिली।"),
    servers_title: SharedString::new_static("सर्वर"),
    folder_show: SharedString::new_static("अपने संगीत फ़ोल्डरों की प्लेलिस्ट फ़ाइलें दिखाएँ"),
    folder_hide: SharedString::new_static("अपने संगीत फ़ोल्डरों की प्लेलिस्ट फ़ाइलें छिपाएँ"),
};

static ES: PlaylistImportStrings = PlaylistImportStrings {
    import: SharedString::new_static("Importar"),
    import_tooltip: SharedString::new_static("Importar listas"),
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
    files_title: SharedString::new_static("Archivos de lista"),
    files_description: SharedString::new_static(
        "Las listas de archivos .m3u, .m3u8 y .pls se copian una sola vez. Las canciones que no están en tu biblioteca se omiten. Si ya tienes una lista con el mismo nombre, solo recibe las canciones que le faltan.",
    ),
    choose_files: SharedString::new_static("Elegir archivos…"),
    choose_folder: SharedString::new_static("Elegir carpeta…"),
    files_failed: SharedString::new_static("No se pudieron leer los archivos de lista."),
    no_files: SharedString::new_static("No se encontraron archivos de lista."),
    servers_title: SharedString::new_static("Servidores"),
    folder_show: SharedString::new_static("Mostrar archivos de lista de tus carpetas de música"),
    folder_hide: SharedString::new_static("Ocultar archivos de lista de tus carpetas de música"),
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
