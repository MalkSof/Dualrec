"""Genera el blog de la landing (español e inglés) a partir de las notas en posts/.

Uso (desde apps/landing):

    python build_blog.py

Genera:
    blog/index.html                 índice en español
    blog/<slug>/index.html          cada nota en español
    blog/feed.xml                   RSS en español
    en/blog/index.html              índice en inglés
    en/blog/<slug>/index.html       cada nota en inglés
    en/blog/feed.xml                RSS en inglés
    sitemap.xml                     conserva las páginas que no son del blog y
                                    reescribe las del blog con sus alternates

No tiene dependencias: alcanza con Python 3.8 o superior. Lo generado se sube al
repo tal cual (GitHub Pages publica la carpeta sin correr nada).

CÓMO AGREGAR UNA NOTA
---------------------
1. Elegí una clave corta para la nota (solo sirve para emparejar los dos
   idiomas y para enlazarla desde otras notas), por ejemplo "editar-con-ia".
2. Creá DOS archivos, uno por idioma, los dos obligatorios:
       posts/editar-con-ia.es.html
       posts/editar-con-ia.en.html
3. Cada archivo empieza con un comentario con los metadatos y sigue con el
   cuerpo en HTML:

       <!--
       slug: como-editar-con-ia
       title: Título de la nota (va en el <h1> y en el <title>)
       description: Resumen de una o dos frases (meta description, tarjeta del índice y RSS).
       date: 2026-12-01
       -->
       <p>Primer párrafo.</p>
       <h2>Un subtítulo</h2>
       <p>Texto, <a href="{{post:grabar-separado}}">enlace a otra nota</a>.</p>
       <ol>
         <li>Paso uno.</li>
       </ol>

       <!-- faq -->
       <h3>¿Una pregunta?</h3>
       <p>Su respuesta.</p>
       <h3>¿Otra pregunta?</h3>
       <p>Otra respuesta.</p>

   - slug: la última parte de la URL (minúsculas, números y guiones). Cada
     idioma tiene el suyo.
   - date: fecha de publicación, AAAA-MM-DD. Opcional: "updated: AAAA-MM-DD"
     si después corregís la nota.
   - El cuerpo no lleva <h1> (lo pone la plantilla): usá <h2>, <h3>, <p>,
     <ul>, <ol>, <code>, <kbd>, <strong>.
   - Todo lo que va después de la línea "<!-- faq -->" son las preguntas
     frecuentes: un <h3> por pregunta y debajo su respuesta. De ahí salen el
     texto visible y el JSON-LD FAQPage, así que siempre coinciden. La sección
     es opcional.
   - Comodines disponibles en el cuerpo:
       {{post:clave}}   URL de otra nota (en el mismo idioma)
       {{home}}         la home en ese idioma
       {{blog}}         el índice del blog en ese idioma
       {{download}}     el instalador de Dualrec
       {{repo}}         el repositorio en GitHub
4. Corré "python build_blog.py". Si falta un idioma, un metadato o un comodín
   apunta a una nota que no existe, el script falla y dice qué pasa. También
   muestra la cantidad de palabras de cada nota.
5. Subí lo generado junto con los fuentes (posts/, blog/, en/blog/ y
   sitemap.xml).

Los botones de descarga (arriba y abajo), la fecha, el cambio de idioma, las
"otras notas", los datos estructurados, el RSS y el sitemap los arma la
plantilla: no hay que escribirlos en cada nota. Si cambiás un slug, el script
borra la carpeta vieja que él mismo había generado.

Los estilos están en assets/site.css (sección "Blog").
"""
import datetime
import glob
import html
import io
import json
import os
import re
import sys

SITE = "https://dualrec.malkadigital.com/"
DOWNLOAD = "https://github.com/MalkSof/Dualrec/releases/latest/download/Dualrec-setup.exe"
REPO = "https://github.com/MalkSof/Dualrec"
MALKA = "https://malkadigital.com/"
ORG = {"@type": "Organization", "name": "Malka Digital", "url": MALKA}
MARK = "<!-- Generado por build_blog.py a partir de posts/: no editar a mano. -->"
LANGS = ("es", "en")
OTHER = {"es": "en", "en": "es"}
WORDS_MIN, WORDS_MAX = 700, 1100

