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
    // Y para ver que una maqueta nueva canta con la versión anterior:
    // `WRUSP_BANCO_BROWSER=ruta/al/browser.rs` de otra versión.
    let fuente = match std::env::var("WRUSP_BANCO_BROWSER") {
        Ok(ruta) => std::fs::read_to_string(ruta).expect("no se puede leer WRUSP_BANCO_BROWSER"),
        Err(_) => include_str!("../src/browser.rs").to_string(),
    };
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
    // `WRUSP_BANCO_FICHERO=/ruta.mp4`: un vídeo de verdad (por ejemplo, uno
    // volcado por Wrusp desde un chat) en vez del generado con ffmpeg.
    if let Ok(ruta) = std::env::var("WRUSP_BANCO_FICHERO") {
        return std::fs::read(&ruta)
            .map_err(|e| eprintln!("no se pudo leer {ruta}: {e}"))
            .ok();
    }
    // `WRUSP_BANCO_MOVFLAGS` permite generar otras variantes (por ejemplo
    // `frag_keyframe+empty_moov+default_base_moof` para un MP4 fragmentado).
    let movflags = std::env::var("WRUSP_BANCO_MOVFLAGS").unwrap_or_else(|_| "-faststart".into());
    video_con_movflags(&movflags)
}

/// Un H.264/AAC ya ordenado (`+faststart`): el caso normal de WhatsApp hoy,
/// en el que el remux no tiene nada que hacer y no debe tocar nada.
fn video_ordenado() -> Option<Vec<u8>> {
    video_con_movflags("+faststart")
}

fn video_con_movflags(movflags: &str) -> Option<Vec<u8>> {
    // Con `WRUSP_BANCO_SEGUNDOS` se alarga el vídeo para tantear a partir de
    // qué tamaño empieza a romperse el blob en el motor.
    let segundos: u32 = std::env::var("WRUSP_BANCO_SEGUNDOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let bitrate = std::env::var("WRUSP_BANCO_BITRATE").unwrap_or_else(|_| "3M".into());
    let dir = std::env::temp_dir().join("wrusp-banco-faststart");
    std::fs::create_dir_all(&dir).ok()?;
    // `WRUSP_BANCO_VCODEC` cambia el códec de vídeo (libx265, libvpx-vp9…) para
    // reproducir en el banco lo que llegue en los chats de verdad.
    let vcodec = std::env::var("WRUSP_BANCO_VCODEC").unwrap_or_else(|_| "libx264".into());
    let perfil = std::env::var("WRUSP_BANCO_PERFIL").unwrap_or_default();
    let etiqueta: String = format!("{movflags}-{vcodec}-{perfil}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let destino = dir.join(format!("video-{etiqueta}-{segundos}s-{bitrate}.mp4"));
    let salida = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!(
            "testsrc2=size=1280x720:rate=30:duration={segundos}"
        ))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={segundos}"))
        .args([
            "-c:v", &vcodec, "-b:v", &bitrate, "-pix_fmt", "yuv420p", "-c:a", "aac",
        ])
        // HEVC en MP4 con la etiqueta que usan los móviles (hvc1).
        .args(if vcodec == "libx265" {
            vec!["-tag:v", "hvc1"]
        } else {
            vec![]
        })
        // `WRUSP_BANCO_PERFIL=baseline` (o main/high) fija el perfil H.264; el
        // registro real trae Baseline nivel 3.1 con marca mp42.
        .args(match std::env::var("WRUSP_BANCO_PERFIL") {
            Ok(perfil) if vcodec == "libx264" => vec![
                "-profile:v".to_string(),
                perfil,
                "-level".to_string(),
                "3.1".to_string(),
                "-brand".to_string(),
                "mp42".to_string(),
            ],
            _ => vec![],
        })
        .args(["-movflags", movflags])
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
  // La fuente que Wrusp acabó poniendo. Hay que cogerla al vuelo: estos MP4
  // de juguete no llevan un códec de verdad, así que el motor rechaza también
  // la fuente buena y Wrusp desmonta el reproductor. Lo que se comprueba aquí
  // es la reordenación, no que el motor sepa decodificar tres kilobytes de
  // relleno.
  async function fuentePuesta(v, url) {
    for (let i = 0; i < 250; i++) {
      const actual = v.src || '';
      if (actual && actual !== url) return actual;
      await new Promise((l) => setTimeout(l, 10));
    }
    return v.src || '';
  }

  async function reordenadoDe(bytes) {
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    v.src = url;
    const arrancando = v.play().catch(() => {});
    const definitiva = await fuentePuesta(v, url);
    await arrancando;
    // Sin cambio, o detenido por el propio script: no lo tocó. Desde la 0.4.12
    // la fuente pasa a `data:` aunque no haya nada que reordenar, así que «no
    // lo tocó» significa que los bytes son los mismos.
    if (!definitiva || definitiva === url) return null;
    try {
      const resp = await fetch(definitiva);
      const nuevo = new Uint8Array(await resp.arrayBuffer());
      // Desde la 0.4.12 la fuente pasa a `data:` aunque no haya nada que
      // reordenar, así que «no lo tocó» es que los bytes son los mismos.
      if (nuevo.length === bytes.length && nuevo.every((b, i) => b === bytes[i])) return null;
      return nuevo;
    } catch (e) {
      return null;
    }
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
    // Sin `await`: si la promesa de play() no se resuelve nunca, el informe
    // tiene que salir igual, con lo que haya pasado hasta entonces.
    let playResuelto = 'pendiente';
    try {
      v.play().then(() => { playResuelto = 'resuelta'; }, (e) => { playResuelto = 'rechazada:' + e.name; });
    } catch (e) { playResuelto = 'lanzó:' + e.name; }
    await new Promise((listo) => setTimeout(listo, 6000));
    diario.push('play=' + playResuelto + ' readyState=' + v.readyState + ' networkState=' + v.networkState);
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
    // Cuántas lecturas de vídeo dispara entrar en el chat. La 0.4.13 lanzaba
    // una por adjunto —cuarenta en el registro real— porque el cupo solo
    // contaba las copias ya terminadas, y con eso el proceso web se iba al
    // 45 % de CPU y a más de un giga. Aquí se cuentan.
    let preparados = 0;
    window.__wruspOrden = (o) => {
      if (decodeURIComponent(String(o)).indexOf('entregado como data:') >= 0) preparados++;
    };
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

    // Que el motor tenga tiempo de arrancar lo que vaya a arrancar. Y que
    // se note si la página se queda muerta: un temporizador de 100 ms que
    // debería dispararse unas cuarenta veces en cuatro segundos.
    let latidos = 0;
    const pulso = setInterval(() => latidos++, 100);
    await new Promise((listo) => setTimeout(listo, 4000));
    clearInterval(pulso);
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
      // Con el cupo puesto salen dos por adelantado y una del vídeo que se
      // pulsa. Seis deja margen y sigue cantando si vuelve la regresión, que
      // daba una lectura por adjunto.
      ['entrar en el chat no lee todos los vídeos', preparados <= 6],
      ['y la página sigue viva mientras tanto', latidos >= 25],
      ['el que se pulsa se reproduce', !fallo && elegido.currentTime > 0.2],
      ['ningún otro adjunto queda roto', rotos === 0],
    ], CUANTOS + ' adjuntos · precargando=' + precargando + ' cargando=' + conRed
       + ' preparados=' + preparados + ' latidos=' + latidos
       + ' rotos=' + rotos + ' t=' + elegido.currentTime.toFixed(2) + ' ' + fallo);
  })();
