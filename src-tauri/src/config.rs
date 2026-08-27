//! Modelo de configuración y persistencia en JSON.
//!
//! Todos los datos viven bajo `~/.local/share/wrusp/`: la configuración en
//! `config.json` y los perfiles de webview (sesión de WhatsApp de cada cuenta)
//! en `profiles/<id>/`.

use crate::runtime::AppHandle;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
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
    let state = app.state::<ConfigState>();
    let mut cfg = state.0.lock().unwrap();
    cfg.download_dir = path;
    save(&app, &cfg);
    Ok(())
}

#[tauri::command]
pub fn set_temp_dir(app: AppHandle, path: String) -> Result<(), String> {
    validate_dir(&path)?;
    let state = app.state::<ConfigState>();
    let mut cfg = state.0.lock().unwrap();
    cfg.temp_dir = path;
    save(&app, &cfg);
    Ok(())
}

#[tauri::command]
pub fn set_log_dir(app: AppHandle, path: String) -> Result<(), String> {
    validate_dir(&path)?;
    let state = app.state::<ConfigState>();
    let mut cfg = state.0.lock().unwrap();
    cfg.log_dir = path;
    save(&app, &cfg);
    Ok(())
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
}

#[tauri::command]
pub fn get_toggles(state: tauri::State<'_, ConfigState>) -> Toggles {
    let cfg = state.0.lock().unwrap();
    Toggles {
        close_to_tray: cfg.close_to_tray,
        notifications: cfg.notifications,
        notification_privacy: cfg.notification_privacy,
        autostart: cfg.autostart,
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
    let state = app.state::<ConfigState>();
    let mut cfg = state.0.lock().unwrap();
    match name.as_str() {
        "closeToTray" => cfg.close_to_tray = enabled,
        "notifications" => cfg.notifications = enabled,
        "notificationPrivacy" => cfg.notification_privacy = enabled,
        "autostart" => {
            cfg.autostart = enabled;
            set_autostart_enabled(enabled);
        }
        other => return Err(format!("Ajuste desconocido: {other}")),
    }
    save(&app, &cfg);
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

/// Evita pisar un fichero ya descargado: `foto.jpg` → `foto (2).jpg`.
pub fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    let parent = path.parent().map(PathBuf::from).unwrap_or_default();
    for n in 2..1000 {
        let candidate = parent.join(format!("{stem} ({n}){}", ext.as_deref().unwrap_or("")));
        if !candidate.exists() {
            return candidate;
        }
    }
    path
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
    config_path(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, cfg: &AppConfig) {
    let Some(path) = config_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(cfg) {
        Ok(json) => {
            if let Err(err) = fs::write(&path, json) {
                eprintln!("wrusp: no se pudo guardar la configuración: {err}");
            }
        }
        Err(err) => eprintln!("wrusp: no se pudo serializar la configuración: {err}"),
    }
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

    #[test]
    fn ruta_unica_no_pisa_ficheros() {
        let temp = std::env::temp_dir().join("wrusp-test-unique.txt");
        let _ = fs::write(&temp, b"test");
        let unica = unique_path(temp.clone());
        assert_ne!(unica, temp);
        assert!(unica.display().to_string().contains("(2)"));
        let _ = fs::remove_file(temp);
    }
}
