//! Ventana única con una pila de webviews.
//!
//! Wrusp tiene **una sola ventana**. Dentro viven varios webviews hijos: el de
//! ajustes (nuestro HTML) y uno por cuenta (WhatsApp Web). Solo uno está
//! visible; cambiar de cuenta es ocultar el actual y mostrar el destino, sin
//! recargar la sesión.
//!
//! En Linux los webviews hijos van al `vbox` GTK de la ventana, que reparte el
//! espacio solo; en Windows y macOS tienen dimensiones propias y `sync_bounds`
//! las actualiza al redimensionar.

use crate::config::{Account, ConfigState};
use crate::runtime::AppHandle;
use crate::{browser, rail, theme};
use std::sync::Mutex;
use tauri::{
    webview::WebviewBuilder, window::WindowBuilder, LogicalPosition, LogicalSize, Manager,
    WebviewUrl,
};

pub const MAIN_WINDOW: &str = "main";
pub const SETTINGS_VIEW: &str = "settings";

const WHATSAPP_URL: &str = "https://web.whatsapp.com";

/// WhatsApp Web rechaza navegadores que no reconoce; nos presentamos como
/// Chrome estable de la plataforma. Junto con `browser::disguise_script()`
/// completa el disfraz (ver ADR-006).
#[cfg(target_os = "linux")]
const CHROME_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
#[cfg(target_os = "windows")]
const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
#[cfg(target_os = "macos")]
const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Vista visible ahora mismo: `settings` o el id de una cuenta.
pub struct ActiveView(pub Mutex<String>);

/// Mensajes sin leer por cuenta, según el título de WhatsApp Web.
pub struct Unread(pub Mutex<std::collections::HashMap<String, u32>>);

/// Límites del zoom, para no dejar la vista inservible.
const ZOOM_MIN: f64 = 0.5;
const ZOOM_MAX: f64 = 2.5;
const ZOOM_STEP: f64 = 0.1;

pub fn view_label(account_id: &str) -> String {
    format!("account-{account_id}")
}

fn account_url() -> tauri::Url {
    // En builds de depuración se puede apuntar el webview a un servidor local
    // para inspeccionar qué ve la página. Nunca se compila en release.
    #[cfg(debug_assertions)]
    let target = std::env::var("WRUSP_TEST_URL").unwrap_or_else(|_| WHATSAPP_URL.to_string());
    #[cfg(not(debug_assertions))]
    let target = WHATSAPP_URL.to_string();

    target.parse().expect("URL de WhatsApp inválida")
}

/// Limita las navegaciones principales de una vista de cuenta. Los recursos
/// internos de WhatsApp siguen pudiendo venir de sus CDNs; lo que se impide es
/// convertir una vista con cámara/micrófono concedidos en un navegador general.
pub(crate) fn account_navigation_allowed(url: &tauri::Url) -> bool {
    if url.as_str() == "about:blank" {
        return true;
    }
    // Descargar un fichero de un chat es, para el motor, navegar a un `blob:`
    // con atributo `download`: WhatsApp descifra el adjunto en memoria y lo
    // entrega así. El interior del blob es el origen que lo creó
    // (`blob:https://web.whatsapp.com/<uuid>`): se aceptan solo los de la
    // propia página, y WebKit convierte esa navegación en una descarga que
    // acaba en `on_download` en vez de sustituir la vista.
    if url.scheme() == "blob" {
        return url
            .path()
            .parse::<tauri::Url>()
            .is_ok_and(|origen| origen.scheme() != "blob" && account_navigation_allowed(&origen));
    }
    if url.scheme() == "https" && url.port_or_known_default() == Some(443) {
        let host = url.host_str().unwrap_or_default();
        // La página en sí, y la infraestructura que carga dentro en iframes:
        // el visor de PDF vive en `webtp.whatsapp.net` y el mantenimiento de
        // caché en `flows.whatsapp.net`. Tratarlos como enlaces externos los
        // cancelaba y los abría en el navegador, así que los PDF no se veían y
        // salían pestañas solas cada poco.
        if host == "web.whatsapp.com" || host.ends_with(".whatsapp.net") {
            return true;
        }
    }

    #[cfg(debug_assertions)]
    if let Ok(test) = std::env::var("WRUSP_TEST_URL") {
        if let Ok(test) = test.parse::<tauri::Url>() {
            return url.scheme() == test.scheme()
                && url.host_str() == test.host_str()
                && url.port_or_known_default() == test.port_or_known_default();
        }
    }

    false
}

