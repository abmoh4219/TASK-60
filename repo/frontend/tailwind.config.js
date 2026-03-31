/** @type {import('tailwindcss').Config} */
module.exports = {
  // Scan all Rust source files and the HTML entry point for utility classes.
  content: [
    './src/**/*.rs',
    './index.html',
  ],

  // Safelist colour variants that are assembled dynamically at runtime
  // (e.g. category_color() and quality_badge() in api/kiosk.rs and pages).
  safelist: [
    {
      pattern: /^(bg|text|border|ring|from|via|to)-(indigo|slate|red|green|emerald|yellow|amber|blue|purple|orange|rose|sky|violet)-(50|100|200|300|400|500|600|700|800|900|950)$/,
    },
    { pattern: /^(bg|text|border)-(white|black|transparent)$/ },
    // Opacity modifiers used in hover utilities e.g. "hover:bg-indigo-50/30"
    { pattern: /^(bg|text|border)-(indigo|slate|red|green|emerald|yellow|amber|blue)-(50|100|200|300|400|500|600|700|800|900)\/\d+$/ },
  ],

  theme: {
    extend: {
      fontFamily: {
        // Inter is preferred; system-ui is the self-contained fallback.
        sans: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
      },
      colors: {
        surface: {
          DEFAULT: '#ffffff',
          raised:  '#f8fafc',
        },
      },
      boxShadow: {
        card:         '0 1px 3px 0 rgb(0 0 0 / 0.08), 0 1px 2px -1px rgb(0 0 0 / 0.06)',
        'card-hover': '0 4px 12px 0 rgb(0 0 0 / 0.10)',
        panel:        '0 8px 30px rgb(0 0 0 / 0.12)',
      },
      keyframes: {
        'slide-in-right': {
          '0%':   { transform: 'translateX(100%)', opacity: '0' },
          '100%': { transform: 'translateX(0)',    opacity: '1' },
        },
        'fade-in': {
          '0%':   { opacity: '0', transform: 'translateY(4px)' },
          '100%': { opacity: '1', transform: 'translateY(0)'   },
        },
      },
      animation: {
        'slide-in-right': 'slide-in-right 0.2s ease-out',
        'fade-in':        'fade-in 0.15s ease-out',
      },
    },
  },

  plugins: [],
};
