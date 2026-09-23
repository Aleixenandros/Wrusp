//! Disfraz de navegador para WhatsApp Web.
//!
//! El motor es WebKitGTK, o sea el mismo que Safari, y WhatsApp lo detecta.
//! Lo que ve la página **depende del dominio**, y eso engañó a la primera
//! medición (ADR-006, corregida en ADR-044). WebKitGTK trae «apaños por sitio»
//! (`enable-site-specific-quirks`, activos por defecto) y `whatsapp.com` está
//! en la lista de dominios a los que presenta el user-agent de Safari **en
//! Mac**, por encima del que fije la aplicación. Medido con WebKitGTK 2.52.5 y
//! el user-agent de Chrome que pone `shell.rs`:
//!
//! ```text
//! http://localhost/          (X11; Linux x86_64) … Chrome/131.0.0.0 Safari/537.36
//! https://web.whatsapp.com/  (Macintosh; Intel Mac OS X 10_15) … Version/60.5 Safari/605.1.15
//! vendor        : Apple Computer, Inc.   (en los dos)
//! userAgentData : null                   (en los dos)
//! ```
//!
//! Un servidor local (`WRUSP_TEST_URL`) no reproduce ese apaño. Para medir lo
//! que ve WhatsApp hay que cargar la página con su dirección base, que es lo
//! que hace `cargo run --example banco_disfraz`.
//!
//! WhatsApp decide el sistema operativo leyendo esa cadena con `ua-parser-js`
//! (módulo `WAWebUA` de su código público). Con «Mac OS» anuncia «WhatsApp
//! para Mac», espera la tecla Comando en sus atajos y pinta los emojis con la
//! fuente del sistema. El script de disfraz se ejecuta antes que el código de
//! la página: corrige la plataforma de esa cadena y completa el resto.

/// Versión de Chrome que decimos ser. Debe cuadrar con `CHROME_UA`.
const CHROME_VERSION: &str = "131";

