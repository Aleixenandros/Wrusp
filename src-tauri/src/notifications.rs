//! Entrega de notificaciones nativas al escritorio.
//!
//! En Linux no basta con conservar el identificador devuelto por `Notify`.
//! GNOME asocia la fuente de notificaciones al nombre único del emisor D-Bus y
//! la elimina cuando ese emisor desaparece. Por eso todas las llamadas pasan
//! por un único worker: su conexión vive hasta que termina Wrusp.

#[cfg(target_os = "linux")]
mod platform {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{mpsc, Mutex, OnceLock},
    };
    use zbus::{blocking::Connection, zvariant::Value};

    const APP_NAME: &str = "Wrusp";
    const APP_ICON: &str = "wrusp";
    // La especificación pide el nombre del fichero sin el sufijo `.desktop`.
    const DESKTOP_ENTRY: &str = "Wrusp";
    const NOTIFICATIONS_BUS: &str = "org.freedesktop.Notifications";
    const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";

    type ClickCallback = Box<dyn Fn(String) + Send + Sync + 'static>;
    static CLICK_CALLBACK: OnceLock<ClickCallback> = OnceLock::new();
    // Cómo se abre un fichero o una carpeta: lo da `main` (el abridor de
    // `config`, sin las variables del motor). Este módulo no depende de él
    // porque `banco_notificaciones` lo compila suelto.
    type OpenCallback = Box<dyn Fn(&std::path::Path) + Send + Sync + 'static>;
    static OPEN_CALLBACK: OnceLock<OpenCallback> = OnceLock::new();

    #[allow(dead_code)] // lo usa la app; `banco_notificaciones`, no
    pub(super) fn on_open(callback: impl Fn(&std::path::Path) + Send + Sync + 'static) {
        let _ = OPEN_CALLBACK.set(Box::new(callback));
    }

    fn abrir(ruta: &std::path::Path) {
        if let Some(abrir) = OPEN_CALLBACK.get() {
            abrir(ruta);
        }
    }
    static DESTINOS: OnceLock<Mutex<HashMap<u32, Destino>>> = OnceLock::new();

    /// A qué lleva pulsar un aviso: a la cuenta del mensaje o al fichero que
    /// se acaba de descargar (PROD-12).
    #[derive(Debug)]
    #[allow(dead_code)] // `banco_notificaciones` solo manda mensajes
    enum Destino {
        Cuenta(String),
        Fichero(PathBuf),
    }

    fn destinos() -> &'static Mutex<HashMap<u32, Destino>> {
        DESTINOS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    #[allow(dead_code)]
    pub(super) fn on_click(callback: impl Fn(String) + Send + Sync + 'static) {
        let _ = CLICK_CALLBACK.set(Box::new(callback));
    }

    #[derive(Debug)]
    struct DesktopNotification {
        destino: Destino,
        title: String,
        body: String,
    }

    type Worker = Result<mpsc::Sender<DesktopNotification>, String>;
    static WORKER: OnceLock<Worker> = OnceLock::new();

    pub(super) fn show(account_id: String, title: String, body: String) {
        enviar(DesktopNotification {
            destino: Destino::Cuenta(account_id),
            title,
            body,
        });
    }

    /// Aviso de descarga terminada, con «Abrir» y «Mostrar en la carpeta».
    #[allow(dead_code)]
    pub(super) fn show_download(ruta: PathBuf) {
        let nombre = ruta
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        enviar(DesktopNotification {
            destino: Destino::Fichero(ruta),
            title: "Descarga terminada".into(),
            body: nombre,
        });
    }

    fn enviar(notification: DesktopNotification) {
        let worker = WORKER.get_or_init(start_worker);
        match worker {
            Ok(sender) => {
                if sender.send(notification).is_err() {
                    eprintln!("wrusp: no se pudo entregar la notificación al worker");
                }
            }
            Err(err) => eprintln!("wrusp: no se pudo iniciar el worker de notificaciones ({err})"),
        }
    }

    fn start_worker() -> Worker {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("wrusp-notifications".into())
            .spawn(move || run_worker(receiver))
            .map_err(|err| err.to_string())?;
        Ok(sender)
    }

    fn spawn_signal_listener(connection: &Connection) {
        let conn = connection.clone();
        let _ = std::thread::Builder::new()
            .name("wrusp-notif-signals".into())
            .spawn(move || {
                // Registrar regla para escuchar la señal ActionInvoked
                let rule = "type='signal',interface='org.freedesktop.Notifications',member='ActionInvoked'";
                let _ = conn.call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "AddMatch",
                    &(rule,),
                );

                for msg in zbus::blocking::MessageIterator::from(&conn) {
                    let Ok(msg) = msg else { continue };
                    let header = msg.header();
                    if let Some(member) = header.member() {
                        if member.as_str() == "ActionInvoked" {
                            if let Ok((id, action)) = msg.body().deserialize::<(u32, String)>() {
                                eprintln!("wrusp: notificación pulsada (id {id}, acción {action})");
                                let destino = destinos().lock().unwrap().remove(&id);
                                match destino {
                                    Some(Destino::Cuenta(cuenta)) => {
                                        if let Some(cb) = CLICK_CALLBACK.get() {
                                            cb(cuenta);
                                        }
                                    }
                                    Some(Destino::Fichero(ruta)) if action == "carpeta" => {
                                        mostrar_en_carpeta(&conn, &ruta)
                                    }
                                    Some(Destino::Fichero(ruta)) => abrir(&ruta),
                                    None => {}
                                }
                            }
                        } else if member.as_str() == "NotificationClosed" {
                            if let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() {
                                let _ = destinos().lock().unwrap().remove(&id);
                            }
                        }
                    }
                }
            });
    }

    fn run_worker(receiver: mpsc::Receiver<DesktopNotification>) {
        let mut connection: Option<Connection> = None;
        while let Ok(notification) = receiver.recv() {
            if connection.is_none() {
                match Connection::session() {
                    Ok(new_connection) => {
                        spawn_signal_listener(&new_connection);
                        connection = Some(new_connection);
                    }
                    Err(err) => {
                        eprintln!(
                            "wrusp: no se pudo conectar al servidor de notificaciones ({err})"
                        );
                        continue;
                    }
                }
            }

            let result = send_notification(
                connection.as_ref().expect("la conexión se acaba de crear"),
                &notification,
            );
            match result {
                Ok(id) => {
                    eprintln!("wrusp: notificación enviada al escritorio (id {id})");
                    destinos().lock().unwrap().insert(id, notification.destino);
                }
                Err(err) => {
                    eprintln!("wrusp: no se pudo mostrar la notificación ({err})");
                    connection = None;
                }
            }
        }
    }

    /// Enseña el fichero seleccionado en el gestor de ficheros
    /// (`org.freedesktop.FileManager1`, que implementan Nautilus, Nemo,
    /// Dolphin…). Corre en el hilo de las señales, nunca en el de GTK. Si no hay
    /// quien lo atienda, se abre la carpeta.
    fn mostrar_en_carpeta(conn: &Connection, ruta: &std::path::Path) {
        let uri = format!("file://{}", ruta.display());
        let mostrado = conn.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowItems",
            &(vec![uri.as_str()], ""),
        );
        if mostrado.is_err() {
            if let Some(carpeta) = ruta.parent() {
                abrir(carpeta);
            }
        }
    }

    fn send_notification(
        connection: &Connection,
        notification: &DesktopNotification,
    ) -> zbus::Result<u32> {
        let (hints, actions) = match notification.destino {
            Destino::Cuenta(_) => (
                HashMap::from([
                    ("desktop-entry", Value::Str(DESKTOP_ENTRY.into())),
                    ("category", Value::Str("im.received".into())),
                    ("sound-name", Value::Str("message-new-instant".into())),
                ]),
                vec!["default", "Abrir"],
            ),
            Destino::Fichero(_) => (
                HashMap::from([
                    ("desktop-entry", Value::Str(DESKTOP_ENTRY.into())),
                    ("category", Value::Str("transfer.complete".into())),
                ]),
                vec!["default", "Abrir", "carpeta", "Mostrar en la carpeta"],
            ),
        };
        let reply = connection.call_method(
            Some(NOTIFICATIONS_BUS),
            NOTIFICATIONS_PATH,
            Some(NOTIFICATIONS_BUS),
            "Notify",
            &(
                APP_NAME,
                0_u32,
                APP_ICON,
                notification.title.as_str(),
                notification.body.as_str(),
                actions,
                hints,
                -1_i32,
            ),
        )?;
        reply.body().deserialize()
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    pub(super) fn on_click(_callback: impl Fn(String) + Send + Sync + 'static) {}
    pub(super) fn show(_account_id: String, _title: String, _body: String) {}
    pub(super) fn show_download(_ruta: std::path::PathBuf) {}
    pub(super) fn on_open(_callback: impl Fn(&std::path::Path) + Send + Sync + 'static) {}
}

#[allow(dead_code)]
pub fn on_notification_click(callback: impl Fn(String) + Send + Sync + 'static) {
    platform::on_click(callback);
}

/// Encola un aviso sin bloquear el hilo principal de la aplicación.
pub fn show(account_id: String, title: String, body: String) {
    platform::show(account_id, title, body);
}

/// Avisa de que una descarga ha terminado, con acciones para abrirla.
#[allow(dead_code)]
pub fn show_download(ruta: std::path::PathBuf) {
    platform::show_download(ruta);
}

/// Cómo abrir el fichero de un aviso de descarga (o su carpeta).
#[allow(dead_code)]
pub fn on_open_path(callback: impl Fn(&std::path::Path) + Send + Sync + 'static) {
    platform::on_open(callback);
}
