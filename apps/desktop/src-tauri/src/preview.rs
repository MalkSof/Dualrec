//! Servidor local mínimo que publica la cámara como MJPEG
//! (`multipart/x-mixed-replace`) para la burbuja y la vista previa. Solo
//! escucha en 127.0.0.1.

use crate::camera::CamShared;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

pub fn start(shared: Arc<CamShared>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new()
        .name("dualrec-preview".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = shared.clone();
                let _ = std::thread::Builder::new()
                    .name("dualrec-preview-client".into())
                    .spawn(move || {
                        let _ = serve(stream, shared);
                    });
            }
        })?;
    Ok(port)
}

fn serve(mut stream: TcpStream, shared: Arc<CamShared>) -> std::io::Result<()> {
    let mut req = [0u8; 1024];
    let _ = stream.read(&mut req)?;
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
        let jpeg = {
            let mut p = shared.preview.lock();
            while p.seq == last {
                shared
                    .preview_cv
                    .wait_for(&mut p, Duration::from_millis(500));
                if p.seq == last {
                    break;
                }
            }
            if p.seq == last {
                None
            } else {
                last = p.seq;
                Some(p.jpeg.clone())
            }
        };
        match jpeg {
            Some(jpeg) => {
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
