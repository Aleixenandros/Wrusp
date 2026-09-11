//! Banco de memoria de los scripts de vídeo que Wrusp inyecta en WhatsApp.
//!
//! Las sesiones de Wrusp llegaban a tres y cuatro gigas con otros tantos de
//! swap, en un equipo que ya iba justo, y el cuelgue que se notaba era el del
//! sistema paginando. Este banco imita una sesión larga —cientos de vídeos que
//! entran y salen de un chat, con el ratón pasando por encima— y compara la
//! memoria del proceso web con los scripts de Wrusp y sin ellos.
//!
//! Lo que busca en concreto es memoria **retenida**: vídeos que ya salieron
//! del documento y que algo de Wrusp mantiene vivos (un escuchador en un
//! contenedor que WhatsApp recicla, un observador que no suelta a su objetivo,
//! un mapa que no se vacía), cada uno con su copia `data:` dentro.
//!
//! **Cuidado: este banco tumbó la sesión de escritorio el 11-09-2026.** Cada
//! elemento `<video>` con fuente abre su propia conexión al bus de sesión de
//! D-Bus y no la suelta hasta cerrar la vista (medido con `banco_mpris`). La
//! versión original creaba 800, el `dbus-broker` de la sesión llegó a su
//! límite de 1.024 descriptores, murió con «Too many open files» y se llevó
//! por delante gnome-shell y todas las aplicaciones abiertas. Ahora son 40
//! ciclos y un freno que para en seco si el bus crece.
//!
//! ```sh
//! cargo run --example banco_memoria
//! ```
//!
//! Necesita sesión gráfica.

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

fn literal(fuente: &str, firma: &str) -> String {
    let inicio = fuente
        .find(firma)
        .unwrap_or_else(|| panic!("no encuentro «{firma}»: cambió de nombre"));
    let cuerpo = &fuente[inicio..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra].to_string()
}

fn script_video() -> String {
    literal(
        include_str!("../src/browser.rs"),
        "pub fn fix_large_mp4_blobs_script() -> String {",
    )
}

const INFORME: &str = r#"<script>
  function informe(nota) { document.title = 'WRUSP:' + nota; }
  window.__wruspOrden = () => {};
</script>"#;

/// Una sesión larga de chat. Dos patrones que hace WhatsApp: un contenedor que
/// se recicla para vídeo tras vídeo (la burbuja de un visor, un hueco de la
/// lista virtualizada) y filas nuevas que entran por abajo y salen por arriba.
/// El ratón pasa por encima de cada vídeo, que es lo que dispara la
/// preparación anticipada con cupo forzado.
const SESION: &str = r#"
<div id="fijo"></div>
<div id="lista"></div>
<script>
  const CICLOS = 40;   // eran 400: ver la advertencia de la cabecera
  const bytes = new Uint8Array(400 * 1024);      // 400 KiB por vídeo
  for (let i = 0; i < bytes.length; i += 4096) bytes[i] = i & 0xff;
  const fijo = document.getElementById('fijo');
  const lista = document.getElementById('lista');
  const espera = (ms) => new Promise((l) => setTimeout(l, ms));
  const pendientesDeRevocar = [];

  (async () => {
    await espera(500);
    for (let i = 0; i < CICLOS; i++) {
      // A — contenedor reciclado.
      const v = document.createElement('video');
      const urlA = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
      v.src = urlA;
      fijo.replaceChildren(v);
      fijo.dispatchEvent(new PointerEvent('pointerenter'));
      v.dispatchEvent(new PointerEvent('pointerenter'));
      pendientesDeRevocar.push(urlA);

      // B — fila nueva por abajo, la más vieja sale por arriba.
      const fila = document.createElement('div');
      fila.innerHTML = '<div class="burbuja"><video></video></div>';
      const urlB = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
      fila.querySelector('video').src = urlB;
      pendientesDeRevocar.push(urlB);
      lista.appendChild(fila);
      fila.firstChild.dispatchEvent(new PointerEvent('pointerenter'));
      if (lista.children.length > 20) lista.firstChild.remove();

      // WhatsApp revoca las URL de lo que ya no enseña; aquí, con retraso.
      while (pendientesDeRevocar.length > 40) URL.revokeObjectURL(pendientesDeRevocar.shift());

      if (i % 10 === 0) await espera(30);
    }
    // Tiempo para que la cola de preparaciones se vacíe y el recolector pase.
    await espera(6000);
    informe('vídeos en el documento=' + document.querySelectorAll('video').length);
  })();
