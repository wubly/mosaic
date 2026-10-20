const READY_CHECK = `({ rows: !!document.querySelector('[data-testid="tracklist-row"]'), text: (document.body && document.body.innerText || '').slice(0, 4000) })`;

const SCRAPE = `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const parseDur = (s) => { const p = (s || '').trim().split(':').map(Number); return !p.length || p.some(isNaN) ? 0 : p.reduce((a, b) => a * 60 + b, 0) * 1000; };
  const firstRow = document.querySelector('[data-testid="tracklist-row"]');
  let scroller = null;
  for (let el = firstRow?.parentElement; el && !scroller; el = el.parentElement) {
    const oy = getComputedStyle(el).overflowY;
    if ((oy === 'auto' || oy === 'scroll' || oy === 'hidden') && el.scrollHeight > el.clientHeight + 4) scroller = el;
  }
  scroller = scroller || document.scrollingElement;
  const main = document.querySelector('main') || document.body;
  const heading = ((document.querySelector('[data-testid="entityTitle"]') || main.querySelector('h1'))?.textContent || '').trim();
  const pageTitle = (document.title || '').replace(/\\s*\\|\\s*Spotify\\s*$/i, '');
  const titleMatch = /^(.*?)\\s+[-–]\\s+(?:playlist|album)\\b/i.exec(pageTitle);
  const rawName = heading || (titleMatch ? titleMatch[1].trim() : pageTitle.trim());
  const name = rawName && !/^spotify$/i.test(rawName) ? rawName : 'Spotify playlist';
  const totalText = (main.innerText.match(/(\\d[\\d,]*)\\s+(?:songs?|tracks?)/i) || [])[1];
  const total = totalText ? parseInt(totalText.replace(/,/g, ''), 10) : 0;
  const seen = new Map();
  const collect = () => {
    document.querySelectorAll('[data-testid="tracklist-row"]').forEach((row) => {
      const title = row.querySelector('a[data-testid="internal-track-link"]')?.textContent?.trim();
      if (!title) return;
      const artists = [...row.querySelectorAll('a[href*="/artist/"]')].map((a) => a.textContent.trim()).filter(Boolean);
      const cells = row.querySelectorAll('[role="gridcell"]');
      const idx = parseInt(row.getAttribute('aria-rowindex') || row.parentElement?.getAttribute('aria-rowindex') || '', 10);
      const key = title + '|' + artists.join(',') + '|' + (idx || '');
      if (!seen.has(key)) seen.set(key, { title, artist: artists.join(', '), duration: parseDur(cells[cells.length - 1]?.textContent), order: idx || seen.size });
    });
  };
  const started = Date.now();
  let stale = 0;
  scroller.scrollTop = 0;
  await sleep(300);
  for (let i = 0; i < 1500 && stale < 4 && Date.now() - started < 100000; i++) {
    const before = seen.size;
    collect();
    if (total && seen.size >= total) break;
    const atBottom = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4;
    scroller.scrollTop += Math.max(200, scroller.clientHeight * 0.5);
    await sleep(260);
    collect();
    await sleep(260);
    collect();
    stale = seen.size === before && atBottom ? stale + 1 : 0;
  }
  return { name, total, tracks: [...seen.values()].sort((a, b) => a.order - b.order).map(({ order, ...t }) => t) };
})()`;

module.exports = { READY_CHECK, SCRAPE };
