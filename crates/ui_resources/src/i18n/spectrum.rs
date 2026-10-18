use gpui::SharedString;

use super::{Lang, active};

pub struct SpectrumStrings {
    pub title: SharedString,
    pub about: SharedString,
    pub show: SharedString,
    pub hide: SharedString,
    pub unavailable: SharedString,
    pub style: SharedString,
    pub style_desc: SharedString,
    pub style_cover: SharedString,
    pub style_silhouette: SharedString,
    pub style_zones: SharedString,
    pub style_analyzer: SharedString,
    pub tilt: SharedString,
    pub tilt_desc: SharedString,
    pub tilt_off: SharedString,
    pub smoothing: SharedString,
    pub smoothing_desc: SharedString,
    pub smoothing_auto: SharedString,
    pub frame_rate: SharedString,
    pub frame_rate_desc: SharedString,
}

pub fn spectrum_strings() -> &'static SpectrumStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static SpectrumStrings {
    match lang {
        Lang::En => &EN,
        Lang::Es => &ES,
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
    }
}

static EN: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Spectrum analyzer"),
    about: SharedString::new_static(
        "The spectrum is drawn in Cover view. The button next to Full screen shows and hides it.",
    ),
    show: SharedString::new_static("Show spectrum"),
    hide: SharedString::new_static("Hide spectrum"),
    unavailable: SharedString::new_static("The spectrum isn't available while casting to a device"),
    style: SharedString::new_static("Style"),
    style_desc: SharedString::new_static(
        "On the cover: a curve along the bottom edge of the cover. Silhouette: a curve under the cover. Zones: frequency zones and a scale. Analyzer: the full grid with zone levels.",
    ),
    style_cover: SharedString::new_static("On the cover"),
    style_silhouette: SharedString::new_static("Silhouette"),
    style_zones: SharedString::new_static("Zones"),
    style_analyzer: SharedString::new_static("Analyzer"),
    tilt: SharedString::new_static("Tilt"),
    tilt_desc: SharedString::new_static(
        "Lifts the highs around 1 kHz by this many dB per octave, so a balanced mix looks level. Mixing analyzers use 4.5; Off shows the spectrum as it is.",
    ),
    tilt_off: SharedString::new_static("Off"),
    smoothing: SharedString::new_static("Smoothing"),
    smoothing_desc: SharedString::new_static(
        "How wide a slice of the spectrum each point averages, in octaves. Auto uses 1/3 for On the cover and Silhouette and 1/6 for Zones and Analyzer.",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Frame rate"),
    frame_rate_desc: SharedString::new_static(
        "60 frames per second move more smoothly; 30 halve the load on the processor and the battery.",
    ),
};

static RU: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Анализатор звука"),
    about: SharedString::new_static(
        "Спектр рисуется в режиме обложки. Кнопка рядом с кнопкой «Полный экран» показывает и прячет его.",
    ),
    show: SharedString::new_static("Показать спектр"),
    hide: SharedString::new_static("Скрыть спектр"),
    unavailable: SharedString::new_static("Спектр недоступен во время трансляции на устройство"),
    style: SharedString::new_static("Стиль"),
    style_desc: SharedString::new_static(
        "На обложке — кривая по нижнему краю обложки. Силуэт — кривая под обложкой. Зоны — частотные зоны и шкала. Анализатор — полная сетка с уровнями зон.",
    ),
    style_cover: SharedString::new_static("На обложке"),
    style_silhouette: SharedString::new_static("Силуэт"),
    style_zones: SharedString::new_static("Зоны"),
    style_analyzer: SharedString::new_static("Анализатор"),
    tilt: SharedString::new_static("Наклон"),
    tilt_desc: SharedString::new_static(
        "Поднимает верха вокруг 1 кГц на столько дБ на октаву, чтобы сбалансированный микс выглядел ровным. В анализаторах для сведения ставят 4.5; «Выкл» показывает спектр как есть.",
    ),
    tilt_off: SharedString::new_static("Выкл"),
    smoothing: SharedString::new_static("Сглаживание"),
    smoothing_desc: SharedString::new_static(
        "Какую долю октавы усредняет каждая точка спектра. «Авто» — 1/3 для стилей «На обложке» и «Силуэт», 1/6 для «Зон» и «Анализатора».",
    ),
    smoothing_auto: SharedString::new_static("Авто"),
    frame_rate: SharedString::new_static("Частота кадров"),
    frame_rate_desc: SharedString::new_static(
        "60 кадров в секунду — плавнее; 30 — вдвое меньше нагрузка на процессор и батарею.",
    ),
};