/// Aísla un script de arranque para que un fallo suyo no deje la vista a medias
/// ni impida ejecutar los siguientes.
///
/// Estos scripts corren antes de que exista el documento, así que es fácil que
/// uno lance —tocar `document.body` cuando todavía es `null`, por ejemplo— y el
/// fallo no deja rastro: no hay consola donde mirarlo. Ha pasado de verdad, y
/// costó encontrarlo.
fn aislado(script: String) -> String {
    if script.is_empty() {
        return script;
    }
    format!("try {{\n{script}\n}} catch (e) {{ console.error('wrusp: falló un script de arranque', e); }}")
}

/// Extrae el contador inicial del título que publica WhatsApp, por ejemplo
/// `(12) WhatsApp`. Cualquier otro título equivale a cero no leídos.
fn unread_count_from_title(title: &str) -> u32 {
    title
        .strip_prefix('(')
        .and_then(|tail| tail.split_once(')'))
        .and_then(|(number, _)| number.parse().ok())
        .unwrap_or(0)
}

/// Punto de entrada del esquema `wrusp://` registrado en `main`. `origin` es la
/// etiqueta de la webview que emitió la orden, y la pone el motor, no la
/// página: es lo único del mensaje que la página no puede falsear.
pub fn handle_uri(app: &AppHandle, origin: &str, uri: &tauri::http::Uri) {
    match uri.to_string().parse::<tauri::Url>() {
        Ok(url) => handle_command(app, origin, &url),
        Err(err) => eprintln!("wrusp: URL de orden ilegible ({err})"),
    }
}

/// ¿Puede la vista `origin` actualizar el contador de la cuenta `id`? Solo la
/// propia: sin esto, cualquier vista podía atribuir no leídos —y con ellos una
/// notificación— a otra cuenta.
fn unread_order_authorized(origin: &str, id: &str) -> bool {
    origin == view_label(id)
}

/// Maneja las órdenes que las vistas envían como `wrusp://…`.
///
/// Es el único canal entre las páginas y Rust: a las webviews de cuenta no se
/// les concede IPC de Tauri (ver `capabilities/default.json`). La acción viaja
/// como **host** (`wrusp://switch/<id>`) y los datos largos, como los de una
/// notificación, en la cadena de consulta.
fn handle_command(app: &AppHandle, origin: &str, url: &tauri::Url) {
    let app = app.clone();
    let origin = origin.to_string();
    let url = url.clone();
    // La orden llega desde el hilo del webview; tocar ventanas exige el
    // principal.
    let _ = app.clone().run_on_main_thread(move || {
        let action = url.host_str().unwrap_or_default().to_string();
        let arg = url.path().trim_start_matches('/').to_string();
        let (action, arg) = (action.as_str(), arg.as_str());
        #[cfg(debug_assertions)]
        println!("wrusp: orden de la barra: {action} {arg}");
        match action {
            "switch" => {
                if let Err(err) = show_account(&app, arg) {
                    eprintln!("wrusp: no se pudo cambiar de cuenta: {err}");
                }
            }
            "settings" | "add" => show_settings(&app),
            "hide" => {
                if let Some(window) = app.get_window(MAIN_WINDOW) {
                    let _ = window.hide();
                }
            }
            "quit" => app.exit(0),
            "zoom" => apply_zoom_step(&app, arg),
            // La página cuenta cómo le ha ido una entrega de ficheros: es la
            // única que ve si WhatsApp la acepta, y sin esto el fallo no dejaba
            // rastro ninguno (ver `filedrop`).
            "log" => {
                let mensaje: String = url
                    .query_pairs()
                    .find(|(clave, _)| clave == "m")
                    .map(|(_, valor)| {
                        valor
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(300)
                            .collect()
                    })
                    .unwrap_or_default();
                if !mensaje.is_empty() {
                    eprintln!("wrusp: página: {mensaje}");
                }
            }
            // Lo pegado, cuando el motor no se lo entrega a la página (ver
            // `filedrop`). Va a la vista visible, no a la que diga la página.
            "pegar" => {
                let etiqueta = active_view_label(&app);
                if etiqueta != SETTINGS_VIEW {
                    crate::filedrop::pegar(&app, &etiqueta);
                }
            }
            // Un medio que falló y que la página ofrece volcar a disco (solo
            // con `WRUSP_GUARDAR_MEDIOS_FALLIDOS`, ver `diagnostico`). Se lee
            // de la vista que lo ofrece, que es la que lo tiene.
            "medio-fallido" => crate::diagnostico::guardar_medio_fallido(&app, &origin, arg),
            // `unread/<id>/<n>`: lo emite el observador del título. El id
            // viaja en la orden, pero manda la vista emisora: solo puede
            // actualizar su propio contador.
            "unread" => {
                if let Some((id, count)) = arg.split_once('/') {
                    if unread_order_authorized(&origin, id) {
                        set_unread(&app, id, count.parse().unwrap_or(0));
                    } else {
                        eprintln!("wrusp: orden unread de {origin} atribuida a {id}: rechazada");
                    }
                }
            }
            other => eprintln!("wrusp: orden desconocida de la barra: {other}"),
        }
        if action == "add" {
            // La página de ajustes enfoca el campo de alta al recibir esto.
            if let Some(view) = app.get_webview(SETTINGS_VIEW) {
                let _ = view.eval("window.__wruspFocusAdd && window.__wruspFocusAdd()");
            }
        }
    });
}

