//! Banco del audio en reposo: un `AudioContext` que nadie usa no debe quedarse
//! sonando.
//!
//! El bundle principal de WhatsApp trae un *polyfill* de Web Audio que, al
//! cargar, crea un `AudioContext` para mirar su prototipo y lo abandona. En
//! Chrome y en Safari nace suspendido, porque ningún gesto lo ha autorizado.
//! WebKitGTK lo arranca, y en la instalación del usuario cada cuenta tenía un
//! flujo de PipeWire mandando silencio sin parar (ADR-047).
//!
//! La página hace lo mismo que ese *polyfill* y el banco mide, sin y con el
//! script de Wrusp: el estado del contexto, la CPU del proceso web en reposo,
//! los flujos de audio en marcha de ese proceso en PipeWire, que `resume()`
//! sigue funcionando y que `decodeAudioData` decodifica con el contexto
//! suspendido (es lo que hace WhatsApp con las notas de voz para Safari).
//!
//! ```sh
//! cargo run --example banco_audio
//! ```
//!
//! Necesita sesión gráfica, y `pw-dump` para contar los flujos (sin él se
//! salta esa comprobación).

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

#[allow(dead_code)]
#[path = "../src/browser.rs"]
mod browser;

/// Lo que hace el *polyfill*: crear el contexto al cargar, tocar el prototipo
/// y soltarlo. Aquí se guarda la referencia solo para poder informar.
const PAGINA: &str = r#"<!doctype html><meta charset="utf-8">
<script>SCRIPT</script>
<script>
  window.contexto = new AudioContext();
  const fuente = window.contexto.createBufferSource();
  const esInstancia = window.contexto instanceof AudioContext
    && Object.getPrototypeOf(window.contexto) === AudioContext.prototype
    && typeof fuente.start === 'function';
  document.title = 'LISTO:' + JSON.stringify({ esInstancia });
  // Rust mide el reposo y luego llama a `probar()`.
  window.probar = async () => {
    const r = { reposo: window.contexto.state };
    // Un WAV mínimo en memoria: 0,1 s de silencio a 8 kHz, 16 bits, mono.
    const muestras = 800, datos = new DataView(new ArrayBuffer(44 + muestras * 2));
    const texto = (o, s) => { for (let i = 0; i < s.length; i++) datos.setUint8(o + i, s.charCodeAt(i)); };
    texto(0, 'RIFF'); datos.setUint32(4, 36 + muestras * 2, true); texto(8, 'WAVEfmt ');
    datos.setUint32(16, 16, true); datos.setUint16(20, 1, true); datos.setUint16(22, 1, true);
    datos.setUint32(24, 8000, true); datos.setUint32(28, 16000, true); datos.setUint16(32, 2, true);
    datos.setUint16(34, 16, true); texto(36, 'data'); datos.setUint32(40, muestras * 2, true);
    try {
      const pcm = await window.contexto.decodeAudioData(datos.buffer);
      r.decodifica = pcm.length > 0;
    } catch (e) { r.decodifica = false; }
    try { await window.contexto.resume(); } catch (e) { /* se informa el estado */ }
    r.trasResume = window.contexto.state;
    document.title = 'FIN:' + JSON.stringify(r);
  };
</script>"#;

fn proceso_web() -> Option<u32> {
    let yo = std::process::id().to_string();
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let estado = std::fs::read_to_string(e.path().join("status")).ok()?;
        let web = estado.lines().next()?.contains("WebKitWebProces");
        let hijo = estado
            .lines()
            .find(|l| l.starts_with("PPid:"))?
            .split_whitespace()
            .nth(1)?
            == yo;
        (web && hijo)
            .then(|| e.file_name().to_string_lossy().parse().ok())
            .flatten()
    })
}

fn cpu(pid: u32) -> f64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let resto = stat.rsplit_once(')').map_or("", |(_, r)| r);
    let campos: Vec<f64> = resto
        .split_whitespace()
        .skip(11)
        .take(2)
        .filter_map(|v| v.parse().ok())
        .collect();
    campos.iter().sum::<f64>() / 100.0
}

/// Flujos de audio en marcha de un proceso en PipeWire, o `None` sin `pw-dump`.
fn flujos_en_marcha(pid: u32) -> Option<usize> {
    let salida = std::process::Command::new("pw-dump").output().ok()?;
    let objetos: serde_json::Value = serde_json::from_slice(&salida.stdout).ok()?;
    Some(
        objetos
            .as_array()?
            .iter()
            .filter(|o| {
                let info = &o["info"];
                info["state"] == "running"
                    && info["props"]["application.process.id"].as_u64() == Some(u64::from(pid))
            })
            .count(),
    )
}