static UK: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Аналізатор звуку"),
    about: SharedString::new_static(
        "Спектр малюється в режимі обкладинки. Кнопка поруч із кнопкою «Повний екран» показує й ховає його.",
    ),
    show: SharedString::new_static("Показати спектр"),
    hide: SharedString::new_static("Сховати спектр"),
    unavailable: SharedString::new_static("Спектр недоступний під час трансляції на пристрій"),
    style: SharedString::new_static("Стиль"),
    style_desc: SharedString::new_static(
        "На обкладинці — крива вздовж нижнього краю обкладинки. Силует — крива під обкладинкою. Зони — частотні зони та шкала. Аналізатор — повна сітка з рівнями зон.",
    ),
    style_cover: SharedString::new_static("На обкладинці"),
    style_silhouette: SharedString::new_static("Силует"),
    style_zones: SharedString::new_static("Зони"),
    style_analyzer: SharedString::new_static("Аналізатор"),
    tilt: SharedString::new_static("Нахил"),
    tilt_desc: SharedString::new_static(
        "Піднімає верхні частоти навколо 1 кГц на стільки дБ на октаву, щоб збалансований мікс виглядав рівним. В аналізаторах для зведення ставлять 4.5; «Вимк.» показує спектр як є.",
    ),
    tilt_off: SharedString::new_static("Вимк."),
    smoothing: SharedString::new_static("Згладжування"),
    smoothing_desc: SharedString::new_static(
        "Яку частку октави усереднює кожна точка спектра. «Авто» — 1/3 для стилів «На обкладинці» та «Силует», 1/6 для «Зон» і «Аналізатора».",
    ),
    smoothing_auto: SharedString::new_static("Авто"),
    frame_rate: SharedString::new_static("Частота кадрів"),
    frame_rate_desc: SharedString::new_static(
        "60 кадрів на секунду — плавніше; 30 — удвічі менше навантаження на процесор і батарею.",
    ),
};

static DE: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Spektrumanalysator"),
    about: SharedString::new_static(
        "Das Spektrum erscheint in der Cover-Ansicht. Die Taste neben „Vollbild“ blendet es ein und aus.",
    ),
    show: SharedString::new_static("Spektrum einblenden"),
    hide: SharedString::new_static("Spektrum ausblenden"),
    unavailable: SharedString::new_static(
        "Das Spektrum ist beim Übertragen auf ein Gerät nicht verfügbar",
    ),
    style: SharedString::new_static("Stil"),
    style_desc: SharedString::new_static(
        "Auf dem Cover: eine Kurve am unteren Rand des Covers. Silhouette: eine Kurve unter dem Cover. Zonen: Frequenzzonen und eine Skala. Analysator: das volle Raster mit den Pegeln der Zonen.",
    ),
    style_cover: SharedString::new_static("Auf dem Cover"),
    style_silhouette: SharedString::new_static("Silhouette"),
    style_zones: SharedString::new_static("Zonen"),
    style_analyzer: SharedString::new_static("Analysator"),
    tilt: SharedString::new_static("Neigung"),
    tilt_desc: SharedString::new_static(
        "Hebt die Höhen um 1 kHz um so viele dB pro Oktave an, dass ein ausgewogener Mix gerade aussieht. Analysatoren fürs Mischen nutzen 4.5; „Aus“ zeigt das Spektrum unverändert.",
    ),
    tilt_off: SharedString::new_static("Aus"),
    smoothing: SharedString::new_static("Glättung"),
    smoothing_desc: SharedString::new_static(
        "Über welchen Ausschnitt des Spektrums jeder Punkt mittelt, in Oktaven. „Auto“ nimmt 1/3 für „Auf dem Cover“ und „Silhouette“ und 1/6 für „Zonen“ und „Analysator“.",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Bildrate"),
    frame_rate_desc: SharedString::new_static(
        "60 Bilder pro Sekunde laufen flüssiger; 30 halbieren die Last für Prozessor und Akku.",
    ),
};

static FR: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Analyseur de spectre"),
    about: SharedString::new_static(
        "Le spectre s'affiche dans la vue pochette. Le bouton à côté de « Plein écran » l'affiche et le masque.",
    ),
    show: SharedString::new_static("Afficher le spectre"),
    hide: SharedString::new_static("Masquer le spectre"),
    unavailable: SharedString::new_static(
        "Le spectre n'est pas disponible pendant la diffusion vers un appareil",
    ),
    style: SharedString::new_static("Style"),
    style_desc: SharedString::new_static(
        "Sur la pochette : une courbe le long du bord inférieur de la pochette. Silhouette : une courbe sous la pochette. Zones : les zones de fréquences et une échelle. Analyseur : la grille complète avec le niveau des zones.",
    ),
    style_cover: SharedString::new_static("Sur la pochette"),
    style_silhouette: SharedString::new_static("Silhouette"),
    style_zones: SharedString::new_static("Zones"),
    style_analyzer: SharedString::new_static("Analyseur"),
    tilt: SharedString::new_static("Inclinaison"),
    tilt_desc: SharedString::new_static(
        "Relève les aigus autour de 1 kHz d'autant de dB par octave, pour qu'un mix équilibré paraisse plat. Les analyseurs de mixage utilisent 4.5 ; « Aucune » montre le spectre tel quel.",
    ),
    tilt_off: SharedString::new_static("Aucune"),
    smoothing: SharedString::new_static("Lissage"),
    smoothing_desc: SharedString::new_static(
        "Largeur de la portion du spectre que chaque point moyenne, en octaves. « Auto » utilise 1/3 pour « Sur la pochette » et « Silhouette », et 1/6 pour « Zones » et « Analyseur ».",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Fréquence d'images"),
    frame_rate_desc: SharedString::new_static(
        "60 images par seconde sont plus fluides ; 30 divisent par deux la charge du processeur et de la batterie.",
    ),
};

