# Trabajos auxiliares · perfil experimental

Historial persistente opt-in ADR0052: [operación](HISTORIAL_TRABAJOS.md).
Snapshots Rust de borradores/estados, recuperación autenticada por conversación
y reinicio sin reenvío; constructores anteriores conservan modo volátil.

## Imágenes: broker distinto y control manual opt-in

Punto2 implementa `image_jobs` y `/api/images/v1` en Rust, separados de los
borradores de este documento y de herramientas del interlocutor. UI opcional
con estilos existentes: petición, estado, cancelación confirmada y PNG validado
para ver/descargar. Apagado en perfiles/CLIs existentes, sin otra inferencia real.
Permiso y recursos del host preceden a activar. No se superpone al Gemma residente:
la lease y guardas impiden cargar sin exclusión; no se descarga un modelo por
iniciativa del navegador. [Contrato/operación y límites](IMAGEN_LOCAL_EXPERIMENTAL.md).

## Prueba reducida real (ADR0049)

El perfil reducido usa Gemma real **sin** Qwen/jobs/imágenes. Se reutiliza
HTML/CSS original en router Rust con modo cerrado: `/api/chat` textual y sin
tools/sesiones de jobs, `/api/jobs` no disponible, health modo `gemma-only`,
`jobs.enabled:false` e `images.available:false` (no respuestas simuladas).
Historial `seiya.gemma-trial.chats.v1`, separado del normal/demo/preparación.
Misma distribución/colores/diseño; solo textos de estado y rutas internas.

Arranque privado05/8014: un residente Gemma E2B/context metadata4096, warmup y
chat completo por PreparedWeb validados con runtime real, no modelo simulado.
Acceso por bootstrap de un uso/cookie privada; guardar pestaña tras abrir.
Vigencia≤30min desde arranque, guardas warning10s acumulado/swap+2GiB elegidas
por usuario, critical/unknown inmediatos. Sin garantía de sesión larga o RAM total.
No instalar/arrancar desde esta guía automáticamente; requiere permiso actual y
puerto libre. El enlace/token no se documenta ni versiona.

Selector CLI `--solo-gemma` y aprobación específica distinta del par. Staging de
solo Gemma, OLLAMA_MAX_LOADED_MODELS=1. Ninguna solicitud a Qwen, aunque el
transporte reutilizado pueda construirse pasivamente sin conectar. Lo implementado
no añade audio/visión, búsqueda web, acceso a archivos o control del PC.

Recuperación previa fue una acción explícitamente autorizada: lease de revisión,
marcador correlacionado con reporte original, ausencia raíz/grupo/puerto de
runtimes previos, host normal y11434 vacío, archivo con acta sin señales/cargas.
No borra evidencia ni confirma retrospectivamente todo el árbol del caso04;
loaders conservan guardas ante cualquier cuarentena nueva. No usar la revisión
como flag genérico para ignorar errores. Operación reproducible y rollback en
ADR0049 y plan activo. El par Gemma+Qwen sigue sin validación conjunta.

## Qué funciona y qué no

Broker Rust, adaptador de navegador y panel web: enviar un borrador, consultar
estado mientras se sigue hablando, cancelar y descargar texto no ejecutado.
Estado del ejecutor, no porcentajes o etapas inventadas por el interlocutor.
Confirmaciones y respuestas a tools de estado se formatean en el host desde el
registro, sin una segunda inferencia que pueda inventar avances. El interlocutor
aún debe elegir la herramienta adecuada; su seguimiento real de instrucciones
no se da por validado con el interlocutor sintético.
Un borrador no acredita sintaxis, pruebas o archivo escrito en un proyecto.

Probados con fixtures y transporte HTTP local real hacia **ejecutores simulados**.
No se ha probado Gemma/Qwen simultáneos ni existe admisión automática de memoria
real en este perfil. No se activó la configuración principal. La UI normal y /v1
del perfil normal se conservan; el perfil jobs es browser-only y rechaza /v1 chat
con409 para impedir eludir la cuarentena. No combinar con web_reader todavía.

## Ver la vista previa

