// Wrusp — lógica de la ventana de gestión (sin frameworks).
// Habla con el backend Rust vía window.__TAURI__ (withGlobalTauri).

const { invoke } = window.__TAURI__.core;

const listEl = document.getElementById("account-list");
const emptyEl = document.getElementById("empty");
const formEl = document.getElementById("add-form");
const nameEl = document.getElementById("add-name");
const themeButtons = document.querySelectorAll(".theme-switch button");

// ── Menú de secciones ───────────────────────────────────
// Los ajustes eran una sola columna con todo apilado y medio escondido en
// <details>; ahora cada cosa vive en su panel y el menú decide cuál se ve. La
// elección se recuerda para que volver a ajustes no obligue a buscar otra vez.
const navButtons = document.querySelectorAll(".nav button");
const PANEL_POR_DEFECTO = "cuentas";

function showPanel(name) {
  let encontrado = false;
  for (const btn of navButtons) {
    const activo = btn.dataset.panel === name;
    encontrado = encontrado || activo;
    btn.classList.toggle("active", activo);
    btn.setAttribute("aria-selected", String(activo));
    document.getElementById(`panel-${btn.dataset.panel}`).hidden = !activo;
  }
  if (!encontrado) return showPanel(PANEL_POR_DEFECTO);
  try {
    localStorage.setItem("wrusp-panel", name);
  } catch {
    // Sin almacenamiento se pierde el recuerdo, no la navegación.
  }
}

navButtons.forEach((btn) =>
  btn.addEventListener("click", () => showPanel(btn.dataset.panel))
);

const media = window.matchMedia("(prefers-color-scheme: dark)");
let themeMode = "system";

// ── Tema ────────────────────────────────────────────────
function applyThemeLocally() {
  const effective =
    themeMode === "system" ? (media.matches ? "dark" : "light") : themeMode;
  document.documentElement.dataset.theme = effective;
  themeButtons.forEach((btn) =>
    btn.classList.toggle("active", btn.dataset.mode === themeMode)
  );
}

media.addEventListener("change", () => {
  if (themeMode === "system") applyThemeLocally();
});

themeButtons.forEach((btn) => {
  btn.addEventListener("click", async () => {
    themeMode = btn.dataset.mode;
    applyThemeLocally();
    try {
      await invoke("set_theme", { theme: themeMode });
    } catch (err) {
      console.error("set_theme:", err);
    }
  });
});

// ── Icono de la aplicación ──────────────────────────────
const iconGrid = document.getElementById("icon-grid");
const iconSearch = document.getElementById("icon-search");
const iconCurrent = document.getElementById("icon-current");
let iconNames = [];
let selectedIcon = "";

function renderIconGrid() {
  const query = iconSearch.value.trim().toLowerCase();
  const visible = query
    ? iconNames.filter((n) => n.includes(query))
    : iconNames;
  iconGrid.replaceChildren(
    ...visible.map((name) => {
      const btn = document.createElement("button");
      btn.title = name;
      btn.classList.toggle("active", name === selectedIcon);
      const img = document.createElement("img");
      img.loading = "lazy";
      img.src = `appicons/${name}.svg`;
      img.alt = name;
      btn.appendChild(img);
      btn.addEventListener("click", async () => {
        try {
          await invoke("set_app_icon", { name });
          selectedIcon = name;
          iconCurrent.src = `appicons/${name}.svg`;
          renderIconGrid();
        } catch (err) {
          console.error("set_app_icon:", err);
        }
      });
      return btn;
    })
  );
}

async function initIconPicker() {
  try {
    iconNames = await (await fetch("appicons/manifest.json")).json();
    // El catálogo tiene que ser una lista. Si llega cualquier otra cosa,
    // `renderIconGrid` revienta fuera de este try y se lleva por delante lo
    // que el arranque hace después: carpetas, interruptores, «Acerca de» y
    // diagnóstico se quedaban sin inicializar, en silencio.
    if (!Array.isArray(iconNames)) throw new Error("el catálogo no es una lista");
    selectedIcon = await invoke("get_app_icon");
    iconCurrent.src = `appicons/${selectedIcon}.svg`;
    iconSearch.addEventListener("input", renderIconGrid);
    renderIconGrid();
  } catch (err) {
    iconNames = [];
    console.error("icon picker:", err);
  }
}

