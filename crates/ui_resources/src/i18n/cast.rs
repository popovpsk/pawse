use gpui::SharedString;

use super::{Lang, active, fill};

pub struct CastStrings {
    pub streaming: SharedString,
    pub searching: SharedString,
    pub none_found: SharedString,
    pub connecting: SharedString,
    pub playing_on_t: SharedString,
    pub connect_failed_t: SharedString,
    pub connection_lost_t: SharedString,
    pub cast_enabled: SharedString,
    pub cast_enabled_desc: SharedString,
    pub device_volume: SharedString,
    pub device_volume_desc: SharedString,
}

impl CastStrings {
    pub fn playing_on(&self, device: &str) -> String {
        fill(&self.playing_on_t, &[device])
    }

    pub fn connect_failed(&self, device: &str, error: &str) -> String {
        fill(&self.connect_failed_t, &[device, error])
    }

    pub fn connection_lost(&self, device: &str) -> String {
        fill(&self.connection_lost_t, &[device])
    }
}

pub fn cast_strings() -> &'static CastStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static CastStrings {
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

static EN: CastStrings = CastStrings {
    streaming: SharedString::new_static("Streaming"),
    searching: SharedString::new_static("Looking for devices…"),
    none_found: SharedString::new_static("No devices found"),
    connecting: SharedString::new_static("Connecting…"),
    playing_on_t: SharedString::new_static("Playing on {}"),
    connect_failed_t: SharedString::new_static("Couldn't connect to {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Lost connection to {}. Playback is back on this computer.",
    ),
    cast_enabled: SharedString::new_static("Stream to network devices"),
    cast_enabled_desc: SharedString::new_static(
        "Show AirPlay, Chromecast and DLNA devices in the list of audio devices.",
    ),
    device_volume: SharedString::new_static("Change the device's volume"),
    device_volume_desc: SharedString::new_static(
        "While streaming, the volume slider sets the volume on the device itself. When off, the device keeps its own volume: AirPlay gets the sound at the player's volume, Chromecast and DLNA get the file as it is, with the slider locked.",
    ),
};

static ZH: CastStrings = CastStrings {
    streaming: SharedString::new_static("串流"),
    searching: SharedString::new_static("正在搜索设备…"),
    none_found: SharedString::new_static("未找到设备"),
    connecting: SharedString::new_static("正在连接…"),
    playing_on_t: SharedString::new_static("正在 {} 上播放"),
    connect_failed_t: SharedString::new_static("无法连接到 {}：{}"),
    connection_lost_t: SharedString::new_static("与 {} 的连接已断开，已改回在本机播放。"),
    cast_enabled: SharedString::new_static("串流到网络设备"),
    cast_enabled_desc: SharedString::new_static(
        "在音频设备列表中显示 AirPlay、Chromecast 和 DLNA 设备。",
    ),
    device_volume: SharedString::new_static("调节设备音量"),
    device_volume_desc: SharedString::new_static(
        "串流时，音量滑块调节设备本身的音量。关闭后，设备保持自己的音量：AirPlay 收到已按播放器音量调整的声音，Chromecast 和 DLNA 收到原始文件，滑块被锁定。",
    ),
};

static PT: CastStrings = CastStrings {
    streaming: SharedString::new_static("Transmissão"),
    searching: SharedString::new_static("Procurando dispositivos…"),
    none_found: SharedString::new_static("Nenhum dispositivo encontrado"),
    connecting: SharedString::new_static("Conectando…"),
    playing_on_t: SharedString::new_static("Tocando em {}"),
    connect_failed_t: SharedString::new_static("Não foi possível conectar a {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Conexão com {} perdida. A reprodução voltou para este computador.",
    ),
    cast_enabled: SharedString::new_static("Transmitir para dispositivos na rede"),
    cast_enabled_desc: SharedString::new_static(
        "Mostra dispositivos AirPlay, Chromecast e DLNA na lista de dispositivos de áudio.",
    ),
    device_volume: SharedString::new_static("Alterar o volume do dispositivo"),
    device_volume_desc: SharedString::new_static(
        "Durante a transmissão, o controle de volume altera o volume do próprio dispositivo. Desativado, o dispositivo mantém o próprio volume: o AirPlay recebe o som já no volume do player, e o Chromecast e o DLNA recebem o arquivo como está, com o controle bloqueado.",
    ),
};

