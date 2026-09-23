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

#[derive(Clone, Serialize, Deserialize)]
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
    /// Dónde y cómo estaba la ventana la última vez.
    #[serde(default)]
    pub window: WindowGeometry,
    /// Corrector ortográfico en la caja de escribir de WhatsApp.
    #[serde(default = "default_true")]
    pub spell_check: bool,
}

/// Posición, tamaño y estado de la ventana principal, para devolverla donde
/// estaba en el arranque siguiente.
///
/// Todo en píxeles **físicos**: son los que hablan los monitores. En logicos
/// no se puede: cada pantalla tiene su factor de escala y la misma coordenada
/// significa sitios distintos según dónde caiga.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct WindowGeometry {
    /// Posición exterior (la que fija el gestor de ventanas).
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    /// Tamaño interior, que es el que se restaura.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Maximizada. Se guarda aparte del tamaño: al maximizar **no** se pisa el
    /// tamaño anterior, o al restaurar la ventana volvería del tamaño de la
    /// pantalla entera y el usuario perdería el suyo.
    #[serde(default)]
    pub maximized: bool,
    /// Lo que el motor añade por su cuenta al tamaño que se le pide: la
    /// sombra de la decoración y la barra de título.
    ///
    /// Medido en este equipo (GNOME sobre Wayland, escala 1): pedir 900×600
    /// devuelve 1070×808, o sea 170 y 208 de más. Sin esto, guardar lo que
    /// informa el motor y volver a pedirlo **hace crecer la ventana en cada
    /// arranque**, que es el fallo clásico de recordar la geometría. Se
    /// aprende sola en cada sesión: depende del tema del escritorio, no de
    /// Wrusp, y puede cambiar sin que cambie nada aquí.
    #[serde(default)]
    pub deco_w: u32,
    #[serde(default)]
    pub deco_h: u32,
}

/// Rectángulo en píxeles físicos: `(x, y, ancho, alto)`.
pub type Rect = (i32, i32, u32, u32);

/// Lo que se solapan dos rectángulos, como `(ancho, alto)` en píxeles.
///
/// Las dos medidas por separado, no el área: una franja de 50 px de ancho por
/// 720 de alto da un área enorme y no deja nada que agarrar con el ratón.
fn overlap(a: Rect, b: Rect) -> (i32, i32) {
    let ancho = (a.0 + a.2 as i32).min(b.0 + b.2 as i32) - a.0.max(b.0);
    let alto = (a.1 + a.3 as i32).min(b.1 + b.3 as i32) - a.1.max(b.1);
    (ancho.max(0), alto.max(0))
}

/// Ancho y alto mínimos que tienen que quedar dentro de una pantalla para dar
/// la posición por buena: un trozo agarrable con el ratón.
const MIN_VISIBLE: (u32, u32) = (200, 80);

/// ¿Se vería la ventana donde se guardó?
///
/// Si se desconecta la pantalla en la que estaba, sus coordenadas quedan
/// apuntando a un sitio que ya no existe y la ventana nace fuera de la vista,
/// sin forma de alcanzarla con el ratón. Cuando eso pasa se conserva el
/// tamaño y se deja que el sistema decida dónde ponerla.
pub fn geometry_on_screen(ventana: Rect, monitores: &[Rect]) -> bool {
    monitores.iter().any(|m| {
        let (ancho, alto) = overlap(ventana, *m);
        ancho >= MIN_VISIBLE.0 as i32 && alto >= MIN_VISIBLE.1 as i32
    })
}

fn default_true() -> bool {
    true
}

