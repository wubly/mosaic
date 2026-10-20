const { USER_AGENT } = require('../core/constants');

async function readEmbed({ kind, id }) {
  const res = await fetch(`https://open.spotify.com/embed/${kind}/${id}`, {
    headers: { 'User-Agent': USER_AGENT, 'Accept-Language': 'en' },
    signal: AbortSignal.timeout(15000),
  });
  if (!res.ok) return null;

  const html = await res.text();
  const data = html.match(/<script id="__NEXT_DATA__" type="application\/json">(.*?)<\/script>/s);
  if (!data) return null;

  const entity = JSON.parse(data[1])?.props?.pageProps?.state?.data?.entity;
  if (!entity?.trackList?.length) return null;

  return {
    via: 'embed',
    name: entity.name || entity.title || 'Spotify playlist',
    owner: entity.subtitle || '',
    cover: entity.coverArt?.sources?.[0]?.url || '',
    tracks: entity.trackList
      .map((t) => ({ title: t.title, artist: t.subtitle || '', duration: t.duration || 0 }))
      .filter((t) => t.title),
  };
}

module.exports = { readEmbed };