/// Oculta la promoción de la app nativa («Descarga WhatsApp para Mac»).
///
/// Desde el ADR-044 el anuncio se corta en origen: con la plataforma del
/// user-agent corregida (`UA_PLATFORM_FIX`), WhatsApp no anuncia su app de
/// escritorio en Linux. Este script queda como red de seguridad, por si alguna
/// variante no pasa por esa comprobación o el motor cambia su apaño. Ojo: el
/// anuncio nuevo de septiembre de 2026 abre la tienda con un botón y no con un
/// enlace, así que la búsqueda por enlace ya no lo ve.
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
/// Desde la 0.4.12 el vídeo llega al motor como `data:` URL (leída con
/// `FileReader`, con tope de tamaño y pocas copias vivas): con vídeos reales
/// volcados de un chat, el cargador de blobs de WebKitGTK 2.52 unas veces los
/// rechazaba sin leer un byte, otras rompía al mover la barra y otras colgaba
/// el proceso web; como `data:` arrancan y saltan (ADR-039). El remux de más
/// abajo se conserva para los que además traen el índice al final.
///
/// Desde la 0.4.15 la fuente de un elemento se fija **una sola vez** (ADR-043):
/// Wrusp retiene el blob que asigna WhatsApp y entrega la `data:` al elemento
/// cuando se va a reproducir, en vez de cambiarle la fuente a un reproductor
/// que ya existe. Ese cambio destruye el pipeline de GStreamer en el hilo
/// principal, y sobre un reproductor que acababa de fallar dejó WhatsApp
/// congelado más de dos minutos esperando un cerrojo.
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

  // Cotas frente a un fichero hostil. Todo lo que se lee aquí son números que
  // vienen del vídeo que alguien ha enviado, y de ellos se reserva memoria:
  // sin tope, una cabecera que declare millones de cajas cuesta más en
  // metadatos que el fichero entero, y una tabla de trozos que declare cuatro
  // mil millones de entradas se come la pestaña antes de que nadie mire si el
  // tamaño cuadra. La idea es de oxidezap (MIT), que la aprendió del mismo
  // sitio: un `stsz` puede declarar un tamaño fijo de un byte y con eso
  // nombrar decenas de millones de muestras dentro de un fichero pequeño.
  //
  // Los números están muy por encima de cualquier vídeo real: un MP4 de
  // WhatsApp tiene unas pocas docenas de cajas por nivel y unos miles de
  // trozos. Lo que compran es que el trabajo quede acotado dijera lo que
  // dijera el fichero.
  const MAX_CAJAS_POR_TRAMO = 100000;
  const MAX_PROFUNDIDAD = 16;
  const MAX_DESPLAZAMIENTOS = 1000000;

  function tipoEn(u8, pos) {
    return String.fromCharCode(u8[pos], u8[pos + 1], u8[pos + 2], u8[pos + 3]);
  }

  // Cajas de un tramo, o null si algo no cuadra: ante la duda no se toca nada.
  function cajas(u8, inicio, fin) {
    const vista = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    const lista = [];
    let pos = inicio;
    while (pos + 8 <= fin) {
      // Una caja mide ocho bytes como poco, así que un tramo grande lleno de
      // cajas mínimas declara millones de entradas y cada una es un objeto.
      if (lista.length >= MAX_CAJAS_POR_TRAMO) return null;
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
  function tablas(u8, inicio, fin, salida, profundidad) {
    // Un contenedor puede anidar contenedores, y nada en el fichero impide
    // que lo haga mil veces: sin este freno la recursión revienta la pila.
    if (profundidad > MAX_PROFUNDIDAD) return false;
    const lista = cajas(u8, inicio, fin);
    if (!lista) return false;
    const vista = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    for (const c of lista) {
      if (c.tipo === 'stco' || c.tipo === 'co64') {
        const base = c.inicio + c.cabecera;
        if (base + 8 > c.inicio + c.tam) return false;
        const cuantos = vista.getUint32(base + 4);
        const ancho = c.tipo === 'stco' ? 4 : 8;
        // El tope se comprueba **antes** de multiplicar: `cuantos` es de 32
        // bits y el producto llega a treinta y cuatro gigabytes.
        if (cuantos > MAX_DESPLAZAMIENTOS) return false;
        if (base + 8 + cuantos * ancho > c.inicio + c.tam) return false;
        salida.total += cuantos;
        if (salida.total > MAX_DESPLAZAMIENTOS) return false;
        salida.lista.push({ base: base + 8, cuantos, ancho });
      } else if (CONTENEDORES.has(c.tipo)) {
        if (!tablas(u8, c.inicio + c.cabecera, c.inicio + c.tam, salida, profundidad + 1))
          return false;
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

    const tabla = { lista: [], total: 0 };
    if (!tablas(u8, moov.inicio + moov.cabecera, moov.inicio + moov.tam, tabla, 0)) return null;

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

    // Búsqueda binaria sobre las cajas ordenadas por su posición original.
    // `reubicar` se llama una vez por trozo, y recorrer la lista entera en
    // cada llamada convierte un fichero con muchas cajas y muchos trozos en
    // trabajo cuadrático dentro del hilo de la página.
    const porInicio = mapa.slice().sort((a, b) => a.caja.inicio - b.caja.inicio);
    const reubicar = (o) => {
      let bajo = 0;
      let alto = porInicio.length - 1;
      while (bajo <= alto) {
        const medio = (bajo + alto) >> 1;
        const { caja, nuevoInicio } = porInicio[medio];
        if (o < caja.inicio) alto = medio - 1;
        else if (o >= caja.inicio + caja.tam) bajo = medio + 1;
        else return o - caja.inicio + nuevoInicio;
      }
      return -1; // apunta fuera de toda caja: no nos metemos
    };

    const nuevo = new Uint8Array(cursor);
    for (const { caja, nuevoInicio } of mapa)
      nuevo.set(u8.subarray(caja.inicio, caja.inicio + caja.tam), nuevoInicio);

    const destinoMoov = mapa.find((e) => e.caja === moov).nuevoInicio;
    const vista = new DataView(nuevo.buffer);
    for (const t of tabla.lista) {
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
  //
  // La regla, desde la 0.4.15 (ADR-043): **la fuente de un elemento que ya
  // tiene reproductor no se cambia nunca.** Cambiarla, o quitarla, destruye su
  // pipeline de GStreamer en el hilo principal, y en uso real, sobre un
  // reproductor que acababa de fallar, esa destrucción se quedó esperando el
  // cerrojo de un pad que tenía cogido un hilo de GStreamer, parado a su vez a
  // la espera del preroll: WhatsApp congelado más de dos minutos, capturado con
  // `eu-stack` en el volcado del proceso.
  //
  // Así que Wrusp ya no repara nada después: decide la fuente antes. Cuando
  // WhatsApp asigna un blob de vídeo a un elemento sin fuente real, Wrusp se lo
  // guarda y le entrega la definitiva una sola vez —la `data:` que el motor sí
  // sabe leer (ADR-039), reordenada si hacía falta—: enseguida si se reproduce
  // solo (los GIF) o cuando alguien lo pulsa. Hasta entonces el elemento no
  // tiene reproductor que destruir, no abre su conexión al bus de sesión
  // (ADR-042) y no ocupa memoria de copia.

  const esMedio = /^(video\/|audio\/|application\/mp4|application\/octet-stream)/i;
  const esVideo = /^video\//i;
  // Un blob sin tipo puede ser un vídeo —WhatsApp no siempre lo etiqueta—,
  // pero por debajo de este tamaño son miniaturas, stickers y notas de voz
  // cortas, y se quedan fuera del mapa.
  const MIN_SIN_TIPO = 64 * 1024;
  // Copias `data:` sin elemento conectado que se guardan por si vuelven a
  // hacer falta. Las que están puestas en un elemento no se sueltan: viven lo
  // que viva él. La copia cuesta cuatro tercios del fichero, y el motor hace
  // las suyas (ver «Reproductores fuera del documento»).
  const MAX_COPIAS = 2;
  // Cuánto espera un vídeo fuera del documento antes de que se desmonte su
  // reproductor: lo justo para no confundir un cambio de sitio con una baja.
  // El que recibió su fuente sin haber entrado nunca espera más: puede ser la
  // sonda de WhatsApp, que tarda hasta 20 s en darse por vencida.
  const APARCAR_TRAS = 4000;
  const APARCAR_SIN_ENTRAR_TRAS = 30000;
  // Por encima de esto el vídeo va como blob: la copia ocuparía cuatro
  // tercios del fichero en la página más lo que el motor decodifique.
  const MAX_DATA = 64 * 1024 * 1024;
  // Si preparar la copia tarda más que esto, el elemento recibe el blob tal
  // cual: mejor el camino de siempre que un vídeo que no arranca nunca.
  const PLAZO_PREPARAR = 8000;

  const crearUrl = URL.createObjectURL;
  const revocarUrl = URL.revokeObjectURL;
  const reproducirNativo = HTMLMediaElement.prototype.play;
  const cargarNativo = HTMLMediaElement.prototype.load;
  const descriptorMedio = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'src');
  const descriptorFuente = Object.getOwnPropertyDescriptor(HTMLSourceElement.prototype, 'src');
  const descriptorPrecarga = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'preload');
  const descriptorAutoplay = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'autoplay');
  const ponerAtributo = Element.prototype.setAttribute;
  const quitarAtributo = Element.prototype.removeAttribute;

  const candidatos = new Map();     // url del blob → { blob, copia, definitiva, trabajo, motivo }
  const conCopia = [];              // urls de blob con copia viva, de vieja a nueva
  const vigilados = new WeakSet();
  const sondeados = new WeakSet();  // medios con fuente remota ya sondeada tras un fallo
  const esperas = new WeakMap();    // nodo retenido → { url, promesa } de su entrega
  const pausadosPorWrusp = new WeakSet();
  let sinFuenteAnotado = false;     // el aviso de `src=""` sale una vez por vista
  // Las copias se preparan de una en una: leer un vídeo entero y pasarlo a
  // base64 es trabajo del hilo de la página, y en paralelo unos cuantos bastan
  // para dejarla muerta (ADR-041).
  let cola = Promise.resolve();

  const anotar = (texto) => {
    if (window.__wruspOrden) window.__wruspOrden('log/?m=' + encodeURIComponent(texto));
  };

  // ── Marcas en el nodo ────────────────────────────────────────────────────
  // En el propio nodo y no en un mapa: una `data:` de diez megas es una cadena
  // de diez megas, y usarla como clave cuesta recorrerla entera cada vez.
  //
  // `__wruspOrigen`: de qué blob salió la `data:` que lleva el nodo.
  // `__wruspRetenida`: el blob que WhatsApp le asignó y aún no se le entrega.

  const marcar = (nodo, origen) => { nodo.__wruspOrigen = origen; };
  const olvidarMarca = (nodo) => { if (nodo.__wruspOrigen) delete nodo.__wruspOrigen; };
  const retenidaDe = (nodo) => nodo.__wruspRetenida;
  const soltarRetenida = (nodo) => {
    if (nodo.__wruspRetenida !== undefined) delete nodo.__wruspRetenida;
  };

  const enUso = (url) => !!url && Array.prototype.some.call(
    document.querySelectorAll('video, audio, source'),
    (nodo) => nodo.isConnected && nodo.__wruspOrigen === url);

  function soltarCopia(url) {
    const entrada = candidatos.get(url);
    if (!entrada || !entrada.copia) return;
    entrada.copia = null;
    entrada.definitiva = null;
    entrada.trabajo = null;
  }

  function recordarCopia(url) {
    conCopia.push(url);
    while (conCopia.length > MAX_COPIAS) {
      // Nunca la recién llegada: es la que alguien está esperando ahora mismo,
      // y aún no está «en uso» porque se entrega después (0.4.13).
      const i = conCopia.slice(0, -1).findIndex((u) => !enUso(u));
      if (i < 0) break; // todas en uso: se espera a que alguna se quede sola
      const [vieja] = conCopia.splice(i, 1);
      soltarCopia(vieja);
    }
  }

  // ── Visibilidad ──────────────────────────────────────────────────────────
  // Pausa los vídeos con autoplay que salen de la pantalla, porque un GIF
  // fuera de vista sigue decodificando, y reanuda al volver los que pausó
  // Wrusp. Pausar no toca la fuente ni el pipeline. En WebKitGTK 2.52 el
  // observador solo avisa de forma fiable la primera vez (ADR-040), así que es
  // un ahorro cuando llega, no algo de lo que dependa nada.
  //
  // Se observa al registrar el nodo y nunca dentro de `play()`: observar allí
  // dejaba un <video> sin tamaño propio cargando sin llegar a tener metadatos.
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
          continue;
        }
        // Un GIF que se quedó sin turno porque salió del chat antes de tiempo.
        if (medio.autoplay) entregarTodas(medio, false);
        if (pausadosPorWrusp.has(medio)) {
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

  function vigilarVisibilidad(medio) {
    if (visorInterseccion && medio instanceof HTMLVideoElement) {
      visorInterseccion.observe(medio);
    }
  }

  function medioDe(nodo) {
    if (nodo instanceof HTMLMediaElement) return nodo;
    if (nodo instanceof HTMLSourceElement && nodo.parentElement instanceof HTMLMediaElement)
      return nodo.parentElement;
    return null;
  }

  // La fuente que tiene de verdad el motor, sin el disfraz de las retenidas.
  function urlDe(nodo) {
    if (nodo instanceof HTMLMediaElement && descriptorMedio && descriptorMedio.get)
      return descriptorMedio.get.call(nodo);
    if (nodo instanceof HTMLSourceElement && descriptorFuente && descriptorFuente.get)
      return descriptorFuente.get.call(nodo);
    return nodo.getAttribute('src') || '';
  }

  // ¿Tiene el nodo una fuente de verdad? `src=""`, que WhatsApp pone a los
  // vídeos que aún no ha descargado, no cuenta: el motor falla al resolverla
  // sin llegar a elegir reproductor, así que no hay pipeline que destruir (y
  // tampoco abre conexión al bus, medido en ADR-042).
  function tieneFuenteReal(nodo) {
    const atributo = nodo.getAttribute('src');
    return atributo !== null && atributo.trim() !== '';
  }

  function ponerUrl(nodo, url, origen) {
    if (nodo instanceof HTMLMediaElement && descriptorMedio && descriptorMedio.set)
      descriptorMedio.set.call(nodo, url);
    else if (nodo instanceof HTMLSourceElement && descriptorFuente && descriptorFuente.set)
      descriptorFuente.set.call(nodo, url);
    else
      ponerAtributo.call(nodo, 'src', url);
    if (origen) marcar(nodo, origen);
  }

  // WebKitGTK levanta un pipeline de GStreamer por cada medio con fuente
  // aunque nadie lo reproduzca: un chat con decenas de vídeos y notas de voz
  // llegaba a saturar la CPU. La precarga se devuelve al reproducir.
  function aplazarPrecarga(medio) {
    if (!descriptorPrecarga || !descriptorPrecarga.set || !descriptorPrecarga.get) return;
    if (medio.autoplay || !medio.paused) return;
    if (descriptorPrecarga.get.call(medio) !== 'none')
      descriptorPrecarga.set.call(medio, 'none');
  }

  const ponerPrecarga = (medio, valor) => {
    if (descriptorPrecarga && descriptorPrecarga.set) descriptorPrecarga.set.call(medio, valor);
  };

  // ── Preparar la copia ────────────────────────────────────────────────────

  // Lectura nativa del blob como `data:` URL; para un vídeo de diez megas son
  // unas decenas de milisegundos y no pasa por JavaScript byte a byte.
  const comoDataUrl = (blob) => new Promise((listo, falla) => {
    const lector = new FileReader();
    lector.onload = () => listo(String(lector.result));
    lector.onerror = () => falla(lector.error || new Error('FileReader'));
    lector.readAsDataURL(blob);
  });

  // Solo vídeo. El audio de las notas de voz nunca ha fallado como blob, y
  // pasarlo por `data:` sería memoria a cambio de nada. Un blob sin tipo se
  // trata como vídeo mientras el sondeo no diga que no es ni un MP4.
  function llevaVideo(entrada) {
    const tipo = entrada.blob.type || '';
    if (tipo) return esVideo.test(tipo);
    return !/^no es MP4/.test(entrada.motivo || '');
  }

  // Deja lista la URL definitiva del blob y la guarda en `entrada.definitiva`
  // **siempre**, aunque sea la original (ver el bucle de la 0.4.6).
  //
  // Un vídeo se entrega al motor como `data:` URL, reordenado antes si traía
  // el índice al final. Medido con vídeos reales volcados de un chat
  // (ADR-039): como `blob:`, WebKitGTK 2.52 unas veces los rechaza sin leer un
  // byte, otras rompe al mover la barra y otras cuelga el proceso web; como
  // `data:` arrancan y saltan. `MediaSource` con el fichero entero también
  // cuelga, así que no es alternativa.
  function preparar(url) {
    const entrada = candidatos.get(url);
    if (!entrada) return Promise.resolve(url);
    if (entrada.definitiva) return Promise.resolve(entrada.definitiva);
    if (entrada.trabajo) return entrada.trabajo;
    entrada.trabajo = (async () => {
      try {
        const motivo = await hayQueReordenar(entrada.blob);
        let fuente = entrada.blob;
        let nota = '';
        if (!motivo) {
          const arreglado = reordenar(await entrada.blob.arrayBuffer());
          if (arreglado) {
            fuente = new Blob([arreglado], { type: entrada.blob.type || 'video/mp4' });
            nota = ', reordenado';
          } else {
            entrada.motivo = 'no reordenable';
          }
        } else {
          entrada.motivo = motivo;
        }
        if (!llevaVideo(entrada) || fuente.size > MAX_DATA) {
          entrada.definitiva = url;
          return url;
        }
        // En una variable propia: `recordarCopia` puede soltar copias, y lo
        // que se devuelve tiene que ser la cadena que se acaba de leer.
        const copia = await comoDataUrl(fuente);
        entrada.copia = copia;
        entrada.definitiva = copia;
        recordarCopia(url);
        anotar('vídeo entregado como data: (' + Math.round(fuente.size / 1024) + ' KiB' + nota + ')');
        return copia;
      } catch (e) {
        entrada.motivo = 'excepción al leerlo';
        entrada.definitiva = url; // ante cualquier sorpresa, el blob original
        return url;
      }
    })();
    return entrada.trabajo;
  }

  const conPlazo = (promesa, ms, alternativa) => Promise.race([
    promesa,
    new Promise((listo) => setTimeout(() => listo(alternativa), ms)),
  ]);

  // ── Fuente retenida ──────────────────────────────────────────────────────

  // ¿Se retiene esta asignación? Solo un blob de vídeo nuestro, de un tamaño
  // que quepa en una copia.
  function retenible(url) {
    const entrada = candidatos.get(url);
    if (!entrada) return false;
    return llevaVideo(entrada) && entrada.blob.size <= MAX_DATA;
  }

  // El elemento ya tiene reproductor y WhatsApp le da otro vídeo: un visor o
  // un hueco de la lista que se recicla. La asignación lo iba a desmontar de
  // todas formas, así que ese desmontaje —el único, el que WhatsApp ya
  // provocaba— se hace aquí, sin fuente detrás, y el reproductor que venga
  // después no tendrá nada que desmontar. Sin `src` y con `load()` el motor se
  // queda vacío sin disparar `error`; con `src=""` lo dispararía y WhatsApp lo
  // vería. Con <source> hijos, `load()` elegiría uno de ellos: eso no se toca.
  function relevar(medio) {
    if (!(medio instanceof HTMLMediaElement) || medio.querySelector('source[src]')) return false;
    quitarAtributo.call(medio, 'src');
    try { medio.load(); } catch (e) { /* el medio ya no está */ }
    return true;
  }

  function retener(nodo, url) {
    nodo.__wruspRetenida = url;
    // Un GIF se reproduce solo y necesita su fuente ya. React pone la fuente y
    // `autoplay` en la misma pasada y en cualquier orden, así que en la tarea
    // siguiente ya se sabe cuál de los dos casos es. Si `autoplay` llega
    // después, lo entrega su propio envoltorio.
    setTimeout(() => {
      const medio = medioDe(nodo);
      if (retenidaDe(nodo) === url && medio && medio.autoplay) entregar(nodo, false);
    }, 0);
  }

  // Entrega al nodo su fuente definitiva, una sola vez, y devuelve la promesa
  // de que ya la tiene. `pedida`: alguien ha llamado a `play()`; si no, es un
  // GIF, y uno que WhatsApp ya ha sacado del chat mientras esperaba turno no
  // se lee (si vuelve, se le entregará entonces).
  function entregar(nodo, pedida) {
    const url = retenidaDe(nodo);
    if (url === undefined) return Promise.resolve();
    const previa = esperas.get(nodo);
    if (previa && previa.url === url) return previa.promesa;
    const promesa = cola = cola
      .then(() => {
        if (retenidaDe(nodo) !== url) return null;
        const medio = medioDe(nodo);
        if (!pedida && !(medio && medio.isConnected)) return null;
        return conPlazo(preparar(url), PLAZO_PREPARAR, url);
      })
      .then((definitiva) => {
        const espera = esperas.get(nodo);
        if (espera && espera.url === url) esperas.delete(nodo);
        // Sin turno, o la página la cambió o la quitó mientras tanto.
        if (definitiva === null || retenidaDe(nodo) !== url) return;
        soltarRetenida(nodo);
        // Revocada sin copia: WhatsApp ya no quiere ese vídeo.
        if (definitiva === url && !candidatos.has(url)) return;
        ponerUrl(nodo, definitiva, definitiva !== url ? url : null);
        const medio = medioDe(nodo);
        // Entregada a un elemento que no está en el documento: si no llega a
        // entrar, su reproductor no debe quedarse vivo hasta el recolector.
        if (medio === nodo && !nodo.isConnected) vigilarFuera(nodo, APARCAR_SIN_ENTRAR_TRAS);
        // Un <source> no carga nada por sí solo: el medio elige fuente al
        // cargar. Solo si nunca eligió ninguna ni ha fallado, que entonces no
        // hay reproductor que destruir.
        if (medio && medio !== nodo && !medio.error
          && (medio.networkState === HTMLMediaElement.NETWORK_EMPTY
            || medio.networkState === HTMLMediaElement.NETWORK_NO_SOURCE)) {
          try { medio.load(); } catch (e) { /* el medio ya no está */ }
        }
      })
      .catch(() => {});
    esperas.set(nodo, { url, promesa });
    return promesa;
  }

  // El medio y sus <source> que estén esperando su fuente.
  function entregarTodas(medio, pedida) {
    const pendientes = [];
    if (retenidaDe(medio) !== undefined) pendientes.push(entregar(medio, pedida));
    for (const fuente of medio.querySelectorAll('source'))
      if (retenidaDe(fuente) !== undefined) pendientes.push(entregar(fuente, pedida));
    return pendientes;
  }

  // ── Reproductores fuera del documento ────────────────────────────────────
  // Un <video> que sale del documento se pausa, pero conserva su reproductor
  // hasta que el recolector destruye el elemento: el pipeline de GStreamer
  // con sus hilos y búferes y, si su fuente es una `data:`, las copias que el
  // motor hace de ella (la del atributo, la del reproductor, las de cada
  // elemento de GStreamer que guarda la URI y el fichero decodificado). En el
  // banco `banco_chat_videos`, medir y reproducir un vídeo de 12 MB sube el
  // proceso web 250 MB, y salir del chat no liberaba nada: el recolector no
  // tiene prisa, porque para él un <video> es un objeto pequeño. En uso real,
  // una tarde recorriendo chats con GIF llegó a 4 GB y otros tantos de swap
  // (ADR-047).
  //
  // Así que, cuando un vídeo al que Wrusp entregó la fuente lleva un rato
  // fuera, se desmonta su reproductor y se le deja la fuente **retenida**,
  // como si WhatsApp acabara de asignarla: si vuelve al documento se le
  // entrega otra vez (al reproducirse, o enseguida si es un GIF), y la página
  // sigue leyendo en `src` el blob que puso.
  //
  // Desmontar es cambiar la fuente, justo lo que el ADR-043 prohíbe hacer
  // sobre un reproductor que acaba de fallar: ese desmontaje se quedó
  // esperando un cerrojo de GStreamer con el pipeline a medio *preroll*. Por
  // eso solo se toca un reproductor sano y quieto —sin error, sin búsqueda en
  // curso y con imagen ya decodificada— que es el mismo desmontaje que haría
  // el recolector. Los demás se le quedan a él, como hasta ahora.
  const fuera = new Map(); // medio → instante a partir del cual se desmonta
  let barrido = 0;
  let desmontados = 0;

  function vigilarFuera(medio, espera) {
    if (!(medio instanceof HTMLVideoElement) || !medio.__wruspOrigen) return;
    fuera.set(medio, performance.now() + espera);
    if (!barrido) barrido = setTimeout(barrer, espera + 50);
  }

  function barrer() {
    barrido = 0;
    const ahora = performance.now();
    let proximo = Infinity;
    for (const [medio, cuando] of fuera) {
      if (medio.isConnected) {
        fuera.delete(medio);
      } else if (cuando > ahora) {
        proximo = Math.min(proximo, cuando);
      } else {
        fuera.delete(medio);
        desmontar(medio);
      }
    }
    if (proximo !== Infinity) barrido = setTimeout(barrer, proximo - ahora + 50);
  }

  function desmontar(medio) {
    const origen = medio.__wruspOrigen;
    if (!origen || medio.isConnected || medio.querySelector('source[src]')) return;
    if (medio.error || medio.seeking || medio.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) return;
    try { if (!medio.paused) medio.pause(); } catch (e) { /* el medio ya no está */ }
    olvidarMarca(medio);
    quitarAtributo.call(medio, 'src');
    // Sin fuente, `load()` deja el motor vacío sin disparar `error` (ver `relevar`).
    try { cargarNativo.call(medio); } catch (e) { /* el medio ya no está */ }
    medio.__wruspRetenida = origen;
    // Y la copia de Wrusp, si ya nadie la lleva puesta: volver a leer el blob
    // cuesta unas decenas de milisegundos.
    if (!enUso(origen)) {
      soltarCopia(origen);
      const i = conCopia.indexOf(origen);
      if (i >= 0) conCopia.splice(i, 1);
    }
    desmontados++;
    window.__wruspDesmontados = desmontados; // lo lee el banco
    if (desmontados === 1 || desmontados % 25 === 0)
      anotar('reproductores fuera del documento desmontados: ' + desmontados);
  }

  // Vuelve al documento uno que se desmontó: un GIF necesita su fuente ya.
  function devolver(medio) {
    if (retenidaDe(medio) === undefined || tieneFuenteReal(medio)) return;
    vigilarVisibilidad(medio);
    if (medio.autoplay) entregarTodas(medio, false);
  }

  // ── Fallos: se anotan y no se tocan ──────────────────────────────────────
  // Hasta la 0.4.14 un fallo se «reparaba»: se cambiaba la fuente por la copia,
  // o por una URL nueva del mismo blob, o se quitaba para desmontar el
  // pipeline. Todo eso destruye el reproductor en el hilo principal, y sobre
  // uno que acaba de fallar es justo cuando esa destrucción se queda esperando
  // un cerrojo de GStreamer: el cuelgue de más de dos minutos del 11-09-2026
  // salió de ahí (ADR-043). Ahora el fallo se anota con todo el detalle, se
  // describe el fichero y el elemento se deja como está. Con la fuente
  // decidida antes, un fallo ya no es algo que Wrusp pueda arreglar después.

  function describirFallido(entrada) {
    if (!entrada || !entrada.blob || entrada.descrito) return;
    entrada.descrito = true;
    codecsDe(entrada.blob).then((codecs) => {
      anotar('códecs del medio que falla: ' + codecs + ' (' + Math.round(entrada.blob.size / 1024) + ' KiB ' + (entrada.blob.type || 'sin tipo') + ')');
    });
    ofrecerVolcado(entrada.blob);
  }

  function vigilarFallo(medio) {
    if (vigilados.has(medio)) return;
    vigilados.add(medio);
    medio.addEventListener('error', () => {
      const codigo = medio.error ? medio.error.code : 0;
      // `src=""`: vídeos que WhatsApp aún no ha descargado. El registro de la
      // 0.4.8 mostró 71 de estos.
      const atributo = medio.getAttribute('src');
      if (atributo !== null && atributo.trim() === '' && !medio.querySelector('source[src]')) {
        if (!sinFuenteAnotado) {
          sinFuenteAnotado = true;
          anotar('medio con src vacío (código ' + codigo + '): se ignora, y los siguientes no se anotan');
        }
        return;
      }
      const fuente = medio.querySelector('source[src]');
      const url = String(urlDe(medio.getAttribute('src') !== null || !fuente ? medio : fuente) || '');
      const origen = medio.__wruspOrigen || (fuente && fuente.__wruspOrigen) || '';
      const entrada = candidatos.get(origen || url);
      // El banco lee de aquí qué pasó.
      window.__wruspUltimoFallo = { codigo, url };
      let esquema = url.slice(0, url.indexOf(':') + 1) || 'sin src';
      if (esquema === 'https:' || esquema === 'http:') {
        try { const u = new URL(url); esquema = u.origin + u.pathname.slice(0, 48); } catch (e) { /* se queda el esquema */ }
      }
      const detalle = ' (' + (medio.autoplay ? 'autoplay, ' : '') + 'código ' + codigo
        + ', red ' + medio.networkState + ', datos ' + medio.readyState + ', '
        + (origen ? 'copia de Wrusp' : entrada ? 'blob sin copia' : 'fuente ajena, ' + esquema)
        + (entrada && entrada.blob ? ', ' + Math.round(entrada.blob.size / 1024) + ' KiB ' + (entrada.blob.type || 'sin tipo') : '')
        + (entrada && entrada.motivo ? ', ' + entrada.motivo : '') + ')';
      anotar('medio con fallo' + detalle + ': no se toca');
      if (entrada) {
        describirFallido(entrada);
        return;
      }
      // Fuente remota: se sondea el principio con una petición de rango, como
      // haría el motor, sin tocar el elemento. Si la sirve el service worker
      // de WhatsApp, aquí se ve.
      if (/^https?:/.test(url) && url !== location.href && !sondeados.has(medio)) {
        sondeados.add(medio);
        fetch(url, { headers: { Range: 'bytes=0-262143' } }).then(async (r) => {
          const tipo = r.headers.get('content-type') || 'sin content-type';
          const rango = r.headers.get('content-range') || 'sin content-range';
          const blob = await r.blob();
          const motivo = await hayQueReordenar(blob);
          const codecs = await codecsDe(blob);
          anotar('sondeo de la fuente remota: HTTP ' + r.status + ', ' + tipo + ', ' + rango + ', ' + Math.round(blob.size / 1024) + ' KiB leídos, ' + (motivo || 'índice al final') + ', ' + codecs);
          ofrecerVolcado(blob);
        }).catch((e) => anotar('sondeo de la fuente remota: no se pudo leer (' + ((e && e.message) || e) + ')'));
      }
    }, true);
  }

  // ── Registro de nodos ────────────────────────────────────────────────────

  // Cualquier fuente remota o de blob, no solo los candidatos: WhatsApp también
  // pone vídeos con fuente `https:` (servidos por su service worker).
  const conFuente = (valor) => /^(blob:|https?:)/.test(String(valor || ''));

  function registrar(nodo) {
    const medio = medioDe(nodo);
    if (!medio) return;
    vigilarFallo(medio);
    vigilarVisibilidad(medio);
    if (conFuente(urlDe(nodo))) aplazarPrecarga(medio);
  }

  HTMLMediaElement.prototype.play = function () {
    const medio = this;
    vigilarFallo(medio);
    ponerPrecarga(medio, 'auto');
    const pendientes = entregarTodas(medio, true);
    if (!pendientes.length) return reproducirNativo.call(medio);
    return Promise.all(pendientes).then(() => reproducirNativo.call(medio));
  };

  // `load()` pide la carga tanto como `play()`. La sonda con la que WhatsApp
  // mide un vídeo (`MediaLoad`, en su código público) crea un <video> suelto,
  // le pone el blob, llama a `load()` y espera `loadedmetadata` y
  // `canplaythrough` sin reproducir nunca; a los 20 s lo da por perdido
  // («video-load-timeout»). La 0.4.15 solo entregaba la fuente con `play()`, y
  // esa sonda no acababa. Si el nodo espera su fuente, la entrega hace las
  // veces de `load()`: poner la fuente ya arranca la carga, y cargar ahora sin
  // ella solo sería un paso más del motor para nada.
  HTMLMediaElement.prototype.load = function () {
    vigilarFallo(this);
    if (!entregarTodas(this, true).length) return cargarNativo.call(this);
    ponerPrecarga(this, 'auto');
  };

  URL.createObjectURL = function (objeto) {
    const url = crearUrl.call(this, objeto);
    if (objeto instanceof Blob) {
      const candidato = objeto.type ? esMedio.test(objeto.type) : objeto.size >= MIN_SIN_TIPO;
      if (candidato) candidatos.set(url, { blob: objeto, copia: null, definitiva: null, trabajo: null });
    }
    return url;
  };

  URL.revokeObjectURL = function (url) {
    const clave = String(url);
    if (candidatos.has(clave)) {
      soltarCopia(clave);
      const i = conCopia.indexOf(clave);
      if (i >= 0) conCopia.splice(i, 1);
      candidatos.delete(clave);
    }
    return revocarUrl.call(this, url);
  };

  // Una asignación de fuente, por la propiedad o por el atributo. Devuelve si
  // Wrusp se ha hecho cargo; si no, pasa al motor tal cual.
  function asignar(nodo, valor) {
    const url = String(valor || '');
    // La misma fuente que ya lleva, en su versión `data:`: volver a asignarla
    // recargaría, o sea, destruiría el reproductor para nada.
    if (url && nodo.__wruspOrigen === url && tieneFuenteReal(nodo)) return true;
    soltarRetenida(nodo);
    olvidarMarca(nodo);
    const medio = medioDe(nodo);
    // Antes de que la fuente llegue: asignarla arranca la carga en el acto, y
    // aplazar la precarga después ya no la cancela.
    if (medio && conFuente(url)) aplazarPrecarga(medio);
    if (!retenible(url)) return false;
    if (tieneFuenteReal(nodo) && !relevar(nodo)) return false;
    retener(nodo, url);
    return true;
  }

  // Tres vías para poner la fuente, y hay que estar en las tres: el hueco de
  // `setAttribute` dejaba pasar vídeos sin aplazar su precarga.
  function envolverSrc(prototipo, descriptor) {
    if (!descriptor || !descriptor.get || !descriptor.set || !descriptor.configurable) return;
    Object.defineProperty(prototipo, 'src', {
      configurable: descriptor.configurable,
      enumerable: descriptor.enumerable,
      // La página ve la fuente que puso, aunque el motor aún no la tenga.
      get() {
        const retenida = retenidaDe(this);
        return retenida !== undefined ? retenida : descriptor.get.call(this);
      },
      set(valor) {
        if (!asignar(this, valor)) descriptor.set.call(this, valor);
        registrar(this);
      },
    });
  }
  envolverSrc(HTMLMediaElement.prototype, descriptorMedio);
  envolverSrc(HTMLSourceElement.prototype, descriptorFuente);

  // `autoplay` puesto después de la fuente: el GIF necesita la suya ya.
  if (descriptorAutoplay && descriptorAutoplay.get && descriptorAutoplay.set && descriptorAutoplay.configurable) {
    Object.defineProperty(HTMLMediaElement.prototype, 'autoplay', {
      configurable: true,
      enumerable: descriptorAutoplay.enumerable,
      get: descriptorAutoplay.get,
      set(valor) {
        descriptorAutoplay.set.call(this, valor);
        if (valor) entregarTodas(this, false);
      },
    });
  }

  const esNodoDeMedio = (nodo) => nodo instanceof HTMLMediaElement || nodo instanceof HTMLSourceElement;

  Element.prototype.setAttribute = function (nombre, valor) {
    const clave = String(nombre).toLowerCase();
    if (clave === 'src' && esNodoDeMedio(this)) {
      if (!asignar(this, valor)) ponerAtributo.call(this, nombre, valor);
      registrar(this);
      return;
    }
    ponerAtributo.call(this, nombre, valor);
    if (clave === 'autoplay' && this instanceof HTMLMediaElement) entregarTodas(this, false);
  };

  // Quitar la fuente de un nodo que la tenía retenida: ya no se le entrega.
  Element.prototype.removeAttribute = function (nombre) {
    if (String(nombre).toLowerCase() === 'src' && esNodoDeMedio(this)) soltarRetenida(this);
    return quitarAtributo.call(this, nombre);
  };

  // Los que llegan al documento con la fuente ya puesta (el analizador de HTML
  // no pasa por `setAttribute`): esos no se pueden retener, solo vigilar. Los
  // que se van, se dejan de observar.
  // Un solo recorrido por subárbol: WhatsApp mete miles de nodos al pintar.
  const MEDIOS_QUE_ENTRAN = 'video, audio[src], source[src]';
  function visitarQueEntra(nodo) {
    if (nodo.hasAttribute('src')) registrar(nodo);
    // Sin fuente: puede ser uno que se desmontó fuera del documento y vuelve.
    else if (nodo instanceof HTMLVideoElement) devolver(nodo);
  }
  function registrarArbol(raiz) {
    if (!raiz || raiz.nodeType !== Node.ELEMENT_NODE) return;
    if (raiz.matches(MEDIOS_QUE_ENTRAN)) visitarQueEntra(raiz);
    for (const nodo of raiz.querySelectorAll(MEDIOS_QUE_ENTRAN)) visitarQueEntra(nodo);
  }
  function olvidarArbol(raiz) {
    if (!raiz || raiz.nodeType !== Node.ELEMENT_NODE) return;
    const videos = raiz instanceof HTMLVideoElement ? [raiz] : raiz.querySelectorAll('video');
    for (const video of videos) {
      if (visorInterseccion) visorInterseccion.unobserve(video);
      vigilarFuera(video, APARCAR_TRAS);
    }
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

/// Un `AudioContext` creado sin gesto del usuario nace suspendido, como en
/// Chrome y en Safari.
///
/// El bundle principal de WhatsApp trae al principio un *polyfill* de Web
/// Audio que, al cargar la página, hace `new AudioContext` para mirar qué
/// métodos tiene el prototipo, y lo abandona sin cerrarlo. En Chrome y en
/// Safari ese contexto nace suspendido, porque ningún gesto del usuario lo ha
/// autorizado, y no cuesta nada. WebKitGTK no aplica esa política: el contexto
/// arranca, y un contexto en marcha no se recoge nunca. En la instalación del
/// usuario cada cuenta tenía un flujo «Playback Stream» abierto en PipeWire
/// mandando silencio sin parar: la mitad de la CPU del proceso web en reposo,
/// el grafo de audio siempre despierto y los auriculares Bluetooth sin poder
/// descansar (ADR-047).
///
/// Aquí se aplica esa misma política: un contexto creado sin activación del
/// usuario se suspende nada más nacer. El código de WhatsApp está escrito para
/// ella, porque es la de los navegadores que admite, y llama a `resume()`
/// cuando de verdad va a sonar; `resume()` no se toca. Decodificar
/// (`decodeAudioData`, que es lo que hace con las notas de voz para Safari)
/// funciona igual con el contexto suspendido.
///
/// Solo en Linux: en Windows y en macOS el motor ya aplica su política.
#[cfg(target_os = "linux")]
pub fn quiet_idle_audio_script() -> String {
    r#"(function () {
  const Nativo = window.AudioContext;
  if (typeof Nativo !== 'function') return;
  const suspender = Nativo.prototype.suspend;

  // Activación del usuario: la del motor si la expone, y si no, un gesto de
  // verdad en los últimos cinco segundos, que es lo que dura en Chrome.
  let ultimoGesto = -Infinity;
  for (const tipo of ['pointerdown', 'mousedown', 'keydown', 'touchend', 'click']) {
    addEventListener(tipo, (e) => { if (e.isTrusted) ultimoGesto = performance.now(); }, true);
  }
  const hayGesto = () => {
    const activacion = navigator.userActivation;
    if (activacion && typeof activacion.isActive === 'boolean') return activacion.isActive;
    return performance.now() - ultimoGesto < 5000;
  };

  const conPolitica = new Proxy(Nativo, {
    construct(objetivo, argumentos, nuevo) {
      const contexto = Reflect.construct(objetivo, argumentos, nuevo);
      if (!hayGesto()) {
        try {
          const p = suspender.call(contexto);
          if (p && p.catch) p.catch(() => {});
        } catch (e) { /* si no se deja, se queda como el motor diga */ }
      }
      return contexto;
    },
  });
  window.AudioContext = conPolitica;
  if (window.webkitAudioContext === Nativo) window.webkitAudioContext = conPolitica;
})();"#
        .to_string()
}

/// Fuera de Linux el motor ya aplica su política de reproducción automática.
#[cfg(not(target_os = "linux"))]
pub fn quiet_idle_audio_script() -> String {
    String::new()
}

/// Corrige la plataforma del user-agent que ve la página (ADR-044).
///
/// En `whatsapp.com`, WebKitGTK sustituye el user-agent de la aplicación por
/// el de Safari en Mac (ver la cabecera del módulo). WhatsApp lee de ahí el
/// sistema operativo, y con «Mac OS»:
///
/// - anuncia su aplicación de escritorio en la bienvenida, bajo la lista de
///   chats, en el menú, en los resultados de búsqueda y en los avisos de
///   llamada. En su código público todas esas variantes pasan por la misma
///   comprobación (`useWAWebDesktopUpsellPlatformAwareOsVersionCheck`), que
///   devuelve falso si el sistema no es Windows ni Mac: con Linux no anuncia
///   nada, y no hay nodo que esconder;
/// - espera la tecla Comando en sus atajos de teclado en vez de Ctrl;
/// - da por hecho que el sistema trae los emojis de Apple y no usa sus propias
///   imágenes (`hasEmoji` en `WAWebUA`).
///
/// Solo se cambia la plataforma. El resto de la cadena se deja como la pone el
/// motor: WhatsApp sigue viendo Safari, que es la identidad con la que ha
/// funcionado siempre en Wrusp (vídeo incluido), y el servidor sigue
/// recibiendo la cabecera que el motor decida. Si el motor deja algún día de
/// sustituirla, la expresión no casa y esto no hace nada.
///
/// **El atajo de «Nuevo chat» (ADR-047).** «Safari fuera de Mac» es una
/// combinación que WhatsApp no espera. Su tabla de atajos
/// (`WAWebKeyboardShortcuts`) le da a Safari «Nuevo chat» con la tecla `N`
/// mayúscula y Control en Mac, pero **sin ningún modificador** en el resto de
/// sistemas: con la plataforma corregida, Mayús+N abría un chat nuevo y no
/// había forma de escribir una N mayúscula. Pasado por su comparador real:
///
/// ```text
///                        Mayús+N         Ctrl+Alt+N     su lista dice
/// Safari en Linux        Nuevo chat      nada           «N»
/// Safari en Mac          nada            nada           Ctrl+Mayús+N
/// Chrome en Linux        nada            Nuevo chat     Ctrl+Alt+N
/// ```
///
/// Así que, solo cuando se ha corregido la plataforma, se escucha el teclado
/// antes que WhatsApp: a Mayús+N se le oculta la mayúscula al comparador (el
/// carácter lo escribe el motor igual, porque eso no depende de lo que lea la
/// página) y Ctrl+Alt+N, el atajo de WhatsApp en Linux y en Windows, se le
/// presenta como la combinación que su tabla espera para Safari. El resto de
/// sus atajos ya son los de Linux, con Ctrl+Alt.
#[cfg(target_os = "linux")]
const UA_PLATFORM_FIX: &str = r"
  const uaVisto = navigator.userAgent || '';
  const PLATAFORMA_MAC = /\(Macintosh;[^)]*\)/;
  if (PLATAFORMA_MAC.test(uaVisto)) {
    const uaLinux = uaVisto.replace(PLATAFORMA_MAC, '(X11; Linux x86_64)');
    enNavigator('userAgent', uaLinux);
    enNavigator('appVersion', uaLinux.replace(/^Mozilla\//, ''));

    // Lo que lee el comparador de WhatsApp se cambia en el propio evento, que
    // sigue su camino: el motor escribe la letra según la tecla de verdad.
    const verComo = (evento, cambios) => {
      for (const [prop, valor] of Object.entries(cambios)) {
        try {
          Object.defineProperty(evento, prop, { value: valor, configurable: true });
        } catch (e) { /* un evento sellado se queda como viene */ }
      }
    };
    // En `window` y en captura: es el primer escuchador de todo el recorrido,
    // y este script corre antes que el código de la página.
    addEventListener('keydown', (evento) => {
      if (evento.isComposing || evento.metaKey) return;
      if (evento.key !== 'N' && evento.key !== 'n') return;
      if (evento.ctrlKey && evento.altKey && !evento.shiftKey) {
        verComo(evento, { key: 'N', shiftKey: true, ctrlKey: false, altKey: false });
      } else if (evento.shiftKey && !evento.ctrlKey && !evento.altKey) {
        // Con bloqueo de mayúsculas llega «n» con Mayús, y WhatsApp también la
        // sube a «N»: sin la mayúscula a la vista, las dos son solo una letra.
        verComo(evento, { shiftKey: false });
      }
    }, true);
  }
";

/// Fuera de Linux no hay WebKitGTK ni apaño que corregir, y en macOS la
/// plataforma «Macintosh» es la verdadera.
#[cfg(not(target_os = "linux"))]
const UA_PLATFORM_FIX: &str = "";

/// Completa el disfraz de navegador que el user-agent empieza.
///
/// WhatsApp mira el user-agent, el objeto `navigator` y algunas señales que
/// solo tiene Chrome. Lo primero que hace este script, en Linux, es corregir
/// la plataforma del user-agent (ver `UA_PLATFORM_FIX`): es lo que decide si
/// WhatsApp anuncia «WhatsApp para Mac». El resto rellena lo que WebKitGTK no
/// trae (`vendor` de Google, `userAgentData`, `window.chrome`, complementos).
///
/// `navigator.platform` y `userAgentData.platform` dicen Windows desde la
/// 0.4.1 (ADR-032), buscando el juego completo de emojis de WhatsApp. En su
/// código público esa decisión sale de la cadena del user-agent y no de estos
/// dos campos, así que quien la arregla es la corrección de plataforma. Se
/// dejan como están: nada de lo leído depende de ellos.
pub fn disguise_script() -> String {
    format!(
        r#"(function () {{
  const define = (obj, prop, value) => {{
    try {{
      Object.defineProperty(obj, prop, {{ get: () => value, configurable: true }});
    }} catch (e) {{ /* si la propiedad no es redefinible, se deja como está */ }}
  }};
  const enNavigator = (prop, value) => define(navigator, prop, value);
{ua_fix}
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
        v = CHROME_VERSION,
        ua_fix = UA_PLATFORM_FIX
    )
}