// ── Carpetas (descargas y temporales) ───────────────────
const folderError = document.getElementById("folder-error");

const FOLDER_FIELDS = [
  { id: "download-dir", command: "set_download_dir" },
  { id: "temp-dir", command: "set_temp_dir" },
  { id: "log-dir", command: "set_log_dir" },
];

function showFolderError(message) {
  folderError.textContent = message;
  folderError.hidden = !message;
}

async function saveFolder(field, value) {
  try {
    await invoke(field.command, { path: value.trim() });
    showFolderError("");
    return true;
  } catch (err) {
    showFolderError(String(err));
    return false;
  }
}

async function initFolders() {
  let folders;
  try {
    folders = await invoke("get_folders");
  } catch (err) {
    console.error("get_folders:", err);
    return;
  }

  const defaults = {
    "download-dir": folders.downloadDefault,
    "temp-dir": folders.tempDefault,
    "log-dir": folders.logDefault,
  };
  const values = {
    "download-dir": folders.downloadDir,
    "temp-dir": folders.tempDir,
    "log-dir": folders.logDir,
  };

  // El registro se consulta abriendo su carpeta en el gestor de ficheros.
  document.getElementById("open-logs").addEventListener("click", async () => {
    try {
      await invoke("open_log_dir");
    } catch (err) {
      showFolderError(String(err));
    }
  });

  for (const field of FOLDER_FIELDS) {
    const input = document.getElementById(field.id);
    input.value = values[field.id] || "";
    input.placeholder = defaults[field.id] || "";

    // Se guarda al salir del campo, no en cada tecla: validar rutas a medio
    // escribir solo produce errores molestos.
    input.addEventListener("blur", () => saveFolder(field, input.value));
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") input.blur();
    });

    const button = document.querySelector(`[data-pick="${field.id}"]`);
    button.addEventListener("click", async () => {
      let picked;
      try {
        picked = await invoke("pick_folder");
      } catch (err) {
        console.error("pick_folder:", err);
        return;
      }
      if (!picked) return;
      input.value = picked;
      await saveFolder(field, picked);
    });
  }
}

// ── Interruptores de comportamiento ─────────────────────
async function initToggles() {
  let toggles;
  try {
    toggles = await invoke("get_toggles");
  } catch (err) {
    console.error("get_toggles:", err);
    return;
  }

  for (const input of document.querySelectorAll("[data-toggle]")) {
    const name = input.dataset.toggle;
    input.checked = Boolean(toggles[name]);
    input.addEventListener("change", async () => {
      try {
        await invoke("set_toggle", { name, enabled: input.checked });
      } catch (err) {
        console.error("set_toggle:", err);
      }
    });
  }
}

// ── Acerca de ───────────────────────────────────────────
/** Compara dos versiones «x.y.z». Devuelve true si `a` es mayor que `b`. */
function esMayor(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) > (pb[i] || 0);
  }
  return false;
}

/** Arquitecturas y los nombres con que aparecen en los ficheros publicados. */
const ARQUITECTURAS = {
  x86_64: ["x86_64", "x64", "amd64"],
  aarch64: ["aarch64", "arm64"],
};

/**
 * El fichero de la versión que le sirve a este sistema, o null si no hay.
 *
 * `sufijo` lo decide Rust leyendo la distribución (ver `package_suffix`).
 * Cada versión publica nueve ficheros y varios pueden compartir extensión
 * entre arquitecturas, así que uno que anuncie una arquitectura distinta de
 * la nuestra se descarta: es peor bajar el paquete equivocado que no bajar
 * ninguno.
 */
function elegirPaquete(assets, sufijo, arquitectura) {
  if (!sufijo) return null;
  const mias = ARQUITECTURAS[arquitectura] || [];
  const ajenas = Object.entries(ARQUITECTURAS)
    .filter(([arch]) => arch !== arquitectura)
    .flatMap(([, nombres]) => nombres);
  const candidatos = assets.filter((a) => a.name && a.name.endsWith(sufijo));
  const nombra = (nombre, lista) =>
    lista.some((token) => nombre.toLowerCase().includes(token));
  return (
    candidatos.find((a) => nombra(a.name, mias)) ||
    candidatos.find((a) => !nombra(a.name, ajenas)) ||
    null
  );
}

