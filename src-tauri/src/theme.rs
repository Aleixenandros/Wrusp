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

/// ¿El tema efectivo es oscuro? Con `System` se pregunta a la ventana, que
/// conoce la preferencia del escritorio.
pub fn is_dark(app: &AppHandle, mode: ThemeMode) -> bool {
    match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => app
            .get_window(shell::MAIN_WINDOW)
            .and_then(|w| w.theme().ok())
            .map(|t| t == tauri::Theme::Dark)
            .unwrap_or(false),
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

/// Aplica el tema actual a la ventana y a las vistas de WhatsApp abiertas.
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
