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
                let refs: Vec<&SecurityOrigin> = permitidos.iter().collect();
                ctx.initialize_notification_permissions(&refs, &[]);
            });
        }

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
