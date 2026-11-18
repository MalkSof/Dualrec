# Dualrec

Grabador estilo Loom que guarda **pantalla, cámara, micrófono y audio del sistema en pistas separadas**, listas para editar. Es la primera pieza de un producto más grande: subir las grabaciones a la nube y consultarlas con IA (ver [Hoja de ruta](#hoja-de-ruta)).

> **Estado: etapas 1 y 2.** La app (Windows) graba y deja una carpeta por grabación con `screen.mp4`, `camera.mp4`, `mic.m4a` y `system.m4a` sincronizados; si se activa la nube, las sube mientras graba y deja un link para compartir con reproductor web. El procesamiento con IA son las etapas siguientes.

## Qué hace hoy

- Elegís **pantalla o ventana**, **cámara**, **micrófono** y si querés el **audio del sistema**.
- **Burbuja de cámara flotante**, siempre visible, que se arrastra y cambia de tamaño con doble clic.
- **Grabar, pausar y cortar**, también con atajos globales: `Ctrl+Alt+R` (grabar / cortar) y `Ctrl+Alt+P` (pausar / reanudar).
- Dualrec y la burbuja **no salen en `screen.mp4`** (se excluyen de la captura; se puede desactivar).
- **H.264 por hardware** cuando hay GPU (NVENC, Quick Sync o AMF) y x264 por CPU si no.
- Cada grabación queda en `Videos\Dualrec\<fecha_hora>\` junto a un `session.json` con el detalle de cada pista.

```
Videos/Dualrec/2026-11-18_18-05-48/
├── screen.mp4     H.264, resolución de la pantalla (o limitada a 1080p)
├── camera.mp4     H.264, resolución de la cámara
├── mic.m4a        AAC, micrófono
├── system.m4a     AAC, lo que suena por los parlantes
└── session.json   manifiesto: pistas, duración, offsets, tamaños
```

## Nube y link para compartir

En **Ajustes → Nube** de la app se carga la dirección de la API y el token de acceso. Con eso activado:

- Cada pista se sube **mientras se graba**, en partes de 8 MB, directo al almacenamiento.
- Al cortar, el **link para compartir queda en el portapapeles**. El reproductor web muestra la pantalla con la cámara en una burbuja que se puede ocultar, y permite bajar cada pista.
- **Si se corta internet, la grabación sigue** y la subida se retoma sola cuando vuelve la conexión.
- **Si la app se cierra a mitad**, al reabrirla retoma las subidas pendientes desde donde quedaron (el estado se guarda en `upload.json`, dentro de la carpeta de la grabación).

Para probar todo en una sola máquina, sin Docker ni cuentas en la nube:

```powershell
cd apps/api;  uv sync;  uv run python dev.py     # API en :8000 con almacenamiento simulado
cd apps/web;  npm install;  npm run dev          # web en :5173
```

y en la app usar `http://127.0.0.1:8000` con el token que imprime `dev.py`. Más detalle en [apps/api](apps/api/README.md) y [apps/web](apps/web/README.md).

### Producción: todo en AWS

Los videos van a **S3**, la base a **RDS PostgreSQL**, la API a **ECS Fargate** y la web a **S3 + CloudFront**, todo bajo un mismo dominio con HTTPS. Se despliega con un comando:

```powershell
powershell -ExecutionPolicy Bypass -File infra/aws/deploy.ps1 -AdminEmail vos@ejemplo.com
```

Arquitectura, costo aproximado y detalles en [infra/aws](infra/aws/README.md).

## Estructura del monorepo

```
apps/
  desktop/        App de escritorio (Tauri 2 + Vue 3 + TypeScript)
    src/          Interfaz: ventana principal y burbuja
    src-tauri/    Motor de captura en Rust
  api/            API en FastAPI: videos, subida multipart, link para compartir
  web/            SPA en Vue 3: reproductor y biblioteca
infra/
  aws/            Plantillas de CloudFormation y script de despliegue
scripts/
  fetch-ffmpeg.ps1   Baja el FFmpeg que se empaqueta con la app
  make-icon.ps1      Genera el ícono fuente
```

### Módulos del motor (`apps/desktop/src-tauri/src`)

| Archivo | Responsabilidad |
| --- | --- |
| `clock.rs` | Reloj monotónico común a todas las pistas (con pausas) |
| `screen.rs` | Captura de pantalla o ventana (Windows Graphics Capture) |
| `camera.rs` | Cámara y vista previa |
| `audio.rs` | Micrófono y audio del sistema (WASAPI loopback) |
| `ffmpeg.rs` | Sidecar de FFmpeg: detección de encoder y argumentos |
| `recorder.rs` | Orquesta una grabación: pistas, sincronía, manifiesto |
| `cloud.rs` | Subida multipart mientras se graba, con reintentos y reanudación |
| `preview.rs` | Servidor local MJPEG para la burbuja |
| `main.rs` | Comandos de Tauri, atajos globales y autoprueba |

### Crates y por qué

| Crate | Para qué |
| --- | --- |
| `tauri` 2 | Ventanas nativas livianas con la interfaz en Vue |
| `windows-capture` | Windows Graphics Capture: captura por GPU de pantallas y ventanas |
| `nokhwa` | Cámara multiplataforma (Media Foundation en Windows) |
| `cpal` | Micrófono y loopback de WASAPI para el audio del sistema |
| `image` | JPEG de la vista previa de la cámara |
| `parking_lot`, `crossbeam-channel` | Estado compartido y colas entre hilos de captura |
| `serde`, `serde_json` | Manifiesto `session.json` y mensajes con la interfaz |
| `chrono`, `dirs`, `anyhow` | Nombres de carpeta, rutas del usuario y errores |
| `tauri-plugin-global-shortcut`, `tauri-plugin-dialog` | Atajos globales y selector de carpeta |
| `ureq` | Cliente HTTP simple y bloqueante para la subida (corre en su propio hilo) |
| `arboard` | Copiar el link para compartir al portapapeles |

## Cómo se sincronizan las pistas

Todas las pistas comparten **un reloj monotónico** que arranca en `t0`, cuando todas las fuentes ya están entregando datos.

- **Video**: se escribe a ritmo constante. El cuadro `k` corresponde al instante `k / fps`; si la fuente no entregó un cuadro nuevo (una pantalla quieta no genera cuadros), se repite el último.
- **Audio**: se cuentan muestras contra el mismo reloj. Los huecos se rellenan con silencio (el loopback de WASAPI no entrega nada cuando no suena nada) y nunca se adelanta al reloj.
- **Pausa**: congela el reloj, así que todas las pistas saltan el mismo tramo.
- **Corte**: primero se congela el reloj y después se cierran las pistas, por lo que todas terminan en el mismo instante.

Resultado: todas las pistas empiezan en `t0` (`offset_ms: 0` en el manifiesto) y duran lo mismo con una diferencia menor a un cuadro. Se alinean en el editor poniéndolas todas al inicio de la línea de tiempo.

**Si la app se cierra a mitad de grabación** no se pierde lo grabado: cada pista se escribe como MP4 fragmentado (`*.part.mp4` / `*.part.m4a`), que es reproducible aunque quede incompleto. Al cortar normalmente se convierte a MP4 clásico sin recodificar.

## Compilar y correr

### Windows

Requisitos:

1. **Node.js 20+**.
2. **Rust** (rustup). Sirve cualquiera de las dos toolchains:
   - MSVC (recomendada por Tauri): instalar *Visual Studio Build Tools* con "Desarrollo para el escritorio con C++".
   - GNU (sin Visual Studio): `rustup default stable-x86_64-pc-windows-gnu` y un MinGW-w64 en el `PATH`, por ejemplo `winget install BrechtSanders.WinLibs.POSIX.UCRT`.
3. **WebView2** (ya viene con Windows 11).
4. **FFmpeg como sidecar**:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1
```

Después:

```powershell
cd apps/desktop
npm install
npm run tauri dev      # desarrollo, con recarga en caliente
npm run tauri build    # instalador en src-tauri/target/release/bundle/nsis/
```

### macOS

Todavía **no está soportado**. Tauri, `nokhwa` y `cpal` ya son multiplataforma, pero falta el backend de captura de pantalla y de audio del sistema con ScreenCaptureKit y el manejo de permisos de macOS. Es el siguiente paso de la etapa 1 y entra detrás de las mismas interfaces (`screen.rs`, `audio.rs`).

## Tests y autoprueba

```powershell
cd apps/desktop/src-tauri
cargo test
```

Los tests cubren el reloj y las pausas, la cuenta de cuadros y muestras, el relleno de silencio, los argumentos de FFmpeg, el ajuste de cuadros cuando cambia el tamaño de la ventana y el manifiesto.

Para probar el motor completo sin interfaz (graba la pantalla principal, cámara, micrófono y audio del sistema, con una pausa en el medio, e imprime el manifiesto):

```powershell
cargo run -- --selftest 10 C:\ruta\de\salida
cargo run -- --selftest 10 C:\ruta\de\salida --fake-camera   # cámara sintética
cargo run -- --caminfo                                        # formatos de la cámara
```

Variables útiles en desarrollo: `DUALREC_ENCODER=x264|nvenc|qsv|amf` fuerza el encoder, `DUALREC_FAKE_CAMERA=1` usa una cámara sintética y `DUALREC_NO_PROTECT=1` deja las ventanas de Dualrec visibles para capturas de pantalla.

## Permisos y problemas frecuentes

- **Cámara o micrófono no abren**: revisar *Configuración → Privacidad y seguridad → Cámara / Micrófono* y que "Permitir que las aplicaciones de escritorio accedan" esté activado.
- **"La cámara no está entregando imagen"**: otra app la está usando (OBS, Teams, el navegador), la tapa de la notebook está cerrada o el obturador de privacidad está activado.
- **El micrófono graba silencio**: mirar el medidor de nivel debajo del selector; si no se mueve, elegir otro micrófono.
- **Ventana minimizada**: Windows no entrega imagen de ventanas minimizadas; hay que restaurarla antes de grabar.

## Diferencias con la especificación original

- **Captura de pantalla**: se usa `windows-capture` directamente en lugar de `scap`. En Windows `scap` es una capa sobre ese mismo crate, y usarlo directo da control del ritmo de cuadros, del borde amarillo y del cursor. El backend de macOS se suma detrás del mismo módulo.
- **Fragmentos en disco**: en vez de escribir archivos sueltos de 8 MB, cada pista se graba como un único MP4 fragmentado (seguro ante cierres) y la subida lo va leyendo en partes de 8 MB a medida que crece. El resultado es el mismo y en disco queda un solo archivo por pista.
- **Procesamiento mínimo en la etapa 2**: al cerrar un video la API reempaqueta cada pista (sin recodificar) para que el navegador pueda saltar a cualquier minuto. Corre como tarea en segundo plano de FastAPI; Celery + Redis entran en la etapa 3, cuando haya trabajo pesado.
- **Todo en AWS**: la especificación mencionaba DigitalOcean Spaces; el almacenamiento, la base, la API y la web van en AWS (S3, RDS, ECS Fargate, CloudFront).
- **Acceso por token**: la especificación no define el inicio de sesión. Por ahora cada usuario tiene un token de acceso que se crea por consola; el registro con usuario y contraseña queda para cuando haya planes y equipos.

## Hoja de ruta

| Etapa | Entrega | Estado |
| --- | --- | --- |
| 1. Grabador local | App que graba pantalla, cámara, micrófono y audio del sistema en archivos separados | Windows listo · macOS pendiente |
| 2. Nube | API FastAPI, subida multipart reanudable, link para compartir | Lista, probada en local · falta desplegarla en AWS |
| 3. Procesamiento | Video compuesto, transcripción, resumen y capítulos | Pendiente |
| 4. Chat con IA | Segmentos con embeddings, chat con citas al minuto | Pendiente |
| 5. MCP + visual | Servidor MCP e indexado de lo que se ve en pantalla | Pendiente |

## Licencias de terceros

Dualrec ejecuta **FFmpeg** como un programa aparte (sidecar). El build que baja `scripts/fetch-ffmpeg.ps1` está licenciado bajo **GPL v3**; su código fuente y licencia están en <https://ffmpeg.org> y <https://www.gyan.dev/ffmpeg/builds/>. Si se distribuye el instalador, hay que acompañarlo con ese aviso.