struct Resultado {
    es_instancia: bool,
    reposo: String,
    cpu_reposo: f64,
    flujos: Option<usize>,
    fin: String,
}

fn correr(script: &str) -> Option<Resultado> {
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(400, 120);
    // La política de reproducción de las vistas de Wrusp: wry pone «permitir»
    // (`autoplay: true`). Con la de fábrica de WebKitGTK, «permitir sin
    // sonido», el contexto no arranca y el banco no mediría nada.
    let politicas = webkit2gtk::WebsitePolicies::builder()
        .autoplay(webkit2gtk::AutoplayPolicy::Allow)
        .build();
    let vista = WebView::builder().website_policies(&politicas).build();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_enable_write_console_messages_to_stdout(true);
    }
    ventana.add(&vista);
    ventana.show_all();

    let titulo = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let salida = titulo.clone();
    vista.connect_title_notify(move |v| {
        let t = v.title().map(|t| t.to_string()).unwrap_or_default();
        if t.starts_with("LISTO:") || t.starts_with("FIN:") {
            *salida.borrow_mut() = t;
            gtk::main_quit();
        }
    });
    vista.load_html(&PAGINA.replace("SCRIPT", script), Some("http://localhost/"));
    let vencido = std::rc::Rc::new(std::cell::Cell::new(false));
    let marca = vencido.clone();
    let plazo = gtk::glib::timeout_add_seconds_local(20, move || {
        marca.set(true);
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    gtk::main();
    let listo = titulo.borrow().clone();
    let es_instancia = listo.contains("\"esInstancia\":true");

    // Reposo: dos segundos para asentarse y cinco midiendo.
    let esperar = |segundos: u64| {
        let hasta = std::time::Instant::now() + std::time::Duration::from_secs(segundos);
        while std::time::Instant::now() < hasta {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    esperar(2);
    let pid = proceso_web()?;
    let antes = cpu(pid);
    esperar(5);
    let cpu_reposo = cpu(pid) - antes;
    let flujos = flujos_en_marcha(pid);

    vista.evaluate_javascript(
        "window.probar()",
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        |_| {},
    );
    gtk::main();
    // Si el plazo ya venció, GLib retiró su fuente y no hay nada que quitar.
    if !vencido.get() {
        plazo.remove();
    }
    let fin = titulo.borrow().clone();
    ventana.close();
    let reposo = fin
        .split("\"reposo\":\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("")
        .to_string();
    Some(Resultado {
        es_instancia,
        reposo,
        cpu_reposo,
        flujos,
        fin,
    })
}

fn main() {
    gtk::init().expect("no hay sesión gráfica");
    let mut fallos = 0;

    println!("── Control: el motor a solas");
    match correr("") {
        Some(r) => println!(
            "   contexto {} · CPU del proceso web en 5 s de reposo {:.2} s · flujos de audio en marcha {}",
            r.reposo,
            r.cpu_reposo,
            r.flujos.map_or("(sin pw-dump)".into(), |n| n.to_string())
        ),
        None => println!("   (no se pudo medir)"),
    }

    println!("\n── Con el script de Wrusp");
    let script = browser::quiet_idle_audio_script();
    match correr(&script) {
        None => {
            println!("   FALLO (no se pudo medir)");
            fallos += 1;
        }
        Some(r) => {
            println!(
                "   CPU del proceso web en 5 s de reposo {:.2} s · {}",
                r.cpu_reposo, r.fin
            );
            let pruebas = [
                (
                    "el contexto creado sin gesto está suspendido",
                    r.reposo == "suspended",
                ),
                (
                    "no queda ningún flujo de audio en marcha",
                    r.flujos.is_none_or(|n| n == 0),
                ),
                (
                    "sigue siendo un AudioContext para la página (instanceof, prototipo)",
                    r.es_instancia,
                ),
                (
                    "decodeAudioData decodifica con el contexto suspendido",
                    r.fin.contains("\"decodifica\":true"),
                ),
                (
                    "resume() lo pone en marcha cuando la página lo pide",
                    r.fin.contains("\"trasResume\":\"running\""),
                ),
            ];
            for (que, ok) in pruebas {
                println!("   {} {que}", if ok { "OK" } else { "FALLO" });
                if !ok {
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