/// Crea la ventana y la vista de ajustes. El resto de vistas son perezosas.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let window = WindowBuilder::new(app, MAIN_WINDOW)
        .title("Wrusp")
        .inner_size(1100.0, 720.0)
        .min_inner_size(640.0, 480.0)
        .build()?;

    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    let handle = app.clone();
    let settings = WebviewBuilder::new(SETTINGS_VIEW, WebviewUrl::App("index.html".into()))
        .on_navigation(move |url| {
            if url.scheme() == "wrusp" {
                handle_command(&handle, SETTINGS_VIEW, url);
                return false;
            }
            true
        })
        .initialization_script(aislado(rail::runtime_script(SETTINGS_VIEW)));

    app.manage(ActiveView(Mutex::new(SETTINGS_VIEW.to_string())));
    app.manage(Unread(Mutex::new(std::collections::HashMap::new())));

    window.add_child(
        settings,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(size.width, size.height),
    )?;

    // El estado va después de crear la vista: el script de arranque solo trae
    // la maquinaria, y los datos llegan por `eval`.
    refresh_rails(app);

    // Al abrir se entra en la primera cuenta: lo que quieres ver al arrancar
    // una app de mensajería son tus mensajes, no los ajustes. Sin cuentas, la
    // pantalla de ajustes es justo lo que hace falta para crear la primera.
    let primera = {
        let state = app.state::<ConfigState>();
        let cfg = state.0.lock().unwrap();
        cfg.accounts.first().map(|a| a.id.clone())
    };
    if let Some(id) = primera {
        show_account(app, &id)?;
    }
    Ok(())
}

/// Script con los datos actuales de la barra (cuentas, vista activa, tema y
/// no leídos).
fn rail_state(app: &AppHandle) -> String {
    let (accounts, mode) = {
        let cfg = app.state::<ConfigState>();
        let cfg = cfg.0.lock().unwrap();
        (cfg.accounts.clone(), cfg.theme)
    };
    let active = app.state::<ActiveView>().0.lock().unwrap().clone();
    let unread = app.state::<Unread>().0.lock().unwrap().clone();
    rail::state_script(&accounts, &active, theme::is_dark(app, mode), &unread)
}

/// Zoom guardado de una cuenta.
fn account_zoom(app: &AppHandle, account_id: &str) -> f64 {
    let cfg = app.state::<ConfigState>();
    let cfg = cfg.0.lock().unwrap();
    cfg.accounts
        .iter()
        .find(|a| a.id == account_id)
        .map(|a| a.zoom)
        .unwrap_or(1.0)
}

/// Aplica un paso de zoom (`in`, `out`, `reset`) a la vista activa y lo
/// recuerda. La página de ajustes no se amplía: no le hace falta.
fn apply_zoom_step(app: &AppHandle, step: &str) {
    let active = app.state::<ActiveView>().0.lock().unwrap().clone();
    if active == SETTINGS_VIEW {
        return;
    }
    let current = account_zoom(app, &active);
    let zoom = match step {
        "in" => (current + ZOOM_STEP).min(ZOOM_MAX),
        "out" => (current - ZOOM_STEP).max(ZOOM_MIN),
        _ => 1.0,
    };
    let guardado = crate::config::mutate(app, |cfg| {
        if let Some(account) = cfg.accounts.iter_mut().find(|a| a.id == active) {
            account.zoom = zoom;
        }
        Ok(())
    });
    if let Err(err) = guardado {
        eprintln!("wrusp: el zoom no se pudo guardar: {err}");
        return;
    }
    if let Some(view) = app.get_webview(&view_label(&active)) {
        let _ = view.set_zoom(zoom);
    }
}

