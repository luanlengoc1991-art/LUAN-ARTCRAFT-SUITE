// The status window's bridge to the main process: its state and its two buttons.
'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('bridge', {
  state: () => ipcRenderer.invoke('state'),
  signIn: () => ipcRenderer.invoke('signIn'),
  check: () => ipcRenderer.invoke('check'),
  onState: (fn) => ipcRenderer.on('state', (_e, s) => fn(s)),
});