MONTHS = {
    "es": ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto",
           "septiembre", "octubre", "noviembre", "diciembre"],
    "en": ["January", "February", "March", "April", "May", "June", "July", "August",
           "September", "October", "November", "December"],
}
DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
MON3 = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

# Textos fijos de la plantilla, por idioma.
T = {
    "es": {
        "home": "",
        "blog": "blog/",
        "locale": "es_AR",
        "rss_lang": "es-AR",
        "og_image": "assets/og-es.png",
        "og_alt": "Dualrec: pantalla, cámara, micrófono y audio del sistema en cuatro archivos separados",
        "nav_home": "Inicio",
        "lang_name": "English",
        "lang_read": "Read this in English",
        "blog_title": "Blog de Dualrec: guías para grabar pantalla y cámara",
        "blog_h1": "Blog de Dualrec",
        "blog_desc": "Guías y consejos para grabar la pantalla y la cámara en Windows, editar con pistas separadas y sacar videos horizontales y verticales de una misma grabación.",
        "blog_sub": "Guías para grabar la pantalla y la cámara en Windows, editar con pistas separadas y aprovechar mejor cada grabación.",
        "feed_title": "Blog de Dualrec",
        "read": "Leer la nota",
        "min_read": "{n} min de lectura",
        "updated": "Actualizada el {d}",
        "faq": "Preguntas frecuentes",
        "cta_text": "<strong>Dualrec</strong> es un grabador gratuito para Windows 10 y 11: guarda pantalla, cámara, micrófono y audio del sistema en cuatro archivos sincronizados.",
        "download": "Descargar para Windows",
        "cta_fine": "Instalador de 30 MB · sin cuenta ni registro · sin marca de agua",
        "end_h2": "Probá Dualrec gratis",
        "end_sub": "Una grabación, cuatro archivos sincronizados. Sin registro, sin marca de agua y sin límite de tiempo. Para Windows 10 y 11.",
        "end_fine": "Windows puede avisar que el instalador no está firmado: elegí “Más información” y después “Ejecutar de todas formas”.",
        "see_home": "Conocer Dualrec",
        "more": "Más notas del blog",
        "all_posts": "Ver todas las notas",
        "maker_eyebrow": "El estudio que hace Dualrec",
        "maker_h2": "¿Tu negocio necesita su propio sistema?",
        "maker_lead": "Diseñamos y desarrollamos software a medida que automatiza tus procesos, te ahorra tiempo y te ayuda a crecer.",
        "maker_cta": "Contanos tu proyecto",
        "maker_work": "Ver proyectos",
        "foot_by": "Dualrec · hecho por",
        "foot_code": "Código en GitHub",
        "foot_releases": "Versiones",
    },
    "en": {
        "home": "en/",
        "blog": "en/blog/",
        "locale": "en_US",
        "rss_lang": "en",
        "og_image": "assets/og-en.png",
        "og_alt": "Dualrec: screen, camera, microphone and system audio in four separate files",
        "nav_home": "Home",
        "lang_name": "Español",
        "lang_read": "Leer en español",
        "blog_title": "Dualrec blog: guides for recording your screen and webcam",
        "blog_h1": "Dualrec blog",
        "blog_desc": "Guides and tips for recording your screen and webcam on Windows, editing with separate tracks and getting horizontal and vertical videos out of one recording.",
        "blog_sub": "Guides for recording your screen and webcam on Windows, editing with separate tracks and getting more out of every recording.",
        "feed_title": "Dualrec blog",
        "read": "Read the post",
        "min_read": "{n} min read",
        "updated": "Updated {d}",
        "faq": "Frequently asked questions",
        "cta_text": "<strong>Dualrec</strong> is a free recorder for Windows 10 and 11. It saves your screen, camera, microphone and system audio as four synced files.",
        "download": "Download for Windows",
        "cta_fine": "30 MB installer · no account or sign-up · no watermark · app in English and Spanish",
        "end_h2": "Try Dualrec for free",
        "end_sub": "One recording, four synced files. No sign-up, no watermark and no time limit. For Windows 10 and 11.",
        "end_fine": "Windows may warn that the installer is not signed: choose “More info” and then “Run anyway”.",
        "see_home": "Learn more about Dualrec",
        "more": "More from the blog",
        "all_posts": "See all posts",
        "maker_eyebrow": "The studio behind Dualrec",
        "maker_h2": "Does your business need its own software?",
        "maker_lead": "We design and build custom software that automates your processes, saves you time and helps you grow.",
        "maker_cta": "Tell us about your project",
        "maker_work": "See our work",
        "foot_by": "Dualrec · made by",
        "foot_code": "Code on GitHub",
        "foot_releases": "Releases",
    },
}

