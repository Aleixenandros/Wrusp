//! Banco de la comprobación de actualizaciones.
//!
//! Desde la 0.4.17 la comprobación **pregunta** antes de descargar nada, y
//! ofrece el paquete que le sirve a este sistema de los nueve que publica cada
//! versión. Eso son tres cosas que pueden salir mal en silencio: que no se
//! pregunte y se abra algo sin permiso, que se ofrezca el paquete de otra
//! distribución o de otra arquitectura, y que «estás al día» no se distinga de
//! «hay una versión nueva».
//!
//! El banco carga la página de ajustes **de verdad** (`ui/index.html`,
//! `ui/main.js` y `ui/style.css`, empotrados aquí para que no haya copias que
//! envejezcan), con dos postizos: el puente con Rust, que apunta lo que se le
//! pide, y `fetch`, que responde con una versión publicada real de GitHub. No
//! toca la red ni crea ningún `<video>`. Necesita sesión gráfica:
//!
//! ```sh
//! cargo run --example banco_actualizacion
//! ```

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

const INDEX: &str = include_str!("../../ui/index.html");
const MAIN_JS: &str = include_str!("../../ui/main.js");
const STYLE: &str = include_str!("../../ui/style.css");

/// Respuesta de `releases/latest` como la da GitHub, recortada a lo que lee la
/// página. Los nombres y los tamaños son los de la v0.4.16 publicada.
const RELEASE: &str = r##"{
  "tag_name": "v0.4.16",
  "html_url": "https://github.com/Aleixenandros/Wrusp/releases/tag/v0.4.16",
  "assets": [
    { "name": "SHA256SUMS.txt", "size": 1024,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/SHA256SUMS.txt" },
    { "name": "Wrusp-0.4.16-1-x86_64.pkg.tar.zst", "size": 4718592,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp-0.4.16-1-x86_64.pkg.tar.zst" },
    { "name": "Wrusp-0.4.16-1.x86_64.rpm", "size": 4928307,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp-0.4.16-1.x86_64.rpm" },
    { "name": "Wrusp_0.4.16_aarch64.app.tar.gz", "size": 4508876,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_aarch64.app.tar.gz" },
    { "name": "Wrusp_0.4.16_aarch64.dmg", "size": 4613734,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_aarch64.dmg" },
    { "name": "Wrusp_0.4.16_amd64.AppImage", "size": 82634342,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_amd64.AppImage" },
    { "name": "Wrusp_0.4.16_amd64.deb", "size": 5452595,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_amd64.deb" },
    { "name": "Wrusp_0.4.16_x64-setup.exe", "size": 3670016,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_x64-setup.exe" },
    { "name": "Wrusp_0.4.16_x64_en-US.msi", "size": 5033164,
      "browser_download_url": "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_x64_en-US.msi" }
  ]
}"##;

/// Un caso: qué versión se cree la aplicación y en qué sistema se cree que
/// está. `espera_descarga` es la dirección que debe pedirse al pulsar
/// «Descargar», o `None` si no debe aparecer la pregunta siquiera.
struct Caso {
    nombre: &'static str,
    version: &'static str,
    sufijo: &'static str,
    arch: &'static str,
    espera_descarga: Option<&'static str>,
}

const CASOS: &[Caso] = &[
    Caso {
        nombre: "Fedora x86_64 con una versión vieja: pregunta y ofrece el .rpm",
        version: "0.4.15",
        sufijo: ".rpm",
        arch: "x86_64",
        espera_descarga: Some(
            "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp-0.4.16-1.x86_64.rpm",
        ),
    },
    Caso {
        nombre: "Debian x86_64: el mismo aviso ofrece el .deb",
        version: "0.4.15",
        sufijo: ".deb",
        arch: "x86_64",
        espera_descarga: Some(
            "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp_0.4.16_amd64.deb",
        ),
    },
    Caso {
        nombre: "Arch x86_64: ofrece el paquete de pacman",
        version: "0.4.15",
        sufijo: ".pkg.tar.zst",
        arch: "x86_64",
        espera_descarga: Some(
            "https://github.com/Aleixenandros/Wrusp/releases/download/v0.4.16/Wrusp-0.4.16-1-x86_64.pkg.tar.zst",
        ),
    },
    // El caso que justifica la comprobación de arquitectura: en un Fedora de
    // ARM el único `.rpm` publicado es de x86_64. Ofrecerlo sería peor que no
    // ofrecer nada, así que se cae a la lista de la versión.
    Caso {
        nombre: "Fedora aarch64: no hay .rpm de su arquitectura, así que no lo ofrece",
        version: "0.4.15",
        sufijo: ".rpm",
        arch: "aarch64",
        espera_descarga: Some("https://github.com/Aleixenandros/Wrusp/releases"),
    },
    Caso {
        nombre: "Ya estamos en la última: ni pregunta ni abre nada",
        version: "0.4.16",
        sufijo: ".rpm",
        arch: "x86_64",
        espera_descarga: None,
    },
    Caso {
        nombre: "Una versión más nueva que la publicada tampoco pregunta",
        version: "0.5.0",
        sufijo: ".rpm",
        arch: "x86_64",
        espera_descarga: None,
    },
];

/// La página de ajustes con los postizos delante de su propio guion.
fn pagina(caso: &Caso) -> String {
    let postizos = format!(
        r#"
// ── Postizos del banco ───────────────────────────────────────────────────
// El puente con Rust: responde lo justo para que la página arranque y apunta
// todo lo que se le pide, que es lo que el banco comprueba al final.
window.__wruspPedido = [];
window.__TAURI__ = {{
  core: {{
    invoke: (orden, args) => {{
      window.__wruspPedido.push({{ orden, args }});
      switch (orden) {{
        case 'get_about':
          return Promise.resolve({{
            version: '{version}',
            repository: 'https://github.com/Aleixenandros/Wrusp',
            releases: 'https://github.com/Aleixenandros/Wrusp/releases',
            issues: 'https://github.com/Aleixenandros/Wrusp/issues',
            license: 'https://github.com/Aleixenandros/Wrusp/blob/main/LICENSE',
            packageSuffix: '{sufijo}',
            arch: '{arch}',
          }});
        case 'list_accounts': return Promise.resolve([]);
        case 'get_theme': return Promise.resolve('system');
        case 'get_app_icon': return Promise.resolve('whatsapp-logo-2449-orange');
        case 'get_toggles': return Promise.resolve({{}});
        case 'get_folders':
          return Promise.resolve({{
            downloadDir: '', downloadDefault: '/tmp', tempDir: '', tempDefault: '/tmp',
            logDir: '', logDefault: '/tmp',
          }});
        case 'get_diagnostics': return Promise.resolve({{}});
        default: return Promise.resolve(null);
      }}
    }},
  }},
}};

// La versión publicada, sin salir a la red. Cualquier otra dirección es un
// fallo: la página no debería pedir nada más.
window.__wruspFetch = [];
window.fetch = (url, opciones) => {{
  window.__wruspFetch.push(String(url));
  if (String(url).includes('api.github.com')) {{
    return Promise.resolve({{ ok: true, status: 200, json: () => Promise.resolve({release}) }});
  }}
  // El catálogo de iconos, que la página pide al arrancar.
  if (String(url).includes('manifest.json')) {{
    return Promise.resolve({{ ok: true, status: 200, json: () => Promise.resolve(['whatsapp-logo-2449-orange']) }});
  }}
  return Promise.resolve({{ ok: false, status: 404, json: () => Promise.resolve({{}}) }});
}};
"#,
        version = caso.version,
        sufijo = caso.sufijo,
        arch = caso.arch,
        release = RELEASE,
    );

    let esperada = caso.espera_descarga.unwrap_or("");
    let comprobaciones = format!(
        r#"
// ── Lo que se comprueba ──────────────────────────────────────────────────
const informe = (pruebas, nota) => {{
  const linea = pruebas.map(([q, ok]) => (ok ? 'OK' : 'FALLO') + ' ' + q).join(' | ');
  document.title = 'WRUSP:' + (pruebas.every(([, ok]) => ok) ? 'TODO BIEN' : 'HAY FALLOS')
    + ' :: ' + linea + (nota ? ' | ' + nota : '');
}};
const abiertas = () => window.__wruspPedido
  .filter((p) => p.orden === 'open_external').map((p) => p.args.url);
const visible = (id) => {{
  const el = document.getElementById(id);
  // `getComputedStyle` de un hijo de un elemento oculto sigue diciendo su
  // propio display, así que lo que vale es si ocupa sitio en la pantalla.
  return !!el && el.getClientRects().length > 0;
}};

const ESPERADA = '{esperada}';
const HAY_VERSION = ESPERADA !== '';

setTimeout(() => {{
  // Como lo hace una persona: primero se abre «Acerca de». Sin esto el panel
  // está oculto y nada de dentro ocupa sitio, así que «se ve» no mediría nada.
  document.querySelector('.nav button[data-panel="acerca"]').click();
  document.getElementById('check-updates').click();
  setTimeout(() => {{
    const pruebas = [];
    const aviso = document.getElementById('update-prompt');
    pruebas.push(['la pestaña «Acerca de» está a la vista', visible('check-updates')]);
    const estado = document.getElementById('about-update').textContent;
    // Lo primero, en todos los casos: comprobar no descarga nada por su
    // cuenta. La pregunta existe justamente para eso.
    pruebas.push(['comprobar no abre nada sin preguntar', abiertas().length === 0]);
    pruebas.push([
      'se consulta la versión publicada una sola vez',
      window.__wruspFetch.filter((u) => u.includes('api.github.com')).length === 1,
    ]);

    if (!HAY_VERSION) {{
      pruebas.push(['sin versión nueva no hay pregunta', !visible('update-prompt')]);
      pruebas.push(['y lo dice', estado.includes('al día')]);
      informe(pruebas, 'estado: ' + estado.trim());
      return;
    }}

    pruebas.push(['con versión nueva aparece la pregunta', visible('update-prompt')]);
    pruebas.push([
      'la pregunta dice qué versión y pregunta',
      /0\.4\.16/.test(aviso.textContent) && aviso.textContent.includes('¿Quieres descargarla?'),
    ]);
    pruebas.push(['se puede decir que no', visible('update-later')]);
    pruebas.push(['y ver las novedades antes de decidir', visible('update-notes')]);

    document.getElementById('update-download').click();
    setTimeout(() => {{
      const urls = abiertas();
      pruebas.push(['al pulsar Descargar se pide una sola dirección', urls.length === 1]);
      pruebas.push(['y es la que le sirve a este sistema', urls[0] === ESPERADA]);
      pruebas.push([
        'ya descargada, no vuelve a ofrecer los mismos botones',
        !visible('update-download'),
      ]);
      pruebas.push([
        'y dice qué ha pasado',
        /descarga/i.test(document.getElementById('update-detail').textContent),
      ]);
      // «Ahora no» tiene que cerrar el aviso, se haya pulsado o no Descargar.
      document.getElementById('update-later').click();
      pruebas.push(['«Ahora no» cierra el aviso', !visible('update-prompt')]);
      informe(pruebas, 'estado: ' + estado.trim() + ' · pedida: ' + (urls[0] || '(ninguna)'));
    }}, 250);
  }}, 400);
}}, 200);
"#,
        esperada = esperada,
    );

    // La página real, con su hoja de estilos y su guion dentro: así el banco
    // mide lo que se publica y no una copia.
    INDEX
        .replace(
            r#"<link rel="stylesheet" href="style.css" />"#,
            &format!("<style>{STYLE}</style>"),
        )
        .replace(
            r#"<script src="main.js"></script>"#,
            &format!("<script>{postizos}</script><script>{MAIN_JS}</script><script>{comprobaciones}</script>"),
        )
}

fn correr(caso: &Caso, fallos: std::rc::Rc<std::cell::Cell<u32>>) {
    let sin_respuesta = fallos.clone();
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(1000, 700);
    let vista = WebView::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_enable_write_console_messages_to_stdout(true);
        // Sin aceleración: el banco no dibuja nada que la necesite y en
        // sesiones sin GL el motor aborta el proceso entero.
        ajustes.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
    }
    ventana.add(&vista);
    ventana.show_all();

    let nombre = caso.nombre;
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
    vista.load_html(&pagina(caso), Some("http://localhost/"));

    gtk::glib::timeout_add_seconds_local(15, move || {
        // Que la maqueta no conteste es un fallo como cualquier otro: casi
        // siempre significa que el guion lanzó y no llegó a informar.
        println!("   FALLO (tiempo agotado: la maqueta no llegó a informar)");
        sin_respuesta.set(sin_respuesta.get() + 1);
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    gtk::main();
    ventana.close();
}

fn main() {
    gtk::init().expect("no hay sesión gráfica");
    let fallos = std::rc::Rc::new(std::cell::Cell::new(0));
    for caso in CASOS {
        correr(caso, fallos.clone());
    }
    println!();
    if fallos.get() > 0 {
        println!("{} maqueta(s) con fallos", fallos.get());
        std::process::exit(1);
    }
    println!("Todo bien.");
}
