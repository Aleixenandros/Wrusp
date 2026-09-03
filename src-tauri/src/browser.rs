//! Disfraz de navegador para WhatsApp Web.
//!
//! El motor es WebKitGTK, o sea el mismo que Safari, y WhatsApp lo detecta
//! aunque el user-agent diga Chrome. Comprobado empíricamente con un servidor
//! local que registró lo que ve la página:
//!
//! ```text
//! userAgent : Mozilla/5.0 (X11; Linux x86_64) ... Chrome/131.0.0.0 Safari/537.36
//! platform  : Linux x86_64
//! vendor    : Apple Computer, Inc.     <-- delata a WebKit
//! userAgentData : null                 <-- Chrome de verdad sí lo expone
//! ```
//!
//! Con `vendor` de Apple, WhatsApp concluye que estás en un equipo Apple y
//! muestra el banner «Descarga WhatsApp para Mac». Este script se ejecuta
//! antes que el código de la página y completa el disfraz que el user-agent
//! ya empezaba.

/// Versión de Chrome que decimos ser. Debe cuadrar con `CHROME_UA`.
const CHROME_VERSION: &str = "131";

/// Oculta la promoción de la app nativa («Descarga WhatsApp para Mac»).
///
/// Corregir `navigator.vendor` quitó la detección de Safari, pero WhatsApp
/// sigue anunciando su app de escritorio —en la bienvenida, junto al código QR
/// y como ventana emergente—, y en Wrusp no tiene sentido.
///
/// Se busca por el enlace a la tienda y por el texto del anuncio, nunca por
/// clases CSS (cambian en cada despliegue). Lo delicado es **hasta dónde** se
/// sube al ocultar: el anuncio de la bienvenida vive dentro del panel de
/// conversación, así que subir un padre de más deja el panel entero en
/// `display: none` y los chats dejan de abrirse. La 0.3.9 hizo justo eso,
/// porque el único freno era el tamaño del candidato y un elemento sin layout
/// mide cero, que pasaba por «pequeño».
///
/// Ahora el ascenso para ante cualquier señal de estructura: un candidato sin
/// medidas, demasiado grande, con demasiados descendientes o que contenga la
/// lista de chats o la caja de escritura no se toca. Y lo ocultado se guarda:
/// en cuanto el anuncio desaparece de su texto, vuelve a mostrarse, de modo que
/// un error de puntería dura una pasada y no toda la sesión.
///
/// Es cosmético: si algún día deja de encontrarlo, lo peor que pasa es que el
/// anuncio vuelva a verse.
pub fn hide_native_app_promo_script() -> String {
    r#"(function () {
  const TIENDAS = ['apps.apple.com', 'microsoft.com/store', 'aka.ms/', 'whatsapp.com/download'];
  // Muy específicos a propósito: un patrón laxo se llevaría por delante
  // mensajes normales del chat.
  const ANUNCIOS = [
    /whatsapp\s+(para|for)\s+(mac|windows|escritorio|desktop)/i,
    /(descarga|descargar|download|get)\s+whatsapp\s+(para|for)\s/i,
    /(consigue|descarga)\s+la\s+(app|aplicación)\s+de\s+escritorio/i,
    /get\s+the\s+desktop\s+app/i,
  ];
  const AREA_MAXIMA = 0.35;   // del área de la ventana
  const HIJOS_MAXIMOS = 60;   // más que esto ya no es una tarjeta
  // Anclas genéricas de la estructura de la página: si el candidato contiene
  // alguna, es armazón y no un anuncio.
  const ESTRUCTURA = '[role="grid"], [role="textbox"], [role="application"], #main, #side, #app';
  // Todo lo que sea contenido de los chats: ni el texto de un mensaje ni un
  // enlace que alguien haya enviado se tocan jamás. Sin esto, un «¿usas
  // WhatsApp para Mac?» en una conversación desaparecía del chat.
  const CONVERSACION = '[role="row"], [role="listitem"], [role="log"], [role="grid"], [role="application"], [data-id]';

  let ocultos = new Set();

  const anotar = (texto) => {
    if (window.__wruspOrden) window.__wruspOrden('log/?m=' + encodeURIComponent(texto));
  };

  const esAnuncio = (texto) => texto.length < 400 && ANUNCIOS.some((r) => r.test(texto));
  const esContenidoDeChat = (nodo) => !!nodo.closest(CONVERSACION);

  // El anuncio por texto solo se busca donde puede estar: la pantalla sin
  // conversación abierta (bienvenida y código QR) y las ventanas emergentes.
  // Con un chat delante, este camino ni se recorre.
  const hayConversacionAbierta = () => !!document.querySelector('[role="application"], [role="row"]');

  // ¿Se puede ocultar esto sin llevarse por delante media interfaz?
  function sePuedeOcultar(nodo) {
    if (!nodo || nodo === document.body || nodo === document.documentElement) return false;
    if (nodo.querySelector(ESTRUCTURA)) return false;
    if (nodo.getElementsByTagName('*').length > HIJOS_MAXIMOS) return false;
    const c = nodo.getBoundingClientRect();
    // Sin medidas no hay forma de juzgar el tamaño: se espera a la pasada
    // siguiente en vez de arriesgarse.
    if (c.width === 0 || c.height === 0) return false;
    return c.width * c.height <= innerWidth * innerHeight * AREA_MAXIMA;
  }

  // La tarjeta que envuelve al elemento, parando en cuanto deje de ser segura.
  function envoltorio(nodo) {
    if (!sePuedeOcultar(nodo)) return null;
    let objetivo = nodo;
    for (let i = 0; i < 6; i++) {
      const padre = objetivo.parentElement;
      if (!sePuedeOcultar(padre)) break;
      objetivo = padre;
    }
    return objetivo;
  }

  // Una emergente ocupa la pantalla entera, así que la regla del tamaño no
  // vale: se cierra por su propio botón y, si no lo tiene, se quita la capa.
  function cerrarEmergente(nodo) {
    const capa = nodo.closest('[role="dialog"], [data-animate-modal-popup], [data-animate-modal-body]');
    if (!capa) return false;
    const cerrar = capa.querySelector(
      'button[aria-label], div[role="button"][aria-label], [data-icon="x"], [data-icon="close"]'
    );
    if (cerrar) {
      cerrar.click();
      anotar('promo: emergente cerrada por su botón');
      return true;
    }
    let capaFija = capa;
    while (capaFija && getComputedStyle(capaFija).position !== 'fixed') capaFija = capaFija.parentElement;
    const objetivo = capaFija || capa;
    if (objetivo.querySelector(ESTRUCTURA)) return false; // no era una emergente
    objetivo.style.display = 'none';
    anotar('promo: emergente oculta');
    return true;
  }

  function ocultar() {
    const nuevos = new Set();

    const esconder = (nodo, motivo) => {
      if (esContenidoDeChat(nodo)) return;
      if (cerrarEmergente(nodo)) return;
      const objetivo = envoltorio(nodo);
      if (!objetivo) return;
      if (!ocultos.has(objetivo)) anotar('promo oculta por ' + motivo + ': <' + objetivo.tagName + '>');
      objetivo.style.display = 'none';
      nuevos.add(objetivo);
    };

    for (const enlace of document.querySelectorAll('a[href]')) {
      const href = enlace.href || '';
      if (TIENDAS.some((t) => href.includes(t))) esconder(enlace, 'enlace');
    }

    // Sin enlace a la tienda: el anuncio puede ser un botón que abre otra cosa.
    // Se mira solo el nodo más hondo que contiene el texto, para no subir de más.
    const emergentes = document.querySelectorAll('[role="dialog"]');
    const ambito = hayConversacionAbierta() ? emergentes : [document];
    for (const raiz of ambito) {
      for (const nodo of raiz.querySelectorAll('div, span, h1, h2, h3, p, button')) {
        if (nodo.children.length > 3) continue;
        if (esAnuncio((nodo.textContent || '').trim())) esconder(nodo, 'texto');
      }
    }

    // Lo que se ocultó antes y ya no lleva el anuncio, vuelve. Sin esto, una
    // equivocación de puntería se queda para toda la sesión: es lo que dejaba
    // el panel de conversación en blanco.
    for (const nodo of ocultos) {
      if (nuevos.has(nodo)) continue;
      if (!nodo.isConnected) continue;
      const texto = (nodo.textContent || '').trim();
      const tieneEnlace = Array.prototype.some.call(
        nodo.querySelectorAll('a[href]'), (a) => TIENDAS.some((t) => (a.href || '').includes(t))
      );
      if (!esAnuncio(texto) && !tieneEnlace) {
        nodo.style.display = '';
        anotar('promo: se devuelve <' + nodo.tagName + '>, ya no anuncia nada');
      } else {
        nuevos.add(nodo);
      }
    }
    ocultos = nuevos;
  }

  let pendiente = 0;
  const pedirRepaso = () => {
    if (pendiente) return;
    // Con una conversación abierta no hay anuncio por texto fuera de una
    // emergente, y los enlaces que haya dentro del chat están expresamente
    // excluidos. Evita recorrer todo el DOM cada vez que carga una miniatura.
    if (!ocultos.size && hayConversacionAbierta()
        && !document.querySelector('[role="dialog"]')) return;
    pendiente = setTimeout(() => {
      pendiente = 0;
      ocultar();
    }, 150);
  };

  const arrancar = () => {
    ocultar();
    // WhatsApp vuelve a pintar la bienvenida al cambiar de chat, y la
    // emergente aparece cuando le conviene.
    new MutationObserver(pedirRepaso).observe(document.body, { childList: true, subtree: true });
  };
  if (document.body) arrancar();
  else document.addEventListener('DOMContentLoaded', arrancar);
})();"#
        .to_string()
}

