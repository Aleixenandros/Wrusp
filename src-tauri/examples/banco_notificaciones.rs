//! Banco manual de las notificaciones de escritorio.
//!
//! Emite dos avisos con un segundo de separación y mantiene el proceso vivo
//! unos instantes. En GNOME deben verse los dos; si la conexión D-Bus se cerrase
//! después de cada `Notify`, GNOME destruiría la fuente y no llegaría a
//! mostrarlos.
//!
//! ```sh
//! cargo run --example banco_notificaciones
//! ```

#[path = "../src/notifications.rs"]
mod notifications;

use std::{thread, time::Duration};

fn main() {
    notifications::show(
        "test".into(),
        "Prueba Wrusp 1".into(),
        "La conexión D-Bus sigue viva".into(),
    );
    thread::sleep(Duration::from_secs(1));
    notifications::show(
        "test".into(),
        "Prueba Wrusp 2".into(),
        "Los dos avisos comparten el mismo emisor".into(),
    );

    // Da tiempo a comprobar visualmente el banner y, sobre todo, evita que el
    // proceso de prueba cierre la conexión nada más enviar el segundo aviso.
    thread::sleep(Duration::from_secs(5));
}
