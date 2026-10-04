use gpui::SharedString;

use super::{Lang, active, fill};

pub struct BitPerfectStrings {
    pub bit_perfect: SharedString,
    pub not_bit_perfect: SharedString,
    pub click_for_details: SharedString,
    pub not_exclusive: SharedString,
    pub native_rate_off: SharedString,
    pub system_volume_t: SharedString,
    pub system_muted: SharedString,
    pub sample_rate_t: SharedString,
    pub bit_depth_t: SharedString,
    pub no_source: SharedString,
    pub about_title: SharedString,
    pub about: SharedString,
    pub about_exclusive: SharedString,
    pub about_native_rate: SharedString,
    pub status_ok: SharedString,
    pub status_issues: SharedString,
    pub not_exclusive_desc: SharedString,
    pub system_volume_desc: SharedString,
    pub system_volume_caution: SharedString,
    pub system_muted_desc: SharedString,
    pub sample_rate_desc: SharedString,
    pub bit_depth_desc: SharedString,
    pub no_source_desc: SharedString,
    pub not_an_error: SharedString,
    pub got_it: SharedString,
}

impl BitPerfectStrings {
    pub fn system_volume(&self, percent: u32) -> String {
        fill(&self.system_volume_t, &[&percent.to_string()])
    }

    pub fn sample_rate(&self, source: &str, device: &str) -> String {
        fill(&self.sample_rate_t, &[source, device])
    }

    pub fn bit_depth(&self, bits: u8) -> String {
        fill(&self.bit_depth_t, &[&bits.to_string()])
    }
}

pub fn bit_perfect_strings() -> &'static BitPerfectStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static BitPerfectStrings {
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

static EN: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect playback"),
    not_bit_perfect: SharedString::new_static("Not bit-perfect"),
    click_for_details: SharedString::new_static("Click for details"),
    not_exclusive: SharedString::new_static("Exclusive mode isn't active"),
    native_rate_off: SharedString::new_static("Native sample rate isn't active"),
    system_volume_t: SharedString::new_static("System volume is at {}%, not 100%"),
    system_muted: SharedString::new_static("System sound is muted"),
    sample_rate_t: SharedString::new_static("The track is {}, but the device runs at {}"),
    bit_depth_t: SharedString::new_static("The track is {}-bit, the device gets 24 bits"),
    no_source: SharedString::new_static("Nothing is playing yet"),
    about_title: SharedString::new_static("What is bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect means nothing changes the audio on its way from the file to the DAC, the chip that turns digital audio into the signal for your headphones or speakers. The DAC gets exactly the data stored in the file.",
    ),
    about_exclusive: SharedString::new_static(
        "Exclusive mode removes most of what could change it: other apps aren't mixed in, and the device switches to each track's sample rate, so nothing has to resample the audio.",
    ),
    about_native_rate: SharedString::new_static(
        "Native sample rate mode makes PipeWire run the device at each track's sample rate, so nothing has to resample the audio.",
    ),
    status_ok: SharedString::new_static(
        "Right now playback is bit-perfect: the device gets the file's data unchanged.",
    ),
    status_issues: SharedString::new_static("What changes the audio right now:"),
    not_exclusive_desc: SharedString::new_static(
        "The audio goes through the system mixer, where it's mixed with other apps and may be resampled.",
    ),
    system_volume_desc: SharedString::new_static(
        "The system volume is applied after Pawse hands the audio off, so the sound is turned down before it reaches the DAC. To clear this, set the system volume to 100% and control loudness on a DAC or amplifier with its own volume knob.",
    ),
    system_volume_caution: SharedString::new_static(
        "Be careful: at 100% the sound can be very loud, especially in headphones plugged straight into the computer.",
    ),
    system_muted_desc: SharedString::new_static(
        "The output device is muted, so the DAC gets silence. Unmute it in the system sound settings.",
    ),
    sample_rate_desc: SharedString::new_static(
        "The device isn't running at the track's sample rate, so the audio can't reach it unchanged. Usually the device doesn't support this rate, or another app switched it.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse sends audio to the device as 32-bit floating point, which keeps 24 bits of precision, so the lowest bits of this track are rounded off. That's far below anything you can hear.",
    ),
    no_source_desc: SharedString::new_static(
        "Nothing is playing yet. Start a track, and this icon will show whether it reaches the device unchanged.",
    ),
    not_an_error: SharedString::new_static(
        "None of this is an error: the music plays normally. The icon only shows whether the device gets exactly what's in the file.",
    ),
    got_it: SharedString::new_static("Got it"),
};

static ZH: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("比特完美播放"),
    not_bit_perfect: SharedString::new_static("非比特完美"),
    click_for_details: SharedString::new_static("点击查看详情"),
    not_exclusive: SharedString::new_static("独占模式未生效"),
    native_rate_off: SharedString::new_static("原生采样率未生效"),
    system_volume_t: SharedString::new_static("系统音量为 {}%，而不是 100%"),
    system_muted: SharedString::new_static("系统已静音"),
    sample_rate_t: SharedString::new_static("曲目为 {}，但设备运行在 {}"),
    bit_depth_t: SharedString::new_static("曲目为 {} 位，设备只收到 24 位"),
    no_source: SharedString::new_static("尚未播放任何内容"),
    about_title: SharedString::new_static("什么是比特完美？"),
    about: SharedString::new_static(
        "比特完美是指音频从文件到 DAC 的途中没有任何改动。DAC 是把数字音频转换成耳机或音箱信号的芯片，它收到的正是文件中存储的数据。",
    ),
    about_exclusive: SharedString::new_static(
        "独占模式去除了大部分可能改动音频的环节：不会混入其他应用的声音，设备也会切换到每首曲目的采样率，因此无需重采样。",
    ),
    about_native_rate: SharedString::new_static(
        "原生采样率模式让 PipeWire 以每首曲目的采样率运行设备，因此无需重采样。",
    ),
    status_ok: SharedString::new_static("当前为比特完美播放：设备收到的是未经改动的文件数据。"),
    status_issues: SharedString::new_static("当前改动音频的因素："),
    not_exclusive_desc: SharedString::new_static(
        "音频经过系统混音器，在那里与其他应用的声音混合，并可能被重采样。",
    ),
    system_volume_desc: SharedString::new_static(
        "系统音量在 Pawse 交出音频之后才生效，所以声音在到达 DAC 之前就被调小了。要消除这一项，请把系统音量调到 100%，并用自带音量旋钮的 DAC 或放大器调节音量。",
    ),
    system_volume_caution: SharedString::new_static(
        "请注意：音量为 100% 时声音可能非常大，尤其是直接插在电脑上的耳机。",
    ),
    system_muted_desc: SharedString::new_static(
        "输出设备已静音，所以 DAC 收到的是静音。请在系统声音设置中取消静音。",
    ),
    sample_rate_desc: SharedString::new_static(
        "设备没有以曲目的采样率运行，所以音频无法原样到达设备。通常是设备不支持该采样率，或者被其他应用切换了。",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse 以 32 位浮点格式向设备发送音频，它保留 24 位精度，因此这首曲目的最低几位会被舍入。这远低于人耳能听到的范围。",
    ),
    no_source_desc: SharedString::new_static(
        "尚未播放任何内容。开始播放一首曲目后，这个图标会显示它是否原样到达设备。",
    ),
    not_an_error: SharedString::new_static(
        "以上都不是错误：音乐照常播放。这个图标只表示设备收到的是否与文件内容完全一致。",
    ),
    got_it: SharedString::new_static("知道了"),
};

static PT: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Reprodução bit-perfect"),
    not_bit_perfect: SharedString::new_static("Não é bit-perfect"),
    click_for_details: SharedString::new_static("Clique para ver detalhes"),
    not_exclusive: SharedString::new_static("O modo exclusivo não está ativo"),
    native_rate_off: SharedString::new_static("A taxa nativa não está ativa"),
    system_volume_t: SharedString::new_static("O volume do sistema está em {}%, não em 100%"),
    system_muted: SharedString::new_static("O som do sistema está mudo"),
    sample_rate_t: SharedString::new_static("A faixa é de {}, mas o dispositivo está em {}"),
    bit_depth_t: SharedString::new_static("A faixa é de {} bits, o dispositivo recebe 24 bits"),
    no_source: SharedString::new_static("Nada está tocando ainda"),
    about_title: SharedString::new_static("O que é bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect significa que nada altera o áudio no caminho do arquivo até o DAC, o chip que transforma o áudio digital no sinal para seus fones ou alto-falantes. O DAC recebe exatamente os dados gravados no arquivo.",
    ),
    about_exclusive: SharedString::new_static(
        "O modo exclusivo remove quase tudo o que poderia alterá-lo: outros aplicativos não são misturados, e o dispositivo muda para a taxa de amostragem de cada faixa, então nada precisa reamostrar o áudio.",
    ),
    about_native_rate: SharedString::new_static(
        "O modo de taxa nativa faz o PipeWire operar o dispositivo na taxa de amostragem de cada faixa, então nada precisa reamostrar o áudio.",
    ),
    status_ok: SharedString::new_static(
        "No momento a reprodução é bit-perfect: o dispositivo recebe os dados do arquivo sem alterações.",
    ),
    status_issues: SharedString::new_static("O que está alterando o áudio agora:"),
    not_exclusive_desc: SharedString::new_static(
        "O áudio passa pelo mixer do sistema, onde é misturado com outros aplicativos e pode ser reamostrado.",
    ),
    system_volume_desc: SharedString::new_static(
        "O volume do sistema é aplicado depois que o Pawse entrega o áudio, então o som é abaixado antes de chegar ao DAC. Para resolver, coloque o volume do sistema em 100% e ajuste o nível em um DAC ou amplificador com controle de volume próprio.",
    ),
    system_volume_caution: SharedString::new_static(
        "Cuidado: em 100% o som pode ficar muito alto, principalmente com fones ligados direto no computador.",
    ),
    system_muted_desc: SharedString::new_static(
        "O dispositivo de saída está mudo, então o DAC recebe silêncio. Reative o som nas configurações de som do sistema.",
    ),
    sample_rate_desc: SharedString::new_static(
        "O dispositivo não está na taxa de amostragem da faixa, então o áudio não consegue chegar a ele sem alterações. Normalmente o dispositivo não suporta essa taxa, ou outro aplicativo a mudou.",
    ),
    bit_depth_desc: SharedString::new_static(
        "O Pawse envia o áudio ao dispositivo em ponto flutuante de 32 bits, que mantém 24 bits de precisão, então os bits mais baixos desta faixa são arredondados. Isso está muito abaixo de qualquer coisa audível.",
    ),
    no_source_desc: SharedString::new_static(
        "Nada está tocando ainda. Comece uma faixa e este ícone vai mostrar se ela chega ao dispositivo sem alterações.",
    ),
    not_an_error: SharedString::new_static(
        "Nada disso é um erro: a música toca normalmente. O ícone só mostra se o dispositivo recebe exatamente o que está no arquivo.",
    ),
    got_it: SharedString::new_static("Entendi"),
};

