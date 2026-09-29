//! Icono de bandeja (StatusNotifierItem en Linux vía appindicator).
//!
//! El menú se reconstruye cada vez que cambia la lista de cuentas. En GNOME
//! sin la extensión AppIndicator el icono no se muestra (limitación del
//! escritorio, no de la app).
//!
//! Cada cuenta lleva sus no leídos en el menú («Trabajo (5)»): en Linux el
//! tooltip de la bandeja no se muestra, así que era el único sitio donde
//! verlos por cuenta sin abrir la ventana (PROD-11). Al cambiar un contador
//! solo se reescribe el texto de esa entrada, no el menú entero.

use crate::config::{Account, ConfigState};
use crate::runtime::{AppHandle, Runtime};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    Manager,
};

pub const TRAY_ID: &str = "wrusp-tray";
const OPEN_PREFIX: &str = "open-";

/// Las entradas de cuenta del menú actual, por id, para reescribir su texto.
struct AccountItems(Mutex<HashMap<String, MenuItem<Runtime>>>);

/// «No molestar» (PROD-10): hasta cuándo no se manda ningún aviso de
/// escritorio, de ninguna cuenta. Es lo mismo que silenciar una cuenta, para
/// todas y con fecha de caducidad. Vive en memoria: al reiniciar vuelven los
/// avisos, que es lo esperable de un silencio temporal.
static NO_MOLESTAR: Mutex<Option<SystemTime>> = Mutex::new(None);

/// Plazos del submenú: id, texto y minutos.
const PLAZOS: [(&str, &str, u64); 4] = [
    ("dnd-30", "30 minutos", 30),
    ("dnd-60", "1 hora", 60),
    ("dnd-120", "2 horas", 120),
    ("dnd-480", "8 horas", 480),
];

/// El submenú y su «Volver a avisar», para reescribirlos al cambiar el plazo.
struct DndItems(Mutex<Option<(Submenu<Runtime>, MenuItem<Runtime>)>>);

/// Si «No molestar» está puesto, hasta cuándo.
pub fn no_molestar() -> Option<SystemTime> {
    let mut hasta = NO_MOLESTAR.lock().unwrap();
    if hasta.is_some_and(|t| t <= SystemTime::now()) {
        *hasta = None;
    }
    *hasta
}

fn dnd_label() -> String {
    match no_molestar() {
        Some(t) => format!("No molestar · hasta las {}", crate::logs::hora_minutos(t)),
        None => "No molestar".to_string(),
    }
}

/// El estado va también aquí: el título de un submenú no siempre se
/// actualiza por dbusmenu (medido en GNOME con AppIndicator) y el de una
/// entrada normal sí.
fn volver_label() -> String {
    match no_molestar() {
        Some(t) => format!(
            "Volver a avisar (en silencio hasta las {})",
            crate::logs::hora_minutos(t)
        ),
        None => "Volver a avisar".to_string(),
    }
}

fn set_no_molestar(app: &AppHandle, hasta: Option<SystemTime>) {
    *NO_MOLESTAR.lock().unwrap() = hasta;
    refresh_dnd(app);
    let Some(t) = hasta else {
        eprintln!("wrusp: no molestar desactivado");
        return;
    };
    eprintln!(
        "wrusp: no molestar hasta las {}",
        crate::logs::hora_minutos(t)
    );
    // Al vencer, el menú vuelve a decir «No molestar» sin esperar a que nadie
    // lo abra. Si entretanto se eligió otro plazo, esto solo lo repinta.
    let app = app.clone();
    std::thread::spawn(move || {
        if let Ok(falta) = t.duration_since(SystemTime::now()) {
            std::thread::sleep(falta + Duration::from_secs(1));
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || refresh_dnd(&handle));
    });
}

fn refresh_dnd(app: &AppHandle) {
    let Some(items) = app.try_state::<DndItems>() else {
        return;
    };
    let actuales = items.0.lock().unwrap();
    if let Some((submenu, volver)) = actuales.as_ref() {
        let _ = submenu.set_text(dnd_label());
        let _ = volver.set_text(volver_label());
        let _ = volver.set_enabled(no_molestar().is_some());
    }
}

