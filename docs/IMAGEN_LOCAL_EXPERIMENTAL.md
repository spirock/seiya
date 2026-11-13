# Imagen local experimental · Rust/macOS

ADR0053 añade [diagnóstico LAN y configuración](DIAGNOSTICO_LAN.md) sin inferencia.
El host opt-in consume límites por turno, dentro de techos anteriores; critical/
unknown siguen siendo stop y no se relajan guardas a mitad de ejecución.

Historial persistente opt-in y recuperación sin replay: [operación](HISTORIAL_TRABAJOS.md),
ADR0052. Sustituye las limitaciones de volatilidad de ADR0050 solo cuando el host
adjunta un almacén estable. Los perfiles previos siguen volátiles.

## Activación temporal con chat y LAN (ADR0051)

Host opt-in `seiya-image-web`: Gemma real e imágenes en la misma UI, **por turnos**,
Qwen apagado. Un slot común y la lease existente; Gemma arranca en runtime privado
por petición y confirma cierre de su árbol antes de liberar el turno. No adopta
Ollama11434 ni cierra servicios compartidos. Recargar añade latencia. Durante imagen
el chat conserva historial pero está ocupado; errores no producen respuesta falsa.
Permiso30min, cancelación/drenaje al cerrar; sin generación automática de imagen.
Imagen conserva guardas estrictas normal/swap+512MiB; Gemma warning10s/swap2GiB,
critical inmediato,120s. Fallo de cleanup/drop bloquea cargas y deja evidencia.

LAN explícita autorizada: IP privada concreta y subred seleccionada, puerto8015,
TLS1.3 y certificado local autofirmado con clave0600 fuera del repositorio.
Bootstrap de un uso, cookie Secure/HttpOnly/SameSite y Host/Origin HTTPS exactos.
No Internet, UPnP, port-forwarding, cambio de firewall ni runtime expuesto.
En el otro PC se debe verificar la huella del certificado antes de confiarlo;
no instalar confianza automáticamente ni compartir la clave privada. Un certificado
no confiado puede mostrar advertencia del navegador. Comprobar desde el otro PC
es validación pendiente del usuario, no acreditada por localhost.
CLI exige `--output ROOT_NUEVO --approval temporary-real-gemma-and-images-sequential-web`
y `--listen IP_PRIVADA:8015 --prefix-length BITS --certificate-der CERT --private-key-der KEY`
además de `--history-dir DIRECTORIO_PRIVADO_ESTABLE` desde ADR0052.
No ejecutar fuera del permiso vigente. La evidencia actual está en
`plans/2026-11-11-activacion-imagenes.md`; rollback: dejar expirar host propio,
sin borrar pesos, historial, evidencias o configuraciones.

## Conexión web implementada, apagada por defecto (punto2)

ADR0050 y [hoja de ruta pública](../ROADMAP.md).
`image_jobs` Rust es un broker independiente del de borradores: petición explícita,
cola, estado observado, cancelación confirmada y PNG autenticado descargable.
`prepared_web` puede adjuntarlo antes de compartir su estado:

```rust,ignore
let executor = Arc::new(NativeImage::new(runtime_files, image_permission));
let images = ImageJobs::new(executor, image_admission);
let web = web.with_images(images)?;
```

Es un esquema de integración del host, **no configuración para copiar/arrancar**:
`image_permission` y `image_admission` deben venir del propietario autorizado
con runtime/recursos comprobados. No son campos del navegador/modelo. Permiso y
ejecutor defaultfalse; las CLIs anteriores no llaman `with_images` o habilitan
cargas. La nueva CLI temporal explícita de ADR0051 es la excepción opt-in.
El archivo de selección de pesos sigue `available_in_seiya:false` correctamente.

API `/api/images/v1` opt-in, separada de OpenAI y texto jobs-v1. Ejemplos y wire:
[`contracts/image-jobs/v0.1.0`](../contracts/image-jobs/v0.1.0/README.md).
Auth/Host/Origin y JSON cerrado≤16KiB antes de control, capacidad opaca volátil
por chat, UUID idempotente por envío y request inmutable. Primer perfil512×512,
seed u32 (UI42), prompt≤2048bytes; sin rutas, URLs o motores en input. Una
ejecución/2 esperas,8 registros/16 sesiones, PNG≤16MiB/32MiB total retenido.
Sesión30min inactiva renovada por controles; no GC/persistencia/replay en esta fase.

