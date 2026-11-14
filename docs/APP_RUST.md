# Aplicación Rust · avance y operación

Incremento local de residencia nativa: [lifecycle macOS](RESIDENCIA_NATIVA.md).
Ollama y generador de imagen propios comparten driver secuencial; cierre de árbol
e intención durable antes de spawn, sin certificar VRAM liberada. chat_images
opt-in permite usar el panel de imágenes desde el chat nativo con límite3584bytes
intacto. Perfiles viejos siguen manuales hasta revisión. Verificado con fixtures,
no modelos reales nuevos ni instalación acreditada; comprobar el SHA del paquete.

Estado y próximos hitos: [hoja de ruta pública](../ROADMAP.md).
Objetivo: launcher/coordinador Rust, mismo diseño web y motores/adaptadores
adecuados a cada plataforma. No reescritura obligatoria de HTML/JS o inferencia.

## Incremento de texto portable · fuentes actuales

Catálogo sustituible de cerebro/trabajadores y backend HTTP local externo en Rust,
con selección secuencial, binding verificado, locks/journal y recursos. Disponible
por `--launch [--catalog RUTA_ABSOLUTA]` y `--check-catalog FILE`; comprobar que el
build descargado corresponda al snapshot actual. [Operación y límites](BACKENDS_LOCALES.md).
Windows/Linux dejan de limitarse al borrador de roles **si se prepara el catálogo y
motor externo**. Sin catálogo, el asistente explica lo que falta y no genera.
macOS sin catálogo conserva MLX pinned y chat de imagen opt-in. Imagen por API
HTTP tiene otro catálogo experimental con PNG nativo macOS; no validación de GPU
ni imágenes nativas Windows/Linux. [Imágenes desde chat](IMAGENES_DESDE_CHAT.md).

**HTML/CSS/JavaScript y diseño se conservan permanentemente**: este incremento no
modifica los seis assets congelados. Por eso permanecen algunas etiquetas heredadas
macOS/Ollama; la advertencia HTTP y el binding efectivo describen el backend nuevo.
No hay inferencia/instalación/GPU real de Windows/Linux acreditadas por sus fixtures.

Las secciones siguientes registran el asistente y las limitaciones del incremento
anterior; donde indican «backend ausente» o «solo IDs en navegador», el catálogo de
texto nuevo amplía ese alcance, no el backend de imágenes ni los perfiles MLX.

## Asistente web primero (fuentes actuales · experimental)

`seiya-desktop --launch` abre la web común en `127.0.0.1:8015` **sin necesitar
desktop.json, motores ni modelos**. No ejecuta inferencia al abrir. Se conserva el
HTML/CSS del chat; hasta configurar, el compositor está desactivado y aparece el
asistente. No hay menú inicial para elegir un JSON o un ejecutable.

1. Detectar instalaciones/perfil: observa rutas conocidas de Ollama y el almacén
   MLX, sin ejecutar programas ni buscar documentos personales.
2. Editar rutas o usar los selectores de macOS desde botones de la web.
3. Asignar cerebro, pipeline de imágenes y grupo de trabajadores. Los IDs son
   editables; no conceden compatibilidad, descarga, ejecución de código o red.
4. Revisar: prepara carpetas privadas propias y valida filesystem. Muestra los
   valores/efectos exactos; aún no guarda el perfil ni inicia motores.
5. Confirmar **guardar sin iniciar** o **guardar e iniciar**. Una aprobación es de
   un uso y contiene bytes inmutables; no se reenvía tras un error de conexión.
6. Tras ready, Abrir chat usa bootstrap de un uso. En el chat, **Configurar modelos**
   vuelve al asistente. Detener motores espera cleanup; después pueden cambiarse
   asignaciones y volver a revisar/iniciar.

**Persistencia:** `~/Library/Application Support/Seiya/desktop-manager.json`
contiene versión1, perfil completo y configuración/asignaciones, con modo600 y guardado atómico
comparado contra baseline bajo lock. El `desktop.json` anterior no se modifica:
se ofrece como importación al no existir el nuevo documento y sigue siendo
rollback para `--serve`. Se conservan también data/coordination personalizados
del perfil migrado; el documento nuevo conserva esas rutas aunque se retire el
perfil anterior. La baseline enviada a la web es un hash, no los bytes del perfil.
Data/coordination e historial no cambian. No se corrigen
permisos de carpetas existentes automáticamente; el error aparece en la web.

