# Dualrec en AWS

Todo corre en una sola cuenta y región de AWS:

| Pieza | Servicio de AWS |
| --- | --- |
| Videos (pistas subidas) | **S3**, bucket privado; se sube y se reproduce con URLs prefirmadas |
| Base de datos | **RDS PostgreSQL** (contraseña generada y guardada en Secrets Manager) |
| API | **ECS Fargate** (ARM), detrás de un balanceador que solo acepta tráfico de CloudFront |
| Web | **S3 + CloudFront** |
| HTTPS y dominio | **CloudFront**: la web en `/` y la API en `/api`, bajo el mismo dominio |
| Imagen de la API | **ECR**, construida por **CodeBuild** (no hace falta Docker local) |
| Logs | **CloudWatch Logs** (`/dualrec/api`) |

```
app de escritorio ──┐                       ┌── /        → S3 (web)
                    ├──► CloudFront (HTTPS) ┤
navegador ──────────┘                       └── /api/*   → balanceador → ECS Fargate (API) → RDS PostgreSQL
        │
        └── subida y reproducción directas con URLs prefirmadas ──► S3 (videos)
```

## Desplegar

Requisitos: [AWS CLI v2](https://docs.aws.amazon.com/cli/) con credenciales de un usuario que pueda crear estos recursos, y Node.js.

```powershell
aws login            # o: aws configure
powershell -ExecutionPolicy Bypass -File infra/aws/deploy.ps1 -AdminEmail vos@ejemplo.com -AdminName "Tu nombre"
```

El script hace todo: crea el repositorio de imágenes, construye la API en CodeBuild, despliega la aplicación, compila la web y la publica. La primera vez tarda unos 20 minutos (casi todo es la base de datos y CloudFront). Al terminar imprime:

- la **dirección de la web**,
- la **dirección para la app de escritorio** (va en *Ajustes → Nube*),
- el comando para leer el **token de acceso** del primer usuario.

Para actualizar después de cambiar código, se corre el mismo comando. Para publicar solo la web: agregar `-WebOnly`.

Otra región: `-Region sa-east-1` (São Paulo queda más cerca de Argentina; cuesta algo más que `us-east-1`).

## Costo aproximado

Estimación para `us-east-1` con los tamaños por defecto, sin contar almacenamiento ni tráfico. Son valores orientativos: conviene confirmarlos en la [calculadora de AWS](https://calculator.aws/).

| Recurso | USD por mes (aprox.) |
| --- | --- |
| RDS PostgreSQL `db.t4g.micro` + 20 GB | 15 |
| Fargate 0,5 vCPU / 1 GB, siempre encendido | 18 |
| Balanceador de aplicación | 17 |
| Direcciones IPv4 públicas (3) | 11 |
| Secrets Manager, logs, ECR | 2 |
| **Total fijo** | **~60** |

A eso se suma S3 (unos 0,023 USD por GB al mes) y la transferencia de datos al reproducir videos. Las cuentas nuevas de AWS tienen créditos y capa gratuita que cubren parte.

## Qué queda protegido

- Los buckets no son públicos. La web solo se lee a través de CloudFront y los videos solo con URLs prefirmadas que vencen.
- La base no tiene acceso desde internet: solo la API puede conectarse.
- El balanceador solo acepta conexiones que vienen de CloudFront.
- La contraseña de la base y el token inicial viven en Secrets Manager, no en el código ni en la plantilla.
- La base tiene copias de seguridad diarias (7 días) y protección contra borrado.

## Usar un dominio propio

Por defecto la dirección es la de CloudFront (`https://dxxxxxxxx.cloudfront.net`). Para usar un dominio propio hay que pedir un certificado en ACM (en `us-east-1`), agregarlo a la distribución junto con el nombre alternativo y apuntar el DNS a CloudFront. Todavía no está automatizado en la plantilla.

## Borrar todo

```powershell
aws cloudformation delete-stack --stack-name dualrec
aws cloudformation delete-stack --stack-name dualrec-build
```

Antes hay que desactivar la protección contra borrado de la base y vaciar el bucket de la web. El bucket de videos **no** se borra con la plantilla (queda retenido a propósito) y de la base queda una copia final.

## Estado

Las plantillas pasan `cfn-lint` sin avisos, pero **todavía no se desplegaron en una cuenta real**: la primera corrida puede requerir ajustes.
