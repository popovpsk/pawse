use gpui::SharedString;

use super::{Lang, active};

pub struct SimilarStrings {
    pub menu: SharedString,
    pub radio: SharedString,
    pub radio_hint: SharedString,
    pub mix: SharedString,
    pub mix_hint: SharedString,
    pub tracks: SharedString,
    pub familiar: SharedString,
    pub any: SharedString,
    pub unheard: SharedString,
    pub not_analysed: SharedString,
    pub queue_not_analysed: SharedString,
    pub nothing_found: SharedString,
    pub failed: SharedString,
}

pub fn similar_strings() -> &'static SimilarStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static SimilarStrings {
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

static EN: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Similar music"),
    radio: SharedString::new_static("Radio from this track"),
    radio_hint: SharedString::new_static("Replaces the rest of the queue"),
    mix: SharedString::new_static("Mix in other artists"),
    mix_hint: SharedString::new_static("1–3 similar tracks after each one"),
    tracks: SharedString::new_static("Tracks"),
    familiar: SharedString::new_static("Familiar"),
    any: SharedString::new_static("Any"),
    unheard: SharedString::new_static("New"),
    not_analysed: SharedString::new_static(
        "This track hasn't been analysed yet. The analysis runs in the background.",
    ),
    queue_not_analysed: SharedString::new_static("No track in the queue has been analysed yet."),
    nothing_found: SharedString::new_static("No similar tracks found."),
    failed: SharedString::new_static("Couldn't find similar tracks."),
};

static ZH: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("相似音乐"),
    radio: SharedString::new_static("以此曲开启电台"),
    radio_hint: SharedString::new_static("替换队列中的其余歌曲"),
    mix: SharedString::new_static("混入其他艺人"),
    mix_hint: SharedString::new_static("每首歌后加入 1–3 首相似歌曲"),
    tracks: SharedString::new_static("歌曲"),
    familiar: SharedString::new_static("熟悉的"),
    any: SharedString::new_static("任意"),
    unheard: SharedString::new_static("新的"),
    not_analysed: SharedString::new_static("这首歌尚未分析，分析正在后台进行。"),
    queue_not_analysed: SharedString::new_static("队列中还没有已分析的歌曲。"),
    nothing_found: SharedString::new_static("没有找到相似的歌曲。"),
    failed: SharedString::new_static("无法找到相似的歌曲。"),
};

static PT: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Música parecida"),
    radio: SharedString::new_static("Rádio a partir desta faixa"),
    radio_hint: SharedString::new_static("Substitui o resto da fila"),
    mix: SharedString::new_static("Misturar outros artistas"),
    mix_hint: SharedString::new_static("1–3 faixas parecidas depois de cada uma"),
    tracks: SharedString::new_static("Faixas"),
    familiar: SharedString::new_static("Conhecidas"),
    any: SharedString::new_static("Qualquer"),
    unheard: SharedString::new_static("Novas"),
    not_analysed: SharedString::new_static(
        "Esta faixa ainda não foi analisada. A análise roda em segundo plano.",
    ),
    queue_not_analysed: SharedString::new_static("Nenhuma faixa da fila foi analisada ainda."),
    nothing_found: SharedString::new_static("Nenhuma faixa parecida encontrada."),
    failed: SharedString::new_static("Não foi possível encontrar faixas parecidas."),
};

static RU: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Похожая музыка"),
    radio: SharedString::new_static("Радио от этого трека"),
    radio_hint: SharedString::new_static("Заменит остальную очередь"),
    mix: SharedString::new_static("Разбавить очередь"),
    mix_hint: SharedString::new_static("1–3 похожих трека других исполнителей после каждого"),
    tracks: SharedString::new_static("Треки"),
    familiar: SharedString::new_static("Знакомые"),
    any: SharedString::new_static("Любые"),
    unheard: SharedString::new_static("Новые"),
    not_analysed: SharedString::new_static("Этот трек ещё не проанализирован. Анализ идёт в фоне."),
    queue_not_analysed: SharedString::new_static("Ни один трек очереди ещё не проанализирован."),
    nothing_found: SharedString::new_static("Похожих треков не нашлось."),
    failed: SharedString::new_static("Не удалось подобрать похожие треки."),
};