/// Los valores de una instalación nueva.
///
/// A mano, y no derivado: derivarlo daba `false` en todo y `""` en el icono,
/// que **no** es lo que dicen los `#[serde(default = …)]` de arriba. Se
/// notaba solo donde no hay `config.json` que leer —una instalación recién
/// hecha, o una a la que se le apartó el fichero por ilegible—: arrancaba sin
/// notificaciones, sin icono elegido y cerrando la ventana en vez de irse a
/// la bandeja, que es justo lo contrario de lo documentado. La prueba de
/// abajo compara los dos caminos para que no puedan volver a separarse.
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            theme: ThemeMode::default(),
            icon: default_icon(),
            download_dir: String::new(),
            temp_dir: String::new(),
            log_dir: String::new(),
            close_to_tray: default_true(),
            notifications: default_true(),
            notification_privacy: false,
            autostart: false,
            save_failed_media: false,
            window: WindowGeometry::default(),
            spell_check: default_true(),
        }
    }
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
/// Variables de entorno que Wrusp fija para los procesos del motor (ver
/// `main.rs` y `logs::init`). Los procesos web las heredan, que es para lo que
/// están; los programas que se abren desde la aplicación no deben: el
/// navegador o el gestor de ficheros, si no estaban abiertos ya, arrancarían
/// con los ajustes de GStreamer y de `malloc` pensados para WebKit (ADR-047).
static VARIABLES_DEL_MOTOR: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// Fija una variable para los procesos del motor, salvo que el usuario traiga
/// la suya, y la anota para no pasársela a nadie más.
pub fn fijar_para_el_motor(variable: &'static str, valor: &str) {
    if std::env::var_os(variable).is_some() {
        return;
    }
    std::env::set_var(variable, valor);
    VARIABLES_DEL_MOTOR.lock().unwrap().push(variable);
}

/// El abridor del escritorio (`xdg-open`), sin las variables del motor.
fn abridor() -> std::process::Command {
    let mut orden = std::process::Command::new(ABRIDOR);
    for variable in VARIABLES_DEL_MOTOR.lock().unwrap().iter() {
        orden.env_remove(variable);
    }
    orden
}

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
    abridor()
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
    pub spell_check: bool,
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
        spell_check: cfg.spell_check,
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
        "spellCheck" => {
            cfg.spell_check = enabled;
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

/// Últimas `lines` líneas del registro, para el visor de Ajustes → Diagnóstico.
///
/// El registro llega a varios megas, así que no se lee entero: se salta al
/// final y se retrocede a trozos hasta juntar los saltos de línea que hagan
/// falta. Leer 5 MB para enseñar 300 líneas es trabajo y memoria por nada.
#[tauri::command]
pub fn get_log_tail(state: tauri::State<'_, ConfigState>, lines: usize) -> Result<String, String> {
    let log_dir = {
        let cfg = state.0.lock().unwrap();
        crate::logs::effective_dir(&cfg.log_dir)
    };
    tail_of_file(&log_dir.join("wrusp.log"), lines)
}

/// Cuánto se lee de golpe al retroceder desde el final del fichero.
const TAIL_CHUNK: u64 = 64 * 1024;
/// Tope duro de líneas, para que la petición de la página no pueda pedir el
/// fichero entero por la puerta de atrás.
const TAIL_MAX_LINES: usize = 2000;

fn tail_of_file(path: &Path, lines: usize) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};

    let lines = lines.clamp(1, TAIL_MAX_LINES);
    let mut fichero =
        fs::File::open(path).map_err(|e| format!("No se pudo abrir el registro: {e}"))?;
    let total = fichero
        .metadata()
        .map_err(|e| format!("No se pudo medir el registro: {e}"))?
        .len();

    let mut desde = total;
    let mut buffer: Vec<u8> = Vec::new();
    // Hacia atrás a trozos, hasta tener una línea de más: la primera puede
    // venir cortada por la mitad y se descarta.
    while desde > 0 && buffer.iter().filter(|b| **b == b'\n').count() <= lines {
        let paso = TAIL_CHUNK.min(desde);
        desde -= paso;
        fichero
            .seek(SeekFrom::Start(desde))
            .map_err(|e| format!("No se pudo recorrer el registro: {e}"))?;
        let mut trozo = vec![0u8; paso as usize];
        fichero
            .read_exact(&mut trozo)
            .map_err(|e| format!("No se pudo leer el registro: {e}"))?;
        trozo.extend_from_slice(&buffer);
        buffer = trozo;
    }

    // El registro trae salida de WebKit y de GStreamer, que no siempre es
    // UTF-8 válido: se sustituye en vez de fallar.
    let texto = String::from_utf8_lossy(&buffer);
    let ultimas: Vec<&str> = texto.lines().rev().take(lines).collect();
    Ok(ultimas.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

/// Informe de diagnóstico en Markdown, listo para pegar en una incidencia.
///
/// Lo que lleva es lo que se pide en cada incidencia de vídeo o de medios, y
/// nada más. **No lleva** nombres de cuentas, identificadores de cuenta,
/// números de teléfono ni una sola línea del registro: son mensajes del
/// usuario. Las rutas del home se acortan a `~`, porque el nombre de la
/// cuenta del sistema suele ser el nombre real de la persona.
#[tauri::command]
pub fn diagnostic_report(app: AppHandle, state: tauri::State<'_, ConfigState>) -> String {
    let diag = get_diagnostics(state.clone());
    let cuentas = state.0.lock().unwrap().accounts.len();
    let ventana = app
        .get_webview_window(crate::shell::MAIN_WINDOW)
        .or_else(|| app.webview_windows().values().next().cloned());
    let pantalla = match ventana {
        Some(w) => match (w.inner_size(), w.scale_factor()) {
            (Ok(size), Ok(escala)) => format!("{}×{} px, escala {escala}", size.width, size.height),
            _ => "desconocida".to_string(),
        },
        None => "sin ventana".to_string(),
    };
    report_markdown(
        &diag,
        cuentas,
        &pantalla,
        &sistema_operativo(),
        &variables_multimedia(),
    )
}

/// Nombre de la distribución, de `/etc/os-release`. Es lo primero que se
/// pregunta en una incidencia de Linux.
fn sistema_operativo() -> String {
    #[cfg(target_os = "linux")]
    {
        let contenido = fs::read_to_string("/etc/os-release").unwrap_or_default();
        for linea in contenido.lines() {
            if let Some(valor) = linea.strip_prefix("PRETTY_NAME=") {
                return valor.trim().trim_matches(['"', '\'']).to_string();
            }
        }
        "Linux".to_string()
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::consts::OS.to_string()
    }
}

/// Las variables del entorno que cambian cómo se decodifica el vídeo. Solo
/// estas: el entorno entero lleva de todo, incluido el nombre del usuario.
fn variables_multimedia() -> Vec<(String, String)> {
    [
        "GST_PLUGIN_FEATURE_RANK",
        "WEBKIT_GST_DISABLE_GL_SINK",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "GST_DEBUG",
        "WRUSP_GUARDAR_MEDIOS_FALLIDOS",
    ]
    .iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_string(), v)))
    .collect()
}

