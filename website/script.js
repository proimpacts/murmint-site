// Where the Download buttons point. Upload the installer (for example to GitHub Releases) and put its address here.
// Until then the main button says the download is coming soon instead of linking to nothing.
var DOWNLOAD_URL = location.protocol === 'file:' ? '' : '/download/installer'; // signed-in Alpha Testers get the installer; others are asked to sign in with Discord

(function () {
  var $ = function (s) { return document.querySelector(s); };
  var $$ = function (s) { return document.querySelectorAll(s); };
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // ---- download buttons ----
  $$('[data-download]').forEach(function (b) {
    if (DOWNLOAD_URL) { b.href = DOWNLOAD_URL; return; }
    if (b.hasAttribute('data-main')) { b.removeAttribute('href'); b.setAttribute('aria-disabled', 'true'); b.textContent = 'Download coming soon'; }
  });
  $$('[data-dl-note]').forEach(function (n) { if (!DOWNLOAD_URL) n.textContent = 'The Windows installer is being finished. Check back soon, or build it from the source today.'; });
  if ($('#yr')) $('#yr').textContent = new Date().getFullYear();

  // ---- light / dark ----
  var root = document.documentElement;
  var setTheme = function (t, keep) {
    root.dataset.theme = t;
    $$('[data-theme-set]').forEach(function (b) { b.setAttribute('aria-pressed', String(b.getAttribute('data-theme-set') === t)); });
    if (keep) { try { localStorage.setItem('theme', t); } catch (e) { /* storage blocked: the choice lasts for this page only */ } }
  };
  setTheme(root.dataset.theme === 'light' ? 'light' : 'dark', false); // dark unless the visitor picked light
  $$('[data-theme-set]').forEach(function (b) { b.addEventListener('click', function () { setTheme(b.getAttribute('data-theme-set'), true); }); });

  // ---- mobile menu ----
  var menu = $('.menu'), links = $('#links');
  if (menu && links) {
    menu.addEventListener('click', function () { var open = links.classList.toggle('open'); menu.setAttribute('aria-expanded', String(open)); });
    links.addEventListener('click', function (e) { if (e.target.closest('a')) { links.classList.remove('open'); menu.setAttribute('aria-expanded', 'false'); } });
  }

  // ---- the one animation: what you say, then what Murmint types ----
  var said = $('#said'), typed = $('#typed');
  if (said && typed) {
    var demos = [
      { say: 'hi maria um thanks for sending the proposal we looked at it with the team and want to move ahead can you share the timeline thanks alex', out: 'Hi Maria,\n\nThanks for sending the proposal. We looked at it with the team and want to move ahead. Can you share the timeline?\n\nThanks,\nAlex' },
      { say: 'can we move the call to three no wait four and uh send me the deck before', out: 'Can we move the call to four and send me the deck before?' },
      { say: 'list these items milk eggs bread and coffee', out: '- Milk\n- Eggs\n- Bread\n- Coffee' },
    ];
    var n = 0, timer = 0;
    var write = function (el, text, done) {
      var i = 0;
      (function step() {
        el.textContent = text.slice(0, i);
        var c = document.createElement('span'); c.className = 'caret'; el.appendChild(c);
        if (i >= text.length) { done(); return; }
        i += Math.max(1, Math.round(text.length / 70));
        timer = setTimeout(step, 24);
      })();
    };
    var play = function () {
      var d = demos[n];
      if (reduced) { said.textContent = d.say; typed.textContent = d.out; return; }
      typed.textContent = '';
      write(said, d.say, function () {
        said.querySelector('.caret') && said.querySelector('.caret').remove();
        timer = setTimeout(function () {
          write(typed, d.out, function () { timer = setTimeout(function () { n = (n + 1) % demos.length; play(); }, 4200); });
        }, 600);
      });
    };
    play();
  }

  // ---- writing styles: same words, different tone ----
  var styleOut = $('#style-out');
  if (styleOut) {
    var styles = {
      natural: 'Can we move Thursday’s call? The deadline is Friday and I still need the numbers from finance.',
      casual: 'Hey, can we push Thursday’s call? Deadline’s Friday and I’m still waiting on finance’s numbers.',
      professional: 'Could we reschedule Thursday’s call? The deadline is Friday, and I am still waiting on the figures from Finance.',
      concise: 'Can we move Thursday’s call? Deadline is Friday; still need finance’s numbers.',
    };
    $$('[data-style]').forEach(function (b) {
      b.addEventListener('click', function () {
        $$('[data-style]').forEach(function (x) { x.setAttribute('aria-pressed', String(x === b)); });
        styleOut.textContent = styles[b.getAttribute('data-style')];
      });
    });
  }
})();

