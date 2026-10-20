const { globalShortcut } = require('electron');
const { getMainWindow } = require('./window');

const KEYS = [
  ['MediaPlayPause', 'toggle'],
  ['MediaNextTrack', 'next'],
  ['MediaPreviousTrack', 'prev'],
  ['MediaStop', 'stop'],
];

function registerMediaKeys() {
  const send = (command) => getMainWindow()?.webContents.send('media-key', command);
  for (const [key, command] of KEYS) {
    try { globalShortcut.register(key, () => send(command)); } catch {}
  }
}

const unregisterMediaKeys = () => globalShortcut.unregisterAll();

module.exports = { registerMediaKeys, unregisterMediaKeys };
