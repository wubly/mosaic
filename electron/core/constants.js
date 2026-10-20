const path = require('path');

const ROOT = path.join(__dirname, '..', '..');

module.exports = {
  DEV: !!process.env.MOSAIC_DEV,
  ROOT,
  OUT_DIR: path.join(ROOT, 'out'),
  APP_ICON: path.join(ROOT, 'build', process.platform === 'win32' ? 'icon.ico' : 'icon.png'),
  PRELOAD: path.join(__dirname, '..', 'preload.js'),
  DEV_URL: 'http://localhost:3917',
  APP_URL: 'app://mosaic/index.html',
  APP_ID: 'app.mosaic.player',
  USER_AGENT: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36',
};
