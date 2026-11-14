# Modelos de texto intercambiables · backend local portable

Implementación experimental: verificar el SHA del paquete frente al snapshot
actual mediante BUILD-INFO.json y el bloque Descargas del README.
Rust conecta motores externos ya preparados con API OpenAI local, por ejemplo
llama.cpp. **No incluye, instala, inicia, descarga ni detiene esos motores/pesos.**
La implementación comparte backend entre Windows/Linux/macOS; pruebas actuales
son de protocolo HTTP y recursos/filesystem simulados en macOS, no validación
real de los tres sistemas, GPU, todos los modelos o sesiones largas.

## Qué permanece igual

- Interfaz en **HTML/CSS/JavaScript**, permanentemente conservada. Los seis assets
  tienen hashes congelados; no se ha portado o sustituido el frontend.
- Nombre público Pegassus/`pegassus`; el binding real pertenece al catálogo.
- Host web127.0.0.1:8015, autenticación y revisión/confirmación antes de iniciar.
- Herramientas de trabajadores: borradores de texto, no shell, proyectos, tests,
  navegación ni permisos de OpenCode. El pipeline de imágenes macOS anterior
  sigue separado; ahora existe imagen HTTP opt-in con PNG nativo macOS y
  herramienta conversacional. Visión/audio no disponibles. Ver
  [imágenes desde chat](IMAGENES_DESDE_CHAT.md).

## Catálogo y asignaciones

Contrato y [ejemplo no ejecutable](../contracts/local-models/v1/catalog.example.json):
[local-models/v1](../contracts/local-models/v1/README.md). Registrar modelos con ID
local, binding exacto publicado por el motor, puerto y límites. No hay marcas
obligatorias. `supports_tools` es una declaración que debe probarse por modelo;
no convierte cualquier plantilla/tokenizador en compatible.

`local-models.json` se guarda por separado de los perfiles anteriores:

