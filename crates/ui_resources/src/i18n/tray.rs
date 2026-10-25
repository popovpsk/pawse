use gpui::SharedString;

use super::{Lang, active};

pub struct TrayStrings {
    pub close_to_tray: SharedString,
    pub close_to_tray_desc: SharedString,
    pub no_tray_hint: SharedString,
    pub show: SharedString,
}

pub fn tray_strings() -> &'static TrayStrings {
    for_lang(active())
}

fn for_lang(lang: Lang) -> &'static TrayStrings {
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

static EN: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Close to tray"),
    close_to_tray_desc: SharedString::new_static(
        "Closing the window keeps Pawse running in the system tray. Click the tray icon to bring the window back.",
    ),
    no_tray_hint: SharedString::new_static(
        "If the desktop has no system tray, closing the window quits as before.",
    ),
    show: SharedString::new_static("Show Pawse"),
};

static ES: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Cerrar a la bandeja"),
    close_to_tray_desc: SharedString::new_static(
        "Al cerrar la ventana, Pawse sigue funcionando en la bandeja del sistema. Haz clic en el icono de la bandeja para volver a abrirla.",
    ),
    no_tray_hint: SharedString::new_static(
        "Si el escritorio no tiene bandeja del sistema, cerrar la ventana sale de la aplicación como antes.",
    ),
    show: SharedString::new_static("Mostrar Pawse"),
};

static ZH: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("关闭时最小化到托盘"),
    close_to_tray_desc: SharedString::new_static(
        "关闭窗口后，Pawse 会继续在系统托盘中运行。点击托盘图标即可恢复窗口。",
    ),
    no_tray_hint: SharedString::new_static("如果桌面没有系统托盘，关闭窗口仍会像以前一样退出。"),
    show: SharedString::new_static("显示 Pawse"),
};

static PT: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Fechar para a bandeja"),
    close_to_tray_desc: SharedString::new_static(
        "Ao fechar a janela, o Pawse continua em execução na bandeja do sistema. Clique no ícone da bandeja para reabri-la.",
    ),
    no_tray_hint: SharedString::new_static(
        "Se a área de trabalho não tiver bandeja do sistema, fechar a janela encerra o app como antes.",
    ),
    show: SharedString::new_static("Mostrar Pawse"),
};

static RU: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Сворачивать в трей при закрытии"),
    close_to_tray_desc: SharedString::new_static(
        "После закрытия окна Pawse продолжает работать в системном трее. Окно возвращается по клику на значок в трее.",
    ),
    no_tray_hint: SharedString::new_static(
        "Если на рабочем столе нет трея, закрытие окна, как и раньше, завершает программу.",
    ),
    show: SharedString::new_static("Показать Pawse"),
};

static JA: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("閉じるとトレイに格納"),
    close_to_tray_desc: SharedString::new_static(
        "ウィンドウを閉じても Pawse はシステムトレイで動作し続けます。トレイアイコンをクリックするとウィンドウが戻ります。",
    ),
    no_tray_hint: SharedString::new_static(
        "デスクトップにシステムトレイがない場合は、これまでどおりウィンドウを閉じると終了します。",
    ),
    show: SharedString::new_static("Pawse を表示"),
};

static DE: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Beim Schließen in den Infobereich"),
    close_to_tray_desc: SharedString::new_static(
        "Nach dem Schließen des Fensters läuft Pawse im Infobereich weiter. Ein Klick auf das Symbol holt das Fenster zurück.",
    ),
    no_tray_hint: SharedString::new_static(
        "Ohne Infobereich auf dem Desktop beendet das Schließen des Fensters die App wie bisher.",
    ),
    show: SharedString::new_static("Pawse anzeigen"),
};

static FR: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Réduire dans la zone de notification"),
    close_to_tray_desc: SharedString::new_static(
        "À la fermeture de la fenêtre, Pawse continue de tourner dans la zone de notification. Cliquez sur l’icône pour la rouvrir.",
    ),
    no_tray_hint: SharedString::new_static(
        "Sans zone de notification sur le bureau, fermer la fenêtre quitte l’application comme avant.",
    ),
    show: SharedString::new_static("Afficher Pawse"),
};

static KO: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("닫을 때 트레이로 최소화"),
    close_to_tray_desc: SharedString::new_static(
        "창을 닫아도 Pawse가 시스템 트레이에서 계속 실행됩니다. 트레이 아이콘을 클릭하면 창이 다시 열립니다.",
    ),
    no_tray_hint: SharedString::new_static(
        "데스크톱에 시스템 트레이가 없으면 이전처럼 창을 닫을 때 종료됩니다.",
    ),
    show: SharedString::new_static("Pawse 표시"),
};

static IT: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Riduci nell’area di notifica"),
    close_to_tray_desc: SharedString::new_static(
        "Chiudendo la finestra, Pawse continua a funzionare nell’area di notifica. Fai clic sull’icona per riaprirla.",
    ),
    no_tray_hint: SharedString::new_static(
        "Se il desktop non ha un’area di notifica, chiudere la finestra esce dall’app come prima.",
    ),
    show: SharedString::new_static("Mostra Pawse"),
};