</script>
"#;

/// Maqueta 4 — `autoplay`, que es como WhatsApp pone los GIF y las
/// previsualizaciones silenciosas. No pasa por `play()`: el motor carga la
/// fuente en cuanto la recibe. Hasta la 0.4.14 recibía el blob, fallaba, y
/// Wrusp le cambiaba la fuente después; desde la 0.4.15 recibe directamente
/// la `data:` (ADR-043), y tiene que arrancar sin que nadie lo toque.
///
/// Y de paso lo de la visibilidad: un GIF que sale de la pantalla se pausa,
/// pero al volver tiene que seguir; la 0.4.4 lo dejaba congelado.
const AUTOPLAY: &str = r#"
<video id="v" autoplay muted loop playsinline style="width:320px;display:block"></video>
<div id="relleno" style="height:4000px"></div>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    const diario = [];
    for (const evento of ['error', 'loadedmetadata', 'playing', 'pause'])
      v.addEventListener(evento, () => {
        diario.push(evento
          + (evento === 'error' ? '(' + ((v.error && v.error.code)
              || (window.__wruspUltimoFallo && window.__wruspUltimoFallo.codigo) || '?') + ')' : '')
          + (v.src === url ? '@original' : '@reordenado'));
      });
    // Testigo: un observador ajeno a Wrusp sobre el mismo elemento. Si este
    // tampoco se entera del desplazamiento, la prueba de la pausa no puede
    // decir nada del script, solo del motor. Medido en WebKitGTK 2.52: no se
    // entera, así que sin este testigo la maqueta acusaba a Wrusp de algo que
    // no depende de él.
    let avisosDelTestigo = 0;
    let fueraSegunTestigo = false;
    new IntersectionObserver((es) => {
      for (const e of es) {
        avisosDelTestigo++;
        if (!e.isIntersecting) fueraSegunTestigo = true;
      }
    }, { rootMargin: '50px' }).observe(v);

    v.src = url;   // sin play(): arranca solo, o no
    await new Promise((listo) => setTimeout(listo, 7000));
    const enMarcha = !v.paused && v.currentTime > 0.2;
    const t1 = v.currentTime;

    // Fuera de la pantalla y de vuelta.
    window.scrollTo(0, 4000);
    await new Promise((listo) => setTimeout(listo, 1500));
    const pausadoFuera = v.paused;
    diario.push('fuera(y=' + Math.round(window.scrollY)
      + ' rect=' + Math.round(v.getBoundingClientRect().top)
      + ' paused=' + pausadoFuera + ' testigo=' + avisosDelTestigo + ')');
    window.scrollTo(0, 0);
    await new Promise((listo) => setTimeout(listo, 2500));
    const sigueAlVolver = !v.paused && v.currentTime !== t1;

    // La pausa solo se le puede exigir a Wrusp si el motor avisó de que el
    // elemento salió. Cuando el testigo no se entera, se comprueba lo único
    // que sí depende del script: que no lo deja parado por su cuenta.
    const pausaJuzgable = fueraSegunTestigo;
    informe([
      ['la fuente queda lista sin que nadie llame a play()', v.src !== url],
      ['el vídeo con autoplay arranca', enMarcha],
      ['no queda un error de medio colgando', !v.error],
      [pausaJuzgable ? 'fuera de la pantalla se pausa' : 'sin aviso del motor: el vídeo sigue como estaba',
       pausaJuzgable ? pausadoFuera : !pausadoFuera],
      ['al volver sigue reproduciéndose', sigueAlVolver],
    ], 'currentTime=' + v.currentTime.toFixed(2) + ' · ' + diario.join(' '));
  })();