Estados reales queued/preparing/running/validating/cancelling y terminales.
Sin porcentajes/ETA inventados. Cancelar no confirma cleanup; se esperan bytes
o fallo confirmado, deadline620s incluida cola y gracia10s. Timeout incierto,
panic/drop→quarantine, sin reset/retry HTTP. Bytes tardíos tras cancelación o
cuarentena del pool se descartan. Cuota llena no pisa artefactos anteriores.
Artefacto solo después de decoder/CRC/zlib/filtros/dimensiones/hash y cleanup;
respuesta PNG binaria con attachment/nombre fijo/nosniff/no-store/length y SHA256.

Adaptador nativo nuevo `image/job.rs`: permiso positivo del host antes de I/O y
durante preparación/ejecución, hashes en bloques cancelables, mismo sd.cpp fijado,
argumentos aislados (no shell), pesos protegidos y red del hijo denegada. Lease
existente impide generar mientras un host Gemma la retenga;11434 debe estar vacío.
**No descarga/cierra/adopta Gemma u otro servicio automáticamente.** El propietario
del flujo debe resolver esa transición antes de activar la capacidad: no prometer
concurrencia de imagen y LLM por este código. No imágenes por herramienta del chat.

El nuevo ejecutor usa memoria estricta normal/swap+512MiB y deadline600s desde
invocación, sin heredar W5 de memoria informativa. Mantiene disco/log y modelo/
hash/lease/ownership, y una future perdida también intenta cleanup. Si no confirma,
guarda marcador persistente; si falla persistencia, retiene flock del proceso.
No garantía hard-real-time/OOM ni presupuesto16GiB. Esta política nueva y el flujo
web **no se han probado con inferencia real**; W5 no demuestra que este perfil
vaya a superar sus guardas en este Mac. Otro ensayo requiere autorización explícita.

Control Imagen opcional en compositor con mismos estilos y panel, sin editar
HTML/CSS original. Solo visible si la capacidad efectiva está disponible. Estado,
cancel, ver Blob PNG y descarga requieren sesión propia; refrescar pierde enlaces
y no reenvía. Confirmar rechazo permite editar; error ambiguo conserva UUID para
retry manual. Revocar capacidad no convierte silenciosamente el texto en chat.
Borrar conversación con imagen activa solicita cancelación y espera confirmación;
cuarentena conserva chat para revisión. URLs Blob se revocan al eliminar chat.
Capacidades, claves y PNG no se guardan en localStorage; el borrador textual
normal del compositor conserva el comportamiento de historial existente.

Validación del incremento: ejecutores/verificador simulados para lifecycle y HTTP,
PNG sintético realmente decodificado en macOS y DOM/red simulados para UI. Sin
generar otra imagen, mostrar como nueva la imagen W5 o abrir servidores/modelos.
El decoder nativo actual sigue limitado a macOS; disponibilidad false en otros
targets hasta adaptador validado. Uniforme/invisible se trata como fallo conservador
en este primer perfil; no garantía de calidad semántica o fidelidad al prompt.
Logs del runtime pueden contener datos del prompt: permanecen privados, no se
devuelven por API. Evidencia del host registra tamaño de prompt, no su texto.

Rollback: no adjuntar ImageJobs; perfiles actuales permanecen sin imágenes. No
instalación, publicación, borrado de pesos/evidencia/historial o cambio de proveedor.

Plan y permisos: `plans/2026-11-11-web-completa-imagenes.md` y ADR0045.
No herramienta web disponible por compilar, descargar o verificar pesos.

## Componentes

- `seiya-contracts::image_job`: petición cerrada y contenedor PNG acotado;
  no ejecuta inferencia ni acredita píxeles por la firma.
- `seiya-observe::image`: SHA256, selección fijada, downloader, decoder RGB/RGBA8,
  proceso original con PID/inicio/grupo, lease, deadline y evidencia.
