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
    pub unreached_title: SharedString,
    pub unreached_t: SharedString,
    pub unreached_windows: SharedString,
    pub unreached_macos: SharedString,
    pub unreached_linux: SharedString,
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

    pub fn unreached(&self, device: &str) -> String {
        fill(&self.unreached_t, &[device])
    }

    pub fn unreached_hint(&self) -> &SharedString {
        if cfg!(target_os = "windows") {
            &self.unreached_windows
        } else if cfg!(target_os = "macos") {
            &self.unreached_macos
        } else {
            &self.unreached_linux
        }
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
    unreached_title: SharedString::new_static("Streaming didn't start"),
    unreached_t: SharedString::new_static(
        "{} accepted the track but never came to fetch it, so playback is back on this computer. Something is blocking the device's connection to Pawse. Most often it's a firewall on this computer; a VPN or a guest Wi-Fi network can do the same.",
    ),
    unreached_windows: SharedString::new_static(
        "Open Windows Security → Firewall & network protection → Allow an app through firewall, find Pawse and tick Private (and Public if this network is set to Public). Then pick the device again.",
    ),
    unreached_macos: SharedString::new_static(
        "Open System Settings → Network → Firewall → Options…, find Pawse and set it to Allow incoming connections. Then pick the device again.",
    ),
    unreached_linux: SharedString::new_static(
        "Allow incoming TCP and UDP ports 39831–39840 in your firewall, then pick the device again:",
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
    unreached_title: SharedString::new_static("串流未能开始"),
    unreached_t: SharedString::new_static(
        "{} 接收了曲目，却一直没有来获取，因此已改回在本机播放。有东西阻止了设备连接到 Pawse。最常见的原因是这台电脑上的防火墙；VPN 或访客 Wi-Fi 网络也可能造成同样的问题。",
    ),
    unreached_windows: SharedString::new_static(
        "打开“Windows 安全中心”→“防火墙和网络保护”→“允许应用通过防火墙”，找到 Pawse，勾选“专用”（如果当前网络被设为公用，也勾选“公用”）。然后重新选择该设备。",
    ),
    unreached_macos: SharedString::new_static(
        "打开“系统设置”→“网络”→“防火墙”→“选项…”，找到 Pawse 并设为“允许传入连接”。然后重新选择该设备。",
    ),
    unreached_linux: SharedString::new_static(
        "在防火墙中放行传入的 TCP 和 UDP 端口 39831–39840，然后重新选择该设备：",
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
    unreached_title: SharedString::new_static("A transmissão não começou"),
    unreached_t: SharedString::new_static(
        "{} aceitou a faixa, mas nunca veio buscá-la, por isso a reprodução voltou para este computador. Algo está bloqueando a conexão do dispositivo com o Pawse. Na maioria das vezes é um firewall neste computador; uma VPN ou uma rede Wi-Fi de convidados pode causar o mesmo.",
    ),
    unreached_windows: SharedString::new_static(
        "Abra Segurança do Windows → Firewall e proteção de rede → Permitir um aplicativo pelo firewall, encontre o Pawse e marque Particular (e Pública, se esta rede estiver definida como pública). Depois escolha o dispositivo novamente.",
    ),
    unreached_macos: SharedString::new_static(
        "Abra Ajustes do Sistema → Rede → Firewall → Opções…, encontre o Pawse e defina como Permitir conexões de entrada. Depois escolha o dispositivo novamente.",
    ),
    unreached_linux: SharedString::new_static(
        "Libere as portas TCP e UDP 39831–39840 de entrada no seu firewall e escolha o dispositivo novamente:",
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
    unreached_title: SharedString::new_static("Трансляция не началась"),
    unreached_t: SharedString::new_static(
        "{} принял трек, но так и не пришёл за ним, поэтому воспроизведение вернулось на этот компьютер. Что-то блокирует подключение устройства к Pawse. Чаще всего это брандмауэр на этом компьютере; то же могут делать VPN или гостевая сеть Wi-Fi.",
    ),
    unreached_windows: SharedString::new_static(
        "Откройте «Безопасность Windows» → «Брандмауэр и защита сети» → «Разрешить работу с приложением через брандмауэр», найдите Pawse и отметьте «Частная» (и «Публичная», если сеть считается публичной). Затем выберите устройство снова.",
    ),
    unreached_macos: SharedString::new_static(
        "Откройте «Системные настройки» → «Сеть» → «Брандмауэр» → «Параметры…», найдите Pawse и выберите «Разрешить входящие подключения». Затем выберите устройство снова.",
    ),
    unreached_linux: SharedString::new_static(
        "Разрешите в брандмауэре входящие TCP- и UDP-порты 39831–39840 и выберите устройство снова:",
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
    unreached_title: SharedString::new_static("ストリーミングを開始できませんでした"),
    unreached_t: SharedString::new_static(
        "{} はトラックを受け付けましたが、取得しに来ませんでした。そのため再生はこのコンピューターに戻りました。デバイスから Pawse への接続が何かにブロックされています。多くの場合、このコンピューターのファイアウォールが原因です。VPN やゲスト用 Wi-Fi でも同じことが起こります。",
    ),
    unreached_windows: SharedString::new_static(
        "「Windows セキュリティ」→「ファイアウォールとネットワーク保護」→「ファイアウォールによるアプリケーションの許可」を開き、Pawse を見つけて「プライベート」（ネットワークがパブリックの場合は「パブリック」も）にチェックを入れてください。その後、もう一度デバイスを選択してください。",
    ),
    unreached_macos: SharedString::new_static(
        "「システム設定」→「ネットワーク」→「ファイアウォール」→「オプション…」を開き、Pawse を探して「着信接続を許可」に設定してください。その後、もう一度デバイスを選択してください。",
    ),
    unreached_linux: SharedString::new_static(
        "ファイアウォールで着信の TCP/UDP ポート 39831–39840 を許可し、もう一度デバイスを選択してください：",
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
    unreached_title: SharedString::new_static("Streaming wurde nicht gestartet"),
    unreached_t: SharedString::new_static(
        "{} hat den Titel angenommen, ihn aber nie abgeholt, deshalb läuft die Wiedergabe wieder auf diesem Computer. Etwas blockiert die Verbindung des Geräts zu Pawse. Meistens ist es eine Firewall auf diesem Computer; auch ein VPN oder ein WLAN für Gäste kann das verursachen.",
    ),
    unreached_windows: SharedString::new_static(
        "Öffnen Sie Windows-Sicherheit → Firewall- & Netzwerkschutz → App durch die Firewall zulassen, suchen Sie Pawse und setzen Sie ein Häkchen bei Privat (und bei Öffentlich, wenn dieses Netzwerk als öffentlich eingestuft ist). Wählen Sie das Gerät danach erneut aus.",
    ),
    unreached_macos: SharedString::new_static(
        "Öffnen Sie Systemeinstellungen → Netzwerk → Firewall → Optionen…, suchen Sie Pawse und stellen Sie „Eingehende Verbindungen erlauben“ ein. Wählen Sie das Gerät danach erneut aus.",
    ),
    unreached_linux: SharedString::new_static(
        "Erlauben Sie in Ihrer Firewall eingehende TCP- und UDP-Ports 39831–39840 und wählen Sie das Gerät danach erneut aus:",
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
    unreached_title: SharedString::new_static("La diffusion n'a pas démarré"),
    unreached_t: SharedString::new_static(
        "{} a accepté le morceau mais n'est jamais venu le récupérer, la lecture est donc revenue sur cet ordinateur. Quelque chose bloque la connexion de l'appareil vers Pawse. Le plus souvent, c'est un pare-feu sur cet ordinateur ; un VPN ou un réseau Wi-Fi invité peut avoir le même effet.",
    ),
    unreached_windows: SharedString::new_static(
        "Ouvrez Sécurité Windows → Pare-feu et protection du réseau → Autoriser une application via le pare-feu, trouvez Pawse et cochez Privé (et Public si ce réseau est défini comme public). Choisissez ensuite de nouveau l'appareil.",
    ),
    unreached_macos: SharedString::new_static(
        "Ouvrez Réglages Système → Réseau → Coupe-feu → Options…, trouvez Pawse et choisissez « Autoriser les connexions entrantes ». Choisissez ensuite de nouveau l'appareil.",
    ),
    unreached_linux: SharedString::new_static(
        "Autorisez les ports TCP et UDP 39831–39840 en entrée dans votre pare-feu, puis choisissez de nouveau l'appareil :",
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
    unreached_title: SharedString::new_static("스트리밍이 시작되지 않았습니다"),
    unreached_t: SharedString::new_static(
        "{}이(가) 트랙을 받아들였지만 가져가지 않아 재생이 이 컴퓨터로 돌아왔습니다. 무언가가 기기에서 Pawse로의 연결을 막고 있습니다. 대개 이 컴퓨터의 방화벽이 원인이며, VPN이나 게스트 Wi-Fi 네트워크도 같은 문제를 일으킬 수 있습니다.",
    ),
    unreached_windows: SharedString::new_static(
        "Windows 보안 → 방화벽 및 네트워크 보호 → 방화벽을 통해 앱 허용을 열고 Pawse를 찾아 '개인'(네트워크가 공용으로 설정되어 있다면 '공용'도)에 체크하세요. 그런 다음 기기를 다시 선택하세요.",
    ),
    unreached_macos: SharedString::new_static(
        "시스템 설정 → 네트워크 → 방화벽 → 옵션…을 열고 Pawse를 찾아 '들어오는 연결 허용'으로 설정하세요. 그런 다음 기기를 다시 선택하세요.",
    ),
    unreached_linux: SharedString::new_static(
        "방화벽에서 들어오는 TCP 및 UDP 포트 39831–39840을 허용한 다음 기기를 다시 선택하세요:",
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
    unreached_title: SharedString::new_static("Lo streaming non è partito"),
    unreached_t: SharedString::new_static(
        "{} ha accettato il brano ma non è mai venuto a prenderlo, quindi la riproduzione è tornata su questo computer. Qualcosa blocca la connessione del dispositivo verso Pawse. Di solito è un firewall su questo computer; anche una VPN o una rete Wi-Fi ospiti può causare lo stesso problema.",
    ),
    unreached_windows: SharedString::new_static(
        "Apri Sicurezza di Windows → Firewall e protezione della rete → Consenti app attraverso il firewall, trova Pawse e seleziona Privata (e Pubblica se questa rete è impostata come pubblica). Poi scegli di nuovo il dispositivo.",
    ),
    unreached_macos: SharedString::new_static(
        "Apri Impostazioni di Sistema → Rete → Firewall → Opzioni…, trova Pawse e impostalo su Consenti connessioni in entrata. Poi scegli di nuovo il dispositivo.",
    ),
    unreached_linux: SharedString::new_static(
        "Consenti nel firewall le porte TCP e UDP 39831–39840 in entrata, poi scegli di nuovo il dispositivo:",
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
    unreached_title: SharedString::new_static("Yayın başlamadı"),
    unreached_t: SharedString::new_static(
        "{} parçayı kabul etti ancak onu almaya hiç gelmedi, bu yüzden oynatma bu bilgisayara geri döndü. Bir şey cihazın Pawse'a bağlanmasını engelliyor. Çoğunlukla bu bilgisayardaki bir güvenlik duvarıdır; VPN veya misafir Wi-Fi ağı da aynı sorunu yaratabilir.",
    ),
    unreached_windows: SharedString::new_static(
        "Windows Güvenliği → Güvenlik duvarı ve ağ koruması → Güvenlik duvarı üzerinden bir uygulamaya izin ver bölümünü açın, Pawse'u bulun ve Özel'i (ağ Genel olarak ayarlıysa Genel'i de) işaretleyin. Ardından cihazı yeniden seçin.",
    ),
    unreached_macos: SharedString::new_static(
        "Sistem Ayarları → Ağ → Güvenlik Duvarı → Seçenekler… bölümünü açın, Pawse'u bulun ve Gelen bağlantılara izin ver olarak ayarlayın. Ardından cihazı yeniden seçin.",
    ),
    unreached_linux: SharedString::new_static(
        "Güvenlik duvarınızda gelen TCP ve UDP 39831–39840 bağlantı noktalarına izin verin, ardından cihazı yeniden seçin:",
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
    unreached_title: SharedString::new_static("Strumieniowanie nie ruszyło"),
    unreached_t: SharedString::new_static(
        "{} przyjęło utwór, ale nigdy po niego nie przyszło, więc odtwarzanie wróciło na ten komputer. Coś blokuje połączenie urządzenia z Pawse. Najczęściej jest to zapora sieciowa na tym komputerze; to samo może powodować VPN lub sieć Wi-Fi dla gości.",
    ),
    unreached_windows: SharedString::new_static(
        "Otwórz Zabezpieczenia Windows → Zapora i ochrona sieci → Zezwalaj aplikacji na dostęp przez zaporę, znajdź Pawse i zaznacz Prywatna (oraz Publiczna, jeśli ta sieć jest ustawiona jako publiczna). Potem wybierz urządzenie ponownie.",
    ),
    unreached_macos: SharedString::new_static(
        "Otwórz Ustawienia systemowe → Sieć → Zapora → Opcje…, znajdź Pawse i ustaw Zezwalaj na połączenia przychodzące. Potem wybierz urządzenie ponownie.",
    ),
    unreached_linux: SharedString::new_static(
        "Zezwól w zaporze na przychodzące porty TCP i UDP 39831–39840, a potem wybierz urządzenie ponownie:",
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
    unreached_title: SharedString::new_static("Streamen is niet gestart"),
    unreached_t: SharedString::new_static(
        "{} heeft het nummer geaccepteerd maar kwam het nooit ophalen, daarom speelt het weer af op deze computer. Iets blokkeert de verbinding van het apparaat met Pawse. Meestal is dat een firewall op deze computer; een vpn of een gastnetwerk kan hetzelfde veroorzaken.",
    ),
    unreached_windows: SharedString::new_static(
        "Open Windows-beveiliging → Firewall en netwerkbeveiliging → Een app toestaan via de firewall, zoek Pawse en vink Privé aan (en Openbaar als dit netwerk als openbaar is ingesteld). Kies het apparaat daarna opnieuw.",
    ),
    unreached_macos: SharedString::new_static(
        "Open Systeeminstellingen → Netwerk → Firewall → Opties…, zoek Pawse en zet het op Sta inkomende verbindingen toe. Kies het apparaat daarna opnieuw.",
    ),
    unreached_linux: SharedString::new_static(
        "Sta in je firewall inkomende TCP- en UDP-poorten 39831–39840 toe en kies het apparaat daarna opnieuw:",
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
    unreached_title: SharedString::new_static("Трансляція не почалася"),
    unreached_t: SharedString::new_static(
        "{} прийняв трек, але так і не прийшов по нього, тому відтворення повернулося на цей комп'ютер. Щось блокує підключення пристрою до Pawse. Найчастіше це брандмауер на цьому комп'ютері; те саме можуть робити VPN або гостьова мережа Wi-Fi.",
    ),
    unreached_windows: SharedString::new_static(
        "Відкрийте «Безпека Windows» → «Брандмауер і захист мережі» → «Дозволити роботу з програмою через брандмауер», знайдіть Pawse і позначте «Приватна» (і «Загальнодоступна», якщо мережу позначено як загальнодоступну). Потім виберіть пристрій знову.",
    ),
    unreached_macos: SharedString::new_static(
        "Відкрийте «Системні налаштування» → «Мережа» → «Брандмауер» → «Параметри…», знайдіть Pawse і виберіть «Дозволити вхідні підключення». Потім виберіть пристрій знову.",
    ),
    unreached_linux: SharedString::new_static(
        "Дозвольте у брандмауері вхідні TCP- і UDP-порти 39831–39840 і виберіть пристрій знову:",
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
    unreached_title: SharedString::new_static("Phát trực tiếp chưa bắt đầu"),
    unreached_t: SharedString::new_static(
        "{} đã nhận bài hát nhưng không bao giờ đến lấy, nên phát nhạc đã quay lại máy tính này. Có thứ gì đó đang chặn kết nối từ thiết bị đến Pawse. Thường là tường lửa trên máy tính này; VPN hoặc mạng Wi-Fi dành cho khách cũng có thể gây ra điều tương tự.",
    ),
    unreached_windows: SharedString::new_static(
        "Mở Bảo mật Windows → Tường lửa và bảo vệ mạng → Cho phép ứng dụng đi qua tường lửa, tìm Pawse và chọn Riêng tư (và Công cộng nếu mạng này được đặt là công cộng). Sau đó chọn lại thiết bị.",
    ),
    unreached_macos: SharedString::new_static(
        "Mở Cài đặt hệ thống → Mạng → Tường lửa → Tùy chọn…, tìm Pawse và đặt thành Cho phép kết nối đến. Sau đó chọn lại thiết bị.",
    ),
    unreached_linux: SharedString::new_static(
        "Cho phép các cổng TCP và UDP 39831–39840 đi vào trong tường lửa của bạn, rồi chọn lại thiết bị:",
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
    unreached_title: SharedString::new_static("Streaming tidak dimulai"),
    unreached_t: SharedString::new_static(
        "{} menerima lagu tetapi tidak pernah datang mengambilnya, jadi pemutaran kembali ke komputer ini. Ada sesuatu yang memblokir koneksi perangkat ke Pawse. Paling sering penyebabnya firewall di komputer ini; VPN atau jaringan Wi-Fi tamu bisa menyebabkan hal yang sama.",
    ),
    unreached_windows: SharedString::new_static(
        "Buka Keamanan Windows → Firewall & perlindungan jaringan → Izinkan aplikasi melalui firewall, cari Pawse dan centang Pribadi (dan Publik jika jaringan ini diatur sebagai publik). Lalu pilih perangkat lagi.",
    ),
    unreached_macos: SharedString::new_static(
        "Buka Pengaturan Sistem → Jaringan → Firewall → Opsi…, cari Pawse dan atur ke Izinkan koneksi masuk. Lalu pilih perangkat lagi.",
    ),
    unreached_linux: SharedString::new_static(
        "Izinkan port TCP dan UDP 39831–39840 yang masuk di firewall Anda, lalu pilih perangkat lagi:",
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
    unreached_title: SharedString::new_static("การสตรีมไม่เริ่มทำงาน"),
    unreached_t: SharedString::new_static(
        "{} รับเพลงไว้แล้วแต่ไม่เคยมาดึงไป การเล่นจึงกลับมาที่คอมพิวเตอร์เครื่องนี้ มีบางอย่างกำลังบล็อกการเชื่อมต่อจากอุปกรณ์มายัง Pawse ส่วนใหญ่มักเป็นไฟร์วอลล์บนคอมพิวเตอร์เครื่องนี้ VPN หรือเครือข่าย Wi-Fi สำหรับแขกก็อาจทำให้เกิดอาการเดียวกันได้",
    ),
    unreached_windows: SharedString::new_static(
        "เปิดความปลอดภัยของ Windows → ไฟร์วอลล์และการป้องกันเครือข่าย → อนุญาตแอปผ่านไฟร์วอลล์ ค้นหา Pawse แล้วเลือก ส่วนตัว (และ สาธารณะ หากเครือข่ายนี้ตั้งเป็นสาธารณะ) จากนั้นเลือกอุปกรณ์อีกครั้ง",
    ),
    unreached_macos: SharedString::new_static(
        "เปิดการตั้งค่าระบบ → เครือข่าย → ไฟร์วอลล์ → ตัวเลือก… ค้นหา Pawse แล้วตั้งเป็น อนุญาตการเชื่อมต่อขาเข้า จากนั้นเลือกอุปกรณ์อีกครั้ง",
    ),
    unreached_linux: SharedString::new_static(
        "อนุญาตพอร์ต TCP และ UDP ขาเข้า 39831–39840 ในไฟร์วอลล์ของคุณ แล้วเลือกอุปกรณ์อีกครั้ง:",
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
    unreached_title: SharedString::new_static("Streamování se nespustilo"),
    unreached_t: SharedString::new_static(
        "{} přijalo skladbu, ale nikdy si pro ni nepřišlo, proto se přehrávání vrátilo na tento počítač. Něco blokuje připojení zařízení k Pawse. Nejčastěji jde o firewall na tomto počítači; totéž může způsobit VPN nebo hostovská síť Wi-Fi.",
    ),
    unreached_windows: SharedString::new_static(
        "Otevřete Zabezpečení Windows → Brána firewall a ochrana sítě → Povolit aplikaci prostřednictvím brány firewall, najděte Pawse a zaškrtněte Soukromá (a Veřejná, pokud je tato síť nastavena jako veřejná). Poté zařízení vyberte znovu.",
    ),
    unreached_macos: SharedString::new_static(
        "Otevřete Nastavení systému → Síť → Firewall → Volby…, najděte Pawse a nastavte Povolit příchozí spojení. Poté zařízení vyberte znovu.",
    ),
    unreached_linux: SharedString::new_static(
        "Povolte ve firewallu příchozí porty TCP a UDP 39831–39840 a poté zařízení vyberte znovu:",
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
    unreached_title: SharedString::new_static("Strömningen startade inte"),
    unreached_t: SharedString::new_static(
        "{} godtog låten men kom aldrig och hämtade den, så uppspelningen är tillbaka på den här datorn. Något blockerar enhetens anslutning till Pawse. Oftast är det en brandvägg på den här datorn; ett VPN eller ett gästnätverk kan ge samma sak.",
    ),
    unreached_windows: SharedString::new_static(
        "Öppna Windows-säkerhet → Brandvägg och nätverksskydd → Tillåt en app genom brandväggen, hitta Pawse och markera Privat (och Offentligt om nätverket är inställt som offentligt). Välj sedan enheten igen.",
    ),
    unreached_macos: SharedString::new_static(
        "Öppna Systeminställningar → Nätverk → Brandvägg → Alternativ…, hitta Pawse och ställ in Tillåt inkommande anslutningar. Välj sedan enheten igen.",
    ),
    unreached_linux: SharedString::new_static(
        "Tillåt inkommande TCP- och UDP-portar 39831–39840 i brandväggen och välj sedan enheten igen:",
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
    unreached_title: SharedString::new_static("स्ट्रीमिंग शुरू नहीं हुई"),
    unreached_t: SharedString::new_static(
        "{} ने ट्रैक स्वीकार किया, लेकिन उसे लेने कभी नहीं आया, इसलिए प्लेबैक इस कंप्यूटर पर लौट आया है। कोई चीज़ डिवाइस से Pawse तक के कनेक्शन को रोक रही है। अक्सर यह इस कंप्यूटर का फ़ायरवॉल होता है; VPN या गेस्ट Wi-Fi नेटवर्क भी यही समस्या पैदा कर सकता है।",
    ),
    unreached_windows: SharedString::new_static(
        "Windows सुरक्षा → फ़ायरवॉल और नेटवर्क सुरक्षा → फ़ायरवॉल के माध्यम से किसी ऐप को अनुमति दें खोलें, Pawse ढूँढें और निजी (और सार्वजनिक, यदि यह नेटवर्क सार्वजनिक के रूप में सेट है) चुनें। फिर डिवाइस दोबारा चुनें।",
    ),
    unreached_macos: SharedString::new_static(
        "सिस्टम सेटिंग्स → नेटवर्क → फ़ायरवॉल → विकल्प… खोलें, Pawse ढूँढें और इसे आने वाले कनेक्शन की अनुमति दें पर सेट करें। फिर डिवाइस दोबारा चुनें।",
    ),
    unreached_linux: SharedString::new_static(
        "अपने फ़ायरवॉल में आने वाले TCP और UDP पोर्ट 39831–39840 की अनुमति दें, फिर डिवाइस दोबारा चुनें:",
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
    unreached_title: SharedString::new_static("La transmisión no se inició"),
    unreached_t: SharedString::new_static(
        "{} aceptó la pista pero nunca vino a buscarla, así que la reproducción ha vuelto a este equipo. Algo está bloqueando la conexión del dispositivo con Pawse. Lo más habitual es un firewall de este equipo; una VPN o una red Wi-Fi de invitados puede causar lo mismo.",
    ),
    unreached_windows: SharedString::new_static(
        "Abre Seguridad de Windows → Firewall y protección de red → Permitir una aplicación a través del firewall, busca Pawse y marca Privada (y Pública si esta red está configurada como pública). Después elige el dispositivo de nuevo.",
    ),
    unreached_macos: SharedString::new_static(
        "Abre Ajustes del Sistema → Red → Firewall → Opciones…, busca Pawse y ponlo en Permitir conexiones entrantes. Después elige el dispositivo de nuevo.",
    ),
    unreached_linux: SharedString::new_static(
        "Permite en tu firewall los puertos TCP y UDP 39831–39840 entrantes y después elige el dispositivo de nuevo:",
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