static ES: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Analizador de espectro"),
    about: SharedString::new_static(
        "El espectro se dibuja en la vista de portada. El botón junto a «Pantalla completa» lo muestra y lo oculta.",
    ),
    show: SharedString::new_static("Mostrar espectro"),
    hide: SharedString::new_static("Ocultar espectro"),
    unavailable: SharedString::new_static(
        "El espectro no está disponible mientras se transmite a un dispositivo",
    ),
    style: SharedString::new_static("Estilo"),
    style_desc: SharedString::new_static(
        "Sobre la portada: una curva a lo largo del borde inferior de la portada. Silueta: una curva bajo la portada. Zonas: zonas de frecuencia y una escala. Analizador: la cuadrícula completa con el nivel de cada zona.",
    ),
    style_cover: SharedString::new_static("Sobre la portada"),
    style_silhouette: SharedString::new_static("Silueta"),
    style_zones: SharedString::new_static("Zonas"),
    style_analyzer: SharedString::new_static("Analizador"),
    tilt: SharedString::new_static("Inclinación"),
    tilt_desc: SharedString::new_static(
        "Realza los agudos alrededor de 1 kHz tantos dB por octava para que una mezcla equilibrada se vea plana. Los analizadores de mezcla usan 4.5; «No» muestra el espectro tal cual.",
    ),
    tilt_off: SharedString::new_static("No"),
    smoothing: SharedString::new_static("Suavizado"),
    smoothing_desc: SharedString::new_static(
        "Qué parte del espectro promedia cada punto, en octavas. «Auto» usa 1/3 para «Sobre la portada» y «Silueta», y 1/6 para «Zonas» y «Analizador».",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Fotogramas por segundo"),
    frame_rate_desc: SharedString::new_static(
        "60 fotogramas por segundo se ven más fluidos; 30 reducen a la mitad la carga del procesador y de la batería.",
    ),
};

static PT: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Analisador de espectro"),
    about: SharedString::new_static(
        "O espectro aparece na visualização da capa. O botão ao lado de \"Tela cheia\" mostra e oculta o espectro.",
    ),
    show: SharedString::new_static("Mostrar espectro"),
    hide: SharedString::new_static("Ocultar espectro"),
    unavailable: SharedString::new_static(
        "O espectro não fica disponível durante a transmissão para um dispositivo",
    ),
    style: SharedString::new_static("Estilo"),
    style_desc: SharedString::new_static(
        "Na capa: uma curva ao longo da borda inferior da capa. Silhueta: uma curva abaixo da capa. Zonas: zonas de frequência e uma escala. Analisador: a grade completa com o nível de cada zona.",
    ),
    style_cover: SharedString::new_static("Na capa"),
    style_silhouette: SharedString::new_static("Silhueta"),
    style_zones: SharedString::new_static("Zonas"),
    style_analyzer: SharedString::new_static("Analisador"),
    tilt: SharedString::new_static("Inclinação"),
    tilt_desc: SharedString::new_static(
        "Realça os agudos em torno de 1 kHz em tantos dB por oitava, para que uma mixagem equilibrada pareça plana. Analisadores de mixagem usam 4.5; \"Não\" mostra o espectro como ele é.",
    ),
    tilt_off: SharedString::new_static("Não"),
    smoothing: SharedString::new_static("Suavização"),
    smoothing_desc: SharedString::new_static(
        "Que parte do espectro cada ponto calcula a média, em oitavas. \"Auto\" usa 1/3 para \"Na capa\" e \"Silhueta\" e 1/6 para \"Zonas\" e \"Analisador\".",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Taxa de quadros"),
    frame_rate_desc: SharedString::new_static(
        "60 quadros por segundo são mais fluidos; 30 reduzem pela metade a carga no processador e na bateria.",
    ),
};

static IT: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Analizzatore di spettro"),
    about: SharedString::new_static(
        "Lo spettro viene disegnato nella vista copertina. Il pulsante accanto a «Schermo intero» lo mostra e lo nasconde.",
    ),
    show: SharedString::new_static("Mostra lo spettro"),
    hide: SharedString::new_static("Nascondi lo spettro"),
    unavailable: SharedString::new_static(
        "Lo spettro non è disponibile durante la trasmissione a un dispositivo",
    ),
    style: SharedString::new_static("Stile"),
    style_desc: SharedString::new_static(
        "Sulla copertina: una curva lungo il bordo inferiore della copertina. Silhouette: una curva sotto la copertina. Zone: le zone di frequenza e una scala. Analizzatore: la griglia completa con il livello delle zone.",
    ),
    style_cover: SharedString::new_static("Sulla copertina"),
    style_silhouette: SharedString::new_static("Silhouette"),
    style_zones: SharedString::new_static("Zone"),
    style_analyzer: SharedString::new_static("Analizzatore"),
    tilt: SharedString::new_static("Inclinazione"),
    tilt_desc: SharedString::new_static(
        "Alza gli acuti intorno a 1 kHz di tanti dB per ottava, così un mix equilibrato appare piatto. Gli analizzatori da mixaggio usano 4.5; «No» mostra lo spettro così com'è.",
    ),
    tilt_off: SharedString::new_static("No"),
    smoothing: SharedString::new_static("Smussatura"),
    smoothing_desc: SharedString::new_static(
        "Quanta parte dello spettro media ogni punto, in ottave. «Auto» usa 1/3 per «Sulla copertina» e «Silhouette» e 1/6 per «Zone» e «Analizzatore».",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Frequenza dei fotogrammi"),
    frame_rate_desc: SharedString::new_static(
        "60 fotogrammi al secondo sono più fluidi; 30 dimezzano il carico su processore e batteria.",
    ),
};