ICON_DOWNLOAD = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 20h14" /></svg>'
ICON_ARROW = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14m0 0-5.5-5.5M19 12l-5.5 5.5" /></svg>'


class BuildError(Exception):
    pass


def esc(text):
    return html.escape(text, quote=True)


def plain(fragment):
    """Texto sin etiquetas ni entidades, en una sola línea."""
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", fragment))).strip() \
        .replace(" ,", ",").replace(" .", ".").replace(" :", ":").replace(" ;", ";") \
        .replace("( ", "(").replace(" )", ")")


def rel(here, target):
    """Ruta relativa desde la carpeta `here` (p. ej. "blog/mi-nota/") hasta `target`, ambas desde la raíz."""
    return "../" * here.count("/") + target or "./"


def long_date(date, lang):
    month = MONTHS[lang][date.month - 1]
    if lang == "es":
        return f"{date.day} de {month} de {date.year}"
    return f"{month} {date.day}, {date.year}"


def rfc822(date):
    return f"{DAYS[date.weekday()]}, {date.day:02d} {MON3[date.month - 1]} {date.year} 12:00:00 +0000"


def write(path, text):
    folder = os.path.dirname(path)
    if folder:
        os.makedirs(folder, exist_ok=True)
    with io.open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


# ---------- Lectura de las notas ----------

def read_post(path, key, lang):
    text = io.open(path, encoding="utf-8").read().replace("\r\n", "\n")
    m = re.match(r"\s*<!--(.*?)-->(.*)\Z", text, re.S)
    if not m:
        raise BuildError(f"{path}: tiene que empezar con un comentario <!-- ... --> con los metadatos")
    meta = {}
    for line in m.group(1).strip().splitlines():
        if not line.strip():
            continue
        if ":" not in line:
            raise BuildError(f"{path}: línea de metadatos sin 'clave: valor': {line.strip()}")
        k, v = line.split(":", 1)
        meta[k.strip().lower()] = v.strip()
    for required in ("slug", "title", "description", "date"):
        if not meta.get(required):
            raise BuildError(f"{path}: falta el metadato '{required}'")
    if not re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*", meta["slug"]):
        raise BuildError(f"{path}: el slug solo puede tener minúsculas, números y guiones: {meta['slug']}")
    try:
        date = datetime.date.fromisoformat(meta["date"])
        updated = datetime.date.fromisoformat(meta["updated"]) if meta.get("updated") else date
    except ValueError as e:
        raise BuildError(f"{path}: fecha inválida ({e}); usá AAAA-MM-DD")

    body, _, faq_src = m.group(2).partition("<!-- faq -->")
    if re.search(r"<h1[\s>]", body):
        raise BuildError(f"{path}: el cuerpo no lleva <h1>; el título sale del metadato 'title'")
    faq = []
    if faq_src.strip():
        parts = re.split(r"<h3[^>]*>(.*?)</h3>", faq_src, flags=re.S)
        if parts[0].strip() or len(parts) < 3:
            raise BuildError(f"{path}: después de <!-- faq --> va un <h3> por pregunta y debajo su respuesta")
        for question, answer in zip(parts[1::2], parts[2::2]):
            if not answer.strip():
                raise BuildError(f"{path}: la pregunta '{plain(question)}' no tiene respuesta")
            faq.append((question.strip(), answer.strip()))
    words = len(plain(body + " " + faq_src).split())
    return {
        "key": key, "lang": lang, "path": T[lang]["blog"] + meta["slug"] + "/",
        "slug": meta["slug"], "title": meta["title"], "description": meta["description"],
        "date": date, "updated": updated, "body": body.strip(), "faq": faq, "words": words,
    }


