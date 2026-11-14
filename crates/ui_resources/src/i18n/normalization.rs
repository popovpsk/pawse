use gpui::SharedString;

use super::{Lang, active};

pub struct NormalizationStrings {
    pub setting: SharedString,
    pub setting_desc: SharedString,
    pub bit_perfect: SharedString,
    pub bit_perfect_desc: SharedString,
}

pub fn normalization_strings() -> &'static NormalizationStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static NormalizationStrings {
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

static EN: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Volume normalization"),
    setting_desc: SharedString::new_static(
        "Plays every track at the same loudness, using ReplayGain tags or a measurement made on this computer. A track that hasn't been measured yet plays as is the first time, and on albums without gaps the volume can step between tracks. Not applied on Chromecast and DLNA devices.",
    ),
    bit_perfect: SharedString::new_static("Volume normalization is on"),
    bit_perfect_desc: SharedString::new_static(
        "Volume normalization changes the level of the track before it reaches the device. Turn it off in Settings → General to hear the file unchanged.",
    ),
};

static ZH: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("音量均衡"),
    setting_desc: SharedString::new_static(
        "根据 ReplayGain 标签或在这台电脑上的测量，让每首曲目以相同响度播放。尚未测量的曲目第一次按原样播放；在无间隔的专辑中，曲目之间的音量可能会有变化。不适用于 Chromecast 和 DLNA 设备。",
    ),
    bit_perfect: SharedString::new_static("音量均衡已开启"),
    bit_perfect_desc: SharedString::new_static(
        "音量均衡会在曲目到达设备之前改变它的电平。在“设置 → 通用”中关闭它，即可听到未经改变的文件。",
    ),
};

static PT: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Normalização de volume"),
    setting_desc: SharedString::new_static(
        "Toca todas as faixas no mesmo volume, usando tags ReplayGain ou uma medição feita neste computador. Uma faixa ainda não medida toca como está na primeira vez, e em álbuns sem pausas o volume pode mudar entre as faixas. Não é aplicada em dispositivos Chromecast e DLNA.",
    ),
    bit_perfect: SharedString::new_static("A normalização de volume está ligada"),
    bit_perfect_desc: SharedString::new_static(
        "A normalização de volume altera o nível da faixa antes que ela chegue ao dispositivo. Desligue-a em Configurações → Geral para ouvir o arquivo sem alterações.",
    ),
};

static RU: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Выравнивание громкости"),
    setting_desc: SharedString::new_static(
        "Все треки звучат одинаково громко: по тегам ReplayGain или по замеру на этом компьютере. Ещё не измеренный трек в первый раз играет как есть, а на альбомах без пауз громкость может меняться на стыке треков. На Chromecast и DLNA не применяется.",
    ),
    bit_perfect: SharedString::new_static("Выравнивание громкости включено"),
    bit_perfect_desc: SharedString::new_static(
        "Выравнивание громкости меняет уровень трека до того, как он попадает на устройство. Выключите его в разделе Настройки → Общие, чтобы слушать файл без изменений.",
    ),
};

static JA: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("音量の均一化"),
    setting_desc: SharedString::new_static(
        "ReplayGain タグまたはこのコンピューターでの測定に基づき、すべてのトラックを同じ音量で再生します。まだ測定されていないトラックは初回はそのまま再生され、曲間のないアルバムではトラックの切り替わりで音量が変わることがあります。Chromecast と DLNA デバイスには適用されません。",
    ),
    bit_perfect: SharedString::new_static("音量の均一化がオンです"),
    bit_perfect_desc: SharedString::new_static(
        "音量の均一化は、トラックがデバイスに届く前にレベルを変えます。ファイルをそのまま聴くには、設定 → 一般でオフにしてください。",
    ),
};

