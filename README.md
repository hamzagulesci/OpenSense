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
| **Main window** | Compact window by the clock, like G-Helper: Mode, GPU Mode, Laptop Keyboard and Battery sections, live CPU/GPU temperatures and fan speeds |
| **Performance modes** | Quiet, Default and Extreme plus your own modes based on them; each mode has its own fan curves, Windows power mode and CPU boost |
| **Fans + Power** | Drag-to-edit CPU and GPU fan curves (temperature to fan speed), Windows power mode, CPU boost, NVIDIA clock limit, fan calibration |
| **GPU mode** | Standard (hybrid) / Ultimate (discrete only) MUX switch with a restart prompt |
| **Keyboard lighting** | 4-zone RGB: static colours and effects (Breathing, Neon, Wave, Shifting, Zoom), speed, direction, brightness, idle timeout plugged / on battery |
| **Battery** | Charge limit 80–100%, one-time charge to 100%, charge / discharge watts, battery health, Windows battery report |
| **Shortcuts** | Ctrl+Shift+F5 next mode, Ctrl+Shift+F12 show/hide, Ctrl+Shift+Alt+1/2/3 Quiet/Default/Extreme, Shift+Volume backlight, Ctrl+Shift+Alt+O overlay; on-screen notification |
| **Overlay** | In-game FPS, temperatures, fan speeds, load and memory, optionally only over games |
| **Automation** | Mode remembered per power source (plugged / battery), clamshell mode, PCIe link power and Modern Standby network tweaks, hibernate timeout, vendor service control |
| **App** | Follows the Windows theme (or dark / light); English and Turkish; tray menu; start with Windows; update check |
| **CLI** | `opensense-cli` for scripting and automation |

OpenSense asks the firmware which features your laptop supports and only shows those.

### Download and install

