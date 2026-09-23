//! Banco del disfraz de navegador: qué user-agent ve una página **de WhatsApp**.
//!
//! Lo que ve la página depende del dominio. WebKitGTK trae «apaños por sitio»
//! y a `whatsapp.com` le presenta el user-agent de Safari en Mac, por encima
//! del que fije la aplicación (ADR-044). Un servidor local no lo reproduce, y
//! por eso la primera medición del proyecto (ADR-006) salió limpia mientras
//! WhatsApp seguía viendo un Mac: anunciaba «WhatsApp para Mac», esperaba la
//! tecla Comando en los atajos y no usaba sus propias imágenes de emojis.
//!
//! Este banco carga la misma página con la dirección base de WhatsApp, sin
//! tocar la red, y comprueba lo que ve: primero el motor a solas, que es el
//! control, y después con el disfraz que inyecta Wrusp. Necesita sesión
//! gráfica:
//!
//! ```sh
//! cargo run --example banco_disfraz
//! ```
//!
//! Y el teclado (ADR-047): con la plataforma corregida WhatsApp ve «Safari en
//! Linux», y su tabla de atajos deja «Nuevo chat» en Mayús+N sin
//! modificadores. La última parte monta un comparador con las reglas de esa
//! tabla, detrás del disfraz y de la barra de Wrusp, y pulsa teclas.

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

// El script tal cual lo inyecta Wrusp, no una copia: si cambia, cambia aquí.
#[allow(dead_code)]
#[path = "../src/browser.rs"]
mod browser;

/// El user-agent que fija `shell.rs` en Linux, sacado de su propia fuente.
fn ua_de_la_aplicacion() -> String {
    let fuente = include_str!("../src/shell.rs");
    let marca = "#[cfg(target_os = \"linux\")]\nconst CHROME_UA: &str = \"";
    let inicio = fuente.find(marca).expect("CHROME_UA cambió de sitio") + marca.len();
    let resto = &fuente[inicio..];
    resto[..resto.find('"').expect("sin cierre")].to_string()
}

const WHATSAPP: &str = "https://web.whatsapp.com/";
const LOCAL: &str = "http://localhost/";

/// Lo que mira cada caso. `SCRIPT` es el disfraz, o nada en el control.
const PAGINA: &str = r#"<!doctype html><meta charset="utf-8">
<script>SCRIPT</script>
<script>
  const ua = navigator.userAgent;
  const version = navigator.appVersion;
  // Lo mismo que decide WhatsApp al leer la cadena con ua-parser-js.
  const sistema = /Macintosh|Mac OS X/.test(ua) ? 'Mac OS'
    : /Windows/.test(ua) ? 'Windows'
    : /Linux/.test(ua) ? 'Linux' : 'otro';
  document.title = 'WRUSP:' + JSON.stringify({
    ua, version, sistema, vendor: navigator.vendor,
    safari: /Version\/[\d.]+ Safari\//.test(ua) && !/Chrome\//.test(ua),
  });
</script>"#;

struct Visto {
    ua: String,
    version: String,
    sistema: String,
    vendor: String,
    safari: bool,
}

