import { createApp } from "vue";
import { createRouter, createWebHistory } from "vue-router";
import App from "./App.vue";
import Library from "./views/Library.vue";
import Watch from "./views/Watch.vue";
import "./styles.css";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", component: Library },
    { path: "/v/:token", component: Watch, props: true },
    { path: "/:rest(.*)*", redirect: "/" },
  ],
});

createApp(App).use(router).mount("#app");