static RU: CastStrings = CastStrings {
    streaming: SharedString::new_static("Трансляция"),
    searching: SharedString::new_static("Поиск устройств…"),
    none_found: SharedString::new_static("Устройства не найдены"),
    connecting: SharedString::new_static("Подключение…"),
    playing_on_t: SharedString::new_static("Играет на {}"),
    connect_failed_t: SharedString::new_static("Не удалось подключиться к {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Связь с {} потеряна. Воспроизведение вернулось на этот компьютер.",
    ),
    cast_enabled: SharedString::new_static("Трансляция на сетевые устройства"),
    cast_enabled_desc: SharedString::new_static(
        "Показывать устройства AirPlay, Chromecast и DLNA в списке аудиоустройств.",
    ),
    device_volume: SharedString::new_static("Менять громкость устройства"),
    device_volume_desc: SharedString::new_static(
        "Во время трансляции ползунок громкости меняет громкость на самом устройстве. Если выключено, устройство сохраняет свою громкость: на AirPlay звук уходит уже с громкостью плеера, а Chromecast и DLNA получают файл как есть, и ползунок заблокирован.",
    ),
};

static JA: CastStrings = CastStrings {
    streaming: SharedString::new_static("ストリーミング"),
    searching: SharedString::new_static("デバイスを検索中…"),
    none_found: SharedString::new_static("デバイスが見つかりません"),
    connecting: SharedString::new_static("接続中…"),
    playing_on_t: SharedString::new_static("{} で再生中"),
    connect_failed_t: SharedString::new_static("{} に接続できません: {}"),
    connection_lost_t: SharedString::new_static(
        "{} との接続が切れました。このコンピュータでの再生に戻りました。",
    ),
    cast_enabled: SharedString::new_static("ネットワーク機器へのストリーミング"),
    cast_enabled_desc: SharedString::new_static(
        "オーディオデバイスの一覧に AirPlay、Chromecast、DLNA 機器を表示します。",
    ),
    device_volume: SharedString::new_static("機器の音量を変更"),
    device_volume_desc: SharedString::new_static(
        "ストリーミング中、音量スライダーで機器本体の音量を変更します。オフにすると機器は自身の音量のままです。AirPlay にはプレーヤーの音量を適用した音が送られ、Chromecast と DLNA にはファイルがそのまま送られ、スライダーはロックされます。",
    ),
};

static DE: CastStrings = CastStrings {
    streaming: SharedString::new_static("Streaming"),
    searching: SharedString::new_static("Suche nach Geräten…"),
    none_found: SharedString::new_static("Keine Geräte gefunden"),
    connecting: SharedString::new_static("Verbinde…"),
    playing_on_t: SharedString::new_static("Wiedergabe auf {}"),
    connect_failed_t: SharedString::new_static("Verbindung mit {} fehlgeschlagen: {}"),
    connection_lost_t: SharedString::new_static(
        "Verbindung zu {} verloren. Die Wiedergabe läuft wieder auf diesem Computer.",
    ),
    cast_enabled: SharedString::new_static("Streaming an Netzwerkgeräte"),
    cast_enabled_desc: SharedString::new_static(
        "Zeigt AirPlay-, Chromecast- und DLNA-Geräte in der Liste der Audiogeräte.",
    ),
    device_volume: SharedString::new_static("Lautstärke des Geräts ändern"),
    device_volume_desc: SharedString::new_static(
        "Beim Streaming ändert der Lautstärkeregler die Lautstärke am Gerät selbst. Aus: Das Gerät behält seine eigene Lautstärke; AirPlay bekommt den Ton bereits in der Lautstärke des Players, Chromecast und DLNA bekommen die Datei unverändert, und der Regler ist gesperrt.",
    ),
};

static FR: CastStrings = CastStrings {
    streaming: SharedString::new_static("Diffusion"),
    searching: SharedString::new_static("Recherche d'appareils…"),
    none_found: SharedString::new_static("Aucun appareil trouvé"),
    connecting: SharedString::new_static("Connexion…"),
    playing_on_t: SharedString::new_static("Lecture sur {}"),
    connect_failed_t: SharedString::new_static("Impossible de se connecter à {} : {}"),
    connection_lost_t: SharedString::new_static(
        "Connexion à {} perdue. La lecture revient sur cet ordinateur.",
    ),
    cast_enabled: SharedString::new_static("Diffusion vers les appareils du réseau"),
    cast_enabled_desc: SharedString::new_static(
        "Affiche les appareils AirPlay, Chromecast et DLNA dans la liste des appareils audio.",
    ),
    device_volume: SharedString::new_static("Régler le volume de l'appareil"),
    device_volume_desc: SharedString::new_static(
        "Pendant la diffusion, le curseur de volume règle le volume de l'appareil lui-même. Désactivé, l'appareil garde son propre volume : AirPlay reçoit le son déjà au volume du lecteur, Chromecast et DLNA reçoivent le fichier tel quel et le curseur est verrouillé.",
    ),
};

