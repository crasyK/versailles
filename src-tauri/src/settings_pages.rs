//! Windows Settings pages for the command bar's search (#56).
//!
//! A fixed table of `ms-settings:` pages (URIs from Microsoft Learn's
//! "Launch Windows Settings" list) with English and German names and
//! keywords in both languages. Matching is case- and diacritic-insensitive:
//! a name or keyword that equals the query, starts with it (also with spaces
//! and hyphens ignored: "wifi" finds "Wi-Fi"), or has a word starting with
//! each query word. Titles come in the Windows display language (German or
//! English).

use serde::Serialize;

pub struct Page {
    pub uri: &'static str,
    pub en: &'static str,
    pub de: &'static str,
    /// Extra words in both languages.
    pub keys: &'static [&'static str],
}

macro_rules! page {
    ($uri:literal, $en:literal, $de:literal, [$($k:literal),* $(,)?]) => {
        Page { uri: concat!("ms-settings:", $uri), en: $en, de: $de, keys: &[$($k),*] }
    };
}

pub const PAGES: &[Page] = &[
    // System
    page!("display", "Display", "Bildschirm", ["screen", "resolution", "scale", "brightness", "monitor", "hdr", "rotation", "auflösung", "skalierung", "helligkeit", "anzeige"]),
    page!("nightlight", "Night light", "Nachtmodus", ["blue light", "warm", "blaulicht", "nachtlicht"]),
    page!("display-advancedgraphics", "Graphics", "Grafik", ["gpu", "graphics card", "grafikkarte", "hardware acceleration"]),
    page!("sound", "Sound", "Sound", ["audio", "volume", "speaker", "lautstärke", "ton", "klang"]),
    page!("sound-devices", "Sound devices", "Alle Audiogeräte", ["audio devices", "audiogeräte", "soundgeräte"]),
    page!("sound-defaultoutputproperties", "Speakers", "Lautsprecher", ["output", "headphones", "default output", "ausgabe", "kopfhörer", "ausgabegerät"]),
    page!("sound-defaultinputproperties", "Microphone", "Mikrofon", ["mic", "input", "recording", "eingabe", "eingabegerät", "aufnahme"]),
    page!("apps-volume", "Volume mixer", "Lautstärkemixer", ["mixer", "app volume", "app-lautstärke"]),
    page!("notifications", "Notifications", "Benachrichtigungen", ["toasts", "banners", "alerts", "mitteilungen", "hinweise"]),
    page!("quiethours", "Focus", "Fokus", ["do not disturb", "focus assist", "nicht stören", "ruhezeiten", "konzentrationsassistent"]),
    page!("powersleep", "Power & battery", "Netzbetrieb und Akku", ["power", "sleep", "battery", "screen off", "standby", "energy", "power mode", "energie", "ruhezustand", "akku", "energiesparen", "netzbetrieb", "energiemodus"]),
    page!("batterysaver", "Energy saver", "Energiesparmodus", ["battery saver", "stromsparmodus", "akku sparen"]),
    page!("storagesense", "Storage", "Speicher", ["disk space", "free space", "drive", "speicherplatz", "festplatte"]),
    page!("storagepolicies", "Storage Sense", "Speicheroptimierung", ["cleanup", "temporary files", "bereinigen", "temporäre dateien"]),
    page!("storagerecommendations", "Cleanup recommendations", "Bereinigungsempfehlungen", ["clean up", "aufräumen"]),
    page!("disksandvolumes", "Disks & volumes", "Datenträger und Volumes", ["partition", "drive", "format", "laufwerk", "festplatte", "datenträger"]),
    page!("savelocations", "Default save locations", "Speicherort für neue Inhalte", ["save location", "speicherort"]),
    page!("multitasking", "Multitasking", "Multitasking", ["snap", "alt tab", "virtual desktops", "andocken", "fenster andocken", "virtuelle desktops"]),
    page!("clipboard", "Clipboard", "Zwischenablage", ["clipboard history", "copy paste", "verlauf", "kopieren"]),
    page!("project", "Projecting to this PC", "Projizieren auf diesen PC", ["miracast", "wireless display", "projizieren"]),
    page!("remotedesktop", "Remote Desktop", "Remotedesktop", ["rdp", "remote"]),
    page!("crossdevice", "Nearby sharing", "Umgebungsfreigabe", ["share", "nearby", "freigabe", "teilen", "in der nähe"]),
    page!("about", "About", "Info", ["system info", "pc name", "rename", "version", "specs", "device specifications", "computer name", "systeminfo", "umbenennen", "gerätename", "spezifikationen", "windows version"]),
    page!("troubleshoot", "Troubleshoot", "Problembehandlung", ["fix", "problems", "repair", "probleme", "reparieren"]),
    page!("recovery", "Recovery", "Wiederherstellung", ["reset", "reset this pc", "go back", "advanced startup", "zurücksetzen", "pc zurücksetzen", "erweiterter start"]),
    page!("activation", "Activation", "Aktivierung", ["product key", "license", "produktschlüssel", "lizenz"]),
    page!("deviceencryption", "Device encryption", "Geräteverschlüsselung", ["bitlocker", "encryption", "verschlüsselung"]),
    page!("developers", "For developers", "Für Entwickler", ["developer mode", "entwicklermodus", "dev"]),
    page!("energyrecommendations", "Energy recommendations", "Energieempfehlungen", ["save energy", "energie sparen"]),
    // Bluetooth & devices
    page!("bluetooth", "Bluetooth & devices", "Bluetooth und Geräte", ["bluetooth", "devices", "pair", "headset", "geräte", "koppeln"]),
    page!("connecteddevices", "Devices", "Geräte", ["add device", "gerät hinzufügen"]),
    page!("printers", "Printers & scanners", "Drucker und Scanner", ["printer", "scanner", "print", "drucker", "drucken"]),
    page!("mousetouchpad", "Mouse", "Maus", ["mouse speed", "scroll", "pointer speed", "cursor speed", "mausgeschwindigkeit", "scrollen", "zeigergeschwindigkeit"]),
    page!("devices-touchpad", "Touchpad", "Touchpad", ["gestures", "trackpad", "gesten"]),
    page!("pen", "Pen & Windows Ink", "Stift und Windows Ink", ["stylus", "pen", "stift"]),
    page!("typing", "Typing", "Eingabe", ["autocorrect", "spelling", "text suggestions", "autokorrektur", "rechtschreibung", "textvorschläge"]),
    page!("autoplay", "AutoPlay", "Automatische Wiedergabe", ["autorun", "autoplay", "automatische wiedergabe"]),
    page!("usb", "USB", "USB", ["usb"]),
    page!("camera", "Cameras", "Kameras", ["webcam", "camera", "kamera"]),
    page!("mobile-devices", "Mobile devices", "Mobile Geräte", ["phone", "phone link", "smartphone", "handy", "telefon", "smartphone-link"]),
    // Network & internet
    page!("network-status", "Network & internet", "Netzwerk und Internet", ["network", "internet", "connection", "netzwerk", "verbindung"]),
    page!("network-wifi", "Wi-Fi", "WLAN", ["wifi", "wlan", "wireless", "drahtlos"]),
    page!("network-wifisettings", "Manage known networks", "Bekannte Netzwerke verwalten", ["known networks", "forget network", "saved networks", "netzwerk vergessen", "gespeicherte netzwerke"]),
    page!("network-ethernet", "Ethernet", "Ethernet", ["lan", "cable", "kabel"]),
    page!("network-vpn", "VPN", "VPN", ["vpn"]),
    page!("network-mobilehotspot", "Mobile hotspot", "Mobiler Hotspot", ["hotspot", "tethering", "share internet", "internet teilen"]),
    page!("network-airplanemode", "Airplane mode", "Flugzeugmodus", ["flight mode", "flugmodus"]),
    page!("network-proxy", "Proxy", "Proxy", ["proxy"]),
    page!("network-cellular", "Cellular", "Mobilfunk", ["mobile data", "sim", "lte", "5g", "mobile daten"]),
    page!("network-dialup", "Dial-up", "DFÜ", ["dial up", "modem", "einwahl"]),
    page!("network-advancedsettings", "Advanced network settings", "Erweiterte Netzwerkeinstellungen", ["adapter", "network adapter", "dns", "ip", "netzwerkadapter"]),
    // Personalization
    page!("personalization", "Personalization", "Personalisierung", ["customize", "anpassen"]),
    page!("personalization-background", "Background", "Hintergrund", ["wallpaper", "desktop background", "hintergrundbild", "desktophintergrund"]),
    page!("personalization-colors", "Colors", "Farben", ["dark mode", "light mode", "accent color", "transparency", "dunkler modus", "heller modus", "akzentfarbe", "transparenz", "dark theme"]),
    page!("themes", "Themes", "Designs", ["theme", "design", "desktop icons", "desktopsymbole"]),
    page!("lockscreen", "Lock screen", "Sperrbildschirm", ["lock screen", "screen saver", "bildschirmschoner"]),
    page!("personalization-textinput", "Text input", "Texteingabe", ["emoji", "touch keyboard theme"]),
    page!("personalization-touchkeyboard", "Touch keyboard", "Bildschirmtastatur", ["on-screen keyboard", "osk", "touch-tastatur"]),
    page!("personalization-start", "Start", "Start", ["start menu", "startmenü", "recommended", "empfohlen"]),
    page!("taskbar", "Taskbar", "Taskleiste", ["taskbar", "system tray", "tray icons", "infobereich", "taskleistensymbole"]),
    page!("fonts", "Fonts", "Schriftarten", ["font", "typeface", "schrift"]),
    page!("personalization-lighting", "Dynamic Lighting", "Dynamische Beleuchtung", ["rgb", "lighting", "beleuchtung"]),
    // Apps
    page!("appsfeatures", "Installed apps", "Installierte Apps", ["apps", "uninstall", "programs", "apps & features", "remove app", "deinstallieren", "programme", "apps und features", "software"]),
    page!("defaultapps", "Default apps", "Standard-Apps", ["default browser", "file associations", "open with", "standardbrowser", "standardprogramme", "dateizuordnung"]),
    page!("startupapps", "Startup apps", "Autostart", ["startup", "autostart", "start with windows", "beim start"]),
    page!("optionalfeatures", "Optional features", "Optionale Features", ["features", "windows features", "zusätzliche features"]),
    page!("appsforwebsites", "Apps for websites", "Apps für Websites", ["links", "web links"]),
    page!("videoplayback", "Video playback", "Videowiedergabe", ["video", "hdr video"]),
    page!("maps", "Offline maps", "Offlinekarten", ["maps", "karten"]),
    // Accounts
    page!("yourinfo", "Your info", "Ihre Infos", ["account", "profile", "microsoft account", "picture", "konto", "profil", "kontobild"]),
    page!("signinoptions", "Sign-in options", "Anmeldeoptionen", ["password", "pin", "windows hello", "fingerprint", "face", "login", "passwort", "kennwort", "fingerabdruck", "gesichtserkennung", "anmelden"]),
    page!("emailandaccounts", "Email & accounts", "E-Mail und Konten", ["email", "mail", "accounts", "konten"]),
    page!("workplace", "Access work or school", "Auf Arbeits- oder Schulkonto zugreifen", ["work account", "school", "arbeitskonto", "schulkonto", "arbeit"]),
    page!("otherusers", "Other users", "Andere Benutzer", ["users", "add user", "benutzer", "benutzer hinzufügen"]),
    page!("family-group", "Family", "Familie", ["family", "parental controls", "jugendschutz"]),
    page!("sync", "Windows backup", "Windows-Sicherung", ["backup", "sync", "onedrive", "sicherung", "synchronisieren"]),
    // Time & language
    page!("dateandtime", "Date & time", "Datum und Uhrzeit", ["clock", "time zone", "date", "time", "uhr", "zeitzone", "uhrzeit", "datum"]),
    page!("regionlanguage", "Language & region", "Sprache und Region", ["language", "display language", "sprache", "anzeigesprache"]),
    page!("regionformatting", "Region", "Region", ["country", "regional format", "date format", "land", "format", "regionales format"]),
    page!("keyboard", "Keyboard", "Tastatur", ["keyboard layout", "input method", "tastaturlayout", "eingabemethode", "tastatursprache"]),
    page!("speech", "Speech", "Spracherkennung", ["voice", "speech recognition", "stimme", "sprachausgabe stimmen"]),
    // Gaming
    page!("gaming-gamebar", "Game Bar", "Game Bar", ["xbox game bar", "gaming", "spiele"]),
    page!("gaming-gamedvr", "Captures", "Aufnahmen", ["screen recording", "game recording", "bildschirmaufnahme", "aufzeichnen"]),
    page!("gaming-gamemode", "Game Mode", "Spielmodus", ["gaming", "game mode", "spiele"]),
    // Accessibility
    page!("easeofaccess-display", "Text size", "Textgröße", ["accessibility", "bigger text", "font size", "barrierefreiheit", "schriftgröße", "größerer text"]),
    page!("easeofaccess-visualeffects", "Visual effects", "Visuelle Effekte", ["animations", "transparency effects", "scrollbars", "animationen", "bildlaufleisten"]),
    page!("easeofaccess-mousepointer", "Mouse pointer and touch", "Mauszeiger und Toucheingabe", ["cursor size", "pointer color", "zeigergröße", "mauszeiger"]),
    page!("easeofaccess-cursor", "Text cursor", "Textcursor", ["caret", "cursor thickness", "cursorstärke"]),
    page!("easeofaccess-magnifier", "Magnifier", "Bildschirmlupe", ["zoom", "magnify", "lupe", "vergrößern"]),
    page!("easeofaccess-colorfilter", "Color filters", "Farbfilter", ["color blind", "grayscale", "invert", "farbenblind", "graustufen", "farbfilter"]),
    page!("easeofaccess-highcontrast", "Contrast themes", "Kontrastdesigns", ["high contrast", "contrast", "hoher kontrast", "kontrast"]),
    page!("easeofaccess-narrator", "Narrator", "Sprachausgabe", ["screen reader", "read aloud", "vorlesen", "bildschirmleser"]),
    page!("easeofaccess-audio", "Audio (accessibility)", "Audio (Barrierefreiheit)", ["mono audio", "flash screen", "monoaudio", "bildschirm blinken"]),
    page!("easeofaccess-closedcaptioning", "Captions", "Untertitel", ["live captions", "subtitles", "liveuntertitel", "untertitel"]),
    page!("easeofaccess-speechrecognition", "Voice access", "Sprachzugriff", ["voice typing", "voice control", "spracheingabe", "sprachsteuerung"]),
    page!("easeofaccess-keyboard", "Keyboard (accessibility)", "Tastatur (Barrierefreiheit)", ["sticky keys", "filter keys", "toggle keys", "on-screen keyboard", "einrastfunktion", "anschlagverzögerung", "bildschirmtastatur"]),
    page!("easeofaccess-mouse", "Mouse keys", "Maustasten", ["mouse keys", "numpad mouse", "maustasten"]),
    page!("easeofaccess-eyecontrol", "Eye control", "Augensteuerung", ["eye tracking", "augensteuerung"]),
    page!("easeofaccess-hearingaids", "Hearing devices", "Hörgeräte", ["hearing aids", "hörgeräte"]),
    // Privacy & security
    page!("privacy", "Privacy & security", "Datenschutz und Sicherheit", ["privacy", "permissions", "datenschutz", "berechtigungen"]),
    page!("privacy-general", "General privacy", "Allgemein (Datenschutz)", ["advertising id", "werbe-id", "werbung"]),
    page!("privacy-location", "Location", "Standort", ["gps", "location services", "ortung", "standortdienste"]),
    page!("privacy-webcam", "Camera access", "Kamerazugriff", ["camera permission", "webcam access", "kameraberechtigung"]),
    page!("privacy-microphone", "Microphone access", "Mikrofonzugriff", ["microphone permission", "mic access", "mikrofonberechtigung"]),
    page!("privacy-activityhistory", "Activity history", "Aktivitätsverlauf", ["timeline", "history", "verlauf"]),
    page!("privacy-feedback", "Diagnostics & feedback", "Diagnose und Feedback", ["telemetry", "diagnostic data", "telemetrie", "diagnosedaten"]),
    page!("privacy-broadfilesystemaccess", "File system access", "Dateisystemzugriff", ["file access", "dateizugriff"]),
    page!("privacy-appdiagnostics", "App diagnostics", "App-Diagnose", ["app diagnostics"]),
    page!("search-permissions", "Search permissions", "Suchberechtigungen", ["safesearch", "cloud search", "search history", "suchverlauf", "sichere suche"]),
    page!("search", "Search", "Suche", ["windows search", "indexing", "search index", "indizierung", "suchindex", "windows durchsuchen"]),
    page!("windowsdefender", "Windows Security", "Windows-Sicherheit", ["antivirus", "defender", "firewall", "virus", "virenschutz", "sicherheit"]),
    page!("findmydevice", "Find my device", "Mein Gerät suchen", ["lost device", "gerät finden"]),
    // Windows Update
    page!("windowsupdate", "Windows Update", "Windows Update", ["updates", "update", "upgrade", "aktualisierung", "aktualisieren"]),
    page!("windowsupdate-history", "Update history", "Updateverlauf", ["installed updates", "uninstall updates", "installierte updates"]),
    page!("windowsupdate-options", "Advanced update options", "Erweiterte Optionen (Windows Update)", ["update options", "updateoptionen"]),
    page!("windowsupdate-activehours", "Active hours", "Nutzungszeit", ["active hours", "nutzungszeit"]),
    page!("windowsupdate-optionalupdates", "Optional updates", "Optionale Updates", ["driver updates", "treiberupdates", "treiber"]),
    page!("delivery-optimization", "Delivery Optimization", "Übermittlungsoptimierung", ["download limit", "bandwidth", "bandbreite"]),
    page!("windowsinsider", "Windows Insider Program", "Windows-Insider-Programm", ["insider", "preview builds"]),
];

