//! Alta, baja y renombrado de cuentas.
//!
//! Las vistas de WhatsApp las gestiona `shell`: aquí solo se toca la
//! configuración y se pide el cambio de vista correspondiente.

use crate::config::{Account, ConfigState};
use crate::runtime::AppHandle;
use crate::{shell, tray};
use tauri::Manager;

#[tauri::command]
pub fn list_accounts(state: tauri::State<'_, ConfigState>) -> Vec<Account> {
    state.0.lock().unwrap().accounts.clone()
}

#[tauri::command]
pub fn add_account(app: AppHandle, name: String) -> Result<Account, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }

    let account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        zoom: 1.0,
        color: None,
        muted: false,
    };
    let cuenta = account.clone();
    crate::config::mutate(&app, move |cfg| {
        cfg.accounts.push(cuenta);
        Ok(())
    })?;
    tray::rebuild_menu(&app);
    shell::show_account(&app, &account.id).map_err(|e| e.to_string())?;
    Ok(account)
}

#[tauri::command]
pub fn rename_account(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }
    crate::config::mutate(&app, |cfg| {
        let Some(account) = cfg.accounts.iter_mut().find(|a| a.id == id) else {
            return Err("Cuenta no encontrada".into());
        };
        account.name = name;
        Ok(())
    })?;
    tray::rebuild_menu(&app);
    shell::refresh_rails(&app);
    Ok(())
}

#[tauri::command]
pub fn set_account_color(app: AppHandle, id: String, color: Option<String>) -> Result<(), String> {
    let color = color
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    crate::config::mutate(&app, |cfg| {
        let Some(account) = cfg.accounts.iter_mut().find(|a| a.id == id) else {
            return Err("Cuenta no encontrada".into());
        };
        account.color = color;
        Ok(())
    })?;
    shell::refresh_rails(&app);
    Ok(())
}

#[tauri::command]
pub fn set_account_muted(app: AppHandle, id: String, muted: bool) -> Result<(), String> {
    crate::config::mutate(&app, |cfg| {
        let Some(account) = cfg.accounts.iter_mut().find(|a| a.id == id) else {
            return Err("Cuenta no encontrada".into());
        };
        account.muted = muted;
        Ok(())
    })?;
    shell::refresh_rails(&app);
    Ok(())
}

#[tauri::command]
pub fn reorder_accounts(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    crate::config::mutate(&app, |cfg| {
        let mut reordered = Vec::with_capacity(cfg.accounts.len());
        for id in &ids {
            if let Some(acc) = cfg.accounts.iter().find(|a| &a.id == id).cloned() {
                reordered.push(acc);
            }
        }
        for acc in &cfg.accounts {
            if !reordered.iter().any(|a| a.id == acc.id) {
                reordered.push(acc.clone());
            }
        }
        cfg.accounts = reordered;
        Ok(())
    })?;
    tray::rebuild_menu(&app);
    shell::refresh_rails(&app);
    Ok(())
}

#[tauri::command]
pub fn remove_account(app: AppHandle, id: String) -> Result<(), String> {
    // La ruta del perfil se resuelve ANTES de tocar nada: valida que el id sea
    // un UUID y que el destino del borrado quede confinado bajo `profiles/`.
    // Un config.json manipulado con `../` no debe poder sacar el
    // `remove_dir_all` de ahí.
    let profile = crate::config::profile_path(&app, &id)?;

    crate::config::mutate(&app, |cfg| {
        let antes = cfg.accounts.len();
        cfg.accounts.retain(|a| a.id != id);
        if cfg.accounts.len() == antes {
            return Err("Cuenta no encontrada".into());
        }
        Ok(())
    })?;

    // Solo con la baja ya persistida se destruye la vista y el perfil; si el
    // guardado falló, la cuenta sigue entera y no se ha perdido nada.
    shell::close_account_view(&app, &id);
    tray::rebuild_menu(&app);

    // Si la cuenta borrada era la vista activa, no queda nada que mostrar.
    if *app.state::<shell::ActiveView>().0.lock().unwrap() == id {
        shell::show_settings(&app);
    } else {
        shell::refresh_rails(&app);
    }

    // Borrar el perfil elimina la sesión de WhatsApp (equivale a cerrar sesión
    // en ese dispositivo). Se hace con un pequeño retraso para dar tiempo al
    // webview destruido a soltar sus ficheros.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(800));
        if let Err(err) = std::fs::remove_dir_all(&profile) {
            if profile.exists() {
                eprintln!(
                    "wrusp: no se pudo borrar el perfil {}: {err}",
                    profile.display()
                );
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn open_account(app: AppHandle, id: String) -> Result<(), String> {
    shell::show_account(&app, &id).map_err(|e| e.to_string())
}
