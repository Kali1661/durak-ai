/** @type {import('tailwindcss').Config} */
const token = name => `rgb(var(--${name}) / <alpha-value>)`

export default {
  content: ['./index.html', './src/**/*.{js,svelte}'],
  darkMode: 'class',
  theme: {
    extend: {
      // Every colour in the app comes from these tokens (defined in src/app.css),
      // so light and dark themes stay in step.
      colors: {
        page: token('page'),
        surface: token('surface'),
        sunken: token('sunken'),
        line: token('line'),
        ink: token('ink'),
        muted: token('muted'),
        accent: token('accent'),
        'on-accent': token('on-accent'),
        gold: token('gold'),
        felt: token('felt'),
        'felt-deep': token('felt-deep'),
        'on-felt': token('on-felt'),
        danger: token('danger'),
        attack: token('attack'),
        defend: token('defend'),
      },
      fontFamily: {
        display: ['Georgia', 'Cambria', '"Times New Roman"', 'serif'],
      },
    },
  },
  plugins: [],
}
