/* ============================================================
   Meeting Scribe — shared shell behavior
   Theme, sidebar tree (rendered from data, expand state
   persisted), context menu, move-to-folder modal.
   ============================================================ */

window.MS = (function () {
  "use strict";

  /* ---------- theme ---------- */

  const THEME_KEY = "ms-theme";

  function applyTheme(t) {
    document.documentElement.setAttribute("data-theme", t);
    document.body && document.body.setAttribute("data-theme", t);
    try { localStorage.setItem(THEME_KEY, t); } catch (_) {}
    document.querySelectorAll(".js-theme").forEach(function (b) {
      b.innerHTML = t === "dark" ? ICONS.sun : ICONS.moon;
      b.setAttribute("data-tip", t === "dark" ? "Tema claro" : "Tema escuro");
    });
  }

  function initTheme() {
    let t = "light";
    try { t = localStorage.getItem(THEME_KEY) || "light"; } catch (_) {}
    applyTheme(t);
    document.addEventListener("click", function (e) {
      const b = e.target.closest(".js-theme");
      if (!b) return;
      const cur = document.body.getAttribute("data-theme") || "light";
      applyTheme(cur === "dark" ? "light" : "dark");
    });
  }

  /* ---------- icons (16px, stroke) ---------- */

  const ICONS = {
    chev: '<svg width="10" height="10" viewBox="0 0 10 10" fill="none"><path d="M3.5 2l3 3-3 3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    folder: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h2.9l1.5 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4v-7.6z" stroke="currentColor" stroke-width="1.2"/></svg>',
    doc: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><rect x="3" y="2" width="10" height="12" rx="1.5" stroke="currentColor" stroke-width="1.2"/><path d="M5.6 5.5h4.8M5.6 8h4.8M5.6 10.5h2.8" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    gear: '<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="2.2" stroke="currentColor" stroke-width="1.2"/><path d="M8 1.8v1.7M8 12.5v1.7M1.8 8h1.7M12.5 8h1.7M3.6 3.6l1.2 1.2M11.2 11.2l1.2 1.2M12.4 3.6l-1.2 1.2M4.8 11.2l-1.2 1.2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    panel: '<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.5" width="12.4" height="11" rx="1.6" stroke="currentColor" stroke-width="1.2"/><path d="M6 2.5v11" stroke="currentColor" stroke-width="1.2"/></svg>',
    refresh: '<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M13.2 6.6A5.4 5.4 0 003.4 4.9M2.8 9.4a5.4 5.4 0 009.8 1.7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><path d="M3.2 2.4v2.8H6M12.8 13.6v-2.8H10" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    moon: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M13.4 9.6A5.6 5.6 0 016.4 2.6a5.6 5.6 0 107 7z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>',
    sun: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="3" stroke="currentColor" stroke-width="1.2"/><path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M12.6 3.4l-1.1 1.1M4.5 11.5l-1.1 1.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    mic: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><rect x="5.6" y="1.8" width="4.8" height="8" rx="2.4" stroke="currentColor" stroke-width="1.2"/><path d="M3.2 7.6a4.8 4.8 0 009.6 0M8 12.4v2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>'
  };

  /* ---------- sidebar data (mirrors ~/Documents/Meeting Scribe) ---------- */

  const TREE = [
    { type: "folder", id: "f-setare", name: "Setare", children: [
      { type: "meeting", id: "m-prov", title: "2026-07-17 - Alinhamento de arquitetura de providers", href: "meeting.html" },
      { type: "meeting", id: "m-otp", title: "2026-07-10 - Revisão do fluxo de OTP" }
    ]},
    { type: "folder", id: "f-taxi", name: "Taxi Executivo", children: [
      { type: "folder", id: "f-fase0", name: "Fase 0", children: [
        { type: "meeting", id: "m-spike", title: "2026-07-08 - Spike Flutter: decisão de stack" }
      ]},
      { type: "meeting", id: "m-kickoff", title: "2026-06-30 - Kickoff do MVP" }
    ]},
    { type: "folder", id: "f-pessoal", name: "Pessoal", children: [
      { type: "meeting", id: "m-mentoria", title: "2026-06-24 - Mentoria de carreira" }
    ]},
    { type: "unfiled", id: "g-unfiled", name: "Sem pasta", children: [
      { type: "meeting", id: "m-retro", title: null, placeholder: "Gravação de 17/07/2026 14:02", href: "meeting-empty.html" }
    ]}
  ];

  const OPEN_KEY = "ms-tree-open";

  function openState() {
    try { return JSON.parse(localStorage.getItem(OPEN_KEY)) || { "f-setare": 1, "g-unfiled": 1 }; }
    catch (_) { return { "f-setare": 1, "g-unfiled": 1 }; }
  }
  function saveOpen(s) { try { localStorage.setItem(OPEN_KEY, JSON.stringify(s)); } catch (_) {} }

  function esc(s) {
    return String(s).replace(/[&<>"]/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });
  }

  function renderNode(node, activeId, open, depth) {
    if (node.type === "meeting") {
      const label = node.title
        ? '<span class="t-name">' + esc(node.title) + "</span>"
        : '<span class="t-name placeholder">' + esc(node.placeholder) + '</span><span class="pending-dot" data-tip="Aguardando primeiro resumo"></span>';
      const cls = "tree-item" + (node.id === activeId ? " active" : "");
      const href = node.href || "#";
      return '<a class="' + cls + '" href="' + href + '" data-id="' + node.id + '" data-kind="meeting">' +
        '<span class="t-ico">' + ICONS.doc + "</span>" + label + "</a>";
    }
    const isOpen = !!open[node.id];
    const kids = (node.children || [])
      .map(function (c) { return renderNode(c, activeId, open, depth + 1); })
      .join("");
    const count = (node.children || []).length;
    return '<div class="tree-group' + (isOpen ? " open" : "") + '" data-id="' + node.id + '">' +
      '<button class="tree-item" data-id="' + node.id + '" data-kind="' + node.type + '" data-count="' + count + '">' +
      '<span class="chev">' + ICONS.chev + "</span>" +
      '<span class="t-ico">' + ICONS.folder + "</span>" +
      '<span class="t-name">' + esc(node.name) + "</span>" +
      "</button>" +
      '<div class="tree-children">' + kids + "</div></div>";
  }

  function renderSidebar(activeId) {
    const el = document.getElementById("sidebar");
    if (!el) return;
    const open = openState();
    el.innerHTML =
      '<div class="titlebar"><div class="traffic"><i></i><i></i><i></i></div></div>' +
      '<div class="sidebar-head"><a class="btn-record" href="recording.html"><span class="dot"></span>Nova reunião</a></div>' +
      '<div class="tree-label"><span>Reuniões</span>' +
      '<button class="icon-btn js-refresh" style="width:22px;height:22px" data-tip="Sincronizar com o Finder">' + ICONS.refresh + "</button></div>" +
      '<nav class="tree" id="tree" aria-label="Pastas de reuniões">' +
      TREE.map(function (n) { return renderNode(n, activeId, open, 0); }).join("") +
      "</nav>" +
      '<div class="sidebar-foot">' +
      '<a class="icon-btn" href="settings.html" data-tip="Configurações">' + ICONS.gear + "</a>" +
      '<button class="icon-btn js-theme" data-tip="Tema"></button>' +
      '<span class="spacer"></span>' +
      '<button class="icon-btn js-collapse" data-tip="Recolher barra lateral">' + ICONS.panel + "</button>" +
      "</div>";

    el.addEventListener("click", function (e) {
      const refresh = e.target.closest(".js-refresh");
      if (refresh) {
        refresh.style.animation = "spin 0.7s linear 2";
        setTimeout(function () { refresh.style.animation = ""; }, 1450);
        return;
      }
      const collapse = e.target.closest(".js-collapse");
      if (collapse) {
        document.querySelector(".app").classList.add("sidebar-hidden");
        return;
      }
      const item = e.target.closest(".tree-item");
      if (!item) return;
      if (item.dataset.kind === "folder" || item.dataset.kind === "unfiled") {
        const g = item.closest(".tree-group");
        g.classList.toggle("open");
        const s = openState();
        if (g.classList.contains("open")) s[g.dataset.id] = 1; else delete s[g.dataset.id];
        saveOpen(s);
      }
      if (item.dataset.kind === "meeting" && item.getAttribute("href") === "#") {
        e.preventDefault();
      }
    });

    el.addEventListener("contextmenu", function (e) {
      const item = e.target.closest(".tree-item");
      if (!item) return;
      e.preventDefault();
      openCtx(e.clientX, e.clientY, item);
    });
  }

  /* ---------- context menu ---------- */

  let ctxEl = null;

  function closeCtx() { if (ctxEl) { ctxEl.remove(); ctxEl = null; } }

  function openCtx(x, y, item) {
    closeCtx();
    const kind = item.dataset.kind;
    const isFolder = kind === "folder" || kind === "unfiled";
    const count = parseInt(item.dataset.count || "0", 10);
    const canDelete = !isFolder || count === 0;
    let html = "";
    if (isFolder) {
      html += "<button data-act='new-folder'>Nova pasta</button>";
      html += "<button data-act='rename'>Renomear</button>";
      html += "<hr/>";
      html += canDelete
        ? "<button data-act='delete' class='destructive'>Excluir pasta</button>"
        : "<button disabled>Excluir pasta<span class='why'>Disponível apenas para pastas vazias — mova as " + count + " reuniões antes.</span></button>";
    } else {
      html += "<button data-act='rename'>Renomear</button>";
      html += "<button data-act='move'>Mover para pasta…</button>";
      html += "<hr/>";
      html += "<button data-act='reveal'>Revelar no Finder</button>";
      html += "<button data-act='delete' class='destructive'>Excluir reunião</button>";
    }
    ctxEl = document.createElement("div");
    ctxEl.className = "ctx";
    ctxEl.innerHTML = html;
    document.body.appendChild(ctxEl);
    const r = ctxEl.getBoundingClientRect();
    ctxEl.style.left = Math.min(x, window.innerWidth - r.width - 10) + "px";
    ctxEl.style.top = Math.min(y, window.innerHeight - r.height - 10) + "px";
    ctxEl.addEventListener("click", function (e) {
      const b = e.target.closest("button[data-act]");
      if (!b) return;
      if (b.dataset.act === "move") openModal("modal-move");
      if (b.dataset.act === "rename") startRename(item);
      closeCtx();
    });
  }

  function startRename(item) {
    const nameEl = item.querySelector(".t-name");
    const old = nameEl.textContent;
    const input = document.createElement("input");
    input.className = "input";
    input.style.cssText = "padding:2px 6px;font-size:12.5px;height:24px";
    input.value = old;
    nameEl.replaceWith(input);
    input.focus();
    input.select();
    function commit(keep) {
      const span = document.createElement("span");
      span.className = "t-name";
      span.textContent = keep && input.value.trim() ? input.value.trim() : old;
      input.replaceWith(span);
    }
    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter") commit(true);
      if (e.key === "Escape") commit(false);
      e.stopPropagation();
    });
    input.addEventListener("blur", function () { commit(true); });
    input.addEventListener("click", function (e) { e.preventDefault(); e.stopPropagation(); });
  }

  /* ---------- modals ---------- */

  function openModal(id) {
    const m = document.getElementById(id);
    if (m) m.classList.add("open");
  }
  function closeModal(id) {
    const m = document.getElementById(id);
    if (m) m.classList.remove("open");
  }

  function injectMoveModal() {
    if (document.getElementById("modal-move")) return;
    const d = document.createElement("div");
    d.className = "overlay";
    d.id = "modal-move";
    d.innerHTML =
      '<div class="modal" role="dialog" aria-label="Mover para pasta">' +
      '<div class="modal-head"><h3>Mover para pasta</h3>' +
      "<p>A reunião será movida no disco — a mudança aparece também no Finder.</p></div>" +
      '<div class="modal-body"><div class="pick-tree">' +
      '<label><input type="radio" name="mv" checked/>' + ICONS.folder + " Setare</label>" +
      '<label><input type="radio" name="mv"/>' + ICONS.folder + " Taxi Executivo</label>" +
      '<label class="indent-1"><input type="radio" name="mv"/>' + ICONS.folder + " Fase 0</label>" +
      '<label><input type="radio" name="mv"/>' + ICONS.folder + " Pessoal</label>" +
      '<label><input type="radio" name="mv"/>' + ICONS.doc + " Sem pasta (raiz)</label>" +
      "</div></div>" +
      '<div class="modal-foot">' +
      '<button class="btn ghost js-close">Cancelar</button>' +
      '<button class="btn primary js-close">Mover</button>' +
      "</div></div>";
    document.body.appendChild(d);
  }

  /* ---------- global wiring ---------- */

  function init(opts) {
    opts = opts || {};
    initTheme();
    renderSidebar(opts.active || null);
    injectMoveModal();

    document.addEventListener("click", function (e) {
      if (ctxEl && !e.target.closest(".ctx")) closeCtx();
      const closeBtn = e.target.closest(".js-close");
      if (closeBtn) closeBtn.closest(".overlay").classList.remove("open");
      const showSidebar = e.target.closest(".js-show-sidebar");
      if (showSidebar) document.querySelector(".app").classList.remove("sidebar-hidden");
      const overlay = e.target.classList && e.target.classList.contains("overlay") ? e.target : null;
      if (overlay) overlay.classList.remove("open");
    });

    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape") {
        closeCtx();
        document.querySelectorAll(".overlay.open").forEach(function (m) { m.classList.remove("open"); });
      }
    });
  }

  return { init: init, openModal: openModal, closeModal: closeModal, icons: ICONS };
})();
