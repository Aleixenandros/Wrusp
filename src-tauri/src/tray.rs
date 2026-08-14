//! Icono de bandeja (StatusNotifierItem en Linux vía appindicator).
//!
//! El menú se reconstruye cada vez que cambia la lista de cuentas. En GNOME
//! sin la extensión AppIndicator el icono no se muestra (limitación del
//! escritorio, no de la app).

use crate::config::ConfigState;
use crate::runtime::{AppHandle, Runtime};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};

pub const TRAY_ID: &str = "wrusp-tray";
const OPEN_PREFIX: &str = "open-";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let icon = crate::icon::current(app).expect("falta el icono de la app");

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Wrusp — WhatsApp no oficial")
        .menu(&build_menu(app)?)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Runtime>> {
    let accounts = app
        .state::<ConfigState>()
        .0
        .lock()
        .unwrap()
        .accounts
        .clone();

    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "show-main",
        "Abrir Wrusp",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    for account in &accounts {
        menu.append(&MenuItem::with_id(
            app,
            format!("{OPEN_PREFIX}{}", account.id),
            &account.name,
            true,
            None::<&str>,
        )?)?;
    }
    if !accounts.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }

    menu.append(&MenuItem::with_id(
        app,
        "settings",
        "Ajustes",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Salir",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

pub fn rebuild_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        match build_menu(app) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(err) => eprintln!("wrusp: no se pudo reconstruir el menú del tray: {err}"),
        }
    }
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "show-main" => crate::shell::focus_window(app),
        "settings" => crate::shell::show_settings(app),
        other => {
            if let Some(account_id) = other.strip_prefix(OPEN_PREFIX) {
                if let Err(err) = crate::shell::show_account(app, account_id) {
                    eprintln!("wrusp: no se pudo mostrar la cuenta: {err}");
                }
            }
        }
    }
}