**Gestor frente a ejecución:** permite sustituir y guardar IDs de cerebro, imágenes
y hasta16 trabajadores únicos. El adaptador macOS solo ejecuta hoy Gemma
`gemma4:e2b-mlx`, cero/un auxiliar `qwen3.5:9b-mlx` y el pipeline opcional
`qwen-image-2.1-turbo-q4`. Otros IDs/pools se conservan pero su activación está
bloqueada; no hay fallback ni carga simultánea implícita. Elegir un ID no descarga
pesos ni valida tokenizador/tools/runtime. Imágenes requieren sd-cli y los tres
assets exactos del ensayo, no solo el denoiser. Guardas/pins/inventario11434 y
lease/cuarentena anteriores permanecen; iniciar puede fallar por ellas.

**Windows/Linux:** comparten el shell/asistente web con `--launch`, no el backend
de inferencia macOS. La web indica que el adaptador falta y bloquea revisión del
motor/inicio; no hay selector nativo equivalente ni guardado de perfil runtime en
esos sistemas. **Guardar asignaciones en este navegador** conserva únicamente
los IDs en `seiya.desktop.roles.v1` de localStorage, sin rutas y sin activarlos.
Ese borrador es local al navegador/origen, no un modelo instalado ni sincronización.
Windows tiene acceso directo web y `Seiya-Rust.cmd`; Ayuda sigue disponible.

Cerrar una pestaña no detiene el proceso: usar **Cerrar Seiya** o la señal de cierre
del proceso propio. La sesión caduca a30min; al cerrar/caducar se revoca admisión y
se espera cleanup, sin matar procesos ajenos ni limpiar cuarentenas. El servidor
shell posee8015; el router del host se monta en él con su propia autenticación,
sin cambiar origen ni adoptar otro servidor. No exposición LAN o remoto.

HTTP/DOM/adaptadores y persistencia se prueban con fixtures: no equivalen a abrir
Finder, instalar la app, probar Windows real o cargar modelos desde este flujo.
Los enlaces Actions del README siguen apuntando al build anterior hasta publicar
y completar un nuevo run; no atribuir el asistente nuevo al DMG anterior.
[Decisión y rollback](decisions/0063-asistente-web-desktop.md).

Comprobación offline del incremento: `make test` (438 Rust,192 Python y4 suites
Node correctas;24 ignored), `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --locked -- -D warnings` y `git diff --check` correctos. Incluye
esquema del gestor, conflictos de baseline, permisos/symlinks, rutas de historial
personalizadas, cookies de shell/runtime independientes, shutdown durante inicio
y confirmación pendiente, variantes no admitidas y ausencia de reenvío POST.
El target Windows no está instalado en el entorno de validación: no se acredita
cross-build/GUI Windows del cambio; pruebas de su payload usan PE sintéticos.

## Launcher de diálogos anterior (build publicado `1d49370`)

El build anterior de Seiya Rust.app invoca `seiya-desktop --launch`: menú nativo **Configurar**
o **Abrir perfil**. Configurar selecciona ejecutable LLM compatible y almacén
existentes, opcionalmente sd-cli/assets ya preparados. No descarga pesos ni elige
otro proveedor. Elegir carpetas ocultas o dentro de .app puede requerir Ir a carpeta
en el selector del SO; no pide ejecutar comandos.

Guardar pide confirmación y solo prepara carpetas privadas propias bajo
`~/Library/Application Support/Seiya/{data,coordination}` y `desktop.json` con
modo600. No cambia permisos externos; un perfil modificado durante el diálogo
produce conflicto, sin sobrescribirlo. Abrir perfil solo lee el JSON elegido;
se revisan rutas y se usa una copia inmutable de lo aprobado, no otro archivo
releído después de confirmar. JSON/paths son datos, no permisos por sí solos.

Tras la revisión, **Solo chat** o **Con opciones** autoriza iniciar el host local.
Mismos modelos/tags/pins y guardas M2; el backend11434 debe responder vacío.
HTTP exacto127.0.0.1:8015, bootstrap de un uso y cookie con guardas Host/Origin;
solo loopback, no LAN, wildcard o certificados creados automáticamente. El chat
existente se abre en navegador tras ready. Abrir app/guardar/iniciar no envía
prompt, genera imágenes o carga modelos. Las opciones CAS previas prevalecen.

