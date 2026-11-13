# Seiya 0.1.0 — verificación local, 2026-10-18

## Actualización Pegassus y web, 2026-10-19

- S3-03/S3-04: adaptador macOS explícito desde Child propio o collector actual,
  comprobación de parentesco/inicio antes/después y probes fijos/cotados. 16 tests
  sintéticos de ownership/ejecutor/pipes/deadlines; smoke nativo separado, helper/
  sleep propios + RSS/VM/swap/collector, sin modelo. Primer fallo count/bytes de
  proc_listchildpids corregido y cubierto por regresión, rerun OK.
  Runner simulado: nueve tests, ABBA×3, validación de trabajo/techo/identidad,
  artefactos nuevos/limitados, cancelación drenada y stop confirmado. No launcher
  real ni campaña de inferencia: integración S3-04b y medición S3-05 pendientes.
  **118 Rust + 86 Python OK**, dos manuales ignored por defecto y 1 smoke nativo
  explícito aparte; fmt/clippy/release-workspace/diff OK. Sin cambios operativos,
  instalación, modelo o proveedor. No baseline ni ahorro de memoria demostrado.

- S3-01/S3-02: `seiya-observe` implementa núcleo puro de snapshots y parsers
  macOS con proveedor/fixtures sintéticos, no probes reales. Diecisiete tests
  nuevos cubren identidad/tiempo/cotas/unidades/errores; sin historial ilimitado,
  falso cero, suma de métricas solapadas o eco de datos del proveedor. Footprint,
  MLX y Metal siguen unsupported. RSS no acredita ownership. 93 Rust + 86 Python
  OK, fmt/clippy/diff/build workspace aprobados. Adaptador/runner/campaña pendientes;
  no cambios a servicios, modelos, configuración o instalación del prototipo.

- S2-03/S2-04 y Sprint 02 cerrados. Cuatro tests TCP combinan cliente sintético,
  BoundServer/HttpTransport y backend Axum propio: presupuestos/options/JSON/SSE,
  IDs/argumentos/resultados/usage, errores y cuarentena sin retries ni DONE falso.
  No tools host ni modelo real nuevos. Baseline de memoria diseñada en
  `docs/BASELINE_MEMORIA.md` + plan JSON; seis tests offline comprueban invariantes,
  **no un recolector ni ahorro RAM**. Suite 76 Rust + 86 Python (72 UI/API + 8
  checker + 6 plan) OK, fmt/clippy/release/diff-check OK. Sin instalar/promocionar
  ni tocar servicios/modelos/config. Próximo incremento: recolector/runner.

- S2-02: política explícita Rust de opciones (ADR 0006). Store solo false/ausente,
  stream_options/include_usage validado, usage SSE opcional, campos raíz no
  soportados rechazados sin eco de datos privados y antes de generar. No se
  reenvían estas opciones al backend ni se promete persistencia. Cuatro tests
  nuevos con fixtures de extensión; golden Python/SSE default conservadas.
  **72 Rust + 80 Python** OK, fmt/clippy/release/diff-check aprobados. Mocks/router
  JSON/SSE, sin inferencia real nueva o cambio de servicios/config/proveedor.

- S2-01: Rust ya interpreta `max_completion_tokens`/`max_tokens`: enteros 1–4096,
  ambos iguales o error 400, mínimo con límite config enviado al backend. Cuatro
  tests nuevos con fixture de extensión Rust, sin cambiar golden ni prototipo
  Python. **68 Rust + 80 Python** (72 UI/API + 8 checker) OK, fmt/clippy/release/
  diff-check aprobados. Transporte simulado/router JSON, sin inferencia real nueva,
  cambios de servicios o proveedor. SSE/opciones restantes se verifican en tareas
  posteriores, no se marcan completas por este cambio.