Si está arrancada en esta sesión: **http://127.0.0.1:8010/**. Recargar tras cambiar
la UI. Banner «VISTA PREVIA SIMULADA»: ni Gemma ni Qwen reciben peticiones. Prueba:

1. «Escribe un programa Python de ejemplo».
2. Sin esperar a que termine, «Hola» o «Cómo va el programa».
3. Observa el panel, cancela o descarga el borrador cuando termine.

El interlocutor de demostración usa reglas fijas; no es un router LLM validado.
El auxiliar Demo espera12s y devuelve texto fijo claramente marcado. Los tiempos
de respuesta de esta vista **no son latencia de Gemma/Qwen**. La descarga la inicia
el usuario; nombre fijo `borrador.txt`, sin ejecutar HTML/código generado.

Arranque manual separado, sin instalar ni leer la configuración principal:

```sh
cargo build -p seiya-server --bin seiya-jobs-preview --locked --offline
cd swarm-app
python3 -m seiya.preview --port 8010 --jobs-binary /ruta/absoluta/target/debug/seiya-jobs-preview
```

No ejecutar si8010 tiene otro propietario. El frontend administra su backend
sintético mediante Popen original, puerto efímero y clave interna aleatoria;
Ctrl-C/SIGTERM cierra esos procesos propios. No detiene Ollama/OpenCode. La clave
de frontend `preview-local` es **pública y solo para la demo**; la página la rellena
automáticamente. No reutilizarla para perfiles reales o datos sensibles.
La demo usa `seiya.preview.chats.v1`, separada de `seiya.chats.v1` del perfil
normal. Recargar la pestaña para aplicar ese aislamiento si se abrió una versión
anterior de la vista previa; no se lee/migra/borra el historial real al hacerlo.

## Integración real, aún no activada

### Límites configurables del ensayo (ADR0047)

Resultado real posterior con configuración elegida por usuario: warning10s y
swap2GiB permiten respuesta breve Gemma; cargar Qwen lleva a **presión crítica**
y se detiene como acordado. Sin web con el par validada. Fallo de confirmación
de cleanup mantiene cuarentena; revisión posterior no observa raíz/grupo/puerto
propios pero no sustituye prueba completa del árbol. ADR0048 refuerza el cierre
con pausado de padres/descendientes verificados para evitar respawn; probado
con helper nativo sin modelos, no revalidado con MLX. No nuevas cargas mientras
persistan bloqueo crítico/cuarentena; ningún permiso anterior autoriza ignorarlos.

Petición posterior del usuario: corregir el preflight y permitir elegir límites
en configuración. Archivo local JSON del operador, no argumentos del modelo:
[`web-trial-limits.example.json`](../contracts/benchmarks/background-jobs/v0.1.0/web-trial-limits.example.json).
El CLI admite `--limits /ruta/al/archivo.json` después de `--output ... --approval ...`.
Se valida antes de abrir recursos. Sin rediseño ni cambios al UI original;
**por ahora es configuración en archivo, no un control nuevo de la web**.

| Campo | Default | Rango |
|---|---:|---:|
| `preflight_min_free_mib` |0|0–8192;0 usa ventana de presión/swap, no floor de páginas libres|
| `preflight_max_swap_growth_mib` |32|0–128|
| `max_swap_growth_mib` |512|64–2048|
| `warning_budget_ms` |0|0–10000 acumulados,0 para al primer warning|
| `max_session_seconds` |1800|60–1800, incluye preparación|
| `web_port` |8011|1024–65535 excepto8010/11434; ocupado falla sin adoptar|

`version:1`, extras/duplicados/tipos/tamaños/rangos inválidos rechazados.
Critical, unknown y observación obsoleta **siempre paran**. No desactivar todas
las guardas desde este archivo. Valores efectivos en evidencia; editar requiere
un arranque explícito del host temporal, no recarga automática o retry del modelo.
No cambiar el servicio compartido ni usar el perfil para prometer capacidad.

El default corrige un falso bloqueo por caché: nueve muestras normales a250ms
durante≥2s, swap estable y validación de disco/host/ownership; páginas libres
diagnósticas, no RAM disponible total. El criterio de2GiB previo se conserva en
evidencia histórica, no se borra. Los demás gates del proyecto no cambian.

