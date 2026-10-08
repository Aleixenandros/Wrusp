//! Evita que los medios de WhatsApp publiquen controles en el escritorio.
//!
//! Desactivar `MediaSession` solo quita la API JavaScript: WebKitGTK 2.54 sigue
//! registrando MPRIS desde `MediaSessionGLib`, incluso para sonidos de avisos.
//! No ofrece un ajuste público para apagar ese registro:
//! https://bugs.webkit.org/show_bug.cgi?id=282000
//!
//! Un proxy D-Bus de esta instancia permite los servicios del escritorio y
//! los nombres propios de Wrusp, pero no poseer `org.mpris.MediaPlayer2.*`.
//! No cambia la configuración del bus del usuario ni la de GNOME.

use std::{
    io::{self, Read},
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::OnceLock,
    time::Duration,
};

static BUS_ORIGINAL: OnceLock<String> = OnceLock::new();

pub struct Proxy {
    proceso: Child,
    vida: UnixStream,
    socket: PathBuf,
}

impl Proxy {
    fn iniciar(bus: &str, identificador: &str) -> io::Result<Self> {
        // XDG_RUNTIME_DIR es privado del usuario; no se publica un socket
        // accesible a otros usuarios en /tmp. El UUID separa instancias.
        let socket = dirs::runtime_dir()
            .ok_or_else(|| io::Error::other("no hay XDG_RUNTIME_DIR"))?
            .join(format!("wrusp-bus-{}", uuid::Uuid::new_v4()));
        let (vida, hijo) = UnixStream::pair()?;
        vida.set_read_timeout(Some(Duration::from_secs(3)))?;
        let proceso = Command::new("xdg-dbus-proxy")
            .arg(bus)
            .arg(&socket)
            .args([
                "--filter",
                // Portales, avisos, gestor de archivos, accesibilidad, temas,
                // dconf, GVfs e insignias del escritorio.
                "--talk=org.freedesktop.*",
                "--talk=org.gnome.*",
                "--talk=org.gtk.*",
                "--talk=org.a11y.*",
                "--talk=ca.desrt.*",
                "--talk=com.canonical.*",
                // libappindicator registra org.kde.StatusNotifierItem-PID-ID.
                "--own=org.kde.*",
                "--fd=0",
            ])
            .arg(format!("--own={identificador}.SingleInstance"))
            // --fd escribe un byte al estar listo y sale cuando cerramos el
            // otro extremo. CLOEXEC evita que los procesos web lo mantengan
            // vivo al salir Wrusp, incluso con process::exit o un fallo.
            .stdin(Stdio::from(OwnedFd::from(hijo)))
            .stdout(Stdio::null())
            .spawn()?;
        let mut proxy = Self {
            proceso,
            vida,
            socket,
        };
        let mut listo = [0];
        proxy.vida.read_exact(&mut listo)?;
        Ok(proxy)
    }