// closed alpha card: shown once a moment after load, until dismissed
(function () {
  var pop = document.getElementById('alpha-pop'); if (!pop) return;
  var seen = false; try { seen = localStorage.getItem('alphaPop') === 'closed'; } catch (e) {}
  if (seen) return;
  setTimeout(function () { pop.hidden = false; }, 1200);
  document.getElementById('alpha-close').addEventListener('click', function () { pop.hidden = true; try { localStorage.setItem('alphaPop', 'closed'); } catch (e) {} });
})();

// ---- motion: a few small touches with anime.js (MIT, served from /assets), in the spirit of reactbits.dev and uiball.com ----
// Only a few things move: the home headline, the memory bars and their numbers, the engine figures, and a small waveform loader.
(function () {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  var tag = document.createElement('script'); tag.src = '/assets/anime.min.js'; tag.async = true;
  tag.onload = function () { if (window.anime) start(window.anime); };
  document.head.appendChild(tag);

  var $$ = function (q, r) { return Array.prototype.slice.call((r || document).querySelectorAll(q)); };

  function start(anime) {
    // 1. the home headline rises in word by word (reactbits SplitText)
    $$('.hero h1').forEach(function (h) {
      var words = [];
      (function walk(node) {
        Array.prototype.slice.call(node.childNodes).forEach(function (n) {
          if (n.nodeType === 3 && n.textContent.trim()) {
            var frag = document.createDocumentFragment();
            n.textContent.split(/(\s+)/).forEach(function (part) {
              if (!part) return;
              if (/^\s+$/.test(part)) { frag.appendChild(document.createTextNode(' ')); return; }
              var o = document.createElement('span'); o.className = 'sw';
              var i = document.createElement('span'); i.className = 'sw-i'; i.textContent = part;
              o.appendChild(i); frag.appendChild(o); words.push(i);
            });
            node.replaceChild(frag, n);
          } else if (n.nodeType === 1 && !n.classList.contains('sw')) walk(n);
        });
      })(h);
      if (!words.length) return;
      anime({ targets: words, translateY: ['105%', '0%'], duration: 600, delay: anime.stagger(40), easing: 'easeOutExpo' });
    });

    // 2. the memory bars grow and their numbers count up when you reach them
    $$('.bars').forEach(function (bars) {
      var rows = $$('.bar-row', bars), fills = rows.map(function (r) { return r.querySelector('.fill'); });
      var widths = fills.map(function (f) { return f.style.width; });
      fills.forEach(function (f) { f.style.width = '0%'; });
      onVisible(bars, function () {
        fills.forEach(function (f, i) { anime({ targets: f, width: widths[i], duration: 900, delay: i * 60, easing: 'easeOutExpo' }); });
        rows.forEach(function (r, i) {
          var v = r.querySelector('.val'), m = v.textContent.match(/^(~?)([\d.]+)\s*(\w+)$/); if (!m) return;
          var end = parseFloat(m[2]), o = { n: 0 };
          anime({ targets: o, n: end, duration: 900, delay: i * 60, easing: 'easeOutExpo',
            update: function () { v.textContent = m[1] + (end % 1 ? o.n.toFixed(1) : Math.round(o.n)) + ' ' + m[3]; } });
        });
      }, 0.3);
    });

    // 3. the engine figures count up
    $$('.figs b').forEach(function (b) {
      var m = b.textContent.match(/^([\d.]+)\s*(.*)$/); if (!m) return;
      var end = parseFloat(m[1]), o = { n: 0 }, unit = m[2] ? ' ' + m[2] : '';
      b.textContent = '0' + unit;
      onVisible(b, function () {
        anime({ targets: o, n: end, duration: 800, easing: 'easeOutExpo', update: function () { b.textContent = (end % 1 ? o.n.toFixed(1) : Math.round(o.n)) + unit; } });
      }, 0.5);
    });

    // 4. a waveform loader (uiball's ldrs waveform) above the closing call to action
    $$('.final').forEach(function (f) {
      var w = document.createElement('div'); w.className = 'wave-loader'; w.setAttribute('aria-hidden', 'true');
      for (var i = 0; i < 5; i++) w.appendChild(document.createElement('i'));
      f.insertBefore(w, f.firstChild);
      anime({ targets: $$('i', w), scaleY: [0.25, 1], duration: 500, delay: anime.stagger(80), direction: 'alternate', loop: true, easing: 'easeInOutSine' });
    });

  }

  function onVisible(el, fn, t) {
    if (!('IntersectionObserver' in window)) { fn(); return; }
    var io = new IntersectionObserver(function (es) { if (es[0].isIntersecting) { io.disconnect(); fn(); } }, { threshold: t || 0.2 });
    io.observe(el);
  }
})();