</script>
"#;

/// RSS en MiB de los procesos web hijos de este banco, el mayor.
fn rss_proceso_web() -> u64 {
    let yo = std::process::id().to_string();
    let mut mayor = 0;
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    for e in dir.flatten() {
        let ruta = e.path();
        let Ok(estado) = std::fs::read_to_string(ruta.join("status")) else {
            continue;
        };
        let es_web = estado
            .lines()
            .next()
            .is_some_and(|l| l.contains("WebKitWebProces"));
        let es_hijo = estado
            .lines()
            .find(|l| l.starts_with("PPid:"))
            .is_some_and(|l| l.split_whitespace().nth(1) == Some(yo.as_str()));
        if !(es_web && es_hijo) {
            continue;
        }
        let rss = estado
            .lines()
            .find(|l| l.starts_with("VmRSS:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            / 1024;
        mayor = mayor.max(rss);
    }
    mayor
}

/// Descriptores del `dbus-broker` de la sesión de este usuario.
fn fds_bus() -> Option<usize> {
    let yo = std::fs::read_to_string("/proc/self/status").ok()?;
    let uid = yo
        .lines()
        .find(|l| l.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .to_string();
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let ruta = e.path();
        let Ok(st) = std::fs::read_to_string(ruta.join("status")) else {
            continue;
        };
        let nombre = st.lines().next().unwrap_or("");
        let suyo = st
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1));
        if nombre.ends_with("dbus-broker") && suyo == Some(uid.as_str()) {
            return std::fs::read_dir(ruta.join("fd")).ok().map(|d| d.count());
        }
    }
    None
}

/// Descriptores de más en el bus antes de parar la prueba.
const FRENO_BUS: usize = 150;

fn correr(nombre: &str, inyectado: &str) {
    let base_bus = fds_bus().unwrap_or(0);
    let pagina = format!(
        "<!doctype html><meta charset=\"utf-8\">{INFORME}<script>{inyectado}</script>{SESION}"
    );
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(900, 600);
    let vista = WebView::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_enable_write_console_messages_to_stdout(false);
    }
    ventana.add(&vista);
    ventana.show_all();

    let nota: std::rc::Rc<std::cell::RefCell<String>> = Default::default();
    let salida = nota.clone();
    vista.connect_title_notify(move |v| {
        let Some(t) = v.title() else { return };
        if let Some(n) = t.strip_prefix("WRUSP:") {
            *salida.borrow_mut() = n.to_string();
            gtk::main_quit();
        }
    });
    vista.load_html(&pagina, Some("http://localhost/"));
    let plazo = gtk::glib::timeout_add_seconds_local(180, || {
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    // El freno: si el bus de sesión crece de más, se para antes de tumbarlo.
    let freno = gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        if fds_bus().unwrap_or(0) > base_bus + FRENO_BUS {
            eprintln!("FRENO: el bus de sesión ha crecido de más; se para el banco");
            std::process::exit(2);
        }
        gtk::glib::ControlFlow::Continue
    });
    gtk::main();
    freno.remove();
    let rss = rss_proceso_web();
    if !nota.borrow().is_empty() {
        plazo.remove();
    }
    println!(
        "{nombre:<18} RSS del proceso web = {rss:>5} MiB · {}",
        nota.borrow()
    );
    vista.load_html("", None);
    unsafe { ventana.destroy() };
    // Que el proceso web anterior muera antes de medir el siguiente.
    let hasta = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < hasta {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn main() {
    gtk::init().expect("no hay sesión gráfica");
    let video = script_video();
    let envuelto = format!("try {{ {video} }} catch (e) {{ console.error(e); }}");
    correr("sin scripts", "");
    correr("script de vídeo", &envuelto);
    correr("sin scripts", "");
    correr("script de vídeo", &envuelto);
}