/// «Nombre» o «Nombre (N)».
fn account_label(account: &Account, unread: &HashMap<String, u32>) -> String {
    match unread.get(&account.id).copied().unwrap_or(0) {
        0 => account.name.clone(),
        n => format!("{} ({n})", account.name),
    }
}

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

    let unread = unread_counts(app);
    let mut items = HashMap::new();
    for account in &accounts {
        let item = MenuItem::with_id(
            app,
            format!("{OPEN_PREFIX}{}", account.id),
            account_label(account, &unread),
            true,
            None::<&str>,
        )?;
        menu.append(&item)?;
        items.insert(account.id.clone(), item);
    }
    match app.try_state::<AccountItems>() {
        Some(state) => *state.0.lock().unwrap() = items,
        None => {
            app.manage(AccountItems(Mutex::new(items)));
        }
    }
    if !accounts.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }

    let plazos = PLAZOS
        .iter()
        .map(|(id, texto, _)| MenuItem::with_id(app, *id, *texto, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let manana = MenuItem::with_id(app, "dnd-manana", "Hasta mañana (8:00)", true, None::<&str>)?;
    let separador = PredefinedMenuItem::separator(app)?;
    let volver = MenuItem::with_id(
        app,
        "dnd-off",
        volver_label(),
        no_molestar().is_some(),
        None::<&str>,
    )?;
    let mut entradas: Vec<&dyn tauri::menu::IsMenuItem<Runtime>> = plazos
        .iter()
        .map(|p| p as &dyn tauri::menu::IsMenuItem<Runtime>)
        .collect();
    entradas.extend([
        &manana as &dyn tauri::menu::IsMenuItem<Runtime>,
        &separador,
        &volver,
    ]);
    let submenu = Submenu::with_id_and_items(app, "dnd", dnd_label(), true, &entradas)?;
    menu.append(&submenu)?;
    match app.try_state::<DndItems>() {
        Some(state) => *state.0.lock().unwrap() = Some((submenu, volver)),
        None => {
            app.manage(DndItems(Mutex::new(Some((submenu, volver)))));
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;

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

fn unread_counts(app: &AppHandle) -> HashMap<String, u32> {
    app.try_state::<crate::shell::Unread>()
        .map(|u| u.0.lock().unwrap().clone())
        .unwrap_or_default()
}

/// Reescribe los contadores de las cuentas en el menú.
pub fn refresh_counts(app: &AppHandle) {
    let Some(items) = app.try_state::<AccountItems>() else {
        return;
    };
    let accounts = app
        .state::<ConfigState>()
        .0
        .lock()
        .unwrap()
        .accounts
        .clone();
    let unread = unread_counts(app);
    let items = items.0.lock().unwrap();
    for account in &accounts {
        if let Some(item) = items.get(&account.id) {
            let _ = item.set_text(account_label(account, &unread));
        }
    }
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
        "dnd-off" => set_no_molestar(app, None),
        "dnd-manana" => set_no_molestar(app, Some(crate::logs::proxima_hora(8))),
        id if id.starts_with("dnd-") => {
            if let Some((_, _, minutos)) = PLAZOS.iter().find(|(plazo, _, _)| *plazo == id) {
                set_no_molestar(
                    app,
                    Some(SystemTime::now() + Duration::from_secs(minutos * 60)),
                );
            }
        }
        other => {
            if let Some(account_id) = other.strip_prefix(OPEN_PREFIX) {
                if let Err(err) = crate::shell::show_account(app, account_id) {
                    eprintln!("wrusp: no se pudo mostrar la cuenta: {err}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::account_label;
    use crate::config::Account;
    use std::collections::HashMap;

    #[test]
    fn la_cuenta_lleva_sus_no_leidos_solo_si_tiene() {
        let cuenta = Account {
            id: "a".into(),
            name: "Trabajo".into(),
            zoom: 1.0,
            color: None,
            muted: false,
        };
        let mut no_leidos = HashMap::new();
        assert_eq!(account_label(&cuenta, &no_leidos), "Trabajo");
        no_leidos.insert("a".to_string(), 0);
        assert_eq!(account_label(&cuenta, &no_leidos), "Trabajo");
        no_leidos.insert("a".to_string(), 5);
        assert_eq!(account_label(&cuenta, &no_leidos), "Trabajo (5)");
    }
}
