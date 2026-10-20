const { app } = require('electron');
const fs = require('fs');
const path = require('path');
const { APP_ID } = require('./constants');

let lowMemory = true;

function applyFlags() {
  app.setAppUserModelId(APP_ID);
  app.commandLine.appendSwitch('disable-features', 'HardwareMediaKeyHandling,MediaSessionService,CalculateNativeWinOcclusion');

  try {
    const early = JSON.parse(fs.readFileSync(path.join(app.getPath('userData'), 'mosaic-config.json'), 'utf8'));
    lowMemory = early.lowMemory !== false;
  } catch {}

  if (lowMemory) {
    app.commandLine.appendSwitch('enable-low-end-device-mode');
    app.disableHardwareAcceleration();
  }
}

const isLowMemory = () => lowMemory;

module.exports = { applyFlags, isLowMemory };
