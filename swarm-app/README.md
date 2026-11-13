# Seiya — prototipo 0.1.0

Esta carpeta conserva la aplicación actual mientras se prepara el núcleo Rust.
El contrato futuro está en `docs/CONTRATO_SEIYA.md` desde la raíz del repositorio;
los instaladores lo incluyen en su directorio de documentación `docs/` junto
al protocolo y los JSON de `contracts/v1/`. Esos contratos son draft, no nuevas
rutas ya implementadas por este servidor Python.

Coordinador Python local **Seiya**, con un único modelo visible: **Pegassus**
(`seiya/pegassus` en OpenCode).
Instalables: **Debian `.deb`** y **macOS `.pkg`** (incluye `Seiya.app`,
que abre Terminal y la interfaz web). No se incluyen modelos ni runtimes.
No instala tareas de inicio ni modifica la configuración de OpenCode.

## Requisitos

- Python 3.10+; el coordinador usa solo la biblioteca estándar.
- **macOS Apple Silicon:** MLX-LM, instalado separadamente en un entorno virtual.
- **Linux:** Ollama instalado separadamente, con modelos compatibles descargados.
- En un Mac de 16 GB: un único runtime, un modelo por turno. No iniciar tres
  servidores independientes. El contexto y los cachés también consumen RAM.

Los modelos propuestos son Gemma 4 E4B, Qwen3.5-9B y Ministral 3 8B Instruct,
cuantizados a 4 bits. Ministral pasó nuestras tareas de código, pero falló
algunas respuestas exactas de herramientas y JSON. Sus votos inválidos se
descartan, **nunca se reparan silenciosamente**. Las pruebas previas de modelos
no demuestran la calidad del enjambre en proyectos reales.

## Desarrollo / uso desde el repositorio

```bash
cd swarm-app
python3 -m seiya init --backend mlx
python3 -m seiya serve --open
```

O bien instalar el paquete Python en un entorno virtual:

```bash
python3 -m venv .venv
.venv/bin/python -m pip install .
.venv/bin/seiya init --backend mlx
.venv/bin/seiya serve
```

Configuración creada por `init` (no sobrescribe un archivo existente):

- macOS: `~/Library/Application Support/Seiya/config.json`
- Linux: `~/.config/seiya/config.json` (respeta `XDG_CONFIG_HOME`)

`seiya init --backend ollama` genera nombres de Ollama que deben comprobarse
en el catálogo local. **No descarga modelos**; ajustar IDs con `ollama list`.
`--config /ruta/config.json` permite perfiles separados para `serve` y `doctor`.

### Runtime MLX para los modelos ya descargados en este Mac

Ejecutar en otra terminal, **sin `--model`**: MLX carga bajo demanda el modelo
solicitado y sustituye el anterior. El coordinador solo envía una solicitud
a la vez. Usar una versión moderna que libere el pool MLX al cambiar de modelo
(probado con MLX-LM 0.32.2). Un solo caché de prompts limita la retención.

```bash
~/colibri-models/mlx-tests-venv/bin/mlx_lm.server \
  --host 127.0.0.1 --port 8081 \
  --decode-concurrency 1 --prompt-concurrency 1 \
  --prefill-step-size 256 --prompt-cache-size 1 \
  --prompt-cache-bytes 128MB --max-tokens 768 \
  --chat-template-args '{"enable_thinking":false}'
```

Las rutas `~/colibri-models/mlx-gemma4-e4b`, `mlx-qwen35-9b` y
`mlx-ministral3-8b` se escriben expandidas en el ejemplo macOS. En otro equipo
deben ajustarse. No habilitar `trust_remote_code` para estas pruebas.

### Runtime Linux / Ollama

Iniciar Ollama según su instalación, configurar IDs existentes y asegurarse
de que su contexto sea moderado (p. ej., 2.048–4.096 tokens). Seiya descarga
de memoria cada modelo con `keep_alive: 0` después de cada llamada. Si falla
esa descarga, aborta: no continúa cargando modelos sobre memoria ocupada.

## API / interfaz web

### Trabajos de programación experimentales

Perfil jobs opcional: broker Rust y panel de trabajos independientes del chat,
con estado, cancelación y descarga de **borradores no ejecutados**. Vista previa
simulada disponible mediante `python3 -m seiya.preview`; no carga Gemma/Qwen ni
cambia la configuración principal. Capacidad configurable por host y reservas,
no por una promesa de que32GB bastan. Integración de admisión real pendiente.
Operación, ejemplo, límites y rollback:
[`TRABAJOS_EN_SEGUNDO_PLANO.md`](../docs/TRABAJOS_EN_SEGUNDO_PLANO.md).

