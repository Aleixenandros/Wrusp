//! Modelo de configuración y persistencia en JSON.
//!
//! Todos los datos viven bajo `~/.local/share/wrusp/`: la configuración en
//! `config.json` y los perfiles de webview (sesión de WhatsApp de cada cuenta)
//! en `profiles/<id>/`.

use crate::runtime::AppHandle;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::Manager;

fn default_zoom() -> f64 {
    1.0
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    /// Factor de zoom de la vista, recordado por cuenta.
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    /// Color de acento personalizado para la barra lateral (ej. "#1fa855").
    #[serde(default)]
    pub color: Option<String>,
    /// ¿Notificaciones silenciadas para esta cuenta?
    #[serde(default)]
    pub muted: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Icono por defecto de la aplicación (decisión del usuario: el naranja).
pub const DEFAULT_ICON: &str = "whatsapp-logo-2449-orange";

fn default_icon() -> String {
    DEFAULT_ICON.to_string()
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub theme: ThemeMode,
    /// Nombre (sin extensión) del icono elegido dentro de `appicons/`.
    #[serde(default = "default_icon")]
    pub icon: String,
    /// Carpeta de descargas. Vacío = la del sistema (XDG_DOWNLOAD_DIR).
    #[serde(default)]
    pub download_dir: String,
    /// Carpeta de temporales. Vacío = la del sistema (TMPDIR, normalmente /tmp).
    #[serde(default)]
    pub temp_dir: String,
    /// Carpeta de registros. Vacío = XDG state (~/.local/state/wrusp/logs).
    #[serde(default)]
    pub log_dir: String,
    /// Al cerrar la ventana, ¿seguir vivo en la bandeja? Si es `false`, cerrar
    /// termina la aplicación.
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// Avisar con una notificación del escritorio al llegar un mensaje.
    #[serde(default = "default_true")]
    pub notifications: bool,
    /// Ocultar contenido del mensaje en notificaciones para mayor privacidad.
    #[serde(default)]
    pub notification_privacy: bool,
    /// Iniciar automáticamente al encender el equipo.
    #[serde(default)]
    pub autostart: bool,
    /// Diagnóstico: guardar en la carpeta de registros los vídeos que no se
    /// reproducen (hasta cinco por sesión), para poder analizarlos.
    #[serde(default)]
    pub save_failed_media: bool,
}

fn default_true() -> bool {
    true
}

/// Carpetas efectivas mostradas en ajustes: el valor configurado y el que se
/// usa de verdad cuando ese valor está vacío.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folders {
    pub download_dir: String,
    pub download_default: String,
    pub temp_dir: String,
    pub temp_default: String,
    pub log_dir: String,
    pub log_default: String,
}

fn system_download_dir() -> PathBuf {
    // `xdg-user-dir` respeta la carpeta traducida del escritorio; si no está,
    // se cae a ~/Descargas... que no existe en todos los idiomas, así que el
    // último recurso es el propio home.
    if let Ok(out) = std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .unwrap_or_else(std::env::temp_dir)
}

/// Carpeta de descargas efectiva (configurada o la del sistema).
pub fn download_dir(app: &AppHandle) -> PathBuf {
    let configured = app
        .state::<ConfigState>()
        .0
        .lock()
        .unwrap()
        .download_dir
        .clone();
    if configured.is_empty() {
        system_download_dir()
    } else {
        PathBuf::from(configured)
    }
}

/// Exporta `TMPDIR` con la carpeta de temporales configurada.
///
/// Se llama antes de construir la app: WebKit lee `TMPDIR` al lanzar sus
/// procesos auxiliares, así que cambiarlo después no tendría efecto. Por eso
/// lee el JSON directamente en vez de usar el estado de Tauri, que aún no
/// existe.
pub fn apply_temp_dir_env() {
    let Some(cfg) = load_from_disk() else {
        return;
    };
    if cfg.temp_dir.is_empty() {
        return;
    }
    if fs::create_dir_all(&cfg.temp_dir).is_ok() {
        std::env::set_var("TMPDIR", &cfg.temp_dir);
    }
}

