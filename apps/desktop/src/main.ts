import { createApp } from "vue";
import App from "./App.vue";
import Bubble from "./Bubble.vue";
import "./styles.css";

// La misma página sirve a las dos ventanas: la principal y la burbuja.
const isBubble = location.hash === "#bubble";
if (isBubble) document.documentElement.classList.add("bubble");
createApp(isBubble ? Bubble : App).mount("#app");