static JA: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("似た音楽"),
    radio: SharedString::new_static("この曲からラジオ"),
    radio_hint: SharedString::new_static("キューの残りを置き換えます"),
    mix: SharedString::new_static("他のアーティストを混ぜる"),
    mix_hint: SharedString::new_static("各曲の後に似た曲を 1〜3 曲"),
    tracks: SharedString::new_static("曲"),
    familiar: SharedString::new_static("聴いたことのある曲"),
    any: SharedString::new_static("すべて"),
    unheard: SharedString::new_static("新しい曲"),
    not_analysed: SharedString::new_static(
        "この曲はまだ分析されていません。分析はバックグラウンドで行われます。",
    ),
    queue_not_analysed: SharedString::new_static("キューの曲はまだ分析されていません。"),
    nothing_found: SharedString::new_static("似た曲は見つかりませんでした。"),
    failed: SharedString::new_static("似た曲を見つけられませんでした。"),
};

static DE: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Ähnliche Musik"),
    radio: SharedString::new_static("Radio ab diesem Titel"),
    radio_hint: SharedString::new_static("Ersetzt den Rest der Warteschlange"),
    mix: SharedString::new_static("Andere Künstler einmischen"),
    mix_hint: SharedString::new_static("1–3 ähnliche Titel nach jedem"),
    tracks: SharedString::new_static("Titel"),
    familiar: SharedString::new_static("Bekannte"),
    any: SharedString::new_static("Alle"),
    unheard: SharedString::new_static("Neue"),
    not_analysed: SharedString::new_static(
        "Dieser Titel wurde noch nicht analysiert. Die Analyse läuft im Hintergrund.",
    ),
    queue_not_analysed: SharedString::new_static(
        "Noch kein Titel der Warteschlange wurde analysiert.",
    ),
    nothing_found: SharedString::new_static("Keine ähnlichen Titel gefunden."),
    failed: SharedString::new_static("Ähnliche Titel konnten nicht gefunden werden."),
};

static FR: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Musique similaire"),
    radio: SharedString::new_static("Radio à partir de ce titre"),
    radio_hint: SharedString::new_static("Remplace le reste de la file"),
    mix: SharedString::new_static("Mélanger d'autres artistes"),
    mix_hint: SharedString::new_static("1 à 3 titres similaires après chacun"),
    tracks: SharedString::new_static("Titres"),
    familiar: SharedString::new_static("Connus"),
    any: SharedString::new_static("Tous"),
    unheard: SharedString::new_static("Nouveaux"),
    not_analysed: SharedString::new_static(
        "Ce titre n'a pas encore été analysé. L'analyse tourne en arrière-plan.",
    ),
    queue_not_analysed: SharedString::new_static("Aucun titre de la file n'a encore été analysé."),
    nothing_found: SharedString::new_static("Aucun titre similaire trouvé."),
    failed: SharedString::new_static("Impossible de trouver des titres similaires."),
};

static KO: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("비슷한 음악"),
    radio: SharedString::new_static("이 곡으로 라디오"),
    radio_hint: SharedString::new_static("대기열의 나머지를 바꿉니다"),
    mix: SharedString::new_static("다른 아티스트 섞기"),
    mix_hint: SharedString::new_static("곡마다 비슷한 곡 1–3개"),
    tracks: SharedString::new_static("곡"),
    familiar: SharedString::new_static("익숙한 곡"),
    any: SharedString::new_static("모두"),
    unheard: SharedString::new_static("새로운 곡"),
    not_analysed: SharedString::new_static(
        "이 곡은 아직 분석되지 않았습니다. 분석은 백그라운드에서 진행됩니다.",
    ),
    queue_not_analysed: SharedString::new_static("대기열의 곡이 아직 분석되지 않았습니다."),
    nothing_found: SharedString::new_static("비슷한 곡을 찾지 못했습니다."),
    failed: SharedString::new_static("비슷한 곡을 찾을 수 없습니다."),
};

