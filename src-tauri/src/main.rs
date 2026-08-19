//! Wrusp — cliente de escritorio no oficial de WhatsApp.
//!
//! Envuelve web.whatsapp.com en webviews WebKitGTK y añade bandeja del sistema,
//! multicuenta con sesiones aisladas y tema claro/oscuro. Toda la interfaz vive
//! en **una sola ventana** con una pila de webviews (ver `shell`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod badge;
mod browser;
mod config;
mod filedrop;
mod icon;
mod logs;
mod permissions;
mod rail;
mod runtime;
mod shell;
mod theme;
mod tray;

use config::ConfigState;
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WindowEvent};

fn main() {
    // Ojo: NO fijar `WEBKIT_DISABLE_DMABUF_RENDERER`. La 0.2.1 lo hacía y con
    // él `requestVideoFrameCallback` no dispara nunca (medido: 0 callbacks
    // frente a ~200 en 8 s sin la variable), y WhatsApp usa esa API para
    // revelar el vídeo al llegar el primer fotograma: el reproductor se
    // quedaba clavado en el póster con el tiempo corriendo. El renderizador
    // por defecto reproduce bien; los vídeos que no arrancan son cosa de
    // códecs del sistema (ver README).

    // Lo primero de todo: desde aquí, stdout y stderr quedan en el fichero de
    // registro y los procesos del webview lo heredan.
    logs::init();

    // Carpeta de temporales configurada por el usuario: debe aplicarse antes de
    // arrancar el webview, porque WebKit lee TMPDIR al lanzar sus procesos.
    config::apply_temp_dir_env();

    // Sin el plugin de notificaciones de Tauri a propósito: inyecta su propio
    // `window.Notification` en TODAS las vistas, y como las de WhatsApp no
    // tienen IPC (ni deben tenerlo), la página recibía
    // «notification.request_permission not allowed», se quedaba sin permiso y
    // no salía ni un aviso. Las notificaciones se envían desde Rust con
    // `notify-rust` y llegan por la señal nativa de WebKit (ver `permissions`).
    let mut builder = tauri::Builder::<runtime::Runtime>::default()
        // Canal de las vistas hacia Rust. Se atiende como petición de red y no
        // como navegación: las navegaciones se pisan entre sí cuando llegan
        // varias a la vez (pasó al notificar y actualizar el contador en el
        // mismo instante). Sigue sin conceder IPC a whatsapp.com: aquí solo se
        // aceptan las órdenes conocidas de `shell`.
        .register_uri_scheme_protocol("wrusp", |ctx, request| {
            shell::handle_uri(ctx.app_handle(), request.uri());
            tauri::http::Response::builder()
                .status(204)
                // La página se sirve desde https://web.whatsapp.com, así que
                // sin esto el navegador descartaría la respuesta.
                .header("Access-Control-Allow-Origin", "*")
                .body(Vec::new())
                .unwrap_or_default()
        });

    // Instancia única: relanzar el binario enfoca la ventana. En depuración se
    // puede desactivar para poder ejecutar una build de prueba mientras la
    // versión instalada sigue abierta.
    let single_instance = cfg!(not(debug_assertions)) || std::env::var("WRUSP_TEST_URL").is_err();
    if single_instance {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::focus_window(app);
        }));
    }

    builder
        .invoke_handler(tauri::generate_handler![
            accounts::list_accounts,
            accounts::add_account,
            accounts::rename_account,
            accounts::remove_account,
            accounts::open_account,
            theme::get_theme,
            theme::set_theme,
            icon::get_app_icon,
            icon::set_app_icon,
            config::get_folders,
            config::set_download_dir,
            config::set_temp_dir,
            config::set_log_dir,
            config::open_log_dir,
            config::pick_folder,
            config::get_toggles,
            config::set_toggle,
            config::get_about,
            config::open_external,
        ])
        .setup(|app| {
            config::debug_assert_identifier(app.handle());
            let cfg = config::load(app.handle());
            app.manage(ConfigState(Mutex::new(cfg)));
            shell::create(app.handle())?;
            theme::apply_theme(app.handle());
            tray::create(app.handle())?;
            icon::apply(app.handle());

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Cerrar la ventana la oculta y la app sigue en la bandeja, salvo
            // que el usuario haya pedido lo contrario en ajustes.
            WindowEvent::CloseRequested { api, .. } => {
                let to_tray = window
                    .app_handle()
                    .state::<ConfigState>()
                    .0
                    .lock()
                    .unwrap()
                    .close_to_tray;
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            // En Windows y macOS los webviews hijos tienen dimensiones propias
            // que hay que refrescar (en Linux el vbox ya reparte el espacio).
            WindowEvent::Resized(_) => shell::sync_bounds(window.app_handle()),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error al iniciar Wrusp")
        .run(|app, event| {
            if let RunEvent::ExitRequested { code, api, .. } = event {
                // Sin código de salida => el proceso se quedó sin ventanas
                // visibles; seguimos vivos en el tray. `app.exit(0)` (menú
                // «Salir») sí trae código y termina de verdad.
                let to_tray = app.state::<ConfigState>().0.lock().unwrap().close_to_tray;
                if code.is_none() && to_tray {
                    api.prevent_exit();
                }
            }
        });
}
