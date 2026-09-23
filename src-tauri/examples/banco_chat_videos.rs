//! Banco de un chat con muchos vídeos: memoria, GPU, CPU y bloqueos del
//! proceso web, con los scripts de Wrusp y los ajustes del motor que pone la
//! aplicación.
//!
//! En uso real, entrar en un chat con muchos GIF y vídeos colgaba Wrusp. La
//! sesión anterior al cuelgue llegó a 4,1 GB de memoria y 4 GB de swap
//! (`systemd`, 22-09-2026), y a los diez minutos de arrancar el proceso web de
//! la cuenta ya tenía 1,4 GB de RSS más 900 MB de memoria gráfica (GTT) sin que
//! nadie lo mirase. Este banco reproduce lo que hace WhatsApp en ese chat —GIF
//! que se reproducen solos, un vídeo que se pulsa, la sonda con la que mide un
//! vídeo, filas que entran y salen al desplazarse— y mide el proceso web en
//! cada fase:
//!
//! - RSS, memoria anónima y el montón de `malloc` (`[heap]`);
//! - memoria gráfica residente (GTT y VRAM) y tiempo de GPU, del `fdinfo` del
//!   dispositivo DRM;
//! - CPU del proceso web y del de la interfaz;
//! - el peor retraso de un latido de 16 ms en la página, que es la congelación
//!   más larga que se vería.
//!
//! Cada variante corre en un proceso propio, porque las variables de entorno
//! solo cuentan al lanzar el proceso web:
//!
//! ```sh
//! cargo run --release --example banco_chat_videos
//! WRUSP_BANCO_VARIANTES=actual,malloc cargo run --release --example banco_chat_videos
//! WRUSP_BANCO_GIF=6 cargo run --release --example banco_chat_videos
//! WRUSP_BANCO_IMAGENES=300 cargo run --release --example banco_chat_videos
//! ```
//!
//! Con `WRUSP_BANCO_IMAGENES` se añade una fase de fotos: imágenes distintas
//! que pasan por la conversación, que es lo que llena las cachés de texturas
//! del pintado por GPU.
//!
//! Necesita sesión gráfica y `ffmpeg`. **Crea vídeos de verdad**, y cada
//! `<video>` con fuente abre una conexión al bus de sesión hasta que el
//! recolector lo destruye (ADR-042): por eso son pocos, por eso cada variante
//! corre en su proceso y por eso lleva el freno del bus. No subir los números
//! a cientos.

use gtk::prelude::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use webkit2gtk::{CacheModel, SettingsExt, WebContext, WebContextExt, WebView, WebViewExt};

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

/// Los scripts de las vistas de cuenta, en el orden en que los pone
/// `shell::create_account_view`. Salen de la propia fuente.
fn scripts_de_wrusp() -> Vec<String> {
    let barra = literal(include_str!("../src/rail.rs"), "pub fn runtime_script(")
        .replace("{{", "\u{1}")
        .replace("}}", "\u{2}")
        .replace('\u{1}', "{")
        .replace('\u{2}', "}")
        .replace("{own}", "\"cuenta\"");
    // `WRUSP_BANCO_BROWSER=ruta/al/browser.rs` de otra versión: su script de
    // vídeo en lugar del actual, para comparar lo que cambia.
    let video = match std::env::var("WRUSP_BANCO_BROWSER") {
        Ok(ruta) => literal(
            &std::fs::read_to_string(ruta).expect("no se puede leer WRUSP_BANCO_BROWSER"),
            "pub fn fix_large_mp4_blobs_script() -> String {",
        ),
        Err(_) => browser::fix_large_mp4_blobs_script(),
    };
    vec![
        browser::disguise_script(),
        browser::hide_webcodecs_script(),
        video,
        browser::quiet_idle_audio_script(),
        browser::hide_native_app_promo_script(),
        barra,
        literal(
            include_str!("../src/filedrop.rs"),
            "pub const SCRIPT: &str =",
        ),
        literal(
            include_str!("../src/clipboard.rs"),
            "pub const SCRIPT: &str =",
        ),
    ]
}

