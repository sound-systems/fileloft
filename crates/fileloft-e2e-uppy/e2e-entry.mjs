import Uppy from '@uppy/core';
import Dashboard from '@uppy/dashboard';
import Tus from '@uppy/tus';

const uppy = new Uppy({ id: 'fileloft-e2e' })
  .use(Dashboard, { inline: true, target: '#uppy-dashboard' })
  .use(Tus, { endpoint: '/files/', chunkSize: 5 * 1024 * 1024, limit: 1 });

uppy.on('upload-success', (_file, response) => {
  const el = document.getElementById('upload-status');
  const uploadURL =
    (response && response.uploadURL) ||
    (response && response.uploadUrl) ||
    '';
  el.dataset.uploadUrl = uploadURL;
  el.classList.add('complete');
  el.textContent = 'upload-complete';
});
