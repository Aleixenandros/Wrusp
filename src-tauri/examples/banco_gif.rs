//! Banco de los GIF: ¿se reproducen cuando deben, y vuelven a reproducirse
//! cuando salen y entran de la vista?
//!
//! En uso real, a los GIF «les cuesta» arrancar y a veces no se reproducen.
//! Dos mecanismos pausan un vídeo silencioso fuera de la vista y ninguno es
//! de fiar en WebKitGTK: el `IntersectionObserver` de Wrusp (ADR-040: solo
//! avisa de forma fiable la primera vez) y la suspensión del propio motor
//! (`MediaPlayerPrivateGStreamer::managePlayerSuspend`: un reproductor
//! silenciado y «no visible en el viewport» se suspende, y las órdenes de
//! `play()` se guardan hasta que vuelva a verse). Este banco reproduce lo
//! que hace WhatsApp con un GIF —fuente `blob:` y `autoplay`, en cualquier
//! orden; filas que entran fuera de la vista; desplazarse y volver; quitar
//! una fila y volver a ponerla; una fila oculta que se revela— y comprueba
//! en cada caso si `currentTime` avanza.
//!
//! Variantes, cada una en su proceso porque las variables del motor solo
//! cuentan al lanzar el proceso web:
//!
//! - `actual`: los scripts de Wrusp tal cual;
//! - `invisibles`: lo mismo con `WEBKIT_GST_ALLOW_PLAYBACK_OF_INVISIBLE_VIDEOS=1`,
//!   que apaga la suspensión del motor;
//! - `motor`: sin los scripts de Wrusp, para separar lo que es del motor.
//!
//! ```sh
//! cargo run --release --example banco_gif
//! WRUSP_BANCO_VARIANTES=actual,motor cargo run --release --example banco_gif
//! ```
//!
//! Necesita sesión gráfica y `ffmpeg`. Crea vídeos de verdad (pocos, y con
//! el freno del bus de sesión del ADR-042).

use gtk::prelude::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use webkit2gtk::{SettingsExt, WebContext, WebView, WebViewExt};

#[allow(dead_code)]
#[path = "../src/browser.rs"]
mod browser;

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

/// Los scripts de las vistas de cuenta, como en `shell::create_account_view`.
fn scripts_de_wrusp() -> Vec<String> {
    let barra = literal(include_str!("../src/rail.rs"), "pub fn runtime_script(")
        .replace("{{", "\u{1}")
        .replace("}}", "\u{2}")
        .replace('\u{1}', "{")
        .replace('\u{2}', "}")
        .replace("{own}", "\"cuenta\"");
    vec![
        browser::disguise_script(),
        browser::hide_webcodecs_script(),
        browser::fix_large_mp4_blobs_script(),
        browser::quiet_idle_audio_script(),
        browser::hide_native_app_promo_script(),
        barra,
    ]
}

// ── Vídeos de prueba ─────────────────────────────────────────────────────────

/// Los mismos GIF que `banco_chat_videos` (misma carpeta: si ya están, no se
/// vuelven a generar).
fn generar(ruta: &std::path::Path, args: &[&str]) -> Vec<u8> {
    if !ruta.exists() {
        let estado = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(args)
            .arg(ruta)
            .status()
            .expect("hace falta ffmpeg en el PATH");
        assert!(
            estado.success(),
            "ffmpeg falló generando {}",
            ruta.display()
        );
    }
    std::fs::read(ruta).expect("no se puede leer el vídeo generado")
}

const GIFS: usize = 4;

fn videos() -> HashMap<String, Vec<u8>> {
    let dir = std::env::temp_dir().join("wrusp-banco-chat-videos");
    std::fs::create_dir_all(&dir).expect("sin carpeta temporal");
    let mut mapa = HashMap::new();
    for i in 0..GIFS {
        let fuente = format!(
            "testsrc2=size=480x270:rate=25:duration=4,hue=h={}",
            i * 37 % 360
        );
        let ruta = dir.join(format!("gif{i}.mp4"));
        let datos = generar(
            &ruta,
            &[
                "-f",
                "lavfi",
                "-i",
                &fuente,
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-profile:v",
                "main",
                "-b:v",
                "450k",
                "-movflags",
                "+faststart",
                "-an",
            ],
        );
        mapa.insert(format!("/gif{i}.mp4"), datos);
    }
    mapa
}