**Actualización posterior ADR0046:** existe host experimental Rust/macOS
`seiya-pair-web` para la prueba real autorizada por usuario. El primer arranque
abrió UI temporal pero bloqueó preflight:128MiB libres<2GiB, presión normal,
**sin iniciar runtime/modelos**. No confundir este estado con integración real
validada. Acceso bootstrap privado, cookie HttpOnly/Strict en Path aleatorio
exclusivo, vigencia≤30min; no claves en UI/historial/logs. La raíz8011 no es el
enlace de acceso: usar el bootstrap privado de un uso. No se publica en docs.
Estado/evidencia en plan activo; misma apariencia, no cambios al UI original.

El host prepara store por links a blobs fijados y copias privadas de manifiestos,
protege fuente, verifica Child/listener y controla presión/swap/ownership/disco.
No modelos compartidos, purge, descargas o excepciones automáticas. Antes de
admitir exige warmup individual de ambos manteniendo residencia del par y un
smoke de tool-use Gemma→broker→Qwen (solo borrador no ejecutado). **Ese camino
real aún no se ejecutó** porque falló el preflight. No activar otro host mientras
exista runtime/lease propio sin confirmar cleanup. No permiso para otra imagen.

### Preparación browser en Rust (W6)

`seiya-server::prepared_web` añade **solo un constructor de librería**, sin CLI,
puertos nuevos o cambios al perfil actual. Reutiliza `swarm-app/seiya/ui.html`
sin cambiar CSS, distribución, colores o controles. Se conserva el diseño por
petición explícita del usuario. Estado de «preparada, no activada» en la línea
ya existente; historial separado `seiya.prepared.chats.v1`. No migra ni borra
conversaciones anteriores. La página estática no incluye claves y permite
introducirlas; API de datos/chat exige auth/Host/Origin antes de leer cuerpos.

`PreparedWeb::blocked` deniega por defecto Gemma E2B/Qwen9B; `with_host` es una
frontera de inyección para código confiable del host, no un permiso en JSON.
El host futuro debe acreditar autorización conjunta, lease, identidades y
recursos del par. **No hay host nativo positivo ni activación real** en W6.
La presencia de una instancia de librería o fixtures no acredita esos controles.

- `/api/chat`: frame cerrado, solo mensajes user/assistant textuales; herramientas
  `code_job_submit/list/status` fijadas por host. No tools del navegador, lector,
  rutas, comandos o generador de imágenes. Una acción por turno; sesión y UUID
  de envío nunca se envían al interlocutor. Acuse/estado desde registro real del
  broker, sin otra inferencia que invente porcentajes o tests.
- `/api/jobs`: sesión/list/status/cancel/artifact con capacidades existentes;
  rechaza submit directo del navegador. Cancelar, consultar y descargar siguen
  accesibles aun con admisión revocada. Idempotencia mantiene el mismo trabajo.
- Cuarentena de Jobs bloquea interlocutor; gate revalidado antes del dispatch
  del transporte, no solo al recibir HTTP. No `/v1` para eludir esta política.
  Revocar gate no interrumpe mágicamente una inferencia ya iniciada: el host
  propietario debe vigilar y confirmar cleanup antes de permitir activación.
- `/health` anuncia integración preparada e imágenes web `available:false`.
  El ensayo de PNG por CLI no registra un ejecutor de imágenes del broker.

7 pruebas Rust vía Router/HTTP en proceso con modelos falsos: auth/Origin/schema,
gate negativo, historial y CSS/estructura conservados, chat durante trabajo,
estado real/acuse, cancelación, idempotencia, descarga propia, args maliciosos y
acciones múltiples rechazadas, cuarentena y rollback. No navegador gráfico,
calidad de tool-use de Gemma o concurrencia real acreditados.

Rollback: omitir constructor; el router normal no incluye estos endpoints.
No se sustituyó la vista previa8010 ni la aplicación Python operativa.

