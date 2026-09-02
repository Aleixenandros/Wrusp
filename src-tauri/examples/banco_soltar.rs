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

/// Maqueta 4 — el fallo de la 0.4.3 a la 0.4.5: WhatsApp se queda el pegado y
/// abre su previsualizador, pero este no lleva ningún marcado que Wrusp
/// reconozca. Antes, al no verlo, se seguía probando el soltado y las entradas
/// de fichero, y cada vía añadía la misma imagen: las fotos se enviaban por
/// duplicado. Un pegado que WhatsApp toma (`preventDefault`) es un pegado
/// hecho, se vea o no.
const PEGADO_SIN_ICONO: &str = r#"
<div id="app">
  <div id="main"><div id="caja" contenteditable="true">escribe aquí</div></div>
  <input id="medios" type="file" accept="image/*,video/*" style="display:none">
  <input id="documentos" type="file" style="display:none">
  <div id="previsualizador" style="display:none"><button>Vale</button></div>
</div>
<script>SCRIPT</script>
<script>
EMPUJE
  let adjuntos = 0;
  const abrir = () => { adjuntos++; document.getElementById('previsualizador').style.display = 'block'; };
  // Todas las vías adjuntan, como en WhatsApp: si Wrusp usa más de una, se nota.
  document.addEventListener('paste', (e) => {
    if (!e.clipboardData || !e.clipboardData.files.length) return;
    e.preventDefault();
    abrir();
  });
  for (const tipo of ['dragenter', 'dragover']) document.addEventListener(tipo, (e) => e.preventDefault(), true);
  document.addEventListener('drop', (e) => { e.preventDefault(); if (e.dataTransfer && e.dataTransfer.files.length) abrir(); }, true);
  for (const id of ['medios', 'documentos'])
    document.getElementById(id).addEventListener('change', function () { if (this.files.length) abrir(); });

  (async () => {
    document.getElementById('caja').focus();
    empujar('captura.png', 'image/png', 'unos bytes de captura');
    await window.__wruspEntregar(-1, -1);   // pegado: sin punto donde soltar
    informe([
      ['se adjunta por el pegado', adjuntos > 0],
      ['y una sola vez, aunque no se reconozca el previsualizador', adjuntos === 1],
      ['queda anotado como pegado', anotado.some((a) => a.startsWith('pegado aceptado'))],
    ], 'adjuntos=' + adjuntos + ' · registro: ' + anotado.join(' / '));
  })();
</script>
"#;

/// Maqueta 5 — la capa «Suelta aquí»: WhatsApp la monta al entrar el arrastre
/// y es ella la que escucha el `drop`. En el registro real el fichero se soltó
/// sobre un enlace de un mensaje y no llegó a ninguna parte: el `drop` tiene
/// que ir sobre lo que haya bajo el ratón *después* del `dragenter`. El botón
/// de enviar lleva aquí el marcado nuevo, para que la confirmación lo vea.
const CAPA_SUELTA_AQUI: &str = r#"
<div id="app">
  <div id="main"><p id="mensaje">Un <a href="/" id="enlace">enlace</a> en un mensaje</p></div>
  <input id="documentos" type="file" style="display:none">
  <div id="previsualizador" style="display:none"><span data-icon="wds-ic-send-filled" style="display:inline-block;width:24px;height:24px"></span></div>
</div>
<script>SCRIPT</script>
<script>
EMPUJE
  let adjuntos = 0;
  let porDonde = '';
  const abrir = (via) => { adjuntos++; porDonde = porDonde || via; document.getElementById('previsualizador').style.display = 'block'; };
  let capa = null;
  document.addEventListener('dragenter', (e) => {
    e.preventDefault();
    if (capa) return;
    capa = document.createElement('div');
    capa.id = 'suelta-aqui';
    capa.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,.3)';
    capa.addEventListener('dragover', (e) => e.preventDefault());
    capa.addEventListener('drop', (e) => {
      e.preventDefault();
      if (e.dataTransfer && e.dataTransfer.files.length) abrir('capa');
    });
    document.body.appendChild(capa);
  }, true);
  document.addEventListener('dragover', (e) => e.preventDefault(), true);
  // El documento corta el evento pero no adjunta: solo la capa adjunta.
  document.addEventListener('drop', (e) => e.preventDefault(), true);
  document.getElementById('documentos').addEventListener('change', function () { if (this.files.length) abrir('entrada'); });

  (async () => {
    const enlace = document.getElementById('enlace').getBoundingClientRect();
    empujar('Informe.pdf', 'application/pdf', 'no soy un pdf de verdad');
    await window.__wruspEntregar(Math.round(enlace.left + 4), Math.round(enlace.top + 4));
    informe([
      ['se adjunta', adjuntos > 0],
      ['por la capa de soltado, no por la puerta de atrás', porDonde === 'capa'],
      ['y una sola vez', adjuntos === 1],
      ['se reconoce el botón de enviar nuevo', anotado.some((a) => a.startsWith('soltado aceptado') && a.includes('botón de enviar'))],
    ], 'adjuntos=' + adjuntos + ' por=' + (porDonde || 'ninguna') + ' · registro: ' + anotado.join(' / '));
  })();
