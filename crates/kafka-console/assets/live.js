// How the console changes while it is open (docs/port/console/live-design.md, CON-800 to CON-812). Ours; needs htmx, idiomorph and Alpine.
//
// A page is a SNAPSHOT: nothing here runs on its own. A person can ask for more, in two ways:
//   1. REFRESH (`refreshControl`): a button (key r) and an interval (Off, 5 s, 15 s, 30 s, 1 min; remembered in localStorage). A refresh
//      fetches the page's regions (`[data-frag]`) with their ETags: an unchanged region answers 304 and nothing happens, a changed
//      one is morphed in place (idiomorph, matched by id), so only what differs is touched. No fade, no animation. Polls are
//      chained (the next is scheduled when the last has finished), stop while the tab is hidden, and are skipped while a field
//      has focus, a menu is open or an overlay is open.
//   2. TAIL (`tailControl`): the Live page and a topic's Messages page. `Live tail` opens ONE EventSource for the tail region; rows
//      arrive at most once a second (the server coalesces), are held when you are reading (scrolled, pointer or focus or a
//      selection in the table) behind a "N new" pill, are put at the top without touching the rows below, and a row that grows
//      by a folded repeat changes in place. Dropped connections come back with a growing wait, from the last row seen.
// Also here: the hover that says how long ago a time was (worked out when the pointer is on it, never updated), and the countdown
// to a deadline (the only timer a page may have, and only while it shows a deadline; no request).
(() => {
  const D = (window.Doze = window.Doze || {});
  const attr = (el, n) => (el ? el.getAttribute(n) : null);
  const store = (() => { try { return window.localStorage; } catch (e) { return null; } })();
  const KEY = 'doze:refresh';
  const hhmmss = (d) => d.toISOString().slice(11, 19);
  const typing = (t) => t && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable || (t.closest && t.closest('[role=listbox],[role=menu]')));
  const overlayOpen = () => !!document.querySelector('[data-overlay],dialog[open]');
  const parse = (html) => { const t = document.createElement('template'); t.innerHTML = html; return t.content; };

  // Something a person is doing that a refresh must not disturb.
  const busy = (scope) => {
    if (overlayOpen() || document.querySelector('[data-open]')) return true;
    const a = document.activeElement;
    return !!(a && a !== document.body && /^(input|select|textarea)$/i.test(a.tagName) && (!scope || scope.contains(a)));
  };

  // ==== Refresh ====
  const etags = {};
  let bar = null, timer = 0, running = null, due = 0;
  const intervalOf = () => { const v = store && Number(store.getItem(KEY)); return v >= 5 ? v : 0; };
  const stamp = (bar_, when) => {
    const t = bar_ && bar_.querySelector('time[data-stamp]');
    if (!t) return;
    t.textContent = hhmmss(when);
    t.setAttribute('datetime', when.toISOString().slice(0, 19) + 'Z');
  };
  const failed = (bar_, text) => {
    const f = bar_ && bar_.querySelector('[data-refresh-failed]');
    if (f) { f.hidden = !text; f.textContent = text || ''; }
  };
  const tailing = (el) => D.tail && D.tail.mode === 'live' && D.tail.region === el;

  // Morph `el` into the region of `html` that has its id. The header of a table never changes (`data-morph-skip`).
  function apply(el, html) {
    const next = [...parse(html).children].find((c) => c.id === el.id);
    if (!next) return false;
    D.skipHeads = true;
    try { window.Idiomorph.morph(el, next, { morphStyle: 'outerHTML' }); } finally { D.skipHeads = false; }
    return true;
  }

  async function poll(el, signal) {
    const url = attr(el, 'data-frag');
    if (!url) return 'none';
    const headers = etags[el.id] ? { 'If-None-Match': etags[el.id] } : {};
    const r = await fetch(url, { signal, headers, credentials: 'same-origin' });
    if (r.status === 304) return { changed: false, date: r.headers.get('Date') };
    if (!r.ok) throw new Error(String(r.status));
    const known = el.id in etags;
    etags[el.id] = r.headers.get('ETag');
    const html = await r.text();
    apply(el, html);
    return { changed: known || true, date: r.headers.get('Date') };
  }

  async function refresh(manual) {
    const b = bar;
    if (!b || !document.contains(b) || running) return;
    if (!manual && busy(document.getElementById('main'))) { schedule(); return; }
    const ids = (attr(b, 'data-regions') || '').split(',').filter(Boolean);
    const regions = ids.map((id) => document.getElementById(id)).filter((el) => el && !tailing(el));
    const c = (running = new AbortController());
    let changed = false, date = null;
    try {
      const done = await Promise.all(regions.map((el) => poll(el, c.signal)));
      done.forEach((d) => { if (d && d.changed) changed = true; if (d && d.date) date = d.date; });
      failed(b, '');
      if (changed || manual) stamp(b, date ? new Date(date) : new Date());
    } catch (e) {
      if (e.name !== 'AbortError') {
        const secs = intervalOf();
        failed(b, secs ? 'Could not refresh · will retry at ' + hhmmss(new Date(Date.now() + secs * 1000)) : 'Could not refresh');
      }
    } finally { running = null; }
    schedule();
  }

  // The next poll is scheduled when the last has finished (never a queue), and not at all while the tab is hidden.
  function schedule() {
    clearTimeout(timer);
    const secs = intervalOf();
    if (!secs || !bar || !document.contains(bar) || document.hidden) return;
    due = Date.now() + secs * 1000;
    timer = setTimeout(() => refresh(false), secs * 1000);
  }
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) { clearTimeout(timer); return; }
    if (intervalOf() && Date.now() >= due) refresh(false); else schedule();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'r' || e.metaKey || e.ctrlKey || e.altKey || e.defaultPrevented || typing(e.target) || overlayOpen()) return;
    if (bar && document.contains(bar)) { e.preventDefault(); refresh(true); }
  });

  document.addEventListener('alpine:init', () => {
    const A = window.Alpine;
    A.data('refreshControl', () => ({
      init() {
        const root = this.$el;
        bar = root;
        root.classList.add('is-on');
        const secs = intervalOf();
        const native = root.querySelector('select');
        if (native && secs) {
          native.value = String(secs);
          const sel = root.querySelector('.dz-select');
          const data = sel && A.$data(sel);
          if (data && data.paint) data.paint();
        }
        schedule();
      },
      now(e) { if (e) e.preventDefault(); refresh(true); },
      picked(e) {
        const v = Number(e.target && e.target.value) || 0;
        if (store) { try { store.setItem(KEY, String(v)); } catch (x) { /* storage is off: the choice lasts for this page */ } }
        if (running) running.abort();
        if (v) refresh(true); else { clearTimeout(timer); failed(bar, ''); }
      },
    }));

    // ==== Tail ====
    const MAX_ROWS = 200, HOLD_PX = 8, BATCH_MS = 1000, WAITS = [1, 2, 4, 8, 15, 30];
    A.data('tailControl', () => {
      let root, es = null, retry = 0, wait = 0, countdown = 0, reopen = 0, lastId = '0', lastFlush = 0, flushAt = 0, tick = 0;
      const pending = new Map(); // row id -> element, newest first
      const q = (s) => root.querySelector(s);
      const T = {
        mode: 'snapshot',
        get region() { return root ? document.getElementById(attr(root, 'data-tail-for')) : null; },
        get rootEl() { const r = this.region; return r && r.querySelector('.tail-root'); },
        get body() { const r = this.rootEl; return r && r.querySelector('tbody[data-tail]'); },
      };
      const status = (s, text) => {
        const c = q('[data-conn]');
        c.setAttribute('data-conn', s);
        q('[data-conn-text]').textContent = text;
      };
      const pill = () => {
        const p = q('[data-held-pill]');
        p.hidden = pending.size === 0;
        q('[data-held-n]').textContent = String(pending.size);
      };
      const holding = () => {
        const body = T.body;
        if (!body) return true;
        const sel = window.getSelection && window.getSelection();
        const reading = (sel && !sel.isCollapsed && body.contains(sel.anchorNode)) || body.contains(document.activeElement) || T.rootEl.matches(':hover');
        return window.scrollY > HOLD_PX || reading || overlayOpen();
      };
      const url = () => {
        const r = T.rootEl;
        if (!r) return null;
        const u = new URL(attr(r, 'data-tail-url'), location.href);
        u.searchParams.set('since', lastId);
        return u.href;
      };
      const idPrefix = () => (attr(T.rootEl, 'data-tail-rows') || '') + '-r-';
      const settle = () => { const e = T.region && T.region.querySelector('[data-empty]'); if (e && T.body) e.hidden = T.body.children.length > 0; };

      function insert() {
        const body = T.body;
        if (!body || !pending.size) return;
        const frag = document.createDocumentFragment();
        for (const el of pending.values()) { el.classList.add('fresh'); frag.append(el); }
        const added = [...frag.children];
        pending.clear();
        body.prepend(frag);
        while (body.children.length > MAX_ROWS) body.lastElementChild.remove();
        settle();
        pill();
        setTimeout(() => added.forEach((r) => r.classList.remove('fresh')), 1600);
      }
      function flush() {
        flushAt = 0;
        lastFlush = Date.now();
        if (holding()) pill(); else insert();
      }
      function later() {
        if (flushAt) return;
        const wait_ = Math.max(0, lastFlush + BATCH_MS - Date.now());
        flushAt = setTimeout(flush, wait_);
      }
      function take(html, id) {
        if (id) lastId = id;
        const body = T.body;
        if (!body) return;
        const seen = idPrefix(), fresh = [];
        for (const row of parse(html).children) {
          if (!row.id || !row.id.startsWith(seen)) continue;
          const on = document.getElementById(row.id);
          if (on) { D.skipHeads = false; window.Idiomorph.morph(on, row, { morphStyle: 'outerHTML' }); }
          else if (pending.has(row.id)) pending.set(row.id, row);
          else fresh.push(row);
        }
        // Newest first: this batch came newest first, and it is newer than what is already held.
        if (fresh.length) {
          const older = [...pending];
          pending.clear();
          fresh.forEach((r) => pending.set(r.id, r));
          older.forEach(([k, v]) => pending.set(k, v));
        }
        while (pending.size > 500) pending.delete(pending.keys().next().value);
        later();
      }

      function open() {
        close(false);
        const u = url();
        if (!u || !window.EventSource) return;
        es = new EventSource(u);
        es.addEventListener('ready', () => { retry = 0; clearInterval(tick); status('live', 'Live'); });
        es.addEventListener('rows', (e) => take(e.data, e.lastEventId));
        es.onerror = () => {
          if (es) es.close();
          es = null;
          lost();
        };
      }
      function lost() {
        if (T.mode !== 'live') return;
        const base = WAITS[Math.min(retry, WAITS.length - 1)];
        wait = Math.round(base * (0.85 + Math.random() * 0.3));
        retry += 1;
        clearInterval(tick);
        const until = Date.now() + wait * 1000;
        const paint = () => {
          const left = Math.max(0, Math.ceil((until - Date.now()) / 1000));
          if (retry <= 1) status('reconnecting', 'Reconnecting…');
          else status('disconnected', 'Disconnected · Retry in ' + left + ' s');
        };
        paint();
        tick = setInterval(paint, 1000);
        clearTimeout(reopen);
        reopen = setTimeout(() => { clearInterval(tick); status('reconnecting', 'Reconnecting…'); open(); }, wait * 1000);
      }
      function close(clearRows) {
        if (es) es.close();
        es = null;
        clearTimeout(reopen);
        clearInterval(tick);
        clearTimeout(flushAt);
        flushAt = 0;
        if (clearRows) { pending.clear(); pill(); }
      }
      const mark = () => {
        const live = T.mode === 'live';
        root.querySelectorAll('[data-tail-mode]').forEach((b) => {
          const on = attr(b, 'data-tail-mode') === (live ? 'live' : 'snapshot');
          b.classList.toggle('on', on);
          b.setAttribute('aria-pressed', String(on));
        });
        if (T.rootEl) T.rootEl.classList.toggle('is-tailing', live);
      };

      return {
        init() {
          root = this.$el;
          D.tail = T;
          const live = q('[data-tail-mode=live]');
          if (live && window.EventSource && !live.hasAttribute('data-unavailable') && T.rootEl && attr(T.rootEl, 'data-tail-url')) live.disabled = false;
          const recheck = () => { if (T.mode === 'live' && pending.size && !holding()) { clearTimeout(flushAt); flushAt = 0; insert(); } };
          window.addEventListener('scroll', recheck, { passive: true });
          document.addEventListener('selectionchange', recheck);
          document.addEventListener('focusout', () => setTimeout(recheck, 0));
          document.addEventListener('mouseout', (e) => { const r = e.target.closest && e.target.closest('.tail-root'); if (r && !r.contains(e.relatedTarget)) recheck(); });
          document.addEventListener('visibilitychange', () => {
            if (T.mode !== 'live') return;
            if (document.hidden) close(false); else { retry = 0; open(); }
          });
          // The region was swapped (a filter): the tail follows the new rows, from the new cursor.
          document.addEventListener('htmx:afterSettle', () => {
            if (!root.isConnected) { close(true); if (D.tail === T) D.tail = null; return; }
            if (T.mode === 'live') { pending.clear(); pill(); lastId = attr(T.rootEl, 'data-cursor') || '0'; mark(); open(); }
          });
          window.addEventListener('pagehide', () => close(false));
          lastId = attr(T.rootEl, 'data-cursor') || '0';
        },
        live() {
          if (T.mode === 'live' || !T.rootEl) return;
          T.mode = 'live';
          lastId = attr(T.rootEl, 'data-cursor') || lastId;
          retry = 0;
          status('reconnecting', 'Connecting…');
          mark();
          open();
        },
        snapshot() {
          if (T.mode === 'snapshot') return;
          T.mode = 'snapshot';
          close(true);
          status('snapshot', 'Snapshot');
          mark();
        },
        show() {
          clearTimeout(flushAt);
          flushAt = 0;
          insert();
          const r = T.rootEl;
          if (r) window.scrollTo(0, Math.max(0, r.getBoundingClientRect().top + window.scrollY - 140));
        },
      };
    });
  });

  // ==== A time says how long ago it was, when the pointer is on it ====
  // The page holds only the absolute time (so a region does not change while the data does not); the relative form is worked out
  // here, once, when it is asked for, and not kept up to date.
  const ago = (ms) => {
    const s = Math.round(ms / 1000);
    if (s < 1) return 'just now';
    if (s < 60) return s + ' s ago';
    if (s < 3600) return Math.floor(s / 60) + ' min ago';
    if (s < 86400) return Math.floor(s / 3600) + ' h ' + Math.floor((s % 3600) / 60) + ' min ago';
    return Math.floor(s / 86400) + ' d ' + Math.floor((s % 86400) / 3600) + ' h ago';
  };
  const rel = (e) => {
    const t = e.target.closest && e.target.closest('time[data-rel]');
    if (!t) return;
    if (!t.hasAttribute('data-abs')) t.setAttribute('data-abs', t.getAttribute('title') || '');
    const at = Date.parse(t.getAttribute('datetime'));
    if (!isNaN(at)) t.setAttribute('title', (t.getAttribute('data-abs') ? t.getAttribute('data-abs') + ' · ' : '') + ago(Date.now() - at));
  };
  document.addEventListener('mouseover', rel);
  document.addEventListener('focusin', rel);

  // ==== A deadline counts down in its own element, with no request ====
  let cd = 0;
  const countdown = () => {
    const els = document.querySelectorAll('[data-until]');
    if (!els.length) { clearInterval(cd); cd = 0; return; }
    els.forEach((el) => {
      const left = Math.round((Date.parse(el.getAttribute('data-until')) - Date.now()) / 1000);
      el.textContent = left > 0 ? '(in ' + (left >= 60 ? Math.floor(left / 60) + ' min ' + (left % 60) + ' s' : left + ' s') + ')' : '(passed)';
    });
  };
  const watch = () => { if (document.querySelector('[data-until]')) { countdown(); if (!cd) cd = setInterval(countdown, 1000); } };
  document.addEventListener('htmx:afterSettle', watch);
  watch();
})();
