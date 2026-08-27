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

/// Evita la sobrecarga de pipelines y la corrupción de MP4 servidos como `blob:` en WebKitGTK.
///
/// 1. Precarga perezosa: WebKitGTK arranca un pipeline de GStreamer por cada
///    `<video>` o `<audio>` en el DOM salvo que su precarga sea `none`. En chats
///    con decenas de notas de voz y vídeos, esto creaba más de 70 demuxers
///    concurrentes saturando la CPU al 100%. Se fuerza `preload="none"` en medios
///    inactivos y solo se activa la precarga al solicitar la reproducción (`play()`).
/// 2. Conversión bajo demanda de blobs de vídeo/audio a `data:` URL al reproducir:
///    WebKit 2.52 falla con el buffer circular de 2 MiB en blobs MP4 cuyo átomo
///    `moov` está al final, corrompiendo el stream y lanzando bucles infinitos de
///    errores en `avdec_aac` y `h264parse`. La entrega en `data:` evita esa ruta.
#[cfg(target_os = "linux")]
pub fn fix_large_mp4_blobs_script() -> String {
    r#"(function () {
  const RETENCION_NODO_NUEVO = 60_000;
  const esMedio = /^(video\/|audio\/|application\/mp4|application\/octet-stream)/i;
  const crearUrl = URL.createObjectURL;
  const revocarUrl = URL.revokeObjectURL;
  const candidatos = new Map();
  const descriptorVideo = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'src');
  const descriptorSource = Object.getOwnPropertyDescriptor(HTMLSourceElement.prototype, 'src');
  const descriptorPreload = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'preload');
  const reproducirNativo = HTMLMediaElement.prototype.play;
  const reproduccionesPendientes = new WeakMap();
  const videosVigilados = new WeakSet();
  let limpiezaPendiente = 0;

  function medioDe(nodo) {
    if (nodo instanceof HTMLMediaElement) return nodo;
    if (nodo instanceof HTMLSourceElement && nodo.parentElement instanceof HTMLMediaElement)
      return nodo.parentElement;
    return null;
  }

  function urlActual(nodo) {
    if (nodo instanceof HTMLMediaElement && descriptorVideo && descriptorVideo.get)
      return descriptorVideo.get.call(nodo);
    if (nodo instanceof HTMLSourceElement && descriptorSource && descriptorSource.get)
      return descriptorSource.get.call(nodo);
    return nodo.getAttribute('src') || '';
  }

  function ponerUrl(nodo, url) {
    if (nodo instanceof HTMLMediaElement && descriptorVideo && descriptorVideo.set)
      descriptorVideo.set.call(nodo, url);
    else if (nodo instanceof HTMLSourceElement && descriptorSource && descriptorSource.set)
      descriptorSource.set.call(nodo, url);
    else
      nodo.setAttribute('src', url);
  }

  function restaurar(medio, posicion, reproduciendo, urlDatos) {
    const listo = () => {
      if (urlActual(medio) !== urlDatos) return;
      if (posicion > 0 && Number.isFinite(medio.duration)) {
        try { medio.currentTime = Math.min(posicion, Math.max(0, medio.duration - 0.001)); }
        catch (e) { /* el medio pudo cambiar */ }
      }
      if (reproduciendo || medio.autoplay)
        medio.play().catch(() => {});
    };
    if (medio.readyState >= HTMLMediaElement.HAVE_METADATA) listo();
    else medio.addEventListener('loadedmetadata', listo, { once: true });
  }

  function reemplazarNodo(nodo, original, datos) {
    if (urlActual(nodo) !== original && nodo.getAttribute('src') !== original) return;
    const medio = medioDe(nodo);
    if (!medio) return;

    const posicion = Number.isFinite(medio.currentTime) ? medio.currentTime : 0;
    const reproduciendo = !medio.paused;
    if (descriptorPreload && descriptorPreload.set) descriptorPreload.set.call(medio, 'auto');
    ponerUrl(nodo, datos);
    medio.load();
    restaurar(medio, posicion, reproduciendo, datos);
  }

  function reemplazar(original, entrada) {
    if (!entrada.datos) return;
    for (const nodo of entrada.nodos.keys())
      reemplazarNodo(nodo, original, entrada.datos);
    for (const nodo of document.querySelectorAll('video[src], audio[src], source[src]')) {
      if (urlActual(nodo) === original || nodo.getAttribute('src') === original) {
        entrada.nodos.set(nodo, true);
        reemplazarNodo(nodo, original, entrada.datos);
      }
    }
  }

  function terminar(entrada) {
    const resolver = entrada.terminar;
    entrada.terminar = null;
    if (resolver) resolver();
  }

  function convertir(original, entrada) {
    if (!entrada.promesa) {
      entrada.promesa = new Promise((resolver) => { entrada.terminar = resolver; });
    }
    if (entrada.lector || entrada.datos) return entrada.promesa;
    const lector = entrada.lector = new FileReader();
    lector.addEventListener('load', () => {
      if (typeof lector.result === 'string') {
        entrada.datos = lector.result;
        reemplazar(original, entrada);
      }
      terminar(entrada);
      if (entrada.revocada) {
        revocarUrl.call(URL, original);
        pedirLimpieza();
      }
    }, { once: true });
    lector.addEventListener('error', () => {
      terminar(entrada);
      candidatos.delete(original);
    }, { once: true });
    lector.addEventListener('abort', () => terminar(entrada), { once: true });
    lector.readAsDataURL(entrada.blob);
    return entrada.promesa;
  }

  function prepararReproduccion(medio, pendiente) {
    const entrada = candidatos.get(pendiente.original);
    if (!entrada) {
      if (reproduccionesPendientes.get(medio) === pendiente)
        reproduccionesPendientes.delete(medio);
      return Promise.resolve();
    }
    return convertir(pendiente.original, entrada).then(() => {
      if (reproduccionesPendientes.get(medio) === pendiente)
        reproduccionesPendientes.delete(medio);
    });
  }

  function vigilarAutoplay(medio) {
    if (videosVigilados.has(medio)) return;
    videosVigilados.add(medio);
    medio.addEventListener('play', () => {
      const pendiente = reproduccionesPendientes.get(medio);
      if (!pendiente) return;
      medio.pause();
      prepararReproduccion(medio, pendiente).then(() => {
        if (medio.isConnected) reproducirNativo.call(medio).catch(() => {});
      });
    });
  }

  function registrar(nodo, valor) {
    const medio = medioDe(nodo);
    if (!medio) return null;
    // Forzar preload="none" por defecto para no arrancar pipelines en medios inactivos
    if (descriptorPreload && descriptorPreload.get && descriptorPreload.set) {
      const actualPreload = descriptorPreload.get.call(medio);
      if (actualPreload !== 'none' && !medio.autoplay && medio.paused) {
        descriptorPreload.set.call(medio, 'none');
      }
    }
    const original = String(valor);
    const entrada = candidatos.get(original);
    if (!entrada) {
      const anterior = reproduccionesPendientes.get(medio);
      if (anterior && anterior.original !== original)
        reproduccionesPendientes.delete(medio);
      return null;
    }
    entrada.nodos.set(nodo, entrada.nodos.get(nodo) || nodo.isConnected);
    vigilarAutoplay(medio);
    if (!entrada.datos) {
      reproduccionesPendientes.set(medio, { original });
    }
    return entrada.datos;
  }

  function nodoUsa(nodo, original, entrada) {
    const actual = urlActual(nodo);
    const atributo = nodo.getAttribute('src') || '';
    return actual === original || atributo === original
      || (!!entrada.datos && (actual === entrada.datos || atributo === entrada.datos));
  }

  function limpiar() {
    limpiezaPendiente = 0;
    const ahora = Date.now();
    for (const [original, entrada] of candidatos) {
      for (const [nodo, estuvoConectado] of entrada.nodos) {
        if (nodo.isConnected) {
          entrada.nodos.set(nodo, true);
          if (!nodoUsa(nodo, original, entrada)) entrada.nodos.delete(nodo);
          continue;
        }
        const aunPuedeInsertarse = !estuvoConectado && entrada.revocadaEn
          && ahora - entrada.revocadaEn < RETENCION_NODO_NUEVO;
        if (!aunPuedeInsertarse) entrada.nodos.delete(nodo);
      }
      if (!entrada.revocada || entrada.nodos.size) continue;
      if (entrada.lector && entrada.lector.readyState === FileReader.LOADING)
        entrada.lector.abort();
      candidatos.delete(original);
    }
  }

  function pedirLimpieza() {
    if (limpiezaPendiente) return;
    limpiezaPendiente = setTimeout(limpiar, 0);
  }

  URL.createObjectURL = function (objeto) {
    const url = crearUrl.call(this, objeto);
    if (objeto instanceof Blob && (objeto.type ? esMedio.test(objeto.type) : true))
      candidatos.set(url, {
        blob: objeto,
        lector: null,
        datos: null,
        nodos: new Map(),
        promesa: null,
        terminar: null,
        revocada: false,
        revocadaEn: 0,
      });
    return url;
  };

  URL.revokeObjectURL = function (url) {
    const original = String(url);
    const entrada = candidatos.get(original);
    if (!entrada) return revocarUrl.call(this, url);
    entrada.revocada = true;
    entrada.revocadaEn = Date.now();
    revocarUrl.call(this, url);
    pedirLimpieza();
    setTimeout(pedirLimpieza, RETENCION_NODO_NUEVO + 1_000);
  };

  function envolverSrc(prototipo, descriptor) {
    if (!descriptor || !descriptor.get || !descriptor.set || !descriptor.configurable) return;
    Object.defineProperty(prototipo, 'src', {
      configurable: descriptor.configurable,
      enumerable: descriptor.enumerable,
      get: descriptor.get,
      set(valor) {
        const datos = registrar(this, valor);
        descriptor.set.call(this, datos || valor);
      },
    });
  }
  envolverSrc(HTMLMediaElement.prototype, descriptorVideo);
  envolverSrc(HTMLSourceElement.prototype, descriptorSource);

  HTMLMediaElement.prototype.play = function () {
    if (descriptorPreload && descriptorPreload.set) descriptorPreload.set.call(this, 'auto');
    const pendiente = reproduccionesPendientes.get(this);
    if (!pendiente) return reproducirNativo.call(this);
    return prepararReproduccion(this, pendiente)
      .then(() => reproducirNativo.call(this));
  };

  function registrarArbol(raiz) {
    if (!raiz || raiz.nodeType !== Node.ELEMENT_NODE) return;
    if (raiz.matches('video[src], audio[src], source[src]'))
      registrar(raiz, urlActual(raiz) || raiz.getAttribute('src'));
    for (const nodo of raiz.querySelectorAll('video[src], audio[src], source[src]'))
      registrar(nodo, urlActual(nodo) || nodo.getAttribute('src'));
  }

  new MutationObserver((mutaciones) => {
    let necesitaLimpieza = false;
    for (const mutacion of mutaciones) {
      for (const nodo of mutacion.addedNodes) registrarArbol(nodo);
      if (mutacion.removedNodes.length) necesitaLimpieza = true;
    }
    if (necesitaLimpieza) pedirLimpieza();
  }).observe(document, {
    childList: true,
    subtree: true,
  });
})();"#
        .to_string()
}

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