static RU: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect воспроизведение"),
    not_bit_perfect: SharedString::new_static("Не bit-perfect"),
    click_for_details: SharedString::new_static("Нажмите, чтобы узнать подробнее"),
    not_exclusive: SharedString::new_static("Эксклюзивный режим не активен"),
    native_rate_off: SharedString::new_static("Родная частота не активна"),
    system_volume_t: SharedString::new_static("Системная громкость {}%, а не 100%"),
    system_muted: SharedString::new_static("Системный звук выключен"),
    sample_rate_t: SharedString::new_static("Трек в {}, а устройство работает на {}"),
    bit_depth_t: SharedString::new_static("Трек {}-битный, а до устройства доходят 24 бита"),
    no_source: SharedString::new_static("Пока ничего не играет"),
    about_title: SharedString::new_static("Что такое bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect значит, что по пути от файла до ЦАП звук ничто не меняет. ЦАП — это микросхема, которая превращает цифровой звук в сигнал для наушников или колонок. Он получает ровно те данные, что записаны в файле.",
    ),
    about_exclusive: SharedString::new_static(
        "Эксклюзивный режим убирает почти всё, что может изменить звук: другие приложения не подмешиваются, а устройство переключается на частоту каждого трека, так что пересэмплировать звук не нужно.",
    ),
    about_native_rate: SharedString::new_static(
        "Режим родной частоты заставляет PipeWire запускать устройство на частоте каждого трека, так что пересэмплировать звук не нужно.",
    ),
    status_ok: SharedString::new_static(
        "Сейчас воспроизведение bit-perfect: устройство получает данные файла без изменений.",
    ),
    status_issues: SharedString::new_static("Что сейчас меняет звук:"),
    not_exclusive_desc: SharedString::new_static(
        "Звук идёт через системный микшер: там он смешивается с другими приложениями и может пересэмплироваться.",
    ),
    system_volume_desc: SharedString::new_static(
        "Системная громкость применяется уже после того, как Pawse отдал звук, поэтому он становится тише ещё до ЦАП. Чтобы это убрать, поставьте системную громкость на 100% и регулируйте громкость на ЦАП или усилителе со своей ручкой громкости.",
    ),
    system_volume_caution: SharedString::new_static(
        "Осторожно: на 100% звук может быть очень громким, особенно в наушниках, подключённых прямо к компьютеру.",
    ),
    system_muted_desc: SharedString::new_static(
        "Звук устройства вывода выключен в системе, поэтому ЦАП получает тишину. Включите звук в системных настройках.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Устройство работает не на частоте трека, поэтому звук не может дойти до него без изменений. Обычно устройство просто не поддерживает эту частоту, или её переключило другое приложение.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse передаёт звук устройству в 32-битном формате с плавающей точкой, а он хранит 24 бита точности, поэтому младшие биты этого трека округляются. Это намного ниже порога слышимости.",
    ),
    no_source_desc: SharedString::new_static(
        "Пока ничего не играет. Включите трек, и этот значок покажет, доходит ли он до устройства без изменений.",
    ),
    not_an_error: SharedString::new_static(
        "Всё это не ошибка: музыка играет как обычно. Значок лишь показывает, получает ли устройство в точности то, что записано в файле.",
    ),
    got_it: SharedString::new_static("Понятно"),
};

static JA: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("ビットパーフェクト再生"),
    not_bit_perfect: SharedString::new_static("ビットパーフェクトではありません"),
    click_for_details: SharedString::new_static("クリックで詳細を表示"),
    not_exclusive: SharedString::new_static("排他モードが有効になっていません"),
    native_rate_off: SharedString::new_static("ネイティブサンプルレートが有効になっていません"),
    system_volume_t: SharedString::new_static("システム音量が 100% ではなく {}% です"),
    system_muted: SharedString::new_static("システムの音声がミュートされています"),
    sample_rate_t: SharedString::new_static("トラックは {} ですが、デバイスは {} で動作しています"),
    bit_depth_t: SharedString::new_static(
        "トラックは {} ビットですが、デバイスに届くのは 24 ビットです",
    ),
    no_source: SharedString::new_static("まだ何も再生されていません"),
    about_title: SharedString::new_static("ビットパーフェクトとは？"),
    about: SharedString::new_static(
        "ビットパーフェクトとは、ファイルから DAC までの間で音声が一切変更されないことです。DAC はデジタル音声をヘッドホンやスピーカー用の信号に変換するチップで、ファイルに記録されたデータそのものを受け取ります。",
    ),
    about_exclusive: SharedString::new_static(
        "排他モードは音声を変えうる要素のほとんどを取り除きます。ほかのアプリの音は混ざらず、デバイスはトラックごとのサンプルレートに切り替わるため、リサンプリングの必要がありません。",
    ),
    about_native_rate: SharedString::new_static(
        "ネイティブサンプルレートモードでは、PipeWire がトラックごとのサンプルレートでデバイスを動作させるため、リサンプリングの必要がありません。",
    ),
    status_ok: SharedString::new_static(
        "現在の再生はビットパーフェクトです。デバイスはファイルのデータを変更なしで受け取っています。",
    ),
    status_issues: SharedString::new_static("現在、音声を変えているもの："),
    not_exclusive_desc: SharedString::new_static(
        "音声はシステムのミキサーを通り、そこでほかのアプリの音と混ぜられ、リサンプリングされることもあります。",
    ),
    system_volume_desc: SharedString::new_static(
        "システム音量は Pawse が音声を渡したあとに適用されるため、DAC に届く前に音が小さくされます。これを解消するには、システム音量を 100% にして、独自の音量ノブを持つ DAC やアンプで音量を調整してください。",
    ),
    system_volume_caution: SharedString::new_static(
        "ご注意ください：100% では、特にコンピューターに直接つないだヘッドホンで、音が非常に大きくなることがあります。",
    ),
    system_muted_desc: SharedString::new_static(
        "出力デバイスがミュートされているため、DAC には無音が届いています。システムのサウンド設定でミュートを解除してください。",
    ),
    sample_rate_desc: SharedString::new_static(
        "デバイスがトラックのサンプルレートで動作していないため、音声は変更なしでは届きません。多くの場合、デバイスがそのレートに対応していないか、別のアプリがレートを切り替えています。",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse は音声を 32 ビット浮動小数点でデバイスに送ります。この形式の精度は 24 ビットなので、このトラックの下位ビットは丸められます。これは聞き取れるレベルをはるかに下回ります。",
    ),
    no_source_desc: SharedString::new_static(
        "まだ何も再生されていません。トラックを再生すると、このアイコンがデバイスに変更なしで届いているかを表示します。",
    ),
    not_an_error: SharedString::new_static(
        "これらはいずれもエラーではなく、音楽は普通に再生されています。このアイコンは、デバイスがファイルの内容をそのまま受け取っているかどうかを示すだけです。",
    ),
    got_it: SharedString::new_static("OK"),
};