| Sistema | Carpeta fija |
|---|---|
| Windows | `%LOCALAPPDATA%\Seiya\` |
| Linux | `~/.local/state/Seiya/` |
| macOS | `~/Library/Application Support/Seiya/` |

El paquete nuevo incluye `local-models.example.json` y `LOCAL_MODELS.md`. Copiar
el ejemplo a una configuración propia **sin sobrescribir una existente**, editar
bindings/puertos y preparar los motores por separado. Los bindings ficticios no
sirven para generar. Unix requiere directorio propio privado y archivo modo600;
no se cambian automáticamente permisos existentes. Windows usa permisos heredados
de LOCALAPPDATA, sin acreditar una auditoría de ACL/owner equivalente.

Validación **offline**:

```sh
seiya-rust --check-catalog /ruta/absoluta/local-models.json
seiya-rust --launch --catalog /ruta/absoluta/local-models.json
```

En Windows, usar los equivalentes con el ejecutable nuevo:

```powershell
.\seiya-desktop.exe --check-catalog "$env:LOCALAPPDATA\Seiya\local-models.json"
.\seiya-desktop.exe --launch
```

Sin `--catalog`, se usa el catálogo de la carpeta fija si existe. Windows/Linux
sin él abren el asistente con instrucciones, no un modelo ficticio. macOS sin él
mantiene el adaptador MLX pinned anterior. Un catálogo inválido se muestra como
error, nunca se oculta cambiando a otro motor/proveedor.

## Usar el asistente sin cambiar los assets

1. Detectar configuración: el backend muestra una advertencia de **HTTP local**.
2. Revisar la ruta Ollama preconfigurada. La advertencia indica encontrado,
   no encontrado en rutas habituales o ruta guardada inválida/inaccesible.
   Editarla manualmente en Configuración; no se lanza ese binario. El almacén
   se conserva como metadato sin explorarlo; imágenes quedan vacías.
3. Elegir cerebro, trabajadores e imagen con sus IDs registrados. Imagen vacía
   salvo que exista image_models compatible; activar image_chat exige tools.
4. Revisar los endpoints/bindings y efectos. Guardar persiste roles y rutas en
   `local-model-roles.json`, mediante revisión CAS; no altera el catálogo ni los
   perfiles macOS anteriores.
5. Confirmar inicio y abrir chat. Abrir/guardar/iniciar host **no hace inferencia**.
   Solo el envío de un mensaje o un borrador solicitado puede generar.
6. Para cambiar roles, detener y esperar cierre confirmado; revisar de nuevo.

### Detección inicial de Ollama

Se comprueban archivos en rutas habituales al obtener la configuración:

| Sistema | Candidatos, en orden |
|---|---|
| macOS | `/Applications/Ollama.app/Contents/Resources/ollama`, `/usr/local/bin/ollama`, `/opt/homebrew/bin/ollama`, `/Applications/Ollama.app/Contents/MacOS/ollama` |
| Linux | `/usr/local/bin/ollama`, `/usr/bin/ollama`, `~/.local/bin/ollama` |
| Windows | `%LOCALAPPDATA%\Programs\Ollama\ollama.exe`, `%ProgramFiles%\Ollama\ollama.exe` |

Si no encuentra ninguno, deja el primer candidato y avisa; no afirma que Ollama
no esté instalado. El override inválido se conserva sin fallback silencioso.
Borrar el campo y guardar restaura autodetección. Encontrado solo acredita un
archivo legible y bits ejecutables en Unix, no autenticidad ni compatibilidad.
No hay arranque, descarga, consulta11434 ni inspección de modelos. En el adaptador
portable se guardan rutas aun sin catálogo, pero iniciar sigue bloqueado. macOS
sin catálogo conserva la revisión estricta del perfil MLX pinned anterior.
La identidad del SO/arquitectura del backend se centraliza en el driver Rust
platform; no se toma del navegador. Se sirve la web en navegador, sin añadir
gestor de ventanas ni requerir Docker. El futuro diseño de tools por manifiesto
está en ADR0067: descubrimiento sin ejecución, activación/permisos explícitos;
no hay todavía loader de extensiones ni control del PC.
La ruta nativa Ollama+generador macOS sí dispone de lifecycle gestionado por
procesos propios; no heredar esa garantía en motores HTTP externos. Ver
[residencia nativa](RESIDENCIA_NATIVA.md).

### Búsqueda integrada opt-in

En el catálogo propio, añadir `"web_search": true` y declarar tools en el cerebro
con entrada≥8192bytes. Es una capacidad declarada que requiere ensayo por modelo;
no se habilita para el perfil MLX pinned de3584bytes ni en la API /v1.
La revisión avisa de que las consultas salen a **html.duckduckgo.com**. El modelo
propone si necesita buscar; saludos no fuerzan red. Abrir/guardar/iniciar o crear
un run no contacta Internet. Sin Python/curl, proveedor alternativo o fallback.

Por turno:≤3 búsquedas,5 resultados por consulta,≤3 páginas,≤6 inferencias y45s
de plazo de red compartido (inferencia también consume ese plazo). Solo HTTPS
público443 con DNS/IP fijados y TLS; búsquedas sin redirects, páginas≤2 saltos
revalidados. Sin cookies/login, proxy, JS o acceso a archivos personales.
Tras leer no se aceptan nuevas búsquedas ni jobs. Solo IDs del run actual;
citas solo a extractos entregados, enlaces resueltos por host. Resultados y páginas
son datos no confiables, no permisos. El host valida referencias, no soporte
semántico, fechas o calidad de síntesis; pueden fallar proveedor o modelo.

Runs/status/cancel reutilizan el protocolo del frontend congelado, referencias
de un uso y auth/Host/Origin existentes. Cancelar HTTP no acredita detener un
modelo; se espera respuesta/deadline. Incertidumbre de red conserva
`coordination/web-reader.inflight` y bloquea reconstrucción; no borrar para
reintentar. Fuentes tienen recorte explícito, historial y schemas no se truncan.
Presupuesto por bytes estimado, no tokenizador/KV real; error context_exhausted
si no cabe. usage=null porque el normalizador no conserva procedencia de
contadores ausentes. Calidad/citas e Internet reales no probados por fixtures.

Al conservar exactamente los assets hay etiquetas heredadas de la ruta macOS
(«Ollama/MLX», «adaptador macOS» y ejemplos de Gemma/Qwen). No describen el motor
HTTP efectivo: seguir la advertencia de configuración y los bindings del backend
en `/health`. No equivale a disponer de MLX en Windows. Corregir esos textos
requiere un incremento de frontend autorizado separado; no se oculta esa limitación.

Cerebro y trabajadores son sustituibles entre registros compatibles. Pool de
hasta16 trabajadores, un turno global y selección round-robin; no votación,
especialización semántica, chat simultáneo o residencia múltiple garantizados.
Los trabajadores mantienen además el plazo de180s del broker de borradores;
un timeout de runtime mayor no amplía ese plazo. Agotarlo con un POST pendiente
conserva incertidumbre/journal, no acredita que el motor externo se haya detenido.
Si el pool no está vacío, el cerebro debe tener tools declaradas y probarse antes
de promocionarlo. La identidad pública no cambia al sustituir el binding.

## Propiedad, recursos y fallos

Los endpoints deben estar dedicados a esta instancia. Locks globales por usuario
y por puerto excluyen otros hosts Seiya cooperativos; no controlan clientes
externos, otros usuarios o runtimes que ignoren el contrato. macOS comparte además
la autoridad de lease/cuarentena del backend anterior.

Antes del POST se comprueba inventario GET /v1/models y binding exacto. Se limita
entrada serializada/salida y nunca se trunca contexto para esconder un error.
Un presupuesto/capacidad inválido se rechaza sin despachar ni declarar que hubo
una generación completada. Sin DNS, proxy, redirects, retries o fallback remoto.

Memoria: límites físicos disponibles Windows/Linux y presión nativa macOS.
Unknown o fuera de política bloquea; empeorar durante un turno cierra admisión y
espera respuesta/deadline. No mide VRAM, caché/KV o residencia real de todos los
pesos; no garantiza que un modelo quepa ni implementa unload de motores externos.
La reserva configurada no es una recomendación automática de RAM segura.

Journal propio `coordination/http-PUERTO.inflight` durable antes de POST. Una
respuesta completa aceptada permite cerrar **ese** journal. Timeout, crash/drop o
resultado ambiguo lo conservan; cambiar catálogo/puertos no evade el bloqueo.
Cancelar un socket no prueba parada del motor, así que cancelar un trabajo espera
respuesta/deadline. No se matan procesos ajenos ni se simula cancelación confirmada.

No hay recuperación automática. Ante journal pendiente, verificar motor, operaciones
activas y propiedad con intervención operativa; **no borrar marcadores para
reintentar**. El nativo macOS actual respeta esos journals y se replica cuarentena
al dominio anterior ante incertidumbre confirmada al terminar el pool. Los binarios
antiguos no entienden journals nuevos: no usarlos como rollback tras un crash sin
revisar antes el runtime externo. Si falla persistir cuarentena, se retiene el lease
durante el proceso; no se promete persistencia perfecta con errores de disco.

## OpenCode, pruebas y rollback

La fila OpenCode del README describe la API parcial del servidor separado y sus
ensayos anteriores, no un plugin ni conexión automática del catálogo al cliente.
`openai_local` no se admite en la configuración API simple para saltarse el host
propietario, recursos y journals. No se ha modificado OpenCode.

Pruebas normales: motores, mensajes y recursos sintéticos; un servidor efímero
propio comprueba el transporte HTTP, no un modelo real. CLI offline comprueba
cero conexiones y conservación del catálogo. Perfiles `desktop.json`,
`desktop-manager.json`, motores, pesos y chats anteriores se conservan. CLI M2 y
prototipo Python siguen como rollback, sujeto a revisión si hay operaciones inciertas.
Builds/instalación/GUI e inferencia reales Windows/Linux permanecen pendientes de
entorno y ensayo aislado autorizado. No confundir esos límites con tests fallidos.

Regresión offline incluyendo imágenes/chat y lifecycle nativo: `make test` (496 Rust,194 Python y5 suites
Node correctas;24 ignored), `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --locked -- -D warnings` y `git diff --check` correctos. Incluye
binding HTTP, rechazo de presupuestos sin despacho/reescritura de contexto,
cancelación con espera de respuesta, journals persistentes y bloqueo al cambiar
de puertos, preservación de catálogos/perfiles y hashes de los seis assets.
