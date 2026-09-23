//! Wrusp — cliente de escritorio no oficial de WhatsApp.
//!
//! Envuelve web.whatsapp.com en webviews WebKitGTK y añade bandeja del sistema,
//! multicuenta con sesiones aisladas y tema claro/oscuro. Toda la interfaz vive
//! en **una sola ventana** con una pila de webviews (ver `shell`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod badge;
mod browser;
mod clipboard;
mod config;
mod diagnostico;
mod filedrop;
mod icon;
mod logs;
mod notifications;
mod permissions;
mod rail;
mod runtime;
mod shell;
mod theme;
mod tray;

use config::ConfigState;
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WindowEvent};

/// Baja el rango de los decodificadores de vídeo por hardware para que
/// GStreamer elija los de software.
///
/// La decodificación por VA-API revienta con los vídeos de WhatsApp. Medido en
/// el registro de un caso real: los tres reproductores de la sesión usaron
/// `vah264dec` y los tres acabaron en `vaEndPicture: operation failed` y
/// «Failed to decode data», con avisos de flujo mal formado. Con un fichero
/// suelto el mismo decodificador va bien, así que lo que no digiere es la forma
/// en que WhatsApp entrega el vídeo (troceado, por MSE).
///
/// El decodificador por hardware gana siempre porque su rango es superior al
/// del de software (257 frente a 256), así que se le baja a cero. Comprobado
/// que con eso `decodebin` pasa de `vah264dec` a `avdec_h264`.
///
/// Solo H.264. Se midió en el banco (03-09-2026, `WRUSP_BANCO_VCODEC`) qué
/// pasa con el resto: HEVC se reproduce por `vah265dec` sin problema, y VP9
/// **solo** se reproduce por hardware, el decodificador de software falla con
/// error de decodificación. Rebajar todos los VA-API «por coherencia» habría
/// roto el VP9.
///
/// Si el usuario ya trae su propia preferencia en el entorno, se respeta: puede
/// querer justo lo contrario.
fn preferir_decodificacion_por_software() {
    // `lp` es la variante de bajo consumo, y `vaapi*` el plugin antiguo.
    config::fijar_para_el_motor(
        "GST_PLUGIN_FEATURE_RANK",
        "vah264dec:0,vah264lpdec:0,vaapih264dec:0,vaapidecodebin:0",
    );
}

/// Evita el sink GL de vídeo de WebKitGTK.
///
/// No es lo mismo que `WEBKIT_DISABLE_DMABUF_RENDERER`: esa variable apaga el
/// renderizador DMA-BUF de toda la vista y deja de disparar
/// `requestVideoFrameCallback`, que WhatsApp usa para revelar el reproductor.
/// Esta otra variable solo hace que GStreamer entregue los fotogramas por el
/// sink de memoria normal. En Fedora 44 con WebKitGTK 2.52 y GStreamer 1.28,
/// el sink GL negocia texturas External OES que no puede volver a mapear
/// (`Cannot map External OES textures`); WhatsApp lee el vídeo para pintar y
/// termina recibiendo buffers inválidos. Banco de pruebas, H.264 1024x576:
/// 345 fallos en 345 fotogramas con GL; 354/354 fotogramas y cero fallos con
/// el sink normal. `requestVideoFrameCallback` sigue disparando.
fn evitar_sink_gl_de_video() {
    config::fijar_para_el_motor("WEBKIT_GST_DISABLE_GL_SINK", "1");
}

/// Hace que los procesos web devuelvan al sistema la memoria que sueltan.
///
/// El `malloc` de glibc sube su umbral de `mmap` cada vez que libera un bloque
/// grande, hasta 32 MB: a partir de ahí, las URI de vídeo de varios megas que
/// copia GStreamer, sus búferes y los ficheros decodificados salen del montón
/// y, al liberarse, se quedan en él. Con un hilo por elemento de cada pipeline
/// (más de doscientos en un chat con GIF), cada hilo acaba además con su propia
/// arena. Medido en `banco_chat_videos` al salir de un chat con 12 GIF y un
/// vídeo (ADR-047): el montón se queda en 38 MB en vez de 91, y el proceso web
/// en 331 MB en vez de 449, sin más CPU en él.
///
/// Fijar el umbral apaga ese ajuste dinámico. Solo para los procesos que
/// lance el motor: glibc lee estas variables al arrancar cada proceso, así que
/// el de la interfaz, ya en marcha, no se entera, y es lo que se busca. En el
/// banco, con ellas puestas también en la interfaz, esta gastaba el doble de
/// CPU al componer. Si el usuario trae las suyas, se respetan.
fn devolver_memoria_en_los_procesos_web() {
    for (variable, valor) in [
        ("MALLOC_ARENA_MAX", "2"),
        ("MALLOC_MMAP_THRESHOLD_", "131072"),
        ("MALLOC_TRIM_THRESHOLD_", "131072"),
    ] {
        config::fijar_para_el_motor(variable, valor);
    }
}

