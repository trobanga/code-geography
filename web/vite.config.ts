import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  // Relative paths: the viewer also operates under a sub-path, such as GitHub Pages.
  base: './',
  plugins: [tailwindcss(), svelte()],
  test: { include: ['src/**/*.test.ts'] },
});
