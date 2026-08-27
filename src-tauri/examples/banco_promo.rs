//! Banco de pruebas del script que oculta el anuncio de la app nativa.
//!
//! Ese script es de los más delicados del proyecto: corre sobre el DOM de
//! WhatsApp, decide **hasta dónde** subir para ocultar una tarjeta y se ejecuta
//! en cada repintado. Equivocarse hacia arriba deja el panel de conversación en
//! `display: none` y los chats dejan de abrirse (pasó en la 0.3.9); equivocarse
//! hacia los lados esconde mensajes de la propia conversación.
//!
//! Este banco levanta dos maquetas con la estructura de WhatsApp Web —roles
//! ARIA incluidos, que son el ancla que usa el script— y comprueba lo que debe
//! desaparecer y lo que no. Necesita sesión gráfica:
//!
//! ```sh
//! cargo run --example banco_promo
//! ```

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

/// El script tal cual lo inyecta Wrusp, sacado de su propia fuente para que el
/// banco no pueda quedarse probando una copia vieja.
fn script() -> String {
    let fuente = include_str!("../src/browser.rs");
    let inicio = fuente
        .find("pub fn hide_native_app_promo_script() -> String {")
        .expect("la función cambió de nombre");
    let cuerpo = &fuente[inicio..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra].to_string()
}

/// Maqueta 1 — el fallo de la 0.3.9: el anuncio de la bienvenida vive dentro
/// del panel de conversación, así que ocultarlo subiendo de más deja el panel
/// muerto para el resto de la sesión.
const BIENVENIDA: &str = r#"
<style>
  html,body{margin:0;height:100%} #app{display:flex;height:100%}
  #side{width:400px;border-right:1px solid #ddd;overflow:auto}
  #main{flex:1;display:flex;align-items:center;justify-content:center}
  .intro{text-align:center;max-width:420px}
</style>
<div id="app">
  <div id="side" role="grid"></div>
  <div id="main">
    <div class="intro">
      <h1>WhatsApp Web</h1>
      <p><span>Descarga WhatsApp para Windows</span></p>
    </div>
  </div>
</div>
<script>
  const lista = document.getElementById('side');
  for (let i = 0; i < 300; i++) {
    const d = document.createElement('div');
    d.setAttribute('role', 'listitem');
    d.innerHTML = '<span>Contacto ' + i + '</span>';
    lista.appendChild(d);
  }
</script>
<script>SCRIPT</script>
<script>
  const visible = (id) => {
    const el = document.getElementById(id);
    if (!el) return false;
    return getComputedStyle(el).display !== 'none' && el.getBoundingClientRect().width > 0;
  };
  setTimeout(() => {
    // Abrir un chat: WhatsApp repinta el panel derecho entero.
    const main = document.getElementById('main');
    main.replaceChildren();
    const conv = document.createElement('div');
    conv.setAttribute('role', 'application');
    conv.innerHTML = '<div role="row">Conversación abierta</div>';
    main.appendChild(conv);
    setTimeout(() => {
      const t0 = performance.now();
      for (let i = 0; i < 200; i++) {
        const d = document.createElement('div');
        d.textContent = 'mutación ' + i;
        lista.appendChild(d);
      }
      const coste = (performance.now() - t0).toFixed(1);
      informe([
        ['el anuncio se oculta', !document.querySelector('.intro')
          || getComputedStyle(document.querySelector('.intro')).display === 'none'],
        ['el panel sigue vivo tras abrir un chat', visible('main')],
        ['la lista de chats sigue viva', visible('side')],
      ], '200 mutaciones en ' + coste + ' ms');
    }, 600);
  }, 700);
</script>
"#;