/** Tamaño en bytes como lo lee una persona. */
function tamano(bytes) {
  if (!bytes) return "tamaño desconocido";
  const mib = bytes / (1024 * 1024);
  return `${mib.toFixed(1).replace(".", ",")} MiB`;
}

async function initAbout() {
  let about;
  try {
    about = await invoke("get_about");
  } catch (err) {
    console.error("get_about:", err);
    return;
  }
  document.getElementById("about-version").textContent = about.version;

  for (const btn of document.querySelectorAll("[data-link]")) {
    btn.addEventListener("click", () => {
      invoke("open_external", { url: about[btn.dataset.link] }).catch((err) =>
        console.error("open_external:", err)
      );
    });
  }

  const estado = document.getElementById("about-update");
  const prompt = document.getElementById("update-prompt");
  const titulo = document.getElementById("update-title");
  const detalle = document.getElementById("update-detail");
  const botones = document.getElementById("update-download").parentElement;

  const abrir = (url) =>
    invoke("open_external", { url }).catch((err) => {
      estado.textContent = "· no se pudo abrir el navegador";
      estado.className = "about-update error";
      console.error("open_external:", err);
    });

  document.getElementById("update-later").addEventListener("click", () => {
    prompt.hidden = true;
  });

  document.getElementById("check-updates").addEventListener("click", async () => {
    estado.textContent = "· comprobando…";
    estado.className = "about-update";
    prompt.hidden = true;
    botones.hidden = false;
    try {
      // Se consulta desde aquí y no desde Rust para no arrastrar un cliente
      // HTTP al binario solo para esto.
      const r = await fetch(
        "https://api.github.com/repos/Aleixenandros/Wrusp/releases/latest",
        { headers: { Accept: "application/vnd.github+json" } }
      );
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const release = await r.json();
      const ultima = release.tag_name.replace(/^v/, "");
      if (!esMayor(ultima, about.version)) {
        estado.textContent = "· estás al día";
        return;
      }
      // Hay versión nueva: se avisa y se pregunta. La descarga no empieza
      // sola, ni aquí ni en el navegador, hasta que se pulse «Descargar».
      estado.textContent = `· hay una versión ${ultima}`;
      estado.className = "about-update nueva";
      const paquete = elegirPaquete(release.assets || [], about.packageSuffix, about.arch);
      titulo.textContent = `Wrusp ${ultima} ya está disponible. ¿Quieres descargarla?`;
      detalle.textContent = paquete
        ? `Se descargará ${paquete.name} (${tamano(paquete.size)}), que es el paquete de este sistema. La descarga la hace tu navegador.`
        : "No hay un paquete reconocible para este sistema, así que se abrirá la lista de descargas de la versión.";
      const notas = release.html_url || about.releases;
      document.getElementById("update-notes").onclick = () => abrir(notas);
      document.getElementById("update-download").onclick = async () => {
        await abrir(paquete ? paquete.browser_download_url : about.releases);
        // La descarga vive ya en el navegador: aquí solo queda decir qué se
        // ha pedido y cómo termina de instalarse.
        botones.hidden = true;
        detalle.textContent = paquete
          ? `Descarga abierta en el navegador: ${paquete.name}. Cuando termine, ábrela para instalar la versión nueva.`
          : "Se ha abierto en el navegador la lista de descargas de la versión.";
      };
      prompt.hidden = false;
    } catch (err) {
      estado.textContent = "· no se pudo comprobar";
      estado.className = "about-update error";
      console.error("comprobar actualizaciones:", err);
    }
  });
}

// ── Cuentas ─────────────────────────────────────────────
const ACCOUNT_COLORS = [
  "",
  "#1fa855",
  "#0ea5e9",
  "#6366f1",
  "#a855f7",
  "#ec4899",
  "#f59e0b",
  "#ef4444",
];

function initials(name) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join("");
}