/// Entrada de las notificaciones que WebKit nos entrega, tanto las que nacen en
/// la página como las del service worker —la vía real de WhatsApp—. Salta al
/// hilo principal porque la señal llega desde el suyo.
pub fn notify_from_webkit(app: &AppHandle, account_id: &str, title: &str, body: &str) {
    let app = app.clone();
    let (account_id, title, body) = (account_id.to_string(), title.to_string(), body.to_string());
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        notify(&handle, &account_id, &title, &body);
    });
}

/// Un aviso solo es redundante si el usuario está mirando ahora mismo la
/// cuenta que lo produjo. «Ventana visible» no basta: puede estar tapada, en
/// segundo plano o en otro escritorio.
fn notification_is_redundant(window_focused: bool, active_account: &str, account_id: &str) -> bool {
    window_focused && active_account == account_id
}

/// Lanza una notificación de escritorio por un mensaje nuevo.
///
/// El título que manda WhatsApp Web es el nombre del contacto y el cuerpo, el
/// mensaje. No se avisa de la cuenta que estás mirando: ya la tienes delante.
fn notify(app: &AppHandle, account_id: &str, title: &str, body: &str) {
    let (enabled, privacy, accounts) = {
        let state = app.state::<ConfigState>();
        let cfg = state.0.lock().unwrap();
        (
            cfg.notifications,
            cfg.notification_privacy,
            cfg.accounts.clone(),
        )
    };
    if !enabled {
        eprintln!("wrusp: notificación descartada: están desactivadas en ajustes");
        return;
    }

    if let Some(account) = accounts.iter().find(|a| a.id == account_id) {
        if account.muted {
            eprintln!(
                "wrusp: notificación descartada: cuenta «{}» silenciada",
                account.name
            );
            return;
        }
    }

    let focused = app
        .get_window(MAIN_WINDOW)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let active = app.state::<ActiveView>().0.lock().unwrap().clone();
    if notification_is_redundant(focused, &active, account_id) {
        // A propósito: la conversación la tienes delante.
        eprintln!("wrusp: notificación descartada: esa cuenta tiene el foco");
        return;
    }

    // Con varias cuentas conviene saber a cuál llega.
    let title = match accounts.iter().find(|a| a.id == account_id) {
        Some(account) if accounts.len() > 1 => format!("{title} · {}", account.name),
        _ => title.to_string(),
    };

    let body_text = if privacy {
        "Nuevo mensaje".to_string()
    } else {
        body.to_string()
    };

    // El módulo conserva una sola conexión D-Bus y atiende su propia cola fuera
    // del hilo de GTK. Además de no congelar la interfaz si el servidor tarda,
    // esto es imprescindible en GNOME: liga la fuente del aviso al emisor y la
    // destruye en cuanto desaparece su nombre D-Bus.
    crate::notifications::show(account_id.to_string(), title, body_text);
}

/// Registra los no leídos de una cuenta y refresca bandeja, título y barra.
fn set_unread(app: &AppHandle, account_id: &str, count: u32) {
    {
        let state = app.state::<Unread>();
        let mut unread = state.0.lock().unwrap();
        if unread.get(account_id) == Some(&count) {
            return;
        }
        unread.insert(account_id.to_string(), count);
    }

    // Aquí NO se notifica: Chromium ya entrega al escritorio el aviso web con
    // remitente y mensaje. Emitir además uno propio y genérico duplicaba cada
    // aviso, con la versión peor llegando la segunda.
    refresh_unread_indicators(app);
    crate::icon::apply(app); // la insignia del icono depende del total
    refresh_rails_soon(app);
}

/// Refresca la barra tras un instante.
///
/// Las órdenes llegan cancelando una navegación, y el `eval` inmediato sobre la
/// vista que la originó se pierde (las demás sí lo reciben). Con una espera
/// corta la vista ya está asentada y recibe el estado.
fn refresh_rails_soon(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || refresh_rails(&handle));
    });
}

