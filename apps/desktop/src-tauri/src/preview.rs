//! Servidor local mínimo que publica la cámara (`/cam.mjpg`) y la pantalla
//! elegida (`/screen.mjpg`) como MJPEG (`multipart/x-mixed-replace`) para la
//! burbuja y la vista previa. Solo escucha en 127.0.0.1.

use crate::camera::CamShared;
use parking_lot::{Condvar, Mutex};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

/// Última imagen JPEG de una fuente que no es la cámara (la pantalla).
#[derive(Default)]
pub struct Feed {
    /// (jpeg, número de secuencia). Un JPEG vacío significa "sin imagen".
    pub frame: Mutex<(Arc<Vec<u8>>, u64)>,
    pub cv: Condvar,
}

impl Feed {
    pub fn publish(&self, jpeg: Vec<u8>) {
        let mut f = self.frame.lock();
        f.0 = Arc::new(jpeg);
        f.1 += 1;
        drop(f);
        self.cv.notify_all();
    }

    /// Espera hasta medio segundo por una imagen más nueva que `last`.
    fn next(&self, last: u64) -> Option<(Arc<Vec<u8>>, u64)> {
        let mut f = self.frame.lock();
        if f.1 == last {
            self.cv.wait_for(&mut f, Duration::from_millis(500));
        }
        (f.1 != last).then(|| (f.0.clone(), f.1))
    }
}

fn next_cam(shared: &CamShared, last: u64) -> Option<(Arc<Vec<u8>>, u64)> {
    let mut p = shared.preview.lock();
    if p.seq == last {
        shared
            .preview_cv
            .wait_for(&mut p, Duration::from_millis(500));
    }
    (p.seq != last).then(|| (p.jpeg.clone(), p.seq))
}

pub fn start(shared: Arc<CamShared>, screen: Arc<Feed>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new()
        .name("dualrec-preview".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = shared.clone();
                let screen = screen.clone();
                let _ = std::thread::Builder::new()
                    .name("dualrec-preview-client".into())
                    .spawn(move || {
                        let _ = serve(stream, shared, screen);
                    });
            }
        })?;
    Ok(port)
}

fn serve(mut stream: TcpStream, shared: Arc<CamShared>, screen: Arc<Feed>) -> std::io::Result<()> {
    let mut req = [0u8; 1024];
    let n = stream.read(&mut req)?;
    let wants_screen = req[..n]
        .split(|b| *b == b'\r')
        .next()
        .map(|line| line.windows(7).any(|w| w == b"/screen"))
        .unwrap_or(false);
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(
        b"HTTP/1.1 200 OK\r\n\
          Content-Type: multipart/x-mixed-replace; boundary=frame\r\n\
          Cache-Control: no-store\r\n\
          Access-Control-Allow-Origin: *\r\n\
          Connection: close\r\n\r\n",
    )?;
    let mut last = 0u64;
    loop {
        let next = if wants_screen {
            screen.next(last)
        } else {
            next_cam(&shared, last)
        };
        match next {
            Some((jpeg, seq)) => {
                last = seq;
                if jpeg.is_empty() {
                    continue;
                }
                write!(
                    stream,
                    "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                    jpeg.len()
                )?;
                stream.write_all(&jpeg)?;
                stream.write_all(b"\r\n")?;
                stream.flush()?;
            }
            // Sin cuadros nuevos: se verifica que el cliente siga conectado.
            None => {
                let mut probe = [0u8; 1];
                stream.set_read_timeout(Some(Duration::from_millis(1)))?;
                if let Ok(0) = stream.read(&mut probe) {
                    return Ok(());
                }
            }
        }
    }
}
