use gpui::SharedString;

use super::{Lang, active, fill};

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
    pub indexing: SharedString,
    pub analysed_t: SharedString,
    pub loading: SharedString,
    pub off: SharedString,
    pub idle: SharedString,
    pub loading_model: SharedString,
    pub analysing: SharedString,
    pub no_model: SharedString,
    pub stopped: SharedString,
    pub no_gpu_short: SharedString,
    pub no_gpu: SharedString,
    pub gpu_failed: SharedString,
    pub gpu_retry: SharedString,
    pub status_failed: SharedString,
    pub failed_t: SharedString,
    pub unreachable_t: SharedString,
    pub close: SharedString,
    pub settings_title: SharedString,
    pub autoplay: SharedString,
    pub autoplay_desc: SharedString,
    pub about: SharedString,
}

impl SimilarStrings {
    pub fn analysed(&self, done: usize, total: usize) -> String {
        fill(&self.analysed_t, &[&done.to_string(), &total.to_string()])
    }

    pub fn failed_count(&self, count: usize) -> String {
        fill(&self.failed_t, &[&count.to_string()])
    }

    pub fn unreachable(&self, count: usize) -> String {
        fill(&self.unreachable_t, &[&count.to_string()])
    }
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
    indexing: SharedString::new_static("Indexing"),
    analysed_t: SharedString::new_static("Tracks analysed: {} of {}"),
    loading: SharedString::new_static("Loading…"),
    off: SharedString::new_static("Sound analysis is off."),
    idle: SharedString::new_static(
        "Nothing to analyse right now. New tracks are analysed after a library scan.",
    ),
    loading_model: SharedString::new_static("Getting the model ready…"),
    analysing: SharedString::new_static("Analysing…"),
    no_model: SharedString::new_static("The model is unavailable. Trying again in 5 minutes."),
    stopped: SharedString::new_static(
        "The analysis stopped on an error (details in the log). It resumes at the next library update.",
    ),
    no_gpu_short: SharedString::new_static("No graphics card found"),
    no_gpu: SharedString::new_static(
        "This computer has no graphics card the analysis can use, so it is unavailable.",
    ),
    gpu_failed: SharedString::new_static(
        "The graphics card did not pass the check (details in the log). The analysis is unavailable until the app restarts.",
    ),
    gpu_retry: SharedString::new_static(
        "The graphics card did not pass the check (details in the log). Trying again in 30 seconds.",
    ),
    status_failed: SharedString::new_static("Couldn't read the progress."),
    failed_t: SharedString::new_static("Could not be analysed: {}"),
    unreachable_t: SharedString::new_static(
        "Tracks on unreachable servers: {}. They are tried again at the next library update.",
    ),
    close: SharedString::new_static("Close"),
    settings_title: SharedString::new_static("Recommendations"),
    autoplay: SharedString::new_static("Continue the queue when it ends"),
    autoplay_desc: SharedString::new_static(
        "When the last track ends, similar-sounding tracks from your library keep playing.",
    ),
    about: SharedString::new_static(
        "Each track is analysed to learn how it sounds; tracks from servers are downloaded for this. While the analysis runs, the app uses more memory and puts more load on the processor and graphics card.",
    ),
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
    indexing: SharedString::new_static("索引"),
    analysed_t: SharedString::new_static("已分析歌曲：{} / {}"),
    loading: SharedString::new_static("加载中…"),
    off: SharedString::new_static("声音分析已关闭。"),
    idle: SharedString::new_static("目前没有需要分析的内容。新歌曲会在扫描音乐库后分析。"),
    loading_model: SharedString::new_static("正在准备模型…"),
    analysing: SharedString::new_static("正在分析…"),
    no_model: SharedString::new_static("模型不可用，5 分钟后重试。"),
    stopped: SharedString::new_static("分析因错误而停止（详情见日志）。将在下次更新音乐库时继续。"),
    no_gpu_short: SharedString::new_static("未找到显卡"),
    no_gpu: SharedString::new_static("这台电脑没有可用于分析的显卡，因此无法分析。"),
    gpu_failed: SharedString::new_static(
        "显卡未通过检查（详情见日志）。重新启动应用之前无法分析。",
    ),
    gpu_retry: SharedString::new_static("显卡未通过检查（详情见日志）。30 秒后重试。"),
    status_failed: SharedString::new_static("无法读取进度。"),
    failed_t: SharedString::new_static("无法分析：{}"),
    unreachable_t: SharedString::new_static(
        "无法连接的服务器上的歌曲：{}。会在下次更新音乐库时重试。",
    ),
    close: SharedString::new_static("关闭"),
    settings_title: SharedString::new_static("推荐"),
    autoplay: SharedString::new_static("播放队列结束后继续播放"),
    autoplay_desc: SharedString::new_static("最后一首播完后，继续播放媒体库中听起来相似的曲目。"),
    about: SharedString::new_static(
        "每首歌曲都会被分析其声音特征，服务器上的歌曲需要为此下载。分析期间，应用会占用更多内存，处理器和显卡的负载也会更高。",
    ),
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
    indexing: SharedString::new_static("Indexação"),
    analysed_t: SharedString::new_static("Faixas analisadas: {} de {}"),
    loading: SharedString::new_static("Carregando…"),
    off: SharedString::new_static("A análise de som está desativada."),
    idle: SharedString::new_static(
        "Nada para analisar agora. Faixas novas são analisadas depois de escanear a biblioteca.",
    ),
    loading_model: SharedString::new_static("Preparando o modelo…"),
    analysing: SharedString::new_static("Analisando…"),
    no_model: SharedString::new_static(
        "O modelo não está disponível. Nova tentativa em 5 minutos.",
    ),
    stopped: SharedString::new_static(
        "A análise parou por um erro (detalhes no log). Ela continua na próxima atualização da biblioteca.",
    ),
    no_gpu_short: SharedString::new_static("Nenhuma placa de vídeo encontrada"),
    no_gpu: SharedString::new_static(
        "Este computador não tem uma placa de vídeo que a análise possa usar, por isso ela não está disponível.",
    ),
    gpu_failed: SharedString::new_static(
        "A placa de vídeo não passou na verificação (detalhes no log). A análise fica indisponível até o app ser reiniciado.",
    ),
    gpu_retry: SharedString::new_static(
        "A placa de vídeo não passou na verificação (detalhes no log). Nova tentativa em 30 segundos.",
    ),
    status_failed: SharedString::new_static("Não foi possível ler o progresso."),
    failed_t: SharedString::new_static("Não foi possível analisar: {}"),
    unreachable_t: SharedString::new_static(
        "Faixas em servidores inacessíveis: {}. Serão tentadas de novo na próxima atualização da biblioteca.",
    ),
    close: SharedString::new_static("Fechar"),
    settings_title: SharedString::new_static("Recomendações"),
    autoplay: SharedString::new_static("Continuar a fila quando ela terminar"),
    autoplay_desc: SharedString::new_static(
        "Quando a última faixa terminar, faixas de som parecido da sua biblioteca continuam tocando.",
    ),
    about: SharedString::new_static(
        "Cada faixa é analisada para saber como soa; as faixas dos servidores são baixadas para isso. Durante a análise, o app usa mais memória e exige mais do processador e da placa de vídeo.",
    ),
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
    indexing: SharedString::new_static("Индексация"),
    analysed_t: SharedString::new_static("Проанализировано треков: {} из {}"),
    loading: SharedString::new_static("Загрузка…"),
    off: SharedString::new_static("Анализ звучания выключен."),
    idle: SharedString::new_static(
        "Сейчас анализировать нечего. Новые треки проанализируются после сканирования медиатеки.",
    ),
    loading_model: SharedString::new_static("Подготовка модели…"),
    analysing: SharedString::new_static("Идёт анализ…"),
    no_model: SharedString::new_static("Модель недоступна. Повторная попытка через 5 минут."),
    stopped: SharedString::new_static(
        "Анализ остановился из-за ошибки (подробности в журнале). Он продолжится при следующем обновлении медиатеки.",
    ),
    no_gpu_short: SharedString::new_static("Видеокарта не найдена"),
    no_gpu: SharedString::new_static(
        "На этом компьютере нет видеокарты, которую может использовать анализ, поэтому он недоступен.",
    ),
    gpu_failed: SharedString::new_static(
        "Видеокарта не прошла проверку (подробности в журнале). Анализ недоступен до перезапуска приложения.",
    ),
    gpu_retry: SharedString::new_static(
        "Видеокарта не прошла проверку (подробности в журнале). Повторная попытка через 30 секунд.",
    ),
    status_failed: SharedString::new_static("Не удалось прочитать прогресс."),
    failed_t: SharedString::new_static("Не удалось проанализировать: {}"),
    unreachable_t: SharedString::new_static(
        "Треки на недоступных серверах: {}. Повторная попытка — при следующем обновлении медиатеки.",
    ),
    close: SharedString::new_static("Закрыть"),
    settings_title: SharedString::new_static("Рекомендации"),
    autoplay: SharedString::new_static("Продолжать очередь, когда она закончится"),
    autoplay_desc: SharedString::new_static(
        "Когда доиграет последний трек, дальше заиграют похожие по звучанию треки из медиатеки.",
    ),
    about: SharedString::new_static(
        "Каждый трек анализируется, чтобы понять, как он звучит; треки с серверов для этого скачиваются. Пока идёт анализ, приложение занимает больше памяти и сильнее нагружает процессор и видеокарту.",
    ),
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
    indexing: SharedString::new_static("インデックス作成"),
    analysed_t: SharedString::new_static("分析済みの曲：{} / {}"),
    loading: SharedString::new_static("読み込み中…"),
    off: SharedString::new_static("サウンド分析はオフです。"),
    idle: SharedString::new_static(
        "現在、分析するものはありません。新しい曲はライブラリのスキャン後に分析されます。",
    ),
    loading_model: SharedString::new_static("モデルを準備中…"),
    analysing: SharedString::new_static("分析中…"),
    no_model: SharedString::new_static("モデルを利用できません。5 分後に再試行します。"),
    stopped: SharedString::new_static(
        "エラーのため分析が停止しました（詳細はログを参照）。次回のライブラリ更新時に再開します。",
    ),
    no_gpu_short: SharedString::new_static("グラフィックカードが見つかりません"),
    no_gpu: SharedString::new_static(
        "このコンピューターには分析に使えるグラフィックカードがないため、分析は利用できません。",
    ),
    gpu_failed: SharedString::new_static(
        "グラフィックカードがチェックに通りませんでした（詳細はログを参照）。アプリを再起動するまで分析は利用できません。",
    ),
    gpu_retry: SharedString::new_static(
        "グラフィックカードがチェックに通りませんでした（詳細はログを参照）。30 秒後に再試行します。",
    ),
    status_failed: SharedString::new_static("進行状況を読み込めませんでした。"),
    failed_t: SharedString::new_static("分析できませんでした：{}"),
    unreachable_t: SharedString::new_static(
        "接続できないサーバーの曲：{}。次回のライブラリ更新時に再試行します。",
    ),
    close: SharedString::new_static("閉じる"),
    settings_title: SharedString::new_static("おすすめ"),
    autoplay: SharedString::new_static("キューが終わったら再生を続ける"),
    autoplay_desc: SharedString::new_static(
        "最後の曲が終わると、ライブラリから似たサウンドの曲を続けて再生します。",
    ),
    about: SharedString::new_static(
        "各曲を分析して、どのように聞こえるかを調べます。サーバーの曲はそのためにダウンロードされます。分析中はメモリの使用量が増え、プロセッサとグラフィックスの負荷も高くなります。",
    ),
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
    indexing: SharedString::new_static("Indexierung"),
    analysed_t: SharedString::new_static("Analysierte Titel: {} von {}"),
    loading: SharedString::new_static("Wird geladen…"),
    off: SharedString::new_static("Die Klanganalyse ist aus."),
    idle: SharedString::new_static(
        "Gerade gibt es nichts zu analysieren. Neue Titel werden nach einem Bibliotheksscan analysiert.",
    ),
    loading_model: SharedString::new_static("Modell wird vorbereitet…"),
    analysing: SharedString::new_static("Analyse läuft…"),
    no_model: SharedString::new_static(
        "Das Modell ist nicht verfügbar. Neuer Versuch in 5 Minuten.",
    ),
    stopped: SharedString::new_static(
        "Die Analyse wurde wegen eines Fehlers angehalten (Details im Protokoll). Sie läuft beim nächsten Bibliotheksupdate weiter.",
    ),
    no_gpu_short: SharedString::new_static("Keine Grafikkarte gefunden"),
    no_gpu: SharedString::new_static(
        "Dieser Computer hat keine Grafikkarte, die die Analyse nutzen kann, daher ist sie nicht verfügbar.",
    ),
    gpu_failed: SharedString::new_static(
        "Die Grafikkarte hat die Prüfung nicht bestanden (Details im Protokoll). Die Analyse ist bis zum Neustart der App nicht verfügbar.",
    ),
    gpu_retry: SharedString::new_static(
        "Die Grafikkarte hat die Prüfung nicht bestanden (Details im Protokoll). Neuer Versuch in 30 Sekunden.",
    ),
    status_failed: SharedString::new_static("Der Fortschritt konnte nicht gelesen werden."),
    failed_t: SharedString::new_static("Nicht analysierbar: {}"),
    unreachable_t: SharedString::new_static(
        "Titel auf nicht erreichbaren Servern: {}. Beim nächsten Bibliotheksupdate wird es erneut versucht.",
    ),
    close: SharedString::new_static("Schließen"),
    settings_title: SharedString::new_static("Empfehlungen"),
    autoplay: SharedString::new_static("Warteschlange am Ende fortsetzen"),
    autoplay_desc: SharedString::new_static(
        "Wenn der letzte Titel endet, laufen ähnlich klingende Titel aus deiner Mediathek weiter.",
    ),
    about: SharedString::new_static(
        "Jeder Titel wird analysiert, um zu erfassen, wie er klingt; Titel von Servern werden dafür heruntergeladen. Während der Analyse braucht die App mehr Arbeitsspeicher und belastet Prozessor und Grafikkarte stärker.",
    ),
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
    indexing: SharedString::new_static("Indexation"),
    analysed_t: SharedString::new_static("Titres analysés : {} sur {}"),
    loading: SharedString::new_static("Chargement…"),
    off: SharedString::new_static("L'analyse du son est désactivée."),
    idle: SharedString::new_static(
        "Rien à analyser pour le moment. Les nouveaux titres sont analysés après l'actualisation de la bibliothèque.",
    ),
    loading_model: SharedString::new_static("Préparation du modèle…"),
    analysing: SharedString::new_static("Analyse en cours…"),
    no_model: SharedString::new_static("Le modèle est indisponible. Nouvel essai dans 5 minutes."),
    stopped: SharedString::new_static(
        "L'analyse s'est arrêtée sur une erreur (détails dans le journal). Elle reprendra à la prochaine actualisation de la bibliothèque.",
    ),
    no_gpu_short: SharedString::new_static("Aucune carte graphique trouvée"),
    no_gpu: SharedString::new_static(
        "Cet ordinateur n'a pas de carte graphique utilisable par l'analyse, elle n'est donc pas disponible.",
    ),
    gpu_failed: SharedString::new_static(
        "La carte graphique n'a pas passé la vérification (détails dans le journal). L'analyse est indisponible jusqu'au redémarrage de l'application.",
    ),
    gpu_retry: SharedString::new_static(
        "La carte graphique n'a pas passé la vérification (détails dans le journal). Nouvel essai dans 30 secondes.",
    ),
    status_failed: SharedString::new_static("Impossible de lire la progression."),
    failed_t: SharedString::new_static("Impossible à analyser : {}"),
    unreachable_t: SharedString::new_static(
        "Titres sur des serveurs injoignables : {}. Nouvel essai à la prochaine actualisation de la bibliothèque.",
    ),
    close: SharedString::new_static("Fermer"),
    settings_title: SharedString::new_static("Recommandations"),
    autoplay: SharedString::new_static("Prolonger la file d'attente à la fin"),
    autoplay_desc: SharedString::new_static(
        "Quand le dernier morceau se termine, des morceaux au son proche de votre bibliothèque prennent le relais.",
    ),
    about: SharedString::new_static(
        "Chaque titre est analysé pour savoir comment il sonne ; les titres des serveurs sont téléchargés pour cela. Pendant l’analyse, l’app utilise plus de mémoire et sollicite davantage le processeur et la carte graphique.",
    ),
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
    indexing: SharedString::new_static("인덱싱"),
    analysed_t: SharedString::new_static("분석된 곡: {} / {}"),
    loading: SharedString::new_static("불러오는 중…"),
    off: SharedString::new_static("사운드 분석이 꺼져 있습니다."),
    idle: SharedString::new_static(
        "지금은 분석할 것이 없습니다. 새 곡은 라이브러리 스캔 후 분석됩니다.",
    ),
    loading_model: SharedString::new_static("모델 준비 중…"),
    analysing: SharedString::new_static("분석 중…"),
    no_model: SharedString::new_static("모델을 사용할 수 없습니다. 5분 후 다시 시도합니다."),
    stopped: SharedString::new_static(
        "오류로 분석이 중단되었습니다(자세한 내용은 로그 참조). 다음 라이브러리 업데이트 때 계속됩니다.",
    ),
    no_gpu_short: SharedString::new_static("그래픽 카드를 찾을 수 없음"),
    no_gpu: SharedString::new_static(
        "이 컴퓨터에는 분석에 사용할 수 있는 그래픽 카드가 없어 분석을 사용할 수 없습니다.",
    ),
    gpu_failed: SharedString::new_static(
        "그래픽 카드가 검사를 통과하지 못했습니다(자세한 내용은 로그 참조). 앱을 다시 시작할 때까지 분석을 사용할 수 없습니다.",
    ),
    gpu_retry: SharedString::new_static(
        "그래픽 카드가 검사를 통과하지 못했습니다(자세한 내용은 로그 참조). 30초 후 다시 시도합니다.",
    ),
    status_failed: SharedString::new_static("진행 상황을 읽을 수 없습니다."),
    failed_t: SharedString::new_static("분석하지 못함: {}"),
    unreachable_t: SharedString::new_static(
        "연결할 수 없는 서버의 곡: {}. 다음 라이브러리 업데이트 때 다시 시도합니다.",
    ),
    close: SharedString::new_static("닫기"),
    settings_title: SharedString::new_static("추천"),
    autoplay: SharedString::new_static("대기열이 끝나면 계속 재생"),
    autoplay_desc: SharedString::new_static(
        "마지막 곡이 끝나면 보관함에서 비슷한 소리의 곡을 이어서 재생합니다.",
    ),
    about: SharedString::new_static(
        "각 곡을 분석해 어떤 소리인지 파악합니다. 서버의 곡은 이를 위해 다운로드됩니다. 분석하는 동안 앱이 메모리를 더 많이 쓰고 프로세서와 그래픽 카드 부하도 커집니다.",
    ),
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
    indexing: SharedString::new_static("Indicizzazione"),
    analysed_t: SharedString::new_static("Brani analizzati: {} su {}"),
    loading: SharedString::new_static("Caricamento…"),
    off: SharedString::new_static("L'analisi del suono è disattivata."),
    idle: SharedString::new_static(
        "Al momento non c'è nulla da analizzare. I nuovi brani vengono analizzati dopo la scansione della libreria.",
    ),
    loading_model: SharedString::new_static("Preparazione del modello…"),
    analysing: SharedString::new_static("Analisi in corso…"),
    no_model: SharedString::new_static(
        "Il modello non è disponibile. Nuovo tentativo tra 5 minuti.",
    ),
    stopped: SharedString::new_static(
        "L'analisi si è fermata per un errore (dettagli nel registro). Riprenderà al prossimo aggiornamento della libreria.",
    ),
    no_gpu_short: SharedString::new_static("Nessuna scheda grafica trovata"),
    no_gpu: SharedString::new_static(
        "Questo computer non ha una scheda grafica utilizzabile per l'analisi, quindi non è disponibile.",
    ),
    gpu_failed: SharedString::new_static(
        "La scheda grafica non ha superato il controllo (dettagli nel registro). L'analisi non è disponibile fino al riavvio dell'app.",
    ),
    gpu_retry: SharedString::new_static(
        "La scheda grafica non ha superato il controllo (dettagli nel registro). Nuovo tentativo tra 30 secondi.",
    ),
    status_failed: SharedString::new_static("Impossibile leggere l'avanzamento."),
    failed_t: SharedString::new_static("Impossibile analizzare: {}"),
    unreachable_t: SharedString::new_static(
        "Brani su server non raggiungibili: {}. Verranno ritentati al prossimo aggiornamento della libreria.",
    ),
    close: SharedString::new_static("Chiudi"),
    settings_title: SharedString::new_static("Consigli"),
    autoplay: SharedString::new_static("Continua la coda quando finisce"),
    autoplay_desc: SharedString::new_static(
        "Quando l'ultimo brano finisce, continuano brani dal suono simile della tua libreria.",
    ),
    about: SharedString::new_static(
        "Ogni brano viene analizzato per capire come suona; i brani dei server vengono scaricati per farlo. Durante l’analisi l’app usa più memoria e impegna di più il processore e la scheda grafica.",
    ),
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
    indexing: SharedString::new_static("Dizinleme"),
    analysed_t: SharedString::new_static("Analiz edilen parçalar: {} / {}"),
    loading: SharedString::new_static("Yükleniyor…"),
    off: SharedString::new_static("Ses analizi kapalı."),
    idle: SharedString::new_static(
        "Şu anda analiz edilecek bir şey yok. Yeni parçalar kitaplık taramasından sonra analiz edilir.",
    ),
    loading_model: SharedString::new_static("Model hazırlanıyor…"),
    analysing: SharedString::new_static("Analiz ediliyor…"),
    no_model: SharedString::new_static("Model kullanılamıyor. 5 dakika sonra yeniden denenecek."),
    stopped: SharedString::new_static(
        "Analiz bir hata nedeniyle durdu (ayrıntılar günlükte). Bir sonraki kitaplık güncellemesinde devam edecek.",
    ),
    no_gpu_short: SharedString::new_static("Ekran kartı bulunamadı"),
    no_gpu: SharedString::new_static(
        "Bu bilgisayarda analizin kullanabileceği bir ekran kartı yok, bu yüzden analiz kullanılamıyor.",
    ),
    gpu_failed: SharedString::new_static(
        "Ekran kartı denetimi geçemedi (ayrıntılar günlükte). Uygulama yeniden başlatılana kadar analiz kullanılamaz.",
    ),
    gpu_retry: SharedString::new_static(
        "Ekran kartı denetimi geçemedi (ayrıntılar günlükte). 30 saniye sonra yeniden denenecek.",
    ),
    status_failed: SharedString::new_static("İlerleme okunamadı."),
    failed_t: SharedString::new_static("Analiz edilemedi: {}"),
    unreachable_t: SharedString::new_static(
        "Erişilemeyen sunuculardaki parçalar: {}. Bir sonraki kitaplık güncellemesinde yeniden denenecek.",
    ),
    close: SharedString::new_static("Kapat"),
    settings_title: SharedString::new_static("Öneriler"),
    autoplay: SharedString::new_static("Kuyruk bitince çalmaya devam et"),
    autoplay_desc: SharedString::new_static(
        "Son parça bitince kitaplığındaki benzer sesli parçalar çalmaya devam eder.",
    ),
    about: SharedString::new_static(
        "Her parça nasıl tınladığını anlamak için analiz edilir; sunuculardaki parçalar bunun için indirilir. Analiz sürerken uygulama daha fazla bellek kullanır, işlemciyi ve ekran kartını daha çok yorar.",
    ),
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
    indexing: SharedString::new_static("Indeksowanie"),
    analysed_t: SharedString::new_static("Przeanalizowane utwory: {} z {}"),
    loading: SharedString::new_static("Wczytywanie…"),
    off: SharedString::new_static("Analiza brzmienia jest wyłączona."),
    idle: SharedString::new_static(
        "Teraz nie ma nic do analizy. Nowe utwory zostaną przeanalizowane po skanowaniu biblioteki.",
    ),
    loading_model: SharedString::new_static("Przygotowywanie modelu…"),
    analysing: SharedString::new_static("Trwa analiza…"),
    no_model: SharedString::new_static("Model jest niedostępny. Ponowna próba za 5 minut."),
    stopped: SharedString::new_static(
        "Analiza zatrzymała się z powodu błędu (szczegóły w dzienniku). Będzie kontynuowana przy następnej aktualizacji biblioteki.",
    ),
    no_gpu_short: SharedString::new_static("Nie znaleziono karty graficznej"),
    no_gpu: SharedString::new_static(
        "Ten komputer nie ma karty graficznej, z której może korzystać analiza, więc jest ona niedostępna.",
    ),
    gpu_failed: SharedString::new_static(
        "Karta graficzna nie przeszła testu (szczegóły w dzienniku). Analiza jest niedostępna do ponownego uruchomienia aplikacji.",
    ),
    gpu_retry: SharedString::new_static(
        "Karta graficzna nie przeszła testu (szczegóły w dzienniku). Ponowna próba za 30 sekund.",
    ),
    status_failed: SharedString::new_static("Nie udało się odczytać postępu."),
    failed_t: SharedString::new_static("Nie udało się przeanalizować: {}"),
    unreachable_t: SharedString::new_static(
        "Utwory na niedostępnych serwerach: {}. Kolejna próba przy następnej aktualizacji biblioteki.",
    ),
    close: SharedString::new_static("Zamknij"),
    settings_title: SharedString::new_static("Rekomendacje"),
    autoplay: SharedString::new_static("Kontynuuj kolejkę, gdy się skończy"),
    autoplay_desc: SharedString::new_static(
        "Gdy skończy się ostatni utwór, grają dalej podobnie brzmiące utwory z twojej biblioteki.",
    ),
    about: SharedString::new_static(
        "Każdy utwór jest analizowany, by poznać jego brzmienie; utwory z serwerów są w tym celu pobierane. Podczas analizy aplikacja zajmuje więcej pamięci i mocniej obciąża procesor oraz kartę graficzną.",
    ),
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
    indexing: SharedString::new_static("Indexering"),
    analysed_t: SharedString::new_static("Geanalyseerde nummers: {} van {}"),
    loading: SharedString::new_static("Laden…"),
    off: SharedString::new_static("Geluidsanalyse staat uit."),
    idle: SharedString::new_static(
        "Er is nu niets te analyseren. Nieuwe nummers worden na een bibliotheekscan geanalyseerd.",
    ),
    loading_model: SharedString::new_static("Model wordt voorbereid…"),
    analysing: SharedString::new_static("Bezig met analyseren…"),
    no_model: SharedString::new_static(
        "Het model is niet beschikbaar. Nieuwe poging over 5 minuten.",
    ),
    stopped: SharedString::new_static(
        "De analyse is gestopt door een fout (details in het logboek). Ze gaat verder bij de volgende bibliotheekupdate.",
    ),
    no_gpu_short: SharedString::new_static("Geen videokaart gevonden"),
    no_gpu: SharedString::new_static(
        "Deze computer heeft geen videokaart die de analyse kan gebruiken, dus die is niet beschikbaar.",
    ),
    gpu_failed: SharedString::new_static(
        "De videokaart is niet door de controle gekomen (details in het logboek). De analyse is niet beschikbaar tot de app opnieuw start.",
    ),
    gpu_retry: SharedString::new_static(
        "De videokaart is niet door de controle gekomen (details in het logboek). Nieuwe poging over 30 seconden.",
    ),
    status_failed: SharedString::new_static("Kon de voortgang niet lezen."),
    failed_t: SharedString::new_static("Kon niet worden geanalyseerd: {}"),
    unreachable_t: SharedString::new_static(
        "Nummers op onbereikbare servers: {}. Ze worden opnieuw geprobeerd bij de volgende bibliotheekupdate.",
    ),
    close: SharedString::new_static("Sluiten"),
    settings_title: SharedString::new_static("Aanbevelingen"),
    autoplay: SharedString::new_static("Wachtrij voortzetten als die afloopt"),
    autoplay_desc: SharedString::new_static(
        "Als het laatste nummer is afgelopen, spelen soortgelijk klinkende nummers uit je bibliotheek verder.",
    ),
    about: SharedString::new_static(
        "Elk nummer wordt geanalyseerd om te bepalen hoe het klinkt; nummers van servers worden daarvoor gedownload. Tijdens de analyse gebruikt de app meer geheugen en belast ze de processor en de videokaart zwaarder.",
    ),
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
    indexing: SharedString::new_static("Індексація"),
    analysed_t: SharedString::new_static("Проаналізовано треків: {} з {}"),
    loading: SharedString::new_static("Завантаження…"),
    off: SharedString::new_static("Аналіз звучання вимкнено."),
    idle: SharedString::new_static(
        "Зараз аналізувати нічого. Нові треки проаналізуються після сканування медіатеки.",
    ),
    loading_model: SharedString::new_static("Підготовка моделі…"),
    analysing: SharedString::new_static("Триває аналіз…"),
    no_model: SharedString::new_static("Модель недоступна. Повторна спроба за 5 хвилин."),
    stopped: SharedString::new_static(
        "Аналіз зупинився через помилку (подробиці в журналі). Він продовжиться під час наступного оновлення медіатеки.",
    ),
    no_gpu_short: SharedString::new_static("Відеокарту не знайдено"),
    no_gpu: SharedString::new_static(
        "На цьому комп'ютері немає відеокарти, яку може використати аналіз, тому він недоступний.",
    ),
    gpu_failed: SharedString::new_static(
        "Відеокарта не пройшла перевірку (подробиці в журналі). Аналіз недоступний до перезапуску застосунку.",
    ),
    gpu_retry: SharedString::new_static(
        "Відеокарта не пройшла перевірку (подробиці в журналі). Повторна спроба за 30 секунд.",
    ),
    status_failed: SharedString::new_static("Не вдалося прочитати прогрес."),
    failed_t: SharedString::new_static("Не вдалося проаналізувати: {}"),
    unreachable_t: SharedString::new_static(
        "Треки на недоступних серверах: {}. Повторна спроба — під час наступного оновлення медіатеки.",
    ),
    close: SharedString::new_static("Закрити"),
    settings_title: SharedString::new_static("Рекомендації"),
    autoplay: SharedString::new_static("Продовжувати чергу, коли вона закінчиться"),
    autoplay_desc: SharedString::new_static(
        "Коли дограє останній трек, далі гратимуть схожі за звучанням треки з медіатеки.",
    ),
    about: SharedString::new_static(
        "Кожен трек аналізується, щоб зрозуміти, як він звучить; треки із серверів для цього завантажуються. Поки триває аналіз, застосунок займає більше пам’яті й сильніше навантажує процесор і відеокарту.",
    ),
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
    indexing: SharedString::new_static("Lập chỉ mục"),
    analysed_t: SharedString::new_static("Bài đã phân tích: {} / {}"),
    loading: SharedString::new_static("Đang tải…"),
    off: SharedString::new_static("Phân tích âm thanh đang tắt."),
    idle: SharedString::new_static(
        "Hiện không có gì để phân tích. Bài mới sẽ được phân tích sau khi quét thư viện.",
    ),
    loading_model: SharedString::new_static("Đang chuẩn bị mô hình…"),
    analysing: SharedString::new_static("Đang phân tích…"),
    no_model: SharedString::new_static("Mô hình không khả dụng. Sẽ thử lại sau 5 phút."),
    stopped: SharedString::new_static(
        "Quá trình phân tích đã dừng do lỗi (chi tiết trong nhật ký). Sẽ tiếp tục ở lần cập nhật thư viện tiếp theo.",
    ),
    no_gpu_short: SharedString::new_static("Không tìm thấy card đồ họa"),
    no_gpu: SharedString::new_static(
        "Máy tính này không có card đồ họa mà quá trình phân tích có thể dùng, nên không thể phân tích.",
    ),
    gpu_failed: SharedString::new_static(
        "Card đồ họa không vượt qua bước kiểm tra (chi tiết trong nhật ký). Không thể phân tích cho đến khi khởi động lại ứng dụng.",
    ),
    gpu_retry: SharedString::new_static(
        "Card đồ họa không vượt qua bước kiểm tra (chi tiết trong nhật ký). Sẽ thử lại sau 30 giây.",
    ),
    status_failed: SharedString::new_static("Không đọc được tiến độ."),
    failed_t: SharedString::new_static("Không phân tích được: {}"),
    unreachable_t: SharedString::new_static(
        "Bài trên máy chủ không truy cập được: {}. Sẽ thử lại ở lần cập nhật thư viện tiếp theo.",
    ),
    close: SharedString::new_static("Đóng"),
    settings_title: SharedString::new_static("Đề xuất"),
    autoplay: SharedString::new_static("Tiếp tục hàng đợi khi phát hết"),
    autoplay_desc: SharedString::new_static(
        "Khi bài cuối kết thúc, các bài có âm thanh tương tự trong thư viện sẽ tiếp tục phát.",
    ),
    about: SharedString::new_static(
        "Mỗi bài được phân tích để biết nó nghe như thế nào; các bài trên máy chủ sẽ được tải xuống để làm việc này. Trong lúc phân tích, ứng dụng dùng nhiều bộ nhớ hơn và khiến bộ xử lý và card đồ họa phải làm việc nặng hơn.",
    ),
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
    indexing: SharedString::new_static("Pengindeksan"),
    analysed_t: SharedString::new_static("Lagu yang dianalisis: {} dari {}"),
    loading: SharedString::new_static("Memuat…"),
    off: SharedString::new_static("Analisis suara nonaktif."),
    idle: SharedString::new_static(
        "Tidak ada yang perlu dianalisis saat ini. Lagu baru dianalisis setelah pemindaian pustaka.",
    ),
    loading_model: SharedString::new_static("Menyiapkan model…"),
    analysing: SharedString::new_static("Menganalisis…"),
    no_model: SharedString::new_static("Model tidak tersedia. Mencoba lagi dalam 5 menit."),
    stopped: SharedString::new_static(
        "Analisis berhenti karena galat (detail di log). Akan dilanjutkan pada pembaruan pustaka berikutnya.",
    ),
    no_gpu_short: SharedString::new_static("Kartu grafis tidak ditemukan"),
    no_gpu: SharedString::new_static(
        "Komputer ini tidak punya kartu grafis yang bisa dipakai untuk analisis, jadi analisis tidak tersedia.",
    ),
    gpu_failed: SharedString::new_static(
        "Kartu grafis tidak lolos pemeriksaan (detail di log). Analisis tidak tersedia sampai aplikasi dimulai ulang.",
    ),
    gpu_retry: SharedString::new_static(
        "Kartu grafis tidak lolos pemeriksaan (detail di log). Mencoba lagi dalam 30 detik.",
    ),
    status_failed: SharedString::new_static("Tidak dapat membaca progres."),
    failed_t: SharedString::new_static("Tidak dapat dianalisis: {}"),
    unreachable_t: SharedString::new_static(
        "Lagu di server yang tidak dapat dijangkau: {}. Akan dicoba lagi pada pembaruan pustaka berikutnya.",
    ),
    close: SharedString::new_static("Tutup"),
    settings_title: SharedString::new_static("Rekomendasi"),
    autoplay: SharedString::new_static("Lanjutkan antrean saat habis"),
    autoplay_desc: SharedString::new_static(
        "Setelah lagu terakhir selesai, lagu dengan suara serupa dari pustaka Anda terus diputar.",
    ),
    about: SharedString::new_static(
        "Setiap lagu dianalisis untuk mengetahui bagaimana suaranya; lagu dari server diunduh untuk itu. Selama analisis berjalan, aplikasi memakai lebih banyak memori dan lebih membebani prosesor dan kartu grafis.",
    ),
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
    indexing: SharedString::new_static("การทำดัชนี"),
    analysed_t: SharedString::new_static("เพลงที่วิเคราะห์แล้ว: {} จาก {}"),
    loading: SharedString::new_static("กำลังโหลด…"),
    off: SharedString::new_static("การวิเคราะห์เสียงปิดอยู่"),
    idle: SharedString::new_static("ตอนนี้ไม่มีอะไรต้องวิเคราะห์ เพลงใหม่จะถูกวิเคราะห์หลังสแกนคลังเพลง"),
    loading_model: SharedString::new_static("กำลังเตรียมโมเดล…"),
    analysing: SharedString::new_static("กำลังวิเคราะห์…"),
    no_model: SharedString::new_static("ใช้โมเดลไม่ได้ จะลองอีกครั้งใน 5 นาที"),
    stopped: SharedString::new_static(
        "การวิเคราะห์หยุดเพราะเกิดข้อผิดพลาด (ดูรายละเอียดในบันทึก) จะทำต่อเมื่ออัปเดตคลังเพลงครั้งถัดไป",
    ),
    no_gpu_short: SharedString::new_static("ไม่พบการ์ดจอ"),
    no_gpu: SharedString::new_static("คอมพิวเตอร์เครื่องนี้ไม่มีการ์ดจอที่ใช้วิเคราะห์ได้ จึงวิเคราะห์ไม่ได้"),
    gpu_failed: SharedString::new_static(
        "การ์ดจอไม่ผ่านการตรวจสอบ (ดูรายละเอียดในบันทึก) วิเคราะห์ไม่ได้จนกว่าจะเปิดแอปใหม่",
    ),
    gpu_retry: SharedString::new_static(
        "การ์ดจอไม่ผ่านการตรวจสอบ (ดูรายละเอียดในบันทึก) จะลองอีกครั้งใน 30 วินาที",
    ),
    status_failed: SharedString::new_static("อ่านความคืบหน้าไม่ได้"),
    failed_t: SharedString::new_static("วิเคราะห์ไม่ได้: {}"),
    unreachable_t: SharedString::new_static(
        "เพลงบนเซิร์ฟเวอร์ที่เชื่อมต่อไม่ได้: {} จะลองใหม่ตอนอัปเดตคลังเพลงครั้งถัดไป",
    ),
    close: SharedString::new_static("ปิด"),
    settings_title: SharedString::new_static("คำแนะนำ"),
    autoplay: SharedString::new_static("เล่นต่อเมื่อคิวจบ"),
    autoplay_desc: SharedString::new_static(
        "เมื่อเพลงสุดท้ายจบ จะเล่นเพลงที่มีเสียงคล้ายกันจากคลังเพลงของคุณต่อ",
    ),
    about: SharedString::new_static(
        "แต่ละเพลงจะถูกวิเคราะห์เพื่อดูว่ามีเสียงแบบไหน เพลงจากเซิร์ฟเวอร์จะถูกดาวน์โหลดเพื่อการนี้ ระหว่างการวิเคราะห์ แอปจะใช้หน่วยความจำมากขึ้นและใช้งานโปรเซสเซอร์กับการ์ดจอหนักขึ้น",
    ),
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
    indexing: SharedString::new_static("Indexace"),
    analysed_t: SharedString::new_static("Analyzované skladby: {} z {}"),
    loading: SharedString::new_static("Načítání…"),
    off: SharedString::new_static("Analýza zvuku je vypnutá."),
    idle: SharedString::new_static(
        "Teď není co analyzovat. Nové skladby se analyzují po prohledání knihovny.",
    ),
    loading_model: SharedString::new_static("Příprava modelu…"),
    analysing: SharedString::new_static("Probíhá analýza…"),
    no_model: SharedString::new_static("Model není dostupný. Další pokus za 5 minut."),
    stopped: SharedString::new_static(
        "Analýza se zastavila kvůli chybě (podrobnosti v protokolu). Bude pokračovat při další aktualizaci knihovny.",
    ),
    no_gpu_short: SharedString::new_static("Grafická karta nenalezena"),
    no_gpu: SharedString::new_static(
        "Tento počítač nemá grafickou kartu, kterou by analýza mohla použít, proto není dostupná.",
    ),
    gpu_failed: SharedString::new_static(
        "Grafická karta neprošla kontrolou (podrobnosti v protokolu). Analýza je nedostupná až do restartu aplikace.",
    ),
    gpu_retry: SharedString::new_static(
        "Grafická karta neprošla kontrolou (podrobnosti v protokolu). Další pokus za 30 sekund.",
    ),
    status_failed: SharedString::new_static("Průběh se nepodařilo načíst."),
    failed_t: SharedString::new_static("Nepodařilo se analyzovat: {}"),
    unreachable_t: SharedString::new_static(
        "Skladby na nedostupných serverech: {}. Znovu se zkusí při další aktualizaci knihovny.",
    ),
    close: SharedString::new_static("Zavřít"),
    settings_title: SharedString::new_static("Doporučení"),
    autoplay: SharedString::new_static("Pokračovat ve frontě, když skončí"),
    autoplay_desc: SharedString::new_static(
        "Když dohraje poslední skladba, budou dál hrát podobně znějící skladby z vaší knihovny.",
    ),
    about: SharedString::new_static(
        "Každá skladba se analyzuje, aby se zjistilo, jak zní; skladby ze serverů se kvůli tomu stahují. Během analýzy aplikace zabírá více paměti a víc vytěžuje procesor a grafickou kartu.",
    ),
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
    indexing: SharedString::new_static("Indexering"),
    analysed_t: SharedString::new_static("Analyserade spår: {} av {}"),
    loading: SharedString::new_static("Läser in…"),
    off: SharedString::new_static("Ljudanalysen är avstängd."),
    idle: SharedString::new_static(
        "Det finns inget att analysera just nu. Nya spår analyseras efter en biblioteksgenomsökning.",
    ),
    loading_model: SharedString::new_static("Förbereder modellen…"),
    analysing: SharedString::new_static("Analyserar…"),
    no_model: SharedString::new_static("Modellen är inte tillgänglig. Nytt försök om 5 minuter."),
    stopped: SharedString::new_static(
        "Analysen stoppade på grund av ett fel (detaljer i loggen). Den fortsätter vid nästa biblioteksuppdatering.",
    ),
    no_gpu_short: SharedString::new_static("Inget grafikkort hittades"),
    no_gpu: SharedString::new_static(
        "Den här datorn har inget grafikkort som analysen kan använda, så den är inte tillgänglig.",
    ),
    gpu_failed: SharedString::new_static(
        "Grafikkortet klarade inte kontrollen (detaljer i loggen). Analysen är inte tillgänglig förrän appen startas om.",
    ),
    gpu_retry: SharedString::new_static(
        "Grafikkortet klarade inte kontrollen (detaljer i loggen). Nytt försök om 30 sekunder.",
    ),
    status_failed: SharedString::new_static("Kunde inte läsa förloppet."),
    failed_t: SharedString::new_static("Kunde inte analyseras: {}"),
    unreachable_t: SharedString::new_static(
        "Spår på servrar som inte går att nå: {}. De provas igen vid nästa biblioteksuppdatering.",
    ),
    close: SharedString::new_static("Stäng"),
    settings_title: SharedString::new_static("Rekommendationer"),
    autoplay: SharedString::new_static("Fortsätt kön när den tar slut"),
    autoplay_desc: SharedString::new_static(
        "När det sista spåret är slut fortsätter liknande spår från ditt bibliotek att spelas.",
    ),
    about: SharedString::new_static(
        "Varje spår analyseras för att ta reda på hur det låter; spår från servrar laddas ned för det. Medan analysen pågår använder appen mer minne och belastar processorn och grafikkortet mer.",
    ),
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
    indexing: SharedString::new_static("इंडेक्सिंग"),
    analysed_t: SharedString::new_static("विश्लेषित ट्रैक: {} / {}"),
    loading: SharedString::new_static("लोड हो रहा है…"),
    off: SharedString::new_static("ध्वनि विश्लेषण बंद है।"),
    idle: SharedString::new_static(
        "अभी विश्लेषण के लिए कुछ नहीं है। नए ट्रैक लाइब्रेरी स्कैन के बाद विश्लेषित होंगे।",
    ),
    loading_model: SharedString::new_static("मॉडल तैयार हो रहा है…"),
    analysing: SharedString::new_static("विश्लेषण चल रहा है…"),
    no_model: SharedString::new_static("मॉडल उपलब्ध नहीं है। 5 मिनट में फिर कोशिश होगी।"),
    stopped: SharedString::new_static(
        "त्रुटि के कारण विश्लेषण रुक गया (विवरण लॉग में)। लाइब्रेरी के अगले अपडेट पर यह जारी रहेगा।",
    ),
    no_gpu_short: SharedString::new_static("ग्राफ़िक्स कार्ड नहीं मिला"),
    no_gpu: SharedString::new_static(
        "इस कंप्यूटर में ऐसा कोई ग्राफ़िक्स कार्ड नहीं है जिसे विश्लेषण इस्तेमाल कर सके, इसलिए यह उपलब्ध नहीं है।",
    ),
    gpu_failed: SharedString::new_static(
        "ग्राफ़िक्स कार्ड जाँच में पास नहीं हुआ (विवरण लॉग में)। ऐप दोबारा शुरू होने तक विश्लेषण उपलब्ध नहीं है।",
    ),
    gpu_retry: SharedString::new_static(
        "ग्राफ़िक्स कार्ड जाँच में पास नहीं हुआ (विवरण लॉग में)। 30 सेकंड में फिर कोशिश होगी।",
    ),
    status_failed: SharedString::new_static("प्रगति नहीं पढ़ी जा सकी।"),
    failed_t: SharedString::new_static("विश्लेषण नहीं हो सका: {}"),
    unreachable_t: SharedString::new_static(
        "पहुँच से बाहर सर्वरों के ट्रैक: {}। लाइब्रेरी के अगले अपडेट पर फिर से कोशिश होगी।",
    ),
    close: SharedString::new_static("बंद करें"),
    settings_title: SharedString::new_static("सुझाव"),
    autoplay: SharedString::new_static("कतार ख़त्म होने पर चलाना जारी रखें"),
    autoplay_desc: SharedString::new_static(
        "आख़िरी ट्रैक ख़त्म होने पर आपकी लाइब्रेरी के मिलती-जुलती आवाज़ वाले ट्रैक चलते रहेंगे।",
    ),
    about: SharedString::new_static(
        "हर ट्रैक का विश्लेषण किया जाता है ताकि पता चले कि वह कैसा सुनाई देता है; सर्वर के ट्रैक इसके लिए डाउनलोड किए जाते हैं। विश्लेषण के दौरान ऐप ज़्यादा मेमोरी लेता है और प्रोसेसर व ग्राफ़िक्स कार्ड पर ज़्यादा भार डालता है।",
    ),
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
    indexing: SharedString::new_static("Indexación"),
    analysed_t: SharedString::new_static("Pistas analizadas: {} de {}"),
    loading: SharedString::new_static("Cargando…"),
    off: SharedString::new_static("El análisis del sonido está desactivado."),
    idle: SharedString::new_static(
        "Ahora no hay nada que analizar. Las pistas nuevas se analizan tras escanear la biblioteca.",
    ),
    loading_model: SharedString::new_static("Preparando el modelo…"),
    analysing: SharedString::new_static("Analizando…"),
    no_model: SharedString::new_static(
        "El modelo no está disponible. Se reintentará en 5 minutos.",
    ),
    stopped: SharedString::new_static(
        "El análisis se detuvo por un error (detalles en el registro). Continuará en la próxima actualización de la biblioteca.",
    ),
    no_gpu_short: SharedString::new_static("No se encontró una tarjeta gráfica"),
    no_gpu: SharedString::new_static(
        "Este equipo no tiene una tarjeta gráfica que el análisis pueda usar, así que no está disponible.",
    ),
    gpu_failed: SharedString::new_static(
        "La tarjeta gráfica no pasó la comprobación (detalles en el registro). El análisis no está disponible hasta reiniciar la app.",
    ),
    gpu_retry: SharedString::new_static(
        "La tarjeta gráfica no pasó la comprobación (detalles en el registro). Se reintentará en 30 segundos.",
    ),
    status_failed: SharedString::new_static("No se pudo leer el progreso."),
    failed_t: SharedString::new_static("No se pudieron analizar: {}"),
    unreachable_t: SharedString::new_static(
        "Pistas en servidores inaccesibles: {}. Se volverá a intentar en la próxima actualización de la biblioteca.",
    ),
    close: SharedString::new_static("Cerrar"),
    settings_title: SharedString::new_static("Recomendaciones"),
    autoplay: SharedString::new_static("Continuar la cola cuando termine"),
    autoplay_desc: SharedString::new_static(
        "Cuando termine la última pista, seguirán sonando pistas de sonido parecido de tu biblioteca.",
    ),
    about: SharedString::new_static(
        "Cada pista se analiza para saber cómo suena; las pistas de los servidores se descargan para ello. Mientras dura el análisis, la app usa más memoria y exige más al procesador y a la tarjeta gráfica.",
    ),
};
