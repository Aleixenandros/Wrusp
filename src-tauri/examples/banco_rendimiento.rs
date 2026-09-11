//! Banco de rendimiento de los scripts que Wrusp inyecta en WhatsApp Web.
//!
//! La aplicación se quedaba parada «de forma constante», y en reposo el
//! proceso web está tranquilo: lo que duele es lo que pasa mientras WhatsApp
//! pinta. WhatsApp es una SPA de React que inserta y quita miles de nodos al
//! desplazarse por un chat, y cada script de Wrusp cuelga de esas mutaciones
//! con un `MutationObserver`, envuelve `setAttribute` o fuerza un cálculo de
//! estilos. Este banco mide cuánto cuesta cada uno por separado y todos
//! juntos, contra una página sintética que muta el DOM al ritmo de WhatsApp.
//!
//! La medida es la que nota el usuario: el retraso de un latido de 16 ms. Si
//! un script bloquea el hilo de la página, el latido llega tarde, y el peor
//! retraso es la congelación más larga que se vería en pantalla.
//!
//! ```sh
//! cargo run --example banco_rendimiento
//! WRUSP_BANCO_SOLO=promo cargo run --example banco_rendimiento
//! ```
//!
//! Necesita sesión gráfica.

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

/// Extrae el primer literal `r#"…"#` que sigue a `firma` en `fuente`.
fn literal(fuente: &str, firma: &str) -> String {
    let inicio = fuente
        .find(firma)
        .unwrap_or_else(|| panic!("no encuentro «{firma}»: cambió de nombre"));
    let cuerpo = &fuente[inicio..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra].to_string()
}

/// Los literales que pasan por `format!` llevan las llaves dobladas.
fn sin_escapar(s: &str) -> String {
    s.replace("{{", "\u{1}")
        .replace("}}", "\u{2}")
        .replace('\u{1}', "{")
        .replace('\u{2}', "}")
}

/// Los scripts tal cual los inyecta Wrusp, sacados de su propia fuente para
/// que el banco no pueda quedarse midiendo una copia vieja.
fn scripts() -> Vec<(&'static str, String)> {
    let browser = include_str!("../src/browser.rs");
    let rail = include_str!("../src/rail.rs");
    let filedrop = include_str!("../src/filedrop.rs");
    let clipboard = include_str!("../src/clipboard.rs");
    vec![
        (
            "disfraz",
            sin_escapar(&literal(browser, "pub fn disguise_script()").replace("{v}", "131")),
        ),
        (
            "webcodecs",
            literal(browser, "pub fn hide_webcodecs_script()"),
        ),
        (
            "video",
            literal(browser, "pub fn fix_large_mp4_blobs_script() -> String {"),
        ),
        (
            "promo",
            literal(browser, "pub fn hide_native_app_promo_script()"),
        ),
        (
            "barra",
            sin_escapar(&literal(rail, "pub fn runtime_script(").replace("{own}", "\"cuenta\"")),
        ),
        ("ficheros", literal(filedrop, "pub const SCRIPT: &str =")),
        (
            "portapapeles",
            literal(clipboard, "pub const SCRIPT: &str ="),
        ),
    ]
}

const INFORME: &str = r#"<script>
  function informe(pruebas, nota) {
    const linea = pruebas.map(([q, ok]) => (ok ? 'OK' : 'FALLO') + ' ' + q).join(' | ');
    document.title = 'WRUSP:' + (pruebas.every(([, ok]) => ok) ? 'TODO BIEN' : 'HAY FALLOS')
      + ' :: ' + linea + (nota ? ' | ' + nota : '');
  }
  // Los scripts anotan por aquí; en el banco no hay nadie escuchando.
  window.__wruspOrden = () => {};
  // Lo que hace Rust en la app: las órdenes a `wrusp://` no navegan. Sin
  // esto, el `fetch` de la barra falla y cae a `location.href`, y la página
  // del banco se iba a otra parte sin llegar a informar.
  const fetchNativo = window.fetch;
  window.fetch = (url, opciones) => String(url).indexOf('wrusp:') === 0
    ? Promise.resolve(new Response(null, { status: 204 }))
    : fetchNativo(url, opciones);
</script>"#;

/// La página y la carga. Imita lo que hace WhatsApp al abrir un chat y
/// desplazarse: una lista de chats a la izquierda, una conversación a la
/// derecha, filas que entran y salen (lista virtualizada), atributos que
/// cambian sin parar, SVG con `calc()` en atributos —el ruido de consola que
/// sale en el registro real— y algún vídeo con fuente `blob:`.
const CARGA: &str = r#"
<div id="app" style="display:flex;height:100vh">
  <div id="side" role="grid" style="width:30%;overflow:auto"></div>
  <div id="main" style="flex:1;overflow:auto"><div id="conv" role="application"></div></div>