static DE: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfekte Wiedergabe"),
    not_bit_perfect: SharedString::new_static("Nicht bit-perfekt"),
    click_for_details: SharedString::new_static("Klicken für Details"),
    not_exclusive: SharedString::new_static("Exklusivmodus ist nicht aktiv"),
    native_rate_off: SharedString::new_static("Native Abtastrate ist nicht aktiv"),
    system_volume_t: SharedString::new_static("Systemlautstärke steht auf {} %, nicht auf 100 %"),
    system_muted: SharedString::new_static("Systemton ist stummgeschaltet"),
    sample_rate_t: SharedString::new_static("Der Titel hat {}, das Gerät läuft mit {}"),
    bit_depth_t: SharedString::new_static("Der Titel hat {} Bit, beim Gerät kommen 24 Bit an"),
    no_source: SharedString::new_static("Es wird noch nichts abgespielt"),
    about_title: SharedString::new_static("Was ist bit-perfekt?"),
    about: SharedString::new_static(
        "Bit-perfekt heißt: Nichts verändert das Audio auf dem Weg von der Datei zum DAC, dem Chip, der digitales Audio in das Signal für Kopfhörer oder Lautsprecher umwandelt. Der DAC bekommt genau die Daten, die in der Datei stehen.",
    ),
    about_exclusive: SharedString::new_static(
        "Der Exklusivmodus entfernt das meiste, was es verändern könnte: Andere Apps werden nicht dazugemischt, und das Gerät wechselt auf die Abtastrate jedes Titels, sodass nichts das Audio umrechnen muss.",
    ),
    about_native_rate: SharedString::new_static(
        "Im Modus „Native Abtastrate“ betreibt PipeWire das Gerät mit der Abtastrate jedes Titels, sodass nichts das Audio umrechnen muss.",
    ),
    status_ok: SharedString::new_static(
        "Gerade ist die Wiedergabe bit-perfekt: Das Gerät bekommt die Daten der Datei unverändert.",
    ),
    status_issues: SharedString::new_static("Was das Audio gerade verändert:"),
    not_exclusive_desc: SharedString::new_static(
        "Das Audio läuft über den Systemmixer, wird dort mit anderen Apps gemischt und eventuell umgerechnet.",
    ),
    system_volume_desc: SharedString::new_static(
        "Die Systemlautstärke wird angewendet, nachdem Pawse das Audio übergeben hat, also wird der Ton leiser gemacht, bevor er den DAC erreicht. Um das zu beheben, stell die Systemlautstärke auf 100 % und regle die Lautstärke an einem DAC oder Verstärker mit eigenem Lautstärkeregler.",
    ),
    system_volume_caution: SharedString::new_static(
        "Vorsicht: Bei 100 % kann es sehr laut werden, besonders mit Kopfhörern direkt am Computer.",
    ),
    system_muted_desc: SharedString::new_static(
        "Das Ausgabegerät ist stummgeschaltet, also bekommt der DAC Stille. Heb die Stummschaltung in den Toneinstellungen des Systems auf.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Das Gerät läuft nicht mit der Abtastrate des Titels, daher kann das Audio nicht unverändert ankommen. Meist unterstützt das Gerät diese Rate nicht, oder eine andere App hat sie umgestellt.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse schickt das Audio als 32-Bit-Gleitkomma an das Gerät, das 24 Bit Genauigkeit behält, daher werden die untersten Bits dieses Titels gerundet. Das liegt weit unter allem, was man hören kann.",
    ),
    no_source_desc: SharedString::new_static(
        "Es wird noch nichts abgespielt. Starte einen Titel, dann zeigt dieses Symbol, ob er unverändert beim Gerät ankommt.",
    ),
    not_an_error: SharedString::new_static(
        "Nichts davon ist ein Fehler: Die Musik spielt ganz normal. Das Symbol zeigt nur, ob das Gerät genau das bekommt, was in der Datei steht.",
    ),
    got_it: SharedString::new_static("Verstanden"),
};

static FR: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Lecture bit-perfect"),
    not_bit_perfect: SharedString::new_static("Pas bit-perfect"),
    click_for_details: SharedString::new_static("Cliquez pour en savoir plus"),
    not_exclusive: SharedString::new_static("Le mode exclusif n'est pas actif"),
    native_rate_off: SharedString::new_static("La fréquence native n'est pas active"),
    system_volume_t: SharedString::new_static("Le volume système est à {} %, pas à 100 %"),
    system_muted: SharedString::new_static("Le son du système est coupé"),
    sample_rate_t: SharedString::new_static("La piste est en {}, mais le périphérique tourne à {}"),
    bit_depth_t: SharedString::new_static("La piste est en {} bits, le périphérique en reçoit 24"),
    no_source: SharedString::new_static("Rien n'est encore en lecture"),
    about_title: SharedString::new_static("Qu'est-ce que le bit-perfect ?"),
    about: SharedString::new_static(
        "Bit-perfect signifie que rien ne modifie le son entre le fichier et le DAC, la puce qui transforme l'audio numérique en signal pour vos écouteurs ou vos enceintes. Le DAC reçoit exactement les données enregistrées dans le fichier.",
    ),
    about_exclusive: SharedString::new_static(
        "Le mode exclusif supprime presque tout ce qui pourrait le modifier : les autres applications ne sont pas mélangées au son, et le périphérique passe à la fréquence d'échantillonnage de chaque piste, donc rien n'a besoin de rééchantillonner l'audio.",
    ),
    about_native_rate: SharedString::new_static(
        "En mode fréquence native, PipeWire fait tourner le périphérique à la fréquence d'échantillonnage de chaque piste, donc rien n'a besoin de rééchantillonner l'audio.",
    ),
    status_ok: SharedString::new_static(
        "En ce moment, la lecture est bit-perfect : le périphérique reçoit les données du fichier sans modification.",
    ),
    status_issues: SharedString::new_static("Ce qui modifie le son en ce moment :"),
    not_exclusive_desc: SharedString::new_static(
        "Le son passe par le mélangeur du système, où il est mélangé avec les autres applications et peut être rééchantillonné.",
    ),
    system_volume_desc: SharedString::new_static(
        "Le volume système est appliqué après que Pawse a transmis le son, donc le son est baissé avant d'atteindre le DAC. Pour régler ce point, mettez le volume système à 100 % et réglez le niveau sur un DAC ou un amplificateur doté de son propre bouton de volume.",
    ),
    system_volume_caution: SharedString::new_static(
        "Attention : à 100 %, le son peut être très fort, surtout avec des écouteurs branchés directement sur l'ordinateur.",
    ),
    system_muted_desc: SharedString::new_static(
        "Le périphérique de sortie est en sourdine, donc le DAC ne reçoit que du silence. Réactivez le son dans les réglages audio du système.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Le périphérique ne tourne pas à la fréquence d'échantillonnage de la piste, donc le son ne peut pas lui parvenir sans modification. En général, le périphérique ne prend pas en charge cette fréquence, ou une autre application l'a changée.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse envoie l'audio au périphérique en virgule flottante 32 bits, qui conserve 24 bits de précision : les bits les plus faibles de cette piste sont donc arrondis. C'est bien en dessous de tout ce qui est audible.",
    ),
    no_source_desc: SharedString::new_static(
        "Rien n'est encore en lecture. Lancez une piste, et cette icône indiquera si elle parvient au périphérique sans modification.",
    ),
    not_an_error: SharedString::new_static(
        "Rien de tout cela n'est une erreur : la musique est lue normalement. L'icône indique seulement si le périphérique reçoit exactement ce que contient le fichier.",
    ),
    got_it: SharedString::new_static("Compris"),
};

static KO: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("비트 퍼펙트 재생"),
    not_bit_perfect: SharedString::new_static("비트 퍼펙트 아님"),
    click_for_details: SharedString::new_static("클릭하여 자세히 보기"),
    not_exclusive: SharedString::new_static("독점 모드가 활성화되지 않았습니다"),
    native_rate_off: SharedString::new_static("네이티브 샘플 레이트가 활성화되지 않았습니다"),
    system_volume_t: SharedString::new_static("시스템 볼륨이 100%가 아닌 {}%입니다"),
    system_muted: SharedString::new_static("시스템 소리가 음소거되어 있습니다"),
    sample_rate_t: SharedString::new_static("트랙은 {}인데 장치는 {}로 동작 중입니다"),
    bit_depth_t: SharedString::new_static("트랙은 {}비트인데 장치에는 24비트만 전달됩니다"),
    no_source: SharedString::new_static("아직 재생 중인 곡이 없습니다"),
    about_title: SharedString::new_static("비트 퍼펙트란?"),
    about: SharedString::new_static(
        "비트 퍼펙트란 파일에서 DAC까지 가는 동안 오디오가 전혀 바뀌지 않는다는 뜻입니다. DAC는 디지털 오디오를 헤드폰이나 스피커용 신호로 바꾸는 칩으로, 파일에 저장된 데이터를 그대로 받습니다.",
    ),
    about_exclusive: SharedString::new_static(
        "독점 모드는 오디오를 바꿀 수 있는 요소를 대부분 없앱니다. 다른 앱의 소리가 섞이지 않고, 장치가 트랙마다 샘플 레이트를 바꾸므로 리샘플링할 필요가 없습니다.",
    ),
    about_native_rate: SharedString::new_static(
        "네이티브 샘플 레이트 모드에서는 PipeWire가 트랙마다 해당 샘플 레이트로 장치를 동작시키므로 리샘플링할 필요가 없습니다.",
    ),
    status_ok: SharedString::new_static(
        "지금은 비트 퍼펙트로 재생 중입니다. 장치가 파일의 데이터를 그대로 받고 있습니다.",
    ),
    status_issues: SharedString::new_static("지금 오디오를 바꾸고 있는 요인:"),
    not_exclusive_desc: SharedString::new_static(
        "오디오가 시스템 믹서를 거치며, 그곳에서 다른 앱의 소리와 섞이고 리샘플링될 수 있습니다.",
    ),
    system_volume_desc: SharedString::new_static(
        "시스템 볼륨은 Pawse가 오디오를 넘긴 뒤에 적용되므로, 소리가 DAC에 도달하기 전에 작아집니다. 이를 해결하려면 시스템 볼륨을 100%로 두고, 자체 볼륨 노브가 있는 DAC나 앰프로 음량을 조절하세요.",
    ),
    system_volume_caution: SharedString::new_static(
        "주의하세요: 100%에서는 특히 컴퓨터에 바로 연결한 헤드폰에서 소리가 매우 클 수 있습니다.",
    ),
    system_muted_desc: SharedString::new_static(
        "출력 장치가 음소거되어 있어 DAC에 무음이 전달됩니다. 시스템 사운드 설정에서 음소거를 해제하세요.",
    ),
    sample_rate_desc: SharedString::new_static(
        "장치가 트랙의 샘플 레이트로 동작하지 않아 오디오가 그대로 전달될 수 없습니다. 보통 장치가 해당 샘플 레이트를 지원하지 않거나 다른 앱이 바꾼 경우입니다.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse는 오디오를 32비트 부동소수점으로 장치에 보내며, 이 형식은 24비트 정밀도를 유지하므로 이 트랙의 하위 비트는 반올림됩니다. 이는 들을 수 있는 수준보다 훨씬 낮습니다.",
    ),
    no_source_desc: SharedString::new_static(
        "아직 재생 중인 곡이 없습니다. 트랙을 재생하면 이 아이콘이 장치에 그대로 전달되는지 보여 줍니다.",
    ),
    not_an_error: SharedString::new_static(
        "이 중 어느 것도 오류가 아닙니다. 음악은 정상적으로 재생됩니다. 이 아이콘은 장치가 파일 내용을 그대로 받는지만 보여 줍니다.",
    ),
    got_it: SharedString::new_static("확인"),
};