/// Configuración leída directamente del disco, para lo que corre antes de que
/// exista la aplicación de Tauri (temporales, registros).
pub fn load_from_disk() -> Option<AppConfig> {
    let raw = fs::read_to_string(data_config_file()?).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Identificador del bundle. Tauri nombra con él los directorios de datos y de
/// configuración, así que **debe coincidir con `identifier` de
/// tauri.conf.json**; `debug_assert_identifier` lo comprueba al arrancar.
pub const APP_IDENTIFIER: &str = "wrusp";

/// Raíz de los perfiles de webview, uno por cuenta. Se resuelve sin pasar por
/// Tauri para poder usarla antes de que exista la aplicación.
pub fn profiles_root_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_IDENTIFIER)
        .join("profiles")
}

/// Aborta en depuración si el identificador de arriba se desincroniza del real.
/// Sin esto, `apply_temp_dir_env` leería un fichero que no existe y la carpeta
/// de temporales se ignoraría en silencio (ya pasó una vez).
pub fn debug_assert_identifier(app: &AppHandle) {
    debug_assert_eq!(
        app.config().identifier,
        APP_IDENTIFIER,
        "APP_IDENTIFIER no coincide con tauri.conf.json"
    );
    let _ = app;
}

/// Ruta del config.json sin pasar por Tauri (ver `apply_temp_dir_env`).
fn data_config_file() -> Option<PathBuf> {
    dirs::data_dir().map(|base| base.join(APP_IDENTIFIER).join("config.json"))
}

#[tauri::command]
pub fn get_folders(state: tauri::State<'_, ConfigState>) -> Folders {
    let cfg = state.0.lock().unwrap();
    Folders {
        download_dir: cfg.download_dir.clone(),
        download_default: system_download_dir().display().to_string(),
        temp_dir: cfg.temp_dir.clone(),
        temp_default: std::env::temp_dir().display().to_string(),
        log_dir: cfg.log_dir.clone(),
        log_default: crate::logs::default_dir().display().to_string(),
    }
}

/// Valida que la ruta sea utilizable como destino: absoluta y escribible.
fn validate_dir(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Ok(()); // vacío = usar la del sistema
    }
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err("La ruta debe ser absoluta".into());
    }
    fs::create_dir_all(&p).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    let probe = p.join(".wrusp-write-test");
    fs::write(&probe, b"").map_err(|e| format!("La carpeta no es escribible: {e}"))?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

#[tauri::command]
pub fn set_download_dir(app: AppHandle, path: String) -> Result<(), String> {
    validate_dir(&path)?;
    mutate(&app, |cfg| {
        cfg.download_dir = path;
        Ok(())
    })
}

#[tauri::command]
pub fn set_temp_dir(app: AppHandle, path: String) -> Result<(), String> {
    validate_dir(&path)?;
    mutate(&app, |cfg| {
        cfg.temp_dir = path;
        Ok(())
    })
}

#[tauri::command]
pub fn set_log_dir(app: AppHandle, path: String) -> Result<(), String> {
    validate_dir(&path)?;
    mutate(&app, |cfg| {
        cfg.log_dir = path;
        Ok(())
    })
}

/// Abre la carpeta de registros en el gestor de ficheros.
#[tauri::command]
pub fn open_log_dir(state: tauri::State<'_, ConfigState>) -> Result<(), String> {
    let dir = crate::logs::effective_dir(&state.0.lock().unwrap().log_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    std::process::Command::new(ABRIDOR)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No se pudo abrir la carpeta: {e}"))
}

/// Interruptores simples de la página de ajustes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toggles {
    pub close_to_tray: bool,
    pub notifications: bool,
    pub notification_privacy: bool,
    pub autostart: bool,
    pub save_failed_media: bool,
}

#[tauri::command]
pub fn get_toggles(state: tauri::State<'_, ConfigState>) -> Toggles {
    let cfg = state.0.lock().unwrap();
    Toggles {
        close_to_tray: cfg.close_to_tray,
        notifications: cfg.notifications,
        notification_privacy: cfg.notification_privacy,
        autostart: cfg.autostart,
        save_failed_media: cfg.save_failed_media,
    }
}

#[cfg(target_os = "linux")]
fn autostart_desktop_file() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("autostart").join("wrusp.desktop"))
}

