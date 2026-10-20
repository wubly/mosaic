function parseSpotifyUrl(input = '') {
  const web = /open\.spotify\.com\/(?:intl-[a-z-]+\/)?(playlist|album)\/([A-Za-z0-9]+)/.exec(input);
  const uri = /spotify:(playlist|album):([A-Za-z0-9]+)/.exec(input);
  const match = web || uri;
  return match ? { kind: match[1], id: match[2] } : null;
}

const canonicalUrl = (input, ref) => (/^https?:/.test(input.trim()) ? input.trim() : `https://open.spotify.com/${ref.kind}/${ref.id}`);

module.exports = { parseSpotifyUrl, canonicalUrl };