function accountRow(account, index, allAccounts) {
  const li = document.createElement("li");
  li.className = "account";

  const reorderDiv = document.createElement("div");
  reorderDiv.className = "reorder-btns";

  const upBtn = document.createElement("button");
  upBtn.type = "button";
  upBtn.className = "icon-btn reorder-btn";
  upBtn.title = "Subir";
  upBtn.disabled = index === 0;
  upBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M18 15l-6-6-6 6"/></svg>';
  upBtn.addEventListener("click", async () => {
    const newOrder = allAccounts.map((a) => a.id);
    [newOrder[index - 1], newOrder[index]] = [newOrder[index], newOrder[index - 1]];
    try {
      await invoke("reorder_accounts", { ids: newOrder });
      await refresh();
    } catch (err) {
      console.error("reorder_accounts:", err);
    }
  });

  const downBtn = document.createElement("button");
  downBtn.type = "button";
  downBtn.className = "icon-btn reorder-btn";
  downBtn.title = "Bajar";
  downBtn.disabled = index === allAccounts.length - 1;
  downBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>';
  downBtn.addEventListener("click", async () => {
    const newOrder = allAccounts.map((a) => a.id);
    [newOrder[index + 1], newOrder[index]] = [newOrder[index], newOrder[index + 1]];
    try {
      await invoke("reorder_accounts", { ids: newOrder });
      await refresh();
    } catch (err) {
      console.error("reorder_accounts:", err);
    }
  });
  reorderDiv.append(upBtn, downBtn);

  const avatar = document.createElement("div");
  avatar.className = "avatar";
  avatar.textContent = initials(account.name) || "?";
  if (account.color) {
    avatar.style.backgroundColor = account.color;
  }

  const name = document.createElement("div");
  name.className = "name";
  name.textContent = account.name;
  name.title = "Doble clic para renombrar";

  const actions = document.createElement("div");
  actions.className = "actions";

  // Botón para rotar color de acento
  const colorBtn = document.createElement("button");
  colorBtn.type = "button";
  colorBtn.className = "icon-btn color-btn";
  colorBtn.title = "Cambiar color de acento";
  colorBtn.style.backgroundColor = account.color || "var(--accent)";
  colorBtn.addEventListener("click", async () => {
    const currentIdx = ACCOUNT_COLORS.indexOf(account.color || "");
    const nextColor = ACCOUNT_COLORS[(currentIdx + 1) % ACCOUNT_COLORS.length];
    try {
      await invoke("set_account_color", { id: account.id, color: nextColor || null });
      await refresh();
    } catch (err) {
      console.error("set_account_color:", err);
    }
  });

  // Botón para silenciar
  const muteBtn = document.createElement("button");
  muteBtn.type = "button";
  muteBtn.className = "icon-btn mute-btn" + (account.muted ? " active" : "");
  muteBtn.title = account.muted ? "Desactivar silencio" : "Silenciar notificaciones";
  muteBtn.innerHTML = account.muted
    ? '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M11 5L6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6"/></svg>'
    : '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M11 5L6 9H2v6h4l5 4V5zM19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"/></svg>';
  muteBtn.addEventListener("click", async () => {
    try {
      await invoke("set_account_muted", { id: account.id, muted: !account.muted });
      await refresh();
    } catch (err) {
      console.error("set_account_muted:", err);
    }
  });

  const openBtn = document.createElement("button");
  openBtn.type = "button";
  openBtn.className = "open";
  openBtn.textContent = "Abrir";
  openBtn.addEventListener("click", async () => {
    try {
      await invoke("open_account", { id: account.id });
    } catch (err) {
      console.error("open_account:", err);
    }
  });

  // Borrado en dos pasos para no depender de diálogos nativos.
  const delBtn = document.createElement("button");
  delBtn.type = "button";
  delBtn.className = "danger";
  delBtn.textContent = "Borrar";
  let armed = false;
  delBtn.addEventListener("click", async () => {
    if (!armed) {
      armed = true;
      delBtn.textContent = "¿Seguro?";
      setTimeout(() => {
        armed = false;
        delBtn.textContent = "Borrar";
      }, 3000);
      return;
    }
    try {
      await invoke("remove_account", { id: account.id });
    } catch (err) {
      console.error("remove_account:", err);
    }
    await refresh();
  });

  // Renombrar con doble clic sobre el nombre.
  name.addEventListener("dblclick", () => {
    const input = document.createElement("input");
    input.type = "text";
    input.className = "rename";
    input.maxLength = 40;
    input.value = account.name;
    li.replaceChild(input, name);
    input.focus();
    input.select();

    const commit = async () => {
      const value = input.value.trim();
      if (value && value !== account.name) {
        try {
          await invoke("rename_account", { id: account.id, name: value });
        } catch (err) {
          console.error("rename_account:", err);
        }
      }
      await refresh();
    };
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") input.blur();
      if (ev.key === "Escape") {
        input.value = account.name;
        input.blur();
      }
    });
    input.addEventListener("blur", commit, { once: true });
  });

  actions.append(colorBtn, muteBtn, openBtn, delBtn);
  li.append(reorderDiv, avatar, name, actions);
  return li;
}