- Inferencia externa privada: stable-diffusion.cpp/Metal. No reimplementación
  Rust de los kernels, instalación o redistribución de pesos/runtime.
- Decoder usa zlib del SO por FFI acotada en macOS. Downloader HTTPS usa TLS Rust
  con certificados públicos, DNS/IP fijada por conexión y redirects limitados.
  Estos adaptadores no se presentan como portables a Linux ya probados.

## Comprobación sin modelos

```sh
cargo test -p seiya-observe --lib image:: --offline --locked
cargo test -p seiya-observe --lib probes::owned --offline --locked
cargo test -p seiya-contracts image_job --offline --locked
cargo build -p seiya-observe --release --bin seiya-image-assets --bin seiya-image-smoke --offline --locked
# Rutas locales explícitas; ROOT_NUEVO debe ser hijo directo del temporal aprobado.
seiya-image-assets --verify DIRECTORIO_EXISTENTE
seiya-image-smoke --rehearsal ROOT_NUEVO
# Revalidar un ensayo existente sin generar otra imagen ni alterar evidencia:
seiya-image-smoke --validate-existing DIRECTORIO_EVIDENCIA
```

No ejecutar una transferencia nueva para verificar archivos existentes. La CLI
de descarga exige selección/aprobación explícita y directorio nuevo; no retoma,
reintenta ni sobrescribe. Preserva parciales tras error. Publica con hard-link
no-clobber y conserva el parcial verificado (mismo inode, no copia adicional).
Solo bytes sintéticos prueban el nuevo transporte Rust; W2 fue transferida antes
con el prototipo retirado y revalidada posteriormente con Rust.

## Ensayo real y límites

La CLI real exige aprobación explícita, binario/hash seleccionado y assets
exactos. No interpretar ese argumento como un permiso concedido por un modelo.
Una imagen sintética512×512, seed42,8 pasos/CFG1/Euler,≤600s de ejecución;
sin LLM residentes. Lease existente compartida, sin unlink/recreación, pesos
protegidos y red del hijo denegada. No adopta ni descarga modelos de Ollama.
Memoria informativa: no garantía de ausencia de OOM, estabilidad o presupuesto
concurrente. El plazo y los límites de disco/log se supervisan; no hard-real-time.

Solo se confirman artefactos PNG regulares≤16MiB tras CRC, dimensiones, zlib,
filtros, contenido no uniforme/visible y hashes. Eso no certifica calidad visual.
La evidencia privada guarda muestras/categorías, no credenciales. Si el cleanup
es incierto se conserva un marcador `image-cleanup-uncertain.json` que bloquea
esta ruta hasta revisión humana; no borrar el marcador para eludir el bloqueo.
macOS no ofrece pidfd: identidad se verifica antes de señalizar pero la carrera
residual entre consulta y syscall se declara, no una garantía absoluta.

Refuerzo posterior ADR0048: close pausa padres/nodos propios verificados antes
de terminar cada proceso, para evitar que un padre reponga workers durante
cleanup. Probado con fixture nativa padre+worker, no con otra imagen real.
Cuarentena de cleanup incierto del ensayo web también bloquea nuevas cargas de
esta ruta; no borrar el marcador para eludirla.

No activación web, API de imágenes, nuevos puertos o cambios al servicio por
estos comandos. Rollback: omitir estas CLIs/módulos; preservar modelos, evidencia,
historial y configuración existente. No retry automático tras el ensayo único.

El resultado real y el estado de disponibilidad se registran en el plan activo;
la vista previa8010 sigue siendo una simulación de modelos.

Ensayo W5 completado:167,335s, PNG512×512 RGBA348659bytes, cubo azul sobre blanco,
exit0 y cleanup/grupo vacío/lease confirmados. Metal seleccionado en Apple M4,
encoder/VAE en CPU; hubo presión warning/crítica y swap host máximo6.670.516.224
bytes. No presupuesto seguro para el conjunto ni estabilidad sostenida. Parser
endurecido revalidó después el mismo PNG sin repetir inferencia. Permiso único
consumido; resultados/hash y muestras completos en el plan, evidencia original
privada preservada. No autoriza otra imagen o activar la web.