#[cfg(target_os = "linux")]
pub fn set_autostart_enabled(enabled: bool) {
    let Some(path) = autostart_desktop_file() else {
        return;
    };
    if enabled {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "wrusp".into());
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Wrusp\nComment=Cliente no oficial de WhatsApp\nExec=\"{exe}\" --hidden\nIcon=wrusp\nTerminal=false\nCategories=Network;InstantMessaging;\n"
        );
        let _ = fs::write(path, desktop);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn set_autostart_enabled(_enabled: bool) {}

#[tauri::command]
pub fn set_toggle(app: AppHandle, name: String, enabled: bool) -> Result<(), String> {
    mutate(&app, |cfg| match name.as_str() {
        "closeToTray" => {
            cfg.close_to_tray = enabled;
            Ok(())
        }
        "notifications" => {
            cfg.notifications = enabled;
            Ok(())
        }
        "notificationPrivacy" => {
            cfg.notification_privacy = enabled;
            Ok(())
        }
        "autostart" => {
            cfg.autostart = enabled;
            Ok(())
        }
        "saveFailedMedia" => {
            cfg.save_failed_media = enabled;
            Ok(())
        }
        other => Err(format!("Ajuste desconocido: {other}")),
    })?;
    // El fichero de autoarranque se toca solo cuando el ajuste ya quedó
    // persistido; si el guardado falla, escritorio y configuración no divergen.
    if name == "autostart" {
        set_autostart_enabled(enabled);
    }
    Ok(())
}

/// Diagnóstico del sistema y estado de los componentes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDiagnostics {
    pub webkit_version: String,
    pub has_h264_decoder: bool,
    pub h264_decoder_name: String,
    pub has_aac_decoder: bool,
    pub gstreamer_cache_size: u64,
    pub profiles_size: u64,
    pub log_size: u64,
    pub os_info: String,
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += dir_size(&entry.path());
                }
            }
        }
    }
    total
}

#[tauri::command]
pub fn get_diagnostics(state: tauri::State<'_, ConfigState>) -> SystemDiagnostics {
    let cfg = state.0.lock().unwrap();

    let mut has_h264 = false;
    let mut h264_name = "No detectado".to_string();
    let mut has_aac = false;

    // Comprobar con gst-inspect-1.0 si está disponible
    if let Ok(out) = std::process::Command::new("gst-inspect-1.0")
        .arg("avdec_h264")
        .output()
    {
        if out.status.success() {
            has_h264 = true;
            h264_name = "avdec_h264 (FFmpeg / libavcodec)".to_string();
        }
    }
    if !has_h264 {
        if let Ok(out) = std::process::Command::new("gst-inspect-1.0")
            .arg("openh264dec")
            .output()
        {
            if out.status.success() {
                has_h264 = true;
                h264_name = "openh264dec (Baseline)".to_string();
            }
        }
    }
    if let Ok(out) = std::process::Command::new("gst-inspect-1.0")
        .arg("avdec_aac")
        .output()
    {
        if out.status.success() {
            has_aac = true;
        }
    }

    // Tamaño de caché de GStreamer
    let gst_cache_dir = dirs::cache_dir()
        .map(|c| c.join("gstreamer-1.0"))
        .unwrap_or_default();
    let gst_cache_size = dir_size(&gst_cache_dir);

    // Tamaño de perfiles
    let prof_dir = profiles_root_dir();
    let profiles_size = dir_size(&prof_dir);

    // Tamaño de logs
    let log_path = crate::logs::effective_dir(&cfg.log_dir).join("wrusp.log");
    let log_size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);

    #[cfg(target_os = "linux")]
    let webkit_version = unsafe {
        format!(
            "WebKitGTK {}.{}.{}",
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
            webkit2gtk::ffi::webkit_get_micro_version()
        )
    };
    #[cfg(not(target_os = "linux"))]
    let webkit_version = "Nativo de la plataforma".to_string();

    let os_info = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);

    SystemDiagnostics {
        webkit_version,
        has_h264_decoder: has_h264,
        h264_decoder_name: h264_name,
        has_aac_decoder: has_aac,
        gstreamer_cache_size: gst_cache_size,
        profiles_size,
        log_size,
        os_info,
    }
}

