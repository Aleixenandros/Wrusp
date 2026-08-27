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
    static NOTIFICATION_ACCOUNTS: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();

    fn notification_accounts() -> &'static Mutex<HashMap<u32, String>> {
        NOTIFICATION_ACCOUNTS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    #[allow(dead_code)]
    pub(super) fn on_click(callback: impl Fn(String) + Send + Sync + 'static) {
        let _ = CLICK_CALLBACK.set(Box::new(callback));
    }

    #[derive(Debug)]
    struct DesktopNotification {
        account_id: String,
        title: String,
        body: String,
    }

    type Worker = Result<mpsc::Sender<DesktopNotification>, String>;
    static WORKER: OnceLock<Worker> = OnceLock::new();

    pub(super) fn show(account_id: String, title: String, body: String) {
        let worker = WORKER.get_or_init(start_worker);
        match worker {
            Ok(sender) => {
                if sender
                    .send(DesktopNotification {
                        account_id,
                        title,
                        body,
                    })
                    .is_err()
                {
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
                                let account_id = {
                                    let mut map = notification_accounts().lock().unwrap();
                                    map.remove(&id)
                                };
                                if let (Some(cb), Some(id)) = (CLICK_CALLBACK.get(), account_id) {
                                    cb(id);
                                }
                            }
                        } else if member.as_str() == "NotificationClosed" {
                            if let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() {
                                let _ = notification_accounts().lock().unwrap().remove(&id);
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
                    notification_accounts()
                        .lock()
                        .unwrap()
                        .insert(id, notification.account_id);
                }
                Err(err) => {
                    eprintln!("wrusp: no se pudo mostrar la notificación ({err})");
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
        let actions = vec!["default", "Abrir"];
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
}

#[allow(dead_code)]
pub fn on_notification_click(callback: impl Fn(String) + Send + Sync + 'static) {
    platform::on_click(callback);
}

/// Encola un aviso sin bloquear el hilo principal de la aplicación.
pub fn show(account_id: String, title: String, body: String) {
    platform::show(account_id, title, body);
}