</script>
"#;

/// Maqueta 5 — el caso de la 0.4.6: un MP4 que ya viene con el índice delante
/// (como casi todos los de WhatsApp hoy, según el registro) puesto en un
/// <video autoplay loop>. El remux no tiene nada que hacer y el manejador de
/// `play` no debe entrar en bucle de pausa y reproducción: se cuentan las
/// pausas. Medido en uso real: 33 s de CPU en una sesión de 30 s.
const YA_ORDENADO: &str = r#"
<video id="v" autoplay muted loop playsinline style="width:320px;display:block"></video>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    let pausas = 0, arranques = 0, errores = 0;
    v.addEventListener('pause', () => pausas++);
    v.addEventListener('play', () => arranques++);
    v.addEventListener('error', () => errores++);
    v.src = url;
    await new Promise((listo) => setTimeout(listo, 6000));
    // Y lo que hace WhatsApp con un GIF al volver a la vista: play() explícito.
    try { await v.play(); } catch (e) {}
    await new Promise((listo) => setTimeout(listo, 2000));
    informe([
      ['la fuente pasa a data:', v.src.indexOf('data:') === 0],
      ['arranca y avanza', !v.paused && v.currentTime > 1],
      ['sin bucle de pausa/reproducción', pausas <= 2 && arranques <= 4],
      ['sin errores de medio', errores === 0],
    ], 'pausas=' + pausas + ' arranques=' + arranques + ' errores=' + errores + ' t=' + v.currentTime.toFixed(2));
  })();
</script>
"#;

/// Copia de `permissions::apagar_sesion_multimedia` (los ejemplos no ven los
/// módulos del binario): apaga la característica MediaSession del motor.
fn apagar_sesion_multimedia(settings: &webkit2gtk::Settings) {
    use std::ffi::CStr;
    use webkit2gtk::glib::translate::ToGlibPtr;

    #[repr(C)]
    struct WebKitFeature {
        _opaco: [u8; 0],
    }
    #[repr(C)]
    struct WebKitFeatureList {
        _opaco: [u8; 0],
    }
    extern "C" {
        fn webkit_settings_get_all_features() -> *mut WebKitFeatureList;
        fn webkit_feature_list_get_length(lista: *mut WebKitFeatureList) -> usize;
        fn webkit_feature_list_get(lista: *mut WebKitFeatureList, i: usize) -> *mut WebKitFeature;
        fn webkit_feature_list_unref(lista: *mut WebKitFeatureList);
        fn webkit_feature_get_identifier(f: *mut WebKitFeature) -> *const std::os::raw::c_char;
        fn webkit_settings_set_feature_enabled(
            settings: *mut webkit2gtk::ffi::WebKitSettings,
            f: *mut WebKitFeature,
            activada: webkit2gtk::glib::ffi::gboolean,
        );
    }
    unsafe {
        let lista = webkit_settings_get_all_features();
        if lista.is_null() {
            return;
        }
        for i in 0..webkit_feature_list_get_length(lista) {
            let caracteristica = webkit_feature_list_get(lista, i);
            if caracteristica.is_null() {
                continue;
            }
            let identificador = webkit_feature_get_identifier(caracteristica);
            if identificador.is_null()
                || CStr::from_ptr(identificador).to_bytes() != b"MediaSession"
            {
                continue;
            }
            webkit_settings_set_feature_enabled(
                settings.to_glib_none().0,
                caracteristica,
                webkit2gtk::glib::ffi::GFALSE,
            );
            break;
        }
        webkit_feature_list_unref(lista);
    }
}

/// Maqueta 6 — ¿por qué falla dentro de WhatsApp lo que aquí se reproduce?
/// Con `WRUSP_BANCO_FICHERO` y el vídeo real volcado: se prueba `canPlayType`
/// con las cadenas de códec que WhatsApp podría declarar, un `<source type>`
/// con ellas, y (con `WRUSP_BANCO_CSP`) la política CSP de WhatsApp como meta.
const DIAGNOSTICO: &str = r#"
CSP_META
<video id="a" muted playsinline style="width:200px"></video>
<video id="b" muted playsinline style="width:200px"></video>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const blob = new Blob([bytes], { type: 'video/mp4' });
    const url = URL.createObjectURL(blob);
    const v = document.createElement('video');
    const cadenas = [
      'video/mp4',
      'video/mp4; codecs="avc1.64001f"',
      'video/mp4; codecs="avc1.64001F, mp4a.40.2"',
      'video/mp4; codecs="avc1.42001f"',
      'video/mp4; codecs="avc1.42E01E, mp4a.40.2"',
      'video/mp4; codecs="avc1.4d401f, mp4a.40.2"',
      'video/mp4; codecs=avc1.64001f,mp4a.40.2',
    ];
    const cpt = cadenas.map((c) => c + ' → "' + v.canPlayType(c) + '"');
    const mse = (typeof MediaSource === 'function')
      ? cadenas.map((c) => MediaSource.isTypeSupported(c)).join(',') : 'sin MSE';

    const esperarFin = (m, ms) => new Promise((listo) => {
      const t = setTimeout(() => listo('sin veredicto en ' + ms + ' ms'), ms);
      m.addEventListener('error', () => { clearTimeout(t); listo('error ' + (m.error && m.error.code) + ' red ' + m.networkState); }, { once: true });
      m.addEventListener('playing', () => { clearTimeout(t); listo('reproduce'); }, { once: true });
    });

    // a) src directo (como el banco de siempre)
    const a = document.getElementById('a');
    a.src = url; a.play().catch(() => {});
    const ra = await esperarFin(a, 6000);

    // b) <source type="…codecs…"> como podría hacerlo WhatsApp
    const b = document.getElementById('b');
    const fuente = document.createElement('source');
    fuente.type = 'video/mp4; codecs="avc1.64001F, mp4a.40.2"';
    fuente.src = url;
    b.appendChild(fuente);
    b.load(); b.play().catch(() => {});
    const rb = await esperarFin(b, 6000);

    informe([
      ['src directo reproduce', ra === 'reproduce'],
      ['<source type=codecs> reproduce', rb === 'reproduce'],
      ['canPlayType admite avc1 High', /64001f"? → "(probably|maybe)/i.test(cpt[1])],
    ], 'a=' + ra + ' · b=' + rb + ' · ' + cpt.join(' · ') + ' · MSE=' + mse
       + ' · csp=' + (document.querySelector('meta[http-equiv]') ? 'sí' : 'no'));
  })();