// ── Vídeos de prueba ─────────────────────────────────────────────────────────

/// Un MP4 generado con ffmpeg. Los GIF de WhatsApp son MP4 H.264 pequeños y
/// sin audio; los vídeos, H.264 con AAC.
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

fn videos(cuantos_gif: usize) -> HashMap<String, Vec<u8>> {
    let dir = std::env::temp_dir().join("wrusp-banco-chat-videos");
    std::fs::create_dir_all(&dir).expect("sin carpeta temporal");
    let mut mapa = HashMap::new();
    for i in 0..cuantos_gif {
        // Cada GIF distinto (otro patrón y otro tono), como en un chat real.
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
    let ruta = dir.join("video.mp4");
    let datos = generar(
        &ruta,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=30:duration=20",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=20",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "4500k",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-movflags",
            "+faststart",
            "-shortest",
        ],
    );
    mapa.insert("/video.mp4".into(), datos);
    mapa
}

// ── Servidor local ───────────────────────────────────────────────────────────

/// Sirve la página y los vídeos en `127.0.0.1`, un hilo por conexión. Devuelve
/// el puerto.
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
                // El resto de cabeceras no importa, pero hay que leerlas.
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

/// Imita el chat. Las fases avisan por el título (`FASE:nombre|latido`) y Rust
/// mide en ese instante; la página no espera a nadie.
const PAGINA: &str = r#"<!doctype html><meta charset="utf-8">
<style>
  body { margin: 0; font: 14px sans-serif; }
  #chat { height: 100vh; overflow-y: auto; }
  .fila { padding: 6px 12px; }
  .burbuja { display: inline-block; background: #d9fdd3; border-radius: 8px; padding: 4px;
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, .13); position: relative; }
  .avatar { width: 40px; height: 40px; border-radius: 50%; vertical-align: top; margin-right: 8px;
    background: conic-gradient(#25d366, #128c7e, #34b7f1, #25d366); display: inline-block; }
  .burbuja svg { width: 16px; height: 11px; margin-left: 4px; }
  video { width: 240px; height: 135px; display: block; background: #000; }
  .miniatura { width: 240px; height: 135px; background: linear-gradient(#335, #557); }
</style>
<script>
  // Lo que hace Rust en la app: las órdenes de la barra no salen de aquí.
  const fetchNativo = window.fetch.bind(window);
  window.fetch = (url, opciones) => String(url).indexOf('wrusp:') === 0
    ? Promise.resolve(new Response(null, { status: 204 }))
    : fetchNativo(url, opciones);
</script>
SCRIPTS
<div id="chat"></div>
<script>
  const GIF = CUANTOS_GIF;
  const IMAGENES = CUANTAS_IMAGENES;
  const espera = (ms) => new Promise((l) => setTimeout(l, ms));

  // Latido: el peor retraso de cada fase es la congelación más larga.
  let ultimo = performance.now(), peor = 0;
  setInterval(() => {
    const ahora = performance.now();
    peor = Math.max(peor, ahora - ultimo - 16);
    ultimo = ahora;
  }, 16);
  const fase = (nombre, extra) => {
    document.title = 'FASE:' + nombre + '|' + Math.round(peor) + '|' + (extra || '');
    peor = 0;
  };

  const chat = document.getElementById('chat');
  let serie = 0;
  // Un GIF de WhatsApp: fuente `blob:` por atributo, `autoplay`, `loop`, sin sonido.
  function filaGif(blob) {
    const f = document.createElement('div');
    f.className = 'fila';
    f.innerHTML = '<div class="burbuja"><video loop muted playsinline></video><span>GIF ' + (serie++) + '</span></div>';
    const v = f.querySelector('video');
    v.muted = true;
    v.setAttribute('autoplay', '');
    // WhatsApp guarda la URL del blob en su modelo y la revoca con ella; el
    // atributo, tras la entrega de Wrusp, ya es la `data:`.
    f.dataset.url = URL.createObjectURL(new Blob([blob], { type: 'video/mp4' }));
    v.setAttribute('src', f.dataset.url);
    return f;
  }
  // El doble visto de WhatsApp es un trazado SVG: con MSAA, cada icono así
  // pasa por las superficies multimuestra del motor.
  const VISTO = '<svg viewBox="0 0 16 11"><path fill="rgb(83,189,235)" d="M11.07.65 10.4.12a.46.46 0 0 0-.66.1L4.88 6.52 2.2 4.1a.47.47 0 0 0-.66.02l-.58.61a.47.47 0 0 0 .02.66l3.65 3.3c.2.18.5.16.67-.05l5.8-7.33a.47.47 0 0 0-.03-.66Zm3.9 0-.67-.53a.46.46 0 0 0-.66.1L8.78 6.52l-.63-.57-.83 1.04 1.28 1.16c.2.18.5.16.67-.05l5.8-7.33a.47.47 0 0 0-.1-.62Z"/></svg>';
  function filaTexto() {
    const f = document.createElement('div');
    f.className = 'fila';
    f.innerHTML = '<span class="avatar"></span><div class="burbuja"><span>Mensaje ' + (serie++)
      + ' con algo de texto y un enlace <a href="https://ejemplo.org/">https://ejemplo.org/' + serie + '</a></span>' + VISTO + '</div>';
    return f;
  }
  const quitarFila = (f) => {
    if (f.dataset.url) URL.revokeObjectURL(f.dataset.url);
    f.remove();
  };

  // La sonda de WhatsApp para medir un vídeo (`WAWebMediaLoad`): un <video>
  // con `load()`, `currentTime = 1` y espera de metadatos sin reproducir. Con
  // Safari, que es lo que ve WhatsApp en Wrusp, lo cuelga del documento
  // mientras mide (ADR-044) y lo quita al terminar.
  function sonda(url) {
    return new Promise((listo) => {
      const v = document.createElement('video');
      v.crossOrigin = 'anonymous';
      v.style.cssText = 'position:absolute;width:1px;height:1px;opacity:0';
      v.src = url;
      document.body.appendChild(v);
      const fin = (r) => { clearTimeout(t); v.remove(); listo(r); };
      const t = setTimeout(() => fin('plazo'), 20000);
      v.addEventListener('loadedmetadata', () => { v.currentTime = 1; });
      v.addEventListener('seeked', () => fin(v.videoWidth + 'x' + v.videoHeight), { once: true });
      v.addEventListener('error', () => fin('error ' + (v.error && v.error.code)), { once: true });
      v.load();
    });
  }

  (async () => {
    const blobs = await Promise.all(
      Array.from({ length: GIF }, (_, i) => fetch('/gif' + i + '.mp4').then((r) => r.blob())));
    const grande = await fetch('/video.mp4').then((r) => r.blob());
    for (let i = 0; i < 40; i++) chat.appendChild(filaTexto());
    await espera(1500);
    fase('base');

    // Abrir el chat: todos los GIF entran en la misma pasada, como al montar
    // la conversación.
    for (let i = 0; i < GIF; i++) {
      chat.appendChild(filaGif(blobs[i]));
      chat.appendChild(filaTexto());
    }
    chat.scrollTop = chat.scrollHeight;
    await espera(6000);
    const sonando = Array.from(document.querySelectorAll('video')).filter((v) => !v.paused && v.readyState >= 2).length;
    fase('abrir', 'GIF en marcha ' + sonando + '/' + GIF);

    // Un vídeo que llega: la sonda lo mide, y después alguien lo pulsa.
    const urlGrande = URL.createObjectURL(new Blob([grande], { type: 'video/mp4' }));
    const medida = await sonda(urlGrande);
    const f = document.createElement('div');
    f.className = 'fila';
    f.innerHTML = '<div class="burbuja"><video playsinline></video></div>';
    const v = f.querySelector('video');
    v.setAttribute('src', urlGrande);
    chat.appendChild(f);
    chat.scrollTop = chat.scrollHeight;
    let reproduce = 'no';
    try { await v.play(); reproduce = 'sí'; } catch (e) { reproduce = 'falla: ' + e.name; }
    // WhatsApp revela su reproductor con el primer `requestVideoFrameCallback`:
    // si deja de llegar, el vídeo se queda en el póster con el tiempo corriendo.
    let cuadros = 0;
    if (v.requestVideoFrameCallback) {
      const contar = () => { cuadros++; v.requestVideoFrameCallback(contar); };
      v.requestVideoFrameCallback(contar);
    }
    await espera(5000);
    fase('video', 'sonda ' + medida + ', reproduce ' + reproduce + ' t=' + v.currentTime.toFixed(1) + ', rVFC ' + cuadros);
    v.pause();

    // Desplazarse: salen filas por arriba y entran GIF nuevos por abajo.
    for (let tanda = 0; tanda < 3; tanda++) {
      const gifs = chat.querySelectorAll('.fila video[autoplay]');
      for (let i = 0; i < Math.min(4, gifs.length); i++) quitarFila(gifs[i].closest('.fila'));
      for (let i = 0; i < 4; i++) {
        chat.appendChild(filaGif(blobs[(tanda * 4 + i) % GIF]));
        chat.appendChild(filaTexto());
      }
      chat.scrollTop = chat.scrollHeight;
      await espera(1500);
    }
    fase('desplazar', 'vídeos en el documento ' + document.querySelectorAll('video').length);

    // Reposo con los GIF en bucle: el coste en régimen.
    await espera(8000);
    fase('reposo');

    // Un chat con muchas fotos: imágenes distintas que entran por abajo y
    // salen por arriba, como al desplazarse. Cada una se decodifica y, con el
    // pintado por GPU, acaba como textura.
    if (IMAGENES > 0) {
      const lienzo = document.createElement('canvas');
      lienzo.width = 640; lienzo.height = 480;
      const g = lienzo.getContext('2d');
      const urls = [];
      for (let i = 0; i < IMAGENES; i++) {
        g.fillStyle = 'hsl(' + (i * 47 % 360) + ', 60%, 50%)';
        g.fillRect(0, 0, 640, 480);
        for (let k = 0; k < 40; k++) {
          g.fillStyle = 'hsl(' + ((i * 13 + k * 29) % 360) + ', 70%, ' + (30 + k % 40) + '%)';
          g.beginPath();
          g.arc((i * 37 + k * 53) % 640, (i * 17 + k * 71) % 480, 20 + (k * 7) % 60, 0, Math.PI * 2);
          g.fill();
        }
        g.fillStyle = '#fff'; g.font = '48px sans-serif'; g.fillText('foto ' + i, 40, 80);
        const blob = await new Promise((l) => lienzo.toBlob(l, 'image/jpeg', 0.8));
        urls.push(URL.createObjectURL(blob));
      }
      for (let i = 0; i < IMAGENES; i++) {
        const f = document.createElement('div');
        f.className = 'fila foto';
        f.innerHTML = '<div class="burbuja"><img width="320" height="240" style="border-radius:6px;display:block"></div>';
        f.querySelector('img').src = urls[i];
        chat.appendChild(f);
        const fotos = chat.querySelectorAll('.foto');
        if (fotos.length > 12) fotos[0].remove();
        chat.scrollTop = chat.scrollHeight;
        if (i % 6 === 5) await espera(120);
      }
      await espera(3000);
      fase('fotos', IMAGENES + ' imágenes');
    }

    // Salir del chat: WhatsApp desmonta la conversación y revoca sus URL.
    for (const fila of Array.from(chat.querySelectorAll('.fila'))) quitarFila(fila);
    URL.revokeObjectURL(urlGrande);
    await espera(8000);
    fase('salir', 'desmontados por Wrusp ' + (window.__wruspDesmontados || 0));
    await espera(300);
    document.title = 'FIN';
  })();
</script>"#;

// ── Medidas del proceso ──────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct Medida {
    rss: u64,
    anon: u64,
    heap: u64,
    gtt: u64,
    vram: u64,
    cpu_web: f64,
    gpu_ns: u64,
    cpu_ui: f64,
    hilos: usize,
    pipelines: usize,
}

fn hijos_web() -> Vec<u32> {
    let yo = std::process::id().to_string();
    let mut pids = Vec::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(estado) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        let web = estado
            .lines()
            .next()
            .is_some_and(|l| l.contains("WebKitWebProces"));
        let hijo = estado
            .lines()
            .find(|l| l.starts_with("PPid:"))
            .is_some_and(|l| l.split_whitespace().nth(1) == Some(yo.as_str()));
        if web && hijo {
            pids.push(pid);
        }
    }
    pids
}

fn kib(texto: &str, clave: &str) -> u64 {
    texto
        .lines()
        .find(|l| l.starts_with(clave))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn cpu_de(pid: &str) -> f64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    // Tras el nombre entre paréntesis: utime y stime son los campos 14 y 15.
    let resto = stat.rsplit_once(')').map_or("", |(_, r)| r);
    let campos: Vec<&str> = resto.split_whitespace().collect();
    let ticks: f64 = campos.get(11).and_then(|v| v.parse().ok()).unwrap_or(0.0)
        + campos.get(12).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    ticks / 100.0
}

/// GTT, VRAM (KiB) y tiempo del motor gráfico (ns) de un proceso, sumando sus
/// clientes DRM distintos.
fn gpu_de(pid: u32) -> (u64, u64, u64) {
    let mut vistos = std::collections::HashSet::new();
    let (mut gtt, mut vram, mut ns) = (0, 0, 0);
    for e in std::fs::read_dir(format!("/proc/{pid}/fdinfo"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let Ok(info) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let Some(cliente) = info
            .lines()
            .find(|l| l.starts_with("drm-client-id:"))
            .map(str::to_string)
        else {
            continue;
        };
        if !vistos.insert(cliente) {
            continue;
        }
        gtt += kib(&info, "drm-resident-gtt:");
        vram += kib(&info, "drm-resident-vram:");
        ns += kib(&info, "drm-engine-gfx:");
    }
    (gtt, vram, ns)
}

fn medir() -> Medida {
    let mut m = Medida {
        cpu_ui: cpu_de("self"),
        ..Default::default()
    };
    for pid in hijos_web() {
        let estado = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        let rss = kib(&estado, "VmRSS:");
        if rss < m.rss {
            continue; // el proceso web de la página es el grande
        }
        m.rss = rss;
        m.anon = kib(&estado, "RssAnon:");
        m.hilos = kib(&estado, "Threads:") as usize;
        let smaps = std::fs::read_to_string(format!("/proc/{pid}/smaps")).unwrap_or_default();
        let mut en_heap = false;
        m.heap = 0;
        for linea in smaps.lines() {
            if linea.contains('-') && linea.split_whitespace().count() >= 5 {
                en_heap = linea.ends_with("[heap]");
            } else if en_heap && linea.starts_with("Rss:") {
                m.heap += kib(linea, "Rss:");
            }
        }
        m.cpu_web = cpu_de(&pid.to_string());
        (m.gtt, m.vram, m.gpu_ns) = gpu_de(pid);
        m.pipelines = std::fs::read_dir(format!("/proc/{pid}/task"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|t| {
                std::fs::read_to_string(t.path().join("comm"))
                    .is_ok_and(|c| c.starts_with("typefind"))
            })
            .count();
    }
    m
}

/// Descriptores del `dbus-broker` de la sesión de este usuario.
fn fds_bus() -> Option<usize> {
    let uid = kib(&std::fs::read_to_string("/proc/self/status").ok()?, "Uid:");
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(st) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        if st.lines().next().unwrap_or("").ends_with("dbus-broker") && kib(&st, "Uid:") == uid {
            return std::fs::read_dir(e.path().join("fd"))
                .ok()
                .map(|d| d.count());
        }
    }
    None
}

const FRENO_BUS: usize = 150;

// ── Variantes ────────────────────────────────────────────────────────────────

/// Una variante: nombre, variables de entorno y modelo de caché del motor.
type Variante = (
    &'static str,
    Vec<(&'static str, String)>,
    Option<CacheModel>,
);

/// Nombre y variables de entorno de cada variante. `actual` es Wrusp tal cual.
/// El modelo de caché del motor se midió en la primera tanda y no movía nada;
/// se deja el parámetro por si hace falta volver a mirarlo.
fn variantes() -> Vec<Variante> {
    let anterior = std::env::var("WRUSP_BANCO_BROWSER_ANTERIOR").unwrap_or_default();
    let malloc = || {
        vec![
            ("MALLOC_ARENA_MAX", "2".to_string()),
            ("MALLOC_MMAP_THRESHOLD_", "131072".to_string()),
            ("MALLOC_TRIM_THRESHOLD_", "131072".to_string()),
        ]
    };
    // El MSAA (8 muestras en x86_64) se midió aquí y en la app con la
    // pantalla del QR: no cambia la memoria gráfica. Quien la llena es la
    // caché de texturas del pintado por GPU (fase de fotos).
    let msaa = |n: &str| ("WEBKIT_SKIA_MSAA_SAMPLE_COUNT", n.to_string());
    let cpu = || ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1".to_string());
    let mut propuesta = malloc();
    propuesta.push(cpu());
    vec![
        ("actual", vec![], None),
        ("anterior", vec![("WRUSP_BANCO_BROWSER", anterior)], None),
        ("malloc", malloc(), None),
        ("msaa0", vec![msaa("0")], None),
        ("msaa4", vec![msaa("4")], None),
        ("cpu", vec![cpu()], None),
        ("propuesta", propuesta, None),
        (
            "cpu+malloc1m",
            vec![
                cpu(),
                ("MALLOC_ARENA_MAX", "2".to_string()),
                ("MALLOC_MMAP_THRESHOLD_", "1048576".to_string()),
                ("MALLOC_TRIM_THRESHOLD_", "1048576".to_string()),
            ],
            None,
        ),
        ("cache", vec![], Some(CacheModel::DocumentViewer)),
    ]
}

/// Corre una variante en este proceso e imprime una línea por fase.
fn correr_variante(nombre: &str, cache: Option<CacheModel>) {
    gtk::init().expect("no hay sesión gráfica");
    let gif: usize = std::env::var("WRUSP_BANCO_GIF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let scripts: String = scripts_de_wrusp()
        .into_iter()
        .map(|s| {
            format!("<script>try {{\n{s}\n}} catch (e) {{ console.error('wrusp', e); }}</script>\n")
        })
        .collect();
    let imagenes: usize = std::env::var("WRUSP_BANCO_IMAGENES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let pagina = PAGINA
        .replace("SCRIPTS", &scripts)
        .replace("CUANTOS_GIF", &gif.to_string())
        .replace("CUANTAS_IMAGENES", &imagenes.to_string());
    let puerto = servir(videos(gif), pagina);

    let contexto = WebContext::new_ephemeral();
    if let Some(modelo) = cache {
        contexto.set_cache_model(modelo);
    }
    // Autoplay permitido: en la app el vídeo lo pulsa alguien, y un `play()`
    // sin gesto fallaría aquí con NotAllowedError.
    let politicas = webkit2gtk::WebsitePolicies::builder()
        .autoplay(webkit2gtk::AutoplayPolicy::Allow)
        .build();
    let vista = WebView::builder()
        .web_context(&contexto)
        .website_policies(&politicas)
        .build();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        // Los de `permissions::configure`, para medir el mismo motor.
        ajustes.set_enable_media_stream(true);
        ajustes.set_enable_webrtc(true);
        ajustes.set_enable_mediasource(true);
        ajustes.set_enable_media_capabilities(true);
        ajustes.set_enable_encrypted_media(true);
        ajustes.set_enable_smooth_scrolling(true);
        ajustes.set_enable_page_cache(true);
        ajustes.set_javascript_can_access_clipboard(true);
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
    let inicio = std::rc::Rc::new(std::cell::RefCell::new(medir()));
    let terminado = std::rc::Rc::new(std::cell::Cell::new(false));
    let fin = terminado.clone();
    let nombre = nombre.to_string();
    vista.connect_title_notify(move |v| {
        let Some(titulo) = v.title() else { return };
        if titulo == "FIN" {
            fin.set(true);
            gtk::main_quit();
            return;
        }
        let Some(resto) = titulo.strip_prefix("FASE:") else {
            return;
        };
        let mut partes = resto.splitn(3, '|');
        let fase = partes.next().unwrap_or("");
        let latido = partes.next().unwrap_or("");
        let nota = partes.next().unwrap_or("");
        let m = medir();
        let antes = inicio.replace(m.clone());
        println!(
            "{nombre:<13} {fase:<10} RSS {:>5} MiB · anón {:>5} · heap {:>4} · GTT {:>5} · VRAM {:>4} · CPU web {:>5.2} s · CPU ui {:>5.2} s · GPU {:>6.0} ms · hilos {:>3} · pipelines {:>2} · peor latido {:>5} ms{}",
            m.rss / 1024,
            m.anon / 1024,
            m.heap / 1024,
            m.gtt / 1024,
            m.vram / 1024,
            m.cpu_web - antes.cpu_web,
            m.cpu_ui - antes.cpu_ui,
            m.gpu_ns.saturating_sub(antes.gpu_ns) as f64 / 1e6,
            m.hilos,
            m.pipelines,
            latido,
            if nota.is_empty() { String::new() } else { format!(" · {nota}") },
        );
    });
    vista.load_uri(&format!("http://127.0.0.1:{puerto}/"));

    let plazo = gtk::glib::timeout_add_seconds_local(150, || {
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
    // Las condiciones de Wrusp (`main.rs`): sin ellas se mediría otro motor.
    for (variable, valor) in [
        (
            "GST_PLUGIN_FEATURE_RANK",
            "vah264dec:0,vah264lpdec:0,vaapih264dec:0,vaapidecodebin:0",
        ),
        ("WEBKIT_GST_DISABLE_GL_SINK", "1"),
        ("GST_DEBUG", "0"),
    ] {
        if std::env::var_os(variable).is_none() {
            std::env::set_var(variable, valor);
        }
    }

    // Dentro de un hijo: correr la variante que diga el padre.
    if let Ok(nombre) = std::env::var("WRUSP_BANCO_VARIANTE_HIJA") {
        let cache = variantes()
            .into_iter()
            .find(|(n, _, _)| *n == nombre)
            .and_then(|(_, _, c)| c);
        correr_variante(&nombre, cache);
        return;
    }

    let elegidas: Option<Vec<String>> = std::env::var("WRUSP_BANCO_VARIANTES")
        .ok()
        .map(|v| v.split(',').map(str::to_string).collect());
    let repeticiones: usize = std::env::var("WRUSP_BANCO_REPETIR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let yo = std::env::current_exe().expect("sin ejecutable");
    for _ in 0..repeticiones {
        for (nombre, entorno, _) in variantes() {
            let pedida = elegidas.as_ref().map(|e| e.iter().any(|x| x == nombre));
            if pedida == Some(false) || (pedida.is_none() && nombre == "cache") {
                continue;
            }
            if nombre == "anterior" && entorno.iter().any(|(_, v)| v.is_empty()) {
                println!(
                    "anterior: sin WRUSP_BANCO_BROWSER_ANTERIOR=ruta/al/browser.rs, se salta\n"
                );
                continue;
            }
            let estado = std::process::Command::new(&yo)
                .env("WRUSP_BANCO_VARIANTE_HIJA", nombre)
                .envs(entorno)
                .status()
                .expect("no se pudo lanzar la variante");
            if estado.code() == Some(2) {
                eprintln!("parado por el freno del bus");
                std::process::exit(2);
            }
            println!();
        }
    }
}
