const { sleep } = require('../core/util');

const MAX_IN_FLIGHT = 4;
const queue = [];
let inFlight = 0;
let serverFailures = 0;

const acquire = () => new Promise((resolve) => {
  const go = () => (inFlight < MAX_IN_FLIGHT ? (inFlight++, resolve()) : queue.push(go));
  go();
});

const release = () => {
  inFlight--;
  queue.shift()?.();
};

async function lrclib(pathAndQuery, attempts = 3) {
  await acquire();
  try {
    for (let i = 0; i < attempts; i++) {
      try {
        const res = await fetch(`https://lrclib.net/api/${pathAndQuery}`, {
          signal: AbortSignal.timeout(8000),
          headers: { 'User-Agent': 'Mosaic/0.2 (desktop player)', 'Lrclib-Client': 'Mosaic/0.2' },
        });
        if (res.status === 404) return null;
        if (res.ok) return await res.json();
        if (res.status < 500) return null;
      } catch {}
      await sleep(500 * (i + 1));
    }
    serverFailures++;
    return null;
  } finally {
    release();
  }
}

const failureCount = () => serverFailures;

module.exports = { lrclib, failureCount };