/// Sustituye la carpeta personal por `~`.
///
/// El nombre de la cuenta del sistema suele ser el nombre real de quien la
/// usa, y aparece en todas las rutas que enseña el informe.
fn anonymize_path(texto: &str, home: &str) -> String {
    if home.is_empty() || home == "/" {
        return texto.to_string();
    }
    texto.replace(home, "~")
}

fn megas(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// Arma el informe. Aparte de `diagnostic_report` para poder probarlo sin
/// aplicación ni ventana.
fn report_markdown(
    diag: &SystemDiagnostics,
    cuentas: usize,
    pantalla: &str,
    sistema: &str,
    variables: &[(String, String)],
) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut salida = String::from("### Diagnóstico de Wrusp\n\n");
    let mut fila = |clave: &str, valor: String| {
        salida.push_str(&format!(
            "- **{clave}:** {}\n",
            anonymize_path(&valor, &home)
        ));
    };
    fila("Wrusp", env!("CARGO_PKG_VERSION").to_string());
    fila("Sistema", format!("{sistema} ({})", diag.os_info));
    fila("Motor", diag.webkit_version.clone());
    fila(
        "Decodificador H.264",
        if diag.has_h264_decoder {
            diag.h264_decoder_name.clone()
        } else {
            "no detectado".to_string()
        },
    );
    fila(
        "Decodificador AAC",
        if diag.has_aac_decoder {
            "avdec_aac".to_string()
        } else {
            "no detectado".to_string()
        },
    );
    fila("Ventana", pantalla.to_string());
    fila("Cuentas configuradas", cuentas.to_string());
    fila(
        "En disco",
        format!(
            "perfiles {}, caché de GStreamer {}, registro {}",
            megas(diag.profiles_size),
            megas(diag.gstreamer_cache_size),
            megas(diag.log_size)
        ),
    );
    if variables.is_empty() {
        fila("Variables multimedia", "ninguna".to_string());
    } else {
        let lista = variables
            .iter()
            .map(|(k, v)| format!("`{k}={v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        fila("Variables multimedia", lista);
    }
    salida.push_str(
        "\n_Sin nombres de cuenta, identificadores ni contenido de mensajes. Las rutas personales aparecen como `~`._\n",
    );
    salida
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

/// Idiomas para el corrector ortográfico, sacados del entorno del escritorio.
///
/// El motor los quiere como `es_ES`, que es como se llaman los diccionarios de
/// hunspell. Las variables del sistema traen cosas como
/// `es_ES.UTF-8`, `ca_ES@valencia` o la lista `LANGUAGE=es:en`, así que se
/// limpian y se quitan los repetidos conservando el orden de preferencia.
/// Inglés se añade al final: casi siempre está instalado y es el idioma en el
/// que se escriben la mitad de los mensajes técnicos.
pub fn spell_check_languages() -> Vec<String> {
    let mut crudos = Vec::new();
    // `LANGUAGE` trae una lista de preferencias separada por `:`; el resto,
    // un idioma cada una. La limpieza se encarga de partirlas.
    for variable in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(valor) = std::env::var(variable) {
            crudos.push(valor);
        }
    }
    crudos.push("en_US".to_string());
    normalize_languages(&crudos)
}

/// Limpia una lista de valores de locale y deja los nombres de diccionario.
///
/// Parte también por `:`, que es como `LANGUAGE` separa sus preferencias: así
/// da igual de qué variable venga cada valor.
fn normalize_languages(crudos: &[String]) -> Vec<String> {
    let mut salida: Vec<String> = Vec::new();
    for crudo in crudos.iter().flat_map(|c| c.split(':')) {
        // `es_ES.UTF-8@euro` → `es_ES`; `es-ES` → `es_ES`.
        let limpio = crudo
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .trim()
            .replace('-', "_");
        // `C` y `POSIX` no son idiomas, son la ausencia de uno.
        if limpio.is_empty() || limpio == "C" || limpio == "POSIX" {
            continue;
        }
        if !limpio
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        if !salida.contains(&limpio) {
            salida.push(limpio);
        }
    }
    salida
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
    /// Final del nombre del fichero que le sirve a este sistema, para que la
    /// comprobación de actualizaciones ofrezca el paquete correcto y no una
    /// lista de nueve. Ver `package_suffix`.
    pub package_suffix: String,
    /// Arquitectura de este binario (`x86_64`, `aarch64`…). Dos paquetes
    /// pueden compartir extensión y diferenciarse solo en esto.
    pub arch: String,
}

#[tauri::command]
pub fn get_about() -> About {
    let repo = env!("CARGO_PKG_REPOSITORY").to_string();
    About {
        version: env!("CARGO_PKG_VERSION").to_string(),
        releases: format!("{repo}/releases"),
        issues: format!("{repo}/issues"),
        license: format!("{repo}/blob/main/LICENSE"),
        package_suffix: package_suffix().to_string(),
        arch: std::env::consts::ARCH.to_string(),
        repository: repo,
    }
}

/// Qué paquete de los publicados le sirve a este sistema, por el final de su
/// nombre (`Wrusp-0.4.17-1.x86_64.rpm` → `.rpm`).
///
/// Cada versión publica nueve ficheros y solo uno se instala aquí. Sin esto,
/// ofrecer la descarga sería abrir la lista entera y que el usuario adivine.
/// Si no se reconoce el sistema, no se adivina: quien pregunta se lleva la
/// cadena vacía y ofrece la página de la versión, que los lista todos.
#[cfg(target_os = "linux")]
fn package_suffix() -> &'static str {
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    package_suffix_from(&os_release)
}

#[cfg(target_os = "windows")]
fn package_suffix() -> &'static str {
    "-setup.exe"
}

