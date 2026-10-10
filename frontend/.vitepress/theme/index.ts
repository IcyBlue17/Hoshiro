import DefaultTheme from "vitepress/theme";
import type { Theme } from "vitepress";
import Landing from "./components/Landing.vue";
import Kbd from "./components/Kbd.vue";
import Pill from "./components/Pill.vue";
import "./style.css";

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component("Landing", Landing);
    app.component("Kbd", Kbd);
    app.component("Pill", Pill);
  },
} satisfies Theme;
