import { build } from 'esbuild';

await build({
  entryPoints: ['src/index.tsx'],
  bundle: true,
  minify: true,
  format: 'iife',
  target: 'es2020',
  outfile: 'dist/bundle.js',
});
await build({
  entryPoints: ['src/styles.css'],
  bundle: true,
  minify: true,
  outfile: 'dist/styles.css',
});