- S1-07/Sprint 01 cerrado en alcance experimental, sin instalación/promoción.
  Guía `docs/OPERACION_RUST_EXPERIMENTAL.md`: arranque/parada, límites, diferencias
  y rollback. Ensayo sintético CLI Python + Rust discovery mantuvo health/modelos/UI
  y mismo PID/HTML Python tras parar Rust, cero llamadas backend; ambos procesos
  propios cerrados con 0. No es restauración de instalación o datos perdidos.
  Suite de cierre: 64 Rust + 80 Python (72 UI/API + 8 checker), fmt/clippy/release/
  diff-check OK. No inferencia real nueva, cambios de configuración, servicios o
  empaquetado. Sprint 02 pendiente: presupuestos/opciones antes de medir memoria.

- S1-06: **dos smoke tests reales Rust/OpenCode/Gemma aprobados**. Dos reads,
  edit y shell por ejecución, 3 tests OK desde OpenCode e independientemente,
  cinco SSE 200/DONE, cero errores o mensajes de compactación, sesión idle/succeeded.
  Perfil privado/instrucciones cortas, máximo prompt 1940 tokens; no prueba de
  agente build completo, sesiones largas o contexto 32K. Tiempos 14.743/15.084 s;
  RSS muestreado no es VRAM ni demuestra ahorro. Ver `docs/PRUEBA_RUST_OPENCODE.md`.
  Python/MLX operativos ya ausentes antes de la prueba; runtime propio autorizado
  por el usuario y cerrado al terminar, sin arrancar/restaurar servicios ausentes.
  OpenCode compartido intacto. Suite 64 Rust + 72 Python/UI + 8 checker aprobada,
  fmt/clippy/release/diff-check OK; sin instalación o promoción automática.

- S1-05: Rust admite SSE bufferizado (no tokens en vivo) con runtime explícito.
  **64 Rust y 72 Python/UI OK**, fmt/clippy/release/diff-check aprobados. Fixtures
  SSE, múltiples llamadas, escaping Unicode/newlines, errores JSON antes de SSE,
  guardas/cola y roundtrip TCP simulado. Smoke release autenticado contra backend
  sintético: dos completions SSE con historial tool intacto, DONE/EOF, SIGTERM 0.
  No se ejecutaron herramientas ni se conectó MLX/OpenCode. Servicios de prueba
  propios cerrados. Prueba real pendiente S1-06; Python operativo sin cambios.

- S1-04: chat JSON Rust opt-in con configuración separada, guardas/payloads y
  cola limitada; sin cambios al servicio Python. **57 Rust y 72 Python/UI OK**.
  Pruebas de concurrencia, lector lento/capacidad, errores antes del backend,
  cierre con consumidor perdido y runtime incierto. Smoke de release con backend
  sintético y SIGTERM durante trabajo activo completó una respuesta y salió con 0.
  Sin MLX/modelos reales, SSE o prueba OpenCode nueva. Stream sigue 501; default
  sin runtime sigue discovery. No se dejó el servidor experimental activo.

- S1-03: adaptador interno Rust + Reqwest local, 15 tests nuevos. **40 Rust y
  72 Python/UI aprobados**. Fixtures de chat/tools/errores con transporte simulado
  y HTTP real contra backends sintéticos en loopback; sin modelos reales. Pruebas
  de respuesta >8 MiB, redirects rechazados, timeouts y exclusión retenida tras
  abandono del consumidor. Tras fallo de transporte el adaptador queda bloqueado
  por estado desconocido del runtime. Chat HTTP Rust sigue 501; no se instaló ni
  activó frente al servidor Python/OpenCode. ADR 0002 y sprint documentan límites.

- S1-02: binario Rust `seiya-server` release, todavía sin inferencia, probado en
  listener loopback efímero. Health/models 200, auth ausente 401, origen externo
  403 y chat 501; cierre de proceso propio por SIGTERM con exit 0 y puerto cerrado.
  Total Rust: 25 tests (16 nuevos de servidor, 9 previos). Las fixtures HTTP de
  chat siguen probándose solo contra Python. No se cambió el servidor operativo,
  los modelos ni la configuración de OpenCode. No se instaló el binario Rust.