El diálogo **Detener** revoca admisión y espera drenaje/cleanup; **Abrir chat** usa
la ruta autenticada del mismo navegador y no reenvía el bootstrap. Si falla abrir,
se detiene el host, sin retries automáticos. Cerrar/cancelar o expirar también
detiene; un cierre incierto no se presenta como éxito. No cerrar el proceso a la
fuerza para saltarse esa espera ni borrar cuarentenas para arrancar de nuevo.

Pruebas con UI/host/browser falsos, filesystem sintético y HTTP in-process. No se
han abierto diálogos, navegador o modelos reales por el agente. GUI real/firmas/
instalación y sesión sostenida pendientes de ensayo explícito. Windows/Linux no
obtienen launcher nativo. [Paridad e historial](PARIDAD_RUST.md): el origen local
no migra silenciosamente chats de la antigua LAN HTTPS. No retirar Python.

## Disponible en el primer incremento

`seiya-desktop --check-config` es **solo un comprobador offline**. No arranca
navegador/servidor, no ejecuta Ollama/sd-cli, no carga pesos,
no conecta con servicios, no crea directorios ni migra historial.

```sh
cargo build --locked -p seiya-server --bin seiya-desktop
./target/debug/seiya-desktop --check-config /ruta/a/desktop.json
```

Preparar manualmente un JSON tomando
[`contracts/desktop/v1/config.example.json`](../contracts/desktop/v1/config.example.json).
Sustituir placeholders por rutas existentes **absolutas y canónicas**, separando
datos, coordinación y dependencias. Si no se quiere imagen, omitir `images` y
dejar features.images=false. No descargar modelos o instalar runtimes solo para
pasar el chequeo. No modificar permisos globales de archivos ajenos por un error.

El informe devuelve solicitado, no capacidad efectiva. `filesystem_checked=true`
no acredita runtime compatible, pesos correctos, RAM suficiente, licencia, carga
real, TLS o seguridad de procesos. En Linux puede comprobar fixtures Unix de un
target macOS, pero target_matches_host=false; no se habilita MLX/Metal por ello.
Las comprobaciones de permisos son bits Unix; ACL/propietario/TOCTOU no validados.
El arranque futuro tendrá que revalidar el entorno y controlar procesos propios.

## Lo que todavía no sustituye

- Los hosts antiguos mantienen sus CLI/rutas actuales. Entradas `--serve` y
  `--launch` usan configuración portable, con permisos/alcances distintos.
- Paquetes Rust paralelos no sustituyen Python. macOS nuevo arranca el launcher;
  Windows/Linux siguen CLI/API sin la web nativa macOS.
- No paridad completa, sesión larga, instalación en otro equipo o nueva inferencia
  acreditadas por este incremento. No se ha eliminado Python de motores externos.

Rollback: no usar el comprobador. El chequeo no escribe, por lo que no hay datos
que deshacer; Python, configuración e historial anteriores quedan intactos.
Los instaladores se adelantaron como experimentales, no acreditan paridad M3.
No retirar prototipo operativo por compilar este ejecutable.

## M2 · host macOS portable, arranque manual

Implementado con pruebas offline/helpers; **no validado aún con modelos reales
desde esta nueva entrada**. No es launcher gráfico ni app autónoma empaquetada.
El operador prepara manualmente directorios/dependencias y TLS, sin instalarlos
o descargarlos por sorpresa. El agente no ejecuta este ejemplo automáticamente:

```sh
./target/debug/seiya-desktop --serve --config /ruta/desktop.json \
  --approval manual-portable-macos-web-no-automatic-inference \
  --listen 192.168.1.20:8015 --prefix-length 24 \
  --certificate-der /ruta/cert.der --private-key-der /ruta/key.der \
  --chat-only
```

`--chat-only` va al final y adjunta solo Gemma. Omitirlo permite Qwen/imágenes
si están instalados y habilitados. No genera prompt de prueba ni warmup al iniciar;
la generación solo se solicita desde el navegador autenticado. Conserva UI,
Host/Origin exactos, bootstrap de un uso, TLS1.3, límites y expiración temporal
existentes (sesión máxima30min). No abre navegador automáticamente. El proceso
imprime la ruta del archivo privado de acceso; no compartir logs/configuración.

