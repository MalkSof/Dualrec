# Dualrec Web

SPA en Vue 3 + Vite + TypeScript con dos pantallas:

- **`/v/:token` — reproductor del link para compartir.** Muestra la pantalla como video principal, la cámara en una burbuja que se puede ocultar o mover de esquina, y el micrófono y el audio del sistema como pistas que se prenden y apagan. Las cuatro pistas se reproducen sincronizadas. Incluye velocidad, pantalla completa, atajos de teclado (espacio, flechas, `f`) y la descarga de cada pista. Mientras el video sube o se procesa, la página espera y se actualiza sola.
- **`/` — biblioteca.** Lista las grabaciones del usuario (entra con su token de acceso) y permite copiar el link, renombrar, pasar a privado y borrar.

## Correr

```bash
cd apps/web
npm install
cp .env.example .env    # VITE_API_URL apunta a la API
npm run dev             # http://localhost:5173
npm run build           # genera dist/ para publicar como sitio estático
```

En AWS la publica `infra/aws/deploy.ps1` (S3 + CloudFront), compilada con `VITE_API_URL=/api` para usar la API del mismo dominio.
