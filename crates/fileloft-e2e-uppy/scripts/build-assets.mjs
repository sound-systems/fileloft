/**
 * Bundles Uppy + plugins into static/vendor/ so the Rust server serves them locally (no CDN).
 * Run from crate root: npm ci && npm run build
 */
import * as esbuild from 'esbuild';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const vendor = join(root, 'static', 'vendor');
mkdirSync(vendor, { recursive: true });

await esbuild.build({
  entryPoints: [join(root, 'e2e-entry.mjs')],
  bundle: true,
  outfile: join(vendor, 'uppy-e2e.js'),
  format: 'esm',
  minify: true,
  platform: 'browser',
  logLevel: 'info',
});

copyFileSync(
  join(root, 'node_modules', '@uppy', 'core', 'dist', 'style.min.css'),
  join(vendor, 'uppy-core.min.css'),
);
copyFileSync(
  join(root, 'node_modules', '@uppy', 'dashboard', 'dist', 'style.min.css'),
  join(vendor, 'uppy-dashboard.min.css'),
);

console.log('OK: static/vendor/uppy-e2e.js, uppy-core.min.css, uppy-dashboard.min.css');