static IT: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Riproduzione bit-perfect"),
    not_bit_perfect: SharedString::new_static("Non bit-perfect"),
    click_for_details: SharedString::new_static("Clicca per i dettagli"),
    not_exclusive: SharedString::new_static("La modalità esclusiva non è attiva"),
    native_rate_off: SharedString::new_static("La frequenza nativa non è attiva"),
    system_volume_t: SharedString::new_static("Il volume di sistema è al {}%, non al 100%"),
    system_muted: SharedString::new_static("L'audio di sistema è disattivato"),
    sample_rate_t: SharedString::new_static("La traccia è a {}, ma il dispositivo funziona a {}"),
    bit_depth_t: SharedString::new_static("La traccia è a {} bit, al dispositivo arrivano 24 bit"),
    no_source: SharedString::new_static("Non è ancora in riproduzione nulla"),
    about_title: SharedString::new_static("Che cos'è il bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect significa che niente modifica l'audio nel percorso dal file al DAC, il chip che trasforma l'audio digitale nel segnale per le cuffie o gli altoparlanti. Il DAC riceve esattamente i dati salvati nel file.",
    ),
    about_exclusive: SharedString::new_static(
        "La modalità esclusiva elimina quasi tutto ciò che potrebbe modificarlo: le altre app non vengono mixate e il dispositivo passa alla frequenza di campionamento di ogni traccia, così niente deve ricampionare l'audio.",
    ),
    about_native_rate: SharedString::new_static(
        "Con la modalità frequenza nativa PipeWire fa funzionare il dispositivo alla frequenza di campionamento di ogni traccia, così niente deve ricampionare l'audio.",
    ),
    status_ok: SharedString::new_static(
        "In questo momento la riproduzione è bit-perfect: il dispositivo riceve i dati del file senza modifiche.",
    ),
    status_issues: SharedString::new_static("Cosa modifica l'audio in questo momento:"),
    not_exclusive_desc: SharedString::new_static(
        "L'audio passa dal mixer di sistema, dove viene mixato con le altre app e può essere ricampionato.",
    ),
    system_volume_desc: SharedString::new_static(
        "Il volume di sistema viene applicato dopo che Pawse ha consegnato l'audio, quindi il suono viene abbassato prima di raggiungere il DAC. Per risolvere, porta il volume di sistema al 100% e regola il livello su un DAC o un amplificatore con una propria manopola del volume.",
    ),
    system_volume_caution: SharedString::new_static(
        "Attenzione: al 100% il suono può essere molto forte, soprattutto con cuffie collegate direttamente al computer.",
    ),
    system_muted_desc: SharedString::new_static(
        "L'audio del dispositivo di uscita è disattivato, quindi il DAC riceve silenzio. Riattivalo nelle impostazioni audio del sistema.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Il dispositivo non funziona alla frequenza di campionamento della traccia, quindi l'audio non può arrivarci senza modifiche. Di solito il dispositivo non supporta questa frequenza, oppure un'altra app l'ha cambiata.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse invia l'audio al dispositivo in virgola mobile a 32 bit, che conserva 24 bit di precisione, quindi i bit più bassi di questa traccia vengono arrotondati. È molto al di sotto di qualsiasi cosa udibile.",
    ),
    no_source_desc: SharedString::new_static(
        "Non è ancora in riproduzione nulla. Avvia una traccia e questa icona mostrerà se arriva al dispositivo senza modifiche.",
    ),
    not_an_error: SharedString::new_static(
        "Niente di tutto questo è un errore: la musica suona normalmente. L'icona indica solo se il dispositivo riceve esattamente ciò che c'è nel file.",
    ),
    got_it: SharedString::new_static("Capito"),
};

static TR: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect oynatma"),
    not_bit_perfect: SharedString::new_static("Bit-perfect değil"),
    click_for_details: SharedString::new_static("Ayrıntılar için tıklayın"),
    not_exclusive: SharedString::new_static("Özel mod etkin değil"),
    native_rate_off: SharedString::new_static("Doğal örnekleme hızı etkin değil"),
    system_volume_t: SharedString::new_static("Sistem ses düzeyi %{}, %100 değil"),
    system_muted: SharedString::new_static("Sistem sesi kapalı"),
    sample_rate_t: SharedString::new_static("Parça {}, ama aygıt {} hızında çalışıyor"),
    bit_depth_t: SharedString::new_static("Parça {} bit, aygıta 24 bit ulaşıyor"),
    no_source: SharedString::new_static("Henüz hiçbir şey çalmıyor"),
    about_title: SharedString::new_static("Bit-perfect nedir?"),
    about: SharedString::new_static(
        "Bit-perfect, dosyadan DAC'a giden yolda sesi hiçbir şeyin değiştirmemesi demektir. DAC, dijital sesi kulaklık veya hoparlörleriniz için sinyale dönüştüren çiptir ve dosyada kayıtlı verilerin tam olarak aynısını alır.",
    ),
    about_exclusive: SharedString::new_static(
        "Özel mod, sesi değiştirebilecek şeylerin çoğunu ortadan kaldırır: diğer uygulamalar karıştırılmaz ve aygıt her parçanın örnekleme hızına geçer, böylece sesi yeniden örneklemek gerekmez.",
    ),
    about_native_rate: SharedString::new_static(
        "Doğal örnekleme hızı modu, PipeWire'ın aygıtı her parçanın örnekleme hızında çalıştırmasını sağlar, böylece sesi yeniden örneklemek gerekmez.",
    ),
    status_ok: SharedString::new_static(
        "Şu anda oynatma bit-perfect: aygıt dosyadaki verileri değiştirilmeden alıyor.",
    ),
    status_issues: SharedString::new_static("Şu anda sesi değiştirenler:"),
    not_exclusive_desc: SharedString::new_static(
        "Ses sistem karıştırıcısından geçiyor; orada diğer uygulamalarla karıştırılıyor ve yeniden örneklenebiliyor.",
    ),
    system_volume_desc: SharedString::new_static(
        "Sistem ses düzeyi, Pawse sesi teslim ettikten sonra uygulanır; bu yüzden ses DAC'a ulaşmadan önce kısılır. Bunu gidermek için sistem ses düzeyini %100'e getirin ve sesi kendi ses düğmesi olan bir DAC veya amfiden ayarlayın.",
    ),
    system_volume_caution: SharedString::new_static(
        "Dikkat: %100'de ses çok yüksek olabilir, özellikle doğrudan bilgisayara takılı kulaklıklarda.",
    ),
    system_muted_desc: SharedString::new_static(
        "Çıkış aygıtının sesi kapalı, bu yüzden DAC sessizlik alıyor. Sistem ses ayarlarından sesi açın.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Aygıt parçanın örnekleme hızında çalışmıyor, bu yüzden ses ona değişmeden ulaşamıyor. Genellikle aygıt bu hızı desteklemiyordur ya da başka bir uygulama hızı değiştirmiştir.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse sesi aygıta 32 bit kayan noktalı olarak gönderir; bu biçim 24 bit hassasiyet korur, bu yüzden bu parçanın en düşük bitleri yuvarlanır. Bu, duyulabilecek her şeyin çok altındadır.",
    ),
    no_source_desc: SharedString::new_static(
        "Henüz hiçbir şey çalmıyor. Bir parça başlatın, bu simge onun aygıta değişmeden ulaşıp ulaşmadığını gösterecek.",
    ),
    not_an_error: SharedString::new_static(
        "Bunların hiçbiri hata değil: müzik normal şekilde çalıyor. Simge yalnızca aygıtın dosyadakinin tam aynısını alıp almadığını gösterir.",
    ),
    got_it: SharedString::new_static("Anladım"),
};