// Terms of service: show which country's law applies, worked out from this device's time zone, then its language setting. Nothing is sent anywhere.
(function () {
  var el = document.getElementById('jurisdiction');
  if (!el) return;
  var ZONES = {
    'America/New_York': 'US', 'America/Chicago': 'US', 'America/Denver': 'US', 'America/Los_Angeles': 'US', 'America/Phoenix': 'US', 'America/Anchorage': 'US', 'America/Detroit': 'US', 'Pacific/Honolulu': 'US',
    'America/Toronto': 'CA', 'America/Vancouver': 'CA', 'America/Edmonton': 'CA', 'America/Winnipeg': 'CA', 'America/Halifax': 'CA', 'America/St_Johns': 'CA',
    'America/Mexico_City': 'MX', 'America/Sao_Paulo': 'BR', 'America/Argentina/Buenos_Aires': 'AR', 'America/Bogota': 'CO', 'America/Lima': 'PE', 'America/Santiago': 'CL', 'America/Caracas': 'VE',
    'Europe/London': 'GB', 'Europe/Dublin': 'IE', 'Europe/Paris': 'FR', 'Europe/Berlin': 'DE', 'Europe/Madrid': 'ES', 'Europe/Rome': 'IT', 'Europe/Amsterdam': 'NL', 'Europe/Brussels': 'BE', 'Europe/Zurich': 'CH', 'Europe/Vienna': 'AT',
    'Europe/Stockholm': 'SE', 'Europe/Oslo': 'NO', 'Europe/Copenhagen': 'DK', 'Europe/Helsinki': 'FI', 'Europe/Warsaw': 'PL', 'Europe/Prague': 'CZ', 'Europe/Lisbon': 'PT', 'Europe/Athens': 'GR', 'Europe/Istanbul': 'TR',
    'Europe/Kyiv': 'UA', 'Europe/Kiev': 'UA', 'Europe/Moscow': 'RU', 'Europe/Budapest': 'HU', 'Europe/Bucharest': 'RO', 'Europe/Sofia': 'BG', 'Europe/Belgrade': 'RS', 'Europe/Zagreb': 'HR', 'Europe/Luxembourg': 'LU',
    'Asia/Dubai': 'AE', 'Asia/Kolkata': 'IN', 'Asia/Calcutta': 'IN', 'Asia/Karachi': 'PK', 'Asia/Dhaka': 'BD', 'Asia/Bangkok': 'TH', 'Asia/Ho_Chi_Minh': 'VN', 'Asia/Singapore': 'SG', 'Asia/Kuala_Lumpur': 'MY', 'Asia/Jakarta': 'ID', 'Asia/Manila': 'PH',
    'Asia/Hong_Kong': 'HK', 'Asia/Shanghai': 'CN', 'Asia/Taipei': 'TW', 'Asia/Seoul': 'KR', 'Asia/Tokyo': 'JP', 'Asia/Jerusalem': 'IL', 'Asia/Riyadh': 'SA', 'Asia/Tehran': 'IR', 'Asia/Colombo': 'LK', 'Asia/Kathmandu': 'NP',
    'Australia/Sydney': 'AU', 'Australia/Melbourne': 'AU', 'Australia/Brisbane': 'AU', 'Australia/Perth': 'AU', 'Australia/Adelaide': 'AU', 'Australia/Hobart': 'AU', 'Pacific/Auckland': 'NZ',
    'Africa/Johannesburg': 'ZA', 'Africa/Lagos': 'NG', 'Africa/Cairo': 'EG', 'Africa/Nairobi': 'KE', 'Africa/Casablanca': 'MA', 'Africa/Accra': 'GH', 'Africa/Algiers': 'DZ'
  };
  var code = '';
  try { code = ZONES[Intl.DateTimeFormat().resolvedOptions().timeZone] || ''; } catch (e) { /* no time zone: use the language instead */ }
  if (!code) {
    var langs = navigator.languages || [navigator.language || ''];
    for (var i = 0; i < langs.length && !code; i++) { var m = /[-_]([A-Za-z]{2})$/.exec(langs[i] || ''); if (m) code = m[1].toUpperCase(); }
  }
  try { if (code) el.textContent = new Intl.DisplayNames(['en'], { type: 'region' }).of(code); } catch (e) { /* keep the plain wording */ }
})();

