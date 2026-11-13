# LAN/HTTPS y límites de memoria · sin inferencia

## Configuración de evaluación (ADR0056)

Botón **Configuración** en la barra lateral; opciones Imágenes y Multiagente
(Gemma coordinador +Qwen auxiliar por turnos), más límites/diagnóstico existentes.
`--evaluation-web` y aprobación `manual-local-images-and-multiagent-evaluation-web`
del host con flags LAN/TLS/history-dir adjuntan ambos adaptadores autorizados,
inicialmente on/on si no existía elección guardada. Nuevos arranques respetan la
elección persistida. No se envían prompts/warmup automáticamente. El perfil Gemma
manual anterior y perfiles sin adaptadores mantienen las funciones no disponibles.

Imagen usa el control explícito Imagen del compositor; no se anuncia como una tool
conversacional que genere por prosa. Multiagente habilita tools reales de borrador
code_job_submit/list/status y Qwen privado con un residente por turno. Un trabajo
en cola no acredita ejecución; los borradores no se prueban, ejecutan o escriben
en proyectos. No búsqueda/shell/archivos por tener una casilla de multiagente.
Se diferencia habilitado de ocupado/no admitido; el host aplica permisos/lease/
memoria. Cambiar opciones no ejecuta modelos, aumenta techos ni recupera fallos.
Para probar nuevos modos, iniciar conversación corta nueva si el historial ya
agota el presupuesto acotado; nunca se trunca silenciosamente.

ADR0053 y plan `plans/2026-11-11-lan-y-diagnostico.md`. No levantar el bloqueo
de carga ADR0051 para comprobar conectividad. `seiya-web-diagnostics` es un host
Rust temporal separado: misma UI, chat explícitamente desactivado, sin broker de
imagen ni ejecutores/transportes reales adjuntos. No lee pesos o invoca runtimes.
No comparte los servicios OpenCode/8010/11434 ni los reinicia.

## Arranque explícito autorizado

```text
seiya-web-diagnostics --output ROOT_NUEVO_PRIVADO
  --approval temporary-LAN-TLS-diagnostics-without-inference
  --listen IP_PRIVADA:8016 --prefix-length BITS
  --certificate-der CERT_DER --private-key-der KEY_DER
  --settings-dir DIRECTORIO_PRIVADO_ESTABLE
  [--previous-stop ARCHIVO_STOP_PRIVADO]
```

ROOT debe ser hijo directo nuevo del temporal operativo aprobado. Settings-dir
existente/absoluto/canónico0700; ficheros0600 y un escritor exclusivo. Para que
un futuro host real use la misma política, seleccionar su `HISTORY/settings`
como settings-dir: `seiya-image-web --history-dir HISTORY` abre esa subcarpeta.
No arrancar el host real sin permiso/preflight nuevos; cambiar settings no lo
activa. Almacén temporal no promete supervivencia a la limpieza del SO.

IP privada concreta, no wildcard; fuente TCP restringida a la subred IPv4 elegida.
TLS1.3, certificado local y clave privada0600 fuera del repositorio. Host/Origin
HTTPS exactos y autenticación antes de controles. Cookie Secure/HttpOnly/
SameSite=Strict. No UPnP, port-forwarding, firewall o confianza global instalados.
Dos bootstrap de un uso: `probe.url` para autocheck del operador y `access.url`
para el usuario. No consumir access.url en el autocheck ni guardar claves en HTML.
Vida del diagnóstico≤30min; acceder tarde no amplía el plazo del host.

En el otro PC abrir el enlace de access.url. Certificado autofirmado puede causar
advertencia: verificar IP y huella SHA256 que facilita el operador por canal
confiable antes de aceptar excepción local. Nunca desactivar TLS globalmente,
compartir key.der/key.pem o importar una CA sin decisión del propietario del PC.
Si no conecta, verificar que ambos estén en la subred y que no haya aislamiento
de clientes Wi-Fi; no cambiar firewall/router automáticamente. Pruebas desde el
propio Mac no acreditan acceso desde otro PC o navegador gráfico.

## Panel y contrato efectivo

`diagnostics.rs` aplica los límites; JS añade únicamente controles con estilos
existentes. API `POST /api/diagnostics/v1`, JSON cerrado≤4KiB, status/configure y
revisión CAS. Contrato/ejemplos: `contracts/diagnostics/v0.1.0/`.