static PL: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Odtwarzanie bit-perfect"),
    not_bit_perfect: SharedString::new_static("Nie bit-perfect"),
    click_for_details: SharedString::new_static("Kliknij, aby zobaczyć szczegóły"),
    not_exclusive: SharedString::new_static("Tryb wyłączny nie jest aktywny"),
    native_rate_off: SharedString::new_static("Natywna częstotliwość nie jest aktywna"),
    system_volume_t: SharedString::new_static("Głośność systemu wynosi {}%, a nie 100%"),
    system_muted: SharedString::new_static("Dźwięk systemu jest wyciszony"),
    sample_rate_t: SharedString::new_static("Utwór ma {}, a urządzenie pracuje z {}"),
    bit_depth_t: SharedString::new_static("Utwór jest {}-bitowy, do urządzenia trafiają 24 bity"),
    no_source: SharedString::new_static("Jeszcze nic nie gra"),
    about_title: SharedString::new_static("Co to jest bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect oznacza, że nic nie zmienia dźwięku w drodze z pliku do przetwornika DAC, czyli układu, który zamienia cyfrowy dźwięk na sygnał dla słuchawek lub głośników. DAC dostaje dokładnie te dane, które są zapisane w pliku.",
    ),
    about_exclusive: SharedString::new_static(
        "Tryb wyłączny usuwa prawie wszystko, co mogłoby go zmienić: inne aplikacje nie są domiksowywane, a urządzenie przełącza się na częstotliwość próbkowania każdego utworu, więc nic nie musi przepróbkowywać dźwięku.",
    ),
    about_native_rate: SharedString::new_static(
        "Tryb natywnej częstotliwości sprawia, że PipeWire uruchamia urządzenie z częstotliwością próbkowania każdego utworu, więc nic nie musi przepróbkowywać dźwięku.",
    ),
    status_ok: SharedString::new_static(
        "W tej chwili odtwarzanie jest bit-perfect: urządzenie dostaje dane z pliku bez zmian.",
    ),
    status_issues: SharedString::new_static("Co w tej chwili zmienia dźwięk:"),
    not_exclusive_desc: SharedString::new_static(
        "Dźwięk przechodzi przez systemowy mikser, gdzie jest miksowany z innymi aplikacjami i może być przepróbkowany.",
    ),
    system_volume_desc: SharedString::new_static(
        "Głośność systemu jest stosowana już po tym, jak Pawse przekaże dźwięk, więc dźwięk jest ściszany, zanim dotrze do DAC. Aby to usunąć, ustaw głośność systemu na 100% i reguluj głośność na przetworniku DAC lub wzmacniaczu z własnym pokrętłem głośności.",
    ),
    system_volume_caution: SharedString::new_static(
        "Uwaga: przy 100% dźwięk może być bardzo głośny, zwłaszcza w słuchawkach podłączonych bezpośrednio do komputera.",
    ),
    system_muted_desc: SharedString::new_static(
        "Urządzenie wyjściowe jest wyciszone, więc DAC dostaje ciszę. Wyłącz wyciszenie w systemowych ustawieniach dźwięku.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Urządzenie nie pracuje z częstotliwością próbkowania utworu, więc dźwięk nie może do niego dotrzeć bez zmian. Zwykle urządzenie nie obsługuje tej częstotliwości albo zmieniła ją inna aplikacja.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse wysyła dźwięk do urządzenia jako 32-bitowe liczby zmiennoprzecinkowe, które zachowują 24 bity precyzji, więc najmłodsze bity tego utworu są zaokrąglane. To daleko poniżej progu słyszalności.",
    ),
    no_source_desc: SharedString::new_static(
        "Jeszcze nic nie gra. Włącz utwór, a ta ikona pokaże, czy dociera do urządzenia bez zmian.",
    ),
    not_an_error: SharedString::new_static(
        "Nic z tego nie jest błędem: muzyka gra normalnie. Ikona pokazuje tylko, czy urządzenie dostaje dokładnie to, co jest w pliku.",
    ),
    got_it: SharedString::new_static("Rozumiem"),
};

static NL: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect afspelen"),
    not_bit_perfect: SharedString::new_static("Niet bit-perfect"),
    click_for_details: SharedString::new_static("Klik voor details"),
    not_exclusive: SharedString::new_static("Exclusieve modus is niet actief"),
    native_rate_off: SharedString::new_static("Native samplefrequentie is niet actief"),
    system_volume_t: SharedString::new_static("Systeemvolume staat op {}%, niet op 100%"),
    system_muted: SharedString::new_static("Systeemgeluid is gedempt"),
    sample_rate_t: SharedString::new_static("Het nummer is {}, maar het apparaat draait op {}"),
    bit_depth_t: SharedString::new_static("Het nummer is {}-bit, het apparaat krijgt 24 bit"),
    no_source: SharedString::new_static("Er speelt nog niets"),
    about_title: SharedString::new_static("Wat is bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect betekent dat niets het geluid verandert op weg van het bestand naar de DAC, de chip die digitale audio omzet in het signaal voor je koptelefoon of speakers. De DAC krijgt precies de gegevens die in het bestand staan.",
    ),
    about_exclusive: SharedString::new_static(
        "Exclusieve modus haalt bijna alles weg wat het kan veranderen: andere apps worden niet bijgemengd en het apparaat schakelt naar de samplefrequentie van elk nummer, zodat niets het geluid hoeft te resamplen.",
    ),
    about_native_rate: SharedString::new_static(
        "In de modus native samplefrequentie laat PipeWire het apparaat op de samplefrequentie van elk nummer draaien, zodat niets het geluid hoeft te resamplen.",
    ),
    status_ok: SharedString::new_static(
        "Op dit moment is het afspelen bit-perfect: het apparaat krijgt de gegevens van het bestand ongewijzigd.",
    ),
    status_issues: SharedString::new_static("Wat het geluid op dit moment verandert:"),
    not_exclusive_desc: SharedString::new_static(
        "Het geluid gaat via de systeemmixer, waar het met andere apps wordt gemengd en mogelijk wordt geresampled.",
    ),
    system_volume_desc: SharedString::new_static(
        "Het systeemvolume wordt toegepast nadat Pawse het geluid heeft doorgegeven, dus het geluid wordt zachter gezet voordat het de DAC bereikt. Om dit op te lossen zet je het systeemvolume op 100% en regel je het volume op een DAC of versterker met een eigen volumeknop.",
    ),
    system_volume_caution: SharedString::new_static(
        "Let op: op 100% kan het geluid erg hard zijn, vooral met een koptelefoon direct op de computer.",
    ),
    system_muted_desc: SharedString::new_static(
        "Het uitvoerapparaat is gedempt, dus de DAC krijgt stilte. Zet het geluid weer aan in de geluidsinstellingen van het systeem.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Het apparaat draait niet op de samplefrequentie van het nummer, dus het geluid kan het niet ongewijzigd bereiken. Meestal ondersteunt het apparaat deze frequentie niet, of heeft een andere app hem omgezet.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse stuurt het geluid naar het apparaat als 32-bit floating point, dat 24 bit precisie behoudt, dus de laagste bits van dit nummer worden afgerond. Dat ligt ver onder alles wat hoorbaar is.",
    ),
    no_source_desc: SharedString::new_static(
        "Er speelt nog niets. Start een nummer, dan laat dit pictogram zien of het het apparaat ongewijzigd bereikt.",
    ),
    not_an_error: SharedString::new_static(
        "Niets hiervan is een fout: de muziek speelt gewoon. Het pictogram laat alleen zien of het apparaat precies krijgt wat er in het bestand staat.",
    ),
    got_it: SharedString::new_static("Begrepen"),
};

static UK: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect відтворення"),
    not_bit_perfect: SharedString::new_static("Не bit-perfect"),
    click_for_details: SharedString::new_static("Натисніть, щоб дізнатися більше"),
    not_exclusive: SharedString::new_static("Ексклюзивний режим не активний"),
    native_rate_off: SharedString::new_static("Рідна частота не активна"),
    system_volume_t: SharedString::new_static("Системна гучність {}%, а не 100%"),
    system_muted: SharedString::new_static("Системний звук вимкнено"),
    sample_rate_t: SharedString::new_static("Трек у {}, а пристрій працює на {}"),
    bit_depth_t: SharedString::new_static("Трек {}-бітний, а до пристрою доходять 24 біти"),
    no_source: SharedString::new_static("Поки нічого не грає"),
    about_title: SharedString::new_static("Що таке bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect означає, що на шляху від файлу до ЦАП звук ніщо не змінює. ЦАП — це мікросхема, яка перетворює цифровий звук на сигнал для навушників або колонок. Він отримує саме ті дані, що записані у файлі.",
    ),
    about_exclusive: SharedString::new_static(
        "Ексклюзивний режим прибирає майже все, що може змінити звук: інші застосунки не домішуються, а пристрій перемикається на частоту кожного треку, тож передискретизувати звук не потрібно.",
    ),
    about_native_rate: SharedString::new_static(
        "Режим рідної частоти змушує PipeWire запускати пристрій на частоті кожного треку, тож передискретизувати звук не потрібно.",
    ),
    status_ok: SharedString::new_static(
        "Зараз відтворення bit-perfect: пристрій отримує дані файлу без змін.",
    ),
    status_issues: SharedString::new_static("Що зараз змінює звук:"),
    not_exclusive_desc: SharedString::new_static(
        "Звук іде через системний мікшер: там він змішується з іншими застосунками й може передискретизуватися.",
    ),
    system_volume_desc: SharedString::new_static(
        "Системна гучність застосовується вже після того, як Pawse віддав звук, тому він стає тихішим ще до ЦАП. Щоб це прибрати, встановіть системну гучність на 100% і регулюйте гучність на ЦАП або підсилювачі з власною ручкою гучності.",
    ),
    system_volume_caution: SharedString::new_static(
        "Обережно: на 100% звук може бути дуже гучним, особливо в навушниках, під'єднаних просто до комп'ютера.",
    ),
    system_muted_desc: SharedString::new_static(
        "Звук пристрою виводу вимкнено в системі, тому ЦАП отримує тишу. Увімкніть звук у системних налаштуваннях.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Пристрій працює не на частоті треку, тому звук не може дійти до нього без змін. Зазвичай пристрій просто не підтримує цю частоту, або її перемкнув інший застосунок.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse передає звук пристрою у 32-бітному форматі з рухомою комою, який зберігає 24 біти точності, тому молодші біти цього треку округлюються. Це набагато нижче порогу чутності.",
    ),
    no_source_desc: SharedString::new_static(
        "Поки нічого не грає. Увімкніть трек, і цей значок покаже, чи доходить він до пристрою без змін.",
    ),
    not_an_error: SharedString::new_static(
        "Усе це не помилка: музика грає як зазвичай. Значок лише показує, чи отримує пристрій саме те, що записано у файлі.",
    ),
    got_it: SharedString::new_static("Зрозуміло"),
};