static IT: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Musica simile"),
    radio: SharedString::new_static("Radio da questo brano"),
    radio_hint: SharedString::new_static("Sostituisce il resto della coda"),
    mix: SharedString::new_static("Mescola altri artisti"),
    mix_hint: SharedString::new_static("1–3 brani simili dopo ciascuno"),
    tracks: SharedString::new_static("Brani"),
    familiar: SharedString::new_static("Conosciuti"),
    any: SharedString::new_static("Tutti"),
    unheard: SharedString::new_static("Nuovi"),
    not_analysed: SharedString::new_static(
        "Questo brano non è ancora stato analizzato. L'analisi è in corso in background.",
    ),
    queue_not_analysed: SharedString::new_static(
        "Nessun brano della coda è stato ancora analizzato.",
    ),
    nothing_found: SharedString::new_static("Nessun brano simile trovato."),
    failed: SharedString::new_static("Impossibile trovare brani simili."),
};

static TR: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Benzer müzik"),
    radio: SharedString::new_static("Bu parçadan radyo"),
    radio_hint: SharedString::new_static("Sıranın geri kalanını değiştirir"),
    mix: SharedString::new_static("Başka sanatçılar kat"),
    mix_hint: SharedString::new_static("Her birinin ardından 1–3 benzer parça"),
    tracks: SharedString::new_static("Parçalar"),
    familiar: SharedString::new_static("Tanıdık"),
    any: SharedString::new_static("Hepsi"),
    unheard: SharedString::new_static("Yeni"),
    not_analysed: SharedString::new_static(
        "Bu parça henüz analiz edilmedi. Analiz arka planda sürüyor.",
    ),
    queue_not_analysed: SharedString::new_static(
        "Sıradaki parçaların hiçbiri henüz analiz edilmedi.",
    ),
    nothing_found: SharedString::new_static("Benzer parça bulunamadı."),
    failed: SharedString::new_static("Benzer parçalar bulunamadı."),
};

static PL: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Podobna muzyka"),
    radio: SharedString::new_static("Radio od tego utworu"),
    radio_hint: SharedString::new_static("Zastępuje resztę kolejki"),
    mix: SharedString::new_static("Dodaj innych wykonawców"),
    mix_hint: SharedString::new_static("1–3 podobne utwory po każdym"),
    tracks: SharedString::new_static("Utwory"),
    familiar: SharedString::new_static("Znane"),
    any: SharedString::new_static("Dowolne"),
    unheard: SharedString::new_static("Nowe"),
    not_analysed: SharedString::new_static(
        "Ten utwór nie został jeszcze przeanalizowany. Analiza trwa w tle.",
    ),
    queue_not_analysed: SharedString::new_static(
        "Żaden utwór w kolejce nie został jeszcze przeanalizowany.",
    ),
    nothing_found: SharedString::new_static("Nie znaleziono podobnych utworów."),
    failed: SharedString::new_static("Nie udało się znaleźć podobnych utworów."),
};

static NL: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Vergelijkbare muziek"),
    radio: SharedString::new_static("Radio vanaf dit nummer"),
    radio_hint: SharedString::new_static("Vervangt de rest van de wachtrij"),
    mix: SharedString::new_static("Andere artiesten ertussen"),
    mix_hint: SharedString::new_static("1–3 vergelijkbare nummers na elk nummer"),
    tracks: SharedString::new_static("Nummers"),
    familiar: SharedString::new_static("Bekend"),
    any: SharedString::new_static("Alle"),
    unheard: SharedString::new_static("Nieuw"),
    not_analysed: SharedString::new_static(
        "Dit nummer is nog niet geanalyseerd. De analyse loopt op de achtergrond.",
    ),
    queue_not_analysed: SharedString::new_static("Nog geen nummer in de wachtrij is geanalyseerd."),
    nothing_found: SharedString::new_static("Geen vergelijkbare nummers gevonden."),
    failed: SharedString::new_static("Kon geen vergelijkbare nummers vinden."),
};

