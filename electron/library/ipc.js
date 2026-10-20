const { ipcMain, BrowserWindow, dialog } = require('electron');
const { AUDIO_EXT, scanPaths } = require('./scanner');

const windowOf = (event) => BrowserWindow.fromWebContents(event.sender);

function registerLibraryIpc() {
  ipcMain.handle('local:pickFolder', async (event) => {
    const result = await dialog.showOpenDialog(windowOf(event), { properties: ['openDirectory', 'multiSelections'] });
    return result.canceled ? [] : result.filePaths;
  });

  ipcMain.handle('local:pickFiles', async (event) => {
    const result = await dialog.showOpenDialog(windowOf(event), {
      properties: ['openFile', 'multiSelections'],
      filters: [{ name: 'Audio', extensions: [...AUDIO_EXT].map((ext) => ext.slice(1)) }],
    });
    return result.canceled ? [] : result.filePaths;
  });

  ipcMain.handle('local:scan', (_event, paths) => scanPaths(paths));
}

module.exports = { registerLibraryIpc };
