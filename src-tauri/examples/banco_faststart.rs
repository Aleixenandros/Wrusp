//! Banco de pruebas del remux «faststart» de los vídeos de WhatsApp.
//!
//! Ese script reordena los bytes de un MP4 —mueve el índice `moov` delante de
//! los datos y corrige los desplazamientos de trozo— para que WebKitGTK pueda
//! reproducirlo. Equivocarse en un solo desplazamiento no da un error visible:
//! da un vídeo que se ve mal o que no arranca, que es justo el síntoma que se
//! quería quitar. Por eso se comprueba byte a byte que cada trozo del fichero
//! reordenado apunta exactamente a los mismos datos que en el original.
//!
//! Necesita sesión gráfica:
//!
//! ```sh
//! cargo run --example banco_faststart
//! ```
//!
//! Si hay `ffmpeg` en el PATH, se añade una prueba de reproducción real: un
//! H.264/AAC con el índice al final, servido como `blob:` igual que hace
//! WhatsApp, tiene que llegar a avanzar en el `<video>`.

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

/// El script tal cual lo inyecta Wrusp, sacado de su propia fuente para que el
/// banco no pueda quedarse probando una copia vieja.
fn script() -> String {
    // Para ver que el banco sirve de algo: sin el script, la maqueta de
    // reproducción tiene que cantar fallos. `WRUSP_BANCO_SIN_SCRIPT=1`.
    if std::env::var_os("WRUSP_BANCO_SIN_SCRIPT").is_some() {
        return String::new();
    }
    let fuente = include_str!("../src/browser.rs");
    let inicio = fuente
        .find("pub fn fix_large_mp4_blobs_script() -> String {")
        .expect("la función cambió de nombre");
    let cuerpo = &fuente[inicio..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra].to_string()
}