static DE: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Lautstärkeangleichung"),
    setting_desc: SharedString::new_static(
        "Spielt jeden Titel gleich laut ab, anhand von ReplayGain-Tags oder einer Messung auf diesem Computer. Ein noch nicht gemessener Titel läuft beim ersten Mal unverändert, und bei Alben ohne Pausen kann die Lautstärke zwischen den Titeln springen. Gilt nicht für Chromecast- und DLNA-Geräte.",
    ),
    bit_perfect: SharedString::new_static("Die Lautstärkeangleichung ist an"),
    bit_perfect_desc: SharedString::new_static(
        "Die Lautstärkeangleichung ändert den Pegel des Titels, bevor er beim Gerät ankommt. Schalte sie unter Einstellungen → Allgemein aus, um die Datei unverändert zu hören.",
    ),
};

static FR: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Normalisation du volume"),
    setting_desc: SharedString::new_static(
        "Lit chaque morceau au même volume, d'après les tags ReplayGain ou une mesure faite sur cet ordinateur. Un morceau pas encore mesuré est lu tel quel la première fois, et sur les albums sans blancs le volume peut changer entre les morceaux. Non appliquée sur les appareils Chromecast et DLNA.",
    ),
    bit_perfect: SharedString::new_static("La normalisation du volume est activée"),
    bit_perfect_desc: SharedString::new_static(
        "La normalisation du volume modifie le niveau du morceau avant qu'il n'atteigne le périphérique. Désactivez-la dans Réglages → Général pour écouter le fichier sans modification.",
    ),
};

static KO: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("음량 평준화"),
    setting_desc: SharedString::new_static(
        "ReplayGain 태그나 이 컴퓨터에서 측정한 값을 바탕으로 모든 트랙을 같은 크기로 재생합니다. 아직 측정되지 않은 트랙은 처음에는 그대로 재생되고, 곡 사이에 쉼이 없는 앨범에서는 트랙이 바뀔 때 음량이 달라질 수 있습니다. Chromecast와 DLNA 기기에는 적용되지 않습니다.",
    ),
    bit_perfect: SharedString::new_static("음량 평준화가 켜져 있습니다"),
    bit_perfect_desc: SharedString::new_static(
        "음량 평준화는 트랙이 장치에 닿기 전에 레벨을 바꿉니다. 파일을 그대로 들으려면 설정 → 일반에서 끄세요.",
    ),
};

static IT: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Normalizzazione del volume"),
    setting_desc: SharedString::new_static(
        "Riproduce ogni brano allo stesso volume, in base ai tag ReplayGain o a una misurazione fatta su questo computer. Un brano non ancora misurato viene riprodotto così com'è la prima volta, e negli album senza pause il volume può cambiare tra un brano e l'altro. Non si applica ai dispositivi Chromecast e DLNA.",
    ),
    bit_perfect: SharedString::new_static("La normalizzazione del volume è attiva"),
    bit_perfect_desc: SharedString::new_static(
        "La normalizzazione del volume cambia il livello del brano prima che arrivi al dispositivo. Disattivala in Impostazioni → Generale per ascoltare il file senza modifiche.",
    ),
};

static TR: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Ses düzeyi eşitleme"),
    setting_desc: SharedString::new_static(
        "Her parçayı ReplayGain etiketlerine ya da bu bilgisayarda yapılan bir ölçüme göre aynı yükseklikte çalar. Henüz ölçülmemiş bir parça ilk seferde olduğu gibi çalar; aralıksız albümlerde ses düzeyi parçalar arasında değişebilir. Chromecast ve DLNA cihazlarında uygulanmaz.",
    ),
    bit_perfect: SharedString::new_static("Ses düzeyi eşitleme açık"),
    bit_perfect_desc: SharedString::new_static(
        "Ses düzeyi eşitleme, parçanın düzeyini aygıta ulaşmadan önce değiştirir. Dosyayı değiştirilmeden dinlemek için Ayarlar → Genel'den kapatın.",
    ),
};

