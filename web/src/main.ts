import { createApp } from "vue";
import App from "./App.vue";
import "element-plus/dist/index.css";
import "./style.css";
import router from "./router/router";

const app = createApp(App);

app.use(router);

app.mount("#app");