</script>
"#;

/// Maquetas 7a/7b/7c — la misma prueba con tres formas de entregar el vídeo
/// al motor, cada una por separado para que un cuelgue de una no tape a las
/// otras: `blob:` (lo que hace WhatsApp), `data:` (lo que hacía la 0.3.8) y
/// `MediaSource` con el fichero entero. Se arranca, se salta al 60 % con la
/// barra y se mira si sigue. Con `WRUSP_BANCO_FICHERO` y un vídeo real.
const PRUEBA_SALTO: &str = r#"
  const B64 = MP4_BASE64;
  const espera = (ms) => new Promise((l) => setTimeout(l, ms));
  const bytesDe = () => { const bruto = atob(B64); const b = new Uint8Array(bruto.length); for (let i = 0; i < bruto.length; i++) b[i] = bruto.charCodeAt(i); return b; };
  async function probar(v, preparar) {
    const diario = [];
    v.addEventListener('error', () => diario.push('error' + (v.error && v.error.code) + '/red' + v.networkState));
    v.addEventListener('stalled', () => diario.push('stalled'));
    const t0 = performance.now();
    try { await preparar(v); } catch (e) { diario.push('prep:' + ((e && e.message) || e)); }
    try { await v.play(); } catch (e) { diario.push('play:' + e.name); }
    await espera(3000);
    const antesSalto = v.currentTime;
    const destino = Number.isFinite(v.duration) ? Math.min(v.duration * 0.6, v.duration - 1) : 5;
    const tSalto = performance.now();
    let seeked = false;
    v.addEventListener('seeked', () => { seeked = true; diario.push('seeked@' + Math.round(performance.now() - tSalto) + 'ms'); }, { once: true });
    try { v.currentTime = destino; } catch (e) { diario.push('seek:' + e.name); }
    await espera(4000);
    const trasSalto = v.currentTime;
    informe([
      ['arranca', antesSalto > 0.5],
      ['salta con la barra', seeked && trasSalto > destino - 0.5],
      ['sigue tras el salto', !v.paused && !v.error && trasSalto > destino],
    ], 'antes=' + antesSalto.toFixed(1) + ' destino=' + destino.toFixed(1) + ' después=' + trasSalto.toFixed(1)
      + ' ' + diario.join(',') + ' (' + Math.round(performance.now() - t0) + ' ms)');
  }
"#;

const SALTO_BLOB: &str = r#"
<video id="v" muted playsinline style="width:240px"></video>
<script>SCRIPT</script>
<script>
PRUEBA_SALTO
  probar(document.getElementById('v'), async (v) => {
    v.src = URL.createObjectURL(new Blob([bytesDe()], { type: 'video/mp4' }));
  });
</script>
"#;

const SALTO_DATA: &str = r#"
<video id="v" muted playsinline style="width:240px"></video>
<script>SCRIPT</script>
<script>
PRUEBA_SALTO
  probar(document.getElementById('v'), async (v) => {
    v.src = 'data:video/mp4;base64,' + B64;
  });
</script>
"#;

const SALTO_MSE: &str = r#"
<video id="v" muted playsinline style="width:240px"></video>
<script>SCRIPT</script>
<script>
PRUEBA_SALTO
  probar(document.getElementById('v'), (v) => new Promise((listo, falla) => {
    if (typeof MediaSource !== 'function') return falla(new Error('sin MediaSource'));
    const tipo = 'video/mp4; codecs="avc1.64001f, mp4a.40.2"';
    if (!MediaSource.isTypeSupported(tipo)) return falla(new Error('tipo no admitido'));
    const ms = new MediaSource();
    v.src = URL.createObjectURL(ms);
    ms.addEventListener('sourceopen', () => {
      try {
        const sb = ms.addSourceBuffer(tipo);
        sb.addEventListener('updateend', () => { try { ms.endOfStream(); } catch (e) {} listo(); }, { once: true });
        sb.addEventListener('error', () => falla(new Error('SourceBuffer error')), { once: true });
        sb.appendBuffer(bytesDe());
      } catch (e) { falla(e); }
    }, { once: true });
    setTimeout(() => falla(new Error('sourceopen no llegó')), 5000);
  }));
