import { createRouter, createWebHashHistory } from "vue-router";
import { getToken } from "../api/api";

const routes = [
  {
    path: "/login",
    name: "Login",
    component: () => import("../views/Login.vue"),
  },
  {
    path: "/",
    name: "Home",
    component: () => import("../views/Home.vue"),
    meta: { requiresAuth: true },
    children: [
      {
        path: "",
        name: "DashboardRoot",
        redirect: "/dashboard",
      },
      {
        path: "dashboard",
        name: "Dashboard",
        component: () => import("../views/Dashboard.vue"),
      },
      {
        path: "servers",
        name: "Servers",
        component: () => import("../views/Servers.vue"),
      },
      {
        path: "projects",
        name: "Projects",
        component: () => import("../views/Projects.vue"),
      },
      {
        path: "deployments",
        name: "Deployments",
        component: () => import("../views/Deployments.vue"),
      },
      {
        path: "settings",
        name: "Settings",
        component: () => import("../views/Settings.vue"),
      },
      {
        path: "ci-generator",
        name: "CiGenerator",
        component: () => import("../views/CiGenerator.vue"),
      },
    ],
  },
];

const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

router.beforeEach((to) => {
  const token = getToken();

  if (to.meta.requiresAuth && !token) {
    return {
      path: "/login",
      query: { redirect: to.fullPath },
    };
  }

  if (to.path === "/login" && token) {
    return { path: "/" };
  }

  return true;
});

export default router;