/// Total de mensajes sin leer sumando todas las cuentas.
pub fn total_unread(app: &AppHandle) -> u32 {
    let accounts = {
        let cfg = app.state::<ConfigState>();
        let cfg = cfg.0.lock().unwrap();
        cfg.accounts.clone()
    };
    let state = app.state::<Unread>();
    let unread = state.0.lock().unwrap();
    accounts
        .iter()
        .map(|a| unread.get(&a.id).copied().unwrap_or(0))
        .sum()
}

/// Escribe el total de no leídos en el título de la ventana y en el tooltip de
/// la bandeja, con el desglose por cuenta.
fn refresh_unread_indicators(app: &AppHandle) {
    let accounts = {
        let cfg = app.state::<ConfigState>();
        let cfg = cfg.0.lock().unwrap();
        cfg.accounts.clone()
    };
    let unread = app.state::<Unread>().0.lock().unwrap().clone();

    let total: u32 = accounts
        .iter()
        .map(|a| unread.get(&a.id).copied().unwrap_or(0))
        .sum();

    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let title = if total > 0 {
            format!("({total}) Wrusp")
        } else {
            "Wrusp".to_string()
        };
        let _ = window.set_title(&title);
    }

    let detail: Vec<String> = accounts
        .iter()
        .filter_map(|a| {
            let n = unread.get(&a.id).copied().unwrap_or(0);
            (n > 0).then(|| format!("{}: {n}", a.name))
        })
        .collect();
    let tooltip = if detail.is_empty() {
        "Wrusp — sin mensajes nuevos".to_string()
    } else {
        format!("Wrusp — {}", detail.join(" · "))
    };
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        let _ = tray.set_tooltip(Some(&tooltip));
    }
}

/// Etiqueta de la vista que debería verse ahora mismo.
pub fn active_view_label(app: &AppHandle) -> String {
    let active = app.state::<ActiveView>().0.lock().unwrap().clone();
    if active == SETTINGS_VIEW {
        SETTINGS_VIEW.to_string()
    } else {
        view_label(&active)
    }
}

/// Deja visible solo la vista activa. Es la única función que decide qué se ve.
pub fn apply_visibility(app: &AppHandle) {
    let Some(window) = app.get_window(MAIN_WINDOW) else {
        return;
    };
    let target = active_view_label(app);
    for view in window.webviews() {
        let _ = if view.label() == target {
            view.show()
        } else {
            view.hide()
        };
    }
}

/// Trae la ventana al frente sin destapar las vistas ocultas.
pub fn focus_window(app: &AppHandle) {
    let Some(window) = app.get_window(MAIN_WINDOW) else {
        return;
    };
    // Solo se muestra si hacía falta; evita trabajo y parpadeos innecesarios.
    let hidden = !window.is_visible().unwrap_or(true);
    if hidden {
        let _ = window.show();
    }
    let _ = window.unminimize();
    let _ = window.set_focus();
    if hidden {
        // Se reafirma la visibilidad tras procesar el cambio de ventana.
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || apply_visibility(&handle));
        });
    }
}

pub fn show_settings(app: &AppHandle) {
    *app.state::<ActiveView>().0.lock().unwrap() = SETTINGS_VIEW.to_string();
    focus_window(app);
    apply_visibility(app);
    refresh_rails(app);
}

/// Muestra la cuenta indicada, creando su webview la primera vez.
pub fn show_account(app: &AppHandle, account_id: &str) -> tauri::Result<()> {
    let account = {
        let state = app.state::<ConfigState>();
        let cfg = state.0.lock().unwrap();
        cfg.accounts.iter().find(|a| a.id == account_id).cloned()
    };
    let Some(account) = account else {
        return Ok(()); // cuenta borrada entre el clic y ahora
    };

    let label = view_label(&account.id);
    if app.get_webview(&label).is_none() {
        create_account_view(app, &account)?;
    }
    if app.get_webview(&label).is_some() {
        *app.state::<ActiveView>().0.lock().unwrap() = account.id.clone();
    }
    focus_window(app);
    apply_visibility(app);
    // Una vista recién creada aparece visible por su cuenta un instante
    // después, así que se recoloca la visibilidad cuando ya está montada.
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || apply_visibility(&app));
    });
    refresh_rails(app);
    Ok(())
}