static UK: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Схожа музика"),
    radio: SharedString::new_static("Радіо від цього треку"),
    radio_hint: SharedString::new_static("Замінить решту черги"),
    mix: SharedString::new_static("Розбавити чергу"),
    mix_hint: SharedString::new_static("1–3 схожі треки інших виконавців після кожного"),
    tracks: SharedString::new_static("Треки"),
    familiar: SharedString::new_static("Знайомі"),
    any: SharedString::new_static("Будь-які"),
    unheard: SharedString::new_static("Нові"),
    not_analysed: SharedString::new_static("Цей трек ще не проаналізовано. Аналіз триває у фоні."),
    queue_not_analysed: SharedString::new_static("Жоден трек черги ще не проаналізовано."),
    nothing_found: SharedString::new_static("Схожих треків не знайшлося."),
    failed: SharedString::new_static("Не вдалося підібрати схожі треки."),
};

static VI: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Nhạc tương tự"),
    radio: SharedString::new_static("Radio từ bài này"),
    radio_hint: SharedString::new_static("Thay phần còn lại của hàng đợi"),
    mix: SharedString::new_static("Trộn nghệ sĩ khác"),
    mix_hint: SharedString::new_static("1–3 bài tương tự sau mỗi bài"),
    tracks: SharedString::new_static("Bài hát"),
    familiar: SharedString::new_static("Quen thuộc"),
    any: SharedString::new_static("Bất kỳ"),
    unheard: SharedString::new_static("Mới"),
    not_analysed: SharedString::new_static(
        "Bài này chưa được phân tích. Quá trình phân tích chạy ở chế độ nền.",
    ),
    queue_not_analysed: SharedString::new_static("Chưa có bài nào trong hàng đợi được phân tích."),
    nothing_found: SharedString::new_static("Không tìm thấy bài tương tự."),
    failed: SharedString::new_static("Không thể tìm bài tương tự."),
};

static ID: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Musik serupa"),
    radio: SharedString::new_static("Radio dari lagu ini"),
    radio_hint: SharedString::new_static("Mengganti sisa antrean"),
    mix: SharedString::new_static("Selipkan artis lain"),
    mix_hint: SharedString::new_static("1–3 lagu serupa setelah setiap lagu"),
    tracks: SharedString::new_static("Lagu"),
    familiar: SharedString::new_static("Familier"),
    any: SharedString::new_static("Semua"),
    unheard: SharedString::new_static("Baru"),
    not_analysed: SharedString::new_static(
        "Lagu ini belum dianalisis. Analisis berjalan di latar belakang.",
    ),
    queue_not_analysed: SharedString::new_static("Belum ada lagu di antrean yang dianalisis."),
    nothing_found: SharedString::new_static("Tidak ada lagu serupa."),
    failed: SharedString::new_static("Tidak dapat menemukan lagu serupa."),
};

static TH: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("เพลงที่คล้ายกัน"),
    radio: SharedString::new_static("วิทยุจากเพลงนี้"),
    radio_hint: SharedString::new_static("แทนที่ส่วนที่เหลือของคิว"),
    mix: SharedString::new_static("แทรกศิลปินอื่น"),
    mix_hint: SharedString::new_static("เพลงที่คล้ายกัน 1–3 เพลงหลังแต่ละเพลง"),
    tracks: SharedString::new_static("เพลง"),
    familiar: SharedString::new_static("ที่คุ้นเคย"),
    any: SharedString::new_static("ทั้งหมด"),
    unheard: SharedString::new_static("ใหม่"),
    not_analysed: SharedString::new_static("ยังไม่ได้วิเคราะห์เพลงนี้ การวิเคราะห์ทำงานอยู่เบื้องหลัง"),
    queue_not_analysed: SharedString::new_static("ยังไม่มีเพลงในคิวที่วิเคราะห์แล้ว"),
    nothing_found: SharedString::new_static("ไม่พบเพลงที่คล้ายกัน"),
    failed: SharedString::new_static("ไม่สามารถหาเพลงที่คล้ายกันได้"),
};