static NL: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Spectrumanalyse"),
    about: SharedString::new_static(
        "Het spectrum verschijnt in de hoesweergave. De knop naast ‘Volledig scherm’ toont en verbergt het.",
    ),
    show: SharedString::new_static("Spectrum tonen"),
    hide: SharedString::new_static("Spectrum verbergen"),
    unavailable: SharedString::new_static(
        "Het spectrum is niet beschikbaar tijdens casten naar een apparaat",
    ),
    style: SharedString::new_static("Stijl"),
    style_desc: SharedString::new_static(
        "Op de hoes: een curve langs de onderrand van de hoes. Silhouet: een curve onder de hoes. Zones: frequentiezones en een schaal. Analyse: het volledige raster met het niveau per zone.",
    ),
    style_cover: SharedString::new_static("Op de hoes"),
    style_silhouette: SharedString::new_static("Silhouet"),
    style_zones: SharedString::new_static("Zones"),
    style_analyzer: SharedString::new_static("Analyse"),
    tilt: SharedString::new_static("Helling"),
    tilt_desc: SharedString::new_static(
        "Tilt de hoge tonen rond 1 kHz zoveel dB per octaaf op dat een gebalanceerde mix vlak oogt. Analysers voor mixen gebruiken 4.5; ‘Uit’ toont het spectrum zoals het is.",
    ),
    tilt_off: SharedString::new_static("Uit"),
    smoothing: SharedString::new_static("Afvlakking"),
    smoothing_desc: SharedString::new_static(
        "Hoe breed het stuk spectrum is waarover elk punt middelt, in octaven. ‘Auto’ gebruikt 1/3 voor ‘Op de hoes’ en ‘Silhouet’ en 1/6 voor ‘Zones’ en ‘Analyse’.",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Beeldsnelheid"),
    frame_rate_desc: SharedString::new_static(
        "60 beelden per seconde lopen vloeiender; 30 halveren de belasting van processor en batterij.",
    ),
};

static SV: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Spektrumanalys"),
    about: SharedString::new_static(
        "Spektrumet visas i omslagsvyn. Knappen bredvid ”Helskärm” visar och döljer det.",
    ),
    show: SharedString::new_static("Visa spektrum"),
    hide: SharedString::new_static("Dölj spektrum"),
    unavailable: SharedString::new_static(
        "Spektrumet är inte tillgängligt när du castar till en enhet",
    ),
    style: SharedString::new_static("Stil"),
    style_desc: SharedString::new_static(
        "På omslaget: en kurva längs omslagets nederkant. Siluett: en kurva under omslaget. Zoner: frekvenszoner och en skala. Analys: hela rutnätet med nivån för varje zon.",
    ),
    style_cover: SharedString::new_static("På omslaget"),
    style_silhouette: SharedString::new_static("Siluett"),
    style_zones: SharedString::new_static("Zoner"),
    style_analyzer: SharedString::new_static("Analys"),
    tilt: SharedString::new_static("Lutning"),
    tilt_desc: SharedString::new_static(
        "Lyfter diskanten runt 1 kHz med så många dB per oktav att en balanserad mix ser jämn ut. Analysatorer för mixning använder 4.5; ”Av” visar spektrumet som det är.",
    ),
    tilt_off: SharedString::new_static("Av"),
    smoothing: SharedString::new_static("Utjämning"),
    smoothing_desc: SharedString::new_static(
        "Hur bred del av spektrumet varje punkt tar medelvärdet av, i oktaver. ”Auto” använder 1/3 för ”På omslaget” och ”Siluett” och 1/6 för ”Zoner” och ”Analys”.",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Bildfrekvens"),
    frame_rate_desc: SharedString::new_static(
        "60 bilder per sekund är mjukare; 30 halverar belastningen på processorn och batteriet.",
    ),
};