static TR: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Kapatınca sistem tepsisine küçült"),
    close_to_tray_desc: SharedString::new_static(
        "Pencere kapatıldığında Pawse sistem tepsisinde çalışmaya devam eder. Pencereyi geri getirmek için tepsi simgesine tıklayın.",
    ),
    no_tray_hint: SharedString::new_static(
        "Masaüstünde sistem tepsisi yoksa pencereyi kapatmak uygulamadan eskisi gibi çıkar.",
    ),
    show: SharedString::new_static("Pawse penceresini göster"),
};

static PL: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Zamykaj do zasobnika"),
    close_to_tray_desc: SharedString::new_static(
        "Po zamknięciu okna Pawse działa dalej w zasobniku systemowym. Kliknij ikonę w zasobniku, aby przywrócić okno.",
    ),
    no_tray_hint: SharedString::new_static(
        "Jeśli pulpit nie ma zasobnika systemowego, zamknięcie okna jak dotąd kończy działanie aplikacji.",
    ),
    show: SharedString::new_static("Pokaż Pawse"),
};

static NL: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Sluiten naar systeemvak"),
    close_to_tray_desc: SharedString::new_static(
        "Als je het venster sluit, blijft Pawse draaien in het systeemvak. Klik op het pictogram om het venster terug te halen.",
    ),
    no_tray_hint: SharedString::new_static(
        "Zonder systeemvak op het bureaublad sluit het venster de app zoals voorheen af.",
    ),
    show: SharedString::new_static("Pawse tonen"),
};

static UK: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Згортати в трей під час закриття"),
    close_to_tray_desc: SharedString::new_static(
        "Після закриття вікна Pawse продовжує працювати в системному треї. Вікно повертається клацанням по значку в треї.",
    ),
    no_tray_hint: SharedString::new_static(
        "Якщо на робочому столі немає трею, закриття вікна, як і раніше, завершує програму.",
    ),
    show: SharedString::new_static("Показати Pawse"),
};

static VI: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Thu nhỏ xuống khay khi đóng"),
    close_to_tray_desc: SharedString::new_static(
        "Khi đóng cửa sổ, Pawse vẫn chạy trong khay hệ thống. Nhấp vào biểu tượng trên khay để mở lại cửa sổ.",
    ),
    no_tray_hint: SharedString::new_static(
        "Nếu màn hình không có khay hệ thống, đóng cửa sổ sẽ thoát ứng dụng như trước.",
    ),
    show: SharedString::new_static("Hiện Pawse"),
};

static ID: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Tutup ke baki sistem"),
    close_to_tray_desc: SharedString::new_static(
        "Saat jendela ditutup, Pawse tetap berjalan di baki sistem. Klik ikon baki untuk membukanya kembali.",
    ),
    no_tray_hint: SharedString::new_static(
        "Jika desktop tidak memiliki baki sistem, menutup jendela akan keluar dari aplikasi seperti sebelumnya.",
    ),
    show: SharedString::new_static("Tampilkan Pawse"),
};

static TH: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("ย่อไปที่ถาดระบบเมื่อปิด"),
    close_to_tray_desc: SharedString::new_static(
        "เมื่อปิดหน้าต่าง Pawse จะยังทำงานต่อในถาดระบบ คลิกไอคอนในถาดเพื่อเปิดหน้าต่างอีกครั้ง",
    ),
    no_tray_hint: SharedString::new_static("หากเดสก์ท็อปไม่มีถาดระบบ การปิดหน้าต่างจะออกจากแอปเหมือนเดิม"),
    show: SharedString::new_static("แสดง Pawse"),
};

static CS: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Zavírat do oznamovací oblasti"),
    close_to_tray_desc: SharedString::new_static(
        "Po zavření okna Pawse dál běží v oznamovací oblasti. Kliknutím na ikonu okno vrátíte.",
    ),
    no_tray_hint: SharedString::new_static(
        "Pokud plocha nemá oznamovací oblast, zavření okna aplikaci ukončí jako dřív.",
    ),
    show: SharedString::new_static("Zobrazit Pawse"),
};

static SV: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("Stäng till systemfältet"),
    close_to_tray_desc: SharedString::new_static(
        "När fönstret stängs fortsätter Pawse att köras i systemfältet. Klicka på ikonen för att visa fönstret igen.",
    ),
    no_tray_hint: SharedString::new_static(
        "Utan systemfält på skrivbordet avslutas appen som förut när fönstret stängs.",
    ),
    show: SharedString::new_static("Visa Pawse"),
};

static HI: TrayStrings = TrayStrings {
    close_to_tray: SharedString::new_static("बंद करने पर ट्रे में छोटा करें"),
    close_to_tray_desc: SharedString::new_static(
        "विंडो बंद करने पर Pawse सिस्टम ट्रे में चलता रहता है। विंडो वापस लाने के लिए ट्रे आइकन पर क्लिक करें।",
    ),
    no_tray_hint: SharedString::new_static(
        "अगर डेस्कटॉप पर सिस्टम ट्रे नहीं है, तो विंडो बंद करने पर ऐप पहले की तरह बंद हो जाएगा।",
    ),
    show: SharedString::new_static("Pawse दिखाएँ"),
};