def load_posts():
    posts = {}
    for path in sorted(glob.glob("posts/*.html")):
        m = re.fullmatch(r"(.+)\.(es|en)\.html", os.path.basename(path))
        if not m:
            raise BuildError(f"{path}: el nombre tiene que ser <clave>.es.html o <clave>.en.html")
        posts.setdefault(m.group(1), {})[m.group(2)] = read_post(path, m.group(1), m.group(2))
    if not posts:
        raise BuildError("No hay notas en posts/")
    for key, by_lang in posts.items():
        for lang in LANGS:
            if lang not in by_lang:
                raise BuildError(f"A la nota '{key}' le falta el archivo posts/{key}.{lang}.html")
        if by_lang["es"]["date"] != by_lang["en"]["date"]:
            raise BuildError(f"La nota '{key}' tiene fechas distintas en cada idioma")
    for lang in LANGS:
        slugs = [p[lang]["slug"] for p in posts.values()]
        repeated = {s for s in slugs if slugs.count(s) > 1}
        if repeated:
            raise BuildError(f"Slug repetido en '{lang}': {', '.join(sorted(repeated))}")
    # Las más nuevas primero; a igual fecha, por clave.
    return sorted(posts.values(), key=lambda p: (-p["es"]["date"].toordinal(), p["es"]["key"]))


def expand(post, posts):
    """Reemplaza los comodines {{...}} del cuerpo y de las preguntas frecuentes."""
    lang, here = post["lang"], post["path"]
    by_key = {p[lang]["key"]: p[lang] for p in posts}

    def one(m):
        name, arg = m.group(1), m.group(2)
        if name == "post":
            if arg not in by_key:
                raise BuildError(f"posts/{post['key']}.{lang}.html: {{{{post:{arg}}}}} apunta a una nota que no existe")
            return rel(here, by_key[arg]["path"])
        if name == "home" and not arg:
            return rel(here, T[lang]["home"])
        if name == "blog" and not arg:
            return rel(here, T[lang]["blog"])
        if name == "download" and not arg:
            return DOWNLOAD
        if name == "repo" and not arg:
            return REPO
        raise BuildError(f"posts/{post['key']}.{lang}.html: comodín desconocido {m.group(0)}")

    def sub(text):
        return re.sub(r"\{\{\s*(\w+)(?::([\w-]+))?\s*\}\}", one, text)

    post["body"] = sub(post["body"])
    post["faq"] = [(sub(q), sub(a)) for q, a in post["faq"]]


# ---------- Plantillas ----------

def jsonld(data):
    text = json.dumps(data, ensure_ascii=False, indent=2).replace("</", "<\\/")
    return '    <script type="application/ld+json">\n' + text + "\n    </script>\n"


def head(lang, here, title, description, alternates, og_type, schemas, extra=""):
    t = T[lang]
    url = SITE + here
    out = f"""<!doctype html>
<html lang="{lang}">
  <head>
    {MARK}
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{esc(title)}</title>
    <meta name="description" content="{esc(description)}" />
    <meta name="theme-color" content="#0b0d12" />
    <link rel="canonical" href="{url}" />
    <link rel="alternate" hreflang="es" href="{SITE}{alternates['es']}" />
    <link rel="alternate" hreflang="en" href="{SITE}{alternates['en']}" />
    <link rel="alternate" hreflang="x-default" href="{SITE}{alternates['en']}" />
    <link rel="alternate" type="application/rss+xml" title="{esc(t['feed_title'])}" href="{SITE}{t['blog']}feed.xml" />
    <meta property="og:title" content="{esc(title)}" />
    <meta property="og:description" content="{esc(description)}" />
    <meta property="og:type" content="{og_type}" />
    <meta property="og:site_name" content="Dualrec" />
    <meta property="og:locale" content="{t['locale']}" />
    <meta property="og:locale:alternate" content="{T[OTHER[lang]]['locale']}" />
    <meta property="og:url" content="{url}" />
    <meta property="og:image" content="{SITE}{t['og_image']}" />
    <meta property="og:image:width" content="1200" />
    <meta property="og:image:height" content="630" />
    <meta property="og:image:alt" content="{esc(t['og_alt'])}" />
{extra}    <meta name="twitter:card" content="summary_large_image" />
    <meta name="twitter:title" content="{esc(title)}" />
    <meta name="twitter:description" content="{esc(description)}" />
    <meta name="twitter:image" content="{SITE}{t['og_image']}" />
"""
    for schema in schemas:
        out += jsonld(schema)
    out += f"""    <link rel="icon" type="image/svg+xml" href="{rel(here, 'assets/logo.svg')}" />
    <link rel="stylesheet" href="{rel(here, 'assets/site.css')}" />
    <!-- Contador de visitas sin cookies (GoatCounter) -->
    <script data-goatcounter="https://duelrec.goatcounter.com/count" async src="https://gc.zgo.at/count.js"></script>
  </head>
"""
    return out