static PL: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Analizator widma"),
    about: SharedString::new_static(
        "Widmo jest rysowane w widoku okładki. Przycisk obok „Pełny ekran” pokazuje je i ukrywa.",
    ),
    show: SharedString::new_static("Pokaż widmo"),
    hide: SharedString::new_static("Ukryj widmo"),
    unavailable: SharedString::new_static(
        "Widmo jest niedostępne podczas przesyłania na urządzenie",
    ),
    style: SharedString::new_static("Styl"),
    style_desc: SharedString::new_static(
        "Na okładce: krzywa wzdłuż dolnej krawędzi okładki. Sylwetka: krzywa pod okładką. Strefy: strefy częstotliwości i skala. Analizator: pełna siatka z poziomem każdej strefy.",
    ),
    style_cover: SharedString::new_static("Na okładce"),
    style_silhouette: SharedString::new_static("Sylwetka"),
    style_zones: SharedString::new_static("Strefy"),
    style_analyzer: SharedString::new_static("Analizator"),
    tilt: SharedString::new_static("Nachylenie"),
    tilt_desc: SharedString::new_static(
        "Podnosi wysokie tony wokół 1 kHz o tyle dB na oktawę, by zrównoważony miks wyglądał płasko. Analizatory do miksu używają 4.5; „Wył.” pokazuje widmo bez zmian.",
    ),
    tilt_off: SharedString::new_static("Wył."),
    smoothing: SharedString::new_static("Wygładzanie"),
    smoothing_desc: SharedString::new_static(
        "Jak szeroki fragment widma uśrednia każdy punkt, w oktawach. „Auto” używa 1/3 dla stylów „Na okładce” i „Sylwetka” oraz 1/6 dla „Stref” i „Analizatora”.",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Liczba klatek"),
    frame_rate_desc: SharedString::new_static(
        "60 klatek na sekundę jest płynniejsze; 30 o połowę zmniejsza obciążenie procesora i baterii.",
    ),
};

static CS: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Analyzátor spektra"),
    about: SharedString::new_static(
        "Spektrum se kreslí v zobrazení obalu. Tlačítko vedle „Celá obrazovka“ ho zobrazí a skryje.",
    ),
    show: SharedString::new_static("Zobrazit spektrum"),
    hide: SharedString::new_static("Skrýt spektrum"),
    unavailable: SharedString::new_static("Spektrum není dostupné při přenosu do zařízení"),
    style: SharedString::new_static("Styl"),
    style_desc: SharedString::new_static(
        "Na obalu: křivka podél spodního okraje obalu. Silueta: křivka pod obalem. Zóny: frekvenční zóny a stupnice. Analyzátor: celá mřížka s úrovní každé zóny.",
    ),
    style_cover: SharedString::new_static("Na obalu"),
    style_silhouette: SharedString::new_static("Silueta"),
    style_zones: SharedString::new_static("Zóny"),
    style_analyzer: SharedString::new_static("Analyzátor"),
    tilt: SharedString::new_static("Sklon"),
    tilt_desc: SharedString::new_static(
        "Zvedne výšky kolem 1 kHz o tolik dB na oktávu, aby vyvážený mix vypadal rovně. Analyzátory pro mix používají 4.5; „Vyp.“ ukáže spektrum tak, jak je.",
    ),
    tilt_off: SharedString::new_static("Vyp."),
    smoothing: SharedString::new_static("Vyhlazení"),
    smoothing_desc: SharedString::new_static(
        "Jak širokou část spektra každý bod průměruje, v oktávách. „Auto“ použije 1/3 pro „Na obalu“ a „Silueta“ a 1/6 pro „Zóny“ a „Analyzátor“.",
    ),
    smoothing_auto: SharedString::new_static("Auto"),
    frame_rate: SharedString::new_static("Snímková frekvence"),
    frame_rate_desc: SharedString::new_static(
        "60 snímků za sekundu je plynulejších; 30 sníží zátěž procesoru a baterie na polovinu.",
    ),
};

static TR: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Spektrum analizörü"),
    about: SharedString::new_static(
        "Spektrum kapak görünümünde çizilir. \"Tam ekran\" düğmesinin yanındaki düğme onu gösterir ve gizler.",
    ),
    show: SharedString::new_static("Spektrumu göster"),
    hide: SharedString::new_static("Spektrumu gizle"),
    unavailable: SharedString::new_static("Bir cihaza yayın yapılırken spektrum kullanılamaz"),
    style: SharedString::new_static("Stil"),
    style_desc: SharedString::new_static(
        "Kapakta: kapağın alt kenarı boyunca bir eğri. Siluet: kapağın altında bir eğri. Bölgeler: frekans bölgeleri ve bir ölçek. Analizör: bölge seviyeleriyle birlikte tam ızgara.",
    ),
    style_cover: SharedString::new_static("Kapakta"),
    style_silhouette: SharedString::new_static("Siluet"),
    style_zones: SharedString::new_static("Bölgeler"),
    style_analyzer: SharedString::new_static("Analizör"),
    tilt: SharedString::new_static("Eğim"),
    tilt_desc: SharedString::new_static(
        "Dengeli bir miks düz görünsün diye 1 kHz çevresinde tizleri oktav başına bu kadar dB yükseltir. Miks analizörleri 4.5 kullanır; \"Kapalı\" spektrumu olduğu gibi gösterir.",
    ),
    tilt_off: SharedString::new_static("Kapalı"),
    smoothing: SharedString::new_static("Yumuşatma"),
    smoothing_desc: SharedString::new_static(
        "Her noktanın spektrumun ne kadar geniş bir bölümünün ortalamasını aldığı, oktav cinsinden. \"Otomatik\", \"Kapakta\" ve \"Siluet\" için 1/3, \"Bölgeler\" ve \"Analizör\" için 1/6 kullanır.",
    ),
    smoothing_auto: SharedString::new_static("Otomatik"),
    frame_rate: SharedString::new_static("Kare hızı"),
    frame_rate_desc: SharedString::new_static(
        "Saniyede 60 kare daha akıcıdır; 30 kare işlemci ve pil yükünü yarıya indirir.",
    ),
};