fn create_account_view(app: &AppHandle, account: &Account) -> tauri::Result<()> {
    let Some(window) = app.get_window(MAIN_WINDOW) else {
        return Ok(());
    };
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    let mode = app.state::<ConfigState>().0.lock().unwrap().theme;
    let handle = app.clone();

    let origen = view_label(&account.id);
    let builder = WebviewBuilder::new(view_label(&account.id), WebviewUrl::External(account_url()))
        .on_navigation(move |url| {
            if url.scheme() == "wrusp" {
                handle_command(&handle, &origen, url);
                return false;
            }
            let allowed = account_navigation_allowed(url);
            if !allowed {
                // La vista sigue sin convertirse en un navegador general, pero
                // el enlace no se pierde: se abre fuera, como haría cualquier
                // aplicación de escritorio.
                crate::config::open_in_browser(url);
            }
            allowed
        })
        .on_new_window(|url, _features| {
            // WhatsApp abre los enlaces de los chats como ventana emergente.
            crate::config::open_in_browser(&url);
            tauri::webview::NewWindowResponse::Deny
        })
        .on_document_title_changed(|webview, title| {
            let account_id = webview.label().strip_prefix("account-").unwrap_or_default();
            let count = unread_count_from_title(&title);
            set_unread(webview.app_handle(), account_id, count);
        })
        .user_agent(CHROME_UA)
        .initialization_script(aislado(browser::disguise_script()))
        .initialization_script(aislado(browser::hide_webcodecs_script()))
        .initialization_script(aislado(crate::diagnostico::init_script()))
        .initialization_script(aislado(browser::fix_large_mp4_blobs_script()))
        .initialization_script(aislado(browser::hide_native_app_promo_script()))
        .initialization_script(aislado(theme::whatsapp_init_script(mode)))
        .initialization_script(aislado(rail::runtime_script(&account.id)))
        .initialization_script(aislado(crate::filedrop::SCRIPT.to_owned()))
        .initialization_script(aislado(crate::clipboard::SCRIPT.to_owned()))
        // Cada descarga pregunta dónde guardarse; la carpeta configurada en
        // ajustes es el punto de partida del diálogo.
        .on_download(|webview, event| {
            let tauri::webview::DownloadEvent::Requested { destination, .. } = event else {
                return true; // el resto de eventos no piden decisión
            };
            // wry propone «Descargas del sistema + nombre sugerido»; de la
            // propuesta solo interesa el nombre.
            let nombre = destination
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| String::from("descarga"));
            let dir = crate::config::download_dir(webview.app_handle());
            let _ = std::fs::create_dir_all(&dir);
            match elegir_destino_descarga(&webview, &dir, &nombre) {
                Some(ruta) => {
                    eprintln!("wrusp: descarga aceptada en {}", ruta.display());
                    *destination = ruta;
                    true
                }
                None => {
                    eprintln!("wrusp: descarga cancelada");
                    false
                }
            }
        });

    // Aislamiento de sesión por cuenta: en Linux, Tauri crea un `WebContext` de
    // WebKitGTK distinto por cada directorio de datos, así que cookies,
    // localStorage e IndexedDB quedan separados. La ruta pasa por la misma
    // validación de confinamiento que el borrado: una cuenta con id inválido no
    // debe abrir vista (ni caer a un perfil compartido).
    let builder = {
        let profile = match crate::config::profile_path(app, &account.id) {
            Ok(profile) => profile,
            Err(err) => {
                eprintln!("wrusp: no se abre la cuenta {:?}: {err}", account.name);
                return Ok(());
            }
        };
        let _ = std::fs::create_dir_all(&profile);
        builder.data_directory(profile)
    };

    let view = window.add_child(
        builder,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(size.width, size.height),
    )?;
    let _ = view.set_zoom(account_zoom(app, &account.id));
    // Sin esto WebKitGTK no expone las APIs de medios —WhatsApp diría que el
    // navegador no admite notas de voz ni cámara— ni nos entrega las
    // notificaciones del service worker.
    crate::permissions::configure(app, &view, &account.id);
    crate::clipboard::configure(&view);

    // El motor entrega el soltado con la ruta del fichero, pero sin construir
    // el `File` que espera la página, así que arrastrar algo a un chat no hacía
    // nada. Aquí se recogen las rutas y se le pasan a la vista (ver `filedrop`).
    let handle = app.clone();
    let etiqueta = view_label(&account.id);
    view.on_webview_event(move |event| {
        let tauri::WebviewEvent::DragDrop(tauri::DragDropEvent::Drop { paths, position }) = event
        else {
            return;
        };
        // La posición viene en píxeles físicos y la página razona en CSS.
        let escala = handle
            .get_window(MAIN_WINDOW)
            .and_then(|w| w.scale_factor().ok())
            .unwrap_or(1.0);
        let x = (position.x / escala).round() as i64;
        let y = (position.y / escala).round() as i64;
        crate::filedrop::soltar(&handle, &etiqueta, paths.clone(), x, y);
    });

    // Arnés de pruebas: soltar de verdad exige una mano en el ratón, así que
    // con `WRUSP_TEST_DROP=/ruta[:/otra]` se simula el soltado en cuanto la
    // vista está en pie. Es justo lo que se rompió en silencio, y así se puede
    // comprobar sin manos. Nunca se compila en release.
    #[cfg(debug_assertions)]
    if let Ok(rutas) = std::env::var("WRUSP_TEST_DROP") {
        let rutas: Vec<std::path::PathBuf> =
            rutas.split(':').map(std::path::PathBuf::from).collect();
        let handle = app.clone();
        let etiqueta = view_label(&account.id);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(4));
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                crate::filedrop::soltar(&app, &etiqueta, rutas, 100, 100);
            });
        });
    }

    Ok(())
}

