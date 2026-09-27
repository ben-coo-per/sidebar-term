<!-- The phone's page, served by the Mac app's Remote server at /m (src-tauri/src/remote/).
     Installs as a home-screen app over HTTPS; its service worker is registered here only. -->
<script lang="ts">
  import "$lib/theme.css";
  import Mobile from "$lib/mobile/Mobile.svelte";

  $effect(() => {
    if (!("serviceWorker" in navigator) || location.protocol !== "https:") return;
    navigator.serviceWorker.register("/service-worker.js").catch((e) => console.warn("service worker:", e));
  });
</script>

<svelte:head>
  <title>sidebar-term</title>
</svelte:head>

<Mobile />