static ZH: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("频谱分析"),
    about: SharedString::new_static("频谱显示在封面视图中。“全屏”旁边的按钮用于显示和隐藏频谱。"),
    show: SharedString::new_static("显示频谱"),
    hide: SharedString::new_static("隐藏频谱"),
    unavailable: SharedString::new_static("投放到设备时无法显示频谱"),
    style: SharedString::new_static("样式"),
    style_desc: SharedString::new_static(
        "封面上：沿封面底边的一条曲线。剪影：封面下方的一条曲线。分区：频段分区和刻度。分析仪：带各分区电平的完整网格。",
    ),
    style_cover: SharedString::new_static("封面上"),
    style_silhouette: SharedString::new_static("剪影"),
    style_zones: SharedString::new_static("分区"),
    style_analyzer: SharedString::new_static("分析仪"),
    tilt: SharedString::new_static("倾斜"),
    tilt_desc: SharedString::new_static(
        "以 1 kHz 为中心，每倍频程把高频抬高这么多 dB，让均衡的混音看起来是平的。混音用的分析仪采用 4.5；“关”按原样显示频谱。",
    ),
    tilt_off: SharedString::new_static("关"),
    smoothing: SharedString::new_static("平滑"),
    smoothing_desc: SharedString::new_static(
        "每个点对多宽的一段频谱取平均，以倍频程计。“自动”对“封面上”和“剪影”用 1/3，对“分区”和“分析仪”用 1/6。",
    ),
    smoothing_auto: SharedString::new_static("自动"),
    frame_rate: SharedString::new_static("帧率"),
    frame_rate_desc: SharedString::new_static(
        "每秒 60 帧更流畅；30 帧可将处理器和电池的负担减半。",
    ),
};

static JA: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("スペクトラムアナライザー"),
    about: SharedString::new_static(
        "スペクトラムはカバー表示に描かれます。「全画面表示」の隣のボタンで表示と非表示を切り替えます。",
    ),
    show: SharedString::new_static("スペクトラムを表示"),
    hide: SharedString::new_static("スペクトラムを隠す"),
    unavailable: SharedString::new_static("デバイスへのキャスト中はスペクトラムを表示できません"),
    style: SharedString::new_static("スタイル"),
    style_desc: SharedString::new_static(
        "カバー上：カバーの下端に沿った曲線。シルエット：カバーの下の曲線。ゾーン：周波数ゾーンと目盛り。アナライザー：ゾーンごとのレベル付きの完全なグリッド。",
    ),
    style_cover: SharedString::new_static("カバー上"),
    style_silhouette: SharedString::new_static("シルエット"),
    style_zones: SharedString::new_static("ゾーン"),
    style_analyzer: SharedString::new_static("アナライザー"),
    tilt: SharedString::new_static("傾き"),
    tilt_desc: SharedString::new_static(
        "1 kHz を中心に、高域を 1 オクターブあたりこの dB だけ持ち上げ、バランスの取れたミックスが平らに見えるようにします。ミックス用のアナライザーは 4.5 を使います。「オフ」はスペクトラムをそのまま表示します。",
    ),
    tilt_off: SharedString::new_static("オフ"),
    smoothing: SharedString::new_static("スムージング"),
    smoothing_desc: SharedString::new_static(
        "各点がスペクトラムのどれだけの幅を平均するか（オクターブ単位）。「自動」は「カバー上」と「シルエット」で 1/3、「ゾーン」と「アナライザー」で 1/6 を使います。",
    ),
    smoothing_auto: SharedString::new_static("自動"),
    frame_rate: SharedString::new_static("フレームレート"),
    frame_rate_desc: SharedString::new_static(
        "毎秒 60 フレームはより滑らかです。30 フレームではプロセッサとバッテリーへの負荷が半分になります。",
    ),
};

static KO: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("스펙트럼 분석기"),
    about: SharedString::new_static(
        "스펙트럼은 커버 보기에 그려집니다. '전체 화면' 옆의 버튼으로 보이거나 숨깁니다.",
    ),
    show: SharedString::new_static("스펙트럼 보기"),
    hide: SharedString::new_static("스펙트럼 숨기기"),
    unavailable: SharedString::new_static("기기로 캐스팅하는 동안에는 스펙트럼을 볼 수 없습니다"),
    style: SharedString::new_static("스타일"),
    style_desc: SharedString::new_static(
        "커버 위: 커버 아래쪽 가장자리를 따라가는 곡선. 실루엣: 커버 아래의 곡선. 구역: 주파수 구역과 눈금. 분석기: 구역별 레벨이 표시되는 전체 격자.",
    ),
    style_cover: SharedString::new_static("커버 위"),
    style_silhouette: SharedString::new_static("실루엣"),
    style_zones: SharedString::new_static("구역"),
    style_analyzer: SharedString::new_static("분석기"),
    tilt: SharedString::new_static("기울기"),
    tilt_desc: SharedString::new_static(
        "1 kHz를 중심으로 옥타브당 이만큼 dB씩 고음을 올려, 균형 잡힌 믹스가 평평하게 보이도록 합니다. 믹싱용 분석기는 4.5를 씁니다. '끔'은 스펙트럼을 그대로 보여 줍니다.",
    ),
    tilt_off: SharedString::new_static("끔"),
    smoothing: SharedString::new_static("스무딩"),
    smoothing_desc: SharedString::new_static(
        "각 점이 스펙트럼을 얼마나 넓게 평균하는지(옥타브 단위). '자동'은 '커버 위'와 '실루엣'에 1/3, '구역'과 '분석기'에 1/6을 씁니다.",
    ),
    smoothing_auto: SharedString::new_static("자동"),
    frame_rate: SharedString::new_static("프레임 속도"),
    frame_rate_desc: SharedString::new_static(
        "초당 60프레임은 더 부드럽고, 30프레임은 프로세서와 배터리 부담을 절반으로 줄입니다.",
    ),
};