- Sprint S1-01: 48 tests nuevos de compatibilidad contra HTTP Python efímero y
  runtime simulado. Fixtures en `contracts/openai-compat/v0.1.0/`, con respuestas
  estrictas, SSE/tools, límites, auth/Host/Origin y cola. Total actual: **72 tests
  Python/UI y 9 Rust** (7 unit + 2 de lectura/consistencia de fixtures). Clippy y
  formato aprobados. No cambia el servidor activo ni prueba todavía un servidor
  Rust o una nueva sesión real de OpenCode. Estado y evidencia en el sprint.

- Limpieza autorizada del legado y contrato multiplataforma: documentación en
  `docs/CONTRATO_SEIYA.md`, protocolo/entrega y JSON en `contracts/v1/`. La
  copia anterior a la limpieza está identificada en `docs/LIMPIEZA.md`.
- Workspace Rust con `seiya-contracts`: **7 tests**, `cargo clippy -D warnings`
  y build release en macOS con Rust 1.93.1. Los tests verifican parsing,
  límites, selección básica de tools y consistencia de referencias JSON; no
  sustituyen validación JSON Schema completa ni un broker de seguridad.
- **24 tests Python/UI aprobados** tras la limpieza; los instaladores incluyen
  los documentos y schemas. El servicio Python y `seiya/pegassus` siguen activos.
  No se ha implementado servidor Rust, FFI, app iOS, visión o control de Mail.
  Los nuevos jobs CI se han añadido pero no se han ejecutado en GitHub.

- Corrección de contexto aplicada: perfil OpenCode 32768/2048 y
  `body.swarm_mode: fast`, tanto para tools como para resúmenes/títulos.
  `max_output_tokens` de la configuración local pasó a 2048, con backup.
  Se reinició solo Seiya desde las fuentes; la cola limitada ya está activa.
  Dos peticiones HTTP simultáneas recibieron 200 (0,26 y 0,41 s), sin 429.
- OpenCode Build estándar, con permisos acotados en carpeta temporal, ejecutó
  unittest: 3/3 OK y cero compactaciones. Un segundo ensayo adjuntó 79.931 bytes
  sintéticos: hubo una compactación con 18.935 tokens de entrada reales, sin
  repetición en bucle. Ejecutó unittest 3/3 OK, pero la primera salida omitió los
  marcadores pedidos. En un turno posterior recuperó correctamente ambos
  marcadores del principio y final y el resultado de los tests. No se afirma
  cumplimiento perfecto ni contexto máximo de 32K validado.
  Logs locales: `~/Library/Logs/seiya-context-regression.jsonl`,
  `seiya-long-context-regression.jsonl` y `seiya-long-context-followup.jsonl`.
  Sesión larga: `ses_eacc97314ffe0aIuCHKeB2qaTS`.
- No se desactivó la compactación, ni se borró o reinició el servicio compartido
  de OpenCode. El runtime MLX permaneció activo; swap tras la prueba ~3,49 GB,
  una cifra acumulada que por sí sola no determina presión activa de memoria.

- Corrección preparada para peticiones concurrentes de OpenCode: una generación
  activa y hasta dos peticiones en espera durante 120 segundos, compartido por
  API y web. No se aumenta la concurrencia MLX. Pruebas de espera, límite de cola
  y liberación tras timeout: **23 tests aprobados**. El servidor que estaba en uso
  al diagnosticar el aviso no se reinició durante la tarea activa del usuario.

- Cambio posterior: búsqueda disponible automáticamente en `/api/chat` por
  defecto, sin casilla en la UI. El modelo decide cuándo llamar a `web_search`.
  La API conserva `web_search: false` para clientes que quieran desactivarla.
  Se añade la fecha local del servidor y orientación para refinar búsquedas de
  noticias sin inventar titulares ni pedir categorías innecesarias. El chat web
  usa un worker; no se ofrece un selector de consenso que no se aplicaría con tools.
  Total actual: **21 tests aprobados**. Las referencias a casillas siguientes
  describen versiones anteriores.