### Requisitos y límites conservados

- macOS con sandbox-exec y runtime Ollama compatible; no cualquier motor por
  tener un ejecutable. Gemma tag/digest siguen fijados por el ensayo. Qwen es
  opcional si falta su manifiesto y features.multiagent=false; si se solicita y
  falta, arranque falla. Manifiestos presentes pero inválidos no se ignoran.
- `model_store` es el directorio que contiene blobs y manifests-v2, no una carpeta
  genérica con GGUF. Datos se guardan en data_dir; cada ejecución crea run-UUID.
- `coordination_dir` **debe** ser la ruta canónica privada
  `$HOME/Library/Application Support/Seiya/coordination`, ya existente. HOME es
  entorno de confianza del operador; no hay selector libre de autoridad.
- Imagen es opcional (omitir objeto images y features.images=false). Si se adjunta,
  conserva selección de assets/hashes y hash sd-cli fijados, más build-evidence.json
  en la estructura de build esperada por el adaptador; no acepta cualquier sd-cli.
  Si está apagada, el preflight de pesos se difiere hasta que se solicite generar.
- El guard de inventario conserva dependencia de `127.0.0.1:11434/api/ps`: debe
  responder vacío; fallo/desconocido/residente rechaza trabajo. No inicia, adopta,
  descarga modelos de ese servicio ni lo detiene. Quitar esa dependencia requiere
  un contrato/probe equivalente posterior, no tratar conexión fallida como libre.
- JSON features solo inicializa un perfil sin elección previa. Settings CAS
  guardado prevalece en siguientes arranques. Solicitado no significa instalado
  o efectivo. Chat-only no adjunta adaptadores aunque haya switches guardados on.

### Coordinación, cierre y rollback

Lock por perfil durante la vida del host: otro host portable con el mismo
data_dir se rechaza. Cada turno adquiere el lock estable y, si existe, el lock
legado. Las cuarentenas se comprueban dentro de los locks; cleanup incierto marca
ambos dominios. Si no se puede persistir, se retienen locks durante el proceso.
No mover/borrar locks, cambiar HOME, reprovisionar el legado ni arrancar otro
coordinador fuera de este contrato. No se garantiza coordinación con programas
externos no cooperativos u otra instalación legacy en otra ruta.

Parada Ctrl+C/SIGTERM: revoca admisión, cancela/drena trabajos y espera cierre
del árbol propio con plazo. No mata procesos ajenos ni elimina evidencias. El
resultado final registra drenaje/fallo; timeout o cleanup incierto no es éxito.
La duración/gestión de cierre heredan límites del host experimental, no garantías
de servicio permanente. Recuperar una cuarentena requiere revisión de procesos y
evidencias; M2 no añade comando para limpiarla. No borrar marcadores para arrancar.

Rollback: volver explícitamente al launcher anterior sin borrar historial/modelos.
La cuarentena replicada en legacy también lo bloquea; revisar solo el marcador
legacy no limpia el estable. Cambiar de perfil no recupera un runtime incierto.
Los instaladores y artefactos publicados anteriormente no cambian con este código.

Evidencia M2:411 tests Rust+172 Python,3 suites Node, fmt/Clippy/release OK;
24 ignored sin ejecutar. Locks y cuarentena comprobados en directorios sintéticos,
cierre con helper sleep propio (no motor/modelos). Nueva entrada real con TLS,
conversación e imagen todavía no ensayada. Pruebas reales anteriores del host
legacy no acreditan automáticamente esta ruta portable.

## Instaladores experimentales priorizados después de M2

Staging Rust paralelo: Seiya Rust.app y comandos seiya-rust/seiya-rust-server,
sin servidor Python en el payload. El build inicial mostraba ayuda en Terminal;
M3 añade diálogos nativos y confirmación de host local, sin iniciar por sorpresa.
Debian solo CLI/API Rust,
no host nativo de imágenes. Detalles, requisitos y rollback en PAQUETES_PRUEBA.md.
M3/paridad no se considera terminada por generar estos instaladores.