/// Pinta el contenido de las páginas con la CPU; la GPU sigue componiendo.
///
/// Con el pintado por GPU (el de fábrica en WebKitGTK 2.52), cada imagen que
/// pasa por la pantalla —fotos, miniaturas, stickers, avatares— acaba como
/// textura en la caché de Skia, y esa caché no se vacía: tiene un presupuesto
/// por hilo de pintado y nada en WebKit la recorta. En la instalación del
/// usuario, el proceso web de una cuenta llevaba 1,19 GB de memoria gráfica a
/// los cuarenta minutos, y creciendo. En este equipo la gráfica va integrada
/// en el procesador, así que esa memoria es RAM, solo que fuera del RSS.
///
/// Medido en `banco_chat_videos` recorriendo 300 fotos distintas (ADR-047):
/// con GPU la memoria gráfica sube de 94 a 449 MB y ahí se queda al salir del
/// chat; con CPU se queda en 51 MB. A cambio, unos 36 MB más de RSS y algo
/// más de CPU mientras se pintan fotos nuevas, repartida en hilos de pintado:
/// el peor bloqueo de la página es el mismo (67 ms). Los GIF y los vídeos no
/// se pintan, se componen, y cuestan lo mismo; `requestVideoFrameCallback`,
/// del que depende el reproductor de WhatsApp, entrega los mismos fotogramas.
///
/// El MSAA de 8 muestras que usa el pintado por GPU en x86_64 no era el
/// culpable: bajarlo a 0 o a 4 no movió la memoria gráfica ni en el banco ni
/// en la app. Quien prefiera la GPU puede arrancar con
/// `WEBKIT_SKIA_ENABLE_CPU_RENDERING=0`, que WebKit entiende y aquí se respeta.
fn pintar_con_la_cpu() {
    config::fijar_para_el_motor("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1");
}

fn main() {
    // Ojo: NO fijar `WEBKIT_DISABLE_DMABUF_RENDERER`. La 0.2.1 lo hacía y con
    // él `requestVideoFrameCallback` no dispara nunca (medido: 0 callbacks
    // frente a ~200 en 8 s sin la variable), y WhatsApp usa esa API para
    // revelar el vídeo al llegar el primer fotograma: el reproductor se
    // quedaba clavado en el póster con el tiempo corriendo. El renderizador
    // general se deja intacto; el que se desactiva más arriba es únicamente el
    // sink GL de vídeo de GStreamer.

    // Lo primero de todo: desde aquí, stdout y stderr quedan en el fichero de
    // registro y los procesos del webview lo heredan.
    logs::init();

    // Decodificación de vídeo por software (ver la función).
    preferir_decodificacion_por_software();

    // Entrega de fotogramas por memoria normal (ver la función).
    evitar_sink_gl_de_video();

    // Memoria liberada que vuelve al sistema en los procesos web (ver la
    // función). Antes de que el motor lance ninguno.
    devolver_memoria_en_los_procesos_web();

    // Pintado por CPU: la caché de texturas del de GPU no se vacía nunca
    // (ver la función).
    pintar_con_la_cpu();

    // Carpeta de temporales configurada por el usuario: debe aplicarse antes de
    // arrancar el webview, porque WebKit lee TMPDIR al lanzar sus procesos.
    config::apply_temp_dir_env();

    // Sin el plugin de notificaciones de Tauri a propósito: inyecta su propio
    // `window.Notification` en TODAS las vistas, y como las de WhatsApp no
    // tienen IPC (ni deben tenerlo), la página recibía
    // «notification.request_permission not allowed», se quedaba sin permiso y
    // no salía ni un aviso. Las notificaciones se envían desde Rust por una
    // conexión D-Bus persistente y llegan por la señal nativa de WebKit (ver
    // `permissions` y `notifications`).
    let mut builder = tauri::Builder::<runtime::Runtime>::default()
        // Canal de las vistas hacia Rust. Se atiende como petición de red y no
        // como navegación: las navegaciones se pisan entre sí cuando llegan
        // varias a la vez (pasó al notificar y actualizar el contador en el
        // mismo instante). Sigue sin conceder IPC a whatsapp.com: aquí solo se
        // aceptan las órdenes conocidas de `shell`.
        .register_uri_scheme_protocol("wrusp", |ctx, request| {
            // La etiqueta de la webview emisora viaja con la petición y la pone
            // el motor: es la identidad en la que se apoya la autorización de
            // las órdenes (una vista no puede hablar en nombre de otra).
            shell::handle_uri(ctx.app_handle(), ctx.webview_label(), request.uri());
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
            accounts::set_account_color,
            accounts::set_account_muted,
            accounts::reorder_accounts,
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
            config::get_diagnostics,
            config::clear_gstreamer_cache,
            config::get_log_tail,
            config::diagnostic_report,
            clipboard::copy_text,
        ])
        .setup(|app| {
            config::debug_assert_identifier(app.handle());
            let cfg = config::load(app.handle());
            app.manage(ConfigState(Mutex::new(cfg)));

            let handle = app.handle().clone();
            notifications::on_notification_click(move |account_id| {
                let h = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    shell::focus_window(&h);
                    let _ = shell::show_account(&h, &account_id);
                });
            });

            shell::create(app.handle())?;
            // Solo la ventana: la vista recién creada ya lleva el tema, y
            // `apply_theme` la recargaría (ver `theme`).
            theme::apply_window_theme(app.handle());
            tray::create(app.handle())?;
            icon::apply(app.handle());

            if std::env::args().any(|a| a == "--hidden") {
                if let Some(w) = app.get_window(shell::MAIN_WINDOW) {
                    let _ = w.hide();
                }
            }

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
            WindowEvent::Resized(_) => {
                shell::sync_bounds(window.app_handle());
                shell::note_geometry(window);
            }
            // Mover no cambia el reparto del espacio, pero sí dónde se abrirá
            // la próxima vez.
            WindowEvent::Moved(_) => shell::note_geometry(window),
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
