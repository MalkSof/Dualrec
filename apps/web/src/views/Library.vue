<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { api, ApiError, formatClock, formatDate, formatSize, STATUS_LABEL, token, type Video } from "../api";

const signedIn = ref(!!token.get());
const tokenInput = ref("");
const userName = ref("");
const videos = ref<Video[]>([]);
const loading = ref(false);
const error = ref("");
const copied = ref("");
const confirming = ref("");

let poll = 0;

async function load() {
  if (!signedIn.value) return;
  try {
    videos.value = await api.videos();
    error.value = "";
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) signOut("El token ya no es válido. Ingresalo de nuevo.");
    else error.value = e instanceof Error ? e.message : String(e);
  }
}

async function signIn() {
  if (!tokenInput.value.trim()) return;
  loading.value = true;
  token.set(tokenInput.value);
  try {
    userName.value = (await api.me()).name;
    signedIn.value = true;
    tokenInput.value = "";
    error.value = "";
    await load();
  } catch (e) {
    token.clear();
    error.value =
      e instanceof ApiError && e.status === 401
        ? "Ese token no es válido."
        : e instanceof Error
          ? e.message
          : String(e);
  } finally {
    loading.value = false;
  }
}

function signOut(message = "") {
  token.clear();
  signedIn.value = false;
  videos.value = [];
  error.value = message;
}

async function copyLink(v: Video) {
  const link = `${location.origin}/v/${v.share_token}`;
  await navigator.clipboard.writeText(link);
  copied.value = v.id;
  setTimeout(() => copied.value === v.id && (copied.value = ""), 1800);
}

async function rename(v: Video, title: string) {
  const clean = title.trim();
  if (!clean || clean === v.title) return;
  try {
    Object.assign(v, await api.update(v.id, { title: clean }));
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

async function toggleVisibility(v: Video) {
  try {
    Object.assign(v, await api.update(v.id, { visibility: v.visibility === "link" ? "private" : "link" }));
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

async function remove(v: Video) {
  if (confirming.value !== v.id) {
    confirming.value = v.id;
    setTimeout(() => confirming.value === v.id && (confirming.value = ""), 4000);
    return;
  }
  try {
    await api.remove(v.id);
    videos.value = videos.value.filter((x) => x.id !== v.id);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

const totalSize = (v: Video) => v.tracks.reduce((a, t) => a + t.size_bytes, 0);

onMounted(async () => {
  document.title = "Biblioteca · Dualrec";
  if (signedIn.value) {
    void api.me().then((m) => (userName.value = m.name)).catch(() => {});
    await load();
  }
  // Los videos que están subiendo o procesándose cambian de estado solos.
  poll = window.setInterval(() => {
    if (videos.value.some((v) => v.status !== "ready" && v.status !== "failed")) void load();
  }, 4000);
});

onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <main class="library">
    <form v-if="!signedIn" class="signin" @submit.prevent="signIn">
      <h1>Tu biblioteca de Dualrec</h1>
      <p>Ingresá tu token de acceso para ver y compartir tus grabaciones.</p>
      <label for="token">Token de acceso</label>
      <input id="token" v-model="tokenInput" type="password" placeholder="dr_…" autocomplete="off" spellcheck="false" />
      <button class="primary" :disabled="loading || !tokenInput.trim()">{{ loading ? "Verificando…" : "Entrar" }}</button>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </form>

    <template v-else>
      <div class="head">
        <h1>Biblioteca</h1>
        <span class="who">{{ userName }}</span>
        <button class="link" @click="signOut()">Salir</button>
      </div>
      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <div v-if="!videos.length" class="empty">
        <h2>Todavía no hay grabaciones</h2>
        <p>Activá “Subir a la nube” en la app de escritorio y tu próxima grabación va a aparecer acá.</p>
      </div>

      <article v-for="v in videos" :key="v.id" class="item">
        <RouterLink :to="`/v/${v.share_token}`" class="thumb" :aria-label="`Ver ${v.title}`">
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5Z" /></svg>
          <span v-if="v.duration_ms">{{ formatClock(v.duration_ms) }}</span>
        </RouterLink>
        <div class="info">
          <input
            class="title"
            :value="v.title"
            aria-label="Título"
            @change="rename(v, ($event.target as HTMLInputElement).value)"
            @keydown.enter="($event.target as HTMLInputElement).blur()"
          />
          <p>
            <span class="status" :class="v.status">{{ STATUS_LABEL[v.status] }}</span>
            {{ formatDate(v.recorded_at) }} · {{ v.tracks.length }} {{ v.tracks.length === 1 ? "pista" : "pistas" }}
            <template v-if="totalSize(v)"> · {{ formatSize(totalSize(v)) }}</template>
          </p>
          <p v-if="v.error" class="warn">{{ v.error }}</p>
        </div>
        <div class="actions">
          <button class="ghost" :disabled="v.visibility !== 'link'" @click="copyLink(v)">{{ copied === v.id ? "¡Copiado!" : "Copiar link" }}</button>
          <button class="ghost" :title="v.visibility === 'link' ? 'Cualquiera con el link puede verlo' : 'Solo vos podés verlo'" @click="toggleVisibility(v)">
            {{ v.visibility === "link" ? "Público" : "Privado" }}
          </button>
          <button class="ghost danger" @click="remove(v)">{{ confirming === v.id ? "¿Borrar?" : "Borrar" }}</button>
        </div>
      </article>
    </template>
  </main>
</template>