- Web: `http://127.0.0.1:8010/`
- OpenAI base URL: `http://127.0.0.1:8010/v1`
- Proveedor: **Seiya** · modelo anunciado por `/v1/models`: **`pegassus`**
- Salud del coordinador: `/health` (no garantiza que el runtime/modelo cargue)
- `seiya doctor` valida la configuración y comprueba el endpoint, sin cargar pesos.

```bash
curl http://127.0.0.1:8010/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"pegassus","messages":[{"role":"user","content":"¿Cuánto es 2+2?"}]}'
```

Para herramientas, el cliente suministra las definiciones `tools`, ejecuta
solo las llamadas autorizadas y envía mensajes `tool` con el `tool_call_id`.
**La API `/v1` no ejecuta las herramientas suministradas por el cliente.** Reenvía
las llamadas estructuradas del worker designado; el cliente debe validar el
esquema de argumentos y aplicar permisos. No se interpreta JSON escrito como
prosa como una llamada ejecutable.

La interfaz web ofrece un chat de estilo conversacional con mensajes, nuevo
chat e historial local, con **búsqueda automática** disponible para el modelo.
No hay casilla que activar. Usa `/api/chat`,
un endpoint separado con la misma protección de Host, Origin y API key.
El modelo decide si necesita buscar o sigue la petición del usuario de hacerlo.
Si no llama a la herramienta no sale a Internet. Al buscar, solo la consulta generada por
el modelo se envía a DuckDuckGo mediante `/usr/bin/curl` (incluido en macOS;
instalar `curl` en Linux). No se envía automáticamente todo el historial,
pero la consulta puede contener datos de la conversación: no introducir datos
privados que no deban enviarse al buscador. Máximo 3 búsquedas, 5 resultados por consulta, 20 segundos
de red por búsqueda. Sin URLs arbitrarias, redirecciones, archivos o comandos
elegidos por el modelo. Solo se leen títulos y fragmentos; no páginas completas.
Los enlaces se muestran por separado y el contenido se trata como texto,
no HTML ejecutable. Las conversaciones tienen UUID y se guardan con mensajes,
fuentes y borradores en `localStorage` de este navegador, sin cifrar. Se restauran
al refrescar; el selector superior permite reabrirlas o eliminar el chat actual.
No se sincronizan con otros navegadores, dispositivos ni entre `localhost` y
`127.0.0.1`. Borrar los datos del sitio borra el historial; no es una copia de
seguridad. Evitar editar el mismo historial en varias pestañas simultáneamente.
En escritorio las conversaciones aparecen en la barra izquierda, con el chat
activo resaltado; en móvil se conserva el selector superior. La lista tiene
scroll propio y se bloquea durante una respuesta. El cambio de presentación
no migra ni borra el historial existente del navegador.
La API key solo vive en la pestaña y nunca se guarda con las conversaciones.

### Lector experimental opt-in (S4-07)

El modo habitual sigue usando snippets. Un perfil separado `web_reader` permite
el flujo búsqueda→lectura→síntesis con fuentes por ID, estado y cancel explícito;
requiere fast/un worker, endpoint lector Rust de librería y claves frontend/lector
desde entorno. No inicia servicios o modelos. El lector Rust no está montado por
la CLI habitual: no basta usar su puerto de discovery como si ofreciera lectura.
[Guía de configuración, pruebas y rollback](../docs/LECTOR_WEB_EXPERIMENTAL.md).

Pruebas de S4-07 con modelo/búsqueda/páginas sintéticos y UI Node; **sin calidad
Gemma, Safari o red pública validados**. Presupuesto de contexto estimado, no
tokenizado, con rechazo explícito si no cabe. Cancel no libera una inferencia
en vuelo antes de finalizar;202 no acredita limpieza del lector. No cambia `/v1`,
ni instala/activa el perfil o migra historial. No guardes claves o tokens de run
en la conversación, JSON de configuración o localStorage.
La línea de estado y «Revisar conexión» consultan `/health` autenticado, sin
reenviar mensajes ni comprobar modelos. El perfil Python normal indica runtime
no comprobado y memoria sin supervisión integrada: coordinador activo no significa
modelo residente. Un fallo conserva el borrador y no lanza retries.
Si se refresca durante una petición, se conserva el mensaje enviado, pero no
se recupera automáticamente la generación en curso. Las conversaciones perdidas
antes de incorporar esta función no se pueden restaurar.