**Installer (recommended):** download `OpenSense-…-setup.exe` from
[Releases](https://github.com/hamzagulesci/OpenSense/releases) and run it. It installs to
`Program Files`, adds a Start menu shortcut and can start OpenSense with Windows. Uninstall it
from *Settings → Apps*.

**Portable:**

1. Download `OpenSense-…-windows-x64.zip` from the same page.
2. Extract it to a permanent folder, for example `C:\Program Files\OpenSense`.
3. Run `opensense.exe`. If it is not running as administrator, it offers to restart with the
   rights the firmware interface requires.
4. Optional: turn on **Run on Startup** in the main window. This starts the small tray process at
   sign-in without a UAC prompt.

**Requirements:** Windows 10 or 11 (64-bit) and an Acer Nitro laptop.

**Uninstall (portable):** turn off *Run on Startup*, choose *Quit* in the tray menu, then delete the
folder. Settings are stored in `%APPDATA%\OpenSense`.

### Works without NitroSense

OpenSense does not use any Acer service or application. It talks to the laptop firmware directly
through Windows' built-in, Microsoft-signed `wmiacpi.sys` driver, using the ACPI-WMI classes
`AcerGamingFunction`, `APGeAction` and `BatteryControl` that are defined in the BIOS.
NitroSense and Acer Care Center can be uninstalled.

### Resource usage

| Process | When it runs | Memory | CPU |
|---|---|---|---|
| Tray process | Always, in the background | ~3 MB (overlay on: a few MB more) | ~0% |
| Main window, Fans + Power, Extra, Updates | Only while open; each fully exits when closed | ~85 MB each while open | ~0% |

The tray process has no graphics context. It applies your saved settings at startup, runs the fan
curves, shortcuts, battery and power automation, draws the overlay and on-screen notifications, and
shows temperatures in its tooltip. Left-click the tray icon to open the main window next to the
clock; click it again or press Esc to close it. Right-click for the menu (modes, GPU mode, overlay).

### FAQ

**The fans don't stop at 0% on a fan curve.**
The firmware keeps the fans at about 2,500 RPM or more whenever OpenSense sets their speed. For the
quietest laptop turn **Apply Fan Curve** off in Fans + Power: in Quiet mode the firmware's own
control slows the fans further when the laptop is cool.

**Do the fan curves keep working when the window is closed?**
Yes. The Acer firmware has no fan curves of its own, so the tray process applies them once a
second; it must stay running (it does with *Start with Windows*).

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
crates/opensense-sys     CPU/GPU monitoring, battery, power and Windows tweaks, settings, autostart
apps/opensense           main window and child windows (egui), tray process, overlay
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
| **Ana pencere** | G-Helper gibi saatin yanında açılan kompakt pencere: Mod, GPU Modu, Klavye ve Pil bölümleri, canlı CPU/GPU sıcaklığı ve fan hızları |
| **Performans modları** | Sessiz, Varsayılan ve Ekstrem ile bunlara dayalı kendi modlarınız; her modun kendi fan eğrileri, Windows güç modu ve CPU Boost ayarı |
| **Fanlar + Güç** | Sürükleyerek düzenlenen CPU ve GPU fan eğrileri (sıcaklığa göre fan hızı), Windows güç modu, CPU Boost, NVIDIA saat sınırı, fan kalibrasyonu |
| **GPU modu** | Standart (hibrit) / Ultimate (yalnız harici) MUX geçişi, yeniden başlatma sorusuyla |
| **Klavye aydınlatması** | 4 bölgeli RGB: sabit renkler ve efektler (Nefes, Neon, Dalga, Kayan, Yakınlaşma), hız, yön, parlaklık, adaptörde / pilde boşta kapanma süresi |
| **Pil** | %80–100 şarj sınırı, bir kerelik %100 şarj, şarj / deşarj gücü, pil sağlığı, Windows pil raporu |
| **Kısayollar** | Ctrl+Shift+F5 sonraki mod, Ctrl+Shift+F12 göster/gizle, Ctrl+Shift+Alt+1/2/3 Sessiz/Varsayılan/Ekstrem, Shift+Ses klavye ışığı, Ctrl+Shift+Alt+O overlay; ekran bildirimi |
| **Overlay** | Oyun içinde FPS, sıcaklıklar, fan hızları, yük ve bellek; istenirse yalnız oyunlarda |
| **Otomasyon** | Güç kaynağına göre hatırlanan mod (adaptör / pil), kapak kapalı modu, PCIe ve Modern Bekleme ağ ayarları, hazırda bekletme süresi, Acer servis kontrolü |
| **Uygulama** | Windows temasını izler (ya da koyu / açık); Türkçe ve İngilizce; tepsi menüsü; Windows ile başlatma; güncelleme kontrolü |
| **Komut satırı** | Betik ve otomasyon için `opensense-cli` |

OpenSense, dizüstünüzün hangi özellikleri desteklediğini firmware'e sorar ve yalnızca onları gösterir.

### İndirme ve kurulum

**Kurulum dosyası (önerilen):** [Releases](https://github.com/hamzagulesci/OpenSense/releases)
sayfasından `OpenSense-…-setup.exe` dosyasını indirip çalıştırın. `Program Files`'a kurar, Başlat
menüsüne kısayol ekler ve isterseniz OpenSense'i Windows ile başlatır. *Ayarlar → Uygulamalar*'dan
kaldırılır.

**Taşınabilir:**

1. Aynı sayfadan `OpenSense-…-windows-x64.zip` dosyasını indirin.
2. Kalıcı bir klasöre çıkarın, örneğin `C:\Program Files\OpenSense`.
3. `opensense.exe`'yi çalıştırın. Yönetici olarak açılmadıysa, firmware arayüzünün gerektirdiği
   yetkiyle yeniden başlatmayı önerir.
4. İsteğe bağlı: ana penceredeki **Başlangıçta çalıştır** seçeneğini açın. Bu, küçük tepsi sürecini
   oturum açılınca UAC sormadan başlatır.

**Gereksinimler:** Windows 10 veya 11 (64 bit) ve bir Acer Nitro dizüstü.

**Kaldırma (taşınabilir):** *Başlangıçta çalıştır*'ı kapatın, tepsi menüsünden *Çıkış*'ı seçin ve
klasörü silin. Ayarlar `%APPDATA%\OpenSense` klasöründe saklanır.

### NitroSense olmadan çalışır

OpenSense hiçbir Acer servisini veya uygulamasını kullanmaz. Dizüstünün firmware'i ile Windows'un
yerleşik, Microsoft imzalı `wmiacpi.sys` sürücüsü üzerinden doğrudan konuşur. Kullandığı ACPI-WMI
sınıfları (`AcerGamingFunction`, `APGeAction` ve `BatteryControl`) BIOS'ta tanımlıdır.
NitroSense ve Acer Care Center kaldırılabilir.

### Kaynak kullanımı

| Süreç | Ne zaman çalışır | Bellek | CPU |
|---|---|---|---|
| Tepsi süreci | Her zaman, arka planda | ~3 MB (overlay açıkken birkaç MB fazla) | ~%0 |
| Ana pencere, Fanlar + Güç, Ekstra, Güncellemeler | Yalnızca açıkken; her biri kapatınca tamamen çıkar | açıkken her biri ~85 MB | ~%0 |

Tepsi sürecinin grafik bağlamı yoktur. Açılışta kayıtlı ayarları uygular; fan eğrilerini,
kısayolları, pil ve güç otomasyonunu çalıştırır; overlay'i ve ekran bildirimlerini çizer, ipucunda
sıcaklıkları gösterir. Tepsi simgesine sol tıklayınca saatin yanında ana pencere açılır; kapatmak için
tekrar tıklayın ya da Esc'ye basın. Sağ tık menüsünde modlar, GPU modu ve overlay bulunur.

### Sık sorulan sorular

**Fan eğrisinde %0'da fanlar durmuyor.**
OpenSense fan hızını ayarladığında firmware fanları yaklaşık 2.500 RPM'in altına indirmez. En sessiz
kullanım için Fanlar + Güç'te **Fan eğrisini uygula**'yı kapatın: Sessiz modda firmware'in kendi
kontrolü, dizüstü serinken fanları daha da yavaşlatır.

**Pencere kapalıyken fan eğrileri çalışmaya devam ediyor mu?**
Evet. Acer firmware'inin kendi fan eğrisi yok; eğrileri tepsi süreci saniyede bir uygular, bu yüzden
tepsi sürecinin açık kalması gerekir (*Windows ile başlat* açıksa kalır).

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
crates/opensense-sys     CPU/GPU izleme, pil, güç ve Windows ayarları, ayarlar, otomatik başlatma
apps/opensense           ana pencere ve alt pencereler (egui), tepsi süreci, overlay
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
