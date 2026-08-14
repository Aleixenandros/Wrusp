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
    /// Al cerrar la ventana, ¿seguir vivo en la bandeja? Si es `false`, cerrar
    /// termina la aplicación.
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// Avisar con una notificación del escritorio al llegar un mensaje.
    #[serde(default = "default_true")]
    pub notifications: bool,
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
    let Some(dir) = data_config_file() else {
        return;
    };
    let Ok(raw) = fs::read_to_string(dir) else {
        return;
    };
    let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw) else {
        return;
    };
    if cfg.temp_dir.is_empty() {
        return;
    }
    if fs::create_dir_all(&cfg.temp_dir).is_ok() {
        std::env::set_var("TMPDIR", &cfg.temp_dir);
    }
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

/// Interruptores simples de la página de ajustes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toggles {
    pub close_to_tray: bool,
    pub notifications: bool,
}

#[tauri::command]
pub fn get_toggles(state: tauri::State<'_, ConfigState>) -> Toggles {
    let cfg = state.0.lock().unwrap();
    Toggles {
        close_to_tray: cfg.close_to_tray,
        notifications: cfg.notifications,
    }
}

#[tauri::command]
pub fn set_toggle(app: AppHandle, name: String, enabled: bool) -> Result<(), String> {
    let state = app.state::<ConfigState>();
    let mut cfg = state.0.lock().unwrap();
    match name.as_str() {
        "closeToTray" => cfg.close_to_tray = enabled,
        "notifications" => cfg.notifications = enabled,
        other => return Err(format!("Ajuste desconocido: {other}")),
    }
    save(&app, &cfg);
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
/// Usa el portal XDG por línea de órdenes en vez del plugin de diálogos de
/// Tauri para no añadir otra dependencia; si no hay portal, la UI permite
/// escribir la ruta a mano.
#[tauri::command]
pub fn pick_folder() -> Option<String> {
    let out = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory", "--title=Elegir carpeta"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
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