</script>
"#;

/// Maqueta 6 — dos Ctrl+V seguidos, el segundo antes de que termine el
/// primero. En el registro real las dos entregas se solapaban y se pisaban.
/// Mientras hay una en curso, la siguiente se descarta.
const DOS_ENTREGAS_SEGUIDAS: &str = r#"
<div id="app">
  <div id="main"><div id="caja" contenteditable="true">escribe aquí</div></div>
  <input id="medios" type="file" accept="image/*,video/*" style="display:none">
  <div id="previsualizador" style="display:none"><span data-icon="send" style="display:inline-block;width:24px;height:24px"></span></div>
</div>
<script>SCRIPT</script>
<script>
EMPUJE
  let adjuntos = 0;
  // WhatsApp tarda en abrir el previsualizador: el segundo pegado llega antes.
  document.addEventListener('paste', (e) => {
    if (!e.clipboardData || !e.clipboardData.files.length) return;
    e.preventDefault();
    adjuntos++;
    setTimeout(() => { document.getElementById('previsualizador').style.display = 'block'; }, 400);
  });
  for (const tipo of ['dragenter', 'dragover', 'drop']) document.addEventListener(tipo, (e) => e.preventDefault(), true);
  document.getElementById('medios').addEventListener('change', function () { if (this.files.length) adjuntos++; });

  (async () => {
    document.getElementById('caja').focus();
    empujar('captura.png', 'image/png', 'primera');
    const primera = window.__wruspEntregar(-1, -1);
    await new Promise((listo) => setTimeout(listo, 50));
    empujar('captura.png', 'image/png', 'segunda');
    await window.__wruspEntregar(-1, -1);
    await primera;
    informe([
      ['solo se adjunta una vez', adjuntos === 1],
      ['la segunda entrega se descarta y queda anotado', anotado.some((a) => a.startsWith('hay una entrega en curso'))],
    ], 'adjuntos=' + adjuntos + ' · registro: ' + anotado.join(' / '));
  })();
</script>
"#;

fn correr(nombre: &str, maqueta: &str, fallos: std::rc::Rc<std::cell::Cell<u32>>) {
    let nombre_para_tiempo = nombre;
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

    // El temporizador se retira en cuanto la maqueta informa, y la ventana se
    // cierra al terminar: si no, una maqueta que tarda deja su temporizador
    // vivo y su página en marcha, y ambos se cuelan en la siguiente
    // (main_quit a destiempo, «FALLO» atribuido a quien no era).
    let temporizador: std::rc::Rc<std::cell::Cell<Option<gtk::glib::SourceId>>> =
        std::rc::Rc::new(std::cell::Cell::new(None));

    let nombre = nombre.to_string();
    let temporizador_informe = temporizador.clone();
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
        if let Some(id) = temporizador_informe.take() {
            id.remove();
        }
        gtk::main_quit();
    });
    vista.load_html(&pagina, Some("http://localhost/"));

    let nombre_tiempo = nombre_para_tiempo.to_string();
    let temporizador_vencido = temporizador.clone();
    temporizador.set(Some(gtk::glib::timeout_add_seconds_local(30, move || {
        // Al vencer, GLib retira la fuente por su cuenta: que nadie intente
        // retirarla otra vez después.
        temporizador_vencido.set(None);
        // Que la maqueta no conteste es un fallo como cualquier otro: casi
        // siempre significa que el script lanzó y no llegó a informar.
        println!("\n── {nombre_tiempo}");
        println!("   FALLO (tiempo agotado: la maqueta no llegó a informar)");
        sin_respuesta.set(sin_respuesta.get() + 1);
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    })));
    gtk::main();
    if let Some(id) = temporizador.take() {
        id.remove();
    }
    // Fuera la página: que no siga reproduciendo ni informando por detrás.
    vista.load_html("", None);
    unsafe {
        ventana.destroy();
    }
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
    correr(
        "WhatsApp toma el pegado pero su previsualizador no se reconoce",
        PEGADO_SIN_ICONO,
        fallos.clone(),
    );
    correr(
        "La capa «Suelta aquí» es la que escucha el drop",
        CAPA_SUELTA_AQUI,
        fallos.clone(),
    );
    correr("Dos Ctrl+V seguidos", DOS_ENTREGAS_SEGUIDAS, fallos.clone());
    println!();
    if fallos.get() > 0 {
        println!("{} maqueta(s) con fallos", fallos.get());
        std::process::exit(1);
    }
    println!("Todo bien.");
}
