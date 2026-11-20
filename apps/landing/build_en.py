"""Genera en/index.html (inglés) a partir de index.html (español).

La página en español es la fuente: se edita index.html a mano y después se corre
    python build_en.py
Si un texto de index.html cambia y no tiene traducción acá, el script falla y
dice cuál falta, para que las dos versiones no se desfasen.
"""
import io
import re
import sys

SITE = "https://dualrec.malkadigital.com/"

# (español, inglés). Se reemplazan en este orden; cada uno tiene que aparecer.
PAIRS = [
    ('<html lang="es">', '<html lang="en">'),
    ("<title>Dualrec — Grabador de pantalla y cámara por separado, gratis para Windows</title>",
     "<title>Dualrec — Free screen and webcam recorder for Windows with separate tracks</title>"),
    ("Grabador de pantalla y cámara para Windows que guarda pantalla, cámara, micrófono y audio del sistema en archivos separados y sincronizados, listos para editar. Gratis, sin registro y sin marca de agua.",
     "Free screen and webcam recorder for Windows. Saves screen, camera, microphone and system audio as separate, synced files that are ready to edit. No sign-up, no watermark."),
    (f'<link rel="canonical" href="{SITE}" />', f'<link rel="canonical" href="{SITE}en/" />'),
    ('content="Dualrec — Grabar pantalla y cámara por separado"', 'content="Dualrec — Record your screen and webcam as separate files"'),
    ("Una grabación, cuatro archivos sincronizados: pantalla, cámara, micrófono y audio del sistema. Gratis para Windows.",
     "One recording, four synced files: screen, camera, microphone and system audio. Free for Windows."),
    ('<meta property="og:locale" content="es_AR" />\n    <meta property="og:locale:alternate" content="en_US" />',
     '<meta property="og:locale" content="en_US" />\n    <meta property="og:locale:alternate" content="es_AR" />'),
    (f'<meta property="og:url" content="{SITE}" />', f'<meta property="og:url" content="{SITE}en/" />'),
    ("assets/og-es.png", "assets/og-en.png"),
    ("Dualrec: pantalla, cámara, micrófono y audio del sistema en cuatro archivos separados", "Dualrec: screen, camera, microphone and system audio in four separate files"),
    ("Grabador de pantalla y cámara que guarda pantalla, cámara, micrófono y audio del sistema en archivos separados y sincronizados.",
     "Screen and webcam recorder that saves screen, camera, microphone and system audio as separate, synced files."),
    ("<!-- Datos estructurados: le dicen a Google qué es Dualrec y habilitan las preguntas frecuentes en los resultados. -->",
     "<!-- Structured data: tells search engines what Dualrec is and enables FAQ rich results. -->"),
    ('"inLanguage": "es"', '"inLanguage": "en"'),
    (f'"url": "{SITE}",', f'"url": "{SITE}en/",'),
    # Preguntas frecuentes (aparecen en los datos estructurados y en la página)
    ("¿Cómo grabo la pantalla y la cámara por separado?", "How do I record my screen and webcam separately?"),
    ("Abrí Dualrec, elegí la pantalla o ventana y la cámara, y apretá Grabar. Al cortar quedan dos videos distintos, screen.mp4 y camera.mp4, además del audio del micrófono y del sistema en archivos aparte.",
     "Open Dualrec, pick the screen or window and the camera, and press Record. When you stop you get two separate videos, screen.mp4 and camera.mp4, plus the microphone and system audio as their own files."),
    ("Abrí Dualrec, elegí la pantalla o ventana y la cámara, y apretá Grabar. Al cortar quedan dos videos distintos, <code>screen.mp4</code> y <code>camera.mp4</code>, además del audio del micrófono y del sistema en archivos aparte.",
     "Open Dualrec, pick the screen or window and the camera, and press Record. When you stop you get two separate videos, <code>screen.mp4</code> and <code>camera.mp4</code>, plus the microphone and system audio as their own files."),
    ("¿Dualrec es gratis?", "Is Dualrec free?"),
    ("Sí. Grabar en tu computadora es gratis, sin registro y sin marca de agua.", "Yes. Recording on your computer is free, with no sign-up and no watermark."),
    ("¿Las pistas quedan sincronizadas?", "Do the tracks stay in sync?"),
    ("Sí. Todas las pistas comparten el mismo reloj: arrancan en el mismo instante y duran lo mismo, así que en el editor se alinean poniéndolas al inicio de la línea de tiempo.",
     "Yes. All tracks share one clock: they start at the same instant and have the same length, so in your editor you just drop them at the start of the timeline and they line up."),
    ("¿Es una alternativa a Loom o a OBS?", "Is it an alternative to Loom or OBS?"),
    ("Sirve para lo mismo que usás un grabador de pantalla con cámara, pero con otro resultado: en vez de un solo video con la cámara pegada, te deja cada fuente en su archivo. Es más simple que OBS (no hay escenas que configurar) y, a diferencia de Loom, graba en tu computadora sin cuenta.",
     "It covers the same job as a screen recorder with a webcam overlay, with a different result: instead of one video with the camera burned in, you get each source as its own file. It is simpler than OBS (no scenes to set up) and, unlike Loom, it records on your computer with no account."),
    ("¿En qué se diferencia de otros grabadores de pantalla?", "How is it different from other screen recorders?"),
    ("La mayoría entrega un solo video con la cámara ya pegada encima de la pantalla. Dualrec entrega cada fuente en su archivo, para que decidas en la edición dónde va la cámara, cuándo se ve y cómo suena cada audio.",
     "Most of them give you a single video with the camera already burned on top of the screen. Dualrec gives you each source as its own file, so you decide in the edit where the camera goes, when it shows and how each audio sounds."),
    ("¿Funciona en Mac?", "Does it work on Mac?"),
    ("Por ahora Dualrec funciona en Windows 10 y 11. La versión para macOS está planeada.", "For now Dualrec runs on Windows 10 and 11. A macOS version is planned."),
    ("¿Necesito internet para grabar?", "Do I need internet to record?"),
    ("No. Dualrec graba sin conexión y los archivos quedan en tu computadora.", "No. Dualrec records offline and the files stay on your computer."),
    ("¿Con qué programas puedo editar las grabaciones?", "Which editors can open the recordings?"),
    ("Con cualquier editor de video. Los videos son MP4 con H.264 y los audios M4A con AAC, formatos que abren DaVinci Resolve, Premiere, CapCut y el resto de los editores habituales.",
     "Any video editor. Videos are MP4 with H.264 and audio is M4A with AAC, formats that DaVinci Resolve, Premiere, CapCut and every common editor can open."),
    # Rutas: la versión en inglés vive en /en/
    ('href="assets/', 'href="../assets/'),
    ('src="assets/', 'src="../assets/'),
    ('<a class="link lang" href="en/" hreflang="en" lang="en">English</a>', '<a class="link lang" href="../" hreflang="es" lang="es">Español</a>'),
    # Menú
    ('<a class="link" href="#pistas">Pistas</a>', '<a class="link" href="#pistas">Tracks</a>'),
    ('<a class="link" href="#funciones">Funciones</a>', '<a class="link" href="#funciones">Features</a>'),
    ('<a class="link" href="#proximamente">Lo que viene</a>', '<a class="link" href="#proximamente">Roadmap</a>'),
    ('<a class="link" href="#preguntas">Preguntas</a>', '<a class="link" href="#preguntas">FAQ</a>'),
    # Portada
    ("Gratis para Windows 10 y 11", "Free for Windows 10 and 11"),
    ("<h1>Grabá tu pantalla y tu cámara <em>por separado</em>.</h1>", "<h1>Record your screen and webcam <em>as separate files</em>.</h1>"),
    ("Dualrec graba pantalla, cámara, micrófono y audio del sistema en cuatro archivos", "Dualrec records screen, camera, microphone and system audio as four synced"),
    ("sincronizados. Editás cada uno como quieras, sin la cámara pegada encima del video.", "files. Edit each one however you like, with no camera burned on top of the video."),
    ("Descargar para Windows", "Download for Windows"),
    ("Ver el código", "View the code"),
    ("Instalador de 30 MB · sin cuenta ni registro · las grabaciones quedan en tu computadora", "30 MB installer · no account or sign-up · recordings stay on your computer · the app interface is in Spanish for now"),
    ("Dualrec después de grabar: lista los archivos screen.mp4, camera.mp4, mic.m4a y system.m4a de la grabación", "Dualrec after a recording: it lists the files screen.mp4, camera.mp4, mic.m4a and system.m4a"),
    # Pistas
    ("Una grabación, cuatro archivos", "One recording, four files"),
    ("Cada fuente se guarda en su propio archivo, en una carpeta por grabación. Todos arrancan en", "Each source is saved to its own file, in one folder per recording. They all start at"),
    ("el mismo instante y duran lo mismo: en el editor los ponés al inicio de la línea de tiempo", "the same instant and have the same length: drop them at the start of your timeline"),
    ("y ya están alineados.", "and they are already aligned."),
    ("Una pantalla entera o una sola ventana, hasta 60 cuadros por segundo.", "A full screen or a single window, at up to 60 frames per second."),
    ("Tu cámara a resolución completa, para ubicarla y recortarla donde quieras.", "Your camera at full resolution, to place and crop wherever you want."),
    ("Tu voz sola, para limpiarla o subirle el volumen sin tocar el resto.", "Your voice on its own, to clean it up or boost it without touching the rest."),
    ("Lo que suena en la computadora, separado del micrófono.", "Whatever plays on the computer, separate from the microphone."),
    ("Así quedan en tu editor: mismas marcas de inicio y de fin.", "This is how they land in your editor: same start and end marks."),
    # Para qué sirve
    ("Para qué sirve", "What people use it for"),
    ("<h3>Tutoriales y cursos</h3>", "<h3>Tutorials and courses</h3>"),
    ("Mostrás la pantalla completa cuando explicás un paso y tu cara cuando hablás. Lo decidís al editar, no al grabar.", "Show the full screen when you explain a step and your face when you talk. You decide in the edit, not while recording."),
    ("<h3>Demos de producto</h3>", "<h3>Product demos</h3>"),
    ("Grabás una vez y armás la versión horizontal para YouTube y la vertical para reels con la cámara en otro lugar.", "Record once and cut a horizontal version for YouTube and a vertical one for shorts, with the camera somewhere else."),
    ("<h3>Clases y presentaciones</h3>", "<h3>Classes and presentations</h3>"),
    ("Si la cámara tapa algo de la diapositiva, la movés o la sacás en ese tramo sin volver a grabar.", "If the camera covers part of a slide, move it or remove it for that part without recording again."),
    ("<h3>Videos con buen audio</h3>", "<h3>Videos with clean audio</h3>"),
    ("Tu voz y el sonido de la computadora van en archivos distintos: le bajás el volumen a uno sin arruinar el otro.", "Your voice and the computer sound are separate files: lower one without ruining the other."),
    # Funciones
    ("Hecho para grabar sin pensar en la grabación", "Built so you can record without thinking about recording"),
    ("Burbuja de cámara flotante", "Floating camera bubble"),
    ("Te ves mientras grabás, la movés a donde no moleste y cambia de tamaño con doble clic.", "See yourself while you record, move it out of the way and resize it with a double click."),
    ("La app no sale en el video", "The app stays out of the video"),
    ("Dualrec y la burbuja quedan fuera de la grabación de pantalla. Se puede desactivar.", "Dualrec and the bubble are left out of the screen recording. You can turn this off."),
    ("Atajos desde cualquier programa", "Shortcuts from any app"),
    ("graba o corta y <kbd>Ctrl</kbd>", "starts or stops and <kbd>Ctrl</kbd>"),
    ("pausa, sin volver a la ventana de Dualrec.", "pauses, without going back to the Dualrec window."),
    ("Pausa que no desincroniza", "Pause that keeps everything in sync"),
    ("Al pausar, las cuatro pistas saltan el mismo tramo. Siguen alineadas al reanudar.", "When you pause, all four tracks skip the same stretch. They are still aligned when you resume."),
    ("Usa tu placa de video", "Uses your graphics card"),
    ("Codifica en H.264 por hardware con NVIDIA, Intel o AMD, y por procesador si no hay.", "Encodes H.264 in hardware with NVIDIA, Intel or AMD, and on the CPU when there is none."),
    ("Si se cierra, no perdés lo grabado", "If it closes, you keep what you recorded"),
    ("Cada pista se escribe en disco desde el primer segundo y se puede reproducir aunque la app se cierre a la mitad.", "Each track is written to disk from the first second and still plays even if the app closes halfway."),
    # Cómo se usa
    ("<h2>Cómo se usa</h2>", "<h2>How it works</h2>"),
    ("<b>Elegí qué grabar</b><span>Pantalla o ventana, cámara, micrófono y audio del sistema. Cada uno se prende o se apaga por separado.</span>",
     "<b>Pick what to record</b><span>Screen or window, camera, microphone and system audio. Each one turns on or off on its own.</span>"),
    ("<b>Apretá Grabar</b><span>O usá el atajo. Podés pausar las veces que quieras.</span>", "<b>Press Record</b><span>Or use the shortcut. Pause as many times as you want.</span>"),
    ("<b>Cortá y abrí la carpeta</b><span>Los archivos quedan en <code>Videos\\Dualrec</code>, una carpeta por grabación, listos para tu editor.</span>",
     "<b>Stop and open the folder</b><span>The files are in <code>Videos\\Dualrec</code>, one folder per recording, ready for your editor.</span>"),
    ("Dualrec grabando: indicador REC con el tiempo y botones de pausar y cortar", "Dualrec while recording: REC indicator with the elapsed time and pause and stop buttons"),
    # Lo que viene
    ("<h2>Lo que viene</h2>", "<h2>Roadmap</h2>"),
    ("Dualrec empieza como grabador. El plan es que cada video se pueda compartir y consultar.", "Dualrec starts as a recorder. The plan is for every video to be shareable and searchable."),
    ("<small>En desarrollo</small>", "<small>In progress</small>"),
    ("<small>Planeado</small>", "<small>Planned</small>"),
    ("Link para compartir", "Share link"),
    ("La grabación se sube mientras grabás y al cortar tenés un link con reproductor web, con la cámara en una burbuja que se puede ocultar.",
     "The recording uploads while you record, and when you stop you get a link with a web player, with the camera in a bubble you can hide."),
    ("Transcripción y resumen", "Transcript and summary"),
    ("Cada video con su texto, un resumen y capítulos generados automáticamente.", "Every video with its text, a summary and automatically generated chapters."),
    ("Preguntale a tus videos", "Ask your videos"),
    ("Buscá en lenguaje natural y saltá al minuto exacto donde lo explicaste.", "Search in plain language and jump to the exact minute where you explained it."),
    ("<h2>Preguntas frecuentes</h2>", "<h2>Frequently asked questions</h2>"),
    # Descarga y pie
    ("<h2>Probalo ahora</h2>", "<h2>Try it now</h2>"),
    ("Gratis, sin registro. Para Windows 10 y 11.", "Free, no sign-up. For Windows 10 and 11."),
    ("Windows puede avisar que el instalador no está firmado: elegí “Más información” y después “Ejecutar de todas formas”.",
     "Windows may warn that the installer is not signed: choose “More info” and then “Run anyway”."),
    ("<h2>¿Necesitás software a medida?</h2>", "<h2>Need custom software?</h2>"),
    ("Dualrec lo hace <b>Malka Digital</b>, un estudio de software. Diseñamos y desarrollamos sistemas, aplicaciones y sitios web a medida que automatizan procesos y le ahorran tiempo a tu negocio.",
     "Dualrec is made by <b>Malka Digital</b>, a software studio. We design and build custom systems, apps and websites that automate processes and save your business time."),
    ("Conocé Malka Digital", "Visit Malka Digital"),
    ("Dualrec · hecho por <a", "Dualrec · made by <a"),
    ("Código en GitHub", "Code on GitHub"),
    (">Versiones<", ">Releases<"),
]

# Letras que solo deberían quedar si algo no se tradujo.
LEFTOVER = re.compile(r"[áéíóúñ¿¡]", re.I)


def main() -> int:
    src = io.open("index.html", encoding="utf-8").read()
    # Lo que está entre estas marcas es solo para la versión en español (el video está en español).
    out = re.sub(r"\s*<!-- solo-es -->.*?<!-- /solo-es -->", "", src, flags=re.S)
    missing = []
    for es, en in PAIRS:
        if es not in out:
            missing.append(es)
        out = out.replace(es, en)
    left = sorted({line.strip() for line in out.splitlines() if LEFTOVER.search(line) and 'lang="es"' not in line})
    if missing or left:
        for m in missing:
            print("No aparece en index.html:", m[:90])
        for line in left:
            print("Quedó sin traducir:", line[:110])
        return 1
    io.open("en/index.html", "w", encoding="utf-8", newline="\n").write(out)
    print("en/index.html generado")
    return 0


if __name__ == "__main__":
    sys.exit(main())