static VI: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Phân tích phổ"),
    about: SharedString::new_static(
        "Phổ được vẽ trong chế độ xem bìa. Nút cạnh nút \"Toàn màn hình\" dùng để hiện và ẩn phổ.",
    ),
    show: SharedString::new_static("Hiện phổ"),
    hide: SharedString::new_static("Ẩn phổ"),
    unavailable: SharedString::new_static("Không thể xem phổ khi đang truyền tới thiết bị"),
    style: SharedString::new_static("Kiểu"),
    style_desc: SharedString::new_static(
        "Trên bìa: một đường cong dọc mép dưới của bìa. Bóng: một đường cong dưới bìa. Vùng: các vùng tần số và một thang đo. Bộ phân tích: lưới đầy đủ kèm mức của từng vùng.",
    ),
    style_cover: SharedString::new_static("Trên bìa"),
    style_silhouette: SharedString::new_static("Bóng"),
    style_zones: SharedString::new_static("Vùng"),
    style_analyzer: SharedString::new_static("Bộ phân tích"),
    tilt: SharedString::new_static("Độ nghiêng"),
    tilt_desc: SharedString::new_static(
        "Nâng dải cao quanh 1 kHz thêm chừng ấy dB mỗi quãng tám để một bản mix cân bằng trông phẳng. Các bộ phân tích dùng khi mix chọn 4.5; \"Tắt\" hiện phổ đúng như nó là.",
    ),
    tilt_off: SharedString::new_static("Tắt"),
    smoothing: SharedString::new_static("Làm mượt"),
    smoothing_desc: SharedString::new_static(
        "Mỗi điểm lấy trung bình trên một đoạn phổ rộng bao nhiêu, tính bằng quãng tám. \"Tự động\" dùng 1/3 cho \"Trên bìa\" và \"Bóng\", 1/6 cho \"Vùng\" và \"Bộ phân tích\".",
    ),
    smoothing_auto: SharedString::new_static("Tự động"),
    frame_rate: SharedString::new_static("Tốc độ khung hình"),
    frame_rate_desc: SharedString::new_static(
        "60 khung hình mỗi giây mượt hơn; 30 giảm một nửa tải cho bộ xử lý và pin.",
    ),
};

static ID: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("Penganalisis spektrum"),
    about: SharedString::new_static(
        "Spektrum digambar di tampilan sampul. Tombol di samping \"Layar penuh\" menampilkan dan menyembunyikannya.",
    ),
    show: SharedString::new_static("Tampilkan spektrum"),
    hide: SharedString::new_static("Sembunyikan spektrum"),
    unavailable: SharedString::new_static("Spektrum tidak tersedia selama transmisi ke perangkat"),
    style: SharedString::new_static("Gaya"),
    style_desc: SharedString::new_static(
        "Di sampul: kurva di sepanjang tepi bawah sampul. Siluet: kurva di bawah sampul. Zona: zona frekuensi dan skala. Penganalisis: kisi lengkap dengan level setiap zona.",
    ),
    style_cover: SharedString::new_static("Di sampul"),
    style_silhouette: SharedString::new_static("Siluet"),
    style_zones: SharedString::new_static("Zona"),
    style_analyzer: SharedString::new_static("Penganalisis"),
    tilt: SharedString::new_static("Kemiringan"),
    tilt_desc: SharedString::new_static(
        "Menaikkan frekuensi tinggi di sekitar 1 kHz sebanyak ini dB per oktaf agar mix yang seimbang tampak datar. Penganalisis untuk mixing memakai 4.5; \"Mati\" menampilkan spektrum apa adanya.",
    ),
    tilt_off: SharedString::new_static("Mati"),
    smoothing: SharedString::new_static("Penghalusan"),
    smoothing_desc: SharedString::new_static(
        "Seberapa lebar bagian spektrum yang dirata-rata oleh setiap titik, dalam oktaf. \"Otomatis\" memakai 1/3 untuk \"Di sampul\" dan \"Siluet\" serta 1/6 untuk \"Zona\" dan \"Penganalisis\".",
    ),
    smoothing_auto: SharedString::new_static("Otomatis"),
    frame_rate: SharedString::new_static("Laju bingkai"),
    frame_rate_desc: SharedString::new_static(
        "60 bingkai per detik lebih mulus; 30 memangkas beban prosesor dan baterai hingga separuh.",
    ),
};

