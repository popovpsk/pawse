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
};

static ZH: CastStrings = CastStrings {
    streaming: SharedString::new_static("串流"),
    searching: SharedString::new_static("正在搜索设备…"),
    none_found: SharedString::new_static("未找到设备"),
    connecting: SharedString::new_static("正在连接…"),
    playing_on_t: SharedString::new_static("正在 {} 上播放"),
    connect_failed_t: SharedString::new_static("无法连接到 {}：{}"),
    connection_lost_t: SharedString::new_static("与 {} 的连接已断开，已改回在本机播放。"),
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
};

static HI: CastStrings = CastStrings {
    streaming: SharedString::new_static("स्ट्रीमिंग"),
    searching: SharedString::new_static("डिवाइस खोजे जा रहे हैं…"),
    none_found: SharedString::new_static("कोई डिवाइस नहीं मिला"),
    connecting: SharedString::new_static("कनेक्ट हो रहा है…"),
    playing_on_t: SharedString::new_static("{} पर चल रहा है"),
    connect_failed_t: SharedString::new_static("{} से कनेक्ट नहीं हो सका: {}"),
    connection_lost_t: SharedString::new_static("{} से कनेक्शन टूट गया। प्लेबैक इस कंप्यूटर पर लौट आया।"),
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