def nav(lang, here, alt_path):
    t, o = T[lang], OTHER[lang]
    return f"""      <nav>
        <a class="brand" href="{rel(here, t['home'])}" aria-label="Dualrec">
          <img src="{rel(here, 'assets/logo.svg')}" alt="" width="30" height="30" />
          Dualrec
        </a>
        <a class="link" href="{rel(here, t['home'])}">{t['nav_home']}</a>
        <a class="link keep" href="{rel(here, t['blog'])}">Blog</a>
        <a class="link" href="{REPO}">GitHub</a>
        <a class="link lang keep" href="{rel(here, alt_path)}" hreflang="{o}" lang="{o}">{t['lang_name']}</a>
      </nav>
"""


def download_button(lang):
    return f"""<a class="btn primary" href="{DOWNLOAD}">
              {ICON_DOWNLOAD}
              {T[lang]['download']}
            </a>"""


def card(lang, here, post, heading):
    t = T[lang]
    return f"""          <a class="post-card" href="{rel(here, post['path'])}">
            <time datetime="{post['date'].isoformat()}">{long_date(post['date'], lang)}</time>
            <{heading}>{esc(post['title'])}</{heading}>
            <p>{esc(post['description'])}</p>
            <span class="go">{t['read']} →</span>
          </a>
"""


def tail(lang, here):
    """Descarga final, bloque de Malka Digital y pie: iguales en todas las páginas del blog."""
    t = T[lang]
    return f"""      <section class="download">
        <h2>{t['end_h2']}</h2>
        <p class="sub">{t['end_sub']}</p>
        <div class="cta">
          {download_button(lang)}
          <a class="btn ghost" href="{rel(here, t['home'])}">{t['see_home']}</a>
        </div>
        <p class="fine">{t['end_fine']}</p>
      </section>

      <section class="maker" id="malka">
        <div class="maker-card">
          <a class="maker-brand" href="{MALKA}" aria-label="Malka Digital">
            <img src="{rel(here, 'assets/malka.png')}" width="44" height="44" alt="" loading="lazy" />
            <span><b>Malka Digital</b><small>Software studio</small></span>
          </a>
          <p class="maker-eyebrow">{t['maker_eyebrow']}</p>
          <h2>{t['maker_h2']}</h2>
          <p class="maker-lead">{t['maker_lead']}</p>
          <div class="maker-cta">
            <a class="btn malka" href="{MALKA}#contacto">{t['maker_cta']} {ICON_ARROW}</a>
            <a class="btn ghost" href="{MALKA}#proyectos">{t['maker_work']}</a>
          </div>
        </div>
      </section>

      <footer>
        <span>{t['foot_by']} <a href="{MALKA}">Malka Digital</a></span>
        <a href="{rel(here, t['home'])}">{t['nav_home']}</a>
        <a href="{rel(here, t['blog'])}">Blog</a>
        <a href="{REPO}">{t['foot_code']}</a>
        <a href="{REPO}/releases">{t['foot_releases']}</a>
      </footer>
    </div>
  </body>
</html>
"""


def breadcrumb(lang, items):
    return {
        "@context": "https://schema.org",
        "@type": "BreadcrumbList",
        "itemListElement": [
            {"@type": "ListItem", "position": i, "name": name, "item": SITE + path}
            for i, (name, path) in enumerate(items, 1)
        ],
    }


