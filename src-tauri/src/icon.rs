//! Icono de aplicación seleccionable.
//!
//! Los PNG (256 px, generados desde `assets/appicons/*.svg`) viajan como
//! recursos del bundle bajo `appicons/`. El elegido se aplica en caliente al
//! tray y a todas las ventanas, y se persiste en la configuración.

use crate::config::{ConfigState, DEFAULT_ICON};
use crate::runtime::AppHandle;
use tauri::{image::Image, path::BaseDirectory, Manager};

/// Carga el PNG del icono `name` desde los recursos del bundle.
/// Los nombres vienen de la UI; se valida que sea un nombre simple
/// (sin separadores) para no salir del directorio de recursos.
fn load(app: &AppHandle, name: &str) -> Option<Image<'static>> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let path = app
        .path()
        .resolve(format!("appicons/{name}.png"), BaseDirectory::Resource)
        .ok()?;
    let bytes = std::fs::read(path).ok()?;
    Image::from_bytes(&bytes).ok().map(|img| img.to_owned())
}

/// Icono configurado, con degradación al de por defecto del bundle.
pub fn current(app: &AppHandle) -> Option<Image<'static>> {
    let name = app.state::<ConfigState>().0.lock().unwrap().icon.clone();
    load(app, &name)
        .or_else(|| load(app, DEFAULT_ICON))
        .or_else(|| {
            app.default_window_icon()
                .map(|icon| icon.clone().to_owned())
        })
}

use std::sync::Mutex;

static LAST_APPLIED: Mutex<Option<(String, u32)>> = Mutex::new(None);

/// Aplica el icono configurado, con la insignia de no leídos si procede, a la
/// bandeja y a la ventana (que es lo que ve la barra de tareas).
pub fn apply(app: &AppHandle) {
    let icon_name = app.state::<ConfigState>().0.lock().unwrap().icon.clone();
    let unread = crate::shell::total_unread(app);

    {
        let mut last = LAST_APPLIED.lock().unwrap();
        if let Some((prev_name, prev_unread)) = last.as_ref() {
            if *prev_name == icon_name && *prev_unread == unread {
                return;
            }
        }
        *last = Some((icon_name.clone(), unread));
    }

    let Some(base) = current(app) else { return };
    let icon = crate::badge::with_unread(&base, unread).unwrap_or(base);

    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        let _ = tray.set_icon(Some(icon.clone()));
    }
    for (_, window) in app.windows() {
        let _ = window.set_icon(icon.clone());
    }
}

#[tauri::command]
pub fn get_app_icon(state: tauri::State<'_, ConfigState>) -> String {
    state.0.lock().unwrap().icon.clone()
}

#[tauri::command]
pub fn set_app_icon(app: AppHandle, name: String) -> Result<(), String> {
    if load(&app, &name).is_none() {
        return Err(format!("Icono desconocido: {name}"));
    }
    {
        let state = app.state::<ConfigState>();
        let mut cfg = state.0.lock().unwrap();
        cfg.icon = name;
        crate::config::save(&app, &cfg);
    }
    apply(&app);
    Ok(())
}