// ── Servidor local ───────────────────────────────────────────────────────────

fn servir(mut ficheros: HashMap<String, Vec<u8>>, pagina: String) -> u16 {
    ficheros.insert("/".into(), pagina.into_bytes());
    let ficheros = std::sync::Arc::new(ficheros);
    let oyente = std::net::TcpListener::bind("127.0.0.1:0").expect("sin puerto local");
    let puerto = oyente.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conexion in oyente.incoming().flatten() {
            let ficheros = ficheros.clone();
            std::thread::spawn(move || {
                let mut lector = BufReader::new(&conexion);
                let mut peticion = String::new();
                if lector.read_line(&mut peticion).is_err() {
                    return;
                }
                let mut linea = String::new();
                while lector.read_line(&mut linea).is_ok_and(|n| n > 2) {
                    linea.clear();
                }
                let ruta = peticion.split_whitespace().nth(1).unwrap_or("/");
                let mut salida = &conexion;
                match ficheros.get(ruta) {
                    Some(datos) => {
                        let tipo = if ruta == "/" {
                            "text/html; charset=utf-8"
                        } else {
                            "video/mp4"
                        };
                        let _ = write!(
                            salida,
                            "HTTP/1.1 200 OK\r\nContent-Type: {tipo}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                            datos.len()
                        );
                        let _ = salida.write_all(datos);
                    }
                    None => {
                        let _ = salida.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
            });
        }
    });
    puerto
}

// ── La página ────────────────────────────────────────────────────────────────

