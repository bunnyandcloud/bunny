/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        bunny: {
          bg: 'var(--bunny-bg)',
          panel: 'var(--bunny-panel)',
          border: 'var(--bunny-border)',
          accent: 'var(--bunny-accent)',
          muted: 'var(--bunny-muted)',
          fg: 'var(--bunny-fg)',
          'on-accent': 'var(--bunny-on-accent)',
          locked: 'var(--bunny-locked)',
        },
      },
    },
  },
  plugins: [],
};
