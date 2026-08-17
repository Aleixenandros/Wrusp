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
/// sigue anunciando su app de escritorio en la pantalla de bienvenida, y en
/// Wrusp no tiene sentido. Se localiza por el enlace de la tienda, no por
/// clases CSS (que cambian en cada despliegue), y solo se oculta el contenedor
/// si es pequeño respecto a la ventana, para no borrar media interfaz si
/// WhatsApp reorganiza su árbol.
///
/// Es cosmético: si algún día deja de encontrarlo, lo peor que pasa es que el
/// anuncio vuelva a verse.
pub fn hide_native_app_promo_script() -> String {
    r#"(function () {
  const TIENDAS = ['apps.apple.com', 'microsoft.com/store', 'aka.ms/', 'whatsapp.com/download'];
  const AREA_MAXIMA = 0.35; // del área de la ventana

  function ocultar() {
    for (const enlace of document.querySelectorAll('a[href]')) {
      const href = enlace.href || '';
      if (!TIENDAS.some((t) => href.includes(t))) continue;

      const limite = innerWidth * innerHeight * AREA_MAXIMA;
      let objetivo = enlace;
      let nodo = enlace.parentElement;
      // Sube hasta la tarjeta que lo envuelve, sin pasarse de tamaño.
      for (let i = 0; i < 6 && nodo && nodo !== document.body; i++) {
        const c = nodo.getBoundingClientRect();
        if (c.width * c.height > limite) break;
        objetivo = nodo;
        nodo = nodo.parentElement;
      }
      objetivo.style.display = 'none';
    }
  }

  const arrancar = () => {
    ocultar();
    // WhatsApp vuelve a pintar la bienvenida al cambiar de chat.
    new MutationObserver(ocultar).observe(document.body, { childList: true, subtree: true });
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

pub fn disguise_script() -> String {
    format!(
        r#"(function () {{
  const define = (prop, value) => {{
    try {{
      Object.defineProperty(navigator, prop, {{ get: () => value, configurable: true }});
    }} catch (e) {{ /* si la propiedad no es redefinible, se deja como está */ }}
  }};

  // El motivo de todo esto: WebKit responde "Apple Computer, Inc.".
  define('vendor', 'Google Inc.');

  const brands = [
    {{ brand: 'Not_A Brand', version: '24' }},
    {{ brand: 'Chromium', version: '{v}' }},
    {{ brand: 'Google Chrome', version: '{v}' }},
  ];
  const platform = navigator.platform.indexOf('Win') === 0
    ? 'Windows'
    : navigator.platform.indexOf('Mac') === 0
    ? 'macOS'
    : 'Linux';

  // WebKit no implementa userAgentData; sin él, algunos detectores descartan
  // Chrome pese al user-agent.
  if (!navigator.userAgentData) {{
    define('userAgentData', {{
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
        platformVersion: '6.0.0',
        uaFullVersion: '{v}.0.0.0',
      }}),
      toJSON: () => ({{ brands, mobile: false, platform }}),
    }});
  }}
}})();"#,
        v = CHROME_VERSION
    )
}