/// Borra los ficheros de caché del registro de GStreamer para forzar su reescaneo al reiniciar.
#[tauri::command]
pub fn clear_gstreamer_cache() -> Result<(), String> {
    let Some(gst_cache_dir) = dirs::cache_dir().map(|c| c.join("gstreamer-1.0")) else {
        return Err("No se encontró la carpeta de caché".into());
    };
    if !gst_cache_dir.exists() {
        return Ok(());
    }
    if let Ok(entries) = fs::read_dir(&gst_cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with("registry."))
                .unwrap_or(false)
            {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

/// Resultado de intentar reservar un destino en exclusiva.
enum Reserva {
    /// El fichero se creó vacío: el nombre es nuestro.
    Hecha,
    /// Ya existe: probar el siguiente.
    Ocupada,
    /// El sistema de ficheros no deja reservar (permisos, carpeta ausente…):
    /// no tiene sentido seguir probando nombres.
    Imposible,
}

fn reservar(path: &Path) -> Reserva {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Reserva::Hecha,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Reserva::Ocupada,
        Err(_) => Reserva::Imposible,
    }
}

/// Evita pisar un fichero ya descargado: `foto.jpg` → `foto (2).jpg`.
///
/// El nombre no se elige mirando si existe —dos descargas a la vez pasaban
/// ambas esa comprobación y acababan en el mismo fichero—: se **reserva**
/// creándolo vacío en exclusiva (`create_new`), que es una sola operación
/// atómica del sistema de ficheros. La descarga escribe después encima de su
/// reserva. Y nunca se vuelve a una ruta ocupada: si se agotan los sufijos
/// numerados, el último recurso es un sufijo de reloj, no pisar la original.
pub fn unique_path(path: PathBuf) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    let ext = ext.as_deref().unwrap_or("");
    let parent = path.parent().map(PathBuf::from).unwrap_or_default();

    match reservar(&path) {
        Reserva::Hecha => return path,
        Reserva::Imposible => return path, // el error real saldrá al escribir
        Reserva::Ocupada => {}
    }
    for n in 2..1000 {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        match reservar(&candidate) {
            Reserva::Hecha => return candidate,
            Reserva::Imposible => return candidate,
            Reserva::Ocupada => {}
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let candidate = parent.join(format!("{stem} ({nanos}){ext}"));
    let _ = reservar(&candidate);
    candidate
}

/// Datos del «Acerca de».
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub version: String,
    pub repository: String,
    pub releases: String,
    pub issues: String,
    pub license: String,
}

#[tauri::command]
pub fn get_about() -> About {
    let repo = env!("CARGO_PKG_REPOSITORY").to_string();
    About {
        version: env!("CARGO_PKG_VERSION").to_string(),
        releases: format!("{repo}/releases"),
        issues: format!("{repo}/issues"),
        license: format!("{repo}/blob/main/LICENSE"),
        repository: repo,
    }
}

/// Programa que abre direcciones en cada sistema.
#[cfg(target_os = "linux")]
const ABRIDOR: &str = "xdg-open";
#[cfg(target_os = "macos")]
const ABRIDOR: &str = "open";
#[cfg(target_os = "windows")]
const ABRIDOR: &str = "explorer";

/// Abre en el navegador del sistema un enlace de un chat.
///
/// Solo `http` y `https`: la URL viene de una página remota, así que esquemas
/// como `file://` o `javascript:` no deben llegar nunca al escritorio.
pub fn open_in_browser(url: &tauri::Url) {
    if !matches!(url.scheme(), "http" | "https") {
        eprintln!("wrusp: enlace ignorado por su esquema: {url}");
        return;
    }
    if let Err(err) = std::process::Command::new(ABRIDOR)
        .arg(url.as_str())
        .spawn()
    {
        eprintln!("wrusp: no se pudo abrir el enlace: {err}");
    }
}

/// Abre una dirección desde la página de ajustes.
///
/// Restringido al propio proyecto: esa página es nuestra, pero un comando que
/// abra cualquier cosa es una puerta que no hace falta dejar abierta.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/Aleixenandros/Wrusp") {
        return Err("Dirección no permitida".into());
    }
    std::process::Command::new(ABRIDOR)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No se pudo abrir el navegador: {e}"))
}

