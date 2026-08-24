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
        sync::{mpsc, OnceLock},
    };
    use zbus::{blocking::Connection, zvariant::Value};

    const APP_NAME: &str = "Wrusp";
    const APP_ICON: &str = "wrusp";
    // La especificación pide el nombre del fichero sin el sufijo `.desktop`.
    const DESKTOP_ENTRY: &str = "Wrusp";
    const NOTIFICATIONS_BUS: &str = "org.freedesktop.Notifications";
    const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";

    #[derive(Debug)]
    struct DesktopNotification {
        title: String,
        body: String,
    }

    type Worker = Result<mpsc::Sender<DesktopNotification>, String>;
    static WORKER: OnceLock<Worker> = OnceLock::new();

    pub(super) fn show(title: String, body: String) {
        let worker = WORKER.get_or_init(start_worker);
        match worker {
            Ok(sender) => {
                if sender.send(DesktopNotification { title, body }).is_err() {
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

    fn run_worker(receiver: mpsc::Receiver<DesktopNotification>) {
        let mut connection = None;
        while let Ok(notification) = receiver.recv() {
            if connection.is_none() {
                match Connection::session() {
                    Ok(new_connection) => connection = Some(new_connection),
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
                }
                Err(err) => {
                    eprintln!("wrusp: no se pudo mostrar la notificación ({err})");
                    // Si GNOME o el bus se reinician, la siguiente petición
                    // abrirá una conexión nueva. No se reintenta esta para no
                    // arriesgarse a duplicarla si solo se perdió la respuesta.
                    connection = None;
                }
            }
        }
    }

    fn send_notification(
        connection: &Connection,
        notification: &DesktopNotification,
    ) -> zbus::Result<u32> {
        let hints = HashMap::from([
            ("desktop-entry", Value::Str(DESKTOP_ENTRY.into())),
            ("category", Value::Str("im.received".into())),
            ("sound-name", Value::Str("message-new-instant".into())),
        ]);
        let actions = Vec::<&str>::new();
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
    // En Windows y macOS WebKit entrega directamente sus notificaciones al
    // sistema. Esta función queda solo para que el código común siga compilando.
    pub(super) fn show(_title: String, _body: String) {}
}

/// Encola un aviso sin bloquear el hilo principal de la aplicación.
pub fn show(title: String, body: String) {
    platform::show(title, body);
}