static VI: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Phát bit-perfect"),
    not_bit_perfect: SharedString::new_static("Không bit-perfect"),
    click_for_details: SharedString::new_static("Nhấp để xem chi tiết"),
    not_exclusive: SharedString::new_static("Chế độ độc quyền chưa hoạt động"),
    native_rate_off: SharedString::new_static("Tần số lấy mẫu gốc chưa hoạt động"),
    system_volume_t: SharedString::new_static("Âm lượng hệ thống đang ở {}%, không phải 100%"),
    system_muted: SharedString::new_static("Âm thanh hệ thống đang bị tắt"),
    sample_rate_t: SharedString::new_static("Bản nhạc là {}, nhưng thiết bị đang chạy ở {}"),
    bit_depth_t: SharedString::new_static("Bản nhạc là {} bit, thiết bị chỉ nhận 24 bit"),
    no_source: SharedString::new_static("Chưa phát gì cả"),
    about_title: SharedString::new_static("Bit-perfect là gì?"),
    about: SharedString::new_static(
        "Bit-perfect nghĩa là không có gì thay đổi âm thanh trên đường từ tệp đến DAC, con chip biến âm thanh số thành tín hiệu cho tai nghe hoặc loa của bạn. DAC nhận đúng dữ liệu được lưu trong tệp.",
    ),
    about_exclusive: SharedString::new_static(
        "Chế độ độc quyền loại bỏ hầu hết những gì có thể thay đổi âm thanh: âm thanh của ứng dụng khác không bị trộn vào, và thiết bị chuyển sang tần số lấy mẫu của từng bản nhạc, nên không cần lấy mẫu lại.",
    ),
    about_native_rate: SharedString::new_static(
        "Chế độ tần số lấy mẫu gốc khiến PipeWire chạy thiết bị ở tần số lấy mẫu của từng bản nhạc, nên không cần lấy mẫu lại.",
    ),
    status_ok: SharedString::new_static(
        "Hiện tại phát bit-perfect: thiết bị nhận dữ liệu của tệp mà không bị thay đổi.",
    ),
    status_issues: SharedString::new_static("Những gì đang thay đổi âm thanh lúc này:"),
    not_exclusive_desc: SharedString::new_static(
        "Âm thanh đi qua bộ trộn của hệ thống, nơi nó được trộn với các ứng dụng khác và có thể bị lấy mẫu lại.",
    ),
    system_volume_desc: SharedString::new_static(
        "Âm lượng hệ thống được áp dụng sau khi Pawse chuyển âm thanh đi, nên âm thanh bị giảm nhỏ trước khi đến DAC. Để khắc phục, hãy đặt âm lượng hệ thống ở 100% và chỉnh âm lượng trên DAC hoặc bộ khuếch đại có núm âm lượng riêng.",
    ),
    system_volume_caution: SharedString::new_static(
        "Hãy cẩn thận: ở mức 100%, âm thanh có thể rất to, nhất là với tai nghe cắm thẳng vào máy tính.",
    ),
    system_muted_desc: SharedString::new_static(
        "Thiết bị đầu ra đang bị tắt tiếng, nên DAC chỉ nhận được im lặng. Hãy bật lại âm thanh trong cài đặt âm thanh của hệ thống.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Thiết bị không chạy ở tần số lấy mẫu của bản nhạc, nên âm thanh không thể đến thiết bị mà không bị thay đổi. Thường là thiết bị không hỗ trợ tần số này, hoặc một ứng dụng khác đã chuyển nó.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse gửi âm thanh đến thiết bị dưới dạng dấu phẩy động 32 bit, giữ được độ chính xác 24 bit, nên các bit thấp nhất của bản nhạc này bị làm tròn. Mức đó thấp hơn rất nhiều so với những gì tai người nghe được.",
    ),
    no_source_desc: SharedString::new_static(
        "Chưa phát gì cả. Hãy phát một bản nhạc, biểu tượng này sẽ cho biết nó có đến thiết bị mà không bị thay đổi hay không.",
    ),
    not_an_error: SharedString::new_static(
        "Không có điều nào ở trên là lỗi: nhạc vẫn phát bình thường. Biểu tượng chỉ cho biết thiết bị có nhận đúng y nguyên nội dung trong tệp hay không.",
    ),
    got_it: SharedString::new_static("Đã hiểu"),
};

static ID: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Pemutaran bit-perfect"),
    not_bit_perfect: SharedString::new_static("Bukan bit-perfect"),
    click_for_details: SharedString::new_static("Klik untuk detail"),
    not_exclusive: SharedString::new_static("Mode eksklusif tidak aktif"),
    native_rate_off: SharedString::new_static("Laju sampel asli tidak aktif"),
    system_volume_t: SharedString::new_static("Volume sistem {}%, bukan 100%"),
    system_muted: SharedString::new_static("Suara sistem dibisukan"),
    sample_rate_t: SharedString::new_static("Lagu ini {}, tetapi perangkat berjalan di {}"),
    bit_depth_t: SharedString::new_static("Lagu ini {}-bit, perangkat hanya menerima 24 bit"),
    no_source: SharedString::new_static("Belum ada yang diputar"),
    about_title: SharedString::new_static("Apa itu bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect berarti tidak ada yang mengubah audio dalam perjalanannya dari file ke DAC, chip yang mengubah audio digital menjadi sinyal untuk headphone atau speaker Anda. DAC menerima data yang persis sama dengan yang tersimpan di file.",
    ),
    about_exclusive: SharedString::new_static(
        "Mode eksklusif menghilangkan hampir semua hal yang bisa mengubahnya: suara aplikasi lain tidak dicampurkan, dan perangkat beralih ke laju sampel setiap lagu, sehingga audio tidak perlu di-resample.",
    ),
    about_native_rate: SharedString::new_static(
        "Mode laju sampel asli membuat PipeWire menjalankan perangkat pada laju sampel setiap lagu, sehingga audio tidak perlu di-resample.",
    ),
    status_ok: SharedString::new_static(
        "Saat ini pemutaran bit-perfect: perangkat menerima data file tanpa perubahan.",
    ),
    status_issues: SharedString::new_static("Yang sedang mengubah audio saat ini:"),
    not_exclusive_desc: SharedString::new_static(
        "Audio melewati mixer sistem, tempat audio dicampur dengan aplikasi lain dan mungkin di-resample.",
    ),
    system_volume_desc: SharedString::new_static(
        "Volume sistem diterapkan setelah Pawse menyerahkan audio, jadi suara sudah dikecilkan sebelum mencapai DAC. Untuk mengatasinya, atur volume sistem ke 100% dan atur kerasnya suara di DAC atau amplifier yang punya kenop volume sendiri.",
    ),
    system_volume_caution: SharedString::new_static(
        "Hati-hati: pada 100% suara bisa sangat keras, terutama di headphone yang dicolokkan langsung ke komputer.",
    ),
    system_muted_desc: SharedString::new_static(
        "Perangkat output dibisukan, jadi DAC hanya menerima keheningan. Aktifkan kembali suaranya di pengaturan suara sistem.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Perangkat tidak berjalan pada laju sampel lagu, jadi audio tidak bisa sampai tanpa perubahan. Biasanya perangkat tidak mendukung laju ini, atau aplikasi lain telah menggantinya.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse mengirim audio ke perangkat sebagai floating point 32-bit, yang mempertahankan presisi 24 bit, jadi bit terendah dari lagu ini dibulatkan. Itu jauh di bawah apa pun yang bisa didengar.",
    ),
    no_source_desc: SharedString::new_static(
        "Belum ada yang diputar. Putar sebuah lagu, dan ikon ini akan menunjukkan apakah lagu itu sampai ke perangkat tanpa perubahan.",
    ),
    not_an_error: SharedString::new_static(
        "Semua ini bukan kesalahan: musik diputar seperti biasa. Ikon ini hanya menunjukkan apakah perangkat menerima persis apa yang ada di file.",
    ),
    got_it: SharedString::new_static("Mengerti"),
};