#[cfg(target_os = "macos")]
fn package_suffix() -> &'static str {
    ".dmg"
}

/// La familia de la distribución, leída de `/etc/os-release`.
///
/// `ID` es la distribución e `ID_LIKE` su familia (una lista separada por
/// espacios): Ubuntu declara `ID=ubuntu` con `ID_LIKE=debian`, y así una
/// derivada que nadie ha visto nunca cae en el paquete de su familia en vez
/// de en el genérico. Los valores pueden venir entrecomillados.
///
/// El AppImage no es el último recurso por comodidad: es el único que funciona
/// sin gestor de paquetes, así que es la respuesta correcta para lo que no se
/// reconoce.
#[cfg(target_os = "linux")]
fn package_suffix_from(os_release: &str) -> &'static str {
    let mut familias = Vec::new();
    for linea in os_release.lines() {
        let Some((clave, valor)) = linea.split_once('=') else {
            continue;
        };
        let clave = clave.trim();
        if clave != "ID" && clave != "ID_LIKE" {
            continue;
        }
        let valor = valor.trim().trim_matches(['"', '\'']);
        familias.extend(valor.split_whitespace().map(str::to_ascii_lowercase));
    }
    // El orden es el del fichero: primero `ID`, que es más específico que la
    // familia, salvo que se hayan escrito al revés.
    for familia in &familias {
        match familia.as_str() {
            "fedora" | "rhel" | "centos" | "almalinux" | "rocky" | "opensuse" | "suse"
            | "mageia" => return ".rpm",
            "debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian" => return ".deb",
            "arch" | "archlinux" | "manjaro" | "endeavouros" | "cachyos" => return ".pkg.tar.zst",
            _ => {}
        }
    }
    ".AppImage"
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
    if let Err(err) = abridor().arg(url.as_str()).spawn() {
        eprintln!("wrusp: no se pudo abrir el enlace: {err}");
    }
}

