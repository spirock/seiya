# Aplicación Rust · avance y operación

Estado y próximos hitos: [hoja de ruta pública](../ROADMAP.md).
Objetivo: launcher/coordinador Rust, mismo diseño web y motores/adaptadores
adecuados a cada plataforma. No reescritura obligatoria de HTML/JS o inferencia.

## Launcher macOS (M3 · pruebas offline)

La nueva Seiya Rust.app invoca `seiya-desktop --launch`: menú nativo **Configurar**
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
