// The console's product logic, on top of the client stack (htmx 2 + idiomorph + Alpine CSP; docs/port/console/client-stack.md).
//
// What the libraries do and this file does not: htmx boosts every link and form (a navigation swaps `main` and keeps the shell),
// pushes the history, restores a page on Back, preloads on intent and cross-fades with a View Transition; idiomorph edits the page
// into the new one so focus, typed text and scroll stay; Alpine (components.js) runs the Select, Combobox, menu and tooltip.
// What this file keeps, because it is the product: the copy buttons, the Live tape (pause, what is held back, new rows), the
// stream's ticks and fragments (ETag), overlays (a drawer or dialog opens over its page, with its own history entry),
// the ⌘K palette's keys, j and k, and the small seams to the stack: scroll memory, the progress hint, focus after a swap.
// Every name below is defined in src/ui/client.rs, and a test checks that they agree. Tests: tools/dom.
//
// Sections, each standing on the ones above it:
//   1. Helpers and the copy buttons      2. Fragments (morph, fade, refresh by ETag)      3. Live (pause, held, new rows)
//   4. Overlays (htmx.ajax into #ovl, history, focus)      5. The palette      6. j and k      7. The stream
//   8. Navigation (scroll, progress, focus, preload on intent)
(() => {
  // ==== 1. Helpers and the copy buttons ====
  const attr = (el, name) => (el ? el.getAttribute(name) : null);
  const banner = () => document.getElementById('stream-banner');
  const reveal = () => {
    document.querySelectorAll('[data-js-only]').forEach((el) => { el.hidden = false; });
    // The top bar's search is offered once there is a script to answer it (the palette): a field that opens it, not one to type in.
    document.querySelectorAll('[data-palette] input').forEach((i) => { i.disabled = false; i.readOnly = true; });
  };
  reveal();
  // Motion only for people who have not asked for less of it.
  const reduced = () => !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);
  // A click this file owns (a pause, an overlay opener, a copy): taken before htmx's own handler on a boosted link sees it.
  const own = (selector, fn) => document.addEventListener('click', (e) => {
    const el = e.target.closest && e.target.closest(selector);
    if (!el || e.defaultPrevented || e.metaKey || e.ctrlKey || e.shiftKey || e.button) return;
    if (fn(el, e) !== false) { e.preventDefault(); e.stopPropagation(); }
  }, true);

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

  // ==== 2. Fragments ====
  // Idiomorph edits the nodes on screen into the new ones, so focus, typed text, caret and scroll position stay. What really
  // changed is remembered, so that only it fades in (below): an unchanged answer changes nothing and moves nothing. The same
  // configuration serves htmx's own `morph` swaps (a filter, a chip, a pager) through Idiomorph.defaults.
  const changed = new Set(), added = [];
  const mark = (el) => { if (el) changed.add(el); };
  const quiet = (el) => el && el.nodeType === 1 && el.hasAttribute('data-open');
  if (window.Idiomorph) {
    const d = window.Idiomorph.defaults;
    d.ignoreActiveValue = true;
    Object.assign(d.callbacks, {
      // A component that is open (a menu, a select's list) is left as the person has it.
      beforeNodeMorphed: (a, b) => {
        if (quiet(a)) return false;
        if (a.nodeType === 3 && a.data !== b.data) mark(a.parentElement);
        return true;
      },
      beforeNodeAdded: (n) => { added.push(n); return true; },
      beforeNodeRemoved: (n) => { mark(n.parentElement); return true; },
      // Alpine owns an `x-show` element's `style`: the server's copy (closed) must not close it.
      beforeAttributeUpdated: (name, node) => {
        if (name === 'style' && node.hasAttribute && node.hasAttribute('x-show')) return false;
        mark(node);
        return true;
      },
    });
  }
  // A very short fade (opacity only, 120 ms) on what changed.
  function fade() {
    for (const n of added.splice(0)) if (n.parentElement) changed.add(n.parentElement);
    const els = [...changed].filter((el) => el.isConnected);
    changed.clear();
    if (reduced()) return;
    els.forEach((el) => { if (el.animate) el.animate([{ opacity: 0.45 }, { opacity: 1 }], { duration: 120, easing: 'ease-out' }); });
  }
  // An answer is HTML whose top-level elements have ids: each one morphs into the element with that id.
  function apply(html) {
    const t = document.createElement('template');
    t.innerHTML = html;
    for (const el of t.content.children) {
      const target = el.id && document.getElementById(el.id);
      if (target) window.Idiomorph.morph(target, el, { morphStyle: 'outerHTML' });
    }
    fade();
  }
  // htmx's morph swaps (a filter, a chip) fade what changed, too.
  document.addEventListener('htmx:afterSwap', fade);

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
      const html = await r.text();
      if (holds(id, html)) { reached(); return; }
      const before = rowsOn(id);
      apply(html);
      markNew(id, before);
      reached();
    } catch (e) { const b = banner(); if (e.name !== 'AbortError' && b) b.hidden = false; }
  }

  // ==== 3. Live (slice 9e) ====
  // A fragment that carries a [data-seen] (a count that grows with every request recorded) is a tape: while it
  // is paused, or a drawer is open over it, new answers are kept aside and counted, so what a person reads never moves.
  // Pause is a link that works with no script (the page as it was); here it holds the tape in place instead. Resume (or
  // closing the drawer) brings the newest in. New rows are marked for a moment so the eye finds them.
  const held = {};
  let pausedAt = null;
  const seenOf = (root) => { const s = root && root.querySelector('[data-seen]'); return s ? Number(s.getAttribute('data-seen')) : null; };
  const overlayOpen = () => !!document.querySelector('[data-overlay],dialog[open]');
  const parse = (html) => { const t = document.createElement('template'); t.innerHTML = html; return t.content; };
  const showHeld = (n) => document.querySelectorAll('[data-held]').forEach((el) => { el.textContent = new Intl.NumberFormat('en-US').format(n); });
  function holds(id, html) {
    const el = document.getElementById(id);
    if (!el || seenOf(el) === null) return false;
    if (pausedAt === null && !overlayOpen()) { delete held[id]; return false; }
    held[id] = html;
    if (pausedAt !== null) showHeld(Math.max(0, seenOf(parse(html)) - pausedAt));
    return true;
  }
  function release() {
    for (const id of Object.keys(held)) { const html = held[id]; delete held[id]; const b = rowsOn(id); apply(html); markNew(id, b); }
  }
  const rowsOn = (id) => { const el = document.getElementById(id); return el ? new Set([...el.querySelectorAll('tr[data-row]')].map((r) => r.getAttribute('data-row'))) : new Set(); };
  function markNew(id, before) {
    const el = document.getElementById(id);
    if (!el || before.size === 0) return;
    el.querySelectorAll('tr[data-row]').forEach((r) => {
      if (before.has(r.getAttribute('data-row'))) return;
      r.classList.add('new');
      setTimeout(() => r.classList.remove('new'), 1600);
    });
  }
  function pause(btn) {
    const live = document.querySelector('[data-seen]');
    pausedAt = live ? Number(live.getAttribute('data-seen')) : 0;
    document.querySelectorAll('[data-paused-banner]').forEach((el) => { el.hidden = false; });
    showHeld(0);
    btn.dataset.label = btn.textContent;
    btn.textContent = 'Resume';
    btn.removeAttribute('data-pause');
    btn.setAttribute('data-resume', '');
  }
  function resume(btn) {
    pausedAt = null;
    document.querySelectorAll('[data-paused-banner]').forEach((el) => { el.hidden = true; });
    btn.textContent = btn.dataset.label || 'Pause';
    btn.removeAttribute('data-resume');
    btn.setAttribute('data-pause', '');
    release();
  }
  own('[data-pause]', (p) => pause(p));
  own('[data-resume]', (r) => (pausedAt !== null ? resume(r) : false));
  // A drawer that closes lets the tape catch up. Keys: j and k move between the rows' links; Enter opens the one in focus.
  document.addEventListener('click', () => setTimeout(() => { if (pausedAt === null && !overlayOpen()) release(); }, 0));
  // A filter that is a select applies as soon as it changes (without a script it is applied with the Enter key or the button).
  // `requestSubmit` goes through the form's submit event, which htmx (or the browser) answers.
  document.addEventListener('change', (e) => {
    const s = e.target.closest && e.target.closest('[data-autosubmit]');
    const f = s && (s.form || s.closest('form'));
    if (f) { if (f.requestSubmit) f.requestSubmit(); else f.submit(); }
  });

  // ==== 4. Overlays ====
  // A drawer or a dialog (src/ui/overlay.rs) opens over the page it was asked from, with no navigation: htmx fetches it into the
  // overlay root (#ovl). The address is pushed with a mark of ours, so Back closes it (htmx must not restore a page for that:
  // the popstate below takes the entries that are ours); Esc, the scrim and the Close control close it too. The page behind is
  // inert, focus stays inside, and closing returns focus to what opened it. A server-rendered overlay (a direct visit, a
  // reload) is adopted.
  let ovl = null; // { el, opener, pushed, y, parent }
  let opening = null; // { opener, push, parent } while a request for an overlay is out
  const root = () => document.getElementById('ovl');
  const panelOf = (el) => el.querySelector('[role=dialog]');
  const pageRoot = () => document.querySelector('.root:not([data-overlay])');
  const here = () => location.pathname + location.search;
  const sameUrl = (a, b) => { try { const x = new URL(a, location.href), y = new URL(b, location.href); return x.pathname + x.search === y.pathname + y.search; } catch (e) { return false; } };
  // Focus goes to the first field of a form, else to the panel itself (its title is read out).
  function focusIn(el) {
    const p = panelOf(el);
    if (!p) return;
    const first = p.querySelector('[autofocus]') || p.querySelector('input:not([type=hidden]):not([readonly]),select,textarea');
    (first || p).focus({ preventScroll: true });
  }
  function mount(el, o) {
    ovl = { el, opener: o.opener, pushed: false, y: window.scrollY || 0, parent: o.parent };
    const r = pageRoot();
    if (r) r.setAttribute('inert', '');
    if (o.animate && !reduced()) el.classList.add('ovl-in');
    reveal();
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
      // A drawer that held the tape lets it catch up now (Live: what came while a record was open).
      if (pausedAt === null && !document.querySelector('[data-overlay]')) release();
      const r = pageRoot();
      if (r) r.removeAttribute('inert');
      if (window.scrollTo) window.scrollTo(0, o.y);
      if (o.opener && o.opener.isConnected) o.opener.focus({ preventScroll: true });
    };
    if (now || reduced()) return finish();
    o.el.classList.remove('ovl-in');
    o.el.classList.add('ovl-out');
    o.el.addEventListener('animationend', finish, { once: true });
    setTimeout(finish, 220);
  }
  // Close: back to the address that was there, through the history if the script pushed this one.
  function dismiss() {
    if (!ovl) return;
    if (ovl.pushed) history.back();
    else if (ovl.el.hasAttribute('data-transient')) teardown();
    else { history.replaceState(null, '', attr(ovl.el, 'data-close') || location.href); teardown(); }
  }
  // The root holds an overlay that was just swapped in (or none): bring our state in line with the DOM. Run after every htmx swap.
  function sync() {
    const el = root() && root().querySelector('[data-overlay]');
    if (!el) {
      if (ovl) { const r = pageRoot(); if (r) r.removeAttribute('inert'); ovl = null; }
      return;
    }
    if (ovl && ovl.el === el) return;
    if (ovl) { ovl.el = el; reveal(); focusIn(el); return; } // the next record, a form's answer: in place, not coming in again
    const o = opening || { opener: null, push: false, parent: here(), animate: false };
    opening = null;
    const fresh = !!o.opener || o.animate;
    mount(el, { opener: o.opener, parent: o.parent, animate: fresh });
    if (el.hasAttribute('data-transient')) jumpSelect(0);
    if (o.push) { history.pushState({ ovl: 1, parent: o.parent }, '', o.push); ovl.pushed = true; }
  }
  function openOverlay(url, opener, push, animate) {
    opening = { opener, push: push ? url : null, parent: here(), animate: !!animate };
    // The overlay (or, if the address answered something else, the page) comes back through htmx; if it cannot, go there.
    const failed = () => { opening = null; location.href = url; };
    const req = window.htmx.ajax('GET', url, { target: '#ovl', swap: 'innerHTML', headers: { 'X-Fragment': '1' } });
    if (req && req.then) req.then(() => { if (!root() || !root().querySelector('[data-overlay]')) failed(); }, failed);
  }
  own('[data-dialog]', (a) => { openOverlay(a.href, a, true); });
  own('[data-dialog-close],[data-overlay-close]', (c) => { if (!ovl || !ovl.el.contains(c)) return false; dismiss(); });
  // A form of an overlay: posted in place. An overlay comes back (a refusal, a field to fix) and replaces this one; a page
  // comes back (the result of a write) and the layer told htmx to swap it into main and replace this address with its own.
  // A submit control that asks for a GET (formmethod, like a dialog's Preview) never writes: its answer is fetched with the
  // fields in the address, and the overlay that comes back replaces this one and takes that address.
  document.addEventListener('submit', (e) => {
    const f = e.target.closest ? e.target.closest('form') : null;
    if (!f || !ovl || !ovl.el.contains(f)) return;
    e.preventDefault();
    e.stopPropagation();
    const sub = e.submitter;
    const via = ((sub && sub.getAttribute('formmethod')) || f.getAttribute('method') || 'post').toLowerCase();
    const data = new FormData(f, sub || undefined);
    f.setAttribute('aria-busy', 'true');
    if (via === 'get') {
      const at = new URL((sub && sub.getAttribute('formaction')) || f.action, location.href);
      at.search = new URLSearchParams(data).toString();
      history.replaceState({ ovl: 1, parent: ovl.parent }, '', at.href);
      opening = null;
      window.htmx.ajax('GET', at.href, { target: '#ovl', swap: 'innerHTML', headers: { 'X-Fragment': '1' } });
    } else {
      window.htmx.ajax('POST', f.action, { target: '#ovl', swap: 'innerHTML', values: Object.fromEntries(data), headers: { 'X-Fragment': '1' } })
        .catch(() => f.submit());
    }
  }, true);
  document.addEventListener('keydown', (e) => {
    if (!ovl) return;
    if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); dismiss(); return; }
    if (e.key !== 'Tab') return;
    // The page behind is inert, so Tab cannot leave through it; this keeps it going round inside the panel.
    const p = panelOf(ovl.el), f = [...p.querySelectorAll('a[href],button,input,select,textarea,[tabindex]')].filter((n) => !n.disabled && !n.hidden && n.tabIndex >= 0 && n.getClientRects().length), at = document.activeElement;
    if (!f.length) { e.preventDefault(); p.focus(); return; }
    const first = f[0], last = f[f.length - 1];
    if (e.shiftKey && (at === first || !p.contains(at))) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && (at === last || !p.contains(at))) { e.preventDefault(); first.focus(); }
  });
  // Back and Forward. An entry of ours (an overlay) beside its parent page is handled here, before htmx (a capture listener on
  // the window runs first, and stopping it keeps htmx from putting a whole page back for what is only an overlay).
  addEventListener('popstate', (e) => {
    const st = history.state, now = location.pathname + location.search;
    if (ovl && !(st && st.ovl)) {
      if (sameUrl(now, ovl.parent)) { e.stopImmediatePropagation(); teardown(); } else { teardown(true); }
    } else if (!ovl && st && st.ovl && sameUrl(st.parent, pageUrl)) {
      e.stopImmediatePropagation();
      openOverlay(location.href, null, false, true);
    } else if (ovl && st && st.ovl && !sameUrl(now, attr(ovl.el, 'data-url') || now)) {
      e.stopImmediatePropagation();
      openOverlay(location.href, null, false);
    }
  }, true);
  // A server-rendered overlay is adopted: the same closing, the same Back.
  function adopt() {
    const first = root() && root().querySelector('[data-overlay]');
    if (!first) return;
    const parent = attr(first, 'data-close') || here();
    mount(first, { opener: null, parent, animate: false });
    history.replaceState({ ovl: 1, parent }, '', location.href);
  }

  // ==== 5. The jump palette ====
  // ⌘K or Ctrl+K, or a click on the top bar's search, opens the palette (/jump, an overlay with no page behind it of its
  // own: no history entry). Typing narrows the names, the arrows move, Enter goes there, Esc closes.
  const palette = () => attr(document.querySelector('[data-palette]'), 'data-palette');
  function openPalette() {
    if (ovl) { if (ovl.el.hasAttribute('data-transient')) dismiss(); return; }
    const url = palette();
    if (url) openOverlay(url, document.activeElement, false, true);
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
  document.addEventListener('click', (e) => { if (!ovl && e.target.closest('[data-palette]')) openPalette(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !ovl && e.target.closest && e.target.closest('[data-palette]')) openPalette(); });

  // ==== 6. j and k through the rows ====
  // j and k move through the rows of the page's table (a row's first link takes the focus; Enter opens it, as on any link). With
  // a drawer open they are the next and the previous record: the opener's neighbours in its table.
  const typing = (t) => t && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable || (t.closest && t.closest('[role=listbox],[role=menu]')));
  const rowLinks = () => [...document.querySelectorAll('main table.t tbody tr')].map((tr) => tr.querySelector('a[href]')).filter(Boolean);
  const tapeLinks = () => [...document.querySelectorAll('table.wire td.tm a')];
  document.addEventListener('keydown', (e) => {
    if ((e.key !== 'j' && e.key !== 'k') || e.metaKey || e.ctrlKey || e.altKey || e.defaultPrevented || typing(e.target)) return;
    const links = rowLinks().length ? rowLinks() : tapeLinks();
    if (!links.length) return;
    const from = ovl ? ovl.opener : document.activeElement && document.activeElement.closest('tr') ? document.activeElement.closest('tr').querySelector('a[href]') : null;
    const at = links.indexOf(from);
    const next = links[at < 0 ? (e.key === 'j' ? 0 : links.length - 1) : at + (e.key === 'j' ? 1 : -1)];
    e.preventDefault();
    if (!next) return;
    if (ovl) { if (next.hasAttribute('data-dialog')) { ovl.opener = next; openOverlay(next.href, next, true); } return; }
    next.focus();
  });

  // ==== 7. The stream ====
  // One connection while the page is on screen. A tick names the fragments to fetch again. The address is on `main`: a swap
  // of main (a navigation) brings the next page's own, and the stream is opened again for it.
  let es = null, wait = 1000, timer = 0, streamUrl = null;
  const close = () => { clearTimeout(timer); if (es) es.close(); es = null; };
  const wanted = () => attr(document.getElementById('main'), 'data-stream') || attr(document.body, 'data-stream');
  const connect = () => {
    close();
    streamUrl = wanted();
    if (!streamUrl || !window.EventSource) return;
    es = new EventSource(streamUrl);
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
  // A page in the back/forward cache or a hidden tab must not hold a stream open: a browser allows 6 connections per origin (spike 2).
  document.addEventListener('visibilitychange', () => (document.hidden ? close() : connect()));
  addEventListener('pagehide', close);
  addEventListener('pageshow', (e) => { if (e.persisted) connect(); });

  // ==== 8. Navigation: the seams to htmx ====
  // Scroll: a new page starts at the top (the swap says so), Back puts the page where it was left, and a control that changes only
  // its own region keeps the scroll (its swap says show:none). Scroll is remembered per address for the session.
  let pageUrl = here();
  const store = (() => { try { return window.sessionStorage; } catch (e) { return null; } })();
  const remember = () => { if (store) { try { store.setItem('doze:y:' + pageUrl, String(window.scrollY || 0)); } catch (e) { /* storage is full or off */ } } };
  if ('scrollRestoration' in history) history.scrollRestoration = 'manual';
  let slow = 0;
  const hx = (name, fn) => document.addEventListener('htmx:' + name, fn);
  hx('beforeRequest', (e) => {
    const h = e.detail.requestConfig && e.detail.requestConfig.headers;
    if (h && h['HX-Preloaded']) return;
    if (e.detail.elt === document.body || (e.detail.requestConfig && e.detail.requestConfig.boosted)) remember();
    // A thin hint only when a request is slow: nothing for a fast one.
    clearTimeout(slow);
    slow = setTimeout(() => document.documentElement.classList.add('is-loading'), 250);
  });
  const done = () => { clearTimeout(slow); document.documentElement.classList.remove('is-loading'); };
  hx('afterRequest', done);
  hx('sendError', (e) => { done(); const p = e.detail.pathInfo && e.detail.pathInfo.requestPath; if (p && e.detail.requestConfig && e.detail.requestConfig.boosted) location.href = p; });
  // After any swap: our state follows the DOM (an overlay came or went), new controls are revealed, the stream follows the page.
  hx('afterSettle', () => {
    pageUrl = ovl && ovl.parent ? ovl.parent : here();
    sync();
    reveal();
    if (wanted() !== streamUrl) connect();
  });
  hx('load', reveal);
  // A page swapped into main takes focus (a screen reader reads the new page), without scrolling.
  hx('afterSwap', (e) => {
    const t = e.detail.target;
    if (!t || t.id !== 'main' || e.detail.requestConfig.headers['HX-Boosted'] === undefined) return;
    const m = document.getElementById('main');
    if (m) { m.setAttribute('tabindex', '-1'); m.focus({ preventScroll: true }); }
  });
  // Back and Forward restore the page (htmx fetches it); the scroll it had comes back with it.
  hx('historyRestore', () => {
    pageUrl = here();
    let y = 0;
    if (store) { try { y = Number(store.getItem('doze:y:' + pageUrl)) || 0; } catch (e) { y = 0; } }
    if (window.scrollTo) window.scrollTo(0, y);
    reveal();
    sync();
    connect();
  });
  hx('pushedIntoHistory', () => { pageUrl = here(); });
  hx('replacedInHistory', () => { pageUrl = here(); });
  addEventListener('pagehide', remember);
  // Preload on intent: a boosted link that the pointer rests on for 65 ms, or that takes focus, has its page fetched ahead with the
  // headers a navigation would send; the server lets the browser keep that answer for five seconds (src/region.rs), so the click
  // that follows is answered from the cache. Links that change a region in place, open an overlay or leave the console are not
  // fetched. (htmx's own preload extension was tried and dropped: it throws on a form with a hidden field and has no focus trigger.)
  const warm = new Map();
  let rest = 0;
  const preloadable = (a) => a && a.href && a.origin === location.origin && !a.target && !a.hasAttribute('download') && !a.hasAttribute('data-dialog') &&
    !a.hasAttribute('data-nopreload') && !a.hasAttribute('hx-get') && !a.hasAttribute('data-pause') && !a.hasAttribute('data-resume') &&
    !a.hasAttribute('data-overlay-close') && !a.hasAttribute('data-dialog-close') && !!a.closest('[hx-boost=true]') && a.getAttribute('hx-boost') !== 'false' && a.href.split('#')[0] !== location.href.split('#')[0];
  function preload(a) {
    if (!preloadable(a)) return;
    const seen = warm.get(a.href);
    if (seen && Date.now() - seen < 4000) return;
    warm.set(a.href, Date.now());
    fetch(a.href, { credentials: 'same-origin', headers: { 'HX-Request': 'true', 'HX-Boosted': 'true', 'HX-Target': 'main', 'HX-Preloaded': 'true' } }).catch(() => warm.delete(a.href));
  }
  document.addEventListener('mouseover', (e) => { const a = e.target.closest && e.target.closest('a[href]'); clearTimeout(rest); if (a) rest = setTimeout(() => preload(a), 65); });
  document.addEventListener('mouseout', () => clearTimeout(rest));
  document.addEventListener('focusin', (e) => preload(e.target.closest && e.target.closest('a[href]')));

  adopt();
  connect();
})();