Constructor de librería `app_with_jobs` exige clave y Job executor + Admission del
host. `Jobs::new` limita a1 auxiliar; `with_parallelism` admite1–4 y4 esperas.
`Blocked` deniega por defecto; `DemoAdmission` es exclusivo de fixtures, no prueba
RAM/ownership. `HttpDraft` solo permite HTTP127.0.0.1 con puerto explícito y ruta
fija `/v1/chat/completions`, sin redirects/proxy, sin cargar/descargar pesos o
ejecutar tools. Fallo incierto entra en cuarentena. El host todavía debe integrar
admisión/ownership/vigilancia reales del pool antes de inyectar un gate positivo.

`planned_parallelism` calcula capacidad **candidata** desde reservas disjuntas
del operador: host, interlocutor y cada auxiliar, limitada por capacidad del
runtime. Pruebas16/32GiB usan importes sintéticos. No lee RAM automáticamente,
no calcula presupuesto desde tamaño de pesos ni suma RSS/Metal. Tener32GiB no
garantiza2/3/4 trabajos. Ese cálculo no concede permisos ni inicia procesos.

Perfil de frontend ilustrativo:
[`jobs.example.json`](../contracts/openai-compat/v0.1.0/jobs.example.json).
Elegido por usuario: Gemma `gemma4:e2b-mlx`, auxiliar Qwen `qwen3.5:9b-mlx`.
El puerto49152 es un placeholder, no un broker operativo. Claves reales solo en
entorno: `SEIYA_API_KEY` del frontend y variable indicada en `jobs.api_key_env`
para su cliente interno. No copiar el ejemplo sobre config existente ni arrancar
un segundo coordinador sobre modelos ajenos. El constructor normal de CLI Rust
no activa jobs automáticamente.

Inventario observado para el Gemma elegido: tag presente,7,5GB de catálogo y
7,4GB mostrados en ps con contexto16384/runner mlx; show declara gemma4/5,2B/nvfp4
y thinking predeterminado true. Son metadatos, no inferencia o presupuesto
validado. No reutilizar los≈4GiB de la calibración del E4B anterior para este tag.

## Lifecycle y privacidad

- API interna `/internal/jobs/v1`, autenticación/Host/Origin, JSON cerrado≤16KiB,
  deadline de cuerpo3s,8 cuerpos simultáneos; nada de shell/endpoints por modelo.
- Sesión opaca por chat, UUID por job y request_id por envío. Repetir el mismo
  request_id/prompt devuelve el job existente; cambiar prompt con ese ID falla.
  Sin retry automático después de error ambiguo.
- Capacidad32 sesiones/64 jobs totales por proceso; prompt≤8192 bytes y resultado
  ≤65536; plazo≤180s incluida cola. TTL de capacidad1800s de inactividad, renovado
  por controles válidos; una página activa puede seguir consultando el job. No hay
  GC/persistencia en este incremento: registros acotados en memoria hasta cerrar
  proceso, incluso si la capacidad expiró. Capacidad llena falla sin sobrescribir.
- Sesiones/API keys no se incluyen en prompts o localStorage. Historial anterior
  no migra. Refrescar pierde el enlace al job y no lo reenvía; no restauración de
  trabajos tras reinicio. No prometer exactly-once entre sesiones/reinicios.
- `cancelling` no significa terminado. Resultados tardíos tras cancelación se
  descartan. Timeout/ejecución incierta→quarantined, pool cerrado y nuevas
  inferencias del perfil bloqueadas; status sigue disponible. Sin reset HTTP.
  Un guard de ownership retiene el slot/cierra pool también si el ejecutor entra
  en panic o se descarta el future, antes de volver a admitir otro trabajo.
- La eliminación del chat con trabajos activos solicita cancelación y se aplaza
  hasta confirmar fin. En cuarentena conserva el chat para revisión humana.
- El estado no acredita GPU descargada o contexto disponible. Recuperar requiere
  operador, comprobar propietario/trabajo en vuelo y recursos. No reiniciar a
  ciegas, borrar modelos o cambiar a remoto.

Rollback: quitar perfil jobs y volver al perfil anterior solo después de revisar
trabajos/recursos; detener vista previa propia sin tocar servicios compartidos.
No se guarda contenido en logs por defecto. No hay generador de imágenes web;
el ensayo local aislado se documenta en [imagen experimental](IMAGEN_LOCAL_EXPERIMENTAL.md).