/// Cada escenario avisa por el título (`FASE:nombre|en marcha/total|detalle
/// de los parados`) y Rust lo imprime.
const PAGINA: &str = r#"<!doctype html><meta charset="utf-8">
<style>
  body { margin: 0; font: 14px sans-serif; }
  #chat { height: 100vh; overflow-y: auto; }
  .fila { padding: 6px 12px; }
  .burbuja { display: inline-block; background: #d9fdd3; border-radius: 8px; padding: 4px; }
  video { width: 240px; height: 135px; display: block; background: #000; }
</style>
<script>
  const fetchNativo = window.fetch.bind(window);
  window.fetch = (url, opciones) => String(url).indexOf('wrusp:') === 0
    ? Promise.resolve(new Response(null, { status: 204 }))
    : fetchNativo(url, opciones);
</script>
SCRIPTS
<div id="chat"></div>
<script>
  const GIF = CUANTOS_GIF;
  const espera = (ms) => new Promise((l) => setTimeout(l, ms));
  const chat = document.getElementById('chat');
  let serie = 0;

  function filaTexto() {
    const f = document.createElement('div');
    f.className = 'fila';
    f.innerHTML = '<div class="burbuja"><span>Mensaje ' + (serie++) + ' con algo de texto para ocupar sitio en la conversación.</span></div>';
    return f;
  }
  // Un GIF de WhatsApp: `blob:` por atributo, `autoplay`, `loop`, sin sonido.
  // React pone los atributos en el orden del JSX, que no se conoce: se prueban
  // los dos.
  function filaGif(blob, srcPrimero) {
    const f = document.createElement('div');
    f.className = 'fila';
    f.innerHTML = '<div class="burbuja"><video loop muted playsinline></video><span>GIF ' + (serie++) + (srcPrimero ? ' (src antes)' : ' (autoplay antes)') + '</span></div>';
    const v = f.querySelector('video');
    v.muted = true;
    f.dataset.url = URL.createObjectURL(new Blob([blob], { type: 'video/mp4' }));
    if (srcPrimero) {
      v.setAttribute('src', f.dataset.url);
      v.setAttribute('autoplay', '');
    } else {
      v.setAttribute('autoplay', '');
      v.setAttribute('src', f.dataset.url);
    }
    return f;
  }
  const abajo = () => { chat.scrollTop = chat.scrollHeight; };
  const arriba = () => { chat.scrollTop = 0; };

  // ¿Se mueve `currentTime` en `ms`? Con `loop`, un clip de 4 s vuelve a 0,
  // así que se toman varias muestras y basta con que cambie alguna. Lo
  // demás, para entender a los que no.
  async function medir(videos, ms) {
    const muestras = videos.map(() => []);
    for (let k = 0; k < 6; k++) {
      videos.forEach((v, i) => muestras[i].push(v.currentTime));
      await espera(ms / 6);
    }
    return videos.map((v, i) => ({
      avanza: muestras[i].some((t, k) => k > 0 && t !== muestras[i][k - 1]),
      pausado: v.paused,
      datos: v.readyState,
      red: v.networkState,
      t: Number(v.currentTime.toFixed(2)),
      retenida: v.__wruspRetenida !== undefined,
      src: (v.getAttribute('src') || '').slice(0, 5) || 'sin src',
      visible: (() => { const r = v.getBoundingClientRect(); return r.bottom > 0 && r.top < innerHeight; })(),
    }));
  }
  async function fase(nombre, videos, ms) {
    const r = await medir(videos, ms);
    const ok = r.filter((x) => x.avanza).length;
    document.title = 'FASE:' + nombre + '|' + ok + '/' + r.length + '|' + JSON.stringify(r.filter((x) => !x.avanza));
    await espera(300);
    return r;
  }

  (async () => {
    const blobs = await Promise.all(
      Array.from({ length: GIF }, (_, i) => fetch('/gif' + i + '.mp4').then((r) => r.blob())));
    for (let i = 0; i < 60; i++) chat.appendChild(filaTexto());
    abajo();
    await espera(800);

    // 1. Entran a la vista, como al abrir un chat.
    const primeros = [];
    for (let i = 0; i < 4; i++) {
      const f = filaGif(blobs[i % GIF], i % 2 === 0);
      chat.appendChild(f);
      chat.appendChild(filaTexto());
      primeros.push(f.querySelector('video'));
    }
    abajo();
    await espera(3000);
    await fase('visibles', primeros, 1500);

    // 2. Entran fuera de la vista (la conversación está arriba) y luego se
    // baja hasta ellos.
    arriba();
    await espera(500);
    const segundos = [];
    for (let i = 0; i < 4; i++) {
      const f = filaGif(blobs[i % GIF], i % 2 === 1);
      chat.appendChild(f);
      chat.appendChild(filaTexto());
      segundos.push(f.querySelector('video'));
    }
    await espera(3000);
    await fase('fuera', segundos, 1500);
    abajo();
    await espera(1500);
    await fase('fuera→dentro', segundos, 1500);

    // 3. Se sube (salen de la vista) y se vuelve a bajar.
    const todos = primeros.concat(segundos);
    arriba();
    await espera(3000);
    await fase('dentro→fuera', todos, 1500);
    abajo();
    await espera(1500);
    await fase('fuera→dentro 2', todos, 1500);

    // 4. Una fila sale del documento más de 4 s (Wrusp desmonta su
    // reproductor) y vuelve.
    const quitados = primeros.slice(0, 2);
    const filas = quitados.map((v) => v.closest('.fila'));
    for (const f of filas) f.remove();
    await espera(5500);
    for (const f of filas) chat.appendChild(f);
    abajo();
    await espera(3000);
    await fase('quitar y volver', quitados, 1500);

    // 5. Una fila que entra oculta y se revela después.
    const ocultos = [];
    for (let i = 0; i < 2; i++) {
      const f = filaGif(blobs[i], i === 0);
      f.style.display = 'none';
      chat.appendChild(f);
      ocultos.push(f.querySelector('video'));
    }
    await espera(1000);
    for (const v of ocultos) v.closest('.fila').style.display = '';
    abajo();
    await espera(3000);
    await fase('oculto→visible', ocultos, 1500);

    // 6. WhatsApp recicla un <video> que ya tiene reproductor y le pone otro
    // GIF (`relevar` en el script de Wrusp).
    const reciclados = ocultos;
    for (let i = 0; i < reciclados.length; i++) {
      const url = URL.createObjectURL(new Blob([blobs[(i + 2) % GIF]], { type: 'video/mp4' }));
      reciclados[i].setAttribute('src', url);
    }
    await espera(3000);
    await fase('reciclado', reciclados, 1500);

    // 7. `autoplay` por propiedad y después de la fuente, como podría hacer
    // React.
    const porPropiedad = [];
    for (let i = 0; i < 2; i++) {
      const f = document.createElement('div');
      f.className = 'fila';
      f.innerHTML = '<div class="burbuja"><video loop muted playsinline></video><span>GIF propiedad</span></div>';
      const v = f.querySelector('video');
      v.muted = true;
      v.src = URL.createObjectURL(new Blob([blobs[i]], { type: 'video/mp4' }));
      v.autoplay = true;
      chat.appendChild(f);
      porPropiedad.push(v);
    }
    abajo();
    await espera(3000);
    await fase('autoplay prop.', porPropiedad, 1500);

    // 8. Varios ciclos de salir y volver: el observador de intersección de
    // Wrusp solo avisa de forma fiable la primera vez (ADR-040). Se miran
    // los que quedan a la vista abajo del todo.
    const abajoDelTodo = () => Array.from(chat.querySelectorAll('video')).filter((v) => {
      const r = v.getBoundingClientRect();
      return r.bottom > 0 && r.top < innerHeight;
    });
    for (let ciclo = 1; ciclo <= 3; ciclo++) {
      arriba();
      await espera(1500);
      abajo();
      await espera(1500);
      await fase('ciclo ' + ciclo, abajoDelTodo(), 1500);
    }

    // 9. La vista se oculta y se vuelve a mostrar (cambiar de cuenta en
    // Wrusp): Rust esconde el widget 2 s al ver el título.
    const visiblesAntes = abajoDelTodo();
    document.title = 'OCULTAR';
    await espera(4500);
    await fase('ocultar/mostrar', visiblesAntes, 1500);

    // De paso, la anchura que ve la página con la barra: `innerWidth` debe
    // ser 60 px menos que el documento (ver `rail.rs`).
    document.title = 'FIN:' + (window.__wruspDesmontados || 0) + '|' + innerWidth + '/' + document.documentElement.clientWidth;
  })();
</script>"#;

/// Descriptores del `dbus-broker` de la sesión de este usuario.
fn fds_bus() -> Option<usize> {
    let uid = std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .to_string();
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(st) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        let mismo_uid = st
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            == Some(uid.as_str());
        if st.lines().next().unwrap_or("").ends_with("dbus-broker") && mismo_uid {
            return std::fs::read_dir(e.path().join("fd"))
                .ok()
                .map(|d| d.count());
        }
    }
    None
}

const FRENO_BUS: usize = 150;

// ── Variantes ────────────────────────────────────────────────────────────────

type Variante = (&'static str, Vec<(&'static str, &'static str)>, bool);

fn variantes() -> Vec<Variante> {
    vec![
        ("actual", vec![], true),
        (
            "invisibles",
            vec![("WEBKIT_GST_ALLOW_PLAYBACK_OF_INVISIBLE_VIDEOS", "1")],
            true,
        ),
        ("motor", vec![], false),
    ]
}

fn correr_variante(nombre: &str, con_scripts: bool) {
    gtk::init().expect("no hay sesión gráfica");
    let scripts: String = if con_scripts {
        scripts_de_wrusp()
            .into_iter()
            .map(|s| {
                format!(
                    "<script>try {{\n{s}\n}} catch (e) {{ console.error('wrusp', e); }}</script>\n"
                )
            })
            .collect()
    } else {
        String::new()
    };
    let pagina = PAGINA
        .replace("SCRIPTS", &scripts)
        .replace("CUANTOS_GIF", &GIFS.to_string());
    let puerto = servir(videos(), pagina);

    let contexto = WebContext::new_ephemeral();
    // Como wry: reproducción automática permitida.
    let politicas = webkit2gtk::WebsitePolicies::builder()
        .autoplay(webkit2gtk::AutoplayPolicy::Allow)
        .build();
    let vista = WebView::builder()
        .web_context(&contexto)
        .website_policies(&politicas)
        .build();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        ajustes.set_enable_media_stream(true);
        ajustes.set_enable_webrtc(true);
        ajustes.set_enable_mediasource(true);
        ajustes.set_enable_media_capabilities(true);
        ajustes.set_enable_encrypted_media(true);
        ajustes.set_enable_smooth_scrolling(true);
        ajustes.set_enable_page_cache(true);
        ajustes.set_enable_write_console_messages_to_stdout(false);
        ajustes.set_user_agent(Some(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        ));
    }
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(1100, 720);
    ventana.add(&vista);
    ventana.show_all();

    let base_bus = fds_bus().unwrap_or(0);
    let terminado = std::rc::Rc::new(std::cell::Cell::new(false));
    let fin = terminado.clone();
    let nombre = nombre.to_string();
    vista.connect_title_notify(move |v| {
        let Some(titulo) = v.title() else { return };
        if titulo == "OCULTAR" {
            // Como `webview.hide()`/`show()` de wry en Linux.
            v.hide();
            let vista = v.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(2000), move || {
                vista.show();
            });
            return;
        }
        if let Some(resto) = titulo.strip_prefix("FIN:") {
            let (desmontados, anchura) = resto.split_once('|').unwrap_or((resto, ""));
            println!(
                "{nombre:<11} fin · desmontados por Wrusp {desmontados} · innerWidth/documento {anchura}"
            );
            fin.set(true);
            gtk::main_quit();
            return;
        }
        let Some(resto) = titulo.strip_prefix("FASE:") else {
            return;
        };
        let mut partes = resto.splitn(3, '|');
        let fase = partes.next().unwrap_or("");
        let cuenta = partes.next().unwrap_or("");
        let parados = partes.next().unwrap_or("");
        println!(
            "{nombre:<11} {fase:<16} en marcha {cuenta:<4}{}",
            if parados == "[]" {
                String::new()
            } else {
                format!(" · parados: {parados}")
            }
        );
    });
    vista.load_uri(&format!("http://127.0.0.1:{puerto}/"));

    let plazo = gtk::glib::timeout_add_seconds_local(120, || {
        println!("   FALLO: tiempo agotado, la página no terminó");
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    let freno = gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        if fds_bus().unwrap_or(0) > base_bus + FRENO_BUS {
            eprintln!("FRENO: el bus de sesión ha crecido de más; se para el banco");
            std::process::exit(2);
        }
        gtk::glib::ControlFlow::Continue
    });
    gtk::main();
    freno.remove();
    if terminado.get() {
        plazo.remove();
    }
}

