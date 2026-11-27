# Directorios de MCP para Dualrec

El repo ya tiene descripción, la página como sitio y los topics `mcp`, `mcp-server`, `model-context-protocol`, `claude` y otros. Sin esos topics, Glama no lo encuentra.

Seguí este orden, porque awesome-mcp-servers exige estar primero en Glama.

## 1. Glama (necesita tu login de GitHub)

1. Entrá a https://glama.ai/mcp/servers y tocá **Add Server**.
2. Pegá `https://github.com/MalkSof/Dualrec`.
3. Cuando aparezca la ficha, copiá el link (algo como `https://glama.ai/mcp/servers/MalkSof/Dualrec`) y pasámelo. Con eso agrego el badge al README y abro el PR de awesome-mcp-servers.

## 2. awesome-mcp-servers (lo hago yo con `gh`, cuando exista la ficha de Glama)

Va en la categoría **Multimedia Process**, en orden alfabético:

```
- [MalkSof/Dualrec](https://github.com/MalkSof/Dualrec) [![MalkSof/Dualrec MCP server](https://glama.ai/mcp/servers/MalkSof/Dualrec/badges/score.svg)](https://glama.ai/mcp/servers/MalkSof/Dualrec) 🦀 🏠 🪟 - Screen and camera recorder that saves screen, webcam, mic and system audio as separate synced tracks. Claude can list recordings, look at frames and render edited MP4s with FFmpeg: vertical reels, camera bubble, cut silences, captions, or a waveform video from an audio-only recording.
```

## 3. mcp.so (necesita tu login)

Entrá a https://mcp.so/submit y cargá estos datos:

- **Name:** Dualrec
- **URL:** https://github.com/MalkSof/Dualrec
- **Description:** la misma de arriba.

## 4. Smithery y PulseMCP

Por ahora no conviene.

- **Smithery:** el proxy cerró el 31/10/2026 y quedó solo como directorio.
- **PulseMCP:** no está aceptando servidores nuevos.

## 5. Registro oficial de MCP (más adelante)

Necesita un paquete npm, PyPI o MCPB. Dualrec se distribuye como instalador `.exe`, así que haría falta empaquetarlo como MCPB. Lo dejamos para después de validar el nicho.
