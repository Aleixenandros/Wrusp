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

    println!();
    if fallos > 0 {
        println!("{fallos} comprobación(es) con fallos");
        std::process::exit(1);
    }
    println!("Todo bien.");
}
