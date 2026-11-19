"""Banda de sonido original del reel (120 BPM, La menor), sincronizada con reel.html.
Uso: uv run --with numpy --with scipy audio.py
"""
import wave
import numpy as np
from scipy.signal import butter, sosfilt

SR = 44100
DUR = 19.0
N = int(SR * DUR)
BEAT = 0.5
rng = np.random.default_rng(7)
mix = np.zeros((N, 2))


def filt(x, kind, hz, order=2):
    return sosfilt(butter(order, hz, kind, fs=SR, output="sos"), x)


def add(sig, t0, gain=1.0, pan=0.0, bus=None):
    bus = mix if bus is None else bus
    i = int(t0 * SR)
    if i >= N:
        return
    sig = sig[: N - i] * gain
    bus[i : i + len(sig), 0] += sig * (1 - max(pan, 0))
    bus[i : i + len(sig), 1] += sig * (1 + min(pan, 0))


def tt(d):
    return np.arange(int(d * SR)) / SR


def kick():
    t = tt(0.42)
    f = 46 + 130 * np.exp(-t * 34)
    body = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * 8.5)
    click = filt(rng.standard_normal(len(t)), "high", 3000) * np.exp(-t * 400) * 0.35
    return np.tanh((body + click) * 1.8)


def clap():
    t = tt(0.3)
    n = filt(rng.standard_normal(len(t)), "band", [900, 4200])
    env = sum(np.exp(-np.clip(t - d, 0, None) * 110) * (t >= d) for d in (0, 0.011, 0.023)) * 0.5 + np.exp(-t * 16) * 0.5
    return n * env


def hat(d=0.05, k=90):
    t = tt(d)
    return filt(rng.standard_normal(len(t)), "high", 7500) * np.exp(-t * k)


def saw(f, t, harm=14):
    return sum(np.sin(2 * np.pi * f * h * t) / h for h in range(1, harm) if f * h < 16000)


def bass(f, d=0.22):
    t = tt(d)
    x = saw(f, t, 9) * 0.55 + np.sin(2 * np.pi * f * t)
    env = np.minimum(t / 0.006, 1) * np.exp(-t * 5) * np.minimum((d - t) / 0.02, 1)
    return np.tanh(filt(x, "low", 420) * env * 1.4)


def pluck(f, d=0.3, bright=3200.0):
    t = tt(d)
    x = saw(f, t) + 0.5 * saw(f * 1.006, t)
    return filt(x, "low", bright) * np.exp(-t * 13) * np.minimum(t / 0.003, 1)


def lead(f, d):
    t = tt(d + 0.15)
    vib = 1 + 0.004 * np.sin(2 * np.pi * 5.5 * t)
    ph = 2 * np.pi * np.cumsum(f * vib) / SR
    x = np.sign(np.sin(ph)) * 0.5 + np.sin(ph) + 0.4 * np.sin(2 * ph)
    env = np.minimum(t / 0.008, 1) * np.where(t < d, 1.0, np.exp(-(t - d) * 28)) * (0.75 + 0.25 * np.exp(-t * 6))
    return filt(x, "low", 4200) * env


def whoosh(d=0.55):
    t = tt(d)
    n = rng.standard_normal(len(t))
    lo, hi = filt(n, "band", [300, 1500]), filt(n, "band", [2500, 9000])
    k = t / d
    return (lo * (1 - k) + hi * k) * np.exp(-((k - 0.72) ** 2) / 0.035)


def riser(d):
    t = tt(d)
    k = t / d
    n = rng.standard_normal(len(t))
    noise = (filt(n, "band", [400, 2000]) * (1 - k) + filt(n, "high", 4000) * k) * k**2
    f = 220 * 2 ** (k * 2)
    tone = np.sin(2 * np.pi * np.cumsum(f) / SR) * k**2 * 0.35
    return noise + tone


def impact():
    t = tt(2.2)
    sub = np.sin(2 * np.pi * np.cumsum(32 + 70 * np.exp(-t * 9)) / SR) * np.exp(-t * 2.6)
    crash = filt(rng.standard_normal(len(t)), "high", 3500) * np.exp(-t * 3.2) * 0.5
    return np.tanh(sub * 1.6) + crash


def pop(f):
    t = tt(0.16)
    fr = f * (1 + 0.9 * np.minimum(t / 0.05, 1))
    return np.sin(2 * np.pi * np.cumsum(fr) / SR) * np.exp(-t * 26) * np.minimum(t / 0.002, 1)


def tick():
    t = tt(0.07)
    return (np.sin(2 * np.pi * 1900 * t) * np.exp(-t * 90) + filt(rng.standard_normal(len(t)), "high", 5000) * np.exp(-t * 500) * 0.6)


def ding(f):
    t = tt(0.9)
    return (np.sin(2 * np.pi * f * t) + 0.5 * np.sin(2 * np.pi * f * 2 * t) + 0.25 * np.sin(2 * np.pi * f * 3.01 * t)) * np.exp(-t * 6)


A = 440.0
note = lambda semis: A * 2 ** (semis / 12)  # semitonos respecto de La4
# acordes por compás: Am, F, C, G (el loop que todo el mundo reconoce)
CHORDS = [(-12, [0, 3, 7, 12]), (-16, [-4, 0, 3, 8]), (-9, [-9, -5, 3, 7]), (-14, [-2, 2, 5, 10])]
ARP = [0, 1, 2, 3, 2, 1, 2, 3, 0, 1, 2, 3, 2, 3, 1, 2]
# melodía principal, en corcheas por compás (None = silencio)
HOOK = [
    [7, None, 7, 5, 7, None, 12, None],
    [8, None, 8, 7, 5, None, 3, None],
    [7, None, 7, 5, 7, None, 10, 12],
    [14, None, 12, None, 10, 7, 5, None],
]