static KO: CastStrings = CastStrings {
    streaming: SharedString::new_static("스트리밍"),
    searching: SharedString::new_static("기기 검색 중…"),
    none_found: SharedString::new_static("기기를 찾을 수 없음"),
    connecting: SharedString::new_static("연결 중…"),
    playing_on_t: SharedString::new_static("{}에서 재생 중"),
    connect_failed_t: SharedString::new_static("{}에 연결할 수 없음: {}"),
    connection_lost_t: SharedString::new_static(
        "{}와(과)의 연결이 끊겼습니다. 이 컴퓨터에서 재생을 계속합니다.",
    ),
    cast_enabled: SharedString::new_static("네트워크 기기로 스트리밍"),
    cast_enabled_desc: SharedString::new_static(
        "오디오 장치 목록에 AirPlay, Chromecast, DLNA 기기를 표시합니다.",
    ),
    device_volume: SharedString::new_static("기기 볼륨 조절"),
    device_volume_desc: SharedString::new_static(
        "스트리밍 중에는 볼륨 슬라이더가 기기 자체의 볼륨을 조절합니다. 끄면 기기가 자체 볼륨을 유지합니다. AirPlay에는 플레이어 볼륨이 적용된 소리가 전송되고, Chromecast와 DLNA에는 파일이 그대로 전송되며 슬라이더는 잠깁니다.",
    ),
};

static IT: CastStrings = CastStrings {
    streaming: SharedString::new_static("Streaming"),
    searching: SharedString::new_static("Ricerca dispositivi…"),
    none_found: SharedString::new_static("Nessun dispositivo trovato"),
    connecting: SharedString::new_static("Connessione…"),
    playing_on_t: SharedString::new_static("In riproduzione su {}"),
    connect_failed_t: SharedString::new_static("Impossibile connettersi a {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Connessione con {} persa. La riproduzione è tornata su questo computer.",
    ),
    cast_enabled: SharedString::new_static("Streaming verso dispositivi di rete"),
    cast_enabled_desc: SharedString::new_static(
        "Mostra i dispositivi AirPlay, Chromecast e DLNA nell'elenco dei dispositivi audio.",
    ),
    device_volume: SharedString::new_static("Regola il volume del dispositivo"),
    device_volume_desc: SharedString::new_static(
        "Durante lo streaming il cursore del volume regola il volume del dispositivo stesso. Se disattivato, il dispositivo mantiene il proprio volume: AirPlay riceve l'audio già al volume del lettore, Chromecast e DLNA ricevono il file così com'è e il cursore è bloccato.",
    ),
};

static TR: CastStrings = CastStrings {
    streaming: SharedString::new_static("Yayın"),
    searching: SharedString::new_static("Cihazlar aranıyor…"),
    none_found: SharedString::new_static("Cihaz bulunamadı"),
    connecting: SharedString::new_static("Bağlanıyor…"),
    playing_on_t: SharedString::new_static("{} üzerinde çalıyor"),
    connect_failed_t: SharedString::new_static("{} cihazına bağlanılamadı: {}"),
    connection_lost_t: SharedString::new_static(
        "{} ile bağlantı kesildi. Çalma bu bilgisayara döndü.",
    ),
    cast_enabled: SharedString::new_static("Ağ cihazlarına yayın"),
    cast_enabled_desc: SharedString::new_static(
        "Ses cihazları listesinde AirPlay, Chromecast ve DLNA cihazlarını gösterir.",
    ),
    device_volume: SharedString::new_static("Cihazın ses düzeyini değiştir"),
    device_volume_desc: SharedString::new_static(
        "Yayın sırasında ses kaydırıcısı cihazın kendi ses düzeyini değiştirir. Kapalıyken cihaz kendi ses düzeyini korur: AirPlay sesi oynatıcının ses düzeyinde alır, Chromecast ve DLNA dosyayı olduğu gibi alır ve kaydırıcı kilitlenir.",
    ),
};