fn main() {
    // Las condiciones de Wrusp (`main.rs`).
    for (variable, valor) in [
        (
            "GST_PLUGIN_FEATURE_RANK",
            "vah264dec:0,vah264lpdec:0,vaapih264dec:0,vaapidecodebin:0",
        ),
        ("WEBKIT_GST_DISABLE_GL_SINK", "1"),
        ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1"),
        ("GST_DEBUG", "0"),
    ] {
        if std::env::var_os(variable).is_none() {
            std::env::set_var(variable, valor);
        }
    }

    if let Ok(nombre) = std::env::var("WRUSP_BANCO_VARIANTE_HIJA") {
        let con_scripts = variantes()
            .into_iter()
            .find(|(n, _, _)| *n == nombre)
            .map(|(_, _, s)| s)
            .unwrap_or(true);
        correr_variante(&nombre, con_scripts);
        return;
    }

    let pedidas = std::env::var("WRUSP_BANCO_VARIANTES").unwrap_or_default();
    let pedidas: Vec<&str> = pedidas.split(',').filter(|s| !s.is_empty()).collect();
    let exe = std::env::current_exe().expect("sin ruta propia");
    for (nombre, entorno, _) in variantes() {
        if !pedidas.is_empty() && !pedidas.contains(&nombre) {
            continue;
        }
        let mut orden = std::process::Command::new(&exe);
        orden.env("WRUSP_BANCO_VARIANTE_HIJA", nombre);
        for (variable, valor) in entorno {
            orden.env(variable, valor);
        }
        let estado = orden.status().expect("no se pudo lanzar la variante");
        if !estado.success() {
            println!("{nombre:<11} FALLO: la variante terminó con {estado}");
        }
    }
}