</script>
"#;

/// Maqueta 8 — la fuente retenida (ADR-043). WhatsApp asigna el blob a un
/// vídeo que nadie ha pulsado: el elemento no recibe fuente real hasta el
/// `play()`, así que no tiene reproductor que destruir ni conexión al bus, y
/// al pulsar recibe la `data:` definitiva de una vez. La página, mientras,
/// sigue viendo en `src` el blob que puso. Y cuando WhatsApp vuelve a asignar
/// el mismo blob al repintar, no se recarga nada. Con la 0.4.14 canta: la
/// fuente real pasaba por el blob y después por la `data:`, y la reasignación
/// desmontaba el reproductor en marcha.
const RETENIDA: &str = r#"
<video id="v" muted playsinline style="width:240px;display:block"></video>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const espera = (ms) => new Promise((l) => setTimeout(l, ms));
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    let errores = 0;
    v.addEventListener('error', () => errores++);
    // `currentSrc` es la fuente que tiene el motor, y el script no la disfraza.
    const vistas = [];
    let previa = '';
    const muestreo = setInterval(() => {
      const c = v.currentSrc || '';
      if (c !== previa) { vistas.push(c.slice(0, 5) || 'nada'); previa = c; }
    }, 10);
    v.src = url;   // sin autoplay y sin play()

    // Lo que hace el usuario: mirar el chat un momento antes de pulsar.
    await espera(2000);
    const vistoPorLaPagina = v.src;
    const realAntes = v.currentSrc;
    const redAntes = v.networkState;
    const precarga = v.preload;
    const erroresAntes = errores;

    let promesa = 'pendiente';
    try { await v.play(); promesa = 'resuelta'; } catch (e) { promesa = 'rechazada:' + e.name; }
    await espera(3000);
    const arranco = v.currentSrc.indexOf('data:') === 0 && v.currentTime > 0.5 && !v.paused;

    // WhatsApp repinta y vuelve a asignar el mismo blob.
    const t = v.currentTime;
    v.src = url;
    await espera(1000);
    clearInterval(muestreo);
    const reales = vistas.filter((x) => x !== 'nada');
    informe([
      ['la página ve el blob que puso', vistoPorLaPagina === url],
      ['sin pulsar, el motor no tiene fuente', realAntes === '' && redAntes === HTMLMediaElement.NETWORK_EMPTY],
      ['ni precarga', precarga === 'none'],
      ['ni falla', erroresAntes === 0],
      ['al pulsar arranca con data:', promesa === 'resuelta' && arranco],
      ['reasignar el mismo blob no lo recarga', v.currentTime > t && !v.paused],
      ['su fuente real se fija una vez', reales.length === 1],
      ['y sin ningún error', errores === 0],
    ], 'fuentes: ' + vistas.join(' ') + ' · red antes=' + redAntes + ' preload=' + precarga
      + ' play=' + promesa + ' t=' + t.toFixed(2) + '→' + v.currentTime.toFixed(2) + ' errores=' + errores);
  })();
</script>
"#;