static PL: CastStrings = CastStrings {
    streaming: SharedString::new_static("Strumieniowanie"),
    searching: SharedString::new_static("Szukanie urządzeń…"),
    none_found: SharedString::new_static("Nie znaleziono urządzeń"),
    connecting: SharedString::new_static("Łączenie…"),
    playing_on_t: SharedString::new_static("Odtwarzanie na {}"),
    connect_failed_t: SharedString::new_static("Nie udało się połączyć z {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Utracono połączenie z {}. Odtwarzanie wróciło na ten komputer.",
    ),
    cast_enabled: SharedString::new_static("Strumieniowanie do urządzeń w sieci"),
    cast_enabled_desc: SharedString::new_static(
        "Pokazuje urządzenia AirPlay, Chromecast i DLNA na liście urządzeń audio.",
    ),
    device_volume: SharedString::new_static("Zmieniaj głośność urządzenia"),
    device_volume_desc: SharedString::new_static(
        "Podczas strumieniowania suwak głośności zmienia głośność samego urządzenia. Gdy wyłączone, urządzenie zachowuje własną głośność: AirPlay dostaje dźwięk już z głośnością odtwarzacza, a Chromecast i DLNA dostają plik bez zmian i suwak jest zablokowany.",
    ),
};

static NL: CastStrings = CastStrings {
    streaming: SharedString::new_static("Streamen"),
    searching: SharedString::new_static("Apparaten zoeken…"),
    none_found: SharedString::new_static("Geen apparaten gevonden"),
    connecting: SharedString::new_static("Verbinden…"),
    playing_on_t: SharedString::new_static("Speelt af op {}"),
    connect_failed_t: SharedString::new_static("Kan geen verbinding maken met {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Verbinding met {} verbroken. Het afspelen gaat verder op deze computer.",
    ),
    cast_enabled: SharedString::new_static("Streamen naar netwerkapparaten"),
    cast_enabled_desc: SharedString::new_static(
        "Toont AirPlay-, Chromecast- en DLNA-apparaten in de lijst met audioapparaten.",
    ),
    device_volume: SharedString::new_static("Volume van het apparaat aanpassen"),
    device_volume_desc: SharedString::new_static(
        "Tijdens het streamen past de volumeschuif het volume van het apparaat zelf aan. Uit: het apparaat houdt zijn eigen volume; AirPlay krijgt het geluid al op het volume van de speler, Chromecast en DLNA krijgen het bestand ongewijzigd en de schuif is vergrendeld.",
    ),
};

static UK: CastStrings = CastStrings {
    streaming: SharedString::new_static("Трансляція"),
    searching: SharedString::new_static("Пошук пристроїв…"),
    none_found: SharedString::new_static("Пристроїв не знайдено"),
    connecting: SharedString::new_static("Підключення…"),
    playing_on_t: SharedString::new_static("Грає на {}"),
    connect_failed_t: SharedString::new_static("Не вдалося підключитися до {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Зв'язок з {} втрачено. Відтворення повернулося на цей комп'ютер.",
    ),
    cast_enabled: SharedString::new_static("Трансляція на мережеві пристрої"),
    cast_enabled_desc: SharedString::new_static(
        "Показувати пристрої AirPlay, Chromecast і DLNA у списку аудіопристроїв.",
    ),
    device_volume: SharedString::new_static("Змінювати гучність пристрою"),
    device_volume_desc: SharedString::new_static(
        "Під час трансляції повзунок гучності змінює гучність на самому пристрої. Якщо вимкнено, пристрій зберігає свою гучність: на AirPlay звук іде вже з гучністю плеєра, а Chromecast і DLNA отримують файл як є, і повзунок заблоковано.",
    ),
};

static VI: CastStrings = CastStrings {
    streaming: SharedString::new_static("Phát trực tuyến"),
    searching: SharedString::new_static("Đang tìm thiết bị…"),
    none_found: SharedString::new_static("Không tìm thấy thiết bị"),
    connecting: SharedString::new_static("Đang kết nối…"),
    playing_on_t: SharedString::new_static("Đang phát trên {}"),
    connect_failed_t: SharedString::new_static("Không thể kết nối với {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Mất kết nối với {}. Đã chuyển phát lại về máy tính này.",
    ),
    cast_enabled: SharedString::new_static("Phát trực tuyến tới thiết bị mạng"),
    cast_enabled_desc: SharedString::new_static(
        "Hiển thị thiết bị AirPlay, Chromecast và DLNA trong danh sách thiết bị âm thanh.",
    ),
    device_volume: SharedString::new_static("Chỉnh âm lượng của thiết bị"),
    device_volume_desc: SharedString::new_static(
        "Khi phát trực tuyến, thanh âm lượng chỉnh âm lượng trên chính thiết bị. Khi tắt, thiết bị giữ âm lượng của riêng nó: AirPlay nhận âm thanh đã theo âm lượng của trình phát, còn Chromecast và DLNA nhận nguyên tệp và thanh âm lượng bị khóa.",
    ),
};