/// ¿Puede la página de ajustes pedir que se abra esta dirección?
///
/// Restringido al repositorio del proyecto: esa página es nuestra, pero un
/// comando que abra cualquier cosa es una puerta que no hace falta dejar
/// abierta, y por ella pasan direcciones que vienen de la API de GitHub (la
/// descarga de una versión, sus notas).
///
/// Se compara **la dirección entendida**, no su principio. Un `starts_with`
/// sobre `https://github.com/Aleixenandros/Wrusp` aceptaba
/// `…/Wrusp-falso/algo` y `…/Wrusp.evil.com`, que son otro sitio: el nombre
/// del repositorio tiene que acabar donde acaba, y el anfitrión tiene que ser
/// `github.com` exactamente, no algo que termine en eso.
fn external_url_allowed(url: &str) -> bool {
    let Ok(url) = url.parse::<tauri::Url>() else {
        return false;
    };
    if url.scheme() != "https" || url.port_or_known_default() != Some(443) {
        return false;
    }
    // Sin credenciales incrustadas: `https://github.com@otro.sitio/` tiene
    // anfitrión `otro.sitio`, pero aun así no hay por qué llevarlas.
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if url.host_str() != Some("github.com") {
        return false;
    }
    let ruta = url.path();
    ruta == "/Aleixenandros/Wrusp" || ruta.starts_with("/Aleixenandros/Wrusp/")
}