static TH: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("การเล่นแบบ bit-perfect"),
    not_bit_perfect: SharedString::new_static("ไม่ใช่ bit-perfect"),
    click_for_details: SharedString::new_static("คลิกเพื่อดูรายละเอียด"),
    not_exclusive: SharedString::new_static("โหมดเอกสิทธิ์ไม่ได้ทำงาน"),
    native_rate_off: SharedString::new_static("อัตราสุ่มตัวอย่างดั้งเดิมไม่ได้ทำงาน"),
    system_volume_t: SharedString::new_static("ระดับเสียงระบบอยู่ที่ {}% ไม่ใช่ 100%"),
    system_muted: SharedString::new_static("ระบบปิดเสียงอยู่"),
    sample_rate_t: SharedString::new_static("แทร็กเป็น {} แต่อุปกรณ์ทำงานที่ {}"),
    bit_depth_t: SharedString::new_static("แทร็กเป็น {} บิต แต่อุปกรณ์ได้รับเพียง 24 บิต"),
    no_source: SharedString::new_static("ยังไม่มีอะไรเล่นอยู่"),
    about_title: SharedString::new_static("bit-perfect คืออะไร?"),
    about: SharedString::new_static(
        "bit-perfect หมายความว่าไม่มีสิ่งใดเปลี่ยนแปลงเสียงระหว่างทางจากไฟล์ไปยัง DAC ซึ่งเป็นชิปที่แปลงเสียงดิจิทัลเป็นสัญญาณสำหรับหูฟังหรือลำโพงของคุณ DAC จะได้รับข้อมูลตรงตามที่บันทึกไว้ในไฟล์ทุกประการ",
    ),
    about_exclusive: SharedString::new_static(
        "โหมดเอกสิทธิ์ตัดสิ่งที่อาจเปลี่ยนเสียงออกไปเกือบทั้งหมด: เสียงจากแอปอื่นจะไม่ถูกผสมเข้ามา และอุปกรณ์จะสลับไปใช้อัตราสุ่มตัวอย่างของแต่ละแทร็ก จึงไม่ต้องรีแซมเปิลเสียง",
    ),
    about_native_rate: SharedString::new_static(
        "โหมดอัตราสุ่มตัวอย่างดั้งเดิมทำให้ PipeWire ใช้อุปกรณ์ที่อัตราสุ่มตัวอย่างของแต่ละแทร็ก จึงไม่ต้องรีแซมเปิลเสียง",
    ),
    status_ok: SharedString::new_static(
        "ตอนนี้การเล่นเป็นแบบ bit-perfect: อุปกรณ์ได้รับข้อมูลจากไฟล์โดยไม่มีการเปลี่ยนแปลง",
    ),
    status_issues: SharedString::new_static("สิ่งที่กำลังเปลี่ยนเสียงอยู่ตอนนี้:"),
    not_exclusive_desc: SharedString::new_static(
        "เสียงผ่านมิกเซอร์ของระบบ ซึ่งจะผสมกับเสียงจากแอปอื่นและอาจถูกรีแซมเปิล",
    ),
    system_volume_desc: SharedString::new_static(
        "ระดับเสียงระบบถูกใช้หลังจากที่ Pawse ส่งเสียงออกไปแล้ว เสียงจึงถูกลดลงก่อนถึง DAC หากต้องการแก้ ให้ตั้งระดับเสียงระบบเป็น 100% แล้วปรับความดังที่ DAC หรือแอมป์ที่มีปุ่มปรับเสียงของตัวเอง",
    ),
    system_volume_caution: SharedString::new_static(
        "โปรดระวัง: ที่ 100% เสียงอาจดังมาก โดยเฉพาะหูฟังที่เสียบเข้ากับคอมพิวเตอร์โดยตรง",
    ),
    system_muted_desc: SharedString::new_static(
        "อุปกรณ์เอาต์พุตถูกปิดเสียง DAC จึงได้รับแต่ความเงียบ เปิดเสียงได้ในการตั้งค่าเสียงของระบบ",
    ),
    sample_rate_desc: SharedString::new_static(
        "อุปกรณ์ไม่ได้ทำงานที่อัตราสุ่มตัวอย่างของแทร็ก เสียงจึงไปถึงอุปกรณ์แบบไม่เปลี่ยนแปลงไม่ได้ โดยปกติอุปกรณ์ไม่รองรับอัตรานี้ หรือมีแอปอื่นเปลี่ยนไป",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse ส่งเสียงไปยังอุปกรณ์ในรูปแบบเลขทศนิยมลอยตัว 32 บิต ซึ่งเก็บความแม่นยำได้ 24 บิต บิตต่ำสุดของแทร็กนี้จึงถูกปัดเศษ ซึ่งต่ำกว่าระดับที่หูได้ยินมาก",
    ),
    no_source_desc: SharedString::new_static(
        "ยังไม่มีอะไรเล่นอยู่ เริ่มเล่นแทร็ก แล้วไอคอนนี้จะแสดงว่าเสียงไปถึงอุปกรณ์โดยไม่เปลี่ยนแปลงหรือไม่",
    ),
    not_an_error: SharedString::new_static(
        "ทั้งหมดนี้ไม่ใช่ข้อผิดพลาด เพลงยังเล่นได้ตามปกติ ไอคอนนี้แค่บอกว่าอุปกรณ์ได้รับข้อมูลตรงตามไฟล์ทุกประการหรือไม่",
    ),
    got_it: SharedString::new_static("เข้าใจแล้ว"),
};

static CS: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect přehrávání"),
    not_bit_perfect: SharedString::new_static("Není bit-perfect"),
    click_for_details: SharedString::new_static("Klikněte pro podrobnosti"),
    not_exclusive: SharedString::new_static("Exkluzivní režim není aktivní"),
    native_rate_off: SharedString::new_static("Nativní vzorkovací frekvence není aktivní"),
    system_volume_t: SharedString::new_static("Hlasitost systému je {} %, ne 100 %"),
    system_muted: SharedString::new_static("Zvuk systému je ztlumený"),
    sample_rate_t: SharedString::new_static("Skladba má {}, ale zařízení běží na {}"),
    bit_depth_t: SharedString::new_static("Skladba je {}bitová, do zařízení jde 24 bitů"),
    no_source: SharedString::new_static("Zatím nic nehraje"),
    about_title: SharedString::new_static("Co je bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect znamená, že nic nemění zvuk na cestě ze souboru do DAC, čipu, který převádí digitální zvuk na signál pro sluchátka nebo reproduktory. DAC dostane přesně ta data, která jsou uložená v souboru.",
    ),
    about_exclusive: SharedString::new_static(
        "Exkluzivní režim odstraní skoro vše, co by ho mohlo změnit: ostatní aplikace se nepřimíchávají a zařízení se přepne na vzorkovací frekvenci každé skladby, takže nic nemusí zvuk převzorkovat.",
    ),
    about_native_rate: SharedString::new_static(
        "Režim nativní vzorkovací frekvence nechá PipeWire provozovat zařízení na vzorkovací frekvenci každé skladby, takže nic nemusí zvuk převzorkovat.",
    ),
    status_ok: SharedString::new_static(
        "Právě teď je přehrávání bit-perfect: zařízení dostává data souboru beze změn.",
    ),
    status_issues: SharedString::new_static("Co právě teď mění zvuk:"),
    not_exclusive_desc: SharedString::new_static(
        "Zvuk jde přes systémový mixér, kde se míchá s ostatními aplikacemi a může se převzorkovat.",
    ),
    system_volume_desc: SharedString::new_static(
        "Hlasitost systému se uplatní až poté, co Pawse zvuk předá, takže se zvuk ztiší dřív, než dorazí do DAC. Abyste to odstranili, nastavte hlasitost systému na 100 % a hlasitost regulujte na DAC nebo zesilovači s vlastním ovladačem hlasitosti.",
    ),
    system_volume_caution: SharedString::new_static(
        "Pozor: na 100 % může být zvuk velmi hlasitý, zvlášť ve sluchátkách zapojených přímo do počítače.",
    ),
    system_muted_desc: SharedString::new_static(
        "Výstupní zařízení je ztlumené, takže DAC dostává ticho. Zrušte ztlumení v systémovém nastavení zvuku.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Zařízení neběží na vzorkovací frekvenci skladby, takže zvuk do něj nemůže dorazit beze změn. Obvykle zařízení tuto frekvenci nepodporuje, nebo ji přepnula jiná aplikace.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse posílá zvuk do zařízení jako 32bitová čísla s plovoucí desetinnou čárkou, která uchovají 24 bitů přesnosti, takže nejnižší bity této skladby se zaokrouhlí. To je hluboko pod hranicí slyšitelnosti.",
    ),
    no_source_desc: SharedString::new_static(
        "Zatím nic nehraje. Spusťte skladbu a tato ikona ukáže, jestli do zařízení dorazí beze změn.",
    ),
    not_an_error: SharedString::new_static(
        "Nic z toho není chyba: hudba hraje normálně. Ikona jen ukazuje, jestli zařízení dostává přesně to, co je v souboru.",
    ),
    got_it: SharedString::new_static("Rozumím"),
};

static SV: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Bit-perfect uppspelning"),
    not_bit_perfect: SharedString::new_static("Inte bit-perfect"),
    click_for_details: SharedString::new_static("Klicka för detaljer"),
    not_exclusive: SharedString::new_static("Exklusivt läge är inte aktivt"),
    native_rate_off: SharedString::new_static("Native samplingsfrekvens är inte aktiv"),
    system_volume_t: SharedString::new_static("Systemvolymen är {} %, inte 100 %"),
    system_muted: SharedString::new_static("Systemljudet är avstängt"),
    sample_rate_t: SharedString::new_static("Låten är {}, men enheten går på {}"),
    bit_depth_t: SharedString::new_static("Låten är {}-bitars, enheten får 24 bitar"),
    no_source: SharedString::new_static("Inget spelas än"),
    about_title: SharedString::new_static("Vad är bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect betyder att inget ändrar ljudet på vägen från filen till DAC:en, chippet som gör om digitalt ljud till signalen för dina hörlurar eller högtalare. DAC:en får exakt de data som finns i filen.",
    ),
    about_exclusive: SharedString::new_static(
        "Exklusivt läge tar bort det mesta som kan ändra det: andra appar blandas inte in, och enheten byter till varje låts samplingsfrekvens, så inget behöver sampla om ljudet.",
    ),
    about_native_rate: SharedString::new_static(
        "Läget native samplingsfrekvens får PipeWire att köra enheten på varje låts samplingsfrekvens, så inget behöver sampla om ljudet.",
    ),
    status_ok: SharedString::new_static(
        "Just nu är uppspelningen bit-perfect: enheten får filens data oförändrade.",
    ),
    status_issues: SharedString::new_static("Det här ändrar ljudet just nu:"),
    not_exclusive_desc: SharedString::new_static(
        "Ljudet går genom systemets mixer, där det blandas med andra appar och kan samplas om.",
    ),
    system_volume_desc: SharedString::new_static(
        "Systemvolymen tillämpas efter att Pawse har lämnat över ljudet, så ljudet sänks innan det når DAC:en. För att åtgärda det, ställ systemvolymen på 100 % och reglera ljudnivån på en DAC eller förstärkare med egen volymratt.",
    ),
    system_volume_caution: SharedString::new_static(
        "Var försiktig: på 100 % kan ljudet bli mycket högt, särskilt i hörlurar som är kopplade direkt till datorn.",
    ),
    system_muted_desc: SharedString::new_static(
        "Utgångsenheten är ljudlös, så DAC:en får tystnad. Slå på ljudet i systemets ljudinställningar.",
    ),
    sample_rate_desc: SharedString::new_static(
        "Enheten går inte på låtens samplingsfrekvens, så ljudet kan inte nå den oförändrat. Oftast stöder enheten inte den frekvensen, eller så har en annan app ändrat den.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse skickar ljudet till enheten som 32-bitars flyttal, som behåller 24 bitars precision, så de lägsta bitarna i den här låten avrundas. Det ligger långt under allt som går att höra.",
    ),
    no_source_desc: SharedString::new_static(
        "Inget spelas än. Starta en låt så visar den här ikonen om den når enheten oförändrad.",
    ),
    not_an_error: SharedString::new_static(
        "Inget av det här är ett fel: musiken spelas som vanligt. Ikonen visar bara om enheten får exakt det som finns i filen.",
    ),
    got_it: SharedString::new_static("Uppfattat"),
};