music = np.zeros((N, 2))  # bus que se agacha con el bombo
kicks = []

for bar in range(9):
    t0 = bar * 2.0
    root, chord = CHORDS[bar % 4]
    intro = bar < 2
    final = bar >= 7
    # arpegio en semicorcheas
    for i, idx in enumerate(ARP):
        ts = t0 + i * 0.125
        if ts >= 18:
            break
        bright = 700 + 900 * (ts / 4) ** 2 if intro else 3400
        add(pluck(note(chord[idx]), bright=bright), ts, 0.16 if intro else 0.13, pan=0.35 * (-1) ** i, bus=music)
        add(pluck(note(chord[idx]), bright=bright * 0.7), ts + 0.1875, 0.05, pan=-0.5 * (-1) ** i, bus=music)
    if intro:
        if bar == 1:
            for i in range(8):
                add(hat(), t0 + i * 0.25, 0.10 + 0.02 * i)
        continue
    # bombo a negras, con corte antes del cierre
    for b in range(4):
        tk = t0 + b * BEAT
        if 13.5 <= tk < 14 or tk >= 18:
            continue
        kicks.append(tk)
        add(kick(), tk, 0.95)
    for b in (1, 3):
        if t0 + b * BEAT < 18:
            add(clap(), t0 + b * BEAT, 0.5)
    for i in range(8):
        ts = t0 + i * 0.25
        if ts >= 18:
            break
        off = i % 2 == 1
        add(hat(0.14, 26) if off else hat(), ts, 0.17 if off else 0.09, pan=0.2 if off else -0.2)
        if off:  # bajo a contratiempo
            add(bass(note(root - 12)), ts, 0.5, bus=music)
            add(bass(note(root - 12), 0.1), ts + 0.125, 0.3, bus=music)
    # melodía: entra en la parte de los archivos y vuelve en el cierre
    if bar >= 5 or bar in (2, 3):
        for i, n in enumerate(HOOK[bar % 4]):
            if n is not None and t0 + i * 0.25 < 18:
                add(lead(note(n), 0.2), t0 + i * 0.25, 0.085 if not final else 0.105, bus=music)
                add(lead(note(n), 0.2), t0 + i * 0.25 + 0.375, 0.03, pan=0.6, bus=music)

# el bombo empuja al resto (sidechain)
t = np.arange(N) / SR
duck = np.ones(N)
for tk in kicks:
    m = t >= tk
    duck[m] = np.minimum(duck[m], 1 - 0.7 * np.exp(-(t[m] - tk) / 0.11))
mix += music * duck[:, None]

# transiciones y efectos, calzados con la imagen
add(riser(2.0), 2.0, 0.34)
for i, ts in enumerate(np.cumsum([0.25] * 4 + [0.125] * 4 + [0.0625] * 6) + 2.75):
    if ts < 3.93:
        add(clap(), ts, 0.16 + 0.02 * i)
add(pop(180), 3.3, 0.45)                       # sello "todo pegado"
mix[int(3.93 * SR) : int(4.0 * SR)] *= 0.0       # silencio justo antes de la caída
add(impact(), 4.0, 0.8)
add(impact(), 14.0, 0.75)
for ts in (5.62, 9.6, 11.62):
    add(whoosh(), ts, 0.3, pan=0.3)
add(riser(0.5), 13.5, 0.3)
for i in range(4):
    add(clap(), 13.5 + i * 0.125, 0.3 + 0.06 * i)
for i, ts in enumerate((5.0, 5.15, 5.3, 5.45)):
    add(pop(520 + i * 90), ts, 0.22, pan=(-0.4, 0.4)[i % 2])
add(tick(), 6.5, 0.7)
add(tick(), 9.5, 0.7)
add(ding(note(12)), 9.5, 0.2)
add(ding(note(19)), 9.62, 0.16)
for i, ts in enumerate((10.0, 10.5, 11.0, 11.5)):
    add(pop(440 * 2 ** ((0, 3, 7, 12)[i] / 12)), ts, 0.4)
for i, ts in enumerate((12.5, 13.0, 13.5)):
    add(pop(600 + i * 150), ts, 0.3)
add(ding(note(12)), 15.5, 0.22)
add(ding(note(19)), 15.5, 0.14)
# golpe final con cola
add(kick(), 18.0, 0.95)
add(impact(), 18.0, 0.6)
add(ding(note(0)), 18.0, 0.2)
add(ding(note(7)), 18.0, 0.14)

mix *= 1.15
mix = np.tanh(mix)                               # limitador suave
fade = np.minimum(1, (DUR - t) / 0.25)
mix *= fade[:, None]
mix *= 0.89 / np.abs(mix).max()
rms = 20 * np.log10(np.sqrt((mix[int(4 * SR) : int(18 * SR)] ** 2).mean()))
print(f"pico {np.abs(mix).max():.2f} · RMS de la parte fuerte {rms:.1f} dBFS")

with wave.open("audio.wav", "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes((mix * 32767).astype("<i2").tobytes())