/// A Settings page that matches a query.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SettingsHit {
    pub kind: &'static str,
    /// In the Windows display language (German or English).
    pub title: String,
    pub uri: &'static str,
    pub en: &'static str,
    pub de: &'static str,
    pub score: u32,
    /// The query is a whole name or keyword ("bluetooth", "night light").
    pub exact: bool,
}

/// Lowercase, umlauts and accents folded (`ä` → `a`, `ß` → `ss`), anything
/// but letters and digits made a space, spaces collapsed.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => out.push('a'),
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' | 'ø' => out.push('o'),
            'ü' | 'ù' | 'ú' | 'û' => out.push('u'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' => out.push('i'),
            'ç' => out.push('c'),
            'ñ' => out.push('n'),
            'ß' => out.push_str("ss"),
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// How well one folded field matches a folded query (0 = not at all).
fn field_score(field: &str, q: &str) -> u32 {
    if field.is_empty() || q.is_empty() {
        return 0;
    }
    let (fc, qc) = (compact(field), compact(q));
    if field == q || fc == qc {
        return 100;
    }
    if fc.starts_with(&qc) {
        return 80;
    }
    let words: Vec<&str> = field.split(' ').collect();
    if q.split(' ').all(|t| t.len() >= 2 && words.iter().any(|w| w.starts_with(t))) {
        return 60;
    }
    0
}

/// The query, plus a spelling with `ae` / `oe` / `ue` for umlauts
/// ("lautstaerke" finds "Lautstärke").
fn query_variants(q: &str) -> Vec<String> {
    let base = fold(q);
    let mut out = vec![base.clone()];
    let alt = base.replace("ae", "a").replace("oe", "o").replace("ue", "u");
    if alt != base {
        out.push(alt);
    }
    out
}

fn page_score(p: &Page, variants: &[String]) -> u32 {
    let mut best = 0;
    for q in variants {
        // A page's own name beats the same match on one of its keywords.
        for name in [p.en, p.de] {
            let s = field_score(&fold(name), q);
            if s > 0 {
                best = best.max(s + 5);
            }
        }
        for k in p.keys {
            best = best.max(field_score(&fold(k), q));
        }
    }
    best
}

/// Settings pages matching `query`, best first, at most `limit`.
pub fn search(query: &str, limit: usize, german: bool) -> Vec<SettingsHit> {
    let variants = query_variants(query);
    if compact(&variants[0]).chars().count() < 2 {
        return Vec::new();
    }
    let mut hits: Vec<(u32, usize)> = PAGES
        .iter()
        .enumerate()
        .map(|(i, p)| (page_score(p, &variants), i))
        .filter(|(s, _)| *s > 0)
        .collect();
    // Best score first; the table order breaks ties (common pages first).
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut seen = std::collections::HashSet::new();
    hits.into_iter()
        .filter(|(_, i)| seen.insert(PAGES[*i].uri))
        .take(limit)
        .map(|(score, i)| {
            let p = &PAGES[i];
            SettingsHit {
                kind: "settings",
                title: if german { p.de } else { p.en }.to_string(),
                uri: p.uri,
                en: p.en,
                de: p.de,
                score,
                exact: score >= 100,
            }
        })
        .collect()
}

/// Whether Windows shows its UI in German.
pub fn ui_is_german() -> bool {
    #[cfg(windows)]
    {
        // Primary language id of the user's display language; 0x07 = German.
        let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        lang & 0x3FF == 0x07
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(q: &str) -> Option<&'static str> {
        search(q, 5, false).first().map(|h| h.uri)
    }

    #[test]
    fn table_is_sane() {
        assert!(PAGES.len() >= 80, "only {} pages", PAGES.len());
        let mut uris = std::collections::HashSet::new();
        for p in PAGES {
            assert!(p.uri.starts_with("ms-settings:") && p.uri.len() > "ms-settings:".len(), "{}", p.uri);
            assert!(!p.uri.contains(' '), "{}", p.uri);
            assert!(uris.insert(p.uri), "duplicate {}", p.uri);
            assert!(!p.en.is_empty() && !p.de.is_empty());
            // Every URI is one Windows' own launcher accepts.
            assert!(crate::apps::denied_target(p.uri).is_none(), "{}", p.uri);
        }
    }

    #[test]
    fn folds_case_umlauts_and_punctuation() {
        assert_eq!(fold("Bluetooth und Geräte"), "bluetooth und gerate");
        assert_eq!(fold("Wi-Fi"), "wi fi");
        assert_eq!(fold("  Größe  "), "grosse");
        assert_eq!(fold("E-Mail und Konten"), "e mail und konten");
    }

    #[test]
    fn english_names_and_keywords() {
        assert_eq!(top("bluetooth"), Some("ms-settings:bluetooth"));
        assert_eq!(top("Display"), Some("ms-settings:display"));
        assert_eq!(top("wifi"), Some("ms-settings:network-wifi"));
        assert_eq!(top("wi-fi"), Some("ms-settings:network-wifi"));
        assert_eq!(top("night light"), Some("ms-settings:nightlight"));
        assert_eq!(top("vpn"), Some("ms-settings:network-vpn"));
        assert_eq!(top("hotspot"), Some("ms-settings:network-mobilehotspot"));
        assert_eq!(top("default apps"), Some("ms-settings:defaultapps"));
        assert_eq!(top("uninstall"), Some("ms-settings:appsfeatures"));
        assert_eq!(top("wallpaper"), Some("ms-settings:personalization-background"));
        assert_eq!(top("dark mode"), Some("ms-settings:personalization-colors"));
        assert_eq!(top("windows update"), Some("ms-settings:windowsupdate"));
        assert_eq!(top("sticky keys"), Some("ms-settings:easeofaccess-keyboard"));
        assert_eq!(top("startup apps"), Some("ms-settings:startupapps"));
        // Prefixes while typing.
        assert_eq!(top("blue"), Some("ms-settings:bluetooth"));
        assert_eq!(top("taskb"), Some("ms-settings:taskbar"));
    }

    #[test]
    fn german_names_and_keywords() {
        assert_eq!(top("Bildschirm"), Some("ms-settings:display"));
        assert_eq!(top("lautstärke"), Some("ms-settings:sound"));
        assert_eq!(top("lautstarke"), Some("ms-settings:sound"));
        assert_eq!(top("lautstaerke"), Some("ms-settings:sound"));
        assert_eq!(top("WLAN"), Some("ms-settings:network-wifi"));
        assert_eq!(top("taskleiste"), Some("ms-settings:taskbar"));
        assert_eq!(top("hintergrund"), Some("ms-settings:personalization-background"));
        assert_eq!(top("Standard-Apps"), Some("ms-settings:defaultapps"));
        assert_eq!(top("nachtmodus"), Some("ms-settings:nightlight"));
        assert_eq!(top("drucker"), Some("ms-settings:printers"));
        assert_eq!(top("nicht stören"), Some("ms-settings:quiethours"));
        assert_eq!(top("datum"), Some("ms-settings:dateandtime"));
        assert_eq!(top("einrastfunktion"), Some("ms-settings:easeofaccess-keyboard"));
        assert_eq!(top("geräte"), Some("ms-settings:connecteddevices"));
    }

    #[test]
    fn titles_follow_the_display_language() {
        let de = search("bluetooth", 1, true);
        assert_eq!(de[0].title, "Bluetooth und Geräte");
        let en = search("bluetooth", 1, false);
        assert_eq!(en[0].title, "Bluetooth & devices");
        assert_eq!(en[0].kind, "settings");
    }

    #[test]
    fn exact_marks_whole_names_and_keywords() {
        assert!(search("night light", 1, false)[0].exact);
        assert!(search("bluetooth", 1, false)[0].exact);
        assert!(!search("blue", 1, false)[0].exact);
    }

    #[test]
    fn noise_finds_nothing() {
        assert!(search("", 5, false).is_empty());
        assert!(search("x", 5, false).is_empty());
        assert!(search("git status", 5, false).is_empty());
        assert!(search("qwertzuiop", 5, false).is_empty());
        assert!(search("'; DROP TABLE --", 5, false).is_empty());
        assert!(search("bluetooth", 0, false).is_empty());
    }
}