static HI: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("बिट-परफ़ेक्ट प्लेबैक"),
    not_bit_perfect: SharedString::new_static("बिट-परफ़ेक्ट नहीं"),
    click_for_details: SharedString::new_static("विवरण के लिए क्लिक करें"),
    not_exclusive: SharedString::new_static("एक्सक्लूसिव मोड सक्रिय नहीं है"),
    native_rate_off: SharedString::new_static("मूल सैंपल दर सक्रिय नहीं है"),
    system_volume_t: SharedString::new_static("सिस्टम वॉल्यूम {}% है, 100% नहीं"),
    system_muted: SharedString::new_static("सिस्टम की आवाज़ म्यूट है"),
    sample_rate_t: SharedString::new_static("ट्रैक {} का है, लेकिन डिवाइस {} पर चल रहा है"),
    bit_depth_t: SharedString::new_static("ट्रैक {}-बिट का है, डिवाइस तक 24 बिट पहुँचते हैं"),
    no_source: SharedString::new_static("अभी कुछ भी नहीं चल रहा"),
    about_title: SharedString::new_static("बिट-परफ़ेक्ट क्या है?"),
    about: SharedString::new_static(
        "बिट-परफ़ेक्ट का मतलब है कि फ़ाइल से DAC तक के रास्ते में ऑडियो में कोई बदलाव नहीं होता। DAC वह चिप है जो डिजिटल ऑडियो को आपके हेडफ़ोन या स्पीकर के सिग्नल में बदलती है। DAC को ठीक वही डेटा मिलता है जो फ़ाइल में सहेजा गया है।",
    ),
    about_exclusive: SharedString::new_static(
        "एक्सक्लूसिव मोड उन ज़्यादातर चीज़ों को हटा देता है जो ऑडियो बदल सकती हैं: दूसरे ऐप्स की आवाज़ नहीं मिलाई जाती, और डिवाइस हर ट्रैक की सैंपल दर पर स्विच हो जाता है, इसलिए ऑडियो को रीसैंपल करने की ज़रूरत नहीं पड़ती।",
    ),
    about_native_rate: SharedString::new_static(
        "मूल सैंपल दर मोड में PipeWire डिवाइस को हर ट्रैक की सैंपल दर पर चलाता है, इसलिए ऑडियो को रीसैंपल करने की ज़रूरत नहीं पड़ती।",
    ),
    status_ok: SharedString::new_static(
        "अभी प्लेबैक बिट-परफ़ेक्ट है: डिवाइस को फ़ाइल का डेटा बिना बदलाव के मिल रहा है।",
    ),
    status_issues: SharedString::new_static("अभी ऑडियो को क्या बदल रहा है:"),
    not_exclusive_desc: SharedString::new_static(
        "ऑडियो सिस्टम मिक्सर से होकर जाता है, जहाँ उसे दूसरे ऐप्स के साथ मिलाया जाता है और उसे रीसैंपल भी किया जा सकता है।",
    ),
    system_volume_desc: SharedString::new_static(
        "सिस्टम वॉल्यूम Pawse के ऑडियो सौंपने के बाद लागू होता है, इसलिए आवाज़ DAC तक पहुँचने से पहले ही धीमी कर दी जाती है। इसे ठीक करने के लिए सिस्टम वॉल्यूम 100% पर रखें और अपने वॉल्यूम नॉब वाले DAC या एम्पलीफ़ायर से आवाज़ नियंत्रित करें।",
    ),
    system_volume_caution: SharedString::new_static(
        "सावधान रहें: 100% पर आवाज़ बहुत तेज़ हो सकती है, खासकर कंप्यूटर में सीधे लगे हेडफ़ोन में।",
    ),
    system_muted_desc: SharedString::new_static(
        "आउटपुट डिवाइस म्यूट है, इसलिए DAC को सिर्फ़ खामोशी मिल रही है। सिस्टम की साउंड सेटिंग्स में इसे अनम्यूट करें।",
    ),
    sample_rate_desc: SharedString::new_static(
        "डिवाइस ट्रैक की सैंपल दर पर नहीं चल रहा, इसलिए ऑडियो उस तक बिना बदलाव के नहीं पहुँच सकता। आम तौर पर डिवाइस यह दर सपोर्ट नहीं करता, या किसी दूसरे ऐप ने इसे बदल दिया है।",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse ऑडियो को 32-बिट फ़्लोटिंग पॉइंट में डिवाइस को भेजता है, जो 24 बिट की सटीकता रखता है, इसलिए इस ट्रैक के सबसे निचले बिट राउंड हो जाते हैं। यह सुनाई देने लायक किसी भी स्तर से बहुत नीचे है।",
    ),
    no_source_desc: SharedString::new_static(
        "अभी कुछ भी नहीं चल रहा। कोई ट्रैक चलाएँ, और यह आइकन दिखाएगा कि वह डिवाइस तक बिना बदलाव के पहुँचता है या नहीं।",
    ),
    not_an_error: SharedString::new_static(
        "इनमें से कुछ भी गड़बड़ी नहीं है: संगीत सामान्य रूप से चल रहा है। यह आइकन सिर्फ़ यह दिखाता है कि डिवाइस को ठीक वही मिल रहा है या नहीं जो फ़ाइल में है।",
    ),
    got_it: SharedString::new_static("ठीक है"),
};

static ES: BitPerfectStrings = BitPerfectStrings {
    bit_perfect: SharedString::new_static("Reproducción bit-perfect"),
    not_bit_perfect: SharedString::new_static("No es bit-perfect"),
    click_for_details: SharedString::new_static("Haz clic para ver detalles"),
    not_exclusive: SharedString::new_static("El modo exclusivo no está activo"),
    native_rate_off: SharedString::new_static("La frecuencia nativa no está activa"),
    system_volume_t: SharedString::new_static("El volumen del sistema está al {} %, no al 100 %"),
    system_muted: SharedString::new_static("El sonido del sistema está silenciado"),
    sample_rate_t: SharedString::new_static("La pista es de {}, pero el dispositivo funciona a {}"),
    bit_depth_t: SharedString::new_static("La pista es de {} bits, el dispositivo recibe 24 bits"),
    no_source: SharedString::new_static("Todavía no se reproduce nada"),
    about_title: SharedString::new_static("¿Qué es bit-perfect?"),
    about: SharedString::new_static(
        "Bit-perfect significa que nada modifica el audio en su camino desde el archivo hasta el DAC, el chip que convierte el audio digital en la señal para tus auriculares o altavoces. El DAC recibe exactamente los datos guardados en el archivo.",
    ),
    about_exclusive: SharedString::new_static(
        "El modo exclusivo elimina casi todo lo que podría modificarlo: no se mezclan otras aplicaciones y el dispositivo cambia a la frecuencia de muestreo de cada pista, así que nada tiene que remuestrear el audio.",
    ),
    about_native_rate: SharedString::new_static(
        "El modo de frecuencia nativa hace que PipeWire use el dispositivo a la frecuencia de muestreo de cada pista, así que nada tiene que remuestrear el audio.",
    ),
    status_ok: SharedString::new_static(
        "Ahora mismo la reproducción es bit-perfect: el dispositivo recibe los datos del archivo sin cambios.",
    ),
    status_issues: SharedString::new_static("Qué modifica el audio ahora mismo:"),
    not_exclusive_desc: SharedString::new_static(
        "El audio pasa por el mezclador del sistema, donde se mezcla con otras aplicaciones y puede remuestrearse.",
    ),
    system_volume_desc: SharedString::new_static(
        "El volumen del sistema se aplica después de que Pawse entrega el audio, así que el sonido se baja antes de llegar al DAC. Para quitar el aviso, pon el volumen del sistema al 100 % y regula el nivel en un DAC o amplificador con su propio control de volumen.",
    ),
    system_volume_caution: SharedString::new_static(
        "Cuidado: al 100 % el sonido puede ser muy fuerte, sobre todo con auriculares conectados directamente al equipo.",
    ),
    system_muted_desc: SharedString::new_static(
        "El dispositivo de salida está silenciado, así que el DAC recibe silencio. Quita el silencio en los ajustes de sonido del sistema.",
    ),
    sample_rate_desc: SharedString::new_static(
        "El dispositivo no funciona a la frecuencia de muestreo de la pista, así que el audio no puede llegar sin cambios. Normalmente el dispositivo no admite esa frecuencia, o la ha cambiado otra aplicación.",
    ),
    bit_depth_desc: SharedString::new_static(
        "Pawse envía el audio al dispositivo en coma flotante de 32 bits, que conserva 24 bits de precisión, así que los bits más bajos de esta pista se redondean. Eso está muy por debajo de lo audible.",
    ),
    no_source_desc: SharedString::new_static(
        "Todavía no se reproduce nada. Pon una pista y este icono mostrará si llega al dispositivo sin cambios.",
    ),
    not_an_error: SharedString::new_static(
        "Nada de esto es un error: la música suena con normalidad. El icono solo indica si el dispositivo recibe exactamente lo que hay en el archivo.",
    ),
    got_it: SharedString::new_static("Entendido"),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_keeps_its_placeholders() {
        for lang in Lang::all() {
            let s = for_lang(*lang);
            assert_eq!(s.system_volume_t.matches("{}").count(), 1, "{lang:?}");
            assert_eq!(s.sample_rate_t.matches("{}").count(), 2, "{lang:?}");
            assert_eq!(s.bit_depth_t.matches("{}").count(), 1, "{lang:?}");
        }
    }
}
