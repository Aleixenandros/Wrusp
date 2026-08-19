//! Capacidades del webview: captura de medios y notificaciones.
//!
//! WebKitGTK viene con casi todo desactivado, y no por descuido: son cosas que
//! la aplicación anfitriona debe conceder a conciencia.
//!
//! - **Captura de medios** (`enable-media-stream`): sin esto no existe
//!   `navigator.mediaDevices` y no hay ni notas de voz ni cámara. El permiso en
//!   sí llega por `permission-request`, que se deniega sola si nadie la
//!   escucha.
//! - **Notificaciones**: se recogen en la señal nativa `show-notification`, que
//!   cubre tanto las que nacen en la página como las del service worker (la vía
//!   real de WhatsApp, a la que un script inyectado no llega).
//!
//! **Llamadas**: se activa `enable-webrtc`, pero conviene saber que en algunas
//! distribuciones —Fedora 44 entre ellas— WebKitGTK se compila **sin WebRTC**:
//! el ajuste se acepta y `RTCPeerConnection` sigue sin existir, así que WhatsApp
//! dice que el navegador no admite llamadas. Se comprobó con una prueba mínima
//! ajena a Wrusp y con `nm -D`, que no exporta un solo símbolo de
//! `PeerConnection`. No hay nada que Wrusp pueda hacer al respecto.

/// Orígenes a los que se concede notificar. Solo WhatsApp (y, en depuración,
/// el servidor de pruebas).
#[cfg(target_os = "linux")]
fn notification_origins() -> Vec<webkit2gtk::SecurityOrigin> {
    use webkit2gtk::SecurityOrigin;

    let mut origins = vec![SecurityOrigin::new("https", "web.whatsapp.com", 443)];

    #[cfg(debug_assertions)]
    if let Ok(test) = std::env::var("WRUSP_TEST_URL") {
        if let Ok(url) = test.parse::<tauri::Url>() {
            if let Some(host) = url.host_str() {
                let port = url.port_or_known_default().unwrap_or(443);
                origins.push(SecurityOrigin::new(url.scheme(), host, port));
            }
        }
    }
    origins
}

/// Contesta la consulta con la que WebKitGTK resuelve `Notification.permission`.
///
/// Desde WebKitGTK 2.40 el estado de un permiso no sale solo de
/// `initialize_notification_permissions`: el motor **pregunta** a la aplicación
/// con la señal `query-permission-state`, y si nadie contesta, la página ve el
/// permiso en «default». Medido en el banco de pruebas: la señal llega con
/// `WebKitWebView` y `WebKitPermissionStateQuery`, y sin respuesta
/// `Notification.permission` se queda en `default` —con lo que WhatsApp no
/// emite un solo aviso— y `requestPermission()` devuelve `denied` porque
/// llamarlo sin gesto del usuario no está permitido. No se rompió al
/// actualizar Wrusp, sino el motor del sistema.
///
/// El binding de Rust marca esta señal como no soportada, así que se conecta
/// por nombre y se responde con la API de C. Solo se concede
/// `notifications`, y solo a los orígenes de `notification_origins`; el resto
/// de permisos se dejan sin contestar para que sigan su camino de siempre
/// (`permission-request`, más abajo).
#[cfg(target_os = "linux")]
fn contestar_consultas_de_permiso(native: &webkit2gtk::WebView) {
    use webkit2gtk::glib::prelude::*;
    use webkit2gtk::glib::translate::{from_glib_none, ToGlibPtr};
    use webkit2gtk::glib::{gobject_ffi, Value};
    use webkit2gtk::{ffi, SecurityOrigin};

    let _ = native.connect_local("query-permission-state", false, move |valores| {
        // Devolver `true` significa «contestada»; `false`, que WebKit siga su
        // política por defecto.
        let concedida = unsafe {
            let Some(valor) = valores.get(1) else {
                return Some(false.to_value());
            };
            let consulta = gobject_ffi::g_value_get_boxed(valor.to_glib_none().0)
                as *mut ffi::WebKitPermissionStateQuery;
            if consulta.is_null() {
                return Some(false.to_value());
            }
            let nombre: String =
                from_glib_none(ffi::webkit_permission_state_query_get_name(consulta));
            let origen: SecurityOrigin = from_glib_none(
                ffi::webkit_permission_state_query_get_security_origin(consulta),
            );
            let permitido = nombre == "notifications"
                && notification_origins().iter().any(|esperado| {
                    esperado.protocol() == origen.protocol() && esperado.host() == origen.host()
                });
            // Se registra en release: es el primer eslabón de las
            // notificaciones y, cuando fallan, hay que poder verlo.
            if nombre == "notifications" {
                eprintln!(
                    "wrusp: permiso «{nombre}» de {}://{} → {}",
                    origen.protocol().unwrap_or_default(),
                    origen.host().unwrap_or_default(),
                    if permitido {
                        "concedido"
                    } else {
                        "NO concedido (origen inesperado)"
                    }
                );
            }
            if permitido {
                ffi::webkit_permission_state_query_finish(
                    consulta,
                    ffi::WEBKIT_PERMISSION_STATE_GRANTED,
                );
            }
            permitido
        };
        Some(Value::from(concedida))
    });
}