/// Abre una dirección del proyecto desde la página de ajustes.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !external_url_allowed(&url) {
        eprintln!("wrusp: dirección no permitida desde ajustes: {url}");
        return Err("Dirección no permitida".into());
    }
    abridor()
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
    fn las_variables_del_motor_no_llegan_a_los_programas_externos() {
        let quitada = |orden: &std::process::Command, variable: &str| {
            orden
                .get_envs()
                .any(|(clave, valor)| clave == variable && valor.is_none())
        };
        std::env::remove_var("WRUSP_PRUEBA_MOTOR");
        fijar_para_el_motor("WRUSP_PRUEBA_MOTOR", "1");
        assert_eq!(std::env::var("WRUSP_PRUEBA_MOTOR").as_deref(), Ok("1"));
        assert!(quitada(&abridor(), "WRUSP_PRUEBA_MOTOR"));

        // La que ya traía el usuario ni se pisa ni se le quita a nadie.
        std::env::set_var("WRUSP_PRUEBA_USUARIO", "suya");
        fijar_para_el_motor("WRUSP_PRUEBA_USUARIO", "nuestra");
        assert_eq!(std::env::var("WRUSP_PRUEBA_USUARIO").as_deref(), Ok("suya"));
        assert!(!abridor()
            .get_envs()
            .any(|(clave, _)| clave == "WRUSP_PRUEBA_USUARIO"));
    }

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

    /// El paquete que se ofrece al actualizar sale de `/etc/os-release`, y
    /// equivocarse ahí es ofrecerle un `.deb` a quien usa Fedora.
    #[cfg(target_os = "linux")]
    #[test]
    fn el_paquete_ofrecido_es_el_de_la_familia_de_la_distribucion() {
        let casos = [
            (
                "ID=fedora\nVERSION_ID=44\n",
                ".rpm",
                "Fedora, el equipo de desarrollo",
            ),
            ("ID=debian\n", ".deb", "Debian"),
            // Una derivada se reconoce por su familia, no por su nombre.
            (
                "ID=ubuntu\nID_LIKE=debian\n",
                ".deb",
                "Ubuntu declara su familia",
            ),
            (
                "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n",
                ".rpm",
                "los valores pueden venir entrecomillados y la familia traer varios",
            ),
            ("ID=manjaro\nID_LIKE=arch\n", ".pkg.tar.zst", "Manjaro"),
            ("ID=arch\n", ".pkg.tar.zst", "Arch"),
            // Sin familia conocida, el AppImage: es el único que no necesita
            // gestor de paquetes.
            (
                "ID=void\n",
                ".AppImage",
                "una distribución que no está en la lista",
            ),
            ("", ".AppImage", "sin fichero que leer"),
            (
                "PRETTY_NAME=\"Algo\"\n# ID=fedora comentado\n",
                ".AppImage",
                "nada que parsear",
            ),
        ];
        for (os_release, esperado, caso) in casos {
            assert_eq!(package_suffix_from(os_release), esperado, "{caso}");
        }
    }

    /// Sin `config.json` se arranca con `AppConfig::default()`, y eso tiene
    /// que valer lo mismo que leer un JSON vacío: es la misma frase dicha en
    /// dos sitios, y se separaron.
    #[test]
    fn la_configuracion_de_una_instalacion_nueva_es_la_documentada() {
        let por_defecto = AppConfig::default();
        let desde_json: AppConfig = serde_json::from_str("{}").unwrap();

        for (nombre, a, b) in [
            (
                "bandeja al cerrar",
                por_defecto.close_to_tray,
                desde_json.close_to_tray,
            ),
            (
                "notificaciones",
                por_defecto.notifications,
                desde_json.notifications,
            ),
            (
                "privacidad",
                por_defecto.notification_privacy,
                desde_json.notification_privacy,
            ),
            ("autoarranque", por_defecto.autostart, desde_json.autostart),
            (
                "volcado de medios",
                por_defecto.save_failed_media,
                desde_json.save_failed_media,
            ),
            ("corrector", por_defecto.spell_check, desde_json.spell_check),
        ] {
            assert_eq!(a, b, "{nombre}: los dos caminos deben coincidir");
        }
        assert_eq!(por_defecto.icon, desde_json.icon, "icono");
        assert_eq!(
            por_defecto.icon, DEFAULT_ICON,
            "y es el icono elegido por defecto"
        );

        // Y lo que de verdad importa de esos valores:
        assert!(
            por_defecto.close_to_tray,
            "cerrar la ventana deja la app en la bandeja"
        );
        assert!(por_defecto.notifications, "una app de mensajería avisa");
        assert!(por_defecto.spell_check, "el corrector viene puesto");
        assert!(
            !por_defecto.autostart,
            "pero no se cuela en el arranque del equipo"
        );
        assert!(
            !por_defecto.save_failed_media,
            "ni guarda vídeos de nadie sin que se lo pidan"
        );
    }

    /// El visor de ajustes enseña el final del registro, que llega a varios
    /// megas: ni se lee entero ni se devuelve más de lo pedido.
    #[test]
    fn la_cola_del_registro_devuelve_las_ultimas_lineas() {
        let dir = carpeta_de_prueba("cola");
        let path = dir.join("wrusp.log");
        let contenido: String = (1..=500).map(|n| format!("línea {n}\n")).collect();
        fs::write(&path, &contenido).unwrap();

        let cola = tail_of_file(&path, 3).unwrap();
        assert_eq!(cola, "línea 498\nlínea 499\nlínea 500");

        // Más líneas de las que hay: se devuelve lo que hay, entero y en orden.
        let todo = tail_of_file(&path, 10_000).unwrap();
        assert_eq!(todo.lines().count(), 500, "no se pierde ninguna");
        assert!(todo.starts_with("línea 1\n"), "empieza por el principio");

        // Un fichero más grande que el trozo de lectura obliga a retroceder
        // varias veces: es el caso que tiene el registro real.
        let gordo = dir.join("gordo.log");
        let relleno: String = (1..=20_000).map(|n| format!("ruido {n}\n")).collect();
        fs::write(&gordo, &relleno).unwrap();
        assert!(
            relleno.len() as u64 > TAIL_CHUNK,
            "el banco tiene que cruzar un trozo"
        );
        let cola = tail_of_file(&gordo, 2).unwrap();
        assert_eq!(cola, "ruido 19999\nruido 20000");

        assert!(tail_of_file(&dir.join("no-existe.log"), 5).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    /// El informe se pega en incidencias públicas: lo que no puede llevar es
    /// tan importante como lo que lleva.
    #[test]
    fn el_informe_de_diagnostico_no_lleva_datos_personales() {
        let diag = SystemDiagnostics {
            webkit_version: "WebKitGTK 2.52.5".into(),
            has_h264_decoder: true,
            h264_decoder_name: "avdec_h264 (FFmpeg / libavcodec)".into(),
            has_aac_decoder: true,
            gstreamer_cache_size: 1024 * 1024,
            profiles_size: 300 * 1024 * 1024,
            log_size: 5 * 1024 * 1024,
            os_info: "linux x86_64".into(),
        };
        let variables = [(
            "GST_PLUGIN_FEATURE_RANK".to_string(),
            "vah264dec:0".to_string(),
        )];
        let informe = report_markdown(
            &diag,
            2,
            "1100×720 px, escala 1",
            "Fedora Linux 44",
            &variables,
        );

        for esperado in [
            env!("CARGO_PKG_VERSION"),
            "Fedora Linux 44",
            "WebKitGTK 2.52.5",
            "avdec_h264",
            "GST_PLUGIN_FEATURE_RANK",
            "300.0 MiB",
        ] {
            assert!(informe.contains(esperado), "debería informar de {esperado}");
        }
        assert!(!informe.contains('@'), "nada que parezca un correo");

        // La carpeta personal lleva el nombre de quien usa el equipo.
        let con_home = anonymize_path(
            "perfiles en /home/aleixenandros/.local/share/wrusp/profiles",
            "/home/aleixenandros",
        );
        assert_eq!(con_home, "perfiles en ~/.local/share/wrusp/profiles");
        // Sin HOME que sustituir, el texto se queda como está en vez de
        // llenarse de tildes.
        assert_eq!(anonymize_path("/usr/share", ""), "/usr/share");
        assert_eq!(anonymize_path("/usr/share", "/"), "/usr/share");
    }

    /// Los idiomas del corrector salen de variables que traen de todo.
    #[test]
    fn los_idiomas_del_corrector_se_limpian_y_no_se_repiten() {
        let casos: [(&[&str], &[&str], &str); 6] = [
            (&["es_ES.UTF-8"], &["es_ES"], "la codificación sobra"),
            (&["ca_ES@valencia"], &["ca_ES"], "la variante también"),
            (&["es-ES"], &["es_ES"], "el guion se normaliza"),
            (
                &["es:en", "es_ES.UTF-8", "en_US"],
                &["es", "en", "es_ES", "en_US"],
                "se conserva el orden de preferencia y no se repite",
            ),
            (&["C", "POSIX", ""], &[], "eso no son idiomas"),
            (
                &["es_ES; rm -rf /", "en_US"],
                &["en_US"],
                "lo que no parece un idioma no llega al motor",
            ),
        ];
        for (entrada, esperado, caso) in casos {
            let crudos: Vec<String> = entrada.iter().map(|s| (*s).to_string()).collect();
            assert_eq!(normalize_languages(&crudos), esperado, "{caso}");
        }
    }

    /// Una ventana recordada en una pantalla que ya no está no puede
    /// restaurarse donde estaba: nacería fuera de la vista.
    #[test]
    fn la_posicion_guardada_solo_vale_si_se_ve_en_alguna_pantalla() {
        // Portátil (0,0 1920x1080) con una pantalla externa a su derecha.
        let dos = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        let solo_portatil = [(0, 0, 1920, 1080)];

        let casos: [(Rect, &[Rect], bool, &str); 7] = [
            ((100, 100, 1100, 720), &dos, true, "dentro del portátil"),
            ((2200, 300, 1100, 720), &dos, true, "dentro de la externa"),
            (
                (2200, 300, 1100, 720),
                &solo_portatil,
                false,
                "en la externa, que ya no está conectada",
            ),
            (
                (1870, 200, 1100, 720),
                &solo_portatil,
                false,
                "a caballo, pero solo asoman 50 px: no hay de dónde agarrarla",
            ),
            (
                (1600, 200, 1100, 720),
                &solo_portatil,
                true,
                "a caballo con un trozo suficiente dentro",
            ),
            (
                (-1100, 0, 1100, 720),
                &dos,
                false,
                "justo a la izquierda de todo",
            ),
            ((0, 0, 1100, 720), &[], false, "sin pantallas que consultar"),
        ];
        for (ventana, monitores, esperado, caso) in casos {
            assert_eq!(geometry_on_screen(ventana, monitores), esperado, "{caso}");
        }
    }

    /// Por este comando pasan direcciones que vienen de la API de GitHub, así
    /// que la comprobación es la única frontera entre «abrir el proyecto» y
    /// «abrir lo que sea».
    #[test]
    fn open_external_solo_abre_el_repositorio_del_proyecto() {
        let permitidas = [
            "https://github.com/Aleixenandros/Wrusp",
            "https://github.com/Aleixenandros/Wrusp/releases",
            "https://github.com/Aleixenandros/Wrusp/releases/tag/v0.4.17",
            "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.17/Wrusp-0.4.17-1.x86_64.rpm",
            "https://github.com/Aleixenandros/Wrusp/blob/main/LICENSE",
        ];
        for url in permitidas {
            assert!(external_url_allowed(url), "debería permitirse: {url}");
        }

        let prohibidas = [
            // Las dos que colaba el `starts_with` anterior: otro repositorio y
            // otro anfitrión que empiezan igual.
            "https://github.com/Aleixenandros/Wrusp-falso/releases",
            "https://github.com/Aleixenandros/Wruspicito",
            "https://github.com.evil.example/Aleixenandros/Wrusp",
            "https://github.com@otro.example/Aleixenandros/Wrusp",
            // Otro dueño, otro esquema, otro puerto.
            "https://github.com/otro/Wrusp/releases",
            "http://github.com/Aleixenandros/Wrusp",
            "https://github.com:8443/Aleixenandros/Wrusp",
            // Esquemas que no deberían llegar nunca al escritorio.
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<h1>hola",
            "",
            "no es una dirección",
        ];
        for url in prohibidas {
            assert!(!external_url_allowed(url), "no debería permitirse: {url}");
        }
    }

    /// El nombre de los ficheros publicados tiene que seguir acabando como
    /// dice `package_suffix`, o la descarga que se ofrece no existirá.
    #[cfg(target_os = "linux")]
    #[test]
    fn los_sufijos_casan_con_los_nombres_que_publica_el_empaquetado() {
        let publicados = [
            "Wrusp-0.4.16-1.x86_64.rpm",
            "Wrusp_0.4.16_amd64.deb",
            "Wrusp-0.4.16-1-x86_64.pkg.tar.zst",
            "Wrusp_0.4.16_amd64.AppImage",
        ];
        for sufijo in [".rpm", ".deb", ".pkg.tar.zst", ".AppImage"] {
            let casan = publicados.iter().filter(|n| n.ends_with(sufijo)).count();
            assert_eq!(
                casan, 1,
                "{sufijo} tiene que casar con un fichero y solo uno"
            );
        }
    }
}