</div>
<script>
  const LOTES = 60;
  const FILAS_POR_LOTE = 40;
  const ATRIBUTOS_POR_LOTE = 300;

  let serie = 0;
  function fila() {
    const f = document.createElement('div');
    f.setAttribute('role', 'row');
    f.setAttribute('data-id', 'm' + (serie++));
    f.className = 'fila';
    let html = '<div class="burbuja"><span dir="auto">Mensaje ' + serie + ' de prueba con algo de texto</span>'
      + '<div class="meta"><span>12:' + (serie % 60) + '</span>'
      + '<svg width="16" height="16"><circle cx="8" cy="8" r="calc(50% - 0px)"></circle>'
      + '<rect width="calc(100% - 0px)" height="calc(100% - 0px)"></rect></svg></div>';
    for (let i = 0; i < 6; i++) html += '<div class="x' + i + '"><span>' + i + '</span></div>';
    html += '</div>';
    f.innerHTML = html;
    return f;
  }

  // Estado de partida: unos miles de nodos, como un chat ya abierto.
  const side = document.getElementById('side');
  const conv = document.getElementById('conv');
  for (let i = 0; i < 120; i++) side.appendChild(fila());
  for (let i = 0; i < 200; i++) conv.appendChild(fila());

  // Latido: cada 16 ms se anota cuánto tarde llegó. El peor retraso es la
  // congelación más larga que se vería.
  let ultimo = performance.now();
  let peor = 0, suma = 0, largas = 0, latidos = 0;
  const pulso = setInterval(() => {
    const ahora = performance.now();
    const retraso = ahora - ultimo - 16;
    ultimo = ahora;
    latidos++;
    if (retraso > 0) suma += retraso;
    if (retraso > peor) peor = retraso;
    if (retraso > 50) largas++;
  }, 16);

  const siguiente = () => new Promise((l) => setTimeout(l, 0));
  const fotograma = () => new Promise((l) => requestAnimationFrame(() => l()));

  (async () => {
    await new Promise((l) => setTimeout(l, 500)); // que los scripts arranquen
    ultimo = performance.now(); peor = 0; suma = 0; largas = 0; latidos = 0;
    const t0 = performance.now();
    for (let lote = 0; lote < LOTES; lote++) {
      // Lista virtualizada: entran filas por abajo y salen por arriba.
      for (let i = 0; i < FILAS_POR_LOTE; i++) conv.appendChild(fila());
      for (let i = 0; i < FILAS_POR_LOTE && conv.firstChild; i++) conv.firstChild.remove();
      // React cambia atributos a mansalva.
      const todos = conv.querySelectorAll('.burbuja, .meta span');
      for (let i = 0; i < ATRIBUTOS_POR_LOTE; i++) {
        const n = todos[(i * 7 + lote) % todos.length];
        n.setAttribute('class', n.className.split(' ')[0] + ' v' + ((lote + i) % 5));
        n.setAttribute('aria-label', 'e' + lote);
      }
      // Cada pocos lotes, un vídeo como los de los chats.
      if (lote % 10 === 0) {
        const v = document.createElement('video');
        v.src = URL.createObjectURL(new Blob([new Uint8Array(200000)], { type: 'video/mp4' }));
        conv.lastChild.appendChild(v);
      }
      // La lista de chats también se reordena cuando llega un mensaje.
      side.insertBefore(side.lastChild, side.firstChild);
      await siguiente();
    }
    await fotograma(); await fotograma(); await siguiente();
    const total = performance.now() - t0;
    clearInterval(pulso);
    informe([['la carga termina', true]],
      'total=' + total.toFixed(0) + 'ms peor=' + peor.toFixed(0) + 'ms retraso_acumulado='
      + suma.toFixed(0) + 'ms tareas_largas=' + largas + ' latidos=' + latidos
      + ' nodos=' + document.getElementsByTagName('*').length);
  })();
</script>
"#;

/// Corre una configuración y devuelve la nota del informe.
fn correr(nombre: &str, inyectados: &str) -> Option<String> {
    if let Ok(solo) = std::env::var("WRUSP_BANCO_SOLO") {
        if !nombre.contains(&solo) && nombre != "sin scripts" {
            return None;
        }
    }
    let pagina = format!(
        "<!doctype html><meta charset=\"utf-8\">{INFORME}<script>{inyectados}</script>{CARGA}"
    );
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(1100, 720);
    let vista = WebView::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_enable_write_console_messages_to_stdout(true);
    }
    ventana.add(&vista);
    ventana.show_all();

    let resultado: std::rc::Rc<std::cell::RefCell<Option<String>>> = Default::default();
    let temporizador: std::rc::Rc<std::cell::Cell<Option<gtk::glib::SourceId>>> =
        Default::default();

    let salida = resultado.clone();
    let temporizador_informe = temporizador.clone();
    vista.connect_title_notify(move |v| {
        let Some(titulo) = v.title() else { return };
        let Some(msg) = titulo.strip_prefix("WRUSP:") else {
            return;
        };
        let nota = msg.split(" | ").last().unwrap_or("").to_string();
        *salida.borrow_mut() = Some(nota);
        if let Some(id) = temporizador_informe.take() {
            id.remove();
        }
        gtk::main_quit();
    });
    vista.load_html(&pagina, Some("http://localhost/"));

    let temporizador_vencido = temporizador.clone();
    temporizador.set(Some(gtk::glib::timeout_add_seconds_local(120, move || {
        temporizador_vencido.set(None);
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    })));
    gtk::main();
    if let Some(id) = temporizador.take() {
        id.remove();
    }
    vista.load_html("", None);
    unsafe { ventana.destroy() };
    let nota = resultado.borrow().clone();
    println!(
        "{nombre:<22} {}",
        nota.clone()
            .unwrap_or_else(|| "FALLO (tiempo agotado: la página no llegó a informar)".into())
    );
    nota
}

fn main() {
    gtk::init().expect("no hay sesión gráfica");
    let todos = scripts();
    // Tres pasadas de la línea base: el ruido entre ejecuciones es el umbral
    // por debajo del cual un script no cuesta nada que medir.
    for _ in 0..2 {
        correr("sin scripts", "");
    }
    for (nombre, script) in &todos {
        correr(
            nombre,
            &format!("try {{ {script} }} catch (e) {{ console.error(e); }}"),
        );
    }
    let juntos: String = todos
        .iter()
        .map(|(_, s)| format!("try {{ {s} }} catch (e) {{ console.error(e); }}\n"))
        .collect();
    correr("todos (como Wrusp)", &juntos);
    correr("sin scripts", "");
}
