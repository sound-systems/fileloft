import Uppy from '@uppy/core';
import Dashboard from '@uppy/dashboard';
import Tus from '@uppy/tus';

window.__e2eLogs = [];
const origError = console.error;
console.error = (...args) => {
  window.__e2eLogs.push(args.map(String).join(' '));
  origError.apply(console, args);
};

const uppy = new Uppy({ id: 'fileloft-e2e', autoProceed: true })
  .use(Dashboard, { inline: true, target: '#uppy-dashboard' })
  .use(Tus, {
    endpoint: '/files',
    // Small chunk keeps the tiny E2E fixture to a single PATCH (fewer round trips).
    chunkSize: 64 * 1024,
    limit: 1,
  });

function markUploadComplete(el, uploadURL) {
  if (!el) {
    return;
  }
  el.dataset.uploadUrl = uploadURL || '';
  el.classList.add('complete');
  el.textContent = 'upload-complete';
}

uppy.on('upload-success', (_file, response) => {
  const el = document.getElementById('upload-status');
  const uploadURL =
    (response && response.uploadURL) ||
    (response && response.uploadUrl) ||
    '';
  markUploadComplete(el, uploadURL);
});

// Fallback: some Uppy/tus-js-client versions populate uploadURL only on the file object.
// `result.successful` is an array of file objects (not IDs).
uppy.on('complete', (result) => {
  const el = document.getElementById('upload-status');
  if (!el || el.classList.contains('complete')) {
    return;
  }
  const file = result.successful && result.successful[0];
  if (!file) {
    return;
  }
  const uploadURL =
    file.uploadURL ||
    (file.tus && file.tus.uploadUrl) ||
    (file.response && file.response.uploadURL) ||
    '';
  markUploadComplete(el, uploadURL);
});

// Expose for diagnostics in e2e test
window.uppy = uppy;
