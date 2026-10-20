const { app } = require('electron');
const fs = require('fs');
const path = require('path');

const userFile = (name) => path.join(app.getPath('userData'), name);

const readJson = (file, fallback) => {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')); } catch { return fallback; }
};

const writeJson = (file, data) => fs.writeFileSync(file, JSON.stringify(data));

const readConfig = () => readJson(userFile('mosaic-config.json'), {});

const writeConfig = (config) => writeJson(userFile('mosaic-config.json'), config);

module.exports = { userFile, readJson, writeJson, readConfig, writeConfig };
