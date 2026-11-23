# Textos de lanzamiento de Dualrec

Textos listos para copiar y pegar. Cada uno va desde tu cuenta: Reddit, Product Hunt y Hacker News castigan las publicaciones automáticas.

Antes de publicar en cada lugar:

- Leé las reglas del subreddit. Varios limitan la autopromoción o piden una etiqueta ("flair") específica.
- Publicá uno por día, no todos juntos, y quedate una hora respondiendo comentarios.
- Adjuntá el video o una captura. Un post con imagen rinde mucho más que uno solo de texto.
- No publiques en shabat.

Orden sugerido: r/ClaudeAI → r/mcp → directorios de MCP → r/devsarg → r/NewTubers → Show HN → Product Hunt (el último, cuando la app ya esté en inglés).

---

## 1. Reddit · r/ClaudeAI (inglés)

**Título**

```
I built a free screen recorder with a built-in MCP server, so Claude can edit my recordings into Reels and long videos
```

**Texto**

```
I record a lot of tutorials and got tired of the editing part, so I added an MCP server to Dualrec, the recorder I've been building.

Dualrec records the screen, webcam, microphone and system audio as four separate, synced files. Because the sources are separate, Claude can decide where each one goes instead of working with an already-mixed video.

What Claude can do through the connector:

- List my recordings and see which tracks each one has
- Detect where I speak and where the silences are
- Pull still frames of the screen and the camera to see what is being shown
- Export a finished MP4: horizontal 1920x1080, vertical 1080x1920 or square, with the camera in a bubble, stacked or side by side, screen zoom, silences removed, a title and burned-in captions

So I can type "make a vertical Reel from the part where I show the dashboard, screen on top, camera below, cut the silences" and get the file.

Everything runs locally with FFmpeg. The video files are not uploaded; Claude only receives the recording details and the still frames it asks for.

Honest limitations: Windows only for now, the app UI is still in Spanish, and Claude can't hear the audio, so captions come from text you give it (transcription is on the roadmap).

It's free, no account, no watermark. It connects to Claude Desktop with one button, or to Claude Code with one command.

Site: https://dualrec.malkadigital.com/en/
Source: https://github.com/MalkSof/Dualrec

Happy to hear what tools you'd want it to expose.
```

---

## 2. Reddit · r/mcp (inglés)

**Título**

```
Dualrec: a local MCP server (Rust, stdio) that turns screen recordings into edited videos with FFmpeg
```

**Texto**

```
I added an MCP server to my screen recorder and wanted to share how it's built, in case it's useful to others doing media tools.

- Written by hand in Rust: newline-delimited JSON-RPC 2.0 over stdio, no SDK. It's the same binary as the desktop app, started with `dualrec.exe --mcp [folder]`.
- Five tools: list_recordings, get_recording (adds speech segments from FFmpeg's silencedetect), get_frames (returns up to 6 JPEGs as image content so the model can see the screen and the camera), render and render_status.
- render builds an FFmpeg filter graph: cuts, camera bubble with an alpha mask, stacked or side-by-side layouts, screen zoom, ASS subtitles for the title and captions, loudnorm. It waits up to 40 s and then returns a job id to poll, so long exports don't hit client timeouts.
- The recorder saves screen, camera, mic and system audio as separate synced files, which is what makes the editing tractable for a model.

Things I learned: returning frames as images was the difference between the model guessing and actually knowing what's on screen, and long-running tools need a job/poll pattern.

Windows only for now. Free and open source.

Repo: https://github.com/MalkSof/Dualrec
Site: https://dualrec.malkadigital.com/en/
```

---

## 3. Reddit · r/NewTubers y r/VideoEditing (inglés)

Sin mencionar MCP en el título: a este público le importa el resultado.

**Título**

```
Free Windows recorder that saves your screen and webcam as separate files (so you can move or remove the facecam in the edit)
```

**Texto**

```
Most screen recorders give you one video with the webcam already burned into a corner. If the bubble covers something, or you want a vertical version, you're stuck.

I made Dualrec to fix that for my own tutorials. One recording gives you four synced files: screen.mp4, camera.mp4, mic.m4a and system.m4a. Drop them at the start of the timeline and they line up.

What that lets you do:

- Move, resize or hide the facecam at any point in the edit
- Make a horizontal video and vertical Shorts from the same recording
- Fix or replace the mic audio without touching the game or system sound

Free, no account, no watermark, no time limit. Windows 10 and 11. It's simpler than OBS: no scenes to set up.

Not included: it doesn't make share links and the app UI is in Spanish for now.

https://dualrec.malkadigital.com/en/

Feedback welcome, especially on what's missing for your workflow.
```

---

## 4. Reddit · r/devsarg y r/Argentina (español)