El chat web automático utiliza un worker con herramientas; el modo consenso
sigue disponible mediante la API para consultas sin herramientas. La búsqueda puede fallar
por cambios del buscador o medidas anti-bot, y lo indica en lugar de fingir
éxito. Las fuentes externas pueden contener datos incorrectos o instrucciones
maliciosas; no son prueba de veracidad. La API OpenCode conserva su protocolo
de herramientas externo, sin activar estas herramientas web automáticamente.

La API implementa un **subconjunto** de chat completions, no toda la API OpenAI:
texto, tools y SSE **bufferizado** (se emite al terminar, no tokens en vivo).
No admite imágenes, `tool_choice`, JSON-schema/`response_format`, logprobs ni
control de llamadas paralelas. Se rechazan estas opciones explícitamente.
Se probó en OpenCode V2.0.32 con un agente de prueba acotado: leyó archivos,
editó una función y ejecutó unittest con sus herramientas reales. Esto no
garantiza tareas largas ni todos los agentes/plugins. No se cambia el proveedor
activo de la sesión donde se instala.

### Puente Rust de inferencia/estado · opt-in experimental

No se activa por actualizar fuentes. Copiar y ajustar **ambos** ejemplos de
[`runtime-bridge.json`](../contracts/openai-compat/v0.1.0/runtime-bridge.json) en
configuraciones nuevas; no usar el identificador synthetic con un runtime real.
Requiere modelo/backend coincidentes y un solo worker MLX/fast; no consenso ni
Ollama. `runtime_bridge.base_url` debe ser HTTP127.0.0.1 numérico con puerto y
`/v1`, distinto del frontend. Clave del puente solo por variable de entorno
`api_key_env`, no en JSON. La clave del frontend es independiente.

El Rust previamente configurado debe anunciar handshake versión1/hex-utf8-v1 y
runtime_status. El header de binding verifica el modelo backend antes del envío;
no prueba hashes/pesos ni autentica. Sin proxies, redirects o fallback al runtime
directo. Tools/historial pasan intactos y `/v1` nunca los ejecuta. La búsqueda de
fragmentos existente de `/api/chat` sigue aparte; este puente no lee páginas.

Health copia solo metadata versionada: endpoint observado, admisión, residencia
desconocida y guardia stop-only si existe. La CLI Rust **no conecta el supervisor
nativo**: memory_admission sigue not_integrated. No presentar este perfil como
protección de memoria MLX. Errores de disponibilidad/incertidumbre/presión tienen
código y no reintentan. Un enlace ambiguo bloquea nuevas generaciones del cliente;
consultar health no lo rearma. Recuperación solo por operador tras comprobar
runtime, generación pendiente, exclusión y recursos; no basta refrescar la página.

Rollback: volver a la copia anterior de configuración sin runtime_bridge, solo
tras confirmar que no hay generación o propietario pendiente; ningún modelo,
historial o proveedor se borra/migra. No arrancar dos coordinadores competidores.
Decisión y límites: [ADR0026](../docs/decisions/0026-puente-http-rust-web-opt-in.md).

Prueba conjunta separada, sin modelos: construir seiya-server con cargo build
locked e invocar `python3 swarm-app/tests/bridge_smoke.py /ruta/absoluta/seiya-server
/directorio/nuevo/absoluto` desde la raíz. Usa solo listeners efímeros propios y
recoge su Popen original. Prueba texto/tools, binding y desconexión incierta; no
ejecuta herramientas ni consulta Internet. No valida Gemma/Safari/sesiones largas.

### Seleccionar Seiya en OpenCode V2

Incorporar la entrada `providers.seiya` de `opencode-provider.json` a la
configuración de OpenCode, conservando las demás entradas y sin reemplazar
el archivo entero. El paquete deja el ejemplo también en
`/usr/local/share/doc/seiya/` (macOS) o `/usr/share/doc/seiya/` (Linux).
Si `SEIYA_API_KEY` está activada en el servidor, sustituir la clave local
de ejemplo por una referencia de entorno, no por un secreto en el repositorio.

Con MLX y Seiya en marcha, usar `/models` y seleccionar **Pegassus** del proveedor
**Seiya**, cuyo ID completo es **`seiya/pegassus`**. También se puede ejecutar:

```bash
opencode run --model seiya/pegassus "Revisa este archivo"
```

El perfil anuncia contexto de 32.768 y salida de 2.048 tokens. Envía
`swarm_mode: "fast"` también en peticiones auxiliares sin tools (resúmenes,
títulos), evitando que activen seis llamadas de consenso y cambios de pesos.
Para una instalación existente, comprobar que `max_output_tokens` en la
configuración Seiya sea 2048 y reiniciar solo Seiya para aplicar ese cambio.
El modelo de OpenCode por defecto no se sustituye. La compactación automática
permanece habilitada; subir el límite no elimina la necesidad de resumir.
Se ha ejercitado una petición de 18.935 tokens reales, no el máximo de 32K
ni una sesión de programación prolongada. Vigilar memoria, swap y latencia.

