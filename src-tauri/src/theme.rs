//! Tema claro/oscuro/sistema.
//!
//! - Ventana: `set_theme()` de Tauri (afecta a `prefers-color-scheme`).
//! - Ajustes: la propia página aplica `data-theme` en JS.
//! - WhatsApp Web: la web lee la clave `theme` de localStorage al arrancar, así
//!   que se fija en un script de inicialización y, al cambiarlo en caliente, se
//!   escribe por `eval` y se recarga la vista (la sesión vive en disco, no se
//!   pierde nada).

use crate::config::{ConfigState, ThemeMode};
use crate::runtime::AppHandle;
use crate::shell;
use tauri::Manager;

pub fn to_tauri_theme(mode: ThemeMode) -> Option<tauri::Theme> {
    match mode {
        ThemeMode::System => None,
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::Dark => Some(tauri::Theme::Dark),
    }
}

/// ¿El tema efectivo es oscuro? Con `System`, lo que prefiera el escritorio.
pub fn is_dark(app: &AppHandle, mode: ThemeMode) -> bool {
    match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => system_is_dark(app),
    }
}

// En Linux no se le pregunta a la ventana. Con el tema del sistema,
// `Window::theme()` de tao abre una conexión nueva al bus de sesión y consulta
// el portal de forma síncrona (25 s de plazo para conectar y 5 para la
// respuesta), y aquí se llega desde `refresh_rails`, en el hilo de GTK, con
// cada cambio de no leídos: entrar en un chat. Era el único D-Bus síncrono de
// Wrusp en ese hilo, y el 28-09-2026 la ventana se quedó 30 s sin atender nada
// al entrar en uno (ADR-048). La preferencia la trae `watch_system_theme`
// desde su propio hilo.
#[cfg(target_os = "linux")]
static SYSTEM_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
fn system_is_dark(_app: &AppHandle) -> bool {
    SYSTEM_DARK.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(not(target_os = "linux"))]
fn system_is_dark(app: &AppHandle) -> bool {
    app.get_window(shell::MAIN_WINDOW)
        .and_then(|w| w.theme().ok())
        .map(|t| t == tauri::Theme::Dark)
        .unwrap_or(false)
}

/// Lee la preferencia de tema del escritorio y la sigue, en un hilo propio y
/// con zbus bloqueante, como las notificaciones: la consulta al portal una
/// vez y luego escucha su `SettingChanged`. Si cambia, redibuja las barras.
#[cfg(target_os = "linux")]
pub fn watch_system_theme(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("wrusp-tema".into())
        .spawn(move || {
            if let Err(err) = follow_portal(&app) {
                eprintln!(
                    "wrusp: no se pudo seguir el tema del sistema ({err}); la barra usa el claro"
                );
            }
        });
}

#[cfg(not(target_os = "linux"))]
pub fn watch_system_theme(_app: &AppHandle) {}

#[cfg(target_os = "linux")]
fn follow_portal(app: &AppHandle) -> zbus::Result<()> {
    use zbus::blocking::MessageIterator;
    use zbus::zvariant::OwnedValue;

    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    let conn = crate::mpris::conexion_sesion()?;
    // La suscripción va antes que la lectura, para no perder un cambio que
    // llegue entre las dos.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Settings")?
        .member("SettingChanged")?
        .add_arg(NAMESPACE)?
        .add_arg(KEY)?
        .build();
    let changes = MessageIterator::for_match_rule(rule, &conn, None)?;

    let reply = conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Settings"),
        "Read",
        &(NAMESPACE, KEY),
    )?;
    let (value,): (OwnedValue,) = reply.body().deserialize()?;
    apply_color_scheme(app, &value);

    for msg in changes {
        let Ok(msg) = msg else { continue };
        if let Ok((_, _, value)) = msg.body().deserialize::<(String, String, OwnedValue)>() {
            apply_color_scheme(app, &value);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_color_scheme(app: &AppHandle, value: &zbus::zvariant::Value) {
    use zbus::zvariant::Value;
    // `Read`, el método antiguo del portal, envuelve el valor en otra variante.
    let mut value = value;
    while let Value::Value(inner) = value {
        value = inner;
    }
    // 1 es «prefiere oscuro»; 0 (sin preferencia) y 2, claro. Como tao.
    let dark = matches!(value, Value::U32(1));
    if SYSTEM_DARK.swap(dark, std::sync::atomic::Ordering::Relaxed) != dark {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || shell::refresh_rails(&handle));
    }
}

/// JS que fija la preferencia de tema de WhatsApp Web.
fn whatsapp_theme_js(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "localStorage.setItem('theme', JSON.stringify('dark'));",
        ThemeMode::Light => "localStorage.setItem('theme', JSON.stringify('light'));",
        ThemeMode::System => "localStorage.removeItem('theme');",
    }
}

/// Script que se ejecuta antes de que cargue WhatsApp Web.
pub fn whatsapp_init_script(mode: ThemeMode) -> String {
    format!(
        "(function() {{ try {{ {} }} catch (e) {{ /* sin storage aún */ }} }})();",
        whatsapp_theme_js(mode)
    )
}

#[tauri::command]
pub fn get_theme(state: tauri::State<'_, ConfigState>) -> ThemeMode {
    state.0.lock().unwrap().theme
}

#[tauri::command]
pub fn set_theme(app: AppHandle, theme: ThemeMode) -> Result<(), String> {
    crate::config::mutate(&app, |cfg| {
        cfg.theme = theme;
        Ok(())
    })?;
    apply_theme(&app);
    Ok(())
}

/// Aplica el tema a la ventana, sin tocar las vistas. Es lo que hace falta al
/// arrancar: la vista de cuenta ya nace con el tema puesto por su script de
/// inicialización, y recargarla aquí hacía que la primera cuenta cargase
/// WhatsApp tres veces seguidas (creación, reinicio del proceso web y esta
/// recarga).
pub fn apply_window_theme(app: &AppHandle) {
    let mode = app.state::<ConfigState>().0.lock().unwrap().theme;
    if let Some(window) = app.get_window(shell::MAIN_WINDOW) {
        let _ = window.set_theme(to_tauri_theme(mode));
    }
}

/// Aplica el tema actual a la ventana y a las vistas de WhatsApp abiertas
/// (recargándolas: WhatsApp lee la preferencia al arrancar).
pub fn apply_theme(app: &AppHandle) {
    let mode = app.state::<ConfigState>().0.lock().unwrap().theme;

    let Some(window) = app.get_window(shell::MAIN_WINDOW) else {
        return;
    };
    let _ = window.set_theme(to_tauri_theme(mode));

    let script = format!(
        "(function() {{ try {{ {} location.reload(); }} catch (e) {{}} }})();",
        whatsapp_theme_js(mode)
    );
    for view in window.webviews() {
        if view.label() != shell::SETTINGS_VIEW {
            let _ = view.eval(&script);
        }
    }

    // Los colores de la barra lateral dependen del tema.
    shell::refresh_rails(app);
}