/// Oculta la parte de vídeo de WebCodecs.
///
/// WebKitGTK anuncia `VideoDecoder` y su `isConfigSupported('avc1.…')`
/// responde que sí, pero la decodificación real no emite un solo fotograma:
/// 240 unidades de acceso H.264 válidas → 0 frames y «Decode error»
/// (comprobado con arnés propio contra WebKitGTK 2.52). WhatsApp, viéndose en
/// Chrome con WebCodecs disponible, elige su reproductor moderno y el vídeo
/// queda muerto: el play no hace nada y el póster no se mueve aunque el tiempo
/// avance. Sin la API a la vista, cae al reproductor `<video>`/MSE, que
/// funciona (verificado: progresivo y MSE, todos los perfiles H.264).
///
/// Solo se retira el lado de vídeo: `AudioDecoder` se deja porque no hay
/// síntomas en notas de voz y quitarlo podría romper lo que hoy funciona.
pub fn hide_webcodecs_script() -> String {
    r#"(function () {
  for (const k of ['VideoDecoder', 'VideoEncoder', 'EncodedVideoChunk']) {
    try { delete window[k]; } catch (e) { /* no redefinible: se queda */ }
  }
})();"#
        .to_string()
}

/// Deja reproducibles los vídeos que WhatsApp sirve como `blob:`.
///
/// WebKitGTK 2.52 entrega los blobs al demuxer a través de un búfer circular
/// pequeño. Cuando el MP4 es mayor que ese búfer y trae su índice (`moov`) al
/// final —la forma habitual en que WhatsApp entrega los vídeos—, `qtdemux`
/// pide el índice, recibe datos de otra posición y muere: en el registro real
/// aparece `atom has bogus size 720732826` seguido de «Este archivo no es
/// válido y no se puede reproducir». Detrás quedan miles de errores por
/// segundo de `avdec_aac` y `h264parse` intentando decodificar basura, que es
/// lo que dejaba la ventana entera sin responder.
///
/// La 0.3.8 lo rodeaba convirtiendo el blob a `data:` URL, con su coste de
/// memoria y de CPU; la 0.4.2 lo retiró y el vídeo volvió a romperse. Wrusp
/// ataca ahora la causa: al reproducir, reordena el MP4 poniendo `moov`
/// delante de `mdat` y corrige los desplazamientos de trozo (`stco`/`co64`).
/// Con el índice al principio el demuxer no necesita ir al final y el búfer
/// deja de importar. El fichero resultante es el mismo vídeo, byte a byte,
/// solo que ordenado: verificado contra `ffmpeg` (los hashes de fotograma
/// coinciden) y contra GStreamer. Un MP4 de 9,6 MiB se reordena en 4,5 ms.
///
/// Antes de leer nada se recorren solo las cabeceras de nivel superior, de 16
/// bytes: si el vídeo ya venía con `moov` delante —o está fragmentado, o no se
/// entiende— no se toca y no cuesta nada.
///
/// Banco propio, obligatorio antes de tocar esto:
/// `cargo run --example banco_faststart`.
///
/// Solo en Linux: el problema es de WebKitGTK sirviendo blobs, y en los otros
/// motores esto sería trabajo para nada.
#[cfg(target_os = "linux")]
pub fn fix_large_mp4_blobs_script() -> String {
    r#"(function () {
  // ── Remux «faststart» ────────────────────────────────────────────────────
  // Mueve el átomo `moov` delante de `mdat` y corrige los desplazamientos de
  // trozo. Con el índice al principio, el demuxer no necesita ir al final del
  // fichero y el búfer circular de WebKit deja de entregar datos de la
  // posición equivocada. Verificado en `cargo run --example banco_faststart`.

  const CONTENEDORES = new Set([
    'moov', 'trak', 'mdia', 'minf', 'stbl', 'edts', 'udta', 'mvex',
  ]);

  function tipoEn(u8, pos) {
    return String.fromCharCode(u8[pos], u8[pos + 1], u8[pos + 2], u8[pos + 3]);
  }

  // Cajas de un tramo, o null si algo no cuadra: ante la duda no se toca nada.
  function cajas(u8, inicio, fin) {
    const vista = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    const lista = [];
    let pos = inicio;
    while (pos + 8 <= fin) {
      let tam = vista.getUint32(pos);
      const tipo = tipoEn(u8, pos + 4);
      let cabecera = 8;
      if (tam === 1) {
        if (pos + 16 > fin) return null;
        const grande = vista.getBigUint64(pos + 8);
        if (grande > BigInt(Number.MAX_SAFE_INTEGER)) return null;
        tam = Number(grande);
        cabecera = 16;
      } else if (tam === 0) {
        tam = fin - pos;
      }
      if (tam < cabecera || pos + tam > fin) return null;
      lista.push({ tipo, inicio: pos, tam, cabecera });
      pos += tam;
    }
    return pos === fin ? lista : null;
  }

  // Tablas de desplazamientos que cuelgan de `moov`.
  function tablas(u8, inicio, fin, salida) {
    const lista = cajas(u8, inicio, fin);
    if (!lista) return false;
    const vista = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    for (const c of lista) {
      if (c.tipo === 'stco' || c.tipo === 'co64') {
        const base = c.inicio + c.cabecera;
        if (base + 8 > c.inicio + c.tam) return false;
        const cuantos = vista.getUint32(base + 4);
        const ancho = c.tipo === 'stco' ? 4 : 8;
        if (base + 8 + cuantos * ancho > c.inicio + c.tam) return false;
        salida.push({ base: base + 8, cuantos, ancho });
      } else if (CONTENEDORES.has(c.tipo)) {
        if (!tablas(u8, c.inicio + c.cabecera, c.inicio + c.tam, salida)) return false;
      }
    }
    return true;
  }

  // ArrayBuffer con `moov` delante, o null si no hace falta o no se entiende.
  function reordenar(buffer) {
    const u8 = new Uint8Array(buffer);
    const nivel = cajas(u8, 0, u8.length);
    if (!nivel) return null;
    // Fragmentado: los desplazamientos viven en `trun`, con otra base.
    if (nivel.some((c) => c.tipo === 'moof' || c.tipo === 'sidx')) return null;

    const moov = nivel.find((c) => c.tipo === 'moov');
    const primerDato = nivel.find((c) => c.tipo === 'mdat');
    if (!moov || !primerDato || moov.inicio < primerDato.inicio) return null;

    const tabla = [];
    if (!tablas(u8, moov.inicio + moov.cabecera, moov.inicio + moov.tam, tabla)) return null;

    const ftyp = nivel.find((c) => c.tipo === 'ftyp');
    const orden = [];
    if (ftyp) orden.push(ftyp);
    orden.push(moov);
    for (const c of nivel) if (c !== ftyp && c !== moov) orden.push(c);

    let cursor = 0;
    const mapa = orden.map((caja) => {
      const entrada = { caja, nuevoInicio: cursor };
      cursor += caja.tam;
      return entrada;
    });

    const reubicar = (o) => {
      for (const { caja, nuevoInicio } of mapa)
        if (o >= caja.inicio && o < caja.inicio + caja.tam)
          return o - caja.inicio + nuevoInicio;
      return -1; // apunta fuera de toda caja: no nos metemos
    };

    const nuevo = new Uint8Array(cursor);
    for (const { caja, nuevoInicio } of mapa)
      nuevo.set(u8.subarray(caja.inicio, caja.inicio + caja.tam), nuevoInicio);

    const destinoMoov = mapa.find((e) => e.caja === moov).nuevoInicio;
    const vista = new DataView(nuevo.buffer);
    for (const t of tabla) {
      const base = t.base - moov.inicio + destinoMoov;
      for (let i = 0; i < t.cuantos; i++) {
        const pos = base + i * t.ancho;
        const viejo = t.ancho === 4
          ? vista.getUint32(pos)
          : Number(vista.getBigUint64(pos));
        const destino = reubicar(viejo);
        if (destino < 0) return null;
        if (t.ancho === 4) {
          if (destino > 0xffffffff) return null;
          vista.setUint32(pos, destino);
        } else {
          vista.setBigUint64(pos, BigInt(destino));
        }
      }
    }
    return nuevo.buffer;
  }

  // ── Sondeo barato ────────────────────────────────────────────────────────
  // Leer el vídeo entero para descubrir que ya estaba bien sale caro, así que
  // primero se recorren solo las cabeceras de nivel superior, de 16 bytes.

  const MAX_CAJAS = 64;

  // Devuelve '' si hay que reordenar, y si no, el motivo en una palabra: es
  // lo que sale en el registro cuando un vídeo falla sin que el remux tenga
  // nada que hacer, y sin ello no se distingue «índice al final» de «códec
  // que el sistema no tiene» o «fragmentado».
  async function hayQueReordenar(blob) {
    let pos = 0;
    let vistosDatos = false;
    const legible = (t) => t.replace(/[^\x20-\x7e]/g, '?');
    for (let i = 0; i < MAX_CAJAS && pos + 8 <= blob.size; i++) {
      const cabecera = new DataView(await blob.slice(pos, pos + 16).arrayBuffer());
      if (cabecera.byteLength < 8) return 'cabecera corta';
      let tam = cabecera.getUint32(0);
      const tipo = String.fromCharCode(
        cabecera.getUint8(4), cabecera.getUint8(5),
        cabecera.getUint8(6), cabecera.getUint8(7));
      if (i === 0 && !/^[a-z0-9 ]{4}$/i.test(tipo)) return 'no es MP4 (empieza por «' + legible(tipo) + '»)';
      if (tam === 1) {
        if (cabecera.byteLength < 16) return 'cabecera corta';
        const grande = cabecera.getBigUint64(8);
        if (grande > BigInt(Number.MAX_SAFE_INTEGER)) return 'caja gigante';
        tam = Number(grande);
      } else if (tam === 0) {
        tam = blob.size - pos;
      }
      if (tam < 8) return 'caja corrupta (' + legible(tipo) + ')';
      if (tipo === 'moof' || tipo === 'sidx') return 'fragmentado (' + tipo + ')';
      if (tipo === 'mdat') vistosDatos = true;
      if (tipo === 'moov') return vistosDatos ? '' : 'índice ya delante';
      pos += tam;
    }
    return 'sin índice entre las primeras cajas';
  }

  // Códecs declarados en el índice: la marca del `ftyp`, los 4CC de las
  // entradas `stsd` y, para H.264, perfil y nivel del `avcC`. Solo cabeceras,
  // y solo cuando un medio ya ha fallado: es lo que permite comparar en el
  // registro un vídeo que no arranca con los que sí lo hacen en el banco.
  async function codecsDe(blob) {
    try {
      const cuatroDe = (v, i) => String.fromCharCode(v[i], v[i + 1], v[i + 2], v[i + 3]).replace(/[^\x20-\x7e]/g, '?');
      const cabeza = new Uint8Array(await blob.slice(0, 16).arrayBuffer());
      const marca = cabeza.length >= 12 && cuatroDe(cabeza, 4) === 'ftyp' ? 'ftyp ' + cuatroDe(cabeza, 8) : 'sin ftyp';
      let pos = 0, moov = null;
      for (let i = 0; i < MAX_CAJAS && pos + 8 <= blob.size; i++) {
        const c = new DataView(await blob.slice(pos, pos + 16).arrayBuffer());
        if (c.byteLength < 8) break;
        let tam = c.getUint32(0);
        const tipo = String.fromCharCode(c.getUint8(4), c.getUint8(5), c.getUint8(6), c.getUint8(7));
        if (tam === 1) { if (c.byteLength < 16) break; tam = Number(c.getBigUint64(8)); }
        else if (tam === 0) tam = blob.size - pos;
        if (tam < 8) break;
        if (tipo === 'moov') { moov = { pos, tam }; break; }
        pos += tam;
      }
      if (!moov || moov.tam > 8 * 1024 * 1024) return marca + ', sin índice legible';
      const u8 = new Uint8Array(await blob.slice(moov.pos, moov.pos + moov.tam).arrayBuffer());
      const buscar = (texto, desde) => {
        const m = Uint8Array.from(texto, (ch) => ch.charCodeAt(0));
        for (let i = desde; i + 4 <= u8.length; i++)
          if (u8[i] === m[0] && u8[i + 1] === m[1] && u8[i + 2] === m[2] && u8[i + 3] === m[3]) return i;
        return -1;
      };
      const pistas = [];
      let i = 0;
      // `i` apunta al tipo «stsd»: versión y banderas (4), número de entradas
      // (4), tamaño de la primera entrada (4) y su tipo, o sea, i + 16.
      while ((i = buscar('stsd', i)) >= 0 && pistas.length < 6) {
        const entrada = cuatroDe(u8, i + 16);
        let extra = '';
        if (entrada === 'avc1' || entrada === 'avc3') {
          const a = buscar('avcC', i);
          // configurationVersion (a+4), perfil (a+5), compatibilidad (a+6), nivel (a+7)
          if (a > 0 && a + 8 <= u8.length) extra = ' perfil ' + u8[a + 5] + ' nivel ' + u8[a + 7];
        }
        pistas.push(entrada + extra);
        i += 4;
      }
      return marca + ', ' + (pistas.length ? pistas.join(' + ') : 'sin stsd');
    } catch (e) {
      return 'códecs ilegibles';
    }
  }

  // Volcado a disco de los medios que fallan, para analizarlos con
  // `gst-discoverer-1.0` o `ffprobe`. La página retiene los últimos cinco y
  // avisa; es Rust quien decide, con el interruptor de ajustes o la variable
  // de entorno del momento, si los pide por `__wruspLeerFallido`. La 0.4.9
  // fijaba la decisión en un script de arranque evaluado al crear la vista, y
  // activar el interruptor después no servía de nada.
  const fallidosGuardados = new Map(); // id → blob
  let contadorFallidos = 0;
  function ofrecerVolcado(blob) {
    if (!blob) return;
    while (fallidosGuardados.size >= 5) fallidosGuardados.delete(fallidosGuardados.keys().next().value);
    const id = 'f' + (++contadorFallidos);
    fallidosGuardados.set(id, blob);
    if (window.__wruspOrden) window.__wruspOrden('medio-fallido/' + id);
  }
  window.__wruspLeerFallido = async function (id) {
    const blob = fallidosGuardados.get(id);
    fallidosGuardados.delete(id);
    if (!blob || blob.size > 64 * 1024 * 1024) return '';
    const bytes = new Uint8Array(await blob.arrayBuffer());
    let bruto = '';
    for (let i = 0; i < bytes.length; i += 8192)
      bruto += String.fromCharCode.apply(null, bytes.subarray(i, i + 8192));
    return btoa(bruto);
  };

  // ── Puente con la página ─────────────────────────────────────────────────

  const esMedio = /^(video\/|audio\/|application\/mp4|application\/octet-stream)/i;
  // Un blob sin tipo puede ser un vídeo —WhatsApp no siempre lo etiqueta—,
  // pero por debajo de este tamaño cabe entero en el búfer del motor y no hay
  // nada que reordenar: miniaturas, stickers y notas de voz cortas se quedan
  // fuera y no ocupan sitio en el mapa.
  const MIN_SIN_TIPO = 64 * 1024;
  // Copias reordenadas vivas como mucho: son la única memoria que añade Wrusp
  // (el blob original lo retiene la propia URL de WhatsApp hasta que la
  // revoca). Al pasar del tope se revoca la más antigua que ningún
  // reproductor conectado esté usando; si hace falta otra vez, se regenera.
  // La 0.4.4 acotaba el mapa de candidatos entero, y con ello expulsaba
  // vídeos que WhatsApp aún no había reproducido: al pulsarlos ya no eran
  // candidatos y fallaban sin remedio.
  const MAX_ARREGLADAS = 16;

  const crearUrl = URL.createObjectURL;
  const revocarUrl = URL.revokeObjectURL;
  const reproducirNativo = HTMLMediaElement.prototype.play;
  const descriptorMedio = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'src');
  const descriptorFuente = Object.getOwnPropertyDescriptor(HTMLSourceElement.prototype, 'src');
  const descriptorPrecarga = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'preload');
  const ponerAtributo = Element.prototype.setAttribute;

  const candidatos = new Map();     // url del blob → { blob, arreglada, trabajo }
  const porArreglada = new Map();   // url reordenada → url original
  const arregladas = [];            // urls originales con copia viva, de vieja a nueva
  const fallidas = new Set();       // urls que ya reventaron: no se reintentan
  const vigilados = new WeakSet();
  const enObras = new WeakSet();    // medios a la espera de su fuente reordenada
  const reparados = new WeakSet();  // medios a los que ya se les cambió la fuente tras un fallo
  let sinFuenteAnotado = false;     // el aviso de `src=""` sale una vez por vista
  const pausadosPorWrusp = new WeakSet();

  const anotar = (texto) => {
    if (window.__wruspOrden) window.__wruspOrden('log/?m=' + encodeURIComponent(texto));
  };

  const escapar = (url) => (window.CSS && CSS.escape) ? CSS.escape(url) : url.replace(/"/g, '\\"');
  const enUso = (url) => !!url && !!document.querySelector(
    'video[src="' + escapar(url) + '"], audio[src="' + escapar(url) + '"], source[src="' + escapar(url) + '"]');

  function recordarArreglada(url) {
    arregladas.push(url);
    while (arregladas.length > MAX_ARREGLADAS) {
      const i = arregladas.findIndex((u) => {
        const e = candidatos.get(u);
        return !e || !enUso(e.arreglada);
      });
      if (i < 0) break; // todas en uso: se espera
      const [vieja] = arregladas.splice(i, 1);
      const entrada = candidatos.get(vieja);
      if (entrada && entrada.arreglada) {
        porArreglada.delete(entrada.arreglada);
        try { revocarUrl.call(URL, entrada.arreglada); } catch (e) { /* ya no existía */ }
        entrada.arreglada = null;
        entrada.definitiva = null;
        entrada.trabajo = null;
      }
    }
  }

  // ── Visibilidad ──────────────────────────────────────────────────────────
  // Un vídeo con autoplay fuera de la pantalla sigue decodificando; con
  // decenas de GIF en un chat eso satura la CPU. Se pausa al salir del
  // viewport y —esto faltaba en la 0.4.4— se reanuda al volver: si no, cada
  // GIF que se desplazaba fuera quedaba congelado en un fotograma para
  // siempre. Solo se reanuda lo que Wrusp pausó, no lo que paró el usuario.
  let visorInterseccion = null;
  if (typeof IntersectionObserver === 'function') {
    visorInterseccion = new IntersectionObserver((entradas) => {
      for (const entrada of entradas) {
        const medio = entrada.target;
        if (!(medio instanceof HTMLVideoElement)) continue;
        if (!medio.isConnected) {
          visorInterseccion.unobserve(medio);
          pausadosPorWrusp.delete(medio);
          continue;
        }
        if (!entrada.isIntersecting) {
          if (medio.autoplay && !medio.paused) {
            pausadosPorWrusp.add(medio);
            try { medio.pause(); } catch (e) { /* el medio ya no está */ }
          }
        } else if (pausadosPorWrusp.has(medio)) {
          pausadosPorWrusp.delete(medio);
          if (medio.paused && !medio.ended) {
            try {
              const p = medio.play();
              if (p && p.catch) p.catch(() => {});
            } catch (e) { /* sin gesto de usuario no siempre deja */ }
          }
        }
      }
    }, { rootMargin: '50px' });
  }

  // Solo los vídeos con autoplay, que son los únicos que se pausan. Observar
  // todos los demás no aportaba nada y tenía un precio medido en el banco:
  // un <video> sin tamaño propio observado justo al llamar a play() se
  // quedaba cargando sin llegar a tener metadatos (readyState 0, stalled).
  function vigilarVisibilidad(medio) {
    if (visorInterseccion && medio instanceof HTMLVideoElement && medio.autoplay) {
      visorInterseccion.observe(medio);
    }
  }

  function medioDe(nodo) {
    if (nodo instanceof HTMLMediaElement) return nodo;
    if (nodo instanceof HTMLSourceElement && nodo.parentElement instanceof HTMLMediaElement)
      return nodo.parentElement;
    return null;
  }

  function urlDe(nodo) {
    if (nodo instanceof HTMLMediaElement && descriptorMedio && descriptorMedio.get)
      return descriptorMedio.get.call(nodo);
    if (nodo instanceof HTMLSourceElement && descriptorFuente && descriptorFuente.get)
      return descriptorFuente.get.call(nodo);
    return nodo.getAttribute('src') || '';
  }

  function ponerUrl(nodo, url) {
    if (nodo instanceof HTMLMediaElement && descriptorMedio && descriptorMedio.set)
      descriptorMedio.set.call(nodo, url);
    else if (nodo instanceof HTMLSourceElement && descriptorFuente && descriptorFuente.set)
      descriptorFuente.set.call(nodo, url);
    else
      ponerAtributo.call(nodo, 'src', url);
  }

  // WebKitGTK levanta un pipeline de GStreamer por cada medio del documento
  // aunque nadie lo reproduzca: un chat con decenas de vídeos y notas de voz
  // llegaba a saturar la CPU. La precarga se devuelve al reproducir.
  function aplazarPrecarga(medio) {
    if (!descriptorPrecarga || !descriptorPrecarga.set || !descriptorPrecarga.get) return;
    if (medio.autoplay || !medio.paused) return;
    if (descriptorPrecarga.get.call(medio) !== 'none')
      descriptorPrecarga.set.call(medio, 'none');
  }

  // El nodo que lleva la URL puede ser el propio medio o un <source> suyo.
  function portador(medio) {
    if (candidatos.has(String(urlDe(medio) || ''))) return medio;
    for (const fuente of medio.querySelectorAll('source[src]'))
      if (candidatos.has(String(urlDe(fuente) || ''))) return fuente;
    return null;
  }

  // Desmonta el pipeline de un medio que no se puede reproducir. Sin esto el
  // motor y WhatsApp se quedan reintentando, y cada intento vuelve a llenar
  // el registro con miles de errores por segundo: eso es lo que dejaba la
  // ventana sin responder (0.4.3). Se quita la fuente también de los <source>
  // hijos: si no, `load()` la vuelve a elegir y el bucle sigue.
  function detener(medio, url, mensaje) {
    if (url) fallidas.add(url);
    try {
      medio.pause();
      medio.removeAttribute('src');
      for (const fuente of medio.querySelectorAll('source[src]')) fuente.removeAttribute('src');
      medio.load();
    } catch (e) { /* el medio ya no está */ }
    anotar(mensaje);
  }

  // Códigos de MediaError: 1 abortado, 2 red, 3 decodificación, 4 fuente no
  // admitida. El 4 es justo el que da GStreamer cuando no digiere el MP4 con
  // el índice al final; la 0.4.4 lo trataba como transitorio y dejaba a
  // WhatsApp reintentando sin fin, que es lo que volvía a clavar el chat.
  //
  // Si la fuente es un blob candidato que aún no se ha reordenado, el fallo
  // se repara aquí mismo: es el camino de los vídeos con autoplay (GIF y
  // previsualizaciones), que cargan al recibir la fuente y fallan antes de
  // que nadie llame a play(). Si ya estaba reordenada, o no era candidata,
  // no hay más que hacer y se detiene.
  // El nodo que lleva exactamente esta URL: el medio o un <source> suyo.
  function portadorDe(medio, url) {
    if (String(urlDe(medio) || '') === url) return medio;
    for (const fuente of medio.querySelectorAll('source[src]'))
      if (String(urlDe(fuente) || '') === url) return fuente;
    return null;
  }

  // Intenta enderezar el blob de un medio que acaba de fallar y lo vuelve a
  // arrancar; si no hay nada que enderezar, lo detiene.
  function reparar(medio, nodo, url, entrada, seguir, detalle) {
    enObras.add(medio);
    return preparar(url)
      .then((definitiva) => {
        if (definitiva === url) {
          enObras.delete(medio);
          detener(medio, url, 'el blob no se puede reordenar (' + (entrada.motivo || '?') + ')' + detalle + ': pipeline detenido');
          describirFallido(entrada);
          return;
        }
        anotar('medio con fallo' + detalle + ': se reordena y se reintenta');
        return usar(medio, nodo, definitiva).then(() => {
          enObras.delete(medio);
          if (seguir && medio.isConnected) return reproducirNativo.call(medio).catch(() => {});
        });
      })
      .catch(() => enObras.delete(medio));
  }

  function vigilarFallo(medio) {
    if (vigilados.has(medio)) return;
    vigilados.add(medio);
    medio.addEventListener('error', () => {
      // Mientras se reordena, el elemento aún lleva la fuente vieja: un fallo
      // aquí lo arregla el cambio de fuente que viene detrás.
      if (enObras.has(medio)) return;
      const codigo = medio.error ? medio.error.code : 0;
      // `src=""`: WhatsApp deja así los vídeos que aún no ha descargado, y el
      // motor, por especificación, falla con código 4 al resolver la cadena
      // vacía a la propia página. No hay nada que reparar ni que sondear:
      // el registro de la 0.4.8 mostró 71 de estos, cada uno con un sondeo
      // que se traía 600 KiB de HTML.
      const atributo = medio.getAttribute('src');
      if (atributo !== null && atributo.trim() === '' && !medio.querySelector('source[src]')) {
        if (!sinFuenteAnotado) {
          sinFuenteAnotado = true;
          anotar('medio con src vacío (código ' + codigo + '): se ignora, y los siguientes no se anotan');
        }
        return;
      }
      const nodo = portador(medio);
      const url = String(urlDe(nodo || medio) || '');
      const entrada = nodo ? candidatos.get(url) : candidatos.get(porArreglada.get(url));
      // Desmontar el pipeline borra `error`, y sin este rastro no hay forma de
      // saber después qué pasó (el banco lo lee de aquí).
      window.__wruspUltimoFallo = { codigo, url };
      let esquema = url.slice(0, url.indexOf(':') + 1) || 'sin src';
      if (esquema === 'https:' || esquema === 'http:') {
        try { const u = new URL(url); esquema = u.origin + u.pathname.slice(0, 48); } catch (e) { /* se queda el esquema */ }
      }
      const detalle = ' (código ' + codigo + ', red ' + medio.networkState + ', datos ' + medio.readyState
        + ', ' + (!entrada ? 'sin blob candidato, ' + esquema : nodo ? 'blob candidato' : 'blob ya reordenado')
        + (entrada && entrada.blob ? ', ' + Math.round(entrada.blob.size / 1024) + ' KiB ' + (entrada.blob.type || 'sin tipo') : '')
        + (entrada && entrada.motivo ? ', ' + entrada.motivo : '')
        + ')';
      if (codigo === 1 || codigo === 2) {
        anotar('medio con aviso de transporte o red' + detalle);
        return;
      }
      const seguir = medio.autoplay || !medio.paused;
      // Candidato aún sin preparar: se intenta enderezar aquí mismo. Es el
      // camino de los vídeos con autoplay, que cargan al recibir la fuente y
      // fallan antes de que nadie llame a play().
      if (nodo && entrada && !entrada.definitiva && !reparados.has(medio)) {
        reparados.add(medio);
        reparar(medio, nodo, url, entrada, seguir, detalle);
        return;
      }
      // Fuente remota: se sondea el principio con una petición de rango, que
      // es lo mismo que hace el motor, y se anota qué contesta y qué códecs
      // trae. Si la sirve el service worker de WhatsApp, aquí se ve.
      if (!entrada && /^https?:/.test(url) && url !== location.href && !reparados.has(medio)) {
        reparados.add(medio);
        detener(medio, url, 'medio con fallo de decodificación' + detalle + ': pipeline detenido');
        fetch(url, { headers: { Range: 'bytes=0-262143' } }).then(async (r) => {
          const tipo = r.headers.get('content-type') || 'sin content-type';
          const rango = r.headers.get('content-range') || 'sin content-range';
          const blob = await r.blob();
          const motivo = await hayQueReordenar(blob);
          const codecs = await codecsDe(blob);
          anotar('sondeo de la fuente remota: HTTP ' + r.status + ', ' + tipo + ', ' + rango + ', ' + Math.round(blob.size / 1024) + ' KiB leídos, ' + (motivo || 'índice al final') + ', ' + codecs);
          ofrecerVolcado(blob);
        }).catch((e) => anotar('sondeo de la fuente remota: no se pudo leer (' + ((e && e.message) || e) + ')'));
        return;
      }
      // Un blob que no pasó por nuestro `createObjectURL` (WhatsApp descifra
      // parte de los medios en workers, y allí no llegamos): se lee con
      // `fetch` y entra por el mismo camino. Si la lectura falla, era una
      // MediaSource u otra cosa, y no hay nada que hacer salvo detenerlo.
      if (!entrada && url.indexOf('blob:') === 0 && !reparados.has(medio)) {
        reparados.add(medio);
        enObras.add(medio);
        fetch(url).then((r) => r.blob()).then((blob) => {
          const nueva = { blob, arreglada: null, definitiva: null, trabajo: null };
          candidatos.set(url, nueva);
          enObras.delete(medio);
          const portadorReal = portadorDe(medio, url) || medio;
          return reparar(medio, portadorReal, url, nueva, seguir,
            detalle.replace('sin blob candidato, blob:', 'blob leído a posteriori, ' + Math.round(blob.size / 1024) + ' KiB ' + (blob.type || 'sin tipo')));
        }).catch(() => {
          enObras.delete(medio);
          detener(medio, url, 'medio con fallo' + detalle + ', el blob no se deja leer (¿MediaSource?): pipeline detenido');
        });
        return;
      }
      detener(medio, url, 'medio con fallo de decodificación' + detalle + ': pipeline detenido');
      if (entrada && entrada.blob) describirFallido(entrada);
    }, true);
  }

  // Tras detener: códecs al registro y, si está activado, volcado a disco.
  function describirFallido(entrada) {
    if (!entrada || !entrada.blob || entrada.descrito) return;
    entrada.descrito = true;
    codecsDe(entrada.blob).then((codecs) => {
      anotar('códecs del medio detenido: ' + codecs + ' (' + Math.round(entrada.blob.size / 1024) + ' KiB ' + (entrada.blob.type || 'sin tipo') + ')');
    });
    ofrecerVolcado(entrada.blob);
  }

  // Antes de que la fuente llegue al elemento: si es uno de nuestros blobs,
  // se aplaza su precarga para que el motor no arranque nada todavía.
  // Cualquier fuente remota o de blob, no solo los candidatos: el registro de
  // la 0.4.7 enseñó que WhatsApp también pone vídeos con fuente `https:`
  // (servidos por su service worker) y cada uno levantaba su pipeline nada
  // más aparecer en el chat.
  const conFuente = (valor) => /^(blob:|https?:)/.test(String(valor || ''));

  function prepararNodo(nodo, valor) {
    if (!conFuente(valor)) return;
    const medio = medioDe(nodo);
    if (medio) aplazarPrecarga(medio);
  }

  function registrar(nodo) {
    const medio = medioDe(nodo);
    if (!medio) return;
    vigilarFallo(medio);
    vigilarVisibilidad(medio);
    if (conFuente(urlDe(nodo))) aplazarPrecarga(medio);
  }

  // Deja lista la URL definitiva del blob: la arreglada si hacía falta
  // reordenar, o la original. Se hace una sola vez por blob.
  // Deja lista la URL definitiva del blob: la arreglada si hacía falta
  // reordenar, o la original. Se hace una sola vez por blob, y el resultado
  // queda en `entrada.definitiva` **también cuando no hay nada que
  // reordenar**. Sin eso, el manejador del evento `play` de más abajo veía un
  // candidato «sin arreglar», lo pausaba, lo preparaba y lo volvía a arrancar,
  // y ese arranque disparaba otro `play`: un bucle infinito de pausa y
  // reproducción por cada GIF cuyo MP4 ya venía bien. Hasta la 0.4.5 el tope
  // de 35 candidatos los expulsaba antes de que se notara; la 0.4.6 quitó el
  // tope y el bucle se llevó por delante cualquier chat con GIF (medido: 33 s
  // de CPU en una sesión de 30 s).
  function preparar(url) {
    const entrada = candidatos.get(url);
    if (!entrada) return Promise.resolve(url);
    if (entrada.definitiva) return Promise.resolve(entrada.definitiva);
    if (entrada.trabajo) return entrada.trabajo;
    entrada.trabajo = (async () => {
      try {
        const motivo = await hayQueReordenar(entrada.blob);
        if (motivo) {
          entrada.motivo = motivo;
          entrada.definitiva = url;
          return url;
        }
        const bytes = await entrada.blob.arrayBuffer();
        const arreglado = reordenar(bytes);
        if (!arreglado) {
          anotar('vídeo con el índice al final que no se ha podido reordenar');
          entrada.motivo = 'no reordenable';
          entrada.definitiva = url;
          return url;
        }
        const tipo = entrada.blob.type || 'video/mp4';
        entrada.arreglada = crearUrl.call(URL, new Blob([arreglado], { type: tipo }));
        entrada.definitiva = entrada.arreglada;
        porArreglada.set(entrada.arreglada, url);
        recordarArreglada(url);
        anotar('vídeo reordenado (' + Math.round(arreglado.byteLength / 1024) + ' KiB)');
        return entrada.arreglada;
      } catch (e) {
        entrada.motivo = 'excepción al leerlo';
        entrada.definitiva = url; // ante cualquier sorpresa, el blob original
        return url;
      }
    })();
    return entrada.trabajo;
  }

  // Metadatos de la fuente actual, o `false` si llega un error o se agota el
  // plazo. Para un blob local llegan en milisegundos.
  function esperarMetadatos(medio, ms) {
    if (medio.readyState >= HTMLMediaElement.HAVE_METADATA) return Promise.resolve(true);
    return new Promise((listo) => {
      let temporizador = 0;
      const fin = (ok) => {
        clearTimeout(temporizador);
        medio.removeEventListener('loadedmetadata', bien);
        medio.removeEventListener('error', mal);
        listo(ok);
      };
      const bien = () => fin(true);
      const mal = () => fin(false);
      temporizador = setTimeout(() => fin(false), ms);
      medio.addEventListener('loadedmetadata', bien);
      medio.addEventListener('error', mal);
    });
  }

  // Sustituye la fuente conservando posición y estado, como haría una recarga
  // normal del propio medio, y no devuelve el control hasta que la fuente
  // nueva tiene metadatos.
  //
  // El orden importa: primero la fuente nueva y después la precarga. Al revés,
  // devolver la precarga con la fuente vieja todavía puesta arrancaba la
  // carga del blob original, y abortarla un instante después con `load()`
  // dejaba a veces la fuente nueva cargando sin llegar a tener metadatos
  // (medido en el banco: readyState 0, «stalled», la promesa de play() sin
  // resolverse nunca; pasaba también con el script de la 0.4.3). Por si el
  // motor se atasca igualmente, se espera a los metadatos y, si no llegan, se
  // vuelve a cargar una vez.
  function usar(medio, nodo, urlNueva) {
    if (urlDe(nodo) === urlNueva) return Promise.resolve();
    const posicion = Number.isFinite(medio.currentTime) ? medio.currentTime : 0;
    ponerUrl(nodo, urlNueva);
    if (descriptorPrecarga && descriptorPrecarga.set)
      descriptorPrecarga.set.call(medio, 'auto');
    medio.load();
    return esperarMetadatos(medio, 2500)
      .then((ok) => {
        if (ok || medio.readyState > 0 || urlDe(nodo) !== urlNueva || medio.error) return ok;
        anotar('la fuente reordenada no arranca (red ' + medio.networkState + '): se vuelve a cargar');
        medio.load();
        return esperarMetadatos(medio, 4000);
      })
      .then(() => {
        if (posicion <= 0) return;
        try {
          if (Number.isFinite(medio.duration))
            medio.currentTime = Math.min(posicion, Math.max(0, medio.duration - 0.001));
        } catch (e) { /* el medio cambió por debajo */ }
      });
  }

  HTMLMediaElement.prototype.play = function () {
    const medio = this;
    vigilarFallo(medio);
    vigilarVisibilidad(medio);
    const nodo = portador(medio);
    if (!nodo) {
      if (fallidas.has(String(urlDe(medio) || '')))
        return Promise.reject(new DOMException('medio descartado tras fallar', 'AbortError'));
      if (descriptorPrecarga && descriptorPrecarga.set)
        descriptorPrecarga.set.call(medio, 'auto');
      return reproducirNativo.call(medio);
    }
    // Con un blob candidato la precarga se devuelve en `usar`, ya con la
    // fuente definitiva puesta: arrancar aquí la carga del original para
    // abortarla enseguida es lo que dejaba la fuente nueva atascada.
    const url = String(urlDe(nodo));
    enObras.add(medio);
    return preparar(url)
      .then((definitiva) => usar(medio, nodo, definitiva))
      .then(() => { enObras.delete(medio); return reproducirNativo.call(medio); })
      .catch((e) => { enObras.delete(medio); throw e; });
  };

  // Un `autoplay` no pasa por `play()`: el motor arranca solo. Se detiene, se
  // prepara la fuente y se vuelve a arrancar.
  document.addEventListener('play', function (evento) {
    const medio = evento.target;
    if (!(medio instanceof HTMLMediaElement)) return;
    vigilarVisibilidad(medio);
    const nodo = portador(medio);
    if (!nodo) return;
    const url = String(urlDe(nodo));
    const entrada = candidatos.get(url);
    // Ya preparado (reordenado o sin nada que reordenar): no se toca. Ver
    // `preparar` para el bucle que había aquí.
    if (!entrada || entrada.definitiva) return;
    medio.pause();
    enObras.add(medio);
    preparar(url)
      .then((definitiva) => usar(medio, nodo, definitiva))
      .then(() => {
        enObras.delete(medio);
        if (medio.isConnected) reproducirNativo.call(medio).catch(() => {});
      })
      .catch(() => enObras.delete(medio));
  }, true);

  URL.createObjectURL = function (objeto) {
    const url = crearUrl.call(this, objeto);
    if (objeto instanceof Blob) {
      const candidato = objeto.type ? esMedio.test(objeto.type) : objeto.size >= MIN_SIN_TIPO;
      if (candidato) candidatos.set(url, { blob: objeto, arreglada: null, definitiva: null, trabajo: null });
    }
    return url;
  };

  URL.revokeObjectURL = function (url) {
    const clave = String(url);
    const entrada = candidatos.get(clave);
    if (entrada) {
      // La copia reordenada solo la conoce Wrusp: si nadie revoca la original,
      // nadie la revocaría nunca.
      if (entrada.arreglada) {
        porArreglada.delete(entrada.arreglada);
        revocarUrl.call(this, entrada.arreglada);
      }
      const i = arregladas.indexOf(clave);
      if (i >= 0) arregladas.splice(i, 1);
      candidatos.delete(clave);
      fallidas.delete(clave);
    }
    return revocarUrl.call(this, url);
  };

  // Tres vías para poner la fuente, y hay que estar en las tres: el hueco de
  // `setAttribute` dejaba pasar vídeos sin aplazar su precarga.
  function envolverSrc(prototipo, descriptor) {
    if (!descriptor || !descriptor.get || !descriptor.set || !descriptor.configurable) return;
    Object.defineProperty(prototipo, 'src', {
      configurable: descriptor.configurable,
      enumerable: descriptor.enumerable,
      get: descriptor.get,
      set(valor) {
        // El orden importa: asignar la fuente arranca la carga en el acto, y
        // aplazar la precarga después ya no la cancela.
        prepararNodo(this, valor);
        descriptor.set.call(this, valor);
        registrar(this);
      },
    });
  }
  envolverSrc(HTMLMediaElement.prototype, descriptorMedio);
  envolverSrc(HTMLSourceElement.prototype, descriptorFuente);

  Element.prototype.setAttribute = function (nombre, valor) {
    const esFuente = String(nombre).toLowerCase() === 'src'
      && (this instanceof HTMLMediaElement || this instanceof HTMLSourceElement);
    if (esFuente) prepararNodo(this, valor);
    ponerAtributo.call(this, nombre, valor);
    if (esFuente) registrar(this);
  };

  // Y los que llegan al documento con la fuente ya puesta; los que se van,
  // se dejan de vigilar.
  function registrarArbol(raiz) {
    if (!raiz || raiz.nodeType !== Node.ELEMENT_NODE) return;
    if (raiz.matches('video[src], audio[src], source[src]')) registrar(raiz);
    for (const nodo of raiz.querySelectorAll('video[src], audio[src], source[src]'))
      registrar(nodo);
  }
  function olvidarArbol(raiz) {
    if (!visorInterseccion || !raiz || raiz.nodeType !== Node.ELEMENT_NODE) return;
    if (raiz instanceof HTMLVideoElement) visorInterseccion.unobserve(raiz);
    for (const nodo of raiz.querySelectorAll('video')) visorInterseccion.unobserve(nodo);
  }

  new MutationObserver((mutaciones) => {
    for (const mutacion of mutaciones) {
      for (const nodo of mutacion.addedNodes) registrarArbol(nodo);
      for (const nodo of mutacion.removedNodes) olvidarArbol(nodo);
    }
  }).observe(document, { childList: true, subtree: true });
})();"#
        .to_string()
}