def render_post(post, alt, others):
    lang, here = post["lang"], post["path"]
    t = T[lang]
    url = SITE + here
    schemas = [
        {
            "@context": "https://schema.org",
            "@type": "BlogPosting",
            "headline": post["title"],
            "description": post["description"],
            "datePublished": post["date"].isoformat(),
            "dateModified": post["updated"].isoformat(),
            "inLanguage": lang,
            "url": url,
            "mainEntityOfPage": {"@type": "WebPage", "@id": url},
            "image": SITE + t["og_image"],
            "wordCount": post["words"],
            "author": ORG,
            "publisher": ORG,
        },
        breadcrumb(lang, [("Dualrec", t["home"]), ("Blog", t["blog"]), (post["title"], here)]),
    ]
    faq_html = ""
    if post["faq"]:
        schemas.append({
            "@context": "https://schema.org",
            "@type": "FAQPage",
            "mainEntity": [
                {"@type": "Question", "name": plain(q), "acceptedAnswer": {"@type": "Answer", "text": plain(a)}}
                for q, a in post["faq"]
            ],
        })
        faq_html = f'\n<h2 id="faq">{t["faq"]}</h2>\n' + "\n".join(f"<h3>{q}</h3>\n{a}" for q, a in post["faq"])

    extra = f'    <meta property="article:published_time" content="{post["date"].isoformat()}" />\n'
    if post["updated"] != post["date"]:
        extra += f'    <meta property="article:modified_time" content="{post["updated"].isoformat()}" />\n'
    minutes = max(1, round(post["words"] / 200))
    meta_line = f'<time datetime="{post["date"].isoformat()}">{long_date(post["date"], lang)}</time> · {t["min_read"].format(n=minutes)}'
    if post["updated"] != post["date"]:
        meta_line += " · " + t["updated"].format(d=long_date(post["updated"], lang))
    o = OTHER[lang]
    meta_line += f' · <a href="{rel(here, alt["path"])}" hreflang="{o}" lang="{o}">{t["lang_read"]}</a>'

    out = head(lang, here, f"{post['title']} — Dualrec", post["description"],
               {lang: here, o: alt["path"]}, "article", schemas, extra)
    out += f"""  <body>
    <div class="wrap">
{nav(lang, here, alt['path'])}
      <main>
        <article class="post">
          <p class="crumbs"><a href="{rel(here, t['home'])}">Dualrec</a> › <a href="{rel(here, t['blog'])}">Blog</a></p>
          <h1>{esc(post['title'])}</h1>
          <p class="post-meta">{meta_line}</p>
          <aside class="post-cta">
            <p>{t['cta_text']}</p>
            <div class="cta">
              {download_button(lang)}
            </div>
            <p class="fine">{t['cta_fine']}</p>
          </aside>
          <div class="prose">
{post['body']}
{faq_html}
          </div>
        </article>
      </main>

"""
    if others:
        out += f"""      <section class="more">
        <h2>{t['more']}</h2>
        <div class="post-list">
{''.join(card(lang, here, p, 'h3') for p in others)}        </div>
        <p class="fine"><a href="{rel(here, t['blog'])}">{t['all_posts']} →</a></p>
      </section>

"""
    return out + tail(lang, here)


def render_index(lang, posts):
    t, o = T[lang], OTHER[lang]
    here = t["blog"]
    schemas = [
        {
            "@context": "https://schema.org",
            "@type": "Blog",
            "name": t["blog_h1"],
            "description": t["blog_desc"],
            "url": SITE + here,
            "inLanguage": lang,
            "publisher": ORG,
            "blogPost": [
                {"@type": "BlogPosting", "headline": p["title"], "url": SITE + p["path"],
                 "datePublished": p["date"].isoformat(), "author": ORG}
                for p in posts
            ],
        },
        breadcrumb(lang, [("Dualrec", t["home"]), ("Blog", here)]),
    ]
    out = head(lang, here, t["blog_title"], t["blog_desc"], {lang: here, o: T[o]["blog"]}, "website", schemas)
    out += f"""  <body>
    <div class="wrap">
{nav(lang, here, T[o]['blog'])}
      <main>
        <header class="blog-head">
          <p class="crumbs"><a href="{rel(here, t['home'])}">Dualrec</a> › Blog</p>
          <h1>{t['blog_h1']}</h1>
          <p class="sub">{t['blog_sub']}</p>
        </header>
        <div class="post-list">
{''.join(card(lang, here, p, 'h2') for p in posts)}        </div>
      </main>

"""
    return out + tail(lang, here)