/// Maqueta 9 — ficheros que cuestan caro. Los números del remux vienen del
/// MP4 que alguien ha enviado y de ellos sale el trabajo que se hace en el
/// hilo de la página, así que conviene tener medido lo que cuesta el peor
/// fichero que llega hasta ahí.
///
/// Honestidad sobre lo que prueba: **ninguno de estos casos rompía** el
/// script anterior. El sondeo barato solo mira 64 cajas de nivel superior,
/// así que un fichero con cientos de miles de ellas ni llega al remux, y las
/// tablas que mienten sobre su tamaño ya las rechazaba la comprobación
/// contra el tamaño real de la caja. Las cotas y la búsqueda binaria que
/// añade la 0.4.13 son defensa en profundidad, tomada de oxidezap (MIT): lo
/// que compran es que el trabajo quede acotado por construcción y no por lo
/// que otro freno deje pasar. Esta maqueta es la que lo deja medido.
const HOSTILES: &str = r#"
<video id="v"></video>
<script>SCRIPT</script>
<script>
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
  const ftyp = () => caja('ftyp', texto('isomiso2avc1mp41'));

  // Una tabla `stco` con `offsets` de verdad dentro.
  function stco(offsets) {
    const c = new Uint8Array(8 + offsets.length * 4);
    const v = new DataView(c.buffer);
    v.setUint32(0, 0);
    v.setUint32(4, offsets.length);
    offsets.forEach((o, i) => v.setUint32(8 + i * 4, o));
    return caja('stco', c);
  }
  // Una `stco` que dice tener `cuantos` entradas y no las trae.
  function stcoMentirosa(cuantos) {
    const c = new Uint8Array(8);
    const v = new DataView(c.buffer);
    v.setUint32(0, 0);
    v.setUint32(4, cuantos);
    return caja('stco', c);
  }
  // Un `moov` anidado `veces` veces dentro de sí mismo.
  function moovProfundo(veces) {
    let dentro = stcoMentirosa(1);
    for (let i = 0; i < veces; i++) dentro = caja('moov', dentro);
    return dentro;
  }

  // Las cajas de nivel superior de lo que el motor acabó recibiendo. Desde
  // que la fuente se entrega como `data:` aunque no haya nada que reordenar,
  // «cambió la URL» ya no dice si el remux actuó: lo dice el orden.
  async function tiposDe(src) {
    const resp = await fetch(src);
    const u8 = new Uint8Array(await resp.arrayBuffer());
    const vista = new DataView(u8.buffer);
    const salida = [];
    let p = 0;
    while (p + 8 <= u8.length && salida.length < 6) {
      let tam = vista.getUint32(p);
      salida.push(String.fromCharCode(u8[p + 4], u8[p + 5], u8[p + 6], u8[p + 7]));
      if (tam === 1) tam = Number(vista.getBigUint64(p + 8));
      else if (tam === 0) tam = u8.length - p;
      if (tam < 8) break;
      p += tam;
    }
    return salida;
  }

  // El script no exporta nada: se ejercita por donde entra, un blob de vídeo
  // que se reproduce. Devuelve cuánto tardó en decidir y si el índice acabó
  // delante de los datos.
  // Igual que en la maqueta del algoritmo: la fuente hay que cogerla al vuelo
  // porque el motor rechaza estos ficheros de juguete y Wrusp los desmonta.
  async function fuentePuesta(v, url) {
    for (let i = 0; i < 250; i++) {
      const actual = v.src || '';
      if (actual && actual !== url) return actual;
      await new Promise((l) => setTimeout(l, 10));
    }
    return v.src || '';
  }

  async function medir(bytes) {
    const url = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    const v = document.getElementById('v');
    const t0 = performance.now();
    v.src = url;
    const arrancando = v.play().catch(() => {});
    const puesta = await fuentePuesta(v, url);
    await arrancando;
    const ms = performance.now() - t0;
    let tipos = [];
    try { tipos = await tiposDe(puesta); } catch (e) { /* fuente retirada */ }
    const iMoov = tipos.indexOf('moov');
    const iDatos = tipos.findIndex((t) => t === 'mdat' || t === 'free');
    const reordenado = iMoov >= 0 && (iDatos < 0 || iMoov < iDatos);
    v.removeAttribute('src');
    v.load();
    URL.revokeObjectURL(url);
    return { ms, reordenado, tipos: tipos.join('/') };
  }

  (async () => {
    const pruebas = [];
    const notas = [];

    // ── El peor fichero que llega al remux: tantas cajas de nivel superior
    // como admite el sondeo, y una tabla con muchos trozos. Reubicar cada
    // trozo recorriendo la lista de cajas es el producto de los dos.
    const CAJAS = 60;   // el sondeo barato para en 64
    const TROZOS = 200000;
    const relleno = [];
    for (let i = 0; i < CAJAS; i++) relleno.push(caja('free', new Uint8Array(0)));
    const f = ftyp();
    const mdat = caja('mdat', new Uint8Array(TROZOS * 4));
    const inicioDatos = f.length + CAJAS * 8 + 8;
    const offsets = [];
    for (let i = 0; i < TROZOS; i++) offsets.push(inicioDatos + i * 4);
    const caro = unir([
      f, ...relleno, mdat,
      anidar(['moov', 'trak', 'mdia', 'minf', 'stbl'], stco(offsets)),
    ]);
    let r = await medir(caro);
    notas.push(CAJAS + ' cajas x ' + TROZOS + ' trozos: ' + Math.round(r.ms) + ' ms ' + r.tipos);
    pruebas.push([CAJAS + ' cajas y ' + TROZOS + ' trozos no atascan la página', r.ms < 5000]);
    pruebas.push(['y el índice acaba delante', r.reordenado]);

    // ── Cotas: nada de esto es válido y todo se rechaza enseguida.
    const gigante = unir([
      f, caja('mdat', new Uint8Array(4096)),
      anidar(['moov', 'trak', 'mdia', 'minf', 'stbl'], stcoMentirosa(0xffffffff)),
    ]);
    r = await medir(gigante);
    notas.push('tabla que miente ' + Math.round(r.ms) + ' ms ' + r.tipos);
    pruebas.push(['una tabla que declara 4.000 millones de trozos no se reordena', !r.reordenado]);
    pruebas.push(['y se rechaza enseguida', r.ms < 4000]);

    const hondo = unir([f, caja('mdat', new Uint8Array(4096)), moovProfundo(20000)]);
    r = await medir(hondo);
    notas.push('anidado 20000 ' + Math.round(r.ms) + ' ms ' + r.tipos);
    pruebas.push(['un moov anidado 20.000 veces no se reordena', !r.reordenado]);
    pruebas.push(['y no revienta la pila', r.ms < 4000]);

    // ── Y lo válido sigue pasando.
    const datosBuenos = new Uint8Array(3000).map((_, i) => (i * 31) & 0xff);
    const mdatBueno = caja('mdat', datosBuenos);
    const chunks = [f.length + 8, f.length + 8 + 1000, f.length + 8 + 2000];
    const bueno = unir([
      f, mdatBueno,
      anidar(['moov', 'trak', 'mdia', 'minf', 'stbl'], stco(chunks)),
    ]);
    r = await medir(bueno);
    notas.push('válido ' + r.tipos);
    pruebas.push(['un MP4 válido con el índice al final se sigue reordenando', r.reordenado]);

    informe(pruebas, notas.join(' · '));
  })();
</script>
"#;