/// Carga la página con `base` como dirección y devuelve lo que vio.
fn medir(base: &str, script: &str, ua: &str) -> Option<Visto> {
    let visto = std::rc::Rc::new(std::cell::RefCell::new(None));
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(480, 160);
    let vista = WebView::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_user_agent(Some(ua));
        ajustes.set_enable_write_console_messages_to_stdout(true);
    }
    ventana.add(&vista);
    ventana.show_all();

    let salida = visto.clone();
    vista.connect_title_notify(move |v| {
        let Some(titulo) = v.title() else { return };
        let Some(json) = titulo.strip_prefix("WRUSP:") else {
            return;
        };
        *salida.borrow_mut() = Some(json.to_string());
        gtk::main_quit();
    });
    vista.load_html(&PAGINA.replace("SCRIPT", script), Some(base));

    let plazo = gtk::glib::timeout_add_seconds_local(15, || {
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    gtk::main();
    // Si el plazo ya saltó, su fuente ya no existe y no hay nada que retirar.
    if visto.borrow().is_some() {
        plazo.remove();
    }
    ventana.close();

    let json = visto.borrow_mut().take()?;
    let campo = |nombre: &str| -> String {
        let marca = format!("\"{nombre}\":\"");
        let Some(inicio) = json.find(&marca) else {
            return String::new();
        };
        let resto = &json[inicio + marca.len()..];
        resto[..resto.find('"').unwrap_or(resto.len())].to_string()
    };
    Some(Visto {
        ua: campo("ua"),
        version: campo("version"),
        sistema: campo("sistema"),
        vendor: campo("vendor"),
        safari: json.contains("\"safari\":true"),
    })
}

/// La barra de Wrusp tal cual, sacada de su fuente: sus atajos comparten
/// teclado con los de WhatsApp.
fn script_barra() -> String {
    let fuente = include_str!("../src/rail.rs");
    let firma = "pub fn runtime_script(";
    let cuerpo = &fuente[fuente.find(firma).expect("runtime_script cambió de nombre")..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra]
        .replace("{{", "\u{1}")
        .replace("}}", "\u{2}")
        .replace('\u{1}', "{")
        .replace('\u{2}', "}")
        .replace("{own}", "\"cuenta\"")
}

/// Un comparador con las reglas de `WAWebKeyboardShortcuts` para Safari fuera
/// de Mac, que es lo que ve WhatsApp con la plataforma corregida. Escrito aquí
/// a partir de lo leído en su código, con la misma normalización: Mayús y una
/// letra se comparan en mayúscula, sin Mayús en minúscula, y los modificadores
/// tienen que coincidir exactamente. Se registra después que Wrusp, como el
/// código de la página.
const TECLADO: &str = r#"<!doctype html><meta charset="utf-8">
<script>
  window.ordenes = [];
  window.fetch = (url) => { window.ordenes.push(String(url).replace('wrusp://', '')); return Promise.resolve(); };
</script>
<script>DISFRAZ</script>
<script>BARRA</script>
<textarea id="caja"></textarea>
<script>
  const TABLA = [
    { key: 'N', mods: [], accion: 'nuevo-chat' },
    { key: 'N', mods: ['ctrl', 'alt'], accion: 'nuevo-grupo' },
    { key: 'p', mods: ['ctrl', 'alt'], accion: 'perfil' },
    { key: 'U', mods: ['ctrl', 'alt'], accion: 'no-leido' },
    { key: 'Tab', mods: ['ctrl', 'alt'], accion: 'chat-siguiente' },
    { key: '+', mods: ['ctrl'], accion: 'zoom-whatsapp' },
    { key: '0', mods: ['ctrl'], accion: 'zoom-whatsapp' },
  ];
  const normalizar = (k, mayus) => {
    if (mayus && k >= 'a' && k <= 'z') return k.toUpperCase();
    return /^[A-Z]$/.test(k) && !mayus ? k.toLowerCase() : k;
  };
  let accion = null;
  addEventListener('keydown', (e) => {
    const k = normalizar(e.key, e.shiftKey);
    for (const r of TABLA) {
      if (r.key !== k) continue;
      if (r.mods.includes('ctrl') !== e.ctrlKey || r.mods.includes('alt') !== e.altKey || e.metaKey) continue;
      accion = r.accion;
      e.preventDefault();
      return;
    }
  }, true);

  const pulsar = (key, mods) => {
    accion = null;
    window.ordenes.length = 0;
    const e = new KeyboardEvent('keydown', {
      key, bubbles: true, cancelable: true,
      shiftKey: mods.includes('S'), ctrlKey: mods.includes('C'), altKey: mods.includes('A'),
    });
    document.getElementById('caja').dispatchEvent(e);
    return { whatsapp: accion, wrusp: window.ordenes.join(',') || null, escribe: !e.defaultPrevented };
  };
  setTimeout(() => {
    document.getElementById('caja').focus();
    const r = {
      mayusN: pulsar('N', 'S'),
      bloqMayus: pulsar('n', 'S'),
      ctrlAltN: pulsar('n', 'CA'),
      ctrlAltMayusN: pulsar('N', 'CAS'),
      ctrlAltP: pulsar('p', 'CA'),
      ctrlAltMayusU: pulsar('U', 'CAS'),
      ctrlP: pulsar('p', 'C'),
      ctrlMas: pulsar('+', 'C'),
      n: pulsar('n', ''),
    };
    document.title = 'WRUSP:' + JSON.stringify(r);
  }, 50);
</script>"#;

/// Pulsa las teclas con el disfraz y la barra delante, y devuelve lo que
/// hizo cada una como JSON.
fn teclado(base: &str, disfraz: &str, ua: &str) -> Option<String> {
    let visto = std::rc::Rc::new(std::cell::RefCell::new(None));
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(480, 160);
    let vista = WebView::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_user_agent(Some(ua));
        ajustes.set_enable_write_console_messages_to_stdout(true);
    }
    ventana.add(&vista);
    ventana.show_all();
    let salida = visto.clone();
    vista.connect_title_notify(move |v| {
        let Some(titulo) = v.title() else { return };
        if let Some(json) = titulo.strip_prefix("WRUSP:") {
            *salida.borrow_mut() = Some(json.to_string());
            gtk::main_quit();
        }
    });
    let pagina = TECLADO
        .replace("DISFRAZ", disfraz)
        .replace("BARRA", &script_barra());
    vista.load_html(&pagina, Some(base));
    let plazo = gtk::glib::timeout_add_seconds_local(15, || {
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    gtk::main();
    if visto.borrow().is_some() {
        plazo.remove();
    }
    ventana.close();
    let json = visto.borrow_mut().take();
    json
}

/// `{"whatsapp":…,"wrusp":…,"escribe":…}` de una tecla del JSON de `teclado`.
fn tecla<'a>(json: &'a str, nombre: &str) -> &'a str {
    let marca = format!("\"{nombre}\":{{");
    let Some(inicio) = json.find(&marca) else {
        return "";
    };
    let resto = &json[inicio + marca.len() - 1..];
    &resto[..resto.find('}').map_or(resto.len(), |f| f + 1)]
}