def render_feed(lang, posts):
    t = T[lang]
    items = "".join(f"""    <item>
      <title>{esc(p['title'])}</title>
      <link>{SITE}{p['path']}</link>
      <guid isPermaLink="true">{SITE}{p['path']}</guid>
      <pubDate>{rfc822(p['date'])}</pubDate>
      <description>{esc(p['description'])}</description>
    </item>
""" for p in posts)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>{esc(t['feed_title'])}</title>
    <link>{SITE}{t['blog']}</link>
    <description>{esc(t['blog_desc'])}</description>
    <language>{t['rss_lang']}</language>
    <lastBuildDate>{rfc822(max(p['updated'] for p in posts))}</lastBuildDate>
    <atom:link href="{SITE}{t['blog']}feed.xml" rel="self" type="application/rss+xml" />
{items}  </channel>
</rss>
"""


def sitemap_entry(paths, lastmod, lang):
    return f"""  <url>
    <loc>{SITE}{paths[lang]}</loc>
    <lastmod>{lastmod.isoformat()}</lastmod>
    <xhtml:link rel="alternate" hreflang="es" href="{SITE}{paths['es']}" />
    <xhtml:link rel="alternate" hreflang="en" href="{SITE}{paths['en']}" />
    <xhtml:link rel="alternate" hreflang="x-default" href="{SITE}{paths['en']}" />
  </url>
"""


def render_sitemap(posts):
    """Conserva las entradas que no son del blog (la home) y reescribe las del blog."""
    existing = io.open("sitemap.xml", encoding="utf-8").read() if os.path.exists("sitemap.xml") else ""
    kept = [b for b in re.findall(r"<url>.*?</url>", existing, re.S)
            if "/blog/" not in re.search(r"<loc>(.*?)</loc>", b, re.S).group(1)]
    out = '<?xml version="1.0" encoding="UTF-8"?>\n'
    out += '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n'
    out += "".join(f"  {b}\n" for b in kept)
    index_paths = {lang: T[lang]["blog"] for lang in LANGS}
    newest = max(p["es"]["updated"] for p in posts)
    for lang in LANGS:
        out += sitemap_entry(index_paths, newest, lang)
    for p in posts:
        paths = {lang: p[lang]["path"] for lang in LANGS}
        for lang in LANGS:
            out += sitemap_entry(paths, p[lang]["updated"], lang)
    return out + "</urlset>\n"


def remove_stale(expected):
    """Borra notas generadas antes cuyo slug ya no existe (solo archivos con la marca del generador)."""
    for lang in LANGS:
        base = T[lang]["blog"]
        if not os.path.isdir(base):
            continue
        for name in os.listdir(base):
            folder = os.path.join(base, name)
            page = os.path.join(folder, "index.html")
            if not os.path.isfile(page) or (base + name + "/") in expected:
                continue
            if MARK in io.open(page, encoding="utf-8").read():
                os.remove(page)
                if not os.listdir(folder):
                    os.rmdir(folder)
                print("Borrada (ya no hay nota con ese slug):", page)


def main():
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    try:
        posts = load_posts()
        for p in posts:
            for lang in LANGS:
                expand(p[lang], posts)
        remove_stale({p[lang]["path"] for p in posts for lang in LANGS})
        for lang in LANGS:
            in_lang = [p[lang] for p in posts]
            for p in posts:
                post = p[lang]
                others = [x for x in in_lang if x is not post][:3]
                write(post["path"] + "index.html", render_post(post, p[OTHER[lang]], others))
                note = "" if WORDS_MIN <= post["words"] <= WORDS_MAX else f"  (fuera de {WORDS_MIN}-{WORDS_MAX})"
                print(f"{post['path']}index.html  {post['words']} palabras{note}")
            write(T[lang]["blog"] + "index.html", render_index(lang, in_lang))
            write(T[lang]["blog"] + "feed.xml", render_feed(lang, in_lang))
            print(f"{T[lang]['blog']}index.html y feed.xml")
        write("sitemap.xml", render_sitemap(posts))
        print("sitemap.xml actualizado")
    except BuildError as e:
        print("Error:", e)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
