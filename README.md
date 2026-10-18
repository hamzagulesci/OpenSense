<div align="center">

<img src="assets/logo.png" width="96" alt="OpenSense logo">

# OpenSense

**An open-source control center for Acer Nitro laptops, written from scratch in Rust.**

[![CI](https://github.com/hamzagulesci/OpenSense/actions/workflows/ci.yml/badge.svg)](https://github.com/hamzagulesci/OpenSense/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-red.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-blue.svg)
![Language](https://img.shields.io/badge/language-Rust-orange.svg)

[English](#english) · [Türkçe](#türkçe)

</div>

---

<a id="english"></a>

## English

OpenSense is a lightweight alternative to Acer **NitroSense**. It controls fans, operation modes,
keyboard lighting and battery charging, and shows live temperatures, without any Acer software
installed.

No code has been copied from anywhere. OpenSense is written entirely from scratch in Rust and is
built to use as few resources as possible.

### Features

| Area | What you can do |
|---|---|
| **Fans** | Silent, Auto, Max and Custom modes; quick presets; one slider for both fans or one per fan |
| **Operation modes** | Quiet, Default and Extreme |
| **Keyboard lighting** | 4-zone RGB, colour themes, effects (Breathing, Neon, Wave, Shifting, Zoom), live keyboard preview, backlight auto-off |
| **Battery** | Limit charging to 80% to extend battery life |
| **Monitoring** | CPU and GPU temperature, usage and fan speed; CPU clock; 3-minute history charts |
| **Hardware** | GPU MUX switch (hybrid / discrete only) and LCD overdrive, on models that support them |
| **Power** | Switch Windows power plans |
| **App** | Dark, OLED black and light themes; English and Turkish; system tray menu; start with Windows |
| **CLI** | `opensense-cli` for scripting and automation |

OpenSense asks the firmware which features your laptop supports and only shows those.

### Download and install

1. Download the latest `OpenSense-…-windows-x64.zip` from
   [Releases](https://github.com/hamzagulesci/OpenSense/releases).
2. Extract it to a permanent folder, for example `C:\Program Files\OpenSense`.
3. Run `opensense.exe`. If it is not running as administrator, it offers to restart with the
   rights the firmware interface requires.
4. Optional: turn on **Settings → Start with Windows**. This starts the small tray process at
   sign-in without a UAC prompt.

**Requirements:** Windows 10 or 11 (64-bit) and an Acer Nitro laptop.

**Uninstall:** turn off *Start with Windows*, choose *Quit* in the tray menu, then delete the folder.
Settings are stored in `%APPDATA%\OpenSense`.

### Works without NitroSense

OpenSense does not use any Acer service or application. It talks to the laptop firmware directly
through Windows' built-in, Microsoft-signed `wmiacpi.sys` driver, using the ACPI-WMI classes
`AcerGamingFunction`, `APGeAction` and `BatteryControl` that are defined in the BIOS.
NitroSense and Acer Care Center can be uninstalled.

### Resource usage

| Process | When it runs | Memory | CPU |
|---|---|---|---|
| Tray process | Always, in the background | ~3 MB | ~0% |
| Window | Only while open; fully exits when closed | ~42 MB in use, ~6–15 MB in the background | ~0.03% |

The tray process has no graphics context. It applies your saved settings at startup, offers quick
actions in its menu and shows temperatures in its tooltip.

### FAQ

**The fans don't stop in Custom mode at 0%.**
The firmware keeps the fans at about 2,500 RPM or more in Custom mode. For the quietest laptop use
**Silent** (lower CPU/GPU power) or **Auto** (the fans slow down further when the laptop is cool).

**Why does it need administrator rights?**
Windows only lets administrators call the firmware interface. With *Start with Windows* enabled,
OpenSense starts elevated through a scheduled task, so there is no prompt at every sign-in.

**GPU mode didn't change.**
Switching between hybrid and discrete-only graphics takes effect after a restart.

### Command line

Run in an administrator terminal:

```bash
opensense-cli status
```

```bash
opensense-cli fan custom --cpu 70 --gpu 60
```

```bash
opensense-cli mode extreme
```

```bash
opensense-cli kb static --colors "#FF0000,#FF8000,#FFFF00,#00FF00" --level 4
```

```bash
opensense-cli battery on
```

`opensense-cli probe` only reads from the firmware and lists what your laptop supports.

### Building from source

Install [Rust](https://rustup.rs) 1.88 or newer (stable, MSVC toolchain), then:

```bash
cargo build --release
```

The executables are written to `target/release/`. To try the interface without the hardware, run
demo mode:

```bash
cargo run -p opensense -- --mock
```

### Project structure

```
crates/opensense-proto   firmware method table and value layouts (with tests)
crates/opensense-hw      WMI (COM) access, demo transport, high-level device API
crates/opensense-sys     CPU/GPU monitoring, power plans, settings, autostart
apps/opensense           desktop app and tray process (egui)
apps/opensense-cli       command-line tool
```

### Contributing

Issues and pull requests are welcome. Please run `cargo fmt`, `cargo clippy` and `cargo test`
before opening a pull request. If you try OpenSense on another Acer model, the output of
`opensense-cli probe` is very helpful.

### Disclaimer

All settings changed in the application are your own responsibility. Fan, operation mode, battery
and power settings affect how your laptop cools and charges; the firmware still enforces its own
safety limits.

OpenSense is not affiliated with or endorsed by Acer. Acer, Nitro, Predator and NitroSense are
trademarks of Acer Inc.

### License

[MIT](LICENSE)

---

<a id="türkçe"></a>

## Türkçe

OpenSense, Acer **NitroSense** için hafif bir alternatiftir. Fanları, çalışma modlarını, klavye
aydınlatmasını ve pil şarjını kontrol eder, sıcaklıkları canlı gösterir. Bunun için hiçbir Acer
yazılımının kurulu olması gerekmez.

Hiçbir yerden kod kopyalanmamıştır. OpenSense tamamen sıfırdan Rust ile yazılmıştır ve olabildiğince
az kaynak kullanacak şekilde tasarlanmıştır.

### Özellikler

| Alan | Neler yapılabilir |
|---|---|
| **Fanlar** | Sessiz, Otomatik, Maksimum ve Özel modlar; hızlı ön ayarlar; iki fan için tek slider ya da fan başına ayrı slider |
| **Çalışma modları** | Sessiz, Varsayılan ve Ekstrem |
| **Klavye aydınlatması** | 4 bölgeli RGB, renk temaları, efektler (Nefes, Neon, Dalga, Kayan, Yakınlaşma), canlı klavye önizlemesi, ışığın otomatik kapanması |
| **Pil** | Pil ömrünü uzatmak için şarjı %80 ile sınırlama |
| **İzleme** | CPU ve GPU sıcaklığı, kullanımı ve fan devri; CPU saat hızı; son 3 dakikanın grafikleri |
| **Donanım** | GPU MUX (hibrit / yalnız harici) ve LCD overdrive, destekleyen modellerde |
| **Güç** | Windows güç planı değiştirme |
| **Uygulama** | Karanlık, OLED siyah ve aydınlık tema; Türkçe ve İngilizce; sistem tepsisi menüsü; Windows ile başlatma |
| **Komut satırı** | Betik ve otomasyon için `opensense-cli` |

OpenSense, dizüstünüzün hangi özellikleri desteklediğini firmware'e sorar ve yalnızca onları gösterir.

### İndirme ve kurulum

1. [Releases](https://github.com/hamzagulesci/OpenSense/releases) sayfasından en güncel
   `OpenSense-…-windows-x64.zip` dosyasını indirin.
2. Kalıcı bir klasöre çıkarın, örneğin `C:\Program Files\OpenSense`.
3. `opensense.exe`'yi çalıştırın. Yönetici olarak açılmadıysa, firmware arayüzünün gerektirdiği
   yetkiyle yeniden başlatmayı önerir.
4. İsteğe bağlı: **Ayarlar → Windows ile başlat** seçeneğini açın. Bu, küçük tepsi sürecini oturum
   açılınca UAC sormadan başlatır.

**Gereksinimler:** Windows 10 veya 11 (64 bit) ve bir Acer Nitro dizüstü.

**Kaldırma:** *Windows ile başlat* seçeneğini kapatın, tepsi menüsünden *Çıkış*'ı seçin ve klasörü
silin. Ayarlar `%APPDATA%\OpenSense` klasöründe saklanır.

### NitroSense olmadan çalışır

OpenSense hiçbir Acer servisini veya uygulamasını kullanmaz. Dizüstünün firmware'i ile Windows'un
yerleşik, Microsoft imzalı `wmiacpi.sys` sürücüsü üzerinden doğrudan konuşur. Kullandığı ACPI-WMI
sınıfları (`AcerGamingFunction`, `APGeAction` ve `BatteryControl`) BIOS'ta tanımlıdır.
NitroSense ve Acer Care Center kaldırılabilir.

### Kaynak kullanımı

| Süreç | Ne zaman çalışır | Bellek | CPU |
|---|---|---|---|
| Tepsi süreci | Her zaman, arka planda | ~3 MB | ~%0 |
| Pencere | Yalnızca açıkken; kapatınca tamamen çıkar | Kullanımda ~42 MB, arka planda ~6–15 MB | ~%0,03 |

Tepsi sürecinin grafik bağlamı yoktur. Açılışta kayıtlı ayarları uygular, menüsünde hızlı işlemler
sunar ve ipucunda sıcaklıkları gösterir.

### Sık sorulan sorular

**Özel modda %0'da fanlar durmuyor.**
Firmware, Özel modda fanları yaklaşık 2.500 RPM'in altına indirmez. En sessiz kullanım için
**Sessiz** (işlemci ve ekran kartı gücü düşer) veya **Otomatik** (dizüstü serinken fanlar daha da
yavaşlar) modunu seçin.

**Neden yönetici yetkisi gerekiyor?**
Windows, firmware arayüzünü yalnızca yöneticilerin kullanmasına izin verir. *Windows ile başlat*
açıksa OpenSense zamanlanmış bir görevle yetkili başlar; her oturum açılışında onay sorulmaz.

**GPU modu değişmedi.**
Hibrit ve yalnız harici GPU arasındaki geçiş yeniden başlatınca geçerli olur.

### Komut satırı

Yönetici terminalinde çalıştırın:

```bash
opensense-cli status
```

```bash
opensense-cli fan custom --cpu 70 --gpu 60
```

```bash
opensense-cli mode extreme
```

```bash
opensense-cli kb static --colors "#FF0000,#FF8000,#FFFF00,#00FF00" --level 4
```

```bash
opensense-cli battery on
```

`opensense-cli probe` firmware'den yalnızca okuma yapar ve dizüstünüzün neleri desteklediğini listeler.

### Kaynaktan derleme

[Rust](https://rustup.rs) 1.88 veya daha yeni bir sürüm kurun (stable, MSVC araç zinciri), ardından:

```bash
cargo build --release
```

Çalıştırılabilir dosyalar `target/release/` klasörüne yazılır. Arayüzü donanım olmadan denemek için
demo modu:

```bash
cargo run -p opensense -- --mock
```

### Proje yapısı

```
crates/opensense-proto   firmware metot tablosu ve değer düzenleri (testlerle)
crates/opensense-hw      WMI (COM) erişimi, demo taşıyıcısı, yüksek seviye cihaz API'si
crates/opensense-sys     CPU/GPU izleme, güç planları, ayarlar, otomatik başlatma
apps/opensense           masaüstü uygulaması ve tepsi süreci (egui)
apps/opensense-cli       komut satırı aracı
```

### Katkıda bulunma

Issue ve pull request'ler memnuniyetle karşılanır. Pull request açmadan önce lütfen `cargo fmt`,
`cargo clippy` ve `cargo test` çalıştırın. OpenSense'i başka bir Acer modelinde denerseniz
`opensense-cli probe` çıktısı çok faydalı olur.

### Sorumluluk reddi

Uygulama içinde yapılan tüm ayarlar kullanıcının sorumluluğundadır. Fan, çalışma modu, pil ve güç
ayarları dizüstünün soğutmasını ve şarjını etkiler; firmware kendi güvenlik sınırlarını yine de uygular.

OpenSense, Acer ile bağlantılı değildir ve Acer tarafından onaylanmamıştır. Acer, Nitro, Predator ve
NitroSense, Acer Inc.'in ticari markalarıdır.

### Lisans

[MIT](LICENSE)