/// Imprime las comprobaciones de un caso y devuelve cuántas fallan.
fn informar(nombre: &str, visto: Option<&Visto>, pruebas: &[(&str, bool)]) -> u32 {
    println!("\n── {nombre}");
    let Some(visto) = visto else {
        println!("   FALLO (tiempo agotado: la página no llegó a informar)");
        return 1;
    };
    println!("   ve: {}", visto.ua);
    let mut fallos = 0;
    for (que, ok) in pruebas {
        println!("   {} {que}", if *ok { "OK" } else { "FALLO" });
        if !ok {
            fallos += 1;
        }
    }
    fallos
}

fn main() {
    gtk::init().expect("no hay sesión gráfica");
    let ua = ua_de_la_aplicacion();
    let disfraz = browser::disguise_script();
    println!("La aplicación fija: {ua}");
    let mut fallos = 0;

    // Control: el motor a solas. No es una prueba de Wrusp sino del motor, así
    // que no suma fallos: dice si la corrección sigue haciendo falta.
    let control = medir(WHATSAPP, "", &ua);
    informar(
        "Control: el motor a solas en web.whatsapp.com",
        control.as_ref(),
        &[],
    );
    match control.as_ref().map(|v| v.sistema.as_str()) {
        Some("Mac OS") => println!("   el motor sustituye el user-agent por el de un Mac: hace falta la corrección"),
        Some(otro) => println!(
            "   AVISO: el motor ya no presenta un Mac (ve {otro}). La corrección no hace nada y se puede revisar el ADR-044"
        ),
        None => {}
    }

    let con = medir(WHATSAPP, &disfraz, &ua);
    let pruebas = con.as_ref().map(|v| {
        vec![
            (
                "WhatsApp no ve un Mac ni un Windows: no anuncia su app de escritorio",
                v.sistema == "Linux",
            ),
            (
                "la plataforma es la de verdad",
                v.ua.contains("(X11; Linux x86_64)"),
            ),
            (
                "appVersion dice lo mismo que userAgent",
                !v.version.contains("Macintosh") && v.ua.ends_with(&v.version),
            ),
            (
                "el resto de la identidad no cambia (sigue siendo la que pone el motor)",
                control.as_ref().is_none_or(|c| c.safari == v.safari),
            ),
            (
                "vendor de Google, como hasta ahora",
                v.vendor == "Google Inc.",
            ),
        ]
    });
    fallos += informar(
        "Con el disfraz en web.whatsapp.com",
        con.as_ref(),
        pruebas.as_deref().unwrap_or(&[]),
    );

    let local = medir(LOCAL, &disfraz, &ua);
    let pruebas = local.as_ref().map(|v| {
        vec![(
            "donde el motor no sustituye nada, el disfraz tampoco toca la cadena",
            v.ua == ua,
        )]
    });
    fallos += informar(
        "Con el disfraz en un servidor local",
        local.as_ref(),
        pruebas.as_deref().unwrap_or(&[]),
    );

    // Teclado, con la dirección de WhatsApp para que la plataforma se corrija.
    println!("\n── Atajos con el disfraz y la barra en web.whatsapp.com");
    match teclado(WHATSAPP, &disfraz, &ua) {
        None => {
            println!("   FALLO (tiempo agotado: la página no llegó a informar)");
            fallos += 1;
        }
        Some(json) => {
            let es = |nombre: &str, whatsapp: &str, wrusp: &str, escribe: bool| {
                tecla(&json, nombre)
                    == format!(
                        "{{\"whatsapp\":{whatsapp},\"wrusp\":{wrusp},\"escribe\":{escribe}}}"
                    )
            };
            let pruebas = [
                (
                    "mayusN",
                    "Mayús+N escribe una N en vez de abrir «Nuevo chat»",
                    es("mayusN", "null", "null", true),
                ),
                (
                    "bloqMayus",
                    "con bloqueo de mayúsculas, Mayús+n escribe una n",
                    es("bloqMayus", "null", "null", true),
                ),
                (
                    "ctrlAltN",
                    "Ctrl+Alt+N abre «Nuevo chat», como WhatsApp en Linux",
                    es("ctrlAltN", "\"nuevo-chat\"", "null", false),
                ),
                (
                    "ctrlAltMayusN",
                    "Ctrl+Alt+Mayús+N sigue abriendo «Nuevo grupo»",
                    es("ctrlAltMayusN", "\"nuevo-grupo\"", "null", false),
                ),
                (
                    "ctrlAltP",
                    "Ctrl+Alt+P es el perfil de WhatsApp, no los ajustes de Wrusp",
                    es("ctrlAltP", "\"perfil\"", "null", false),
                ),
                (
                    "ctrlAltMayusU",
                    "Ctrl+Alt+Mayús+U marca como no leído, no añade una cuenta",
                    es("ctrlAltMayusU", "\"no-leido\"", "null", false),
                ),
                (
                    "ctrlP",
                    "Ctrl+P abre los ajustes de Wrusp y WhatsApp no lo ve",
                    es("ctrlP", "null", "\"settings\"", false),
                ),
                (
                    "ctrlMas",
                    "Ctrl++ lo atiende el zoom de Wrusp y no le llega a WhatsApp",
                    es("ctrlMas", "null", "\"zoom/in\"", false),
                ),
                (
                    "n",
                    "la n minúscula es solo una letra",
                    es("n", "null", "null", true),
                ),
            ];
            for (nombre, que, ok) in pruebas {
                if ok {
                    println!("   OK {que}");
                } else {
                    println!("   FALLO {que}: {}", tecla(&json, nombre));
                    fallos += 1;
                }
            }
        }
    }

    println!();
    if fallos > 0 {
        println!("{fallos} comprobación(es) con fallos");
        std::process::exit(1);
    }
    println!("Todo bien.");
}