/// Abre el selector de carpetas del escritorio y devuelve la ruta elegida.
///
/// Prueba en cascada: zenity (GTK), kdialog (KDE/Qt) o qarma; si no hay
/// selector disponible, la UI permite escribir la ruta a mano.
#[tauri::command]
pub fn pick_folder() -> Option<String> {
    // 1. Zenity (GNOME / GTK)
    if let Ok(out) = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory", "--title=Elegir carpeta"])
        .output()
    {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    // 2. Kdialog (KDE / Qt)
    if let Ok(out) = std::process::Command::new("kdialog")
        .args(["--getexistingdirectory", "--title", "Elegir carpeta"])
        .output()
    {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    // 3. Qarma (clon de zenity en Qt)
    if let Ok(out) = std::process::Command::new("qarma")
        .args(["--file-selection", "--directory", "--title=Elegir carpeta"])
        .output()
    {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

/// Estado global gestionado por Tauri.
pub struct ConfigState(pub Mutex<AppConfig>);

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("config.json"))
}

pub fn profiles_dir(app: &AppHandle) -> PathBuf {
    let path = profiles_root_dir();
    debug_assert_eq!(
        app.path()
            .app_data_dir()
            .ok()
            .map(|dir| dir.join("profiles")),
        Some(path.clone()),
        "la raíz manual de perfiles no coincide con la de Tauri"
    );
    path
}

pub fn load(app: &AppHandle) -> AppConfig {
    let Some(path) = config_path(app) else {
        return AppConfig::default();
    };
    let mut cfg = load_config_file(&path);
    sanitize_accounts(&mut cfg.accounts);
    cfg
}

/// Lee la configuración del disco. Un fichero ausente es la primera ejecución;
/// uno ilegible se aparta a `config.json.corrupto` en vez de tratarlo como
/// vacío: es la única copia de las cuentas del usuario, y el siguiente guardado
/// la pisaría sin que nadie se enterase.
fn load_config_file(path: &Path) -> AppConfig {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return AppConfig::default(),
    };
    match serde_json::from_str(&raw) {
        Ok(cfg) => cfg,
        Err(err) => {
            let rescate = unique_path(path.with_extension("json.corrupto"));
            match fs::rename(path, &rescate) {
                Ok(()) => eprintln!(
                    "wrusp: config.json ilegible ({err}); se conserva en {} y se arranca con la configuración por defecto",
                    rescate.display()
                ),
                Err(e) => eprintln!(
                    "wrusp: config.json ilegible ({err}) y no se pudo apartar ({e}); se arranca con la configuración por defecto"
                ),
            }
            AppConfig::default()
        }
    }
}

/// Descarta cuentas cuyo id no sea un UUID canónico o repita uno anterior.
///
/// El id nombra el directorio del perfil (`profiles/<id>`), así que un id
/// manipulado con `../` o una ruta absoluta podría sacar el borrado de una
/// cuenta fuera de `profiles/`; los duplicados romperían el aislamiento de
/// sesiones. La app siempre generó UUIDs, de modo que aquí solo cae lo que
/// alguien haya editado a mano en config.json.
fn sanitize_accounts(accounts: &mut Vec<Account>) {
    let mut vistos = std::collections::HashSet::new();
    accounts.retain(|a| {
        if !valid_account_id(&a.id) {
            eprintln!(
                "wrusp: cuenta {:?} descartada: su id {:?} no es un UUID válido",
                a.name, a.id
            );
            return false;
        }
        if !vistos.insert(a.id.clone()) {
            eprintln!(
                "wrusp: cuenta {:?} descartada: id {:?} duplicado",
                a.name, a.id
            );
            return false;
        }
        true
    });
}

/// ¿Es el id un UUID en su forma canónica (minúsculas, con guiones)?
/// Es la única forma que genera `add_account`, y no contiene separadores de
/// ruta ni nada que pueda escapar de `profiles/`.
pub fn valid_account_id(id: &str) -> bool {
    uuid::Uuid::try_parse(id).is_ok_and(|u| u.as_hyphenated().to_string() == id)
}

/// Ruta del perfil de una cuenta, validando el id y que el resultado quede
/// confinado como hijo directo de la raíz de perfiles.
pub fn profile_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    confined_profile_path(&profiles_dir(app), id)
}

