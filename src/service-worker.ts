// The phone's service worker: caches the built page and assets so the installed home-screen
// app opens instantly and still opens (to a "reconnecting" banner) when the Mac is unreachable.
// Registered by src/routes/m/+page.svelte, only over HTTPS; the Mac's own webview never has it.
// The WebSocket and the pairing API are never cached.

/// <reference types="@sveltejs/kit" />
/// <reference no-default-lib="true"/>
/// <reference lib="esnext" />
/// <reference lib="webworker" />

import { build, files, version } from "$service-worker";

const sw = self as unknown as ServiceWorkerGlobalScope;
const CACHE = `sidebar-term-${version}`;
/** The page shell the SPA boots from (the Mac serves index.html at /m). */
const PAGE = "/m";
const ASSETS = [...build, ...files.filter((f) => f.startsWith("/icons/") || f === "/manifest.webmanifest")];

sw.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll([...ASSETS, PAGE]))
      .then(() => sw.skipWaiting()),
  );
});

sw.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => sw.clients.claim()),
  );
});

sw.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== location.origin || url.pathname === "/ws" || url.pathname.startsWith("/api/")) return;

  // The page: the network's copy when it answers, the cached shell when it does not.
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request)
        .then((res) => {
          void caches.open(CACHE).then((cache) => cache.put(PAGE, res.clone()));
          return res;
        })
        .catch(async () => (await caches.match(PAGE)) ?? Response.error()),
    );
    return;
  }

  // Built assets are immutable: the cache first.
  if (ASSETS.includes(url.pathname)) {
    event.respondWith(caches.match(request).then((hit) => hit ?? fetch(request)));
    return;
  }

  event.respondWith(fetch(request).catch(async () => (await caches.match(request)) ?? Response.error()));
});