La prueba de integración no sustituyó el modelo predeterminado del usuario.
Con definiciones `tools`, esta versión usa el worker designado (Gemma), no
consenso entre los tres para cada acción. El consenso de tres participantes
funciona en solicitudes de texto sin herramientas; no es todavía un agente
cooperativo completo de ejecución/revisión dentro de OpenCode.

Opcional: `SEIYA_API_KEY=... seiya serve`. El servidor solo escucha en loopback,
restringe Host/Origin y no permite CORS abierto. Sin clave, otras aplicaciones
locales pueden acceder; con clave deben usar `Authorization: Bearer ...`.
La UI no persiste la clave. Los endpoints de runtime deben protegerse también;
por ejemplo, la API MLX permite cargar rutas locales y no se debe exponer a la red.

## Qué hace realmente el consenso

1. Pide propuestas independientes a todos los modelos, sin compartir borradores.
2. Cada modelo evalúa respuestas anónimas, rotando el orden, y llama a la
   herramienta interna `cast_vote` con argumentos estructurados. Esa herramienta
   solo registra una elección; no ejecuta código ni acciones de sistema.
3. Solo devuelve una propuesta existente con mayoría absoluta. Votos inválidos,
   empates o fallos no producen un consenso inventado: HTTP 502 explicativo.
4. `swarm_mode: "fast"` omite el consenso y consulta al worker configurado.
5. En solicitudes con herramientas o historial de tools, el worker mantiene
   el protocolo; no se votan ni ejecutan acciones entre varios agentes.

El resultado incluye `swarm` con modelo seleccionado, votos y tiempos.
**No ejecuta pruebas ni combina parches**: `tests_executed` es siempre false
para consenso. Planificación → ejecución de pruebas → revisión con aislamiento
de proyectos es una fase posterior, no se vende como implementada.

No se suman parámetros ni se garantiza una mejora de calidad. Una sola
petición de consenso produce 3 propuestas + 3 revisiones con los modelos
por turnos, por lo que puede ser bastante más lenta que el modo rápido.
No hay cancelación del backend cuando se desconecta el cliente; una petición
activa termina o agota su timeout por llamada. Se ejecuta una petición a la vez;
hasta otras dos esperan un máximo de 120 segundos sin cargar modelos en paralelo.
La cola llena o el timeout de espera producen 429. No hay cola ilimitada ni
cancelación automática del backend al abandonar el cliente.
Máximo 256 KiB por request, 100 mensajes, 64 herramientas.

## Construir instalables

Desde `swarm-app`, sin instalar ni pedir privilegios:

```bash
python3 packaging/build.py deb --implementation python   # Linux o macOS
python3 packaging/build.py mac --implementation python   # requiere macOS / pkgbuild
python3 packaging/build.py all --implementation python  # ambos, desde macOS
```

Resultados:

- `dist/seiya_0.1.0_all.deb` — depende de `python3 (>= 3.10)`.
- `dist/Seiya-0.1.0.pkg` — instala el comando y `/Applications/Seiya.app`.

Linux: `sudo apt install ./dist/seiya_0.1.0_all.deb`, luego `seiya init --backend ollama`.
macOS: abrir el `.pkg`, luego Seiya desde Aplicaciones o `seiya init; seiya serve`.
Si falta Python compatible en macOS, el lanzador muestra el requisito; no
instala Homebrew, Python ni modelos por su cuenta. `SEIYA_PYTHON` permite
seleccionar un ejecutable. Los instalables no contienen scripts postinstall.

**Son paquetes locales sin firma ni notarización**, no una distribución pública
lista para Gatekeeper. La compilación `.deb` es multiplataforma, pero se debe
probar su instalación en Debian/Ubuntu antes de distribuirla. Se requiere
certificado Developer ID para firmar/notarizar la distribución macOS.

Desinstalar Linux: `sudo apt remove seiya`. En macOS, retirar `Seiya.app`,
`/usr/local/bin/seiya`, `/usr/local/lib/seiya` y `/usr/local/share/doc/seiya`
después de parar el servidor. Los pesos/configuración del usuario se conservan.

## Verificación

```bash
python3 -m unittest discover -s tests -v
```

Las pruebas usan backends locales simulados; no descargan ni arrancan modelos.
La prueba real del runtime es complementaria y debe registrarse por separado.