- Historial web local con UUID por conversación, selector, borrado confirmado,
  borradores y fuentes persistentes. Test JavaScript en DOM simulado ejecutado
  desde unittest: recarga, recuperación de mensaje enviado durante generación,
  cambio de chat, eliminación, no persistencia de credenciales y errores de
  almacenamiento. No equivale a una prueba visual de navegador real.
  Total actual: **20 tests aprobados**.

- Corrección de capacidades: se inyecta un mensaje de sistema en ambos modos
  del chat web. Sin búsqueda explica la casilla; con búsqueda reconoce
  `web_search`. Se probó la pregunta exacta «hola puedes buscar en internet ?»
  en ambos estados, incluyendo una negativa antigua en el historial: respondió
  correctamente en español. La petición posterior de buscar Python ejecutó
  realmente la herramienta y devolvió la URL oficial. **19 tests aprobados**.

- El proveedor sigue siendo `seiya`; el ID público del modelo pasa a `pegassus`
  (`seiya/pegassus` en OpenCode). Las menciones a `seiya/seiya` de las pruebas
  anteriores son históricas, no la configuración actual.
- Interfaz rediseñada: conversación, compositor inferior, nuevo chat, selector
  rápido/consenso y búsqueda opt-in. Renderizado como texto y enlaces validados;
  sin dependencias CDN ni HTML procedente del modelo. No se ha hecho una prueba
  automatizada visual de navegador.
- Prueba real del nuevo endpoint `/api/chat` con Gemma/MLX: llamó a
  `web_search` con `Python official website`, recibió 5 resultados de DuckDuckGo
  y respondió con `https://www.python.org/` en 3,459 s. Es búsqueda de títulos y
  fragmentos, no navegación completa. El buscador puede fallar por anti-bot.
- Nuevas pruebas cubren búsqueda simulada y fallo de red, límites de llamadas,
  rechazo de mensajes forjados, parsing de enlaces y validación HTTP con auth.
  Las pruebas históricas de código en OpenCode no implican consenso con tools.
- Resultado actual: **18 tests aprobados**; sintaxis JavaScript validada con
  `node --check`. Payload macOS y SHA256 comparados con las fuentes finales.
- Se repitió OpenCode con **`seiya/pegassus`**: ejecutó `read calculator.py` y
  `shell python3 -m unittest -v`, 3/3 OK. Logs:
  `~/Library/Logs/seiya-pegassus-opencode.jsonl`.
- La actualización instalada no terminó por timeout de autorización de macOS;
  no se considera aplicada. Para pruebas se detuvo el servidor instalado viejo
  y se arrancó el código del proyecto en el puerto 8010. La web nueva y la
  búsqueda se verificaron también en ese puerto; MLX sigue en 8081.
  El paquete actualizado está en `swarm-app/dist/Seiya-0.1.0.pkg`.
  Abrir la app instalada antigua tras detener este servidor puede recuperar
  la versión anterior hasta que se complete la actualización con permisos.

## Verificado

- 12 pruebas automáticas: configuración local, IDs únicos, un único endpoint,
  consenso, votos inválidos, empate, respuestas equivalentes, tools, replay de
  resultados, SSE, autenticación, Host/Origin, concurrencia y fallo de descarga
  de memoria de Ollama. También estructura Debian y CLI del payload extraído.
- Se generaron `.deb` y `.pkg` en macOS. Se inspeccionaron sus payloads sin
  instalar. El contenido Python del `.pkg` se comparó con las fuentes.
  `pkgbuild` retornó éxito pero emitió cuatro avisos `write: Permission denied`;
  se verificaron todas las fuentes finales y el lanzador extraídos del paquete.
  Esto no sustituye una instalación limpia ni la validación de Gatekeeper.
- Integración real: MLX-LM 0.32.2 / MLX 0.32.4, Apple M4 16 GB. Un único
  runtime MLX con modelos cargados bajo demanda. Sin modificar OpenCode.