    fn direccion(&self) -> String {
        format!(
            "unix:path={}",
            gtk::gio::dbus_address_escape_value(&self.socket.to_string_lossy())
        )
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.vida.shutdown(std::net::Shutdown::Both);
        // También se recoge el hijo si el arranque falló o agotó el plazo.
        let _ = self.proceso.kill();
        let _ = self.proceso.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Después de inicializar los plugins de Tauri y antes de crear las vistas.
/// Si falta el proxy o no arranca, Wrusp sigue funcionando y deja el motivo
/// en el registro.
pub fn configurar(identificador: &str) -> Option<Proxy> {
    let resultado = (|| {
        let bus = gtk::gio::dbus_address_get_for_bus_sync(
            gtk::gio::BusType::Session,
            None::<&gtk::gio::Cancellable>,
        )
        .map_err(io::Error::other)?;
        let proxy = Proxy::iniciar(&bus, identificador)?;
        let _ = BUS_ORIGINAL.set(bus.to_string());
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", proxy.direccion());
        Ok::<_, io::Error>(proxy)
    })();
    match resultado {
        Ok(proxy) => {
            eprintln!("wrusp: controles multimedia del escritorio desactivados");
            Some(proxy)
        }
        Err(err) => {
            eprintln!("wrusp: no se pudieron desactivar los controles multimedia: {err}");
            None
        }
    }
}

/// Los programas externos usan el bus real, aunque nazcan desde Wrusp: el
/// navegador y el gestor de archivos no deben depender de nuestro proxy.
pub fn restaurar_bus(orden: &mut Command) {
    if let Some(bus) = BUS_ORIGINAL.get() {
        orden.env("DBUS_SESSION_BUS_ADDRESS", bus);
    }
}

/// Las conexiones Rust usan el bus original. xdg-dbus-proxy < 0.1.6 no admite
/// la negociación en paralelo de zbus (upstream #21); GLib, que usa WebKit,
/// sí funciona con esas versiones. También mantiene los avisos y el tema
/// independientes del filtro multimedia.
pub fn conexion_sesion() -> zbus::Result<zbus::blocking::Connection> {
    match BUS_ORIGINAL.get() {
        Some(bus) => zbus::blocking::connection::Builder::address(bus.as_str())?.build(),
        None => zbus::blocking::Connection::session(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::{gio, glib::variant::ToVariant};
    use zbus::blocking::{connection::Builder, fdo::DBusProxy};

    struct Avisos;

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Avisos {
        fn get_capabilities(&self) -> Vec<&str> {
            vec!["body", "actions"]
        }
    }

    #[test]
    #[ignore = "requiere xdg-dbus-proxy y un bus aislado: dbus-run-session -- cargo test -- --ignored"]
    fn el_proxy_bloquea_mpris_y_conserva_la_instancia_y_el_escritorio() {
        // La instancia única se inicializa antes de instalar el filtro.
        let instancia = Builder::session()
            .unwrap()
            .name("wrusp.SingleInstance")
            .unwrap()
            .build()
            .unwrap();
        let proxy = configurar("wrusp").unwrap();
        // El cliente del filtro es GLib, igual que en los procesos WebKit.
        // Con zbus fallaría la propia conexión en Ubuntu (proxy 0.1.5).
        let web = gio::DBusConnection::for_address_sync(
            &proxy.direccion(),
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None::<&gio::DBusAuthObserver>,
            None::<&gio::Cancellable>,
        )
        .unwrap();
        let pedir_nombre = |nombre: &str| {
            web.call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&(nombre, 0_u32).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                2000,
                None::<&gio::Cancellable>,
            )
        };
        assert!(pedir_nombre("org.mpris.MediaPlayer2.wrusp.Prueba").is_err());
        pedir_nombre("org.kde.StatusNotifierItem.Prueba").unwrap();

        // Los clientes Rust creados después del filtro deben poder conectar
        // al escritorio con la misma biblioteca que usa la aplicación.
        let escritorio = conexion_sesion().unwrap();
        escritorio
            .object_server()
            .at("/org/freedesktop/Notifications", Avisos)
            .unwrap();
        escritorio
            .request_name("org.freedesktop.Notifications")
            .unwrap();
        // El filtro tampoco impide que otros programas publiquen MPRIS.
        escritorio
            .request_name("org.mpris.MediaPlayer2.otro")
            .unwrap();
        let conn = conexion_sesion().unwrap();
        let dbus = DBusProxy::new(&conn).unwrap();
        assert_eq!(
            dbus.get_name_owner("wrusp.SingleInstance".try_into().unwrap())
                .unwrap(),
            instancia.unique_name().unwrap().clone()
        );
        assert!(dbus
            .name_has_owner("org.freedesktop.Notifications".try_into().unwrap())
            .unwrap());
        let avisos = zbus::blocking::Proxy::new(
            &conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .unwrap();
        assert_eq!(
            avisos
                .call::<_, _, Vec<String>>("GetCapabilities", &())
                .unwrap(),
            ["body", "actions"]
        );

        let socket = proxy.socket.clone();
        web.close_sync(None::<&gio::Cancellable>).unwrap();
        drop(conn);
        drop(proxy);
        assert!(!socket.exists());
    }
}