/// Maqueta 10 — el cuelgue capturado en uso real el 11-09-2026: cambiar la
/// fuente de un `<video>` que ya tiene reproductor destruye el pipeline de
/// GStreamer **en el hilo principal**, y esa destrucción espera el cerrojo de
/// un *pad* que tiene cogido un hilo de GStreamer. Aquí se mide cuánto dura
/// ese cambio, sin scripts de Wrusp: es el coste de hacerlo.
const COSTE_CAMBIO: &str = r#"
<video id="v" autoplay muted loop playsinline style="width:320px"></video>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const espera = (ms) => new Promise((l) => setTimeout(l, ms));
    const v = document.getElementById('v');
    const tiempos = [];
    for (let i = 0; i < 4; i++) {
      v.src = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
      await espera(2500);                    // que suene de verdad
      const sonaba = !v.paused && v.currentTime > 0.3;
      const t0 = performance.now();
      v.src = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
      tiempos.push((performance.now() - t0).toFixed(0) + (sonaba ? '' : '?'));
      await espera(1500);
    }
    informe([['la medida termina', true]],
      'cambiar la fuente de un vídeo que suena bloquea el hilo: ' + tiempos.join(', ') + ' ms');
  })();
</script>
"#;

/// Maqueta 11 — lo que hace Wrusp con un GIF: un `<video autoplay loop>` con
/// fuente `blob:`. Cambiar la fuente de un elemento que ya tiene reproductor
/// destruye su pipeline en el hilo principal (maqueta 10), y en uso real,
/// sobre un reproductor que acababa de fallar, esa destrucción dejó WhatsApp
/// parado más de dos minutos esperando un cerrojo de GStreamer. La regla que
/// se comprueba es estricta: la fuente **real** del elemento (`currentSrc`,
/// que el script no puede disfrazar) se fija una vez y no cambia nunca.
const GIF_SIN_CAMBIO: &str = r#"
<video id="v" autoplay muted loop playsinline style="width:320px"></video>
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const espera = (ms) => new Promise((l) => setTimeout(l, ms));
    const v = document.getElementById('v');
    let ultimo = performance.now(), peor = 0;
    const pulso = setInterval(() => { const a = performance.now(); peor = Math.max(peor, a - ultimo - 16); ultimo = a; }, 16);
    const vistas = [];
    let previa = '', empezo = false;
    v.addEventListener('playing', () => { empezo = true; });
    const muestreo = setInterval(() => {
      const c = v.currentSrc || '';
      if (c !== previa) { vistas.push((empezo ? 'sonando→' : 'antes→') + (c.slice(0, 5) || 'nada')); previa = c; }
    }, 10);
    v.src = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    await espera(6000);
    clearInterval(muestreo); clearInterval(pulso);
    const reales = vistas.filter((x) => !x.endsWith('→nada'));
    informe([
      ['el GIF arranca', !v.paused && v.currentTime > 0.5],
      ['su fuente real se fija una vez y no cambia', reales.length === 1],
    ], 'fuentes: ' + vistas.join(' ') + ' · peor latido ' + peor.toFixed(0) + ' ms');
  })();
</script>
"#;

/// Maqueta 12 — la sonda con la que WhatsApp mide un vídeo (`MediaLoad`, en
/// su código público, copiada paso a paso): un <video> suelto, fuera del
/// documento, con `crossOrigin`, el blob, `load()` y `currentTime = 1`; espera
/// `loadedmetadata` y `canplaythrough`, vuelve a 0 y espera el `seeked`, sin
/// llamar nunca a `play()`. WhatsApp da el vídeo por perdido a los 20 s
/// («video-load-timeout»). La 0.4.15 retenía la fuente hasta el `play()` y
/// esta sonda no acababa nunca: salió en el registro real a los pocos minutos
/// de instalarla.
const SONDA_WHATSAPP: &str = r#"
<script>SCRIPT</script>
<script>
  (async () => {
    const bruto = atob(MP4_BASE64);
    const bytes = new Uint8Array(bruto.length);
    for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
    const blob = new Blob([bytes], { type: 'video/mp4' });
    const t0 = performance.now();
    const i = document.createElement('video');
    i.setAttribute('crossOrigin', 'anonymous');
    i.volume = 0;
    i.muted = true;
    i.playsinline = true;
    let meta = false, listo = false, error = '';
    const r = await new Promise((resolver) => {
      const p = () => { if (meta && listo) resolver('ok'); };
      i.onloadedmetadata = () => { i.onloadedmetadata = null; meta = true; p(); };
      i.oncanplaythrough = () => {
        i.oncanplaythrough = null;
        i.onseeked = () => { listo = true; i.onseeked = null; p(); };
        i.currentTime = 0;
      };
      i.onerror = () => { error = ' error ' + (i.error ? i.error.code : '?'); resolver('error'); };
      i.src = URL.createObjectURL(blob);
      i.load();
      i.currentTime = 1;
      setTimeout(() => resolver('plazo agotado'), 10000);
    });
    const ms = performance.now() - t0;
    const ancho = i.videoWidth, alto = i.videoHeight, duracion = i.duration;
    // Y lo que hace WhatsApp al terminar con ella.
    i.pause(); i.src = ''; i.load();
    informe([
      ['la sonda termina sin reproducir', r === 'ok'],
      ['con dimensiones y duración', ancho > 0 && duracion > 5],
    ], 'resultado=' + r + ' en ' + ms.toFixed(0) + ' ms · ' + ancho + 'x' + alto + ' duración=' + duracion + error);
  })();
</script>
"#;