async function refresh() {
  try {
    const accounts = await invoke("list_accounts");
    listEl.replaceChildren(...accounts.map((acc, idx) => accountRow(acc, idx, accounts)));
    emptyEl.hidden = accounts.length > 0;
  } catch (err) {
    console.error("list_accounts:", err);
  }
}

// ── Diagnóstico ─────────────────────────────────────────
function formatBytes(bytes) {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return (bytes / Math.pow(k, i)).toFixed(1) + " " + sizes[i];
}

async function loadDiagnostics() {
  try {
    const diag = await invoke("get_diagnostics");
    const h264El = document.getElementById("diag-h264");
    const h264DetailEl = document.getElementById("diag-h264-detail");
    if (diag.hasH264Decoder) {
      h264El.textContent = "Disponible";
      h264El.className = "diag-status ok";
      h264DetailEl.textContent = diag.h264DecoderName;
    } else {
      h264El.textContent = "No disponible";
      h264El.className = "diag-status error";
      h264DetailEl.textContent = "Instala gstreamer1-plugin-libav / gstreamer1.0-libav";
    }

    const aacEl = document.getElementById("diag-aac");
    if (diag.hasAacDecoder) {
      aacEl.textContent = "Disponible";
      aacEl.className = "diag-status ok";
    } else {
      aacEl.textContent = "Integrado o nativo";
      aacEl.className = "diag-status ok";
    }

    document.getElementById("diag-webkit").textContent = diag.webkitVersion;
    document.getElementById("diag-os").textContent = diag.osInfo;
    document.getElementById("diag-profiles").textContent = formatBytes(diag.profilesSize);
    document.getElementById("diag-logs").textContent = formatBytes(diag.logSize);
  } catch (err) {
    console.error("get_diagnostics:", err);
  }
}

function initDiagnostics() {
  document.getElementById("refresh-diagnostics").addEventListener("click", loadDiagnostics);
  const statusEl = document.getElementById("diag-action-status");
  document.getElementById("clear-gst-cache").addEventListener("click", async () => {
    statusEl.textContent = "Limpiando…";
    try {
      await invoke("clear_gstreamer_cache");
      statusEl.textContent = "Caché de GStreamer borrada con éxito.";
      statusEl.style.color = "var(--accent)";
      await loadDiagnostics();
    } catch (err) {
      statusEl.textContent = "Error: " + err;
      statusEl.style.color = "var(--danger)";
    }
  });
}

// Lo llama Rust cuando se pulsa «+» en la barra lateral. El campo puede estar
// en un panel que no se ve, así que primero se enseña el suyo.
window.__wruspFocusAdd = () => {
  showPanel("cuentas");
  nameEl.focus();
  nameEl.select();
};

formEl.addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const name = nameEl.value.trim();
  if (!name) return;
  try {
    await invoke("add_account", { name });
    nameEl.value = "";
    await refresh();
  } catch (err) {
    console.error("add_account:", err);
  }
});

// ── Arranque ────────────────────────────────────────────
(async function init() {
  let recordado = null;
  try {
    recordado = localStorage.getItem("wrusp-panel");
  } catch {
    recordado = null;
  }
  showPanel(recordado || PANEL_POR_DEFECTO);

  try {
    themeMode = await invoke("get_theme");
  } catch {
    themeMode = "system";
  }
  applyThemeLocally();
  await initIconPicker();
  await initFolders();
  await initToggles();
  await initAbout();
  initDiagnostics();
  await loadDiagnostics();
  await refresh();
})();