| Campo | Rango permitido | Default |
|---|---|---|
| Gemma warning acumulado |0..10000ms|10000ms|
| Gemma crecimiento swap |64..2048MiB|2048MiB|
| Gemma plazo por turno |10..120s|120s|
| Imagen crecimiento swap |64..512MiB|512MiB|
| Imagen plazo |30..600s|600s|
| Duración sesión siguiente arranque real |60..1800s|1800s|

Imagen nunca permite warning; critical/unknown siempre detienen. No sliders para
desactivarlos o borrar cuarentena. Guardar no carga modelos ni cambia proveedor,
no descarga pesos, no confirma permisos de inferencia o capacidad de RAM.
Se puede volver a los techos previos dentro del rango, no aumentarlos.
El host real captura la política al empezar un turno; no aplica relajaciones a
una ejecución iniciada. Duración sesión solo se usa al arrancar, no prolonga una
sesión en curso. Las CLIs de ensayos anteriores conservan sus propias políticas.

Snapshot de configuración/último stop atómico antes de ACK. Conflicto409 requiere
actualizar y revisar cambios;400 fuera de contrato y503 almacenamiento bloqueado.
Un error ambiguo no confirma ni desmiente escritura: status/revisión antes de
reenviar, sin reintento automático. Campos deshabilitados durante envío para no
pisar una edición con un ACK tardío. Fallo de almacenamiento cierra admisión del
host real; editar no elimina su fallo/quarentena ni reinicia servicios.

## Qué significa cada diagnóstico

Presión/swap actuales son observación read-only del **host completo**, con fecha,
no RSS/pico del modelo, porcentaje, presupuesto seguro o residencia comprobada.
Estado unknown se presenta como desconocido. Normal ahora no invalida critical
previo ni habilita inferencia por sí mismo. El panel separa el último bloqueo de
la muestra actual: importar un stop previo lo etiqueta ensayo anterior.

Importación solo desde ruta explícita del operador, archivo regular≤64KiB y
muestra acotada; no rutas del navegador, prompts, PIDs o logs en respuestas.
Futuros stops de Gemma e imagen registran muestra/condición desde código del host.
Critical se identifica con presión4; otras observaciones incompletas se muestran
como política/observación no válida, no se inventa una causa más precisa.
Este registro no sustituye evidencia nativa de cleanup/lease ni su revisión.

## Verificación y rollback

Prueba breve de funcionamiento posterior autorizada, sin web: ADR0054 y
`plans/2026-11-12-prueba-gemma.md`. Seiya-image-web tiene variante aditiva
`--gemma-smoke ROOT_NUEVO --approval one-isolated-gemma-e2b-response-with-memory-guards`:
un marcador/≤120s/≤256tokens, default conservador vigente y evidencia por proceso,
sin preparar imagen o adjuntar servidor. No ejecutar por copiar el comando:
necesita permiso específico del ensayo y preflight frescos; no retry automático.
Una respuesta válida no elimina cuarentena previa ni autoriza/promueve la web.

Prueba interactiva ADR0055: `seiya-image-web --gemma-web --output ROOT_NUEVO
--approval temporary-manual-gemma-only-LAN-web` y flags LAN/TLS/history-dir del
host. No smoke, imágenes o Qwen: solo chat real que carga al escribir el usuario.
El agente verifica conexión/health con probe distinto, nunca consume el enlace
del usuario o envía un marcador para sustituir su conversación. Fecha/enlace
vigentes y límites en `plans/2026-11-12-gemma-web-usuario.md`.

Tests normales: límites, CAS, almacenamiento/fallo, HTTP/auth/framing y DOM con
fixtures. Runtime real **no probado**. Autocheck LAN autorizado usa un cliente
TLS con confianza explícita en el certificado/IP destino; no modo insecure.
Se comprueban401 sin cookie,200 con cookie,403 Host/Origin,503 chat y404 imágenes
en perfil sin inferencia. No confundir rechazo TLS del cliente con un403 HTTP.
La validación remota/navegador del usuario queda registrada separadamente.

Rollback: dejar expirar/parar solo el host de diagnóstico propio y omitir
`with_diagnostics`; conservar settings/certificados/evidencia/historial. No commit,
publicación, instalación o cambio de servidores compartidos por estos comandos.
