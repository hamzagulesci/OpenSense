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

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Tr => "tr",
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