fn confined_profile_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_account_id(id) {
        return Err(format!("Identificador de cuenta no válido: {id:?}"));
    }
    let path = root.join(id);
    // Con el id ya validado no puede escapar, pero comprobarlo cuesta poco y
    // aguanta aunque la validación de arriba cambie algún día.
    if path.parent() != Some(root) || path.file_name() != Some(std::ffi::OsStr::new(id)) {
        return Err(format!("Ruta de perfil fuera de profiles/: {id:?}"));
    }
    Ok(path)
}

pub fn save(app: &AppHandle, cfg: &AppConfig) -> Result<(), String> {
    let path =
        config_path(app).ok_or_else(|| "No se pudo resolver la carpeta de datos".to_string())?;
    write_config_atomic(&path, cfg)
}

/// Escribe la configuración a un temporal del mismo directorio, lo sincroniza
/// y lo renombra sobre el definitivo. Un cierre a mitad de escritura deja el
/// `config.json` anterior intacto, nunca uno truncado.
fn write_config_atomic(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("No se pudo serializar la configuración: {e}"))?;
    let dir = path
        .parent()
        .ok_or_else(|| "Ruta de configuración sin carpeta".to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta de datos: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    let escribir = |tmp: &Path| -> io::Result<()> {
        let mut f = fs::File::create(tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()
    };
    escribir(&tmp).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("No se pudo guardar la configuración: {e}")
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("No se pudo guardar la configuración: {e}")
    })
}

