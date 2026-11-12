// The console's own script: no library. Pages are complete without it; this adds what a script can: controls that need one
// (copy), fragments that refetch when the server says they changed, forms that swap their answer in place, drawers and
// dialogs that open over the page they were asked from, and the banner when the stream is lost. Every name below is defined in src/ui/client.rs, and a test checks that they agree.
// Measured by the D4a spike (docs/port/phase9/client-spike.md); the rules it found are marked (spike n).
(() => {
  const attr = (el, name) => (el ? el.getAttribute(name) : null);
  const banner = () => document.getElementById('stream-banner');
  const reveal = () => document.querySelectorAll('[data-js-only]').forEach((el) => { el.hidden = false; });
  reveal();
  // Motion only for people who have not asked for less of it.
  const reduced = () => !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);

  // Copy: the text of the button, or of the element it names. "Copy as" changes the form of the address, for the buttons
  // that carry the address (the client snippets, data-copy-forms) and for no other: a card that copies its own lines copies those.
  let copyAs = 'code';
  const address = () => attr(document.querySelector('[data-address]'), 'data-address') || '';
  const textOf = (btn) => {
    const own = attr(btn, 'data-copy');
    if (own !== null) return own;
    if (btn.hasAttribute('data-copy-forms')) {
      if (copyAs === 'properties') return 'bootstrap.servers=' + address();
      if (copyAs === 'environment') return 'KAFKA_BOOTSTRAP_SERVERS=' + address();
    }
    const from = document.getElementById(attr(btn, 'data-copy-from'));
    return from ? from.textContent : '';
  };
  // The clipboard API exists only in a secure context (https or localhost): over http://<LAN address> it is absent, and the
  // old way (select the text of a field and ask the document to copy it) still works there.
  function copyOld(text) {
    const was = document.activeElement;
    const field = document.createElement('textarea');
    field.value = text;
    field.setAttribute('readonly', '');
    field.style.position = 'fixed';
    field.style.opacity = '0';
    document.body.append(field);
    field.select();
    try { document.execCommand('copy'); } catch (e) { /* nothing to do: the text is on the page */ }
    field.remove();
    if (was && was.focus) was.focus();
  }
  function copy(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).catch(() => copyOld(text));
    else copyOld(text);
  }
  document.addEventListener('click', (e) => {
    const pick = e.target.closest('[data-copy-as]');
    if (pick) {
      copyAs = attr(pick, 'data-copy-as');
      pick.parentElement.querySelectorAll('[data-copy-as]').forEach((b) => b.classList.toggle('on', b === pick));
      return;
    }
    const btn = e.target.closest('[data-copy],[data-copy-from]');
    if (btn) { copy(textOf(btn)); confirmCopy(btn); }
  });
  // A copy says so, gently: the label becomes "Copied" for a moment, in the width it had, and is announced.
  function confirmCopy(btn) {
    if (btn.hasAttribute('data-copied')) return;
    const was = btn.innerHTML, w = btn.getBoundingClientRect ? btn.getBoundingClientRect().width : 0;
    btn.setAttribute('data-copied', '');
    if (w) btn.style.minWidth = w + 'px';
    btn.textContent = 'Copied';
    setTimeout(() => { btn.innerHTML = was; btn.style.minWidth = ''; btn.removeAttribute('data-copied'); }, 1400);
  }

  // Edit the nodes on screen into the new ones, so focus, typed text, caret and scroll position stay. What really changed is
  // remembered, so that only it fades in (below): an unchanged answer changes nothing and moves nothing.
  const changed = new Set();
  function morph(a, b) {
    if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) { if (a.parentElement) changed.add(a.parentElement); return a.replaceWith(b); }
    if (a.nodeType === 3) { if (a.data !== b.data) { a.data = b.data; if (a.parentElement) changed.add(a.parentElement); } return; }
    if (a.nodeType !== 1) return;
    const focused = a === document.activeElement;
    for (const { name, value } of [...b.attributes]) if (a.getAttribute(name) !== value && !(focused && name === 'value')) a.setAttribute(name, value);
    for (const { name } of [...a.attributes]) if (!b.hasAttribute(name)) a.removeAttribute(name);
    if (a.localName === 'input' && !focused && a.value !== b.value) a.value = b.value;
    const ac = [...a.childNodes], bc = [...b.childNodes];
    bc.forEach((n, i) => { if (ac[i]) morph(ac[i], n); else { a.append(n); changed.add(a); } });
    ac.slice(bc.length).forEach((n) => { n.remove(); changed.add(a); });
  }
  // A very short fade (opacity only, 120 ms) on what changed.
  function fade() {
    const els = [...changed].filter((el) => el.isConnected);
    changed.clear();
    if (reduced()) return;
    els.forEach((el) => { if (el.animate) el.animate([{ opacity: 0.45 }, { opacity: 1 }], { duration: 120, easing: 'ease-out' }); });
  }
  // An answer is HTML whose top-level elements have ids: each one morphs into the element with that id.
  function apply(html) {
    const t = document.createElement('template');
    t.innerHTML = html;
    for (const el of t.content.children) { const target = el.id && document.getElementById(el.id); if (target) morph(target, el); }
    fade();
  }

  // A fragment: fetched again on a tick. The script keeps the ETag and sends it (spike 1: a browser turns a 304 into a 200
  // for fetch), and the newest request wins per fragment (spike 4).
  const etags = {}, ctl = {};
  // A fetch that came back (even with 304) says the console's process is there: the banner of a lost connection goes.
  const reached = () => { const b = banner(); if (b) b.hidden = true; };
  async function refresh(id) {
    const url = attr(document.getElementById(id), 'data-frag');
    if (!url) return;
    if (ctl[id]) ctl[id].abort();
    const c = (ctl[id] = new AbortController());
    try {
      const headers = etags[id] ? { 'If-None-Match': etags[id] } : {};
      const r = await fetch(url, { signal: c.signal, headers });
      if (r.status === 304) { reached(); return; }
      if (!r.ok) throw new Error(String(r.status));
      etags[id] = r.headers.get('ETag');
      apply(await r.text());
      reached();
    } catch (e) { const b = banner(); if (e.name !== 'AbortError' && b) b.hidden = false; }
  }

  // A form that is not in an overlay: posted in place; the answer is the fragment named by data-target. If that fails the
  // form posts as it would without us.
  async function postInPlace(f) {
    try {
      const r = await fetch(f.action, { method: 'POST', body: new URLSearchParams(new FormData(f)), headers: { 'X-Fragment': '1' } });
      if (!r.ok) throw new Error(String(r.status));
      apply(await r.text());
    } catch (err) { f.submit(); }
  }

  // ---- Overlays: a drawer or a dialog (src/ui/overlay.rs) opens over the page it was asked from, with no navigation. The
  // address is pushed, so Back closes it; Esc, the scrim and the Close control close it too. The page behind is inert, focus
  // stays inside, and closing returns focus to what opened it. A server-rendered overlay (a direct visit, a reload) is adopted.
  let ovl = null; // { el, opener, pushed, y }
  let swapped = false; // the page was replaced by an answer: the history no longer describes it
  const panelOf = (el) => el.querySelector('[role=dialog]');
  const pageRoot = () => document.querySelector('.root:not([data-overlay])');
  const asOverlay = (html) => {
    const t = document.createElement('template');
    t.innerHTML = html;
    return t.content.querySelector('[data-overlay]');
  };
  const focusables = (p) => [...p.querySelectorAll('a[href],button,input,select,textarea,[tabindex]')].filter((e) => !e.disabled && !e.hidden && e.tabIndex >= 0);
  // Focus goes to the first field of a form, else to the panel itself (its title is read out).
  function focusIn(el) {
    const p = panelOf(el);
    if (!p) return;
    const first = p.querySelector('[autofocus]') || p.querySelector('input:not([type=hidden]):not([readonly]),select,textarea');
    (first || p).focus({ preventScroll: true });
  }
  function mount(el, opener, animate) {
    ovl = { el, opener, pushed: false, y: window.scrollY || 0 };
    document.body.append(el);
    reveal();
    const r = pageRoot();
    if (r) r.setAttribute('inert', '');
    if (animate && !reduced()) el.classList.add('ovl-in');
    focusIn(el);
  }
  // Closing is the opening in reverse and a little faster; with reduced motion it is at once.
  function teardown(now) {
    const o = ovl;
    if (!o) return;
    ovl = null;
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      o.el.remove();
      const r = pageRoot();
      if (r) r.removeAttribute('inert');
      if (window.scrollTo) window.scrollTo(0, o.y);
      if (o.opener && o.opener.isConnected) o.opener.focus({ preventScroll: true });
    };
    if (now || reduced()) return finish();
    o.el.classList.remove('ovl-in');
    o.el.classList.add('ovl-out');
    o.el.addEventListener('animationend', finish, { once: true });
    setTimeout(finish, 200);
  }
  // Close: back to the address that was there, through the history if the script pushed this one.
  function dismiss() {
    if (!ovl) return;
    if (ovl.pushed) history.back();
    else if (ovl.el.hasAttribute('data-transient')) teardown();
    else { history.replaceState(null, '', attr(ovl.el, 'data-close') || location.href); teardown(); }
  }
  // Put an overlay's content in place of the open one (the next record, a form's answer) without closing it.
  function replaceOverlay(el, url) {
    ovl.el.replaceWith(el);
    ovl.el = el;
    reveal();
    if (url) history.replaceState({ ovl: 1 }, '', url);
    focusIn(el);
  }
  async function openOverlay(url, opener, push) {
    let el;
    try {
      const r = await fetch(url, { headers: { 'X-Fragment': '1' } });
      if (!r.headers.get('x-overlay')) throw new Error(String(r.status));
      el = asOverlay(await r.text());
      if (!el) throw new Error('no overlay');
    } catch (err) { location.href = url; return; }
    if (ovl) return replaceOverlay(el, push ? url : null);
    mount(el, opener, true);
    if (push) { history.pushState({ ovl: 1 }, '', url); ovl.pushed = true; }
  }
  // A page came back instead of an overlay (the result of a write): it replaces this one, softly where the browser can.
  function swapPage(html, url) {
    const doc = new DOMParser().parseFromString(html, 'text/html');
    history.replaceState(null, '', url || (ovl && attr(ovl.el, 'data-close')) || location.href);
    swapped = true;
    const put = () => {
      ovl = null;
      document.title = doc.title;
      document.body.setAttribute('data-stream', attr(doc.body, 'data-stream') || '');
      document.body.replaceChildren(...doc.body.childNodes);
      reveal();
      const m = document.getElementById('main');
      if (m) { m.setAttribute('tabindex', '-1'); m.focus({ preventScroll: true }); }
      listen();
    };
    if (document.startViewTransition && !reduced()) document.startViewTransition(put); else put();
  }
  // A form of an overlay: posted in place. An overlay comes back (a refusal, a field to fix) and replaces this one; a page
  // comes back (the result) and replaces the page. If the request cannot be made at all the form posts as it would without us.
  async function submitOverlay(f, submitter) {
    const body = new URLSearchParams(new FormData(f, submitter || undefined));
    f.setAttribute('aria-busy', 'true');
    let r;
    try { r = await fetch(f.action, { method: 'POST', body, headers: { 'X-Fragment': '1' } }); } catch (err) { f.submit(); return; }
    const text = await r.text();
    if (r.headers.get('x-overlay')) { const el = asOverlay(text); if (el) return replaceOverlay(el, null); }
    swapPage(text, r.redirected ? r.url : null);
  }

  document.addEventListener('click', (e) => {
    const closer = e.target.closest('[data-dialog-close],[data-overlay-close]');
    if (closer && ovl && ovl.el.contains(closer)) { e.preventDefault(); dismiss(); return; }
    const a = e.target.closest('a[data-dialog]');
    if (!a || e.defaultPrevented || e.metaKey || e.ctrlKey || e.shiftKey || e.button) return;
    e.preventDefault();
    openOverlay(a.href, a, true);
  });
  document.addEventListener('submit', (e) => {
    const f = e.target.closest ? e.target.closest('form') : null;
    if (!f) return;
    if (ovl && ovl.el.contains(f)) { e.preventDefault(); submitOverlay(f, e.submitter); return; }
    if (f.hasAttribute('data-ajax')) { e.preventDefault(); postInPlace(f); }
  });
  document.addEventListener('keydown', (e) => {
    if (!ovl) return;
    if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); dismiss(); return; }
    if (e.key !== 'Tab') return;
    const p = panelOf(ovl.el), f = focusables(p), at = document.activeElement;
    if (!f.length) { e.preventDefault(); p.focus(); return; }
    const first = f[0], last = f[f.length - 1];
    if (e.shiftKey && (at === first || !p.contains(at))) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && (at === last || !p.contains(at))) { e.preventDefault(); first.focus(); }
  });
  // ---- Jump: ⌘K or Ctrl+K, or a click on the top bar's search, opens the palette (/jump, an overlay with no page behind it of its
  // own: no history entry). Typing narrows the topics, the arrows move, Enter goes there, Esc closes.
  const palette = () => attr(document.querySelector('[data-palette]'), 'data-palette');
  function openPalette() {
    if (ovl) { if (ovl.el.hasAttribute('data-transient')) dismiss(); return; }
    const url = palette();
    if (url) openOverlay(url, document.activeElement, false);
  }
  const jumpLinks = () => (ovl ? [...ovl.el.querySelectorAll('.jump-list li:not([hidden]) a')] : []);
  function jumpSelect(to) {
    const all = jumpLinks();
    all.forEach((a, i) => { a.classList.toggle('sel', i === to); if (i === to) { a.setAttribute('aria-current', 'true'); if (a.scrollIntoView) a.scrollIntoView({ block: 'nearest' }); } else a.removeAttribute('aria-current'); });
  }
  document.addEventListener('input', (e) => {
    if (!ovl || !e.target.hasAttribute || !e.target.hasAttribute('data-jump-input')) return;
    const q = e.target.value.trim().toLowerCase(), items = [...ovl.el.querySelectorAll('.jump-list li')];
    items.forEach((li) => { li.hidden = !!q && !attr(li.firstElementChild, 'data-jump').includes(q); });
    const none = ovl.el.querySelector('.jump-empty');
    if (none) none.hidden = !items.length || items.some((li) => !li.hidden);
    jumpSelect(0);
  });
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && !e.altKey && (e.key === 'k' || e.key === 'K')) { e.preventDefault(); openPalette(); return; }
    if (!ovl || !ovl.el.hasAttribute('data-transient')) return;
    const all = jumpLinks(), at = all.findIndex((a) => a.classList.contains('sel'));
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); jumpSelect(Math.max(0, Math.min(all.length - 1, at + (e.key === 'ArrowDown' ? 1 : -1)))); }
    else if (e.key === 'Enter' && all.length) { e.preventDefault(); (all[Math.max(0, at)]).click(); }
  });
  const search = document.querySelector('[data-palette]');
  if (search) {
    const field = search.querySelector('input');
    if (field) { field.disabled = false; field.readOnly = true; field.addEventListener('keydown', (e) => { if (e.key === 'Enter') openPalette(); }); }
    search.addEventListener('click', () => openPalette());
  }

  // j and k move through the rows of the page's table (a row's first link takes the focus; Enter opens it, as on any link). With
  // a drawer open they are the next and the previous record: the opener's neighbours in its table.
  const typing = (t) => t && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable);
  const rowLinks = () => [...document.querySelectorAll('main table.t tbody tr')].map((tr) => tr.querySelector('a[href]')).filter(Boolean);
  document.addEventListener('keydown', (e) => {
    if ((e.key !== 'j' && e.key !== 'k') || e.metaKey || e.ctrlKey || e.altKey || e.defaultPrevented || typing(e.target)) return;
    const links = rowLinks();
    if (!links.length) return;
    const from = ovl ? ovl.opener : document.activeElement && document.activeElement.closest('tr') ? document.activeElement.closest('tr').querySelector('a[href]') : null;
    const at = links.indexOf(from);
    const next = links[at < 0 ? (e.key === 'j' ? 0 : links.length - 1) : at + (e.key === 'j' ? 1 : -1)];
    e.preventDefault();
    if (!next) return;
    if (ovl) { if (next.hasAttribute('data-dialog')) { ovl.opener = next; openOverlay(next.href, next, true); } return; }
    next.focus();
  });

  // Back and Forward: the history says whether an overlay belongs on this entry.
  addEventListener('popstate', () => {
    if (swapped) { location.reload(); return; }
    const want = !!(history.state && history.state.ovl);
    if (ovl && !want) teardown();
    else if (!ovl && want) openOverlay(location.href, null, false);
  });
  // A server-rendered overlay is adopted: the same closing, the same Back.
  const first = document.querySelector('[data-overlay]');
  if (first) { ovl = { el: first, opener: null, pushed: false, y: 0 }; history.replaceState({ ovl: 1 }, '', location.href); focusIn(first); }

  // The stream: one connection while the page is on screen. A tick names the fragments to fetch again.
  let es = null, wait = 1000, timer = 0;
  const close = () => { clearTimeout(timer); if (es) es.close(); es = null; };
  const connect = () => {
    close();
    const url = attr(document.body, 'data-stream');
    if (!url || !window.EventSource) return;
    es = new EventSource(url);
    es.addEventListener('tick', (e) => e.data.split(',').filter(Boolean).forEach(refresh));
    es.onopen = () => { wait = 1000; const b = banner(); if (b) b.hidden = true; };
    // Reconnect for ever with a growing wait (spike 3): the console left open while the broker is down must come back.
    es.onerror = () => {
      const b = banner();
      if (b) b.hidden = false;
      close();
      timer = setTimeout(connect, wait);
      wait = Math.min(wait * 2, 15000);
    };
  };
  const listen = connect;
  // A page in the back/forward cache or a hidden tab must not hold a stream open: a browser allows 6 connections per origin (spike 2).
  document.addEventListener('visibilitychange', () => (document.hidden ? close() : connect()));
  addEventListener('pagehide', close);
  addEventListener('pageshow', (e) => { if (e.persisted) connect(); });
  connect();
})();