/// Destino de una descarga: diálogo nativo de «guardar como», partiendo de la
/// carpeta configurada y con el nombre que sugiere la página. `None` = el
/// usuario canceló y la descarga no ocurre.
///
/// Corre dentro de la señal con la que WebKit espera el destino, en el hilo
/// principal: `run()` abre un bucle GTK anidado, así que la ventana sigue
/// respondiendo mientras el diálogo está abierto.
#[cfg(target_os = "linux")]
fn elegir_destino_descarga(
    webview: &tauri::Webview<crate::runtime::Runtime>,
    dir: &std::path::Path,
    nombre: &str,
) -> Option<std::path::PathBuf> {
    use gtk::prelude::{FileChooserExt, NativeDialogExt};

    // Arnés de pruebas: sin nadie delante que pulse botones, la descarga se
    // acepta sola en la carpeta configurada. Con un número de segundos se
    // bombea antes el bucle GTK ese tiempo, que es lo que hace el diálogo
    // mientras espera al usuario: comprueba que WebKit aguanta la espera
    // dentro de su señal. Nunca se compila en release.
    #[cfg(debug_assertions)]
    if let Ok(valor) = std::env::var("WRUSP_TEST_AUTOSAVE") {
        let fin =
            std::time::Instant::now() + std::time::Duration::from_secs(valor.parse().unwrap_or(0));
        while std::time::Instant::now() < fin {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        return Some(crate::config::unique_path(dir.join(nombre)));
    }

    let Ok(ventana) = webview.window().gtk_window() else {
        // Sin ventana GTK no hay diálogo posible: a la carpeta configurada.
        return Some(crate::config::unique_path(dir.join(nombre)));
    };
    let dialogo = gtk::FileChooserNative::new(
        Some("Guardar fichero"),
        Some(&ventana),
        gtk::FileChooserAction::Save,
        Some("Guardar"),
        Some("Cancelar"),
    );
    dialogo.set_modal(true);
    dialogo.set_do_overwrite_confirmation(true);
    let _ = dialogo.set_current_folder(dir);
    dialogo.set_current_name(nombre);
    let respuesta = dialogo.run();
    (respuesta == gtk::ResponseType::Accept)
        .then(|| dialogo.filename())
        .flatten()
}

/// En Windows y macOS se conserva el comportamiento de siempre —a la carpeta
/// configurada, sin preguntar— hasta que esas builds se verifiquen.
#[cfg(not(target_os = "linux"))]
fn elegir_destino_descarga(
    _webview: &tauri::Webview<crate::runtime::Runtime>,
    dir: &std::path::Path,
    nombre: &str,
) -> Option<std::path::PathBuf> {
    Some(crate::config::unique_path(dir.join(nombre)))
}

/// Destruye la vista de una cuenta (al borrarla).
pub fn close_account_view(app: &AppHandle, account_id: &str) {
    if let Some(view) = app.get_webview(&view_label(account_id)) {
        let _ = view.close();
    }
}

/// Redibuja la barra lateral en todas las vistas: se llama al añadir, renombrar
/// o borrar cuentas, al cambiar de vista y al cambiar el tema.
pub fn refresh_rails(app: &AppHandle) {
    let Some(window) = app.get_window(MAIN_WINDOW) else {
        return;
    };
    let script = rail_state(app);
    for view in window.webviews() {
        let _ = view.eval(&script);
    }
}

/// Ajusta el tamaño de las vistas al de la ventana.
///
/// En Windows y macOS los webviews hijos tienen dimensiones propias que hay
/// que refrescar al
/// redimensionar.
pub fn sync_bounds(app: &AppHandle) {
    let Some(window) = app.get_window(MAIN_WINDOW) else {
        return;
    };
    let Ok(scale) = window.scale_factor() else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    let size = size.to_logical::<f64>(scale);
    for view in window.webviews() {
        let _ = view.set_position(LogicalPosition::new(0.0, 0.0));
        let _ = view.set_size(LogicalSize::new(size.width, size.height));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        account_navigation_allowed, notification_is_redundant, unread_count_from_title,
        unread_order_authorized,
    };

    #[test]
    fn una_vista_solo_actualiza_su_propio_contador() {
        assert!(unread_order_authorized("account-personal", "personal"));

        assert!(!unread_order_authorized("account-trabajo", "personal"));
        assert!(!unread_order_authorized("settings", "personal"));
        assert!(!unread_order_authorized("personal", "personal")); // sin prefijo
    }

    #[test]
    fn notifications_are_only_redundant_for_the_focused_account() {
        assert!(notification_is_redundant(true, "personal", "personal"));
        assert!(!notification_is_redundant(false, "personal", "personal"));
        assert!(!notification_is_redundant(true, "personal", "trabajo"));
    }

    #[test]
    fn account_views_only_accept_whatsapp_origins() {
        let whatsapp = "https://web.whatsapp.com/".parse().unwrap();
        // Infraestructura que la propia página carga en iframes.
        let pdf = "https://webtp.whatsapp.net/pdf-viewer/?locale=es_ES"
            .parse()
            .unwrap();
        let flows = "https://flows.whatsapp.net/flows/cache_management/"
            .parse()
            .unwrap();

        let subdomain = "https://assets.web.whatsapp.com/".parse().unwrap();
        let lookalike = "https://web.whatsapp.com.example.org/".parse().unwrap();
        // No basta con que el dominio termine parecido.
        let net_lookalike = "https://malo.whatsapp.net.example.org/".parse().unwrap();
        // La web pública no es la aplicación: sus enlaces se abren fuera.
        let publica = "https://www.whatsapp.com/".parse().unwrap();
        let insecure = "http://web.whatsapp.com/".parse().unwrap();
        let custom_port = "https://web.whatsapp.com:444/".parse().unwrap();

        assert!(account_navigation_allowed(&whatsapp));
        assert!(account_navigation_allowed(&pdf));
        assert!(account_navigation_allowed(&flows));

        assert!(!account_navigation_allowed(&subdomain));
        assert!(!account_navigation_allowed(&lookalike));
        assert!(!account_navigation_allowed(&net_lookalike));
        assert!(!account_navigation_allowed(&publica));
        assert!(!account_navigation_allowed(&insecure));
        assert!(!account_navigation_allowed(&custom_port));
    }

    #[test]
    fn blob_downloads_only_from_the_page_itself() {
        // Así entrega WhatsApp los ficheros de un chat: navegación a un blob
        // creado por la propia página, que WebKit convierte en descarga.
        let propio = "blob:https://web.whatsapp.com/3f0a2b9c".parse().unwrap();

        let ajeno = "blob:https://ejemplo.org/3f0a2b9c".parse().unwrap();
        let publico = "blob:https://www.whatsapp.com/3f0a2b9c".parse().unwrap();
        let anidado = "blob:blob:https://web.whatsapp.com/x".parse().unwrap();
        let ilegible = "blob:no-es-una-url".parse().unwrap();

        assert!(account_navigation_allowed(&propio));

        assert!(!account_navigation_allowed(&ajeno));
        assert!(!account_navigation_allowed(&publico));
        assert!(!account_navigation_allowed(&anidado));
        assert!(!account_navigation_allowed(&ilegible));
    }

    #[test]
    fn unread_count_only_comes_from_the_title_prefix() {
        assert_eq!(unread_count_from_title("(12) WhatsApp"), 12);
        assert_eq!(unread_count_from_title("WhatsApp (12)"), 0);
        assert_eq!(unread_count_from_title("(no) WhatsApp"), 0);
        assert_eq!(unread_count_from_title("WhatsApp"), 0);
    }
}
