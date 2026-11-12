// The console's own script. Pages are complete without it; this only adds what a script can: controls that need one
// (copy), fragments that refetch when the server says they changed, and the banner when it cannot say so any more.
// Every name below is defined in src/ui/client.rs, and a test checks that they agree.
(() => {
  const attr = (el, name) => (el ? el.getAttribute(name) : null);
  // Controls that only a script can serve are hidden in the markup; show them now.
  document.querySelectorAll('[data-js-only]').forEach((el) => { el.hidden = false; });

  // Copy: the text of the button, or of the element it names; "Copy as" changes the form of the address.
  let copyAs = 'code';
  const address = () => attr(document.querySelector('[data-address]'), 'data-address') || '';
  const textOf = (btn) => {
    const own = attr(btn, 'data-copy');
    if (own !== null) return own;
    if (copyAs === 'properties') return 'bootstrap.servers=' + address();
    if (copyAs === 'environment') return 'KAFKA_BOOTSTRAP_SERVERS=' + address();
    const from = document.getElementById(attr(btn, 'data-copy-from'));
    return from ? from.textContent : '';
  };
  document.addEventListener('click', (e) => {
    const pick = e.target.closest('[data-copy-as]');
    if (pick) {
      copyAs = attr(pick, 'data-copy-as');
      pick.parentElement.querySelectorAll('[data-copy-as]').forEach((b) => b.classList.toggle('on', b === pick));
      return;
    }
    const btn = e.target.closest('[data-copy],[data-copy-from]');
    if (btn && navigator.clipboard) navigator.clipboard.writeText(textOf(btn));
  });

  // The stream: one connection while the page is on screen; a tick names the fragments to fetch again.
  const url = attr(document.body, 'data-stream');
  if (!url || !window.EventSource) return;
  const banner = document.getElementById('stream-banner');
  let wait = 1000;
  const open = () => {
    const es = new EventSource(url);
    es.addEventListener('tick', (e) => {
      e.data.split(',').forEach((id) => {
        const el = document.getElementById(id);
        if (el && window.htmx) window.htmx.trigger(el, 'doze:tick');
      });
    });
    es.onopen = () => { wait = 1000; if (banner) banner.hidden = true; };
    es.onerror = () => {
      es.close();
      if (banner) banner.hidden = false;
      setTimeout(open, wait);
      wait = Math.min(wait * 2, 15000);
    };
  };
  open();
})();