/// Aplica una mutación a la configuración y la persiste. Si la mutación o la
/// escritura fallan, la configuración en memoria vuelve a como estaba: lo que
/// la UI da por hecho y lo que hay en disco no se separan.
pub fn mutate<R>(
    app: &AppHandle,
    f: impl FnOnce(&mut AppConfig) -> Result<R, String>,
) -> Result<R, String> {
    let state = app.state::<ConfigState>();
    let mut cfg = state.0.lock().unwrap();
    let backup = cfg.clone();
    let out = match f(&mut cfg) {
        Ok(out) => out,
        Err(err) => {
            *cfg = backup;
            return Err(err);
        }
    };
    if let Err(err) = save(app, &cfg) {
        *cfg = backup;
        return Err(err);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializar_config_antigua_con_valores_por_defecto() {
        let json_antiguo = r#"{
            "accounts": [
                { "id": "123", "name": "Personal", "zoom": 1.0 }
            ],
            "theme": "system",
            "close_to_tray": true,
            "notifications": true
        }"#;

        let cfg: AppConfig = serde_json::from_str(json_antiguo).expect("deserializar");
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(cfg.accounts[0].color, None);
        assert!(!cfg.accounts[0].muted);
        assert!(!cfg.notification_privacy);
        assert!(!cfg.autostart);
    }

    #[test]
    fn serializar_y_deserializar_cuenta_completa() {
        let cuenta = Account {
            id: "abc".into(),
            name: "Trabajo".into(),
            zoom: 1.2,
            color: Some("#3b82f6".into()),
            muted: true,
        };
        let raw = serde_json::to_string(&cuenta).unwrap();
        let vuelta: Account = serde_json::from_str(&raw).unwrap();
        assert_eq!(vuelta.color, Some("#3b82f6".into()));
        assert!(vuelta.muted);
        assert_eq!(vuelta.zoom, 1.2);
    }

    /// Carpeta temporal propia de cada test: se ejecutan en paralelo y no
    /// deben pisarse entre sí.
    fn carpeta_de_prueba(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wrusp-test-{nombre}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crear carpeta de prueba");
        dir
    }

    #[test]
    fn ruta_unica_no_pisa_ficheros() {
        let dir = carpeta_de_prueba("unique");
        let temp = dir.join("fichero.txt");
        fs::write(&temp, b"test").unwrap();
        let unica = unique_path(temp.clone());
        assert_ne!(unica, temp);
        assert!(unica.display().to_string().contains("(2)"));
        assert_eq!(
            fs::read(&temp).unwrap(),
            b"test",
            "el original queda intacto"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn descargas_concurrentes_reciben_destinos_distintos() {
        let dir = carpeta_de_prueba("unique-concurrente");
        fs::write(dir.join("descarga.pdf"), b"contenido previo").unwrap();

        let mut hilos = Vec::new();
        for _ in 0..8 {
            let destino = dir.join("descarga.pdf");
            hilos.push(std::thread::spawn(move || unique_path(destino)));
        }
        let rutas: Vec<PathBuf> = hilos.into_iter().map(|h| h.join().unwrap()).collect();

        let unicas: std::collections::HashSet<&PathBuf> = rutas.iter().collect();
        assert_eq!(
            unicas.len(),
            rutas.len(),
            "cada descarga con su ruta: {rutas:?}"
        );
        assert!(!rutas.contains(&dir.join("descarga.pdf")));
        assert_eq!(
            fs::read(dir.join("descarga.pdf")).unwrap(),
            b"contenido previo",
            "nadie pisa el fichero preexistente"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ids_de_cuenta_solo_uuid_canonico() {
        assert!(valid_account_id("3f0a2b9c-1234-4abc-8def-0123456789ab"));

        assert!(!valid_account_id(""));
        assert!(!valid_account_id("../otro"));
        assert!(!valid_account_id("/etc/passwd"));
        assert!(!valid_account_id("3F0A2B9C-1234-4ABC-8DEF-0123456789AB")); // mayúsculas
        assert!(!valid_account_id("{3f0a2b9c-1234-4abc-8def-0123456789ab}")); // con llaves
        assert!(!valid_account_id("3f0a2b9c12344abc8def0123456789ab")); // sin guiones
    }

    #[test]
    fn el_saneado_descarta_ids_invalidos_y_duplicados() {
        let cuenta = |id: &str, name: &str| Account {
            id: id.into(),
            name: name.into(),
            zoom: 1.0,
            color: None,
            muted: false,
        };
        let valida = "3f0a2b9c-1234-4abc-8def-0123456789ab";
        let mut accounts = vec![
            cuenta(valida, "buena"),
            cuenta(valida, "duplicada"),
            cuenta("../fuera", "escapista"),
            cuenta("/etc", "absoluta"),
            cuenta("", "vacía"),
        ];
        sanitize_accounts(&mut accounts);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "buena");
    }

    #[test]
    fn la_ruta_de_perfil_queda_confinada() {
        let root = carpeta_de_prueba("perfiles");
        let id = "3f0a2b9c-1234-4abc-8def-0123456789ab";

        let ruta = confined_profile_path(&root, id).expect("UUID válido");
        assert_eq!(ruta, root.join(id));
        assert_eq!(ruta.parent(), Some(root.as_path()));

        assert!(confined_profile_path(&root, "../fuera").is_err());
        assert!(confined_profile_path(&root, "/etc/passwd").is_err());
        assert!(confined_profile_path(&root, "").is_err());
        assert!(confined_profile_path(&root, "no-un-uuid").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn guardado_atomico_y_recarga() {
        let dir = carpeta_de_prueba("guardado");
        let path = dir.join("config.json");
        let cfg = AppConfig {
            download_dir: "/tmp/descargas".into(),
            ..Default::default()
        };

        write_config_atomic(&path, &cfg).expect("guardar");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "sin temporal residual"
        );

        let releida = load_config_file(&path);
        assert_eq!(releida.download_dir, "/tmp/descargas");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn json_corrupto_se_aparta_en_vez_de_perderse() {
        let dir = carpeta_de_prueba("corrupto");
        let path = dir.join("config.json");
        fs::write(&path, "{\"accounts\": [").unwrap(); // truncado

        let cfg = load_config_file(&path);
        assert!(cfg.accounts.is_empty(), "arranca con defaults");
        assert!(!path.exists(), "el corrupto ya no está en su sitio");
        let rescate = dir.join("config.json.corrupto");
        assert_eq!(
            fs::read_to_string(&rescate).unwrap(),
            "{\"accounts\": [",
            "…porque se apartó entero para poder recuperarlo"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