/// Maqueta 2 — con un chat abierto: nada del contenido de la conversación se
/// toca, y el anuncio del código QR y la ventana emergente sí.
const CONVERSACION: &str = r#"
<style>
  html,body{margin:0;height:100%} #app{display:flex;height:100%}
  #side{width:360px;border-right:1px solid #ddd}
  #main{flex:1;display:flex;flex-direction:column}
  .conv{flex:1;padding:10px} .caja{border-top:1px solid #ddd;padding:10px}
  .modal{position:fixed;inset:0;background:rgba(0,0,0,.4)}
  .qr{padding:20px;border:1px solid #ddd;margin:10px;width:300px}
</style>
<div id="app">
  <div id="side" role="grid"><div role="listitem"><span>Contacto</span></div></div>
  <div id="main">
    <div class="conv" id="conv" role="application">
      <div role="row"><div id="msg1">Oye, ¿tú usas WhatsApp para Mac o la web?</div></div>
      <div role="row"><div id="msg2">Me he bajado WhatsApp para Windows y va fino</div></div>
      <div role="row"><div id="msg3"><a href="https://apps.apple.com/app/whatsapp">Mira, aquí está</a></div></div>
    </div>
    <div class="caja" id="caja" role="textbox">Escribe un mensaje</div>
  </div>
</div>
<div class="qr" id="qr">
  <div id="tarjeta-qr">
    <p>Escanea el código QR</p>
    <a href="https://www.whatsapp.com/download" id="enlace-tienda">Descargar WhatsApp para escritorio</a>
  </div>
</div>
<div class="modal" id="modal" role="dialog">
  <div>
    <button aria-label="Cerrar" id="cerrar">x</button>
    <h2>Descarga WhatsApp para Mac</h2>
  </div>
</div>
<script>SCRIPT</script>
<script>
  document.getElementById('cerrar').addEventListener('click', () => {
    const m = document.getElementById('modal');
    if (m) m.remove();
  });
  const visible = (id) => {
    const el = document.getElementById(id);
    if (!el) return false;
    return getComputedStyle(el).display !== 'none' && el.getBoundingClientRect().width > 0;
  };
  setTimeout(() => {
    const pruebas = [
      ['los mensajes del chat no se tocan', visible('msg1') && visible('msg2')],
      ['un enlace a la tienda enviado por alguien tampoco', visible('msg3')],
      ['la caja de escritura sigue viva', visible('caja')],
      ['el anuncio del código QR se oculta', !visible('enlace-tienda')],
      ['la ventana emergente se cierra', !document.getElementById('modal')],
    ];
    // Y la vuelta atrás: si el anuncio desaparece, lo ocultado se devuelve.
    document.getElementById('tarjeta-qr').innerHTML = '<p>Escanea el código QR</p>';
    setTimeout(() => {
      pruebas.push(['lo ocultado vuelve cuando ya no anuncia nada', visible('tarjeta-qr')]);
      informe(pruebas, '');
    }, 500);
  }, 800);
</script>
"#;

const INFORME: &str = r#"<script>
  function informe(pruebas, nota) {
    const linea = pruebas.map(([q, ok]) => (ok ? 'OK' : 'FALLO') + ' ' + q).join(' | ');
    document.title = 'WRUSP:' + (pruebas.every(([, ok]) => ok) ? 'TODO BIEN' : 'HAY FALLOS')
      + ' :: ' + linea + (nota ? ' | ' + nota : '');
  }
</script>"#;

fn correr(nombre: &str, maqueta: &str, fallos: std::rc::Rc<std::cell::Cell<u32>>) {
    let pagina = format!(
        "<!doctype html><meta charset=\"utf-8\">{INFORME}{}",
        maqueta.replace(
            "<script>SCRIPT</script>",
            &format!("<script>{}</script>", script())
        )
    );
    let sin_respuesta = fallos.clone();
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(1100, 700);
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

    gtk::glib::timeout_add_seconds_local(15, move || {
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
        "Bienvenida con el anuncio dentro del panel",
        BIENVENIDA,
        fallos.clone(),
    );
    correr(
        "Conversación abierta, código QR y emergente",
        CONVERSACION,
        fallos.clone(),
    );
    println!();
    if fallos.get() > 0 {
        println!("{} maqueta(s) con fallos", fallos.get());
        std::process::exit(1);
    }
    println!("Todo bien.");
}