static PL: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Normalizacja głośności"),
    setting_desc: SharedString::new_static(
        "Odtwarza każdy utwór z tą samą głośnością, na podstawie tagów ReplayGain lub pomiaru na tym komputerze. Utwór jeszcze niezmierzony gra za pierwszym razem bez zmian, a na albumach bez przerw głośność może się zmieniać między utworami. Nie działa na urządzeniach Chromecast i DLNA.",
    ),
    bit_perfect: SharedString::new_static("Normalizacja głośności jest włączona"),
    bit_perfect_desc: SharedString::new_static(
        "Normalizacja głośności zmienia poziom utworu, zanim dotrze on do urządzenia. Wyłącz ją w Ustawienia → Ogólne, aby słuchać pliku bez zmian.",
    ),
};

static NL: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Volumenormalisatie"),
    setting_desc: SharedString::new_static(
        "Speelt elk nummer even luid af, op basis van ReplayGain-tags of een meting op deze computer. Een nog niet gemeten nummer speelt de eerste keer ongewijzigd, en op albums zonder pauzes kan het volume tussen nummers verspringen. Niet op Chromecast- en DLNA-apparaten.",
    ),
    bit_perfect: SharedString::new_static("Volumenormalisatie staat aan"),
    bit_perfect_desc: SharedString::new_static(
        "Volumenormalisatie verandert het niveau van het nummer voordat het het apparaat bereikt. Zet het uit via Instellingen → Algemeen om het bestand ongewijzigd te horen.",
    ),
};

static UK: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Вирівнювання гучності"),
    setting_desc: SharedString::new_static(
        "Усі треки звучать однаково гучно: за тегами ReplayGain або за вимірюванням на цьому комп'ютері. Ще не виміряний трек уперше грає як є, а на альбомах без пауз гучність може змінюватися на стику треків. На Chromecast і DLNA не застосовується.",
    ),
    bit_perfect: SharedString::new_static("Вирівнювання гучності увімкнено"),
    bit_perfect_desc: SharedString::new_static(
        "Вирівнювання гучності змінює рівень треку, перш ніж той потрапить на пристрій. Вимкніть його в розділі Налаштування → Загальні, щоб слухати файл без змін.",
    ),
};

static VI: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Chuẩn hóa âm lượng"),
    setting_desc: SharedString::new_static(
        "Phát mọi bản nhạc với cùng độ lớn, dựa trên thẻ ReplayGain hoặc phép đo trên máy tính này. Bản nhạc chưa được đo sẽ phát nguyên bản ở lần đầu, và với album không có khoảng lặng, âm lượng có thể thay đổi giữa các bản. Không áp dụng cho thiết bị Chromecast và DLNA.",
    ),
    bit_perfect: SharedString::new_static("Chuẩn hóa âm lượng đang bật"),
    bit_perfect_desc: SharedString::new_static(
        "Chuẩn hóa âm lượng thay đổi mức của bản nhạc trước khi nó đến thiết bị. Hãy tắt nó trong Cài đặt → Chung để nghe tệp mà không bị thay đổi.",
    ),
};

static ID: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Normalisasi volume"),
    setting_desc: SharedString::new_static(
        "Memutar setiap lagu dengan kenyaringan yang sama, berdasarkan tag ReplayGain atau pengukuran di komputer ini. Lagu yang belum diukur diputar apa adanya pada kali pertama, dan pada album tanpa jeda volume bisa berubah di antara lagu. Tidak berlaku di perangkat Chromecast dan DLNA.",
    ),
    bit_perfect: SharedString::new_static("Normalisasi volume aktif"),
    bit_perfect_desc: SharedString::new_static(
        "Normalisasi volume mengubah level lagu sebelum sampai ke perangkat. Matikan di Pengaturan → Umum untuk mendengar file tanpa perubahan.",
    ),
};