- `/v1/models` anuncia solo **seiya**.
- Suite de herramientas real a través de Seiya, worker Gemma E4B: primer
  intento 17/18, segundo 16/18. En el segundo, todas las llamadas se ejecutaron
  y tuvieron resultados reales; dos respuestas finales devolvieron un objeto
  JSON textual en vez del contenido exacto solicitado. Se conservan esos fallos.
- Consenso inicial con votos como texto: falló correctamente con HTTP 502,
  porque dos modelos devolvieron bloques Markdown en vez de JSON estricto.
- Consenso corregido con herramienta `cast_vote`: tres propuestas de `4` para
  `2+2`, dos votos estructurados válidos (Qwen y Ministral), abstención/error
  de Gemma, mayoría absoluta 2/3. Resultado `4` en 31.396 segundos.

Resultados completos:
`~/colibri-models/tool-test-results/seiya-integration-structured/`.
El intento inicial se conserva en `seiya-integration/`.

## Instalación real en macOS y OpenCode (2026-10-19)

- Instalado con el instalador de macOS y autorización del usuario. La lectura
  directa desde iCloud falló; se copió el paquete idéntico al temporal aprobado
  y la instalación pasó. Se comprobó versión 0.1.0 y coincidencia de fuentes.
- El lanzador inicial basado en AppleScript quedó esperando autorización
  para controlar Terminal. Se sustituyó por `open -a Terminal Seiya.command`,
  se reconstruyó y reinstaló; la app arrancó API y web con HTTP 200.
- Suite instalada: 16/18. Las dos respuestas fallidas envolvieron en JSON
  textual el contenido leído; los tool calls, sus resultados y efectos fueron reales.
- OpenCode **v2.0.32**, `run --standalone --model seiya/seiya`, agente primario
  de prueba con 8 pasos máximos y permisos acotados, en un proyecto temporal:
  `read calculator.py`, `read test_calculator.py`, `edit calculator.py`,
  `shell python3 -m unittest -v`. Corrigió suma/resta y pasaron las 3 pruebas.
  Se volvió a ejecutar unittest de forma independiente: 3/3 OK.
- Se añadió `providers.seiya` a la configuración global del usuario, con copia
  de seguridad. Se comprobó `opencode models --standalone`: `seiya/seiya`.
  Todos los valores previos, incluido el modelo por defecto, se conservaron.
- Logs privados locales: `~/Library/Logs/seiya-opencode-smoke.jsonl` y
  `~/colibri-models/tool-test-results/seiya-installed-mac/`.

## No verificado / no implementado

- Instalación real en Debian/Ubuntu, ni en un Mac limpio. El `.deb` se ha
  construido y extraído, no instalado con apt/dpkg en Linux.
- Firma Developer ID, notarización, distribución pública macOS/Gatekeeper.
- Integración amplia con el agente Build estándar, todos los plugins,
  permisos y esquemas posibles de herramientas de OpenCode. El smoke test
  real descrito arriba no establece fiabilidad general.
- Agente autónomo de edición/ejecución. Las herramientas del cliente se
  delegan al worker, no se votan ni ejecutan desde Seiya.
- Mejora de calidad frente a un solo modelo. El consenso aritmético solo
  prueba el protocolo; no demuestra competencia en programación compleja.
- Rondas automáticas de reparación, ramas aisladas de cambios, sandbox o
  ejecución automática de pruebas generadas por modelos.
- Streaming token a token, cancelación de generación, contexto largo y
  estabilidad bajo carga sostenida. SSE es bufferizado.

Los servidores temporales iniciales se detuvieron. En la prueba instalada
del 19 de octubre, Seiya y MLX se dejaron activos para uso del usuario.
No hay servicios de inicio añadidos por Seiya, ni pesos en los instalables.

También se añadieron builds CI para macOS y Linux en
`.github/workflows/seiya-packages.yml`, incluida instalación/desinstalación del
`.deb` en un runner Linux. Ese workflow **no se ha ejecutado** desde esta sesión.
Los 12 tests se ejecutaron localmente con Python 3.11 y 3.14.
