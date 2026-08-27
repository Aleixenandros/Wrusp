//! Banco de pruebas de la entrega de ficheros al chat (ver `filedrop`).
//!
//! Arrastrar algo a un chat dejó de funcionar sin decir nada, y lo peor es que
//! el registro decía que sí: el script daba por bueno el primer destino que
//! llamaba a `preventDefault`, y eso lo hace WhatsApp en toda la ventana para
//! que el navegador no abra el fichero en la vista. «Soltado aceptado en P» y
//! el fichero a ninguna parte.
//!
//! Estas maquetas imitan ese comportamiento —alguien que corta el evento sin
//! adjuntar nada— y comprueban lo único que importa: que el adjunto acaba
//! puesto. Necesita sesión gráfica:
//!
//! ```sh
//! cargo run --example banco_soltar
//! ```

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

/// El script tal cual lo inyecta Wrusp, sacado de su propia fuente para que el
/// banco no pueda quedarse probando una copia vieja.
fn script() -> String {
    let fuente = include_str!("../src/filedrop.rs");
    let inicio = fuente
        .find("pub const SCRIPT: &str =")
        .expect("la constante cambió de nombre");
    let cuerpo = &fuente[inicio..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra].to_string()
}

const INFORME: &str = r#"<script>
  function informe(pruebas, nota) {
    const linea = pruebas.map(([q, ok]) => (ok ? 'OK' : 'FALLO') + ' ' + q).join(' | ');
    document.title = 'WRUSP:' + (pruebas.every(([, ok]) => ok) ? 'TODO BIEN' : 'HAY FALLOS')
      + ' :: ' + linea + (nota ? ' | ' + nota : '');
  }
</script>"#;

/// Lo que hace Rust: empujar los bytes en trozos y pedir la entrega. Se imita
/// aquí para que el banco recorra el mismo camino que el de verdad.
const EMPUJE: &str = r#"
  // Lo que Wrusp anota acaba en el registro; aquí se recoge para el informe.
  const anotado = [];
  window.__wruspOrden = (orden) => anotado.push(decodeURIComponent(orden.replace('log/?m=', '')));

  function empujar(nombre, tipo, texto) {
    const bytes = new TextEncoder().encode(texto);
    let b64 = '';
    for (const b of bytes) b64 += String.fromCharCode(b);
    window.__wruspTrozo(0, btoa(b64));
    window.__wruspFichero(0, nombre, tipo);
  }
"#;

/// Maqueta 1 — el fallo real: alguien corta el evento en toda la ventana sin
/// adjuntar nada, y la única vía que queda es la entrada de fichero.
const CORTA_SIN_ADJUNTAR: &str = r#"
<div id="app">
  <div id="main"><p id="mensaje">Un mensaje cualquiera del chat</p></div>
  <input id="medios" type="file" accept="image/*,video/*" style="display:none">
  <input id="documentos" type="file" style="display:none">
  <div id="previsualizador" style="display:none"><span data-icon="send" style="display:inline-block;width:24px;height:24px"></span></div>
</div>
<script>SCRIPT</script>
<script>
EMPUJE
  // Exactamente lo que hace WhatsApp: cortar el evento en toda la ventana para
  // que el navegador no abra el fichero. No adjunta nada.
  for (const tipo of ['dragenter', 'dragover', 'drop'])
    document.addEventListener(tipo, (e) => e.preventDefault(), true);

  let porDonde = '';
  for (const id of ['medios', 'documentos'])
    document.getElementById(id).addEventListener('change', function () {
      if (!this.files.length) return;
      porDonde = id;
      document.getElementById('previsualizador').style.display = 'block';
    });

  (async () => {
    empujar('Fichas.pdf', 'application/pdf', 'no soy un pdf de verdad');
    await window.__wruspEntregar(60, 20);   // soltado sobre el mensaje
    informe([
      ['el fichero acaba adjuntado', porDonde !== ''],
      ['un PDF va a la entrada de documentos', porDonde === 'documentos'],
      ['no se da por bueno un preventDefault a secas',
       !anotado.some((a) => a.startsWith('soltado aceptado'))],
    ], 'por=' + (porDonde || 'ninguna') + ' · registro: ' + anotado.join(' / '));
  })();
</script>
"#;

/// Maqueta 2 — el soltado sí llega a su sitio: entonces se usa esa vía y no se
/// adjunta dos veces por la puerta de atrás.
const SOLTADO_QUE_FUNCIONA: &str = r#"
<div id="app">
  <div id="main"><p id="mensaje">Un mensaje cualquiera del chat</p></div>
  <input id="medios" type="file" accept="image/*,video/*" style="display:none">
  <div id="previsualizador" style="display:none"><span data-icon="send" style="display:inline-block;width:24px;height:24px"></span></div>