static CS: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Podobná hudba"),
    radio: SharedString::new_static("Rádio od této skladby"),
    radio_hint: SharedString::new_static("Nahradí zbytek fronty"),
    mix: SharedString::new_static("Přimíchat jiné interprety"),
    mix_hint: SharedString::new_static("1–3 podobné skladby po každé"),
    tracks: SharedString::new_static("Skladby"),
    familiar: SharedString::new_static("Známé"),
    any: SharedString::new_static("Libovolné"),
    unheard: SharedString::new_static("Nové"),
    not_analysed: SharedString::new_static(
        "Tato skladba ještě nebyla analyzována. Analýza běží na pozadí.",
    ),
    queue_not_analysed: SharedString::new_static(
        "Žádná skladba ve frontě ještě nebyla analyzována.",
    ),
    nothing_found: SharedString::new_static("Nenašly se žádné podobné skladby."),
    failed: SharedString::new_static("Podobné skladby se nepodařilo najít."),
};

static SV: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Liknande musik"),
    radio: SharedString::new_static("Radio från det här spåret"),
    radio_hint: SharedString::new_static("Ersätter resten av kön"),
    mix: SharedString::new_static("Blanda in andra artister"),
    mix_hint: SharedString::new_static("1–3 liknande spår efter varje"),
    tracks: SharedString::new_static("Spår"),
    familiar: SharedString::new_static("Bekanta"),
    any: SharedString::new_static("Alla"),
    unheard: SharedString::new_static("Nya"),
    not_analysed: SharedString::new_static(
        "Det här spåret har inte analyserats än. Analysen körs i bakgrunden.",
    ),
    queue_not_analysed: SharedString::new_static("Inget spår i kön har analyserats än."),
    nothing_found: SharedString::new_static("Inga liknande spår hittades."),
    failed: SharedString::new_static("Kunde inte hitta liknande spår."),
};

static HI: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("मिलता-जुलता संगीत"),
    radio: SharedString::new_static("इस ट्रैक से रेडियो"),
    radio_hint: SharedString::new_static("कतार का बाकी हिस्सा बदल देगा"),
    mix: SharedString::new_static("दूसरे कलाकार मिलाएँ"),
    mix_hint: SharedString::new_static("हर ट्रैक के बाद 1–3 मिलते-जुलते ट्रैक"),
    tracks: SharedString::new_static("ट्रैक"),
    familiar: SharedString::new_static("जाने-पहचाने"),
    any: SharedString::new_static("कोई भी"),
    unheard: SharedString::new_static("नए"),
    not_analysed: SharedString::new_static(
        "इस ट्रैक का अभी विश्लेषण नहीं हुआ है। विश्लेषण बैकग्राउंड में चल रहा है।",
    ),
    queue_not_analysed: SharedString::new_static("कतार के किसी भी ट्रैक का अभी विश्लेषण नहीं हुआ है।"),
    nothing_found: SharedString::new_static("कोई मिलता-जुलता ट्रैक नहीं मिला।"),
    failed: SharedString::new_static("मिलते-जुलते ट्रैक नहीं मिल सके।"),
};

static ES: SimilarStrings = SimilarStrings {
    menu: SharedString::new_static("Música parecida"),
    radio: SharedString::new_static("Radio a partir de esta pista"),
    radio_hint: SharedString::new_static("Sustituye el resto de la cola"),
    mix: SharedString::new_static("Mezclar otros artistas"),
    mix_hint: SharedString::new_static("1–3 pistas parecidas después de cada una"),
    tracks: SharedString::new_static("Pistas"),
    familiar: SharedString::new_static("Conocidas"),
    any: SharedString::new_static("Cualquiera"),
    unheard: SharedString::new_static("Nuevas"),
    not_analysed: SharedString::new_static(
        "Esta pista aún no se ha analizado. El análisis se ejecuta en segundo plano.",
    ),
    queue_not_analysed: SharedString::new_static("Ninguna pista de la cola se ha analizado aún."),
    nothing_found: SharedString::new_static("No se encontraron pistas parecidas."),
    failed: SharedString::new_static("No se pudieron encontrar pistas parecidas."),
};