/// Base64 sin dependencias: el banco solo necesita meter unos bytes en la
/// página.
fn base64(datos: &[u8]) -> String {
    const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut salida = String::with_capacity(datos.len().div_ceil(3) * 4);
    for trozo in datos.chunks(3) {
        let b = [
            trozo[0],
            *trozo.get(1).unwrap_or(&0),
            *trozo.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        salida.push(ALFABETO[(n >> 18 & 63) as usize] as char);
        salida.push(ALFABETO[(n >> 12 & 63) as usize] as char);
        salida.push(if trozo.len() > 1 {
            ALFABETO[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        salida.push(if trozo.len() > 2 {
            ALFABETO[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    salida
}

/// Un H.264/AAC de verdad con el índice al final, si hay `ffmpeg` a mano.
/// Es el caso que rompía: `-movflags -faststart` deja `moov` detrás de `mdat`,
/// y pasa de los 2 MiB del búfer con el que WebKitGTK sirve los blobs, que es
/// la condición exacta del fallo.
fn video_real() -> Option<Vec<u8>> {
    // Con `WRUSP_BANCO_SEGUNDOS` se alarga el vídeo para tantear a partir de
    // qué tamaño empieza a romperse el blob en el motor.
    let segundos: u32 = std::env::var("WRUSP_BANCO_SEGUNDOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let bitrate = std::env::var("WRUSP_BANCO_BITRATE").unwrap_or_else(|_| "3M".into());
    let dir = std::env::temp_dir().join("wrusp-banco-faststart");
    std::fs::create_dir_all(&dir).ok()?;
    let destino = dir.join(format!("indice-al-final-{segundos}s-{bitrate}.mp4"));
    let salida = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!(
            "testsrc2=size=1280x720:rate=30:duration={segundos}"
        ))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={segundos}"))
        .args([
            "-c:v", "libx264", "-b:v", &bitrate, "-pix_fmt", "yuv420p", "-c:a", "aac",
        ])
        .args(["-movflags", "-faststart"])
        .arg(&destino)
        .status()
        .ok()?;
    if !salida.success() {
        return None;
    }
    std::fs::read(&destino).ok()
}

const INFORME: &str = r#"<script>
  function informe(pruebas, nota) {
    const linea = pruebas.map(([q, ok]) => (ok ? 'OK' : 'FALLO') + ' ' + q).join(' | ');
    document.title = 'WRUSP:' + (pruebas.every(([, ok]) => ok) ? 'TODO BIEN' : 'HAY FALLOS')
      + ' :: ' + linea + (nota ? ' | ' + nota : '');
  }
</script>"#;

/// Maqueta 1 — el algoritmo, con MP4 armados a mano para poder comprobar cada
/// desplazamiento contra los bytes a los que debía apuntar.
const ALGORITMO: &str = r#"
<video id="v"></video>
<script>SCRIPT</script>
<script>
  // Constructor de MP4 de juguete: cajas con el tamaño y el tipo de siempre.
  const texto = (s) => Uint8Array.from(s, (c) => c.charCodeAt(0));
  function caja(tipo, cuerpo) {
    const b = new Uint8Array(8 + cuerpo.length);
    new DataView(b.buffer).setUint32(0, b.length);
    b.set(texto(tipo), 4);
    b.set(cuerpo, 8);
    return b;
  }
  const unir = (trozos) => {
    const total = trozos.reduce((n, t) => n + t.length, 0);
    const b = new Uint8Array(total);
    let p = 0;
    for (const t of trozos) { b.set(t, p); p += t.length; }
    return b;
  };
  const anidar = (tipos, hoja) => tipos.reduceRight((dentro, t) => caja(t, dentro), hoja);

  function tablaDesplazamientos(tipo, offsets) {
    const ancho = tipo === 'stco' ? 4 : 8;
    const c = new Uint8Array(8 + offsets.length * ancho);
    const v = new DataView(c.buffer);
    v.setUint32(0, 0);                 // versión y banderas
    v.setUint32(4, offsets.length);
    offsets.forEach((o, i) => {
      if (ancho === 4) v.setUint32(8 + i * 4, o);
      else v.setBigUint64(8 + i * 8, BigInt(o));
    });
    return caja(tipo, c);
  }

  // [ftyp][mdat][moov] — el orden que rompe en WebKitGTK.
  function conIndiceAlFinal(tipoTabla) {
    const ftyp = caja('ftyp', texto('isomiso2avc1mp41'));
    const datos = new Uint8Array(3000).map((_, i) => (i * 31) & 0xff);
    const mdat = caja('mdat', datos);
    const chunks = [ftyp.length + 8, ftyp.length + 8 + 1000, ftyp.length + 8 + 2000];
    const moov = anidar(['moov', 'trak', 'mdia', 'minf', 'stbl'],
                        tablaDesplazamientos(tipoTabla, chunks));
    return { bytes: unir([ftyp, mdat, moov]), chunks, ftyp, moov, mdat };
  }

  // Recorre las cajas de nivel superior del resultado.
  function tipos(u8) {
    const v = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    const salida = [];
    let p = 0;
    while (p + 8 <= u8.length) {
      let tam = v.getUint32(p);
      salida.push(String.fromCharCode(u8[p + 4], u8[p + 5], u8[p + 6], u8[p + 7]));
      if (tam === 1) tam = Number(v.getBigUint64(p + 8));
      else if (tam === 0) tam = u8.length - p;
      if (tam < 8) return salida;
      p += tam;
    }
    return salida;
  }

  const iguales = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);

  // El script no exporta nada, así que se ejercita por donde entra de verdad:
  // un blob de vídeo que se reproduce. `URL.createObjectURL` está envuelto, y
  // al pulsar play la fuente del <video> tiene que quedar reordenada.
  async function reordenadoDe(bytes) {
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    v.src = url;
    try { await v.play(); } catch (e) { /* sin códec: da igual, interesa la URL */ }
    const definitiva = v.src;
    if (definitiva === url) return null;   // no lo tocó
    const resp = await fetch(definitiva);
    return new Uint8Array(await resp.arrayBuffer());
  }

  (async () => {
    const pruebas = [];
    for (const tipoTabla of ['stco', 'co64']) {
      const caso = conIndiceAlFinal(tipoTabla);
      const nuevo = await reordenadoDe(caso.bytes);
      pruebas.push([tipoTabla + ': se reordena', !!nuevo]);
      if (!nuevo) continue;
      pruebas.push([tipoTabla + ': mismo tamaño', nuevo.length === caso.bytes.length]);
      pruebas.push([tipoTabla + ': orden ftyp/moov/mdat',
                    iguales(tipos(nuevo), ['ftyp', 'moov', 'mdat'])]);
      // Lo que de verdad importa: cada desplazamiento nuevo tiene que dar con
      // los mismos bytes que daba el viejo en el fichero original.
      const marca = texto(tipoTabla);
      let pos = -1;
      for (let i = 0; i + 4 <= nuevo.length; i++)
        if (nuevo[i] === marca[0] && nuevo[i+1] === marca[1]
            && nuevo[i+2] === marca[2] && nuevo[i+3] === marca[3]) { pos = i; break; }
      const base = pos + 4 + 8;
      const v = new DataView(nuevo.buffer, nuevo.byteOffset, nuevo.byteLength);
      const ancho = tipoTabla === 'stco' ? 4 : 8;
      let bien = pos > 0;
      let movidos = 0;
      caso.chunks.forEach((viejo, i) => {
        const p = base + i * ancho;
        const nuevoOff = ancho === 4 ? v.getUint32(p) : Number(v.getBigUint64(p));
        if (nuevoOff !== viejo) movidos++;
        for (let k = 0; k < 64; k++)
          if (nuevo[nuevoOff + k] !== caso.bytes[viejo + k]) bien = false;
      });
      pruebas.push([tipoTabla + ': los desplazamientos se mueven',
                    movidos === caso.chunks.length]);
      pruebas.push([tipoTabla + ': cada trozo apunta a los mismos bytes', bien]);
    }

    // Lo que NO debe tocarse.
    const yaBien = (() => {
      const c = conIndiceAlFinal('stco');
      return unir([c.ftyp, c.moov, c.mdat]);
    })();
    pruebas.push(['con el índice ya delante no se toca', (await reordenadoDe(yaBien)) === null]);
    pruebas.push(['lo que no es un MP4 no se toca',
                  (await reordenadoDe(texto('esto no es un mp4 ni de lejos'))) === null]);
    const truncado = conIndiceAlFinal('stco').bytes.slice(0, 1500);
    pruebas.push(['un fichero a medias no se toca', (await reordenadoDe(truncado)) === null]);

    informe(pruebas, '');
  })();
</script>
"#;

/// Maqueta 2 — reproducción de verdad: H.264/AAC con el índice al final,
/// servido como `blob:` igual que WhatsApp. Sin el remux, WebKitGTK muere con
/// «atom has bogus size».
const REPRODUCCION: &str = r#"
<video id="v" muted playsinline></video>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    // Interesa saber si el fallo llega con la fuente original o con la ya
    // reordenada: no es el mismo problema ni la misma solución.
    const diario = [];
    for (const evento of ['error', 'loadedmetadata', 'canplay', 'playing', 'stalled'])
      v.addEventListener(evento, () => {
        diario.push(evento
          + (evento === 'error' ? '(' + ((v.error && v.error.code)
              || (window.__wruspUltimoFallo && window.__wruspUltimoFallo.codigo) || '?') + ')' : '')
          + (v.src === url ? '@original' : '@reordenado'));
      });
    v.src = url;
    try { await v.play(); } catch (e) { diario.push('play:' + e.name); }
    await new Promise((listo) => setTimeout(listo, 5000));
    const fallo = diario.filter((d) => d.startsWith('error')).join(',');
    informe([
      ['la fuente queda reordenada', v.src !== url],
      ['el motor no da error de medio', !fallo],
      ['el vídeo avanza', v.currentTime > 0.2],
      ['tiene duración', Number.isFinite(v.duration) && v.duration > 5],
    ], 'currentTime=' + v.currentTime.toFixed(2) + ' duración=' + v.duration
       + ' · ' + diario.join(' '));
  })();
</script>
"#;

/// Maqueta 3 — un chat con muchos adjuntos, que es donde aparece el fallo de
/// verdad: en el registro real hay `blob-media-player-40`, o sea cuarenta
/// reproductores vivos. WebKitGTK levanta un pipeline de GStreamer por cada
/// medio del documento aunque nadie lo toque, y ahí es donde `qtdemux` empieza
/// a recibir datos de la posición equivocada («atom has bogus size»).
const MUCHOS: &str = r#"
<div id="chat"></div>
<script>SCRIPT</script>
<script>
  const CUANTOS = 24;
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);

    const chat = document.getElementById('chat');
    const videos = [];
    for (let i = 0; i < CUANTOS; i++) {
      // Un blob distinto por adjunto, como haría WhatsApp al descifrar cada uno.
      const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
      const v = document.createElement('video');
      v.muted = true;
      v.playsInline = true;
      v.style.width = '160px';
      v.src = url;
      chat.appendChild(v);
      videos.push(v);
    }

    // Que el motor tenga tiempo de arrancar lo que vaya a arrancar.
    await new Promise((listo) => setTimeout(listo, 4000));
    const precargando = videos.filter((v) => v.preload !== 'none').length;
    const conRed = videos.filter((v) => v.networkState === HTMLMediaElement.NETWORK_LOADING).length;

    // Y ahora el usuario pulsa reproducir en uno del medio, como haría.
    const elegido = videos[CUANTOS >> 1];
    let fallo = '';
    elegido.addEventListener('error', () => {
      fallo = 'code=' + ((elegido.error && elegido.error.code)
        || (window.__wruspUltimoFallo && window.__wruspUltimoFallo.codigo) || '?');
    });
    try { await elegido.play(); } catch (e) { fallo = fallo || ('play:' + e.name); }
    await new Promise((listo) => setTimeout(listo, 5000));

    const rotos = videos.filter((v) => v.error).length;
    informe([
      ['los adjuntos inactivos no precargan', precargando === 0],
      ['ninguno abre pipeline por su cuenta', conRed === 0],
      ['el que se pulsa se reproduce', !fallo && elegido.currentTime > 0.2],
      ['ningún otro adjunto queda roto', rotos === 0],
    ], CUANTOS + ' adjuntos · precargando=' + precargando + ' cargando=' + conRed
       + ' rotos=' + rotos + ' t=' + elegido.currentTime.toFixed(2) + ' ' + fallo);
  })();
</script>
"#;

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

/// Las mismas condiciones en las que corre Wrusp: sin ellas el banco mediría
/// otro motor. La decodificación por hardware revienta con estos vídeos
/// (ADR-0.3.6) y el sink GL entrega buffers que no se pueden mapear
/// (ADR-0.3.7); ambas cosas dan «Decode error» y no tienen nada que ver con el
/// remux, así que confundirlas saldría caro.
fn como_wrusp() {
    for (variable, valor) in [
        (
            "GST_PLUGIN_FEATURE_RANK",
            "vah264dec:0,vah264lpdec:0,vaapih264dec:0,vaapidecodebin:0",
        ),
        ("WEBKIT_GST_DISABLE_GL_SINK", "1"),
    ] {
        if std::env::var_os(variable).is_none() {
            std::env::set_var(variable, valor);
        }
    }
}

fn main() {
    como_wrusp();
    gtk::init().expect("no hay sesión gráfica");
    let fallos = std::rc::Rc::new(std::cell::Cell::new(0));

    correr("Reordenación, byte a byte", ALGORITMO, fallos.clone());

    match video_real() {
        Some(mp4) => {
            println!("\n(vídeo de prueba: {} KiB)", mp4.len() / 1024);
            let incrustado = format!("'{}'", base64(&mp4));
            correr(
                "Reproducción de un H.264/AAC con el índice al final",
                &REPRODUCCION.replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
            correr(
                "Un chat con dos docenas de adjuntos",
                &MUCHOS.replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
        }
        None => println!(
            "\n── Reproducción real\n   (saltada: hace falta ffmpeg en el PATH para generar el vídeo)"
        ),
    }

    println!();
    if fallos.get() > 0 {
        println!("{} maqueta(s) con fallos", fallos.get());
        std::process::exit(1);
    }
    println!("Todo bien.");
}