static ID: CastStrings = CastStrings {
    streaming: SharedString::new_static("Streaming"),
    searching: SharedString::new_static("Mencari perangkat…"),
    none_found: SharedString::new_static("Tidak ada perangkat"),
    connecting: SharedString::new_static("Menghubungkan…"),
    playing_on_t: SharedString::new_static("Diputar di {}"),
    connect_failed_t: SharedString::new_static("Tidak dapat terhubung ke {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Koneksi ke {} terputus. Pemutaran kembali ke komputer ini.",
    ),
    cast_enabled: SharedString::new_static("Streaming ke perangkat jaringan"),
    cast_enabled_desc: SharedString::new_static(
        "Menampilkan perangkat AirPlay, Chromecast, dan DLNA di daftar perangkat audio.",
    ),
    device_volume: SharedString::new_static("Ubah volume perangkat"),
    device_volume_desc: SharedString::new_static(
        "Saat streaming, penggeser volume mengubah volume perangkat itu sendiri. Jika nonaktif, perangkat mempertahankan volumenya sendiri: AirPlay menerima suara yang sudah sesuai volume pemutar, sedangkan Chromecast dan DLNA menerima file apa adanya dan penggeser dikunci.",
    ),
};

static TH: CastStrings = CastStrings {
    streaming: SharedString::new_static("สตรีมมิง"),
    searching: SharedString::new_static("กำลังค้นหาอุปกรณ์…"),
    none_found: SharedString::new_static("ไม่พบอุปกรณ์"),
    connecting: SharedString::new_static("กำลังเชื่อมต่อ…"),
    playing_on_t: SharedString::new_static("กำลังเล่นบน {}"),
    connect_failed_t: SharedString::new_static("เชื่อมต่อกับ {} ไม่ได้: {}"),
    connection_lost_t: SharedString::new_static(
        "การเชื่อมต่อกับ {} ขาดหาย กลับมาเล่นบนคอมพิวเตอร์เครื่องนี้แล้ว",
    ),
    cast_enabled: SharedString::new_static("สตรีมมิงไปยังอุปกรณ์ในเครือข่าย"),
    cast_enabled_desc: SharedString::new_static(
        "แสดงอุปกรณ์ AirPlay, Chromecast และ DLNA ในรายการอุปกรณ์เสียง",
    ),
    device_volume: SharedString::new_static("ปรับระดับเสียงของอุปกรณ์"),
    device_volume_desc: SharedString::new_static(
        "ระหว่างสตรีมมิง แถบเลื่อนระดับเสียงจะปรับระดับเสียงที่ตัวอุปกรณ์ หากปิด อุปกรณ์จะใช้ระดับเสียงของตัวเอง: AirPlay จะได้รับเสียงที่ปรับตามระดับเสียงของเครื่องเล่นแล้ว ส่วน Chromecast และ DLNA จะได้รับไฟล์ตามเดิมและแถบเลื่อนจะถูกล็อก",
    ),
};

static CS: CastStrings = CastStrings {
    streaming: SharedString::new_static("Streamování"),
    searching: SharedString::new_static("Hledání zařízení…"),
    none_found: SharedString::new_static("Žádná zařízení nenalezena"),
    connecting: SharedString::new_static("Připojování…"),
    playing_on_t: SharedString::new_static("Hraje na {}"),
    connect_failed_t: SharedString::new_static("Nelze se připojit k {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Spojení s {} bylo ztraceno. Přehrávání se vrátilo na tento počítač.",
    ),
    cast_enabled: SharedString::new_static("Streamování do síťových zařízení"),
    cast_enabled_desc: SharedString::new_static(
        "Zobrazuje zařízení AirPlay, Chromecast a DLNA v seznamu zvukových zařízení.",
    ),
    device_volume: SharedString::new_static("Měnit hlasitost zařízení"),
    device_volume_desc: SharedString::new_static(
        "Při streamování mění posuvník hlasitosti hlasitost samotného zařízení. Když je vypnuto, zařízení si ponechá vlastní hlasitost: AirPlay dostává zvuk už s hlasitostí přehrávače, Chromecast a DLNA dostávají soubor beze změn a posuvník je zamčený.",
    ),
};