/// Fuera de Linux el motor sirve los blobs de vídeo por su cuenta.
#[cfg(not(target_os = "linux"))]
pub fn fix_large_mp4_blobs_script() -> String {
    String::new()
}

/// Completa el disfraz de Chrome que el user-agent empieza.
///
/// El user-agent por sí solo no basta: WhatsApp mira además el objeto
/// `navigator` y algunas señales que solo tiene Chrome. Con `vendor` de Apple
/// y sin `window.chrome`, concluye que estás en Safari —o sea, en un Mac— y
/// ofrece «WhatsApp para Mac» en la bienvenida, junto al código QR y en una
/// ventana emergente.
///
/// Además, en Linux las fuentes de emojis COLRv1 vectoriales no están totalmente
/// soportadas por Cairo/WebKitGTK. Al disfrazar la plataforma como Windows,
/// WhatsApp Web entrega su conjunto completo de imágenes/sprites de emojis
/// (estilo Apple), asegurando que el 100% de los emojis se vean sin huecos en blanco.
pub fn disguise_script() -> String {
    format!(
        r#"(function () {{
  const define = (obj, prop, value) => {{
    try {{
      Object.defineProperty(obj, prop, {{ get: () => value, configurable: true }});
    }} catch (e) {{ /* si la propiedad no es redefinible, se deja como está */ }}
  }};
  const enNavigator = (prop, value) => define(navigator, prop, value);

  // El motivo de todo esto: WebKit responde "Apple Computer, Inc.".
  enNavigator('vendor', 'Google Inc.');

  const brands = [
    {{ brand: 'Not_A Brand', version: '24' }},
    {{ brand: 'Chromium', version: '{v}' }},
    {{ brand: 'Google Chrome', version: '{v}' }},
  ];
  // Windows garantiza el juego completo de emojis de WhatsApp y atajos con Ctrl
  const platform = 'Windows';
  enNavigator('platform', 'Win32');

  // WebKit no implementa userAgentData; sin él, algunos detectores descartan
  // Chrome pese al user-agent.
  if (!navigator.userAgentData) {{
    enNavigator('userAgentData', {{
      brands,
      mobile: false,
      platform,
      getHighEntropyValues: async () => ({{
        architecture: 'x86',
        bitness: '64',
        brands,
        fullVersionList: brands,
        mobile: false,
        model: '',
        platform,
        platformVersion: '10.0.0',
        uaFullVersion: '{v}.0.0.0',
      }}),
      toJSON: () => ({{ brands, mobile: false, platform }}),
    }});
  }}

  // `!!window.chrome` es la comprobación más extendida para separar Chrome de
  // Safari, y es justo la que nos delataba: WebKitGTK no lo tiene.
  if (!window.chrome) {{
    const inicio = Date.now();
    window.chrome = {{
      runtime: {{}},
      app: {{ isInstalled: false }},
      csi: () => ({{ startE: inicio, onloadT: inicio, pageT: Date.now() - inicio, tran: 15 }}),
      loadTimes: () => ({{
        commitLoadTime: inicio / 1000,
        finishDocumentLoadTime: inicio / 1000,
        firstPaintTime: inicio / 1000,
        navigationType: 'Other',
        wasFetchedViaSpdy: false,
        wasNpnNegotiated: true,
        npnNegotiatedProtocol: 'h2',
      }}),
    }};
  }}

  // Chrome expone siempre estos complementos del visor de PDF; en WebKitGTK la
  // lista viene vacía, y una lista vacía es señal de Safari o de un navegador
  // automatizado.
  if (!navigator.plugins || navigator.plugins.length === 0) {{
    const NOMBRES = [
      ['PDF Viewer', 'internal-pdf-viewer'],
      ['Chrome PDF Viewer', 'internal-pdf-viewer'],
      ['Chromium PDF Viewer', 'internal-pdf-viewer'],
      ['Microsoft Edge PDF Viewer', 'internal-pdf-viewer'],
      ['WebKit built-in PDF', 'internal-pdf-viewer'],
    ];
    const tipos = ['application/pdf', 'text/pdf'].map((type) => ({{
      type, suffixes: 'pdf', description: 'Portable Document Format',
    }}));
    const lista = NOMBRES.map(([name, filename]) => ({{
      name, filename, description: 'Portable Document Format',
      length: tipos.length, item: (i) => tipos[i] || null, namedItem: (t) => tipos.find((x) => x.type === t) || null,
    }}));
    lista.item = (i) => lista[i] || null;
    lista.namedItem = (n) => lista.find((p) => p.name === n) || null;
    lista.refresh = () => {{}};
    enNavigator('plugins', lista);
    enNavigator('mimeTypes', Object.assign(tipos.slice(), {{
      item: (i) => tipos[i] || null,
      namedItem: (t) => tipos.find((x) => x.type === t) || null,
    }}));
  }}

  // Detalles sueltos que Chrome tiene y WebKitGTK no. Solo se rellenan si
  // faltan: si el motor los añade algún día, manda el motor.
  if (navigator.pdfViewerEnabled === undefined) enNavigator('pdfViewerEnabled', true);
  if (navigator.deviceMemory === undefined) enNavigator('deviceMemory', 8);
  if (navigator.maxTouchPoints === undefined) enNavigator('maxTouchPoints', 0);

  // Rastros que solo deja Safari: verlos basta para descartar Chrome.
  try {{ delete window.safari; }} catch (e) {{ /* no siempre es borrable */ }}
  if ('standalone' in navigator) enNavigator('standalone', undefined);
}})();"#,
        v = CHROME_VERSION
    )
}