static TH: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("ตัววิเคราะห์สเปกตรัม"),
    about: SharedString::new_static("สเปกตรัมแสดงในมุมมองปก ปุ่มข้าง \"เต็มหน้าจอ\" ใช้แสดงและซ่อนสเปกตรัม"),
    show: SharedString::new_static("แสดงสเปกตรัม"),
    hide: SharedString::new_static("ซ่อนสเปกตรัม"),
    unavailable: SharedString::new_static("ไม่สามารถแสดงสเปกตรัมขณะส่งไปยังอุปกรณ์"),
    style: SharedString::new_static("รูปแบบ"),
    style_desc: SharedString::new_static(
        "บนปก: เส้นโค้งตามขอบล่างของปก เงา: เส้นโค้งใต้ปก โซน: โซนความถี่และสเกล ตัววิเคราะห์: ตารางเต็มพร้อมระดับของแต่ละโซน",
    ),
    style_cover: SharedString::new_static("บนปก"),
    style_silhouette: SharedString::new_static("เงา"),
    style_zones: SharedString::new_static("โซน"),
    style_analyzer: SharedString::new_static("ตัววิเคราะห์"),
    tilt: SharedString::new_static("ความเอียง"),
    tilt_desc: SharedString::new_static(
        "ยกเสียงแหลมรอบ 1 kHz ขึ้นตามจำนวน dB ต่ออ็อกเทฟนี้ เพื่อให้มิกซ์ที่สมดุลดูเรียบ ตัววิเคราะห์สำหรับมิกซ์ใช้ 4.5 ส่วน \"ปิด\" แสดงสเปกตรัมตามจริง",
    ),
    tilt_off: SharedString::new_static("ปิด"),
    smoothing: SharedString::new_static("การปรับเรียบ"),
    smoothing_desc: SharedString::new_static(
        "แต่ละจุดเฉลี่ยสเปกตรัมกว้างเท่าใด หน่วยเป็นอ็อกเทฟ \"อัตโนมัติ\" ใช้ 1/3 สำหรับ \"บนปก\" และ \"เงา\" และ 1/6 สำหรับ \"โซน\" และ \"ตัววิเคราะห์\"",
    ),
    smoothing_auto: SharedString::new_static("อัตโนมัติ"),
    frame_rate: SharedString::new_static("อัตราเฟรม"),
    frame_rate_desc: SharedString::new_static(
        "60 เฟรมต่อวินาทีลื่นไหลกว่า ส่วน 30 เฟรมลดภาระของโปรเซสเซอร์และแบตเตอรี่ลงครึ่งหนึ่ง",
    ),
};

static HI: SpectrumStrings = SpectrumStrings {
    title: SharedString::new_static("स्पेक्ट्रम विश्लेषक"),
    about: SharedString::new_static(
        "स्पेक्ट्रम कवर दृश्य में बनता है। \"पूर्ण स्क्रीन\" के बगल वाला बटन इसे दिखाता और छिपाता है।",
    ),
    show: SharedString::new_static("स्पेक्ट्रम दिखाएँ"),
    hide: SharedString::new_static("स्पेक्ट्रम छिपाएँ"),
    unavailable: SharedString::new_static("किसी डिवाइस पर कास्ट करते समय स्पेक्ट्रम उपलब्ध नहीं है"),
    style: SharedString::new_static("शैली"),
    style_desc: SharedString::new_static(
        "कवर पर: कवर के निचले किनारे के साथ एक वक्र। छाया: कवर के नीचे एक वक्र। ज़ोन: आवृत्ति ज़ोन और एक पैमाना। विश्लेषक: हर ज़ोन के स्तर के साथ पूरा ग्रिड।",
    ),
    style_cover: SharedString::new_static("कवर पर"),
    style_silhouette: SharedString::new_static("छाया"),
    style_zones: SharedString::new_static("ज़ोन"),
    style_analyzer: SharedString::new_static("विश्लेषक"),
    tilt: SharedString::new_static("झुकाव"),
    tilt_desc: SharedString::new_static(
        "1 kHz के आसपास ऊँची आवृत्तियों को प्रति ऑक्टेव इतने dB उठाता है कि संतुलित मिक्स सपाट दिखे। मिक्सिंग वाले विश्लेषक 4.5 इस्तेमाल करते हैं; \"बंद\" स्पेक्ट्रम को जैसा है वैसा दिखाता है।",
    ),
    tilt_off: SharedString::new_static("बंद"),
    smoothing: SharedString::new_static("स्मूदिंग"),
    smoothing_desc: SharedString::new_static(
        "हर बिंदु स्पेक्ट्रम के कितने चौड़े हिस्से का औसत लेता है, ऑक्टेव में। \"स्वतः\" \"कवर पर\" और \"छाया\" के लिए 1/3 तथा \"ज़ोन\" और \"विश्लेषक\" के लिए 1/6 इस्तेमाल करता है।",
    ),
    smoothing_auto: SharedString::new_static("स्वतः"),
    frame_rate: SharedString::new_static("फ़्रेम दर"),
    frame_rate_desc: SharedString::new_static(
        "60 फ़्रेम प्रति सेकंड ज़्यादा सहज हैं; 30 प्रोसेसर और बैटरी पर भार आधा कर देते हैं।",
    ),
};