static TH: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("ปรับระดับเสียงให้เท่ากัน"),
    setting_desc: SharedString::new_static(
        "เล่นทุกแทร็กด้วยความดังเท่ากัน โดยอิงจากแท็ก ReplayGain หรือการวัดบนคอมพิวเตอร์เครื่องนี้ แทร็กที่ยังไม่ได้วัดจะเล่นตามเดิมในครั้งแรก และในอัลบั้มที่ไม่มีช่วงเงียบ ระดับเสียงอาจเปลี่ยนระหว่างแทร็ก ไม่ใช้กับอุปกรณ์ Chromecast และ DLNA",
    ),
    bit_perfect: SharedString::new_static("การปรับระดับเสียงให้เท่ากันเปิดอยู่"),
    bit_perfect_desc: SharedString::new_static(
        "การปรับระดับเสียงให้เท่ากันจะเปลี่ยนระดับของแทร็กก่อนที่จะไปถึงอุปกรณ์ ปิดได้ที่ การตั้งค่า → ทั่วไป เพื่อฟังไฟล์โดยไม่เปลี่ยนแปลง",
    ),
};

static CS: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Vyrovnání hlasitosti"),
    setting_desc: SharedString::new_static(
        "Přehrává každou skladbu stejně hlasitě podle tagů ReplayGain nebo měření na tomto počítači. Ještě nezměřená skladba hraje poprvé beze změny a u alb bez pauz se hlasitost může mezi skladbami změnit. Na zařízeních Chromecast a DLNA se nepoužívá.",
    ),
    bit_perfect: SharedString::new_static("Vyrovnání hlasitosti je zapnuté"),
    bit_perfect_desc: SharedString::new_static(
        "Vyrovnání hlasitosti mění úroveň skladby dřív, než dorazí do zařízení. Vypněte ho v Nastavení → Obecné, pokud chcete slyšet soubor beze změn.",
    ),
};

static SV: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Volymutjämning"),
    setting_desc: SharedString::new_static(
        "Spelar varje spår lika högt, utifrån ReplayGain-taggar eller en mätning på den här datorn. Ett spår som inte mätts än spelas oförändrat första gången, och på album utan pauser kan volymen ändras mellan spåren. Används inte på Chromecast- och DLNA-enheter.",
    ),
    bit_perfect: SharedString::new_static("Volymutjämning är på"),
    bit_perfect_desc: SharedString::new_static(
        "Volymutjämning ändrar spårets nivå innan det når enheten. Stäng av den under Inställningar → Allmänt för att höra filen oförändrad.",
    ),
};

static HI: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("वॉल्यूम सामान्यीकरण"),
    setting_desc: SharedString::new_static(
        "ReplayGain टैग या इस कंप्यूटर पर किए गए माप के आधार पर हर ट्रैक को एक जैसी आवाज़ में बजाता है। जो ट्रैक अभी मापा नहीं गया, वह पहली बार जैसा है वैसा बजता है, और बिना अंतराल वाले एल्बमों में ट्रैक बदलते समय वॉल्यूम बदल सकता है। Chromecast और DLNA डिवाइस पर लागू नहीं होता।",
    ),
    bit_perfect: SharedString::new_static("वॉल्यूम सामान्यीकरण चालू है"),
    bit_perfect_desc: SharedString::new_static(
        "वॉल्यूम सामान्यीकरण ट्रैक के डिवाइस तक पहुँचने से पहले उसका स्तर बदल देता है। फ़ाइल को बिना बदलाव के सुनने के लिए इसे सेटिंग्स → सामान्य में बंद करें।",
    ),
};

static ES: NormalizationStrings = NormalizationStrings {
    setting: SharedString::new_static("Normalización del volumen"),
    setting_desc: SharedString::new_static(
        "Reproduce cada pista al mismo volumen, según las etiquetas ReplayGain o una medición hecha en este ordenador. Una pista que aún no se ha medido suena tal cual la primera vez, y en álbumes sin pausas el volumen puede cambiar entre pistas. No se aplica en dispositivos Chromecast y DLNA.",
    ),
    bit_perfect: SharedString::new_static("La normalización del volumen está activada"),
    bit_perfect_desc: SharedString::new_static(
        "La normalización del volumen cambia el nivel de la pista antes de que llegue al dispositivo. Desactívala en Ajustes → General para oír el archivo sin cambios.",
    ),
};
