//! UI translations. Keys are the English strings themselves, so English
//! needs no table and a missing Turkish entry falls back to English.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Tr,
}

impl Lang {
    /// Resolves the `language` setting; anything but "tr"/"en" (first run)
    /// follows the Windows display language.
    pub fn resolve(setting: &str) -> Self {
        match setting {
            "tr" => Lang::Tr,
            "en" => Lang::En,
            _ => system(),
        }
    }
}

#[cfg(windows)]
fn system() -> Lang {
    // Primary language id 0x1F = Turkish.
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    if id & 0x3FF == 0x1F {
        Lang::Tr
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn system() -> Lang {
    match std::env::var("LANG") {
        Ok(l) if l.starts_with("tr") => Lang::Tr,
        _ => Lang::En,
    }
}

/// Language from the saved settings (for window titles set before the
/// window's own state exists).
pub fn ui_lang() -> Lang {
    Lang::resolve(&opensense_sys::config::AppConfig::load().language)
}

pub fn tr(lang: Lang, s: &'static str) -> &'static str {
    if lang == Lang::En {
        return s;
    }
    match s {
        // navigation
        "Monitor" => "İzleme",
        "Fan control" => "Fan kontrolü",
        "Lighting" => "Aydınlatma",
        "Settings" => "Ayarlar",
        "About" => "Hakkında",
        "AC power" => "Adaptör",
        "Battery" => "Pil",
        // banner
        "OpenSense needs administrator rights to talk to the Acer firmware." => {
            "OpenSense'in Acer firmware'i ile konuşabilmesi için yönetici yetkisi gerekiyor."
        }
        "Restart as administrator" => "Yönetici olarak yeniden başlat",
        "Hardware unavailable" => "Donanıma erişilemiyor",
        // monitor
        "System monitor" => "Sistem izleme",
        "Usage" => "Kullanım",
        "Clock" => "Saat hızı",
        "Fan" => "Fan",
        "Min / Max" => "En düşük / En yüksek",
        "Discrete GPU" => "Harici GPU",
        "Operation mode" => "Çalışma modu",
        "Quiet" => "Sessiz",
        "Default" => "Varsayılan",
        "Extreme" => "Ekstrem",
        "Quiet: low noise, fans stay on Auto." => "Sessiz: düşük ses, fanlar Otomatik'te kalır.",
        "Default: balanced performance and noise." => "Varsayılan: dengeli performans ve ses.",
        "Extreme: maximum performance, needs the power adapter." => {
            "Ekstrem: en yüksek performans, güç adaptörü gerekir."
        }
        "Extreme needs AC power with the full-power adapter." => "Ekstrem mod için tam güçlü adaptör takılı olmalı.",
        // fans
        "CPU fan" => "CPU fanı",
        "GPU fan" => "GPU fanı",
        "Fan mode" => "Fan modu",
        "Auto" => "Otomatik",
        "Max" => "Maksimum",
        "Custom" => "Özel",
        "The laptop adjusts fan speed to temperature. Recommended for everyday use." => {
            "Dizüstü fan hızını sıcaklığa göre ayarlar. Günlük kullanım için önerilir."
        }
        "Fans run at full speed. Best cooling, loudest." => "Fanlar tam hızda döner. En iyi soğutma, en yüksek ses.",
        "Choose the fan speed yourself." => "Fan hızını kendiniz seçin.",
        "Quick presets" => "Hızlı ayarlar",
        "Silent" => "Sessiz",
        "Balanced" => "Dengeli",
        "Cool" => "Serin",
        "Full" => "Tam",
        "Adjust CPU and GPU together" => "CPU ve GPU'yu birlikte ayarla",
        "Both fans" => "İki fan",
        "quiet" => "sessiz",
        "moderate" => "orta",
        "loud" => "yüksek ses",
        "very loud" => "çok yüksek ses",
        "CPU is very hot. Maximum fan speed is recommended." => "CPU çok sıcak. Maksimum fan hızı önerilir.",
        "Use Maximum" => "Maksimum'a geç",
        "CoolBoost (higher fan curve)" => "CoolBoost (daha agresif fan eğrisi)",
        // lighting
        "Keyboard lighting" => "Klavye aydınlatması",
        "This model does not report an RGB keyboard." => "Bu model RGB klavye bildirmiyor.",
        "Solid colours" => "Sabit renkler",
        "Effects" => "Efektler",
        "Brightness" => "Parlaklık",
        "Off" => "Kapalı",
        "Themes" => "Temalar",
        "Colour" => "Renk",
        "Click a zone on the keyboard, then pick a colour." => "Klavyeden bir bölgeye tıklayın, sonra bir renk seçin.",
        "Apply to all zones" => "Tüm bölgelere uygula",
        "Zone" => "Bölge",
        "More colours…" => "Diğer renkler…",
        "Speed" => "Hız",
        "Slow" => "Yavaş",
        "Fast" => "Hızlı",
        "Direction" => "Yön",
        "Left" => "Sola",
        "Right" => "Sağa",
        "Breathing" => "Nefes",
        "Neon" => "Neon",
        "Wave" => "Dalga",
        "Shifting" => "Kayan",
        "Zoom" => "Yakınlaşma",
        "Fades one colour in and out." => "Tek renk yavaşça yanıp söner.",
        "Cycles through all colours." => "Tüm renkler arasında geçiş yapar.",
        "A rainbow sweeps across the keyboard." => "Gökkuşağı klavye boyunca akar.",
        "One colour slides across the zones." => "Bir renk bölgeler arasında kayar.",
        "Colour pulses out from the centre." => "Renk ortadan dışa doğru yayılır.",
        "Turn backlight off after 30 s idle" => "30 sn işlem yapılmazsa ışığı kapat",
        "Nitro Red" => "Nitro Kırmızı",
        "Ocean" => "Okyanus",
        "Sunset" => "Gün batımı",
        "Rainbow" => "Gökkuşağı",
        "Forest" => "Orman",
        "Ice" => "Buz",
        "Purple" => "Mor",
        "White" => "Beyaz",
        // settings
        "Language" => "Dil",
        "Temperature unit" => "Sıcaklık birimi",
        "Start with Windows" => "Windows ile başlat",
        "Hardware" => "Donanım",
        "Re-read" => "Yeniden oku",
        "Read modes and settings from the firmware again" => "Modları ve ayarları firmware'den tekrar oku",
        "LCD overdrive" => "LCD overdrive",
        "Windows power plan" => "Windows güç planı",
        "Settings file" => "Ayar dosyası",
        "Could not save settings" => "Ayarlar kaydedilemedi",
        // worker status
        "Fans: Auto" => "Fanlar: Otomatik",
        "Fans: Max" => "Fanlar: Maksimum",
        "Fans: Custom" => "Fanlar: Özel",
        "CoolBoost updated" => "CoolBoost güncellendi",
        "Operation mode changed" => "Çalışma modu değişti",
        "Keyboard colours applied" => "Klavye renkleri uygulandı",
        "Lighting effect applied" => "Aydınlatma efekti uygulandı",
        "GPU mode set, reboot to apply" => "GPU modu ayarlandı, geçerli olması için yeniden başlatın",
        "LCD overdrive updated" => "LCD overdrive güncellendi",
        "Keyboard timeout updated" => "Klavye zaman aşımı güncellendi",
        "No hardware connection" => "Donanım bağlantısı yok",
        "Demo mode" => "Demo modu",
        "Simulated sensors; nothing is sent to the laptop." => "Sensörler taklit ediliyor; dizüstüne hiçbir şey gönderilmiyor.",
        "Hardware connected" => "Donanım bağlı",
        "Talking to the laptop firmware directly (ACPI-WMI)." => "Dizüstü firmware'i ile doğrudan konuşuyor (ACPI-WMI).",
        "Hardware not connected" => "Donanım bağlı değil",
        "Run OpenSense as administrator to control the laptop." => "Dizüstünü kontrol etmek için OpenSense'i yönetici olarak çalıştırın.",
        "Lowers CPU/GPU power so the fans can run slowest. Quietest option." => {
            "İşlemci ve ekran kartı gücünü düşürür, fanlar en yavaş döner. En sessiz seçenek."
        }
        "In Custom mode the firmware keeps the fans at about 2,500 RPM or more, even at 0%. For the quietest laptop use Silent or Auto: in Auto the fans slow down further when the laptop is cool." => {
            "Özel modda firmware fanları %0'da bile yaklaşık 2.500 RPM'in altına indirmez. En sessiz kullanım için Sessiz veya Otomatik'i seçin: Otomatik'te fanlar dizüstü serinken daha da yavaşlar."
        }
        "Theme" => "Tema",
        "Dark" => "Karanlık",
        "OLED black" => "OLED siyah",
        "Light" => "Aydınlık",
        "Keep running in the system tray" => "Sistem tepsisinde çalışmaya devam et",
        "Mode" => "Mod",
        "Temperature, last 3 minutes" => "Sıcaklık, son 3 dakika",
        "Fan speed, last 3 minutes" => "Fan hızı, son 3 dakika",
        "Appearance" => "Görünüm",
        "Behaviour" => "Davranış",
        "Sensor refresh" => "Sensör yenileme",
        "How often temperatures and fan speeds are read." => "Sıcaklık ve fan hızlarının ne sıklıkla okunacağı.",
        "Restore settings at startup" => "Açılışta ayarları geri yükle",
        "Re-applies operation mode, fans and lighting." => "Çalışma modunu, fanları ve aydınlatmayı yeniden uygular.",
        "The window closes completely; a ~3 MB tray process stays." => "Pencere tamamen kapanır; ~3 MB'lık bir tepsi süreci çalışır.",
        "Starts the tray process at sign-in, without a UAC prompt." => "Oturum açılınca tepsi sürecini UAC sormadan başlatır.",
        "GPU mode" => "GPU modu",
        "Discrete only gives more performance; takes effect after a restart." => "Yalnız harici GPU daha fazla performans verir; yeniden başlatınca geçerli olur.",
        "Discrete only" => "Yalnız harici",
        "Hybrid" => "Hibrit",
        "Faster pixel response, may add ghosting." => "Daha hızlı piksel tepkisi, gölgelenme yapabilir.",
        "Firmware state" => "Firmware durumu",
        "Version" => "Sürüm",
        "An open-source control center for Acer Nitro laptops: fans, operation modes, keyboard lighting and system monitoring." => {
            "Acer Nitro dizüstüleri için açık kaynak kontrol merkezi: fanlar, çalışma modları, klavye aydınlatması ve sistem izleme."
        }
        "About the software" => "Yazılım hakkında",
        "No code has been copied from anywhere: OpenSense is written entirely from scratch." => "Hiçbir yerden kod kopyalanmamıştır: OpenSense tamamen sıfırdan yazılmıştır.",
        "Written in Rust and optimised to run with minimal resources (about 3 MB in the background)." => {
            "Rust ile yazılmıştır ve en az kaynakla çalışacak şekilde optimize edilmiştir (arka planda yaklaşık 3 MB)."
        }
        "Works without NitroSense or any Acer service installed: it talks to the laptop firmware directly through Windows' built-in ACPI-WMI driver." => {
            "NitroSense veya herhangi bir Acer servisi kurulu olmadan çalışır: dizüstü firmware'i ile Windows'un yerleşik ACPI-WMI sürücüsü üzerinden doğrudan konuşur."
        }
        "This device" => "Bu cihaz",
        "Model" => "Model",
        "Firmware connection" => "Firmware bağlantısı",
        "Supported" => "Destekleniyor",
        "Not supported" => "Desteklenmiyor",
        "Operation modes" => "Çalışma modları",
        "RGB keyboard" => "RGB klavye",
        "Technology" => "Teknoloji",
        "Interface" => "Arayüz",
        "Hardware access" => "Donanım erişimi",
        "Licence" => "Lisans",
        "Disclaimer" => "Sorumluluk reddi",
        "All settings changed in the application are your own responsibility. Fan, operation mode and power settings change how your laptop cools itself; the firmware still enforces its own safety limits." => {
            "Uygulama içinde yapılan tüm ayarlar sizin sorumluluğunuzdadır. Fan, çalışma modu ve güç ayarları dizüstünün soğutmasını etkiler; firmware kendi güvenlik sınırlarını yine de uygular."
        }
        "OpenSense is not affiliated with or endorsed by Acer. Acer, Nitro and NitroSense are trademarks of Acer Inc." => {
            "OpenSense, Acer ile bağlantılı değildir ve Acer tarafından onaylanmamıştır. Acer, Nitro ve NitroSense, Acer Inc.'in ticari markalarıdır."
        }
        "Limit charging to 80%" => "Şarjı %80 ile sınırla",
        "Extends battery life when the laptop is mostly plugged in. Stored in the firmware." => {
            "Dizüstü çoğunlukla prize takılıyken pil ömrünü uzatır. Firmware'de saklanır."
        }
        "Battery charge limit updated" => "Pil şarj sınırı güncellendi",
        // tray
        "Show OpenSense" => "OpenSense'i göster",
        "Mode: Quiet" => "Mod: Sessiz",
        "Mode: Default" => "Mod: Varsayılan",
        "Mode: Extreme" => "Mod: Ekstrem",
        "Quit" => "Çıkış",
        // quick panel
        "Overlay" => "Overlay",
        "Overlay only in games" => "Overlay yalnız oyunlarda",
        "Overlay On" => "Overlay açık",
        "Overlay Off" => "Overlay kapalı",
        "In-game overlay on / off" => "Oyun içi overlay aç / kapat",
        "In-game overlay (Ctrl + Shift + Alt + O)" => "Oyun içi overlay (Ctrl + Shift + Alt + O)",
        "Download" => "İndir",
        "Name" => "Ad",
        "Installed" => "Yüklü",
        "Available" => "Mevcut",
        "Can't reach GitHub" => "GitHub'a ulaşılamıyor",
        "Installing…" => "Yükleniyor…",
        "Download and install" => "İndir ve yükle",
        "Open release page" => "Sürüm sayfasını aç",
        "OpenSense is up to date." => "OpenSense güncel.",
        "Update installed." => "Güncelleme yüklendi.",
        "Restart OpenSense" => "OpenSense'i yeniden başlat",
        "Update failed" => "Güncelleme başarısız",
        "BIOS and Drivers" => "BIOS ve Sürücüler",
        "NVIDIA graphics driver" => "NVIDIA ekran sürücüsü",
        "Search for your model on Acer's site." => "Acer sitesinde modelinizi arayın.",
        "Acer Drivers and Manuals" => "Acer sürücü ve kılavuzları",
        "Key Bindings" => "Kısayol Tuşları",
        "Enable keyboard shortcuts" => "Klavye kısayollarını etkinleştir",
        "Laptop Backlight" => "Klavye Işığı",
        "Animation Speed" => "Animasyon hızı",
        "Zone colours" => "Bölge renkleri",
        "Backlight timeout plugged / on battery" => "Işık zaman aşımı adaptörde / pilde",
        "Other" => "Diğer",
        "Scan" => "Tara",
        "Save a report of every firmware value and open it" => "Tüm firmware değerlerinin raporunu kaydet ve aç",
        "Disable PCIe Link State Management (plugged in)" => "PCIe bağlantı durumu yönetimini kapat (adaptörde)",
        "Disable networking in Modern Standby" => "Modern Bekleme'de ağı kapat",
        "Keep app window always on top" => "Uygulama penceresini her zaman üstte tut",
        "Auto Toggle Clamshell Mode" => "Kapak kapalı modunu otomatik aç/kapat",
        "Show on-screen notifications" => "Ekran bildirimlerini göster",
        "Time to hibernate in sleep on battery" => "Uykudan hazırda beklemeye geç",
        "Turn off my screen after" => "Ekranı kapat",
        "Put my device to sleep after" => "Uykuya geç",
        "Never" => "Hiçbir zaman",
        "Acer Services Running" => "Çalışan Acer servisleri",
        "Stop" => "Durdur",
        "Start" => "Başlat",
        "Windows" => "Windows",
        "Extreme needs AC power" => "Ekstrem için adaptör gerekli",
        "Next performance mode" => "Sonraki performans modu",
        "Previous performance mode" => "Önceki performans modu",
        "Show / hide OpenSense" => "OpenSense'i göster / gizle",
        "Quiet mode" => "Sessiz mod",
        "Default mode" => "Varsayılan mod",
        "Extreme mode" => "Ekstrem mod",
        "Keyboard backlight down" => "Klavye ışığını azalt",
        "Keyboard backlight up" => "Klavye ışığını artır",
        "Black and white tray icon" => "Siyah beyaz tepsi simgesi",
        "Restart now? (No = apply at the next restart)" => "Şimdi yeniden başlatılsın mı? (Hayır = bir sonraki açılışta uygulanır)",
        "Windows Power Mode" => "Windows Güç Modu",
        "Best Power Efficiency" => "En iyi güç verimliliği",
        "Best Performance" => "En iyi performans",
        "High Performance Plan" => "Yüksek Performans planı",
        "Disabled" => "Devre dışı",
        "Enabled" => "Etkin",
        "Aggressive" => "Agresif",
        "Efficient Enabled" => "Verimli etkin",
        "Efficient Aggressive" => "Verimli agresif",
        "Aggressive at Guaranteed" => "Garantilide agresif",
        "Efficient at Guaranteed" => "Garantilide verimli",
        "Energy saver is on: Windows controls the power mode." => "Enerji tasarrufu açık: güç modunu Windows yönetiyor.",
        "Disabling boost lowers temperatures and fan noise at some cost in speed." => "Boost'u kapatmak biraz hız pahasına sıcaklığı ve fan sesini düşürür.",
        "GPU Settings" => "GPU Ayarları",
        "Core Clock Limit" => "Çekirdek saat sınırı",
        "Highest supported clock" => "Desteklenen en yüksek saat",
        "Lower limits reduce GPU heat and fan noise. Needs administrator rights." => "Düşük sınır GPU ısısını ve fan sesini azaltır. Yönetici izni gerekir.",
        "Fan Curves" => "Fan Eğrileri",
        "Remove mode" => "Modu sil",
        "Rename mode" => "Modu yeniden adlandır",
        "Add mode" => "Mod ekle",
        "Delete" => "Sil",
        "Clamp to Grid" => "Izgaraya hizala",
        "CPU Fan Profile" => "CPU Fan Profili",
        "GPU Fan Profile" => "GPU Fan Profili",
        "Hold SHIFT to drag all points together" => "Tüm noktaları birlikte sürüklemek için SHIFT'e basılı tutun",
        "Factory Defaults" => "Varsayılanlar",
        "Calibrate" => "Kalibre et",
        "Apply Fan Curve" => "Fan eğrisini uygula",
        "Measuring Max Speed" => "En yüksek hız ölçülüyor",
        "Measured" => "Ölçüldü",
        "Fans + Power" => "Fanlar + Güç",
        "GPU Mode" => "GPU Modu",
        "iGPU + dGPU" => "iGPU + dGPU",
        "dGPU exclusive" => "Yalnız dGPU",
        "restart" => "yeniden başlat",
        "Standard" => "Standart",
        "Ultimate" => "Ultimate",
        "Enables dGPU for standard use" => "Standart kullanım için dGPU'yu etkinleştirir",
        "Routes laptop screen to dGPU, maximizing FPS" => "Ekranı doğrudan dGPU'ya bağlar, FPS'i en üst düzeye çıkarır",
        "Ultimate Mode requires restart" => "Ultimate modu yeniden başlatma gerektirir",
        "Switching off Ultimate Mode requires restart" => "Ultimate modunu kapatmak yeniden başlatma gerektirir",
        "Laptop Keyboard" => "Klavye",
        "Color" => "Renk",
        "Extra" => "Ekstra",
        "OK" => "Tamam",
        "Battery Charge Limit" => "Şarj Sınırı",
        "Run on Startup" => "Başlangıçta çalıştır",
        "Battery Health" => "Pil sağlığı",
        "One time charge to 100%" => "Bir kereliğine %100 şarj et",
        "Updates" => "Güncellemeler",
        "Can't connect to the Acer firmware. Run OpenSense as administrator." => "Acer firmware'ine bağlanılamadı. OpenSense'i yönetici olarak çalıştırın.",
        "Fans" => "Fanlar",
        "Keyboard" => "Klavye",
        "Charging" => "Şarj oluyor",
        "Takes effect after a restart." => "Yeniden başlatınca geçerli olur.",
        "Open OpenSense" => "OpenSense'i aç",
        "Charge limit" => "Şarj sınırı",
        "Discharging" => "Deşarj",
        "Plugged in" => "Prize takılı",
        "On battery" => "Pilde",
        "Health" => "Sağlık",
        "Battery health" => "Pil sağlığı",
        "Full charge capacity" => "Tam şarj kapasitesi",
        "Design capacity" => "Tasarım kapasitesi",
        "Charge cycles" => "Şarj döngüsü",
        "Applies after a restart" => "Yeniden başlatınca uygulanır",
        "Discrete only: more performance, shorter battery life." => "Yalnız harici: daha yüksek performans, daha kısa pil ömrü.",
        "Hybrid: the integrated GPU drives the screen, better battery life." => "Hibrit: ekranı dahili GPU sürer, pil ömrü daha uzun.",
        "The change takes effect after Windows restarts. Restart now?" => "Değişiklik Windows yeniden başlayınca geçerli olur. Şimdi yeniden başlatılsın mı?",
        "Restart now" => "Şimdi yeniden başlat",
        "Later" => "Sonra",
        "Cancel" => "Vazgeç",
        "The firmware did not answer." => "Firmware yanıt vermedi.",
        "Set the CPU and GPU fans to different speeds." => "CPU ve GPU fanlarını farklı hızlara ayarlayın.",
        "Between 80% and 100% the tray keeps the level by switching the firmware's 80% limit." => "%80 ile %100 arasında seviyeyi tepsi süreci, firmware'in %80 sınırını açıp kapatarak korur.",
        "Charge" => "Şarj",
        "More" => "Diğer",
        "Static" => "Sabit",
        "Extra Settings" => "Ek Ayarlar",
        "CPU Boost" => "CPU Boost",
        // hardware test
        "Hardware Test" => "Donanım Testi",
        "Check which features work on this laptop and create a compatibility report" => {
            "Bu dizüstünde hangi özelliklerin çalıştığını denetleyin ve uyumluluk raporu oluşturun"
        }
        "Device" => "Cihaz",
        "Detecting…" => "Algılanıyor…",
        "Automatic tests (read only)" => "Otomatik testler (salt okunur)",
        "Tests that change hardware (ask first)" => "Donanımı değiştiren testler (önce sorulur)",
        "Need a restart (not run)" => "Yeniden başlatma gerektirenler (çalıştırılmaz)",
        "Running…" => "Çalışıyor…",
        "Verified" => "Doğrulandı",
        "Detected" => "Algılandı",
        "Failed" => "Başarısız",
        "Not run" => "Çalıştırılmadı",
        "Run" => "Çalıştır",
        "Run test" => "Testi çalıştır",
        "CPU temperature" => "CPU sıcaklığı",
        "GPU temperature" => "GPU sıcaklığı",
        "CPU fan speed" => "CPU fan hızı",
        "GPU fan speed" => "GPU fan hızı",
        "Performance modes" => "Performans modları",
        "Battery charge limit" => "Pil şarj sınırı",
        "CoolBoost" => "CoolBoost",
        "Screen overdrive" => "Ekran overdrive",
        "Keyboard light timeout" => "Klavye ışığı zaman aşımı",
        "GPU mode (MUX)" => "GPU modu (MUX)",
        "Firmware values" => "Firmware değerleri",
        "Switch performance mode" => "Performans modu değiştirme",
        "Keyboard colours" => "Klavye renkleri",
        "Change charge limit" => "Şarj sınırını değiştirme",
        "Switch GPU mode" => "GPU modu değiştirme",
        "Change screen overdrive" => "Ekran overdrive değiştirme",
        "The fans run at full speed for up to 10 seconds, then return to automatic." => {
            "Fanlar en fazla 10 saniye tam hızda döner, sonra otomatiğe döner."
        }
        "Switches to another performance mode for a moment, then back." => {
            "Kısa süreliğine başka bir performans moduna geçer, sonra geri döner."
        }
        "The keyboard turns red for 2 seconds, then your colours return." => {
            "Klavye 2 saniye kırmızı olur, sonra kendi renkleriniz geri gelir."
        }
        "Turns the 80% charge limit on or off for a moment, then back." => {
            "%80 şarj sınırını kısa süreliğine açar ya da kapatır, sonra geri alır."
        }
        "Needs a restart, so it is not run here." => "Yeniden başlatma gerektirdiği için burada çalıştırılmaz.",
        "Compatibility report" => "Uyumluluk raporu",
        "Only the values above. No serial numbers, user names or files." => {
            "Yalnızca yukarıdaki değerler. Seri numarası, kullanıcı adı ya da dosya yok."
        }
        "Show report" => "Raporu göster",
        "Hide report" => "Raporu gizle",
        "Save JSON" => "JSON kaydet",
        "Copy" => "Kopyala",
        "Share on GitHub" => "GitHub'da paylaş",
        "Opens a pre-filled issue in your browser. Nothing is sent until you submit it there." => {
            "Tarayıcınızda doldurulmuş bir issue açar. Siz orada gönderene kadar hiçbir şey gönderilmez."
        }
        "Report saved." => "Rapor kaydedildi.",
        "Report copied." => "Rapor kopyalandı.",
        "Run all tests" => "Tüm testleri çalıştır",
        "All tests run one after another (about 25 seconds):" => "Tüm testler sırayla çalışır (yaklaşık 25 saniye):",
        "supported; not run, needs a restart" => "destekleniyor; yeniden başlatma gerektirdiği için çalıştırılmadı",
        "toggled and restored" => "değiştirildi ve geri alındı",
        "firmware accepted the colours" => "firmware renkleri kabul etti",
        "80% limit on" => "%80 sınırı açık",
        "80% limit off" => "%80 sınırı kapalı",
        "not run: needs a restart" => "çalıştırılmadı: yeniden başlatma gerekir",
        "Rename" => "Yeniden adlandır",
        "Done" => "Tamam",
        "Shift + Volume Down" => "Shift + Ses kısma",
        "Shift + Volume Up" => "Shift + Ses açma",
        "sec" => "sn",
        "min" => "dk",
        "Fans and Power" => "Fanlar ve Güç",
        "Power plan" => "Güç planı",
        "This power plan has no power modes. Pick a mode above to switch back to Balanced." => {
            "Bu güç planında güç modu yok. Dengeli plana dönmek için yukarıdan bir mod seçin."
        }
        "Open the Windows battery report" => "Windows pil raporunu aç",
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_and_translation() {
        assert_eq!(tr(Lang::En, "Fan control"), "Fan control");
        assert_eq!(tr(Lang::Tr, "Fan control"), "Fan kontrolü");
        assert_eq!(tr(Lang::Tr, "not translated"), "not translated");
        assert_eq!(Lang::resolve("tr"), Lang::Tr);
    }
}