**Título**

```
Hice un grabador de pantalla gratis que guarda la cámara por separado y se conecta con Claude para editar los videos
```

**Texto**

```
Buenas. Grabo bastantes tutoriales y me cansé de dos cosas: que la cámara quede pegada arriba de la pantalla y de perder horas editando. Así que armé Dualrec.

Qué hace:

- Graba pantalla, cámara, micrófono y audio de la compu en cuatro archivos separados y sincronizados. Después movés o sacás la cámara en la edición.
- Trae un conector MCP para Claude: le escribís "hacé un reel vertical con la parte donde muestro el panel y sacá los silencios" y exporta el MP4. Todo corre en tu máquina con FFmpeg, no se sube nada.

Está hecho en Rust con Tauri. Es gratis, sin cuenta, sin marca de agua y el código está en GitHub.

Lo que le falta: solo Windows por ahora, no genera link para compartir y Claude no escucha el audio (los subtítulos salen de un texto que le pasás).

Página: https://dualrec.malkadigital.com/
Código: https://github.com/MalkSof/Dualrec

Cualquier crítica viene bien, sobre todo de quien grabe seguido.
```

---

## 5. Hacker News · Show HN (inglés)

**Título** (máximo 80 caracteres)

```
Show HN: Dualrec – Screen recorder that saves each source as its own file
```

**URL:** `https://github.com/MalkSof/Dualrec`

**Primer comentario** (publicalo vos apenas sale el post)

```
Hi HN. I record tutorials and kept running into the same problem: the recorder bakes the webcam into the screen video, so I can't move it later or make a vertical cut.

Dualrec is a small Rust/Tauri app for Windows that records the screen, the webcam, the microphone and the system audio as four separate files that share one clock, so they line up at the start of the timeline. It drives FFmpeg as a sidecar.

It also includes an MCP server (hand-written JSON-RPC over stdio, same binary) so an assistant like Claude can list recordings, find the speech segments, look at frames and render a finished video with FFmpeg: layouts, cuts, zoom, captions.

Limitations: Windows only, UI in Spanish for now, no transcription, no share links.

I'd appreciate feedback on the sync approach and on the MCP tool design.
```

---

## 6. Product Hunt (inglés)

Conviene lanzarlo cuando la app esté en inglés. Sale mejor de martes a jueves, a las 00:01 hora del Pacífico (05:01 de Buenos Aires).

**Nombre:** `Dualrec`

**Tagline** (máximo 60 caracteres)

```
Record screen and webcam as separate files, edit with AI
```

**Descripción** (máximo 260 caracteres)

```
Free Windows recorder that saves screen, webcam, mic and system audio as four synced files. Move or remove the camera in the edit, or connect Claude and ask for a finished Reel or long video. No account, no watermark.
```

**Temas:** Video, Productivity, Artificial Intelligence, Open Source

**Primer comentario**

```
Hi Product Hunt! I'm Uriel from Malka Digital.

I built Dualrec because every recorder I tried glued my webcam onto the screen video. With separate files I can decide in the edit where the camera goes, and make vertical and horizontal versions from one recording.

The newest part is a connector for Claude: you describe the video you want and it exports the MP4 on your computer.

It's free and open source. I'd love to know what would make you switch from your current recorder.
```

**Galería:** el reel de Claude (`marketing/reel/dualrec-claude-reel.mp4`), una captura de la app y una de la carpeta con los cuatro archivos.

---

## 7. Directorios de servidores MCP

Dónde anotarlo (revisá en cada uno cómo se envía; la mayoría es un formulario o un pull request):

- La lista `awesome-mcp-servers` de GitHub (pull request agregando una línea)
- mcp.so
- Glama (glama.ai/mcp/servers)
- PulseMCP
- Smithery

**Línea para listas tipo "awesome"**

```
- [Dualrec](https://github.com/MalkSof/Dualrec) - Screen recorder for Windows that saves screen, webcam, mic and system audio as separate files, with a built-in MCP server to list recordings, detect speech, view frames and render edited videos (horizontal, vertical, square) locally with FFmpeg.
```

**Datos para los formularios**

| Campo | Valor |
|---|---|
| Nombre | Dualrec |
| Descripción corta | Turn screen recordings into edited videos: separate tracks plus FFmpeg rendering, all local. |
| Categoría | Multimedia / Video |
| Transporte | stdio |
| Plataforma | Windows 10 y 11 |
| Comando | `dualrec.exe --mcp <carpeta de grabaciones>` |
| Herramientas | list_recordings, get_recording, get_frames, render, render_status |
| Repositorio | https://github.com/MalkSof/Dualrec |
| Sitio | https://dualrec.malkadigital.com/en/ |
| Precio | Gratis |