/// Pide un proceso web nuevo, una sola vez, nada más montar la vista.
///
/// `Notification.permission` no lo resuelve la consulta de arriba: sale de la
/// lista que WebKit pide **al lanzar el proceso web**, con la señal
/// `initialize-notification-permissions`. Esa señal se emite antes de que
/// Tauri nos deje tocar el webview, así que la primera carga siempre se
/// quedaba sin permiso —la página veía «default» y WhatsApp no emitía un solo
/// aviso— y no había forma de concedérselo después:
/// `requestPermission()` sin gesto del usuario devuelve «denied».
///
/// Medido en el banco de pruebas: con el proceso web recién arrancado el
/// permiso es «default»; tras reiniciarlo con la señal ya conectada, WebKit
/// pide la lista, la sembramos y la página pasa a «granted». Se hace al
/// arrancar la vista, cuando la carga acaba de empezar y reiniciarla no cuesta
/// nada.
#[cfg(target_os = "linux")]
fn pedir_proceso_web_nuevo(native: &webkit2gtk::WebView) {
    use std::cell::RefCell;
    use std::rc::Rc;
    use webkit2gtk::glib;
    use webkit2gtk::WebViewExt;

    // Dónde estaba yendo la vista cuando pedimos el reinicio. Se guarda porque
    // en ese momento la carga aún no está confirmada y `reload()` dejaría la
    // vista en blanco.
    let destino: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let pendiente = destino.clone();
    native.connect_web_process_terminated(move |vista, _motivo| {
        // Solo se recarga el reinicio que hemos pedido nosotros; si el proceso
        // se cae de verdad, WebKit muestra su propia página de error.
        if let Some(url) = pendiente.borrow_mut().take() {
            vista.load_uri(&url);
        }
    });

    // En cuanto el bucle respire: así el proceso ya existe y termina de verdad.
    let vista = native.clone();
    glib::idle_add_local_once(move || {
        let url = vista.uri().map(|u| u.to_string()).unwrap_or_default();
        if url.is_empty() {
            return; // sin destino conocido no se toca nada
        }
        *destino.borrow_mut() = Some(url);
        vista.terminate_web_process();
    });
}

