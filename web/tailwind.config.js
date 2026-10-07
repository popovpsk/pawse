import plugin from "tailwindcss/plugin";

export default {
  content: ["./index.html", "./src/**/*.{svelte,ts}"],
  theme: {
    extend: {},
  },
  plugins: [
    plugin(({ addVariant }) => {
      addVariant("can-hover", "@media (hover: hover) and (pointer: fine)");
    }),
  ],
  future: {
    hoverOnlyWhenSupported: true,
  },
};
