'use client';
import { useEffect } from 'react';
import { argbFromHex, hexFromArgb, sourceColorFromImage, themeFromSourceColor } from '@material/material-color-utilities';

const DEFAULT_SEED = '#6750a4';
const seedCache = new Map();

async function seedFromArtwork(url) {
  if (seedCache.has(url)) return seedCache.get(url);
  const img = new Image();
  img.crossOrigin = 'anonymous';
  img.src = url;
  await img.decode();
  const seed = hexFromArgb(await sourceColorFromImage(img));
  seedCache.set(url, seed);
  return seed;
}

function applyScheme(seed) {
  const { palettes: p } = themeFromSourceColor(argbFromHex(seed));
  const t = (pal, n) => hexFromArgb(pal.tone(n));
  const vars = {
    '--md-primary': t(p.primary, 80),
    '--md-on-primary': t(p.primary, 20),
    '--md-primary-container': t(p.primary, 30),
    '--md-on-primary-container': t(p.primary, 90),
    '--md-surface': t(p.neutral, 6),
    '--md-on-surface': t(p.neutral, 90),
    '--md-on-surface-variant': t(p.neutralVariant, 80),
    '--md-surface-container': t(p.neutral, 12),
    '--md-surface-container-high': t(p.neutral, 17),
    '--md-surface-container-highest': t(p.neutral, 22),
    '--md-outline': t(p.neutralVariant, 60),
    '--md-outline-variant': t(p.neutralVariant, 30),
  };
  const root = document.documentElement.style;
  for (const [k, v] of Object.entries(vars)) root.setProperty(k, v);
}

export default function useDynamicTheme(artwork, enabled = true) {
  useEffect(() => {
    let live = true;
    (async () => {
      if (!enabled || !artwork) return applyScheme(DEFAULT_SEED);
      try {
        const seed = await seedFromArtwork(artwork);
        if (live) applyScheme(seed);
      } catch {
        if (live) applyScheme(DEFAULT_SEED);
      }
    })();
    return () => { live = false; };
  }, [artwork, enabled]);
}