static SV: CastStrings = CastStrings {
    streaming: SharedString::new_static("Strömning"),
    searching: SharedString::new_static("Söker efter enheter…"),
    none_found: SharedString::new_static("Inga enheter hittades"),
    connecting: SharedString::new_static("Ansluter…"),
    playing_on_t: SharedString::new_static("Spelar på {}"),
    connect_failed_t: SharedString::new_static("Kunde inte ansluta till {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Anslutningen till {} bröts. Uppspelningen fortsätter på den här datorn.",
    ),
    cast_enabled: SharedString::new_static("Strömning till nätverksenheter"),
    cast_enabled_desc: SharedString::new_static(
        "Visar AirPlay-, Chromecast- och DLNA-enheter i listan över ljudenheter.",
    ),
    device_volume: SharedString::new_static("Ändra enhetens volym"),
    device_volume_desc: SharedString::new_static(
        "Under strömning ändrar volymreglaget volymen på själva enheten. Av: enheten behåller sin egen volym; AirPlay får ljudet redan i spelarens volym, Chromecast och DLNA får filen som den är och reglaget är låst.",
    ),
};

static HI: CastStrings = CastStrings {
    streaming: SharedString::new_static("स्ट्रीमिंग"),
    searching: SharedString::new_static("डिवाइस खोजे जा रहे हैं…"),
    none_found: SharedString::new_static("कोई डिवाइस नहीं मिला"),
    connecting: SharedString::new_static("कनेक्ट हो रहा है…"),
    playing_on_t: SharedString::new_static("{} पर चल रहा है"),
    connect_failed_t: SharedString::new_static("{} से कनेक्ट नहीं हो सका: {}"),
    connection_lost_t: SharedString::new_static("{} से कनेक्शन टूट गया। प्लेबैक इस कंप्यूटर पर लौट आया।"),
    cast_enabled: SharedString::new_static("नेटवर्क डिवाइस पर स्ट्रीमिंग"),
    cast_enabled_desc: SharedString::new_static(
        "ऑडियो डिवाइस की सूची में AirPlay, Chromecast और DLNA डिवाइस दिखाएँ।",
    ),
    device_volume: SharedString::new_static("डिवाइस का वॉल्यूम बदलें"),
    device_volume_desc: SharedString::new_static(
        "स्ट्रीमिंग के दौरान वॉल्यूम स्लाइडर डिवाइस का अपना वॉल्यूम बदलता है। बंद होने पर डिवाइस अपना वॉल्यूम रखता है: AirPlay को प्लेयर के वॉल्यूम वाली आवाज़ मिलती है, Chromecast और DLNA को फ़ाइल जैसी है वैसी मिलती है और स्लाइडर लॉक रहता है।",
    ),
};

static ES: CastStrings = CastStrings {
    streaming: SharedString::new_static("Transmisión"),
    searching: SharedString::new_static("Buscando dispositivos…"),
    none_found: SharedString::new_static("No se encontraron dispositivos"),
    connecting: SharedString::new_static("Conectando…"),
    playing_on_t: SharedString::new_static("Reproduciendo en {}"),
    connect_failed_t: SharedString::new_static("No se pudo conectar a {}: {}"),
    connection_lost_t: SharedString::new_static(
        "Se perdió la conexión con {}. La reproducción volvió a este equipo.",
    ),
    cast_enabled: SharedString::new_static("Transmitir a dispositivos de la red"),
    cast_enabled_desc: SharedString::new_static(
        "Muestra los dispositivos AirPlay, Chromecast y DLNA en la lista de dispositivos de audio.",
    ),
    device_volume: SharedString::new_static("Cambiar el volumen del dispositivo"),
    device_volume_desc: SharedString::new_static(
        "Durante la transmisión, el control de volumen cambia el volumen del propio dispositivo. Desactivado, el dispositivo mantiene su propio volumen: AirPlay recibe el sonido ya con el volumen del reproductor, y Chromecast y DLNA reciben el archivo tal cual, con el control bloqueado.",
    ),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_keeps_its_placeholders() {
        for lang in Lang::all() {
            let s = for_lang(*lang);
            assert_eq!(s.playing_on_t.matches("{}").count(), 1, "{lang:?}");
            assert_eq!(s.connect_failed_t.matches("{}").count(), 2, "{lang:?}");
            assert_eq!(s.connection_lost_t.matches("{}").count(), 1, "{lang:?}");
        }
    }
}