</div>
<script>SCRIPT</script>
<script>
EMPUJE
  let adjuntos = 0;
  for (const tipo of ['dragenter', 'dragover'])
    document.addEventListener(tipo, (e) => e.preventDefault(), true);
  // El panel de conversación acepta el soltado de verdad.
  document.getElementById('main').addEventListener('drop', (e) => {
    e.preventDefault();
    if (!e.dataTransfer || !e.dataTransfer.files.length) return;
    adjuntos++;
    document.getElementById('previsualizador').style.display = 'block';
  });
  document.getElementById('medios').addEventListener('change', function () {
    if (this.files.length) adjuntos++;
  });

  (async () => {
    empujar('foto.png', 'image/png', 'unos bytes');
    await window.__wruspEntregar(60, 20);
    informe([
      ['se adjunta por el soltado', adjuntos > 0],
      ['y una sola vez', adjuntos === 1],
      ['queda anotado como soltado', anotado.some((a) => a.startsWith('soltado aceptado'))],
    ], 'adjuntos=' + adjuntos + ' · registro: ' + anotado.join(' / '));
  })();
</script>
"#;

/// Maqueta 3 — una imagen tiene que ir a la entrada de medios, no a la de
/// documentos; equivocarse ahí manda la foto como fichero adjunto.
const ELIGE_LA_ENTRADA: &str = r#"
<div id="app">
  <div id="main"><p id="mensaje">Chat</p></div>
  <input id="documentos" type="file" style="display:none">
  <input id="medios" type="file" accept="image/*,video/*" style="display:none">
  <div id="previsualizador" style="display:none"><span data-icon="send" style="display:inline-block;width:24px;height:24px"></span></div>
</div>
<script>SCRIPT</script>
<script>
EMPUJE
  for (const tipo of ['dragenter', 'dragover', 'drop'])
    document.addEventListener(tipo, (e) => e.preventDefault(), true);
  let porDonde = '';
  for (const id of ['documentos', 'medios'])
    document.getElementById(id).addEventListener('change', function () {
      if (!this.files.length) return;
      porDonde = id;
      document.getElementById('previsualizador').style.display = 'block';
    });

  (async () => {
    empujar('vacaciones.jpg', 'image/jpeg', 'unos bytes de foto');
    await window.__wruspEntregar(60, 20);
    informe([
      ['una imagen va a la entrada de medios', porDonde === 'medios'],
    ], 'por=' + (porDonde || 'ninguna') + ' · registro: ' + anotado.join(' / '));
  })();
</script>
"#;

fn correr(nombre: &str, maqueta: &str, fallos: std::rc::Rc<std::cell::Cell<u32>>) {
    let pagina = format!(
        "<!doctype html><meta charset=\"utf-8\">{INFORME}{}",
        maqueta
            .replace(
                "<script>SCRIPT</script>",
                &format!("<script>{}</script>", script())
            )
            .replace("EMPUJE", EMPUJE)
    );
    let sin_respuesta = fallos.clone();
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(900, 600);
    let vista = WebView::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_enable_write_console_messages_to_stdout(true);
    }
    ventana.add(&vista);
    ventana.show_all();

    let nombre = nombre.to_string();
    vista.connect_title_notify(move |v| {
        let Some(titulo) = v.title() else { return };
        let Some(msg) = titulo.strip_prefix("WRUSP:") else {
            return;
        };
        println!("\n── {nombre}");
        for parte in msg.split(" :: ").nth(1).unwrap_or("").split(" | ") {
            println!("   {parte}");
        }
        if msg.starts_with("HAY FALLOS") {
            fallos.set(fallos.get() + 1);
        }
        gtk::main_quit();
    });
    vista.load_html(&pagina, Some("http://localhost/"));

    gtk::glib::timeout_add_seconds_local(30, move || {
        // Que la maqueta no conteste es un fallo como cualquier otro: casi
        // siempre significa que el script lanzó y no llegó a informar.
        println!("   FALLO (tiempo agotado: la maqueta no llegó a informar)");
        sin_respuesta.set(sin_respuesta.get() + 1);
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    gtk::main();
}

fn main() {
    gtk::init().expect("no hay sesión gráfica");
    let fallos = std::rc::Rc::new(std::cell::Cell::new(0));
    correr(
        "Alguien corta el evento sin adjuntar nada",
        CORTA_SIN_ADJUNTAR,
        fallos.clone(),
    );
    correr(
        "El soltado llega a su sitio",
        SOLTADO_QUE_FUNCIONA,
        fallos.clone(),
    );
    correr(
        "Cada fichero a su entrada",
        ELIGE_LA_ENTRADA,
        fallos.clone(),
    );
    println!();
    if fallos.get() > 0 {
        println!("{} maqueta(s) con fallos", fallos.get());
        std::process::exit(1);
    }
    println!("Todo bien.");
}
