/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './pages/**/*.{js,ts,jsx,tsx,mdx}',
    './components/**/*.{js,ts,jsx,tsx,mdx}',
    './app/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    extend: {
      colors: {
        background: '#0B0E14',
        surface: '#151922',
        primary: '#00F5FF',
        secondary: '#7928CA',
        accent: '#FF0080',
        warning: '#FFB800',
        success: '#00E676',
      },
    },
  },
  plugins: [],
}