/// Apaga la característica del motor que registra un reproductor en el
/// escritorio por cada audio o vídeo.
///
/// WebKitGTK publica una sesión MPRIS por cada elemento multimedia que suena
/// —así aparecen los controles de reproducción de GNOME—, pero **no la retira
/// al terminar**: cada nota de voz, cada vídeo y cada sonido de notificación
/// deja un «wrusp» más en el panel de medios, y se acumulan hasta cerrar la
/// aplicación. Comprobado en el bus de la sesión: varios
/// `org.mpris.MediaPlayer2.org.webkit.app-….instance-N` vivos a la vez, todos
/// del proceso web de Wrusp.
///
/// Se desactiva la característica `MediaSession` del motor, que es de donde
/// cuelga ese registro. El precio es no controlar la reproducción de WhatsApp
/// con las teclas de medios del teclado; a cambio, el panel de medios del
/// escritorio deja de llenarse de entradas muertas. La API de características
/// no está en el binding de Rust, así que se declara a mano.
#[cfg(target_os = "linux")]
fn apagar_sesion_multimedia(settings: &webkit2gtk::Settings) {
    use std::ffi::CStr;
    use webkit2gtk::glib::translate::ToGlibPtr;

    #[repr(C)]
    struct WebKitFeature {
        _opaco: [u8; 0],
    }
    #[repr(C)]
    struct WebKitFeatureList {
        _opaco: [u8; 0],
    }
    extern "C" {
        fn webkit_settings_get_all_features() -> *mut WebKitFeatureList;
        fn webkit_feature_list_get_length(lista: *mut WebKitFeatureList) -> usize;
        fn webkit_feature_list_get(lista: *mut WebKitFeatureList, i: usize) -> *mut WebKitFeature;
        fn webkit_feature_list_unref(lista: *mut WebKitFeatureList);
        fn webkit_feature_get_identifier(f: *mut WebKitFeature) -> *const std::os::raw::c_char;
        fn webkit_settings_set_feature_enabled(
            settings: *mut webkit2gtk::ffi::WebKitSettings,
            f: *mut WebKitFeature,
            activada: webkit2gtk::glib::ffi::gboolean,
        );
    }

    unsafe {
        let lista = webkit_settings_get_all_features();
        if lista.is_null() {
            return;
        }
        for i in 0..webkit_feature_list_get_length(lista) {
            let caracteristica = webkit_feature_list_get(lista, i);
            if caracteristica.is_null() {
                continue;
            }
            let identificador = webkit_feature_get_identifier(caracteristica);
            if identificador.is_null()
                || CStr::from_ptr(identificador).to_bytes() != b"MediaSession"
            {
                continue;
            }
            webkit_settings_set_feature_enabled(
                settings.to_glib_none().0,
                caracteristica,
                webkit2gtk::glib::ffi::GFALSE,
            );
            break;
        }
        webkit_feature_list_unref(lista);
    }
}