/// Descriptores del `dbus-broker` de la sesión de este usuario. Cada `<video>`
/// con fuente abre una conexión propia al bus y no la suelta hasta que el
/// recolector destruye el elemento (ADR-042): un banco con cientos de vídeos
/// tumbó el escritorio el 11-09-2026.
fn fds_bus() -> Option<usize> {
    let yo = std::fs::read_to_string("/proc/self/status").ok()?;
    let uid = yo
        .lines()
        .find(|l| l.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .to_string();
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let ruta = e.path();
        let Ok(st) = std::fs::read_to_string(ruta.join("status")) else {
            continue;
        };
        let nombre = st.lines().next().unwrap_or("");
        let suyo = st
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1));
        if nombre.ends_with("dbus-broker") && suyo == Some(uid.as_str()) {
            return std::fs::read_dir(ruta.join("fd")).ok().map(|d| d.count());
        }
    }
    None
}

/// Descriptores de más en el bus antes de parar el banco en seco.
const FRENO_BUS: usize = 150;

fn correr(nombre: &str, maqueta: &str, fallos: std::rc::Rc<std::cell::Cell<u32>>) {
    let nombre_para_tiempo = nombre;
    let base_bus = fds_bus().unwrap_or(0);
    // `WRUSP_BANCO_SOLO=texto` corre solo las maquetas cuyo nombre lo contenga.
    if let Ok(solo) = std::env::var("WRUSP_BANCO_SOLO") {
        if !nombre.contains(&solo) {
            return;
        }
    }
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
        // `WRUSP_BANCO_AJUSTES=1`: los mismos ajustes del motor que pone Wrusp
        // en `permissions::configure`, para que el banco mida el mismo motor.
        if std::env::var_os("WRUSP_BANCO_AJUSTES").is_some() {
            ajustes.set_enable_media_stream(true);
            ajustes.set_enable_webrtc(true);
            ajustes.set_enable_mediasource(true);
            ajustes.set_enable_media_capabilities(true);
            ajustes.set_enable_encrypted_media(true);
            ajustes.set_enable_smooth_scrolling(true);
            ajustes.set_enable_page_cache(true);
            ajustes.set_javascript_can_access_clipboard(true);
            ajustes.set_user_agent(Some("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"));
            apagar_sesion_multimedia(&ajustes);
        }
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
    // `WRUSP_BANCO_ESPERA=segundos` alarga el plazo: las maquetas con vídeos
    // reales de varios MB y varias pruebas seguidas no caben en 30 s.
    let espera: u32 = std::env::var("WRUSP_BANCO_ESPERA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    temporizador.set(Some(gtk::glib::timeout_add_seconds_local(
        espera,
        move || {
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
        },
    )));
    // El freno: si el bus de sesión crece de más, se para antes de tumbarlo.
    let freno = gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        if fds_bus().unwrap_or(0) > base_bus + FRENO_BUS {
            eprintln!("FRENO: el bus de sesión ha crecido de más; se para el banco");
            std::process::exit(2);
        }
        gtk::glib::ControlFlow::Continue
    });
    gtk::main();
    freno.remove();
    if let Some(id) = temporizador.take() {
        id.remove();
    }
    // Fuera la página: que no siga reproduciendo ni informando por detrás.
    vista.load_html("", None);
    unsafe {
        ventana.destroy();
    }
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
    correr(
        "Ficheros que cuestan caro: trabajo acotado y cotas del remux",
        HOSTILES,
        fallos.clone(),
    );

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
            correr(
                "La fuente se retiene hasta que se pulsa",
                &RETENIDA.replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
            correr(
                "Autoplay: la fuente llega antes de arrancar, y vuelve tras salir de pantalla",
                &AUTOPLAY.replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
            {
                let meta = std::env::var("WRUSP_BANCO_CSP")
                    .map(|c| format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">", c.replace('"', "&quot;")))
                    .unwrap_or_default();
                correr(
                    "Diagnóstico: canPlayType, <source type> y CSP",
                    &DIAGNOSTICO.replace("MP4_BASE64", &incrustado).replace("CSP_META", &meta),
                    fallos.clone(),
                );
            }
            if std::env::var_os("WRUSP_BANCO_FICHERO").is_some() {
                for (nombre, maqueta) in [
                    ("Salto con la barra: blob:", SALTO_BLOB),
                    ("Salto con la barra: data:", SALTO_DATA),
                    ("Salto con la barra: MediaSource", SALTO_MSE),
                ] {
                    correr(
                        nombre,
                        &maqueta
                            .replace("PRUEBA_SALTO", PRUEBA_SALTO)
                            .replace("MP4_BASE64", &incrustado),
                        fallos.clone(),
                    );
                }
            }
            correr(
                "Cambio de fuente: cuánto bloquea desmontar un vídeo que suena",
                &COSTE_CAMBIO
                    .replace("<script>SCRIPT</script>", "")
                    .replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
            correr(
                "Un GIF recibe su fuente una sola vez",
                &GIF_SIN_CAMBIO.replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
            correr(
                "La sonda de WhatsApp mide el vídeo sin reproducirlo",
                &SONDA_WHATSAPP.replace("MP4_BASE64", &incrustado),
                fallos.clone(),
            );
            if let Some(ordenado) = video_ordenado() {
                let incrustado = format!("'{}'", base64(&ordenado));
                correr(
                    "Un MP4 ya ordenado con autoplay no entra en bucle",
                    &YA_ORDENADO.replace("MP4_BASE64", &incrustado),
                    fallos.clone(),
                );
            }
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