/// Activa las capacidades del webview y engancha permisos y notificaciones.
#[cfg(target_os = "linux")]
pub fn configure(app: &tauri::AppHandle, webview: &tauri::webview::Webview, account_id: &str) {
    use webkit2gtk::glib::ObjectExt;
    use webkit2gtk::{
        NotificationExt, NotificationPermissionRequest, PermissionRequestExt, SecurityOrigin,
        SettingsExt, UserMediaPermissionRequest, WebContextExt, WebViewExt,
    };

    let app = app.clone();
    let account_id = account_id.to_string();

    let result = webview.with_webview(move |platform| {
        let native = platform.inner();

        // Solo en depuración y solo con WRUSP_TEST_URL: acepta el certificado
        // autofirmado del servidor de pruebas, necesario para reproducir un
        // contexto seguro (las notificaciones no existen fuera de él).
        #[cfg(debug_assertions)]
        #[allow(deprecated)] // la alternativa moderna no está en este binding
        if std::env::var("WRUSP_TEST_URL").is_ok() {
            use webkit2gtk::TLSErrorsPolicy;
            if let Some(ctx) = WebViewExt::context(&native) {
                ctx.set_tls_errors_policy(TLSErrorsPolicy::Ignore);
            }
        }

        // ── Capacidades para llamadas ───────────────────────────
        if let Some(settings) = WebViewExt::settings(&native) {
            settings.set_enable_media_stream(true);
            settings.set_enable_webrtc(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_media_capabilities(true);
            // Contenido cifrado: algunos flujos de medios lo piden.
            settings.set_enable_encrypted_media(true);
            // Portapapeles: WebKitGTK lo trae cerrado a JavaScript, así que
            // WhatsApp no podía leer lo que se pega ni escribir al copiar.
            // Pegar una captura con Ctrl+V no enviaba nada al chat.
            settings.set_javascript_can_access_clipboard(true);
            // La consola de la página sale por stdout, que `logs` ya dejó
            // apuntando al fichero de registro: los errores del reproductor
            // de WhatsApp quedan consultables desde ajustes.
            settings.set_enable_write_console_messages_to_stdout(true);
            // Sin esto, cada audio y cada vídeo deja un control de medios
            // muerto en el escritorio (ver la función).
            apagar_sesion_multimedia(&settings);
        }

        // ── Permiso de notificaciones, concedido de antemano ────
        // `Notification.requestPermission()` exige un gesto del usuario en
        // WebKit: llamado desde el código de la página devuelve «denied» sin
        // llegar siquiera a preguntarnos. Con esta API se declara el permiso
        // por origen antes de que la página cargue, que es justo para lo que
        // existe.
        if let Some(ctx) = WebViewExt::context(&native) {
            let permitidos = notification_origins();
            let refs: Vec<&SecurityOrigin> = permitidos.iter().collect();
            ctx.initialize_notification_permissions(&refs, &[]);
            // WebKit vuelve a pedir la lista cuando le conviene.
            ctx.connect_initialize_notification_permissions(move |ctx| {
                let permitidos = notification_origins();
                // En release también: es la línea que dice si la página va a
                // arrancar con permiso para notificar.
                eprintln!(
                    "wrusp: WebKit pide los permisos de notificación → concedidos a {}",
                    permitidos
                        .iter()
                        .map(|o| o.host().map(|h| h.to_string()).unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let refs: Vec<&SecurityOrigin> = permitidos.iter().collect();
                ctx.initialize_notification_permissions(&refs, &[]);
            });
        }

        // La concesión de arriba ya no basta por sí sola (ver
        // `contestar_consultas_de_permiso`), pero se deja: es la vía por la que
        // WebKit siembra el proveedor de notificaciones.
        contestar_consultas_de_permiso(&native);
        pedir_proceso_web_nuevo(&native);

        // ── Resto de permisos ───────────────────────────────────
        native.connect_permission_request(|_, request| {
            #[cfg(debug_assertions)]
            println!(
                "wrusp: permiso solicitado: {}",
                ObjectExt::type_(request).name()
            );
            // Cámara, micrófono y notificaciones: lo que WhatsApp necesita.
            // Cualquier otro permiso (geolocalización, por ejemplo) se deja
            // caer para que WebKit lo deniegue.
            if request.is::<UserMediaPermissionRequest>()
                || request.is::<NotificationPermissionRequest>()
            {
                request.allow();
                return true;
            }
            false
        });

        // ── Notificaciones ──────────────────────────────────────
        // Esta señal llega tanto si la notificación nace en la página como si
        // la lanza el service worker, que es lo que el puente de JavaScript no
        // podía interceptar.
        native.connect_show_notification(move |_, notification| {
            let title = notification
                .title()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let body = notification
                .body()
                .map(|s| s.to_string())
                .unwrap_or_default();
            // Sin contenido: el registro no es sitio para los mensajes de
            // nadie. Basta saber que el motor entregó una.
            eprintln!(
                "wrusp: el motor entrega una notificación (cuenta {account_id}, {} y {} caracteres)",
                title.chars().count(),
                body.chars().count()
            );
            crate::shell::notify_from_webkit(&app, &account_id, &title, &body);
            true // ya la mostramos nosotros
        });
    });

    if let Err(err) = result {
        eprintln!("wrusp: no se pudo configurar el webview: {err}");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn configure(_app: &tauri::AppHandle, _webview: &tauri::webview::Webview, _account_id: &str) {
    // En Windows (WebView2) y macOS (WKWebView) tanto la captura de medios como
    // las notificaciones web funcionan sin configuración adicional.
}
